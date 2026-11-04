//! Bounded benchmark instruction tuning with a separate holdout gate.
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::api::ApiClient;
use crate::bench::{self, Task};
use crate::config::{nur_home, private_atomic_write, Config};
use crate::error::{NurError, Result};
use crate::gepa::{self, Candidate, Score};

const MAX_INSTRUCTION_BYTES: usize = 4096;

fn read_git_pointer(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(8193)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 8192 {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// Use Git's common directory so linked worktrees share a tuned project.
/// No Git subprocess is launched on the model-request path.
fn project_root(cwd: &Path) -> Option<PathBuf> {
    let cwd = cwd.canonicalize().ok()?;
    let Some(root) = cwd.ancestors().find(|dir| dir.join(".git").exists()) else {
        return Some(cwd);
    };
    let dot_git = root.join(".git");
    let gitdir = if dot_git.is_dir() {
        dot_git.canonicalize().ok()?
    } else {
        let text = read_git_pointer(&dot_git)?;
        let path = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());
        let gitdir = (if path.is_absolute() {
            path
        } else {
            root.join(path)
        })
        .canonicalize()
        .ok()?;
        // A worktree's admin directory points back to its .git file. Without
        // that check a forged pointer could import another project's tuning.
        let valid = read_git_pointer(&gitdir.join("gitdir"))
            .and_then(|text| {
                let path = PathBuf::from(text.trim());
                (if path.is_absolute() {
                    path
                } else {
                    gitdir.join(path)
                })
                .canonicalize()
                .ok()
            })
            .zip(dot_git.canonicalize().ok())
            .is_some_and(|(back, current)| back == current);
        if !valid {
            return Some(root.to_path_buf());
        }
        gitdir
    };
    let common_file = gitdir.join("commondir");
    let common = if common_file.exists() {
        let text = read_git_pointer(&common_file)?;
        let path = PathBuf::from(text.trim());
        (if path.is_absolute() {
            path
        } else {
            gitdir.join(path)
        })
        .canonicalize()
        .ok()?
    } else {
        gitdir
    };
    Some(common)
}

fn active_path(cwd: &Path, provider: &str, model: &str) -> Option<PathBuf> {
    let root = project_root(cwd)?;
    #[cfg(unix)]
    let mut bytes = {
        use std::os::unix::ffi::OsStrExt;
        root.as_os_str().as_bytes().to_vec()
    };
    #[cfg(windows)]
    let mut bytes: Vec<u8> = {
        use std::os::windows::ffi::OsStrExt;
        root.as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect()
    };
    #[cfg(not(any(unix, windows)))]
    let mut bytes = root.to_string_lossy().as_bytes().to_vec();
    for component in [provider, model] {
        bytes.extend_from_slice(&(component.len() as u64).to_le_bytes());
        bytes.extend_from_slice(component.as_bytes());
    }
    Some(
        nur_home()
            .join("bench/tuned")
            .join(format!("{}.txt", crate::agent::receipt::sha256_hex(&bytes))),
    )
}

pub(crate) fn active_instruction(cwd: &Path, provider: &str, model: &str) -> Option<String> {
    use std::io::Read;
    let path = active_path(cwd, provider, model)?;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take((MAX_INSTRUCTION_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > MAX_INSTRUCTION_BYTES {
        return None;
    }
    let instruction = String::from_utf8(bytes).ok()?;
    (!instruction.trim().is_empty()).then_some(instruction)
}

pub(crate) fn apply_instruction(instruction: &str, user_text: &str) -> String {
    format!("[Tuned project instruction]\n{instruction}\n\n[User request]\n{user_text}")
}

fn tasks(names: &[String]) -> Result<Vec<Task>> {
    names
        .iter()
        .map(|name| {
            let task = bench::load_task(name)
                .ok_or_else(|| NurError::Other(format!("no bench task `{name}`")))?;
            if task.check.is_none() {
                return Err(NurError::Other(format!(
                    "bench task `{name}` needs a check for self-tuning"
                )));
            }
            Ok(task)
        })
        .collect()
}

async fn evaluate(
    tasks: &[Task],
    instruction: &str,
    client: &ApiClient,
    cfg: &Config,
    cwd: &Path,
) -> Result<(Score, Vec<String>)> {
    let mut results = Vec::new();
    let mut failures = Vec::new();
    for task in tasks {
        let mut evaluated_task = task.clone();
        if !instruction.trim().is_empty() {
            evaluated_task.prompt = apply_instruction(instruction, &task.prompt);
        }
        let result = Box::pin(bench::run_one(
            &evaluated_task,
            &cfg.model,
            client,
            cfg,
            cwd,
            "",
            true,
        ))
        .await?;
        if !result.passed {
            failures.push(format!(
                "task `{}` failed{}",
                task.name,
                result
                    .error
                    .as_deref()
                    .map(|e| format!(": {e}"))
                    .unwrap_or_default()
            ));
        }
        results.push(result);
    }
    Ok((Score::from_results(&results), failures))
}

struct Audit<'a> {
    run_id: uuid::Uuid,
    train: &'a [String],
    held_out: &'a [String],
    provider: &'a str,
    model: &'a str,
}

impl Audit<'_> {
    fn record(
        &self,
        round: u32,
        decision: &str,
        baseline: (f64, f64),
        train: f64,
        holdout: Option<f64>,
        proposal: &str,
    ) -> Result<()> {
        let dir = nur_home().join("bench");
        std::fs::create_dir_all(&dir)?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("self-tune.jsonl"))?;
        let entry = serde_json::json!({
            "run_id": self.run_id.to_string(), "round": round, "decision": decision,
            "train_tasks": self.train, "held_out_tasks": self.held_out,
            "provider": self.provider, "model": self.model,
            "baseline_train_pass_rate": baseline.0, "baseline_held_out_pass_rate": baseline.1,
            "train_pass_rate": train, "held_out_pass_rate": holdout,
            "proposal": crate::secrets::redact(proposal)
        });
        writeln!(file, "{entry}")?;
        file.flush()?;
        Ok(())
    }
}

fn acquire_lease(path: &Path) -> Result<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    file.try_lock().map_err(|_| {
        NurError::Other("self-tune is already running for this project and model".into())
    })?;
    Ok(file)
}

pub async fn run(
    train_names: &[String],
    held_out_names: &[String],
    rounds: u32,
    client: ApiClient,
    cfg: Config,
    cwd: PathBuf,
) -> Result<()> {
    if train_names.is_empty() || held_out_names.is_empty() || !(1..=10).contains(&rounds) {
        return Err(NurError::Other(
            "self-tune needs nonempty train and held-out sets and 1-10 rounds".into(),
        ));
    }
    let mut seen = std::collections::HashSet::new();
    if train_names
        .iter()
        .chain(held_out_names)
        .any(|name| !seen.insert(name))
    {
        return Err(NurError::Other(
            "self-tune task sets must be disjoint and contain no duplicates".into(),
        ));
    }
    if !bench::is_git_repo_pub(&cwd) {
        return Err(NurError::Other(
            "self-tune needs a git repository for isolated worktrees".into(),
        ));
    }
    let train = tasks(train_names)?;
    let held_out = tasks(held_out_names)?;
    // Distinct spellings can resolve to the same saved task on a
    // case-insensitive filesystem, or a task file can declare another name.
    let mut identities = std::collections::HashSet::new();
    if train.iter().chain(&held_out).any(|task| {
        let identity = if cfg!(windows) {
            task.name.to_lowercase()
        } else {
            task.name.clone()
        };
        !identities.insert(identity)
    }) {
        return Err(NurError::Other(
            "self-tune task sets must be disjoint and contain no duplicates".into(),
        ));
    }
    let active = active_path(&cwd, &cfg.provider, &cfg.model)
        .ok_or_else(|| NurError::Other("self-tune cannot resolve the project directory".into()))?;
    if let Some(parent) = active.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _lease = acquire_lease(&active.with_extension("lock"))?;
    let mut instruction = active_instruction(&cwd, &cfg.provider, &cfg.model).unwrap_or_default();
    let (mut train_score, mut failures) =
        Box::pin(evaluate(&train, &instruction, &client, &cfg, &cwd)).await?;
    let (mut held_out_score, _) =
        Box::pin(evaluate(&held_out, &instruction, &client, &cfg, &cwd)).await?;
    let task_texts: Vec<_> = train.iter().map(|task| task.prompt.clone()).collect();
    let audit = Audit {
        run_id: uuid::Uuid::new_v4(),
        train: train_names,
        held_out: held_out_names,
        provider: &cfg.provider,
        model: &cfg.model,
    };
    for round in 1..=rounds {
        let candidate = Candidate {
            id: round as usize,
            instruction: instruction.clone(),
            parent: None,
            generation: round,
        };
        let prompt = gepa::mutation_prompt(&candidate, &train_score, &failures, &task_texts);
        let request = crate::api::fusion::question_request(&cfg.model, &prompt);
        let response = client.create_response(&request).await?;
        let proposal =
            crate::secrets::redact(&gepa::clean_instruction(&response.output_text())).into_owned();
        if proposal.is_empty() || proposal.len() > MAX_INSTRUCTION_BYTES || proposal == instruction
        {
            audit.record(
                round,
                "rejected_invalid",
                (train_score.pass_rate, held_out_score.pass_rate),
                train_score.pass_rate,
                None,
                &proposal,
            )?;
            continue;
        }
        let (new_train, new_failures) =
            Box::pin(evaluate(&train, &proposal, &client, &cfg, &cwd)).await?;
        if new_train.pass_rate <= train_score.pass_rate {
            audit.record(
                round,
                "rejected_train",
                (train_score.pass_rate, held_out_score.pass_rate),
                new_train.pass_rate,
                None,
                &proposal,
            )?;
            continue;
        }
        let (new_held_out, _) =
            Box::pin(evaluate(&held_out, &proposal, &client, &cfg, &cwd)).await?;
        if new_held_out.pass_rate < held_out_score.pass_rate {
            audit.record(
                round,
                "rejected_holdout",
                (train_score.pass_rate, held_out_score.pass_rate),
                new_train.pass_rate,
                Some(new_held_out.pass_rate),
                &proposal,
            )?;
            continue;
        }
        audit.record(
            round,
            "accepting",
            (train_score.pass_rate, held_out_score.pass_rate),
            new_train.pass_rate,
            Some(new_held_out.pass_rate),
            &proposal,
        )?;
        if let Err(error) = private_atomic_write(&active, proposal.as_bytes()) {
            audit.record(
                round,
                "activation_failed",
                (train_score.pass_rate, held_out_score.pass_rate),
                new_train.pass_rate,
                Some(new_held_out.pass_rate),
                &proposal,
            )?;
            return Err(error.into());
        }
        audit.record(
            round,
            "accepted",
            (train_score.pass_rate, held_out_score.pass_rate),
            new_train.pass_rate,
            Some(new_held_out.pass_rate),
            &proposal,
        )?;
        instruction = proposal;
        train_score = new_train;
        held_out_score = new_held_out;
        failures = new_failures;
    }
    println!(
        "self-tune decisions saved to {}; project instruction: {}",
        nur_home().join("bench/self-tune.jsonl").display(),
        active.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::acquire_lease;

    #[test]
    fn concurrent_tuning_uses_an_exclusive_lease() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tune.lock");
        let first = acquire_lease(&path).unwrap();
        assert!(acquire_lease(&path).is_err());
        drop(first);
        assert!(acquire_lease(&path).is_ok());
    }
}

//! Task benchmarking (ported from wizard's `bench`).
//!
//! Record your real tasks as trajectories, then replay each one across several
//! models in **isolated git worktrees** and score them (pass/fail via a check
//! command, wall time, tokens). "The new model is better" becomes a number.
//!
//! Storage: `~/.nur/bench/<name>.json`. Runs happen in throwaway worktrees under
//! `~/.nur/bench/worktrees/` and are torn down after.

use crate::agent::{self, AgentRunner, Session};
use crate::api::ApiClient;
use crate::cli::BenchCmd;
use crate::config::{muse_home, Config};
use crate::error::{MuseError, Result};
use crate::theme;
use crate::usage::UsageTracker;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// A recorded benchmark task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub name: String,
    pub prompt: String,
    /// Shell command deciding pass/fail (exit 0 = pass). None → pass iff the
    /// agent turn itself succeeded.
    pub check: Option<String>,
}

/// One model's result on a task.
#[derive(Debug, Clone)]
pub struct BenchResult {
    pub model: String,
    pub passed: bool,
    pub secs: f64,
    pub tokens: u64,
    pub error: Option<String>,
}

// ── pure helpers (unit-tested) ───────────────────────────────────────────

/// A filesystem/git-safe branch + worktree name for a run.
pub fn worktree_branch(task: &str, model: &str, ts: u128) -> String {
    let clean = |s: &str| -> String {
        s.chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>()
    };
    format!("nur-bench-{}-{}-{}", clean(task), clean(model), ts)
}

/// Parse a `--models a,b,c` list; empty entries dropped.
pub fn parse_models(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Render a scoreboard for a task's results (winner = passing, then fewest
/// seconds, then fewest tokens).
pub fn scoreboard(task: &str, results: &[BenchResult]) -> String {
    let mut out = format!("bench · {task}\n");
    out.push_str(&format!(
        "  {:<28} {:<6} {:>9} {:>10}\n",
        "model", "result", "seconds", "tokens"
    ));
    let mut ranked: Vec<&BenchResult> = results.iter().collect();
    ranked.sort_by(|a, b| {
        b.passed
            .cmp(&a.passed)
            .then(
                a.secs
                    .partial_cmp(&b.secs)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(a.tokens.cmp(&b.tokens))
    });
    for (i, r) in ranked.iter().enumerate() {
        let mark = if r.passed { "PASS" } else { "FAIL" };
        let win = if i == 0 && r.passed { " ◀ best" } else { "" };
        let err = r
            .error
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(|s| format!("  · {s}"))
            .unwrap_or_default();
        out.push_str(&format!(
            "  {:<28} {:<6} {:>9.1} {:>10}{}{}\n",
            r.model, mark, r.secs, r.tokens, win, err
        ));
    }
    out
}

pub fn bench_dir() -> PathBuf {
    muse_home().join("bench")
}
fn worktrees_dir() -> PathBuf {
    bench_dir().join("worktrees")
}
fn task_path(name: &str) -> PathBuf {
    bench_dir().join(format!("{name}.json"))
}

pub(crate) fn list_tasks_pub() -> Vec<Task> {
    list_tasks()
}

pub(crate) fn is_git_repo_pub(dir: &Path) -> bool {
    is_git_repo(dir)
}

pub fn load_task(name: &str) -> Option<Task> {
    let s = std::fs::read_to_string(task_path(name)).ok()?;
    serde_json::from_str(&s).ok()
}

fn save_task(t: &Task) -> Result<()> {
    std::fs::create_dir_all(bench_dir())?;
    std::fs::write(
        task_path(&t.name),
        serde_json::to_string_pretty(t).unwrap_or_default(),
    )?;
    Ok(())
}

fn list_tasks() -> Vec<Task> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(bench_dir()) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) == Some("json") {
                if let Ok(s) = std::fs::read_to_string(&p) {
                    if let Ok(t) = serde_json::from_str::<Task>(&s) {
                        out.push(t);
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

// ── driver ───────────────────────────────────────────────────────────────

pub async fn run_bench(
    action: &BenchCmd,
    client: ApiClient,
    cfg: Config,
    cwd: PathBuf,
) -> Result<()> {
    match action {
        BenchCmd::Add {
            name,
            prompt,
            check,
        } => {
            print!("{}", add_task(name, &prompt.join(" "), check.as_deref())?);
            Ok(())
        }
        BenchCmd::List => {
            print!("{}", list_report());
            Ok(())
        }
        BenchCmd::Remove { name } => {
            println!("{}", remove_report(name));
            Ok(())
        }
        BenchCmd::Run { name, models } => {
            run_tasks(name, models.as_deref(), client, cfg, cwd).await
        }
        BenchCmd::Optimize { name, gens, pop } => {
            crate::gepa::run_optimize(name, *gens, *pop, client, cfg, cwd).await
        }
    }
}

/// Record a task; returns a confirmation. Shared by the CLI and `/bench add`.
pub fn add_task(name: &str, prompt: &str, check: Option<&str>) -> Result<String> {
    let t = Task {
        name: name.to_string(),
        prompt: prompt.to_string(),
        check: check.map(str::to_string),
    };
    save_task(&t)?;
    let mut msg = format!("recorded bench task `{}`", t.name);
    if t.check.is_none() {
        msg.push_str(
            "\nno check set — a run passes if the agent turn succeeds. Add a gate via CLI: nur bench add <name> \"...\" --check \"cargo test\"",
        );
    }
    Ok(msg)
}

/// List recorded tasks. Shared by the CLI and `/bench list`.
pub fn list_report() -> String {
    let tasks = list_tasks();
    if tasks.is_empty() {
        return "no bench tasks — add one: /bench add fix \"make the tests pass\"".to_string();
    }
    let mut s = String::from("bench tasks:\n");
    for t in tasks {
        let chk = t.check.as_deref().unwrap_or("(agent success)");
        s.push_str(&format!("  {:<16} check: {chk}\n", t.name));
    }
    s
}

/// Remove a task; returns a status message. Shared by the CLI and `/bench remove`.
pub fn remove_report(name: &str) -> String {
    if std::fs::remove_file(task_path(name)).is_ok() {
        format!("removed `{name}`")
    } else {
        format!("no task `{name}`")
    }
}

async fn run_tasks(
    name: &str,
    models: Option<&str>,
    client: ApiClient,
    cfg: Config,
    cwd: PathBuf,
) -> Result<()> {
    if !is_git_repo(&cwd) {
        return Err(MuseError::Other(
            "bench run needs a git repo (it replays in isolated worktrees) — run inside your project".into(),
        ));
    }
    let tasks: Vec<Task> = if name == "all" {
        list_tasks()
    } else {
        vec![load_task(name).ok_or_else(|| {
            MuseError::Other(format!("no bench task `{name}` — see nur bench list"))
        })?]
    };
    if tasks.is_empty() {
        return Err(MuseError::Other("no bench tasks recorded".into()));
    }
    let model_list = match models {
        Some(m) => parse_models(m),
        None => vec![cfg.model.clone()],
    };
    if model_list.is_empty() {
        return Err(MuseError::Other(
            "no models to compare (--models a,b)".into(),
        ));
    }

    theme::print_info(&format!(
        "bench · {} task(s) × {} model(s)",
        tasks.len(),
        model_list.len()
    ));

    for task in &tasks {
        let mut results = Vec::new();
        for model in &model_list {
            theme::print_info(&format!("→ {} · {}", task.name, model));
            let result = run_one(task, model, &client, &cfg, &cwd, "").await;
            match result {
                Ok(r) => results.push(r),
                Err(e) => {
                    theme::print_info(&format!("  setup error: {e}"));
                    results.push(BenchResult {
                        model: model.clone(),
                        passed: false,
                        secs: 0.0,
                        tokens: 0,
                        error: Some(e.to_string()),
                    });
                }
            }
        }
        print!("{}", scoreboard(&task.name, &results));
    }
    Ok(())
}

/// Replay `task` once with `model` in a throwaway worktree.
///
/// `prefix` is prepended to the task prompt — empty for a plain benchmark, and
/// the candidate instruction when the GEPA optimiser is driving (see
/// [`crate::gepa`]). Keeping it a parameter rather than a second copy of this
/// function is what lets the optimiser reuse the whole isolated-worktree,
/// check-command, token-accounting apparatus unchanged.
pub(crate) async fn run_one(
    task: &Task,
    model: &str,
    client: &ApiClient,
    cfg: &Config,
    repo: &Path,
    prefix: &str,
) -> Result<BenchResult> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let branch = worktree_branch(&task.name, model, ts);
    std::fs::create_dir_all(worktrees_dir())?;
    let wt = worktrees_dir().join(&branch);
    let wt_str = wt.display().to_string();

    git(repo, &["worktree", "add", "-b", &branch, &wt_str])
        .map_err(|e| MuseError::Other(format!("git worktree add failed: {e}")))?;

    // Run the agent in the worktree with this model (auto-approve, sandboxed).
    let mut cfg_m = cfg.clone();
    cfg_m.model = model.to_string();
    let runner = Arc::new(AgentRunner {
        client: client.clone(),
        config: cfg_m,
        cwd: wt.clone(),
        permission_mode: agent::SharedMode::new(agent::PermissionMode::Auto),
        verbose: false,
        approved_tools: Arc::new(Mutex::new(std::collections::HashSet::new())),
        tools: crate::tools::ToolHost::default(),
        permissions: agent::SharedPermissions::load(&wt),
        hooks: agent::hooks::HooksConfig::load(),
        is_subagent: false,
    });
    let session = Session::new(model, &wt_str);
    let mut usage = UsageTracker::new(session.id.clone(), model.to_string(), wt.clone());
    usage.set_provider(cfg.provider.clone());
    let cancel = tokio_util::sync::CancellationToken::new();

    let start = Instant::now();
    let prompt = if prefix.trim().is_empty() {
        task.prompt.clone()
    } else {
        format!(
            "{}

{}",
            prefix.trim(),
            task.prompt
        )
    };
    let (_s, u, result, _interrupted) =
        agent::run_collect(runner, session, usage, prompt, cancel).await;
    let secs = start.elapsed().as_secs_f64();
    let tokens = u.session_usage().total_tokens;

    let (passed, error) = match &task.check {
        Some(cmd) => (shell_ok_in(&wt, cmd), result.err()),
        None => (result.is_ok(), result.err()),
    };

    // Tear down the worktree + branch.
    let _ = git(repo, &["worktree", "remove", "--force", &wt_str]);
    let _ = git(repo, &["branch", "-D", &branch]);

    Ok(BenchResult {
        model: model.to_string(),
        passed,
        secs,
        tokens,
        error,
    })
}

fn is_git_repo(dir: &Path) -> bool {
    git(dir, &["rev-parse", "--is-inside-work-tree"]).is_ok()
}

fn git(cwd: &Path, args: &[&str]) -> Result<()> {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| MuseError::Other(e.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(MuseError::Other(format!("git {} failed", args.join(" "))))
    }
}

/// Run a shell command in `dir`; true iff it exits 0.
fn shell_ok_in(dir: &Path, cmd: &str) -> bool {
    let mut c = if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", cmd]);
        c
    } else {
        let mut c = std::process::Command::new("sh");
        c.args(["-c", cmd]);
        c
    };
    c.current_dir(dir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_round_trips() {
        let t = Task {
            name: "fix".into(),
            prompt: "make the tests pass".into(),
            check: Some("cargo test".into()),
        };
        let json = serde_json::to_string(&t).unwrap();
        let back: Task = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "fix");
        assert_eq!(back.check.as_deref(), Some("cargo test"));
    }

    #[test]
    fn worktree_branch_is_sanitized() {
        let b = worktree_branch("fix bug", "openai/gpt 5", 123);
        assert_eq!(b, "nur-bench-fix-bug-openai-gpt-5-123");
        assert!(!b.contains('/') && !b.contains(' '));
    }

    #[test]
    fn parse_models_splits_and_trims() {
        assert_eq!(parse_models("a, b ,c"), vec!["a", "b", "c"]);
        assert!(parse_models("  ").is_empty());
    }

    #[test]
    fn scoreboard_ranks_passing_fastest_first() {
        let results = vec![
            BenchResult {
                model: "slow-pass".into(),
                passed: true,
                secs: 20.0,
                tokens: 900,
                error: None,
            },
            BenchResult {
                model: "fail".into(),
                passed: false,
                secs: 3.0,
                tokens: 100,
                error: None,
            },
            BenchResult {
                model: "fast-pass".into(),
                passed: true,
                secs: 5.0,
                tokens: 800,
                error: None,
            },
        ];
        let board = scoreboard("t", &results);
        let fast = board.find("fast-pass").unwrap();
        let slow = board.find("slow-pass").unwrap();
        let fail = board.find("fail").unwrap();
        assert!(fast < slow, "faster passing model ranks first");
        assert!(slow < fail, "passing models rank above failing");
        assert!(board.contains("◀ best"));
        assert!(board.contains("PASS") && board.contains("FAIL"));
    }
}

//! Background jobs — push long-running work off the agent turn so the TUI
//! stays interactive and the model can keep working.
//!
//! Jobs live in process memory (+ a small status file under `~/.nur/bg-jobs/`)
//! so `/bg list` and the status chip work without blocking the agent loop.
//!
//! **Never store secrets** in job labels or result previews.

use crate::config::muse_home;
use crate::error::{MuseError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static REGISTRY: OnceLock<Arc<Mutex<JobRegistry>>> = OnceLock::new();

fn registry() -> Arc<Mutex<JobRegistry>> {
    REGISTRY
        .get_or_init(|| Arc::new(Mutex::new(JobRegistry::default())))
        .clone()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl JobState {
    pub fn as_str(self) -> &'static str {
        match self {
            JobState::Running => "running",
            JobState::Completed => "completed",
            JobState::Failed => "failed",
            JobState::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobInfo {
    pub id: u64,
    pub label: String,
    pub kind: String,
    pub state: JobState,
    pub created_at: u64,
    pub finished_at: Option<u64>,
    pub result_preview: Option<String>,
    pub error: Option<String>,
}

#[derive(Default)]
struct JobRegistry {
    next_id: u64,
    jobs: HashMap<u64, JobRecord>,
}

struct JobRecord {
    info: JobInfo,
    /// Full result kept in memory (also written to disk when done).
    result: Option<String>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn jobs_dir() -> PathBuf {
    muse_home().join("bg-jobs")
}

fn job_result_path(id: u64) -> PathBuf {
    jobs_dir().join(format!("{id}.txt"))
}

fn truncate_preview(s: &str, max: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= max {
        return t.to_string();
    }
    let head: String = t.chars().take(max.saturating_sub(1)).collect();
    format!("{head}…")
}

/// Spawn a named background job that runs `work` on a detached thread.
/// Returns the job id immediately.
pub fn spawn<F>(label: impl Into<String>, kind: impl Into<String>, work: F) -> u64
where
    F: FnOnce(Arc<std::sync::atomic::AtomicBool>) -> std::result::Result<String, String>
        + Send
        + 'static,
{
    let label = label.into();
    let kind = kind.into();
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cancel_worker = cancel.clone();

    let id = {
        let reg = registry();
        let mut g = reg.lock().unwrap_or_else(|e| e.into_inner());
        g.next_id = g.next_id.saturating_add(1).max(1);
        let id = g.next_id;
        g.jobs.insert(
            id,
            JobRecord {
                info: JobInfo {
                    id,
                    label: label.clone(),
                    kind: kind.clone(),
                    state: JobState::Running,
                    created_at: now_secs(),
                    finished_at: None,
                    result_preview: None,
                    error: None,
                },
                result: None,
                cancel,
            },
        );
        id
    };

    let _ = fs::create_dir_all(jobs_dir());

    thread::Builder::new()
        .name(format!("nur-bg-{id}"))
        .spawn(move || {
            let started = Instant::now();
            let outcome = work(cancel_worker.clone());
            let elapsed = started.elapsed();
            let reg = registry();
            let mut g = reg.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(rec) = g.jobs.get_mut(&id) {
                if cancel_worker.load(std::sync::atomic::Ordering::SeqCst)
                    && matches!(rec.info.state, JobState::Running)
                {
                    rec.info.state = JobState::Cancelled;
                    rec.info.finished_at = Some(now_secs());
                    rec.info.error = Some("cancelled".into());
                    return;
                }
                match outcome {
                    Ok(result) => {
                        rec.info.state = JobState::Completed;
                        rec.info.finished_at = Some(now_secs());
                        rec.info.result_preview = Some(truncate_preview(&result, 240));
                        rec.result = Some(result.clone());
                        let header = format!(
                            "# bg job {id} · {} · {} · {:?}\n\n",
                            rec.info.label, rec.info.kind, elapsed
                        );
                        let _ = fs::write(job_result_path(id), format!("{header}{result}"));
                    }
                    Err(err) => {
                        rec.info.state = JobState::Failed;
                        rec.info.finished_at = Some(now_secs());
                        rec.info.error = Some(truncate_preview(&err, 400));
                        rec.result = Some(err.clone());
                        let _ = fs::write(job_result_path(id), format!("# failed\n\n{err}"));
                    }
                }
            }
        })
        .ok();

    id
}

/// Spawn a detached OS command as a background job (stdout/stderr captured).
pub fn spawn_command(label: &str, program: &str, args: &[String]) -> u64 {
    let program = program.to_string();
    let args = args.to_vec();
    let label_owned = label.to_string();
    spawn(label_owned.clone(), "command", move |cancel| {
        if cancel.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("cancelled before start".into());
        }
        let mut cmd = Command::new(&program);
        cmd.args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("spawn {program}: {e}"))?;
        // Poll until exit or cancel.
        loop {
            if cancel.load(std::sync::atomic::Ordering::SeqCst) {
                let _ = child.kill();
                return Err("cancelled".into());
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    let out = child.wait_with_output().ok();
                    let stdout = out
                        .as_ref()
                        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                        .unwrap_or_default();
                    let stderr = out
                        .as_ref()
                        .map(|o| String::from_utf8_lossy(&o.stderr).to_string())
                        .unwrap_or_default();
                    if status.success() {
                        return Ok(format!(
                            "ok · {program}\n{stdout}{stderr}"
                        ));
                    }
                    return Err(format!(
                        "exit {status}\n{stdout}{stderr}"
                    ));
                }
                Ok(None) => thread::sleep(Duration::from_millis(120)),
                Err(e) => return Err(format!("wait: {e}")),
            }
        }
    })
}

pub fn list_jobs() -> Vec<JobInfo> {
    let reg = registry();
    let g = reg.lock().unwrap_or_else(|e| e.into_inner());
    let mut v: Vec<_> = g.jobs.values().map(|r| r.info.clone()).collect();
    v.sort_by_key(|j| std::cmp::Reverse(j.id));
    v
}

/// Count of jobs currently running — cheap for a future statusline count
/// beyond the label chip in [`status_chip`].
#[allow(dead_code)]
pub fn running_count() -> usize {
    list_jobs()
        .into_iter()
        .filter(|j| j.state == JobState::Running)
        .count()
}

pub fn get(id: u64) -> Option<JobInfo> {
    let reg = registry();
    let g = reg.lock().unwrap_or_else(|e| e.into_inner());
    g.jobs.get(&id).map(|r| r.info.clone())
}

pub fn result(id: u64) -> Result<String> {
    let reg = registry();
    let g = reg.lock().unwrap_or_else(|e| e.into_inner());
    let rec = g
        .jobs
        .get(&id)
        .ok_or_else(|| MuseError::Tool(format!("unknown bg job {id}")))?;
    match rec.info.state {
        JobState::Running => Ok(format!(
            "job {id} still running · {}\n  use bg(action=status, id={id}) or wait",
            rec.info.label
        )),
        JobState::Cancelled => Ok(format!("job {id} cancelled · {}", rec.info.label)),
        JobState::Failed => Ok(format!(
            "job {id} failed · {}\n{}",
            rec.info.label,
            rec.info.error.clone().unwrap_or_default()
        )),
        JobState::Completed => {
            if let Some(r) = &rec.result {
                return Ok(r.clone());
            }
            // Fall back to disk.
            drop(g);
            fs::read_to_string(job_result_path(id)).map_err(|e| {
                MuseError::Tool(format!("job {id} completed but result missing: {e}"))
            })
        }
    }
}

pub fn cancel(id: u64) -> Result<String> {
    let reg = registry();
    let mut g = reg.lock().unwrap_or_else(|e| e.into_inner());
    let rec = g
        .jobs
        .get_mut(&id)
        .ok_or_else(|| MuseError::Tool(format!("unknown bg job {id}")))?;
    if rec.info.state != JobState::Running {
        return Ok(format!(
            "job {id} is already {} — nothing to cancel",
            rec.info.state.as_str()
        ));
    }
    rec.cancel
        .store(true, std::sync::atomic::Ordering::SeqCst);
    rec.info.state = JobState::Cancelled;
    rec.info.finished_at = Some(now_secs());
    rec.info.error = Some("cancel requested".into());
    Ok(format!("cancelled bg job {id} · {}", rec.info.label))
}

/// One-line chip for the TUI status bar.
pub fn status_chip() -> Option<String> {
    let jobs = list_jobs();
    let running: Vec<_> = jobs
        .iter()
        .filter(|j| j.state == JobState::Running)
        .collect();
    if running.is_empty() {
        // Surface the most recent completion briefly if finished in last 30s.
        let recent = jobs.iter().find(|j| {
            j.state == JobState::Completed
                && j.finished_at
                    .map(|t| now_secs().saturating_sub(t) < 30)
                    .unwrap_or(false)
        });
        return recent.map(|j| format!("bg✓#{}", j.id));
    }
    if running.len() == 1 {
        Some(format!("bg·{}…", truncate_preview(&running[0].label, 18)))
    } else {
        Some(format!("bg×{}…", running.len()))
    }
}

/// Pretty multi-line report for tool / slash.
pub fn report() -> String {
    let jobs = list_jobs();
    if jobs.is_empty() {
        return "bg jobs: none\n  bg(action=run, command=…) or bg(action=spawn, label=…, kind=…)\n  /bg list\n".into();
    }
    let mut s = String::from("bg jobs (newest first):\n");
    for j in jobs.iter().take(20) {
        s.push_str(&format!(
            "  #{:<4} {:<10} {} · {}\n",
            j.id,
            j.state.as_str(),
            j.kind,
            j.label
        ));
        if let Some(p) = &j.result_preview {
            s.push_str(&format!("         {}\n", truncate_preview(p, 100)));
        }
        if let Some(e) = &j.error {
            s.push_str(&format!("         err: {}\n", truncate_preview(e, 100)));
        }
    }
    s.push_str("  /bg <id>  ·  bg(action=result, id=N)  ·  bg(action=cancel, id=N)\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_and_complete() {
        let id = spawn("unit-test", "test", |_c| {
            thread::sleep(Duration::from_millis(50));
            Ok("hello-bg".into())
        });
        // Wait up to 2s.
        for _ in 0..40 {
            if let Some(j) = get(id) {
                if j.state != JobState::Running {
                    break;
                }
            }
            thread::sleep(Duration::from_millis(50));
        }
        let j = get(id).expect("job");
        assert_eq!(j.state, JobState::Completed);
        let r = result(id).unwrap();
        assert!(r.contains("hello-bg"));
    }
}

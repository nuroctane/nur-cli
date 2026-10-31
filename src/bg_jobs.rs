//! Background jobs — push long-running work off the agent turn so the TUI
//! stays interactive and the model can keep working.
//!
//! Jobs live in process memory (+ a small status file under `~/.nur/bg-jobs/`)
//! so `/bg list` and the status chip work without blocking the agent loop.
//!
//! **Never store secrets** in job labels or result previews.

use crate::config::nur_home;
use crate::error::{NurError, Result};
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
    /// Whether the model should hear about this job finishing. Only jobs the
    /// model launched itself report back; TUI-side work (heartbeat, installs)
    /// would otherwise inject "#N completed" noise into its turn.
    notify_model: bool,
    /// Set once the agent loop has told the model this job finished.
    reported: bool,
}

/// Job kinds started by the model (shell handoff, `bg(action=run)`).
fn kind_reports_to_model(kind: &str) -> bool {
    matches!(kind, "shell" | "command")
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn jobs_dir() -> PathBuf {
    nur_home().join("bg-jobs")
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

/// Preview for a failed job: the first line (an exit/status line for command
/// jobs) plus the tail, where a build or test run states its reason.
fn failure_preview(s: &str, max: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= max {
        return t.to_string();
    }
    let first: String = t
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(max / 4)
        .collect();
    let tail_chars = max.saturating_sub(first.chars().count() + 1).max(1);
    let tail_start = t
        .char_indices()
        .rev()
        .nth(tail_chars - 1)
        .map(|(i, _)| i)
        .unwrap_or(0);
    format!("{first}…{}", &t[tail_start..])
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
                notify_model: kind_reports_to_model(&kind),
                reported: false,
            },
        );
        id
    };

    let _ = fs::create_dir_all(jobs_dir());

    let spawned = thread::Builder::new()
        .name(format!("nur-bg-{id}"))
        .spawn(move || {
            let started = Instant::now();
            let outcome = work(cancel_worker.clone());
            let cancelled = cancel_worker.load(std::sync::atomic::Ordering::SeqCst);
            finish(id, &label, &kind, outcome, started.elapsed(), cancelled);
        });
    if let Err(e) = spawned {
        let reg = registry();
        let mut g = reg.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(rec) = g.jobs.get_mut(&id) {
            rec.info.state = JobState::Failed;
            rec.info.finished_at = Some(now_secs());
            rec.info.error = Some(format!("could not start worker thread: {e}"));
        }
    }

    id
}

/// Record a finished job. The preview and the on-disk copy are produced
/// before the registry lock is taken: `list_jobs` runs on every TUI frame
/// and must not wait behind a multi-megabyte write.
fn finish(
    id: u64,
    label: &str,
    kind: &str,
    outcome: std::result::Result<String, String>,
    elapsed: Duration,
    cancelled: bool,
) {
    let (ok, text) = match outcome {
        Ok(t) => (true, t),
        Err(t) => (false, t),
    };
    let (preview, header) = if cancelled {
        (None, None)
    } else if ok {
        (
            Some(truncate_preview(&text, 240)),
            Some(format!(
                "# bg job {id} · {label} · {kind} · {elapsed:?}\n\n"
            )),
        )
    } else {
        (
            Some(failure_preview(&text, 400)),
            Some(format!(
                "# bg job {id} · {label} · {kind} · FAILED after {elapsed:?}\n\n"
            )),
        )
    };
    if let Some(header) = header {
        let _ = fs::write(job_result_path(id), format!("{header}{text}"));
    }

    let reg = registry();
    let mut g = reg.lock().unwrap_or_else(|e| e.into_inner());
    let Some(rec) = g.jobs.get_mut(&id) else {
        return;
    };
    rec.info.finished_at = Some(now_secs());
    // `cancel()` already flipped the state so `/bg list` shows it at once;
    // the worker's late return must not turn that into "failed".
    if cancelled || rec.info.state == JobState::Cancelled {
        rec.info.state = JobState::Cancelled;
        rec.info.error = Some("cancelled".into());
        rec.result = Some(text);
        return;
    }
    if ok {
        rec.info.state = JobState::Completed;
        rec.info.result_preview = preview;
    } else {
        rec.info.state = JobState::Failed;
        rec.info.error = preview;
    }
    rec.result = Some(text);
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
        crate::tools::tool_env(&mut cmd);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = cmd.spawn().map_err(|e| format!("spawn {program}: {e}"))?;
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
                        return Ok(format!("ok · {program}\n{stdout}{stderr}"));
                    }
                    return Err(format!("exit {status}\n{stdout}{stderr}"));
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

/// Jobs that finished since the last call, oldest first. Each is returned
/// once, so the agent loop can tell the model about completions exactly once.
pub fn take_unreported_completions() -> Vec<JobInfo> {
    let reg = registry();
    let mut g = reg.lock().unwrap_or_else(|e| e.into_inner());
    let mut done: Vec<JobInfo> = g
        .jobs
        .values_mut()
        .filter(|r| r.notify_model && !r.reported && r.info.state != JobState::Running)
        .map(|r| {
            r.reported = true;
            r.info.clone()
        })
        .collect();
    done.sort_by_key(|j| j.id);
    done
}

/// One compact note for the model about finished background jobs, or `None`
/// when nothing new finished.
pub fn completion_note() -> Option<String> {
    let done = take_unreported_completions();
    if done.is_empty() {
        return None;
    }
    let mut s = String::from("[bg] background job(s) finished:\n");
    for j in done {
        match j.state {
            JobState::Completed => {
                s.push_str(&format!(
                    "  #{} completed · {}\n    {}\n    full output: bg(action=result, id={})\n",
                    j.id,
                    j.label,
                    j.result_preview.unwrap_or_default().replace('\n', " ⏎ "),
                    j.id
                ));
            }
            JobState::Failed => {
                s.push_str(&format!(
                    "  #{} FAILED · {}\n    {}\n    details: bg(action=result, id={})\n",
                    j.id,
                    j.label,
                    j.error.unwrap_or_default().replace('\n', " ⏎ "),
                    j.id
                ));
            }
            JobState::Cancelled => {
                s.push_str(&format!("  #{} cancelled · {}\n", j.id, j.label));
            }
            JobState::Running => {}
        }
    }
    Some(s)
}

/// Model-launched jobs still running, oldest first, as one note that survives
/// compaction: the summary may drop the `bg` call that started a job, but the
/// model must still know what is in flight so it can wait or poll for it (the
/// finished result still arrives through `completion_note`).
pub fn running_note() -> Option<String> {
    let reg = registry();
    let g = reg.lock().unwrap_or_else(|e| e.into_inner());
    let mut running: Vec<&JobInfo> = g
        .jobs
        .values()
        .filter(|r| r.notify_model && r.info.state == JobState::Running)
        .map(|r| &r.info)
        .collect();
    if running.is_empty() {
        return None;
    }
    running.sort_by_key(|j| j.id);
    let now = now_secs();
    let mut s = String::from("[bg] background job(s) still running - results arrive when done:\n");
    for j in running {
        s.push_str(&format!(
            "  #{} running {}s · {}\n    status: bg(action=status, id={}) · output: bg(action=result, id={})\n",
            j.id,
            now.saturating_sub(j.created_at),
            truncate_preview(&j.label, 100),
            j.id,
            j.id
        ));
    }
    Some(s)
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
        .ok_or_else(|| NurError::Tool(format!("unknown bg job {id}")))?;
    match rec.info.state {
        JobState::Running => Ok(format!(
            "job {id} still running · {}\n  use bg(action=status, id={id}) or wait",
            rec.info.label
        )),
        JobState::Cancelled => Ok(format!("job {id} cancelled · {}", rec.info.label)),
        JobState::Failed => {
            // A failing build or test run is the common case: the full output
            // is the result, not the 400-char preview in `error`.
            let header = format!("job {id} failed · {}\n", rec.info.label);
            if let Some(r) = &rec.result {
                return Ok(format!("{header}{r}"));
            }
            let preview = rec.info.error.clone().unwrap_or_default();
            drop(g);
            match fs::read_to_string(job_result_path(id)) {
                Ok(text) => Ok(format!("{header}{text}")),
                Err(_) => Ok(format!("{header}{preview}")),
            }
        }
        JobState::Completed => {
            if let Some(r) = &rec.result {
                return Ok(r.clone());
            }
            // Fall back to disk.
            drop(g);
            fs::read_to_string(job_result_path(id))
                .map_err(|e| NurError::Tool(format!("job {id} completed but result missing: {e}")))
        }
    }
}

pub fn cancel(id: u64) -> Result<String> {
    let reg = registry();
    let mut g = reg.lock().unwrap_or_else(|e| e.into_inner());
    let rec = g
        .jobs
        .get_mut(&id)
        .ok_or_else(|| NurError::Tool(format!("unknown bg job {id}")))?;
    if rec.info.state != JobState::Running {
        return Ok(format!(
            "job {id} is already {} — nothing to cancel",
            rec.info.state.as_str()
        ));
    }
    rec.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
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

    /// Jobs write result files under the nur home, so each test re-runs
    /// itself in a child process with a throwaway `NUR_HOME` and only asserts
    /// there. Returns `true` inside the child.
    fn in_throwaway_home(test: &str) -> bool {
        const CHILD: &str = "NUR_TEST_BG_JOBS_CHILD";
        if std::env::var_os(CHILD).is_some() {
            return true;
        }
        let short = test.rsplit("::").next().unwrap_or(test);
        let home = std::env::temp_dir().join(format!("nur-bg-{}-{short}", std::process::id()));
        fs::create_dir_all(&home).unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture"])
            .env(CHILD, "1")
            .env("NUR_HOME", &home)
            .output()
            .unwrap();
        let _ = fs::remove_dir_all(&home);
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        false
    }

    fn wait_done(id: u64) -> JobInfo {
        for _ in 0..100 {
            if let Some(j) = get(id) {
                if j.state != JobState::Running {
                    return j;
                }
            }
            thread::sleep(Duration::from_millis(50));
        }
        panic!("job {id} did not finish");
    }

    #[test]
    fn spawn_and_complete() {
        if !in_throwaway_home("bg_jobs::tests::spawn_and_complete") {
            return;
        }
        let id = spawn("unit-test", "test", |_c| {
            thread::sleep(Duration::from_millis(50));
            Ok("hello-bg".into())
        });
        let j = wait_done(id);
        assert_eq!(j.state, JobState::Completed);
        let r = result(id).unwrap();
        assert!(r.contains("hello-bg"));
    }

    // Failure modes: a failed job's output collapses to the 400-char preview,
    // or the preview keeps the head and loses the reason at the tail.
    #[test]
    fn failed_job_keeps_its_full_output() {
        if !in_throwaway_home("bg_jobs::tests::failed_job_keeps_its_full_output") {
            return;
        }
        let body = format!(
            "exit status: 101\n{}\nerror: could not compile `nur`\nExit code: 101",
            "   Compiling line\n".repeat(200)
        );
        let expected = body.clone();
        let id = spawn("cargo build", "shell", move |_c| Err(body));
        let j = wait_done(id);
        assert_eq!(j.state, JobState::Failed);
        let preview = j.error.unwrap();
        assert!(preview.starts_with("exit status: 101…"), "{preview}");
        assert!(preview.ends_with("Exit code: 101"), "{preview}");
        assert!(preview.chars().count() <= 400);
        let r = result(id).unwrap();
        assert!(
            r.starts_with(&format!("job {id} failed · cargo build\n")),
            "{r}"
        );
        assert!(
            r.ends_with(&expected),
            "full output missing: len {} vs {}; tail {:?}",
            r.len(),
            expected.len(),
            &r[r.len().saturating_sub(120)..]
        );
        // The on-disk copy serves the same text once memory is gone.
        let disk = fs::read_to_string(job_result_path(id)).unwrap();
        assert!(disk.ends_with(&expected));
    }

    // Failure mode: `cancel()` flips the state first, so the worker's late
    // return used to overwrite it with "failed · cancelled".
    #[test]
    fn cancelled_job_reports_as_cancelled() {
        if !in_throwaway_home("bg_jobs::tests::cancelled_job_reports_as_cancelled") {
            return;
        }
        let id = spawn("sleepy", "shell", |cancel| {
            while !cancel.load(std::sync::atomic::Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(10));
            }
            Err("cancelled".into())
        });
        cancel(id).unwrap();
        let note = completion_note().expect("cancellation is reported");
        assert!(
            note.contains(&format!("#{id} cancelled · sleepy")),
            "{note}"
        );
        // Wait for the worker to observe the flag and finish.
        for _ in 0..100 {
            if get(id).and_then(|j| j.error) == Some("cancelled".into()) {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let j = get(id).unwrap();
        assert_eq!(j.state, JobState::Cancelled);
        assert!(
            report().contains(&format!("#{id:<4} cancelled")),
            "{}",
            report()
        );
        assert!(completion_note().is_none(), "reported exactly once");
    }

    // Failure mode: TUI-side jobs (heartbeat, installs) announce themselves
    // to the model as "#N completed".
    #[test]
    fn only_model_launched_kinds_report_completion() {
        if !in_throwaway_home("bg_jobs::tests::only_model_launched_kinds_report_completion") {
            return;
        }
        let quiet = spawn("heartbeat", "heartbeat", |_c| Ok("tick".into()));
        let loud = spawn("echo hi", "shell", |_c| Ok("hi".into()));
        wait_done(quiet);
        wait_done(loud);
        let done = take_unreported_completions();
        assert_eq!(done.iter().map(|j| j.id).collect::<Vec<_>>(), vec![loud]);
        assert!(take_unreported_completions().is_empty());
    }
}

//! Monitors: event-streaming background processes, modelled on Claude
//! Code's Monitor tool (2.1.98+, April 2026).
//!
//! A monitor runs a shell command in the background and each stdout line
//! is delivered to the model as an event, the moment it lands. That is the
//! opposite of `bg`: fire-and-forget, one notification on finish, no
//! visibility along the way. Use `monitor` for a live stream (build/deploy
//! logs, `tail -F` of a log file, a long test run); use `bg` when a single
//! notification on completion is enough.
//!
//! Semantics copied from Claude's Monitor:
//! - Lines arriving within 200ms of the first line in a batch are one
//!   notification.
//! - Stderr goes to a file (`~/.nur/monitors/<id>.stderr`) and never
//!   triggers events (merge with `2>&1` if failures must reach the model).
//! - Exit ends the watch and the exit code is reported.
//! - Cancel ends it. `timeout_ms` (default 5 minutes) ends it; a
//!   `persistent` monitor has no timeout and runs until stopped or nur exits.
//! - Stop, timeout and exit kill the whole process tree, so a `tail -F`
//!   under the shell does not outlive the watch.
//! - Never restored on resume: monitors live in process memory, so a new
//!   session starts with none.
//!
//! Events reach the model through the agent loop, the same way `bg`
//! completions do (`monitor_events_note`, injected once per round). The
//! `monitor` tool's `events` action drains them on demand.

use crate::config::nur_home;
use crate::error::{NurError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Lines within this window of the first line in a batch are one event.
const BATCH_WINDOW: Duration = Duration::from_millis(200);
/// Default watch length, matching Claude's Monitor.
const DEFAULT_TIMEOUT_MS: u64 = 300_000;
/// Cap on lines kept in one pending event (a runaway process must not
/// grow memory without bound).
const MAX_EVENT_LINES: usize = 200;
/// Cap on pending events per monitor.
const MAX_PENDING_EVENTS: usize = 50;

static REGISTRY: OnceLock<Arc<Mutex<MonitorRegistry>>> = OnceLock::new();

fn registry() -> Arc<Mutex<MonitorRegistry>> {
    REGISTRY
        .get_or_init(|| Arc::new(Mutex::new(MonitorRegistry::default())))
        .clone()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonitorState {
    Running,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
}

impl MonitorState {
    pub fn as_str(self) -> &'static str {
        match self {
            MonitorState::Running => "running",
            MonitorState::Completed => "completed",
            MonitorState::Failed => "failed",
            MonitorState::Cancelled => "cancelled",
            MonitorState::TimedOut => "timed out",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorInfo {
    pub id: u64,
    pub label: String,
    pub command: String,
    pub state: MonitorState,
    pub persistent: bool,
    pub timeout_ms: u64,
    pub created_at: u64,
    pub finished_at: Option<u64>,
    pub exit_code: Option<i32>,
    pub stderr_path: String,
}

/// One batch of stdout lines that arrived within the batch window.
#[derive(Debug, Clone)]
struct MonitorEvent {
    lines: Vec<String>,
}

#[derive(Default)]
struct MonitorRegistry {
    next_id: u64,
    monitors: HashMap<u64, MonitorRecord>,
}

struct MonitorRecord {
    info: MonitorInfo,
    pending: Vec<MonitorEvent>,
    /// Lines arrived but not yet delivered: the current batch, still inside
    /// its 200ms window. Lives here (not in the reader thread) so a
    /// flusher thread and the drain paths can flush it when the window
    /// elapses even if no further line arrives.
    batch: Vec<String>,
    batch_started: Option<Instant>,
    /// Dropped lines, when the pending cap was hit.
    dropped_lines: usize,
    /// The live child, so stop/timeout can kill it. Taken (None) once the
    /// reader thread has reaped the exit.
    child: Option<Child>,
    /// Set once the agent loop has been told this monitor finished.
    exit_reported: bool,
    notify_model: bool,
    owner_session: Option<String>,
}

/// Move the current batch into the pending queue, unconditionally.
fn flush_batch(rec: &mut MonitorRecord) {
    if rec.batch.is_empty() {
        return;
    }
    let lines = std::mem::take(&mut rec.batch);
    rec.batch_started = None;
    if rec.pending.len() >= MAX_PENDING_EVENTS {
        rec.dropped_lines += lines.len();
    } else {
        rec.pending.push(MonitorEvent { lines });
    }
}

/// Move the current batch into the pending queue once its 200ms window
/// has elapsed. Lines still inside the window stay batched, so a line
/// arriving at 180ms still groups with the batch.
fn flush_stale_batch(rec: &mut MonitorRecord) {
    match rec.batch_started {
        Some(start) if start.elapsed() >= BATCH_WINDOW => flush_batch(rec),
        _ => {}
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn monitors_dir() -> PathBuf {
    // cargo test does not set NUR_HOME; tests must not write the real home.
    if cfg!(test) {
        return std::env::temp_dir().join("nur-monitors-test");
    }
    nur_home().join("monitors")
}

fn stderr_path(id: u64) -> PathBuf {
    monitors_dir().join(format!("{id}.stderr"))
}

/// Start a monitor. Returns the monitor id. The process runs under the
/// shell (`sh -c` / `cmd /C`), same as `bg`, so the same permission rules
/// apply.
pub fn start(
    label: &str,
    command: &str,
    cwd: Option<&std::path::Path>,
    timeout_ms: Option<u64>,
    persistent: bool,
    owner_session: Option<String>,
) -> Result<u64> {
    // A persistent monitor has no timeout: it runs until stopped or nur exits.
    let timeout_ms = if persistent {
        0
    } else {
        timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS)
    };
    let _ = std::fs::create_dir_all(monitors_dir());
    let id = {
        let _registry = registry();
        let mut reg = _registry.lock().unwrap_or_else(|e| e.into_inner());
        reg.next_id += 1;
        reg.next_id
    };
    let stderr_file = std::fs::File::create(stderr_path(id))
        .map_err(|e| NurError::Tool(format!("cannot create monitor stderr file: {e}")))?;

    // The same shell as the `bash` tool (Git Bash on Windows when present),
    // so the syntax the model writes for bash works here too.
    let words = crate::tools::model_shell_words(command);
    let mut cmd = Command::new(&words[0]);
    cmd.args(&words[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(stderr_file));
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    // Same environment as bash/bg: provider keys scrubbed, saved secrets in.
    crate::tools::tool_env(&mut cmd);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    // Own process group, so stop/timeout can kill the whole tree.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| NurError::Tool(format!("cannot start monitor command: {e}")))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| NurError::Tool("cannot pipe monitor stdout".into()))?;

    let info = MonitorInfo {
        id,
        label: label.to_string(),
        command: command.to_string(),
        state: MonitorState::Running,
        persistent,
        timeout_ms,
        created_at: now_secs(),
        finished_at: None,
        exit_code: None,
        stderr_path: stderr_path(id).to_string_lossy().into_owned(),
    };
    {
        let _registry = registry();
        let mut reg = _registry.lock().unwrap_or_else(|e| e.into_inner());
        reg.monitors.insert(
            id,
            MonitorRecord {
                info,
                pending: Vec::new(),
                batch: Vec::new(),
                batch_started: None,
                dropped_lines: 0,
                child: Some(child),
                exit_reported: false,
                notify_model: true,
                owner_session,
            },
        );
    }

    // Reader thread: line-buffered stdout. Each line joins the current
    // batch; a line arriving after the batch's 200ms window flushes the
    // batch into the pending queue first. EOF means the process exited;
    // flush the last batch, reap the exit, and mark the monitor finished.
    let reg = registry();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break, // EOF: process exited.
                Ok(_) => {
                    let text = line.trim_end_matches(['\n', '\r']).to_string();
                    let _registry = registry();
                    let mut g = _registry.lock().unwrap_or_else(|e| e.into_inner());
                    let Some(rec) = g.monitors.get_mut(&id) else {
                        break;
                    };
                    // Stopped or timed out: a straggler line from a dying
                    // tree is not an event.
                    if rec.info.state != MonitorState::Running {
                        continue;
                    }
                    if rec.batch_started.is_none() {
                        rec.batch_started = Some(Instant::now());
                    }
                    if rec
                        .batch_started
                        .is_some_and(|start| start.elapsed() >= BATCH_WINDOW)
                    {
                        flush_batch(rec);
                        rec.batch_started = Some(Instant::now());
                    }
                    if rec.batch.len() < MAX_EVENT_LINES {
                        rec.batch.push(text);
                    } else {
                        rec.dropped_lines += 1;
                    }
                }
                Err(_) => break,
            }
        }
        let _registry = reg.clone();
        let mut g = _registry.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(rec) = g.monitors.get_mut(&id) {
            flush_batch(rec);
        }
        // Reap the exit and mark the monitor finished. A state already set
        // by stop/timeout wins over the exit code (a killed process has no
        // code, and must not overwrite TimedOut with Cancelled).
        if let Some(rec) = g.monitors.get_mut(&id) {
            let code = rec
                .child
                .as_mut()
                .and_then(|c| c.wait().ok())
                .and_then(|s| s.code());
            rec.child = None;
            rec.info.finished_at = Some(now_secs());
            rec.info.exit_code = code;
            if rec.info.state == MonitorState::Running {
                rec.info.state = match code {
                    Some(0) => MonitorState::Completed,
                    Some(_) => MonitorState::Failed,
                    None => MonitorState::Cancelled,
                };
            }
        }
    });

    // Flusher thread: wakes every 100ms and moves a batch into the pending
    // queue once its 200ms window has elapsed, so lines are delivered even
    // when no further line arrives. Exits once the monitor is finished.
    {
        let reg = registry();
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(100));
            let _registry = reg.clone();
            let mut g = _registry.lock().unwrap_or_else(|e| e.into_inner());
            let Some(rec) = g.monitors.get_mut(&id) else {
                break;
            };
            flush_stale_batch(rec);
            if rec.info.state != MonitorState::Running {
                break;
            }
        });
    }

    // Watchdog thread: kill the process when the timeout elapses. The
    // reader thread then reaps it; a killed process reports as timed out
    // when the watchdog marked it first.
    if timeout_ms > 0 {
        let reg = registry();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(timeout_ms));
            let pid = {
                let mut g = reg.lock().unwrap_or_else(|e| e.into_inner());
                g.monitors
                    .get_mut(&id)
                    .and_then(|rec| end_running(rec, MonitorState::TimedOut))
            };
            if let Some(pid) = pid {
                kill_tree(pid);
            }
        });
    }
    Ok(id)
}

/// Stop a monitor by id. Kills the process; the reader thread reaps it.
pub fn stop(id: u64) -> Result<String> {
    let (pid, label) = {
        let _registry = registry();
        let mut g = _registry.lock().unwrap_or_else(|e| e.into_inner());
        let Some(rec) = g.monitors.get_mut(&id) else {
            return Err(NurError::Tool(format!("unknown monitor {id}")));
        };
        if rec.info.state != MonitorState::Running {
            return Ok(format!(
                "monitor #{id} is already {}",
                rec.info.state.as_str()
            ));
        }
        (
            end_running(rec, MonitorState::Cancelled),
            rec.info.label.clone(),
        )
    };
    if let Some(pid) = pid {
        kill_tree(pid);
    }
    Ok(format!("monitor #{id} stopped · {label}"))
}

/// Kill every running monitor's process tree. Called on exit so a
/// persistent watch never outlives nur.
pub fn stop_all() {
    let pids: Vec<u32> = {
        let _registry = registry();
        let mut g = _registry.lock().unwrap_or_else(|e| e.into_inner());
        g.monitors
            .values_mut()
            .filter_map(|rec| end_running(rec, MonitorState::Cancelled))
            .collect()
    };
    for pid in pids {
        kill_tree(pid);
    }
}

/// Mark a running monitor as ended with `state` and return the pid to kill.
/// The kill runs outside the registry lock (taskkill can be slow).
fn end_running(rec: &mut MonitorRecord, state: MonitorState) -> Option<u32> {
    if rec.info.state != MonitorState::Running {
        return None;
    }
    rec.info.state = state;
    rec.child.as_ref().map(|c| c.id())
}

/// Kill a monitor's whole process tree: the shell plus anything it started
/// (a `tail -F` under `cmd /C` otherwise keeps the stdout pipe open).
fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(0x0800_0000)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(unix)]
    {
        // The child leads its own process group (`process_group(0)`).
        // SAFETY: kill(2) with a negative pid signals that group only.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
}

pub fn get(id: u64) -> Option<MonitorInfo> {
    let _registry = registry();
    let g = _registry.lock().unwrap_or_else(|e| e.into_inner());
    g.monitors.get(&id).map(|r| r.info.clone())
}

pub fn list() -> Vec<MonitorInfo> {
    let _registry = registry();
    let g = _registry.lock().unwrap_or_else(|e| e.into_inner());
    let mut v: Vec<_> = g.monitors.values().map(|r| r.info.clone()).collect();
    v.sort_by_key(|m| std::cmp::Reverse(m.id));
    v
}

/// Pending events for one monitor, oldest first, drained (each event is
/// delivered once). A batch whose 200ms window has elapsed is flushed
/// first, so lines are not held back waiting for a next line.
pub fn take_events(id: u64) -> Result<Vec<Vec<String>>> {
    let _registry = registry();
    let mut g = _registry.lock().unwrap_or_else(|e| e.into_inner());
    let Some(rec) = g.monitors.get_mut(&id) else {
        return Err(NurError::Tool(format!("unknown monitor {id}")));
    };
    flush_stale_batch(rec);
    Ok(std::mem::take(&mut rec.pending)
        .into_iter()
        .map(|e| e.lines)
        .collect())
}

/// One compact note for the model about monitor events and exits since the
/// last round, or `None` when nothing new happened. Each event is reported
/// once, exactly like `bg` completions.
pub fn events_note_for(session: Option<&str>) -> Option<String> {
    let _registry = registry();
    let mut g = _registry.lock().unwrap_or_else(|e| e.into_inner());
    let mut out = String::new();
    let mut ids: Vec<u64> = g.monitors.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        let Some(rec) = g.monitors.get_mut(&id) else {
            continue;
        };
        if !rec.notify_model
            || (rec.owner_session.is_some() && rec.owner_session.as_deref() != session)
        {
            continue;
        }
        flush_stale_batch(rec);
        let events = std::mem::take(&mut rec.pending);
        let dropped = std::mem::take(&mut rec.dropped_lines);
        if !events.is_empty() {
            out.push_str(&format!(
                "[monitor] #{} ({}) events:\n",
                rec.info.id, rec.info.label
            ));
            for event in events {
                for line in event.lines {
                    out.push_str(&format!("  {line}\n"));
                }
            }
            if dropped > 0 {
                out.push_str(&format!("  ... {dropped} lines dropped (cap)\n"));
            }
        }
        if !rec.exit_reported && rec.info.state != MonitorState::Running {
            rec.exit_reported = true;
            let code = rec
                .info
                .exit_code
                .map(|c| format!("exit code {c}"))
                .unwrap_or_default();
            out.push_str(&format!(
                "[monitor] #{} ({}) ended: {} {}\n",
                rec.info.id,
                rec.info.label,
                rec.info.state.as_str(),
                code
            ));
            if rec.info.state == MonitorState::Failed {
                out.push_str(&format!("  stderr: {}\n", rec.info.stderr_path));
            }
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Monitors still running, oldest first, as one note that survives
/// compaction: the summary may drop the `monitor` call that started one,
/// but the model must still know what is in flight.
pub fn running_note(session: &str) -> Option<String> {
    let _registry = registry();
    let g = _registry.lock().unwrap_or_else(|e| e.into_inner());
    let mut running: Vec<&MonitorInfo> = g
        .monitors
        .values()
        .filter(|r| {
            r.notify_model
                && r.info.state == MonitorState::Running
                && (r.owner_session.is_none() || r.owner_session.as_deref() == Some(session))
        })
        .map(|r| &r.info)
        .collect();
    if running.is_empty() {
        return None;
    }
    running.sort_by_key(|m| m.id);
    let mut s = String::from("[monitor] still running, events arrive as they stream:\n");
    for m in running {
        s.push_str(&format!(
            "  #{} · {} · {} · timeout {}s{}\n",
            m.id,
            m.label,
            m.command,
            m.timeout_ms / 1000,
            if m.persistent { " · persistent" } else { "" }
        ));
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Batching: lines within 200ms are one event; a pause splits them.
    #[test]
    fn lines_within_200ms_batch_into_one_event() {
        // Shell-neutral: the monitor runs under the bash tool's shell, which
        // is Git Bash, pwsh or cmd depending on the machine.
        let cmd = "python -u -c \"import time; print('a'); print('b'); time.sleep(1); print('c')\"";
        let id = start(
            "batch test",
            cmd,
            None,
            Some(60_000),
            false,
            Some("batch-test".into()),
        )
        .expect("start");
        // Give the reader thread time to drain the fast lines.
        thread::sleep(Duration::from_millis(500));
        let events = take_events(id).expect("events");
        assert!(!events.is_empty(), "fast lines should arrive");
        assert_eq!(events[0], vec!["a".to_string(), "b".to_string()]);
        // The later line arrives as its own event after the sleep.
        thread::sleep(Duration::from_millis(900));
        let events = take_events(id).expect("events");
        assert_eq!(events, vec![vec!["c".to_string()]]);
        let _ = stop(id);
    }

    #[test]
    fn exit_is_reported_once_with_code() {
        let id = start("exit test", "exit 3", None, Some(60_000), false, None).expect("start");
        for _ in 0..50 {
            thread::sleep(Duration::from_millis(100));
            if let Some(info) = get(id) {
                if info.state != MonitorState::Running {
                    break;
                }
            }
        }
        let info = get(id).expect("info");
        assert_eq!(info.state, MonitorState::Failed);
        assert_eq!(info.exit_code, Some(3));
        let note = events_note_for(None).expect("note");
        assert!(note.contains("ended: failed"));
        assert!(note.contains("exit code 3"));
        assert!(events_note_for(None).is_none(), "reported exactly once");
    }

    #[test]
    fn stderr_produces_no_events() {
        let cmd = "python -u -c \"import sys; print('out'); print('err', file=sys.stderr)\"";
        let id = start(
            "stderr test",
            cmd,
            None,
            Some(60_000),
            false,
            Some("stderr-test".into()),
        )
        .expect("start");
        thread::sleep(Duration::from_millis(500));
        let events = take_events(id).expect("events");
        assert_eq!(events, vec![vec!["out".to_string()]]);
        let _ = stop(id);
    }

    // Failure mode: stop killed only the shell, a grandchild kept the stdout
    // pipe open, and the reader never saw EOF (the process lived on).
    #[test]
    fn stop_kills_the_whole_tree() {
        // A grandchild that inherits stdout and outlives a shell-only kill.
        let cmd = "python -c \"import subprocess, sys; subprocess.run([sys.executable, '-c', 'import time; time.sleep(30)'])\"";
        let id = start(
            "tree test",
            cmd,
            None,
            Some(60_000),
            false,
            Some("tree-test".into()),
        )
        .expect("start");
        thread::sleep(Duration::from_millis(300));
        stop(id).expect("stop");
        let mut reaped = false;
        for _ in 0..100 {
            thread::sleep(Duration::from_millis(100));
            if get(id).and_then(|m| m.finished_at).is_some() {
                reaped = true;
                break;
            }
        }
        assert!(reaped, "reader saw EOF after stop");
        assert_eq!(get(id).unwrap().state, MonitorState::Cancelled);
    }
}

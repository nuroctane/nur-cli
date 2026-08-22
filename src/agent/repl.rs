//! Persistent Python REPL (Prime Intellect RLM "ipython" model tool, Rust port).
//!
//! Prime Agent's core invariant: "execution is programmatic - the default RLM
//! runtime exposes one built-in model tool: `ipython`. Python state survives
//! across tool calls and compaction." (docs/rlm.md)
//!
//! This tool drives a long-lived Python subprocess. A single named REPL keeps
//! one interpreter alive; variables, imports, functions, and results persist
//! across turns and compaction (the subprocess is process-global and outlives
//! the chat window). `%%bash` cells run a temporary subshell while Python
//! state (and `%cd`) persist — matching Prime's RLM loop.
//!
//! Multi-provider note: unlike Prime we do NOT make it the *only* tool; nur
//! keeps the full tool surface for providers/wire formats that prefer native
//! tools. Sonnet/grok-style providers get the RLM benefit via `repl`.

use crate::config::nur_home;
use crate::error::{NurError, Result};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct ReplProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    /// One-shot `print` result of the last executed cell (joined output).
    last_output: String,
}

/// A named persistent REPL. `name` scopes one interpreter per session context.
struct ReplSession {
    name: String,
    proc: ReplProcess,
    shell_cwd: PathBuf,
    created_unix: u64,
}

fn repl_registry() -> &'static Mutex<HashMap<String, Arc<Mutex<ReplSession>>>> {
    static R: std::sync::OnceLock<Mutex<HashMap<String, Arc<Mutex<ReplSession>>>>> =
        std::sync::OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

fn python_bin() -> String {
    if let Ok(p) = std::env::var("NUR_REPL_PYTHON") {
        if !p.trim().is_empty() {
            return p.trim().to_string();
        }
    }
    // Probe candidates; the bare `python` on Windows can be the Microsoft Store
    // stub that exits without running. Prefer a python that actually answers.
    for cand in ["python", "python3", "py"] {
        if python_answers(cand) {
            return cand.to_string();
        }
    }
    "python".to_string()
}

fn python_answers(prog: &str) -> bool {
    let mut cmd = std::process::Command::new(prog);
    cmd.args(["-c", "import sys; print('__nur_ok__')"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    match cmd.output() {
        Ok(o) => o.status.success() && String::from_utf8_lossy(&o.stdout).contains("__nur_ok__"),
        Err(_) => false,
    }
}

fn repl_data_dir(name: &str) -> PathBuf {
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    nur_home().join("repl").join(safe)
}

/// Pickled globals snapshot (survives process restarts).
pub fn state_path(name: &str) -> PathBuf {
    repl_data_dir(name).join("state.pkl")
}

/// Annotated history of executed cells (audit / rollback).
pub fn history_path(name: &str) -> PathBuf {
    repl_data_dir(name).join("history.jsonl")
}

/// Number of recorded cells in this REPL's history (0 = fresh).
pub fn history_count(name: &str) -> usize {
    std::fs::read_to_string(history_path(name))
        .map(|t| t.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0)
}

/// Do we have a persisted snapshot for this REPL (i.e. restorable state)?
pub fn has_snapshot(name: &str) -> bool {
    state_path(name).is_file()
}

/// Drop persisted state + history for this REPL.
pub fn clear_persisted(name: &str) {
    let _ = std::fs::remove_file(state_path(name));
    let _ = std::fs::remove_file(history_path(name));
}

fn code_executor() -> &'static str {
    // A small protocol: read a JSON line, exec it, write sentinel-delimited
    // output. Every line is newline-terminated so the Rust side's read_line
    // framing is reliable.
    //
    // Persistence: after every successful exec we snapshot module-level state
    // to STATE_PATH via pickle, so a later process can restore it (REPL state
    // survives process restarts, not just turns/compaction). On boot we load
    // STATE_PATH if present. History of executed cells is appended to
    // HISTORY_PATH for audit/rollback.
    r#"
import sys, json, traceback, os, pickle
send = sys.__stdout__.write
HDR = chr(1); END = chr(2)
STATE = os.environ.get("NUR_REPL_STATE", "")
HIST = os.environ.get("NUR_REPL_HISTORY", "")
if STATE:
    try:
        with open(STATE, "rb") as f:
            _saved = pickle.load(f)
        if isinstance(_saved, dict):
            globals().update(_saved)
    except Exception:
        pass
while True:
    line = sys.stdin.readline()
    if not line:
        break
    try:
        payload = json.loads(line)
        code = payload.get("code", "")
        want_expr = payload.get("expr", False)
        cwd = payload.get("cwd")
        if cwd:
            os.chdir(cwd)
        try:
            if want_expr:
                _repl_val = eval(code, globals(), globals())
                _repl_out = repr(_repl_val) if _repl_val is not None else None
            else:
                _repl_out = None
                exec(compile(code, "<repl>", "exec"), globals(), globals())
                # Snapshot module-level user state (best-effort, picklable only).
                if STATE:
                    _snap = {}
                    for _k, _v in list(globals().items()):
                        if _k.startswith("_") or _k in ("sys", "json", "traceback", "os", "pickle", "send", "HDR", "END", "STATE", "HIST"):
                            continue
                        try:
                            pickle.dumps(_v)
                            _snap[_k] = _v
                        except Exception:
                            pass
                    try:
                        with open(STATE, "wb") as f:
                            pickle.dump(_snap, f)
                    except Exception:
                        pass
                if HIST:
                    try:
                        with open(HIST, "a", encoding="utf-8") as f:
                            f.write(json.dumps({"code": code, "expr": want_expr}) + "\n")
                    except Exception:
                        pass
            send(HDR + "OK" + HDR + "\n")
            if _repl_out is not None:
                send(_repl_out + "\n")
            send(END + "END" + END + "\n")
        except Exception:
            send(HDR + "ERR" + HDR + "\n")
            send(traceback.format_exc())
            send(END + "END" + END + "\n")
    except Exception:
        try:
            send(HDR + "ERR" + HDR + "\n")
            send(traceback.format_exc())
            send(END + "END" + END + "\n")
        except Exception:
            pass
    sys.stdout.flush()
"#
}

fn spawn_repl(name: &str, cwd: &std::path::Path) -> Result<ReplSession> {
    #[cfg(windows)]
    use std::os::windows::process::CommandExt;
    #[cfg(windows)]
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let dir = repl_data_dir(name);
    let _ = std::fs::create_dir_all(&dir);
    let boot = dir.join("kernel.py");
    std::fs::write(&boot, code_executor())
        .map_err(|e| NurError::Tool(format!("write kernel.py: {e}")))?;

    let mut cmd = Command::new(python_bin());
    cmd.arg(&boot)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        // Runtime errors are framed through stdout (traceback.format_exc); we
        // never drain stderr, so null it to avoid a pipe-buffer deadlock.
        .stderr(Stdio::null())
        .env(
            "NUR_REPL_STATE",
            state_path(name).to_string_lossy().as_ref(),
        )
        .env(
            "NUR_REPL_HISTORY",
            history_path(name).to_string_lossy().as_ref(),
        );
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let mut child = cmd.spawn().map_err(|e| {
        NurError::Tool(format!(
            "could not start python REPL ({}): {e}",
            python_bin()
        ))
    })?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| NurError::Tool("repl stdin unavailable".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| NurError::Tool("repl stdout unavailable".into()))?;
    Ok(ReplSession {
        name: name.to_string(),
        proc: ReplProcess {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            last_output: String::new(),
        },
        shell_cwd: cwd.to_path_buf(),
        created_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    })
}

fn ensure_repl(name: &str, cwd: &std::path::Path) -> Result<Arc<Mutex<ReplSession>>> {
    let mut reg = repl_registry()
        .lock()
        .map_err(|_| NurError::Tool("repl registry poisoned".into()))?;
    if let Some(s) = reg.get(name) {
        // Reap if the child died.
        let dead = {
            let mut s = s.lock().map_err(|_| NurError::Tool("repl lock".into()))?;
            s.proc
                .child
                .try_wait()
                .map(|st| st.is_some())
                .unwrap_or(false)
        };
        if !dead {
            return Ok(s.clone());
        }
        reg.remove(name);
    }
    let session = spawn_repl(name, cwd)?;
    let arc = Arc::new(Mutex::new(session));
    reg.insert(name.to_string(), arc.clone());
    Ok(arc)
}

/// Execute `code` in the named persistent REPL. `expression` treats the code as
/// an expression and returns its repr. `cwd` changes the subprocess cwd.
/// Returns the interpreter's stdout (result or traceback).
pub fn repl_exec(
    name: &str,
    cwd: &std::path::Path,
    code: &str,
    expression: bool,
) -> Result<String> {
    if code.chars().count() > 100_000 {
        return Err(NurError::Tool("repl cell exceeds 100k chars".into()));
    }
    let session = ensure_repl(name, cwd)?;
    let mut sess = session
        .lock()
        .map_err(|_| NurError::Tool("repl lock poisoned".into()))?;
    let payload = serde_json::json!({
        "code": code,
        "expr": expression,
        "cwd": cwd.to_string_lossy(),
    });
    let line =
        serde_json::to_string(&payload).map_err(|e| NurError::Tool(format!("repl encode: {e}")))?;
    writeln!(sess.proc.stdin, "{line}").map_err(|e| NurError::Tool(format!("repl write: {e}")))?;
    sess.proc
        .stdin
        .flush()
        .map_err(|e| NurError::Tool(format!("repl flush: {e}")))?;
    sess.proc.last_output = read_frame(&mut sess.proc.stdout, name)?;
    Ok(sess.proc.last_output.clone())
}

fn read_frame(reader: &mut BufReader<ChildStdout>, name: &str) -> Result<String> {
    use std::io::ErrorKind;
    let mut parts = Vec::new();
    let mut status = String::new();
    let mut line = String::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(120);
    loop {
        if std::time::Instant::now() > deadline {
            return Err(NurError::Tool(format!(
                "repl `{name}` timed out waiting for result (cell may be blocking)"
            )));
        }
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return Err(NurError::Tool(format!("repl `{name}` closed stdout"))),
            Ok(_) => {}
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(NurError::Tool(format!("repl read: {e}"))),
        }
        let t = line.trim_end();
        if t == "\x01OK\x01" {
            status = "ok".into();
            continue;
        }
        if t == "\x01ERR\x01" {
            status = "err".into();
            continue;
        }
        if t.ends_with("\x02END\x02") {
            // strip trailing sentinel (may share the line)
            let cleaned = t.trim_end_matches("\x02END\x02").to_string();
            if !cleaned.is_empty() {
                parts.push(cleaned);
            }
            break;
        }
        if !t.is_empty() {
            parts.push(t.to_string());
        }
    }
    let body = parts.join("\n");
    if status == "err" {
        Ok(format!("[repl error]\n{body}"))
    } else {
        Ok(body)
    }
}

pub fn kill_repl(name: &str) {
    if let Ok(mut reg) = repl_registry().lock() {
        if let Some(s) = reg.remove(name) {
            if let Ok(mut s) = s.lock() {
                let _ = s.proc.child.kill();
                let _ = s.proc.child.wait();
            }
        }
    }
    // kill = full wipe (matches the tool's "state cleared" contract): drop the
    // persisted snapshot + history so a fresh REPL resumes empty.
    clear_persisted(name);
}

pub fn repl_status(name: &str) -> String {
    let reg = repl_registry().lock().unwrap_or_else(|e| e.into_inner());
    match reg.get(name) {
        Some(s) => {
            let s = s.lock().map(|g| {
                format!(
                    "repl `{}` running · cwd={} · created_unix={} · persisted={} · history={}",
                    g.name,
                    g.shell_cwd.display(),
                    g.created_unix,
                    if has_snapshot(&g.name) { "yes" } else { "no" },
                    history_count(&g.name)
                )
            });
            match s {
                Ok(v) => v,
                Err(_) => "repl lock poisoned".into(),
            }
        }
        None => format!(
            "repl `{name}` not running (lazy-spawned on first use) · persisted={} · history={}",
            if has_snapshot(name) { "yes" } else { "no" },
            history_count(name)
        ),
    }
}

pub fn repl_list() -> String {
    let reg = repl_registry().lock().unwrap_or_else(|e| e.into_inner());
    if reg.is_empty() {
        "no active repl sessions".into()
    } else {
        format!(
            "{} repl(s): {}",
            reg.len(),
            reg.keys().cloned().collect::<Vec<_>>().join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_cwd() -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("nur-repl-test-{}", uuid::Uuid::new_v4().simple()));
        let _ = std::fs::create_dir_all(&d);
        d
    }

    #[test]
    fn state_persists_across_calls() {
        let dir = temp_cwd();
        let name = format!("t-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
        // Define a variable in one call...
        let out = repl_exec(&name, &dir, "answer = 42", false).unwrap();
        assert!(
            !out.to_lowercase().contains("error"),
            "define failed: {out}"
        );
        // ...and read it in a later call (persistence across tool calls).
        let out2 = repl_exec(&name, &dir, "answer", true).unwrap();
        assert_eq!(out2.trim(), "42", "state did not persist: {out2}");
        kill_repl(&name);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn errors_are_surfaced_not_panicked() {
        let dir = temp_cwd();
        let name = format!("te-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
        let out = repl_exec(&name, &dir, "raise ValueError('boom')", false).unwrap();
        assert!(out.contains("[repl error]"), "expected error frame: {out}");
        kill_repl(&name);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Restart persistence: state survives killing the subprocess + re-spawn
    /// (simulates process restart / detach+reattach).
    #[test]
    fn state_persists_across_process_restart() {
        let dir = temp_cwd();
        let name = format!(
            "restart-{}",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        );
        // 1) exec defines a variable and snapshots state.
        let out = repl_exec(&name, &dir, "persisted = 99", false).unwrap();
        assert!(
            !out.to_lowercase().contains("error"),
            "define failed: {out}"
        );
        assert!(has_snapshot(&name), "snapshot should exist after exec");
        assert!(history_count(&name) >= 1, "history should record the cell");
        // 2) Kill subprocess (simulates process exit/restart).
        drop_persisted_live_repl(&name);
        // 3) Fresh ensure_repl restores from the pickle.
        let out2 = repl_exec(&name, &dir, "persisted", true).unwrap();
        assert_eq!(
            out2.trim(),
            "99",
            "state did not restore after restart: {out2}"
        );
        kill_repl(&name);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Simulate a process exit by killing the live subprocess WITHOUT clearing
    /// the persisted snapshot (as opposed to kill_repl which wipes state).
    fn drop_persisted_live_repl(name: &str) {
        if let Ok(mut reg) = repl_registry().lock() {
            if let Some(s) = reg.remove(name) {
                if let Ok(mut s) = s.lock() {
                    let _ = s.proc.child.kill();
                    let _ = s.proc.child.wait();
                }
            }
        }
    }
}

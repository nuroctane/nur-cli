//! Honest shell selection — never claim "bash" when running cmd.exe.

use crate::error::{NurError, Result};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Default when the model omits `timeout_ms`.
pub const DEFAULT_TIMEOUT_MS: u64 = 60_000;
/// Hard ceiling — models (especially Claude) often request huge timeouts that
/// just make hung commands feel immortal.
pub const MAX_TIMEOUT_MS: u64 = 180_000;
/// No stdout/stderr bytes for this long → treat as hung (waiting on stdin, etc.).
/// Kept long enough that quiet compilers/linkers are not killed mid-build.
const IDLE_TIMEOUT_MS: u64 = 90_000;
/// Grace before idle timeout starts (compilers are quiet at startup).
const IDLE_GRACE_MS: u64 = 20_000;
/// How long we wait for pipe drain threads after killing the process tree.
const JOIN_AFTER_KILL_MS: u64 = 2_000;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn read_all(pipe: Option<impl Read>, progress_ms: Arc<AtomicU64>) -> Vec<u8> {
    // Cap at 2MB per stream to prevent memory blow-up from cat largefile etc
    const CAP: usize = 2_000_000;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    let Some(mut p) = pipe else {
        return buf;
    };
    loop {
        match p.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                progress_ms.store(now_ms(), Ordering::Relaxed);
                if buf.len() < CAP {
                    let take = n.min(CAP - buf.len());
                    buf.extend_from_slice(&tmp[..take]);
                }
                // Past CAP: keep draining so the child does not block on a full pipe.
            }
            Err(_) => break,
        }
    }
    buf
}

fn join_with_timeout(handle: thread::JoinHandle<Vec<u8>>, timeout_ms: u64) -> Vec<u8> {
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(handle.join().unwrap_or_default());
    });
    rx.recv_timeout(Duration::from_millis(timeout_ms))
        .unwrap_or_default()
}

#[derive(Debug, Clone)]
pub struct ShellBackend {
    pub kind: ShellKind,
    pub program: PathBuf,
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    /// Real bash (Git Bash, WSL, or /bin/bash).
    Bash,
    /// PowerShell 7+ (`pwsh`).
    Pwsh,
    /// Windows PowerShell 5.
    PowerShell,
    /// Last-resort cmd.exe.
    Cmd,
}

/// Process-wide cached shell backend. `detect_shell` probes the filesystem and
/// scans PATH, so callers on hot paths (system prompt, every bash call) use this.
pub fn shell_backend() -> &'static ShellBackend {
    static B: std::sync::OnceLock<ShellBackend> = std::sync::OnceLock::new();
    B.get_or_init(detect_shell)
}

/// Detect the best available shell (prefer `shell_backend()` — this probes disk).
pub fn detect_shell() -> ShellBackend {
    // 1) Explicit override
    if let Ok(p) = std::env::var("NUR_SHELL") {
        let pb = PathBuf::from(&p);
        if pb.is_file() || which_exists(&p) {
            let kind = if p.to_ascii_lowercase().contains("bash") {
                ShellKind::Bash
            } else if p.to_ascii_lowercase().contains("pwsh") {
                ShellKind::Pwsh
            } else if p.to_ascii_lowercase().contains("powershell") {
                ShellKind::PowerShell
            } else {
                ShellKind::Bash
            };
            return ShellBackend {
                kind,
                program: pb,
                label: format!("NUR_SHELL={p}"),
            };
        }
    }

    // 2) Prefer real bash (Git for Windows, user PATH, WSL via bash.exe)
    let bash_candidates = [
        r"C:\Program Files\Git\bin\bash.exe",
        r"C:\Program Files\Git\usr\bin\bash.exe",
        r"C:\Program Files (x86)\Git\bin\bash.exe",
    ];
    for c in bash_candidates {
        let p = PathBuf::from(c);
        if p.is_file() {
            return ShellBackend {
                kind: ShellKind::Bash,
                program: p,
                label: "git-bash".into(),
            };
        }
    }
    if let Some(p) = which("bash") {
        return ShellBackend {
            kind: ShellKind::Bash,
            program: p,
            label: "bash".into(),
        };
    }

    // 3) PowerShell 7
    if let Some(p) = which("pwsh") {
        return ShellBackend {
            kind: ShellKind::Pwsh,
            program: p,
            label: "pwsh".into(),
        };
    }

    // 4) Windows PowerShell
    #[cfg(windows)]
    {
        let ps = PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe");
        if ps.is_file() {
            return ShellBackend {
                kind: ShellKind::PowerShell,
                program: ps,
                label: "powershell".into(),
            };
        }
        if let Some(p) = which("powershell") {
            return ShellBackend {
                kind: ShellKind::PowerShell,
                program: p,
                label: "powershell".into(),
            };
        }
    }

    // 5) cmd last resort
    #[cfg(windows)]
    {
        ShellBackend {
            kind: ShellKind::Cmd,
            program: PathBuf::from("cmd.exe"),
            label: "cmd.exe".into(),
        }
    }
    #[cfg(not(windows))]
    {
        ShellBackend {
            kind: ShellKind::Bash,
            program: PathBuf::from("/bin/sh"),
            label: "sh".into(),
        }
    }
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let c = dir.join(name);
        if c.is_file() {
            return Some(c);
        }
        #[cfg(windows)]
        {
            let exe = dir.join(format!("{name}.exe"));
            if exe.is_file() {
                return Some(exe);
            }
        }
    }
    None
}

fn which_exists(name: &str) -> bool {
    Path::new(name).is_file() || which(name).is_some()
}

/// Kill a process and its whole tree (grandchildren included).
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        // taskkill /T takes the entire tree down; child.kill() alone leaves
        // grandchildren (e.g. cmd → node) running.
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Clamp model-requested timeouts into a sane band.
pub fn clamp_timeout_ms(requested: u64) -> u64 {
    requested.clamp(1_000, MAX_TIMEOUT_MS)
}

pub fn run_in_shell(
    backend: &ShellBackend,
    command: &str,
    cwd: &Path,
    timeout_ms: u64,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<String> {
    let kind = backend.kind;
    let label = backend.label.clone();
    let timeout_ms = clamp_timeout_ms(timeout_ms);

    let mut cmd = Command::new(&backend.program);
    cmd.current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Nudge CLIs away from interactive prompts (stdin is already null).
        .env("CI", "1")
        .env("TERM", "dumb")
        .env("NO_COLOR", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env("NPM_CONFIG_YES", "true")
        .env("DEBIAN_FRONTEND", "noninteractive")
        .env("PYTHONUNBUFFERED", "1");
    match kind {
        ShellKind::Bash => {
            cmd.args(["-lc", command]);
        }
        ShellKind::Pwsh | ShellKind::PowerShell => {
            cmd.args(["-NoProfile", "-NonInteractive", "-Command", command]);
        }
        ShellKind::Cmd => {
            cmd.args(["/C", command]);
        }
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| NurError::Tool(format!("command failed to start: {e}")))?;

    // Drain pipes on threads so a chatty child can't deadlock on a full pipe.
    let progress = Arc::new(AtomicU64::new(now_ms()));
    let out_pipe = child.stdout.take();
    let err_pipe = child.stderr.take();
    let prog_out = Arc::clone(&progress);
    let prog_err = Arc::clone(&progress);
    let out_h = thread::spawn(move || read_all(out_pipe, prog_out));
    let err_h = thread::spawn(move || read_all(err_pipe, prog_err));

    let started = Instant::now();
    let deadline = started + Duration::from_millis(timeout_ms);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if cancel.is_cancelled() {
                    kill_tree(&mut child);
                    let _ = join_with_timeout(out_h, JOIN_AFTER_KILL_MS);
                    let _ = join_with_timeout(err_h, JOIN_AFTER_KILL_MS);
                    return Err(NurError::Tool(
                        "command cancelled by user (process tree killed)".into(),
                    ));
                }
                if Instant::now() >= deadline {
                    kill_tree(&mut child);
                    let _ = join_with_timeout(out_h, JOIN_AFTER_KILL_MS);
                    let _ = join_with_timeout(err_h, JOIN_AFTER_KILL_MS);
                    return Err(NurError::Tool(format!(
                        "command timed out after {timeout_ms}ms (process tree killed). \
                         Do not retry the same command with a longer timeout - use \
                         list_dir/read_file/grep/glob, or a narrower non-interactive command."
                    )));
                }
                // Idle: no pipe bytes for IDLE_TIMEOUT after grace → likely waiting on stdin.
                if started.elapsed() >= Duration::from_millis(IDLE_GRACE_MS) {
                    let last = progress.load(Ordering::Relaxed);
                    let idle_for = now_ms().saturating_sub(last);
                    if idle_for >= IDLE_TIMEOUT_MS {
                        kill_tree(&mut child);
                        let _ = join_with_timeout(out_h, JOIN_AFTER_KILL_MS);
                        let _ = join_with_timeout(err_h, JOIN_AFTER_KILL_MS);
                        return Err(NurError::Tool(format!(
                            "command idle for {idle_for}ms with no output (process tree killed). \
                             Likely waiting for interactive input or stuck. Do not retry the \
                             identical command - switch to a dedicated tool or a non-interactive form."
                        )));
                    }
                }
                thread::sleep(Duration::from_millis(30));
            }
            Err(e) => {
                kill_tree(&mut child);
                let _ = join_with_timeout(out_h, JOIN_AFTER_KILL_MS);
                let _ = join_with_timeout(err_h, JOIN_AFTER_KILL_MS);
                return Err(NurError::Tool(format!("command wait failed: {e}")));
            }
        }
    };

    let stdout_bytes = out_h.join().unwrap_or_default();
    let stderr_bytes = err_h.join().unwrap_or_default();
    let stdout = String::from_utf8_lossy(&stdout_bytes);
    let stderr = String::from_utf8_lossy(&stderr_bytes);
    let code = status.code().unwrap_or(-1);

    let mut out = format!("shell: {label}\nexit_code: {code}\n");
    if !stdout.is_empty() {
        out.push_str("stdout:\n");
        out.push_str(&truncate(&stdout, 80_000));
        out.push('\n');
    }
    if !stderr.is_empty() {
        out.push_str("stderr:\n");
        out.push_str(&truncate(&stderr, 40_000));
        out.push('\n');
    }
    if stdout.is_empty() && stderr.is_empty() {
        out.push_str("(no output)\n");
    }
    if code != 0 {
        out.push_str(
            "note: non-zero exit - do NOT retry this identical command. \
             Read stderr, then switch approach (dedicated tool or different flags).\n",
        );
    }
    if kind == ShellKind::Cmd {
        out.push_str(
            "note: shell is cmd.exe — use Windows syntax (dir, type, findstr). \
             Install Git Bash or set NUR_SHELL for real bash.\n",
        );
    }
    Ok(out)
}

fn truncate(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut cut = max_bytes.min(s.len());
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…\n[truncated {} bytes]", &s[..cut], s.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_bounds_timeout() {
        assert_eq!(clamp_timeout_ms(0), 1_000);
        assert_eq!(clamp_timeout_ms(30_000), 30_000);
        assert_eq!(clamp_timeout_ms(999_999), MAX_TIMEOUT_MS);
    }
}

//! Honest shell selection — never claim "bash" when running cmd.exe.

use crate::error::{MuseError, Result};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn read_all(pipe: Option<impl Read>) -> Vec<u8> {
    // Cap at 2MB per stream to prevent memory blow-up from cat largefile etc
    const CAP: u64 = 2_000_000;
    let mut buf = Vec::new();
    if let Some(p) = pipe {
        let mut limited = p.take(CAP);
        let _ = limited.read_to_end(&mut buf);
    }
    buf
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
    if let Ok(p) = std::env::var("META_SHELL").or_else(|_| std::env::var("MUSE_SHELL")) {
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
                label: format!("META_SHELL={p}"),
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
        return ShellBackend {
            kind: ShellKind::Cmd,
            program: PathBuf::from("cmd.exe"),
            label: "cmd.exe".into(),
        };
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

pub fn run_in_shell(
    backend: &ShellBackend,
    command: &str,
    cwd: &Path,
    timeout_ms: u64,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<String> {
    let kind = backend.kind;
    let label = backend.label.clone();

    let mut cmd = Command::new(&backend.program);
    cmd.current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
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
        .map_err(|e| MuseError::Tool(format!("command failed to start: {e}")))?;

    // Drain pipes on threads so a chatty child can't deadlock on a full pipe.
    let out_pipe = child.stdout.take();
    let err_pipe = child.stderr.take();
    let out_h = thread::spawn(move || read_all(out_pipe));
    let err_h = thread::spawn(move || read_all(err_pipe));

    // Poll for exit; on deadline or user cancel, kill the whole process tree
    // so Esc/timeouts never leave orphaned shells running.
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if cancel.is_cancelled() {
                    kill_tree(&mut child);
                    let _ = out_h.join();
                    let _ = err_h.join();
                    return Err(MuseError::Tool(
                        "command cancelled by user (process tree killed)".into(),
                    ));
                }
                if Instant::now() >= deadline {
                    kill_tree(&mut child);
                    let _ = out_h.join();
                    let _ = err_h.join();
                    return Err(MuseError::Tool(format!(
                        "command timed out after {timeout_ms}ms (process tree killed)"
                    )));
                }
                thread::sleep(Duration::from_millis(30));
            }
            Err(e) => {
                kill_tree(&mut child);
                return Err(MuseError::Tool(format!("command wait failed: {e}")));
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
    if kind == ShellKind::Cmd {
        out.push_str(
            "note: shell is cmd.exe — use Windows syntax (dir, type, findstr). \
             Install Git Bash or set META_SHELL for real bash.\n",
        );
    }
    Ok(out)
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…\n[truncated {} chars]", &s[..max], s.len())
    }
}

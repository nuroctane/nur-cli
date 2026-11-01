//! OS-level sandbox for the `bash` tool.
//!
//! Approval gates *whether* a command runs; this decides *what it can reach*
//! when it does. Linux uses bubblewrap (bwrap) when it is on PATH: a
//! read-only bind of the whole system, a writable /tmp, the workspace
//! writable only in `workspace-write` mode, and no network in `read-only`
//! mode. macOS uses Seatbelt (`sandbox-exec`) with a generated profile.
//! Windows has no per-process file/network jail that is small enough to ship
//! this cycle, so it stays gated-but-unsandboxed and says so.
//!
//! Failure philosophy matches sgrep: a missing sandbox binary DEGRADES to an
//! unsandboxed run with a one-line note on stderr, never an error. CI has no
//! bwrap, and an agent whose every shell call failed would be useless.

use std::path::Path;

/// `[sandbox] mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// No sandbox; the child inherits the user's privileges (today's behavior).
    Off,
    /// Filesystem read-only except the workspace and a private /tmp; network off.
    ReadOnly,
    /// Workspace writable, /tmp writable, network off.
    WorkspaceWrite,
}

impl Mode {
    pub fn parse(s: &str) -> Mode {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "false" => Mode::Off,
            "workspace-write" | "workspace_write" | "write" => Mode::WorkspaceWrite,
            _ => Mode::ReadOnly,
        }
    }

    /// The default: read-only on Linux/macOS, off where there is no jail.
    pub fn default_for_host() -> Mode {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            Mode::ReadOnly
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            Mode::Off
        }
    }
}

/// The command actually executed: either the shell as-is, or a sandbox
/// wrapper around it.
pub struct Wrapped {
    /// argv for the wrapper; empty means "no wrapper, run normally".
    pub argv: Vec<String>,
    /// One-line note for the tool result when the sandbox degraded.
    pub note: Option<String>,
}

impl Wrapped {
    pub fn none() -> Wrapped {
        Wrapped {
            argv: Vec::new(),
            note: None,
        }
    }

    pub fn is_wrapped(&self) -> bool {
        !self.argv.is_empty()
    }
}

/// Which shell command to run the user's command with, per host backend.
fn shell_words() -> Vec<String> {
    #[cfg(windows)]
    {
        vec!["cmd.exe".into(), "/C".into()]
    }
    #[cfg(not(windows))]
    {
        vec!["/bin/sh".into(), "-c".into()]
    }
}

/// Wrap `command` for `mode` in `cwd`. `enabled` is `[sandbox] enabled`.
pub fn wrap(command: &str, cwd: &Path, mode: Mode, enabled: bool) -> Wrapped {
    if !enabled || mode == Mode::Off {
        return Wrapped::none();
    }
    #[cfg(target_os = "linux")]
    {
        return wrap_linux(command, cwd, mode);
    }
    #[cfg(target_os = "macos")]
    {
        return wrap_macos(command, cwd, mode);
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (command, cwd, mode);
        Wrapped {
            argv: Vec::new(),
            note: Some(
                "[sandbox] note: no OS sandbox on this platform; the command ran with your \
                 privileges (approval-gated)."
                    .into(),
            ),
        }
    }
}

#[cfg(target_os = "linux")]
fn wrap_linux(command: &str, cwd: &Path, mode: Mode) -> Wrapped {
    let Some(bwrap) = which("bwrap") else {
        return Wrapped {
            argv: Vec::new(),
            note: Some(
                "[sandbox] note: bwrap not found, command ran unsandboxed (install \
                 bubblewrap to enable the read-only jail)."
                    .into(),
            ),
        };
    };
    let mut argv = vec![
        bwrap,
        "--die-with-parent".into(),
        "--unshare-pid".into(),
        "--dev".into(),
        "/dev".into(),
        "--proc".into(),
        "/proc".into(),
        "--tmpfs".into(),
        "/tmp".into(),
    ];
    if mode == Mode::WorkspaceWrite {
        argv.extend(["--bind".into(), cwd.display().to_string(), cwd.display().to_string()]);
        argv.extend(["--ro-bind".into(), "/".into(), "/".into()]);
    } else {
        argv.extend(["--ro-bind".into(), "/".into(), "/".into()]);
    }
    // --unshare-net only when it does not break local sockets a build needs;
    // network off is the point of read-only, so keep it.
    argv.push("--unshare-net".into());
    argv.extend(["--chdir".into(), cwd.display().to_string()]);
    argv.extend(shell_words());
    argv.push(command.to_string());
    Wrapped { argv, note: None }
}

#[cfg(target_os = "macos")]
fn wrap_macos(command: &str, cwd: &Path, mode: Mode) -> Wrapped {
    let Some(sandbox_exec) = which("sandbox-exec") else {
        return Wrapped {
            argv: Vec::new(),
            note: Some(
                "[sandbox] note: sandbox-exec missing, command ran unsandboxed.".into(),
            ),
        };
    };
    // Seatbelt profile: read everywhere, write only in the workspace (in
    // workspace-write) and /tmp, no network at all.
    let workspace = cwd.display().to_string().replace('"', "\\\"");
    let write_rule = if mode == Mode::WorkspaceWrite {
        format!("(allow file-write* (subpath \"{workspace}\"))")
    } else {
        String::new()
    };
    let profile = format!(
        "(version 1)\n(allow default)\n(deny network*)\n(deny file-write*)\n\
         (allow file-write* (subpath \"/tmp\") (subpath \"/private/tmp\"))\n{write_rule}\n"
    );
    let mut argv = vec![
        sandbox_exec,
        "-p".into(),
        profile,
    ];
    argv.extend(shell_words());
    argv.push(command.to_string());
    Wrapped { argv, note: None }
}

/// PATH probe shared by both wrappers.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn which(program: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(program);
        if candidate.is_file() {
            return Some(candidate.display().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_parsing_is_fail_closed_to_read_only() {
        assert_eq!(Mode::parse("off"), Mode::Off);
        assert_eq!(Mode::parse("Workspace-Write"), Mode::WorkspaceWrite);
        assert_eq!(Mode::parse("bogus"), Mode::ReadOnly);
    }

    #[test]
    fn disabled_or_off_never_wraps() {
        let cwd = Path::new(".");
        assert!(!wrap("ls", cwd, Mode::ReadOnly, false).is_wrapped());
        assert!(!wrap("ls", cwd, Mode::Off, true).is_wrapped());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_degrades_with_a_note() {
        let w = wrap("dir", Path::new("."), Mode::ReadOnly, true);
        assert!(!w.is_wrapped());
        assert!(w.note.unwrap().contains("no OS sandbox"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_without_bwrap_degrades_instead_of_failing() {
        // CI has no bwrap: the contract is a note, never an error.
        let w = wrap("ls", Path::new("."), Mode::ReadOnly, true);
        if !w.is_wrapped() {
            assert!(w.note.is_some());
        }
    }
}

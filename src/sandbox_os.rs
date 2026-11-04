//! OS-level sandbox for the `bash` tool.
//!
//! Approval gates *whether* a command runs; this decides *what it can reach*
//! when it does. Linux uses bubblewrap (bwrap) when it is on PATH: a
//! read-only bind of the whole system, a writable /tmp, the workspace
//! writable only in `workspace-write` mode, plus Landlock and seccomp to
//! restrict writes and network access. macOS uses Seatbelt (`sandbox-exec`).
//! Windows defaults to `off` because it has no OS sandbox here.
//!
//! An enabled sandbox fails closed when its backend is unavailable. Users can
//! explicitly choose `off` when they intend to run commands without isolation.

use std::ffi::OsString;
use std::path::Path;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::path::PathBuf;

/// `[sandbox] mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// No sandbox; the child inherits the user's privileges (today's behavior).
    Off,
    /// Filesystem read-only with a private writable /tmp; network off.
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
}

/// The command actually executed: either the shell as-is, or a sandbox
/// wrapper around it.
pub struct Wrapped {
    /// argv for the wrapper; empty means "no wrapper, run normally".
    pub argv: Vec<OsString>,
    /// Why an enabled sandbox could not be prepared.
    pub failure: Option<String>,
}

impl Wrapped {
    pub fn none() -> Wrapped {
        Wrapped {
            argv: Vec::new(),
            failure: None,
        }
    }

    #[cfg(test)]
    pub fn is_wrapped(&self) -> bool {
        !self.argv.is_empty()
    }
}

/// Wrap the selected shell and its arguments for `mode` in `cwd`.
pub fn wrap(shell: &[OsString], cwd: &Path, mode: Mode, enabled: bool) -> Wrapped {
    if !enabled || mode == Mode::Off {
        return Wrapped::none();
    }
    #[cfg(target_os = "linux")]
    {
        wrap_linux(shell, cwd, mode)
    }
    #[cfg(target_os = "macos")]
    {
        wrap_macos(shell, cwd, mode)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (shell, cwd, mode);
        Wrapped {
            argv: Vec::new(),
            failure: Some("[sandbox] no OS sandbox is available on this platform; set sandbox.mode = \"off\" to run without it".into()),
        }
    }
}

#[cfg(target_os = "linux")]
fn wrap_linux(shell: &[OsString], cwd: &Path, mode: Mode) -> Wrapped {
    let Some(bwrap) = which("bwrap", cwd) else {
        return Wrapped {
            argv: Vec::new(),
            failure: Some("[sandbox] bubblewrap is unavailable; install it or explicitly set sandbox.mode = \"off\"".into()),
        };
    };
    let helper = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            return Wrapped {
                argv: Vec::new(),
                failure: Some(format!(
                    "[sandbox] cannot locate the sandbox helper: {error}"
                )),
            }
        }
    };
    let mut argv: Vec<OsString> = vec![
        bwrap.into_os_string(),
        "--die-with-parent".into(),
        "--new-session".into(),
        "--unshare-pid".into(),
        // The root must be mounted first so the later, more specific mounts
        // remain visible inside the sandbox.
        "--ro-bind".into(),
        "/".into(),
        "/".into(),
        "--dev".into(),
        "/dev".into(),
        "--proc".into(),
        "/proc".into(),
        "--tmpfs".into(),
        "/tmp".into(),
    ];
    // A workspace under /tmp would otherwise disappear behind the private
    // tmpfs. Re-mount it after /tmp with the access requested by the mode.
    argv.extend([
        if mode == Mode::WorkspaceWrite {
            "--bind"
        } else {
            "--ro-bind"
        }
        .into(),
        cwd.as_os_str().to_os_string(),
        cwd.as_os_str().to_os_string(),
    ]);
    // Both sandbox modes disable network access.
    argv.push("--unshare-net".into());
    argv.extend(["--chdir".into(), cwd.as_os_str().to_os_string()]);
    argv.extend([
        "--ro-bind".into(),
        helper.as_os_str().to_os_string(),
        helper.as_os_str().to_os_string(),
    ]);
    argv.extend([
        helper.as_os_str().to_os_string(),
        "--nur-sandbox-internal".into(),
        if mode == Mode::WorkspaceWrite {
            "workspace-write"
        } else {
            "read-only"
        }
        .into(),
        cwd.as_os_str().to_os_string(),
    ]);
    argv.extend_from_slice(shell);
    Wrapped {
        argv,
        failure: None,
    }
}

#[cfg(target_os = "macos")]
fn wrap_macos(shell: &[OsString], cwd: &Path, mode: Mode) -> Wrapped {
    let Some(sandbox_exec) = which("sandbox-exec", cwd) else {
        return Wrapped {
            argv: Vec::new(),
            failure: Some("[sandbox] sandbox-exec is unavailable; explicitly set sandbox.mode = \"off\" to run without it".into()),
        };
    };
    // Seatbelt profile: read everywhere, write only in the workspace (in
    // workspace-write) and /tmp, no network at all.
    let Some(workspace) = cwd.to_str() else {
        return Wrapped {
            argv: Vec::new(),
            failure: Some("[sandbox] workspace path is not valid UTF-8".into()),
        };
    };
    if workspace.chars().any(char::is_control) {
        return Wrapped {
            argv: Vec::new(),
            failure: Some("[sandbox] workspace path contains control characters".into()),
        };
    }
    let workspace = workspace.replace('\\', "\\\\").replace('"', "\\\"");
    let write_rule = if mode == Mode::WorkspaceWrite {
        format!("(allow file-write* (subpath \"{workspace}\"))")
    } else {
        String::new()
    };
    let profile = format!(
        "(version 1)\n(allow default)\n(deny network*)\n(deny file-write*)\n\
         (allow file-write* (subpath \"/tmp\") (subpath \"/private/tmp\"))\n{write_rule}\n"
    );
    let mut argv = vec![sandbox_exec.into_os_string(), "-p".into(), profile.into()];
    argv.extend_from_slice(shell);
    Wrapped {
        argv,
        failure: None,
    }
}

/// PATH probe shared by both wrappers.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn which(program: &str, workspace: &Path) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let workspace = workspace.canonicalize().ok();
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(program);
        if let Ok(resolved) = candidate.canonicalize() {
            if resolved.is_file()
                && !workspace
                    .as_ref()
                    .is_some_and(|root| resolved.starts_with(root))
            {
                return Some(resolved);
            }
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
        assert!(!wrap(&[], cwd, Mode::ReadOnly, false).is_wrapped());
        assert!(!wrap(&[], cwd, Mode::Off, true).is_wrapped());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_rejects_an_unavailable_requested_sandbox() {
        let w = wrap(&[], Path::new("."), Mode::ReadOnly, true);
        assert!(!w.is_wrapped());
        assert!(w.failure.unwrap().contains("no OS sandbox"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_without_bwrap_reports_failure() {
        let w = wrap(&[], Path::new("."), Mode::ReadOnly, true);
        if !w.is_wrapped() {
            assert!(w.failure.is_some());
        }
    }
}

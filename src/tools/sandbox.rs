//! Workspace sandbox: keep tools inside the session cwd and refuse drive-root walks.

use crate::error::{MuseError, Result};
use std::path::{Component, Path, PathBuf};

/// Paths that must never be used as a workspace root (no sandbox).
pub fn is_dangerous_workspace(path: &Path) -> bool {
    let Ok(canon) = path.canonicalize() else {
        // If it doesn't exist yet, still check raw form.
        return is_filesystem_root(path);
    };
    is_filesystem_root(&canon)
}

fn is_filesystem_root(path: &Path) -> bool {
    let s = path.to_string_lossy();
    // Unix /
    if path.parent().is_none() {
        return true;
    }
    // Windows drive roots: C:\ C:/
    #[cfg(windows)]
    {
        let t = s.trim_end_matches(['\\', '/']);
        if t.len() == 2 && t.as_bytes()[1] == b':' {
            return true;
        }
        // \\?\C:\
        if t.len() >= 6 && t.starts_with(r"\\?") {
            let rest = &t[4..];
            if rest.len() == 2 && rest.as_bytes()[1] == b':' {
                return true;
            }
        }
    }
    false
}

/// Resolve `path` against `cwd` and ensure the result stays under `cwd`.
pub fn resolve_in_workspace(cwd: &Path, path: &str) -> Result<PathBuf> {
    let cwd = cwd
        .canonicalize()
        .unwrap_or_else(|_| cwd.to_path_buf());
    let joined = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        cwd.join(path)
    };

    // Normalize .. without requiring the path to exist.
    let normalized = normalize_path(&joined);
    let cwd_norm = normalize_path(&cwd);

    if !path_is_within(&normalized, &cwd_norm) {
        return Err(MuseError::Tool(format!(
            "path escapes workspace sandbox\n  path: {}\n  workspace: {}\n\
             Refuse: tools only operate under the session cwd.",
            normalized.display(),
            cwd_norm.display()
        )));
    }
    Ok(normalized)
}

fn path_is_within(path: &Path, root: &Path) -> bool {
    let p = path.components().collect::<Vec<_>>();
    let r = root.components().collect::<Vec<_>>();
    if p.len() < r.len() {
        return false;
    }
    p.iter().zip(r.iter()).all(|(a, b)| a == b)
}

/// Lexical normalization (does not touch the filesystem).
pub fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::Prefix(p) => out.push(p.as_os_str()),
            Component::RootDir => out.push(c.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(s) => out.push(s),
        }
    }
    out
}

/// Prefer a git work tree root when the user launched from a dangerous root.
pub fn prefer_git_root(cwd: &Path) -> PathBuf {
    if !is_dangerous_workspace(cwd) {
        return cwd.to_path_buf();
    }
    // Walk up from process cwd first (often more specific than passed cwd)
    let start = std::env::current_dir().unwrap_or_else(|_| cwd.to_path_buf());
    let mut dir = start.as_path();
    loop {
        if dir.join(".git").exists() {
            return dir.to_path_buf();
        }
        match dir.parent() {
            Some(p) if p != dir => dir = p,
            _ => break,
        }
    }
    cwd.to_path_buf()
}

/// Pick a safe workspace when the user starts from `C:\` / `/`.
/// Order: explicit --cwd (caller) → git root → META_CWD → last session → Laboratory → home.
/// Returns (path, optional human reason for the adjustment).
pub fn resolve_safe_workspace(requested: &Path, explicit_cwd: bool) -> Result<(PathBuf, Option<String>)> {
    // Explicit --cwd that is already safe.
    if !is_dangerous_workspace(requested) {
        let p = requested
            .canonicalize()
            .unwrap_or_else(|_| requested.to_path_buf());
        return Ok((p, None));
    }

    // Explicit --cwd C:\ is still refused (user forced an unsafe root).
    if explicit_cwd {
        return Err(MuseError::Other(format!(
            "refusing --cwd at filesystem root ({})\n\
             Pick a project folder, e.g.\n\
               meta --cwd C:\\Users\\{}\\laboratory\\meta-cli\n\
             or:  cd path\\to\\repo  then  meta",
            requested.display(),
            std::env::var("USERNAME")
                .or_else(|_| std::env::var("USER"))
                .unwrap_or_else(|_| "you".into())
        )));
    }

    // 1) Nearest git root from process cwd
    let git = prefer_git_root(requested);
    if !is_dangerous_workspace(&git) {
        return Ok((
            git.canonicalize().unwrap_or(git),
            Some("using nearest git repository (not drive root)".into()),
        ));
    }

    // 2) Env override
    for var in ["META_CWD", "MUSE_CWD"] {
        if let Ok(v) = std::env::var(var) {
            let p = PathBuf::from(v.trim());
            if p.is_dir() && !is_dangerous_workspace(&p) {
                return Ok((
                    p.canonicalize().unwrap_or(p),
                    Some(format!("using {var}")),
                ));
            }
        }
    }

    // 3) Last session cwd from ~/.muse/latest_session.json
    if let Some(p) = last_session_cwd() {
        if p.is_dir() && !is_dangerous_workspace(&p) {
            return Ok((
                p.canonicalize().unwrap_or(p),
                Some("using last session workspace".into()),
            ));
        }
    }

    // 4) Common project folders under home
    if let Some(home) = dirs::home_dir() {
        for name in ["laboratory", "Laboratory", "projects", "Projects", "code", "src", "dev"] {
            let p = home.join(name);
            if p.is_dir() && !is_dangerous_workspace(&p) {
                // Prefer meta-cli inside laboratory if present
                let meta_cli = p.join("meta-cli");
                if meta_cli.is_dir() {
                    return Ok((
                        meta_cli.canonicalize().unwrap_or(meta_cli),
                        Some(format!("using ~\\{name}\\meta-cli (started from drive root)")),
                    ));
                }
                return Ok((
                    p.canonicalize().unwrap_or(p),
                    Some(format!("using ~\\{name} (started from drive root)")),
                ));
            }
        }
        // 5) Home directory itself
        if !is_dangerous_workspace(&home) {
            return Ok((
                home.canonicalize().unwrap_or(home),
                Some("using home directory (started from drive root)".into()),
            ));
        }
    }

    Err(MuseError::Other(format!(
        "refusing to run with workspace at filesystem root ({})\n\
         cd into a project first, or:\n\
           meta --cwd C:\\Users\\you\\path\\to\\repo\n\
         Optional default: set user env META_CWD to your usual project folder.",
        requested.display()
    )))
}

fn last_session_cwd() -> Option<PathBuf> {
    let path = crate::config::muse_home().join("latest_session.json");
    let text = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let s = v.get("cwd")?.as_str()?;
    Some(PathBuf::from(s))
}

pub fn sandbox_warning(cwd: &Path) -> Option<String> {
    if is_dangerous_workspace(cwd) {
        Some(format!(
            "workspace is filesystem root ({}) — refuse wide globs; \
             start meta from a project directory or set --cwd / META_CWD",
            cwd.display()
        ))
    } else {
        None
    }
}

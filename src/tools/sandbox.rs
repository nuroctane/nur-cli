//! Workspace sandbox: keep tools inside the session cwd and refuse drive-root walks.

use crate::error::{MuseError, Result};
use std::path::{Component, Path, PathBuf};

/// Windows `canonicalize` returns verbatim (`\\?\C:\...`) paths; strip the
/// prefix so comparisons against non-verbatim roots are consistent.
fn strip_verbatim(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if s.starts_with(r"\\?\") {
        PathBuf::from(s[4..].to_string())
    } else {
        path.to_path_buf()
    }
}

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
///
/// - Lexical `..` collapse before the check
/// - Windows paths compared case-insensitively
/// - If the path exists, canonicalize to block symlink escapes outside cwd
/// - If it doesn't exist, canonicalize the deepest existing ancestor so a
///   junction/symlink parent can't smuggle a write outside the workspace
pub fn resolve_in_workspace(cwd: &Path, path: &str) -> Result<PathBuf> {
    let cwd_canon = cwd
        .canonicalize()
        .map(|p| strip_verbatim(&p))
        .unwrap_or_else(|_| normalize_path(cwd));
    let joined = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        cwd_canon.join(path)
    };

    // Lexical normalize first (handles .. without requiring existence).
    let lexical = normalize_path(&joined);
    let cwd_norm = normalize_path(&cwd_canon);

    if !path_is_within(&lexical, &cwd_norm) {
        return Err(escape_err(&lexical, &cwd_norm));
    }

    // If it exists, re-check via canonicalize (symlink / junction escape).
    if lexical.exists() {
        let real = lexical
            .canonicalize()
            .map(|p| strip_verbatim(&p))
            .map_err(|e| MuseError::Tool(format!("resolve {}: {e}", lexical.display())))?;
        if !path_is_within(&real, &cwd_canon) && !path_is_within(&real, &cwd_norm) {
            return Err(escape_err(&real, &cwd_norm));
        }
        return Ok(real);
    }

    // Doesn't exist yet (e.g. write_file target): canonicalize the deepest
    // existing ancestor to catch junction/symlink parents.
    let mut anc = lexical.as_path();
    while let Some(parent) = anc.parent() {
        if parent.exists() {
            let real_parent = parent
                .canonicalize()
                .map(|p| strip_verbatim(&p))
                .map_err(|e| MuseError::Tool(format!("resolve {}: {e}", parent.display())))?;
            if !path_is_within(&real_parent, &cwd_canon) && !path_is_within(&real_parent, &cwd_norm)
            {
                return Err(escape_err(&real_parent, &cwd_norm));
            }
            break;
        }
        anc = parent;
    }

    Ok(lexical)
}

fn escape_err(path: &Path, root: &Path) -> MuseError {
    MuseError::Tool(format!(
        "path escapes workspace sandbox\n  path: {}\n  workspace: {}\n\
         Refuse: tools only operate under the session cwd.",
        path.display(),
        root.display()
    ))
}

fn path_is_within(path: &Path, root: &Path) -> bool {
    let p: Vec<String> = path.components().map(|c| component_key(&c)).collect();
    let r: Vec<String> = root.components().map(|c| component_key(&c)).collect();
    if p.len() < r.len() {
        return false;
    }
    p.iter().zip(r.iter()).all(|(a, b)| a == b)
}

fn component_key(c: &Component<'_>) -> String {
    let s = c.as_os_str().to_string_lossy().into_owned();
    #[cfg(windows)]
    {
        // Compare case-insensitively on Windows.
        return s.to_ascii_lowercase();
    }
    #[cfg(not(windows))]
    {
        s
    }
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
pub fn resolve_safe_workspace(
    requested: &Path,
    explicit_cwd: bool,
) -> Result<(PathBuf, Option<String>)> {
    // Explicit --cwd that is already safe.
    if !is_dangerous_workspace(requested) {
        let p = requested
            .canonicalize()
            .map(|p| strip_verbatim(&p))
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
            git.canonicalize().map(|p| strip_verbatim(&p)).unwrap_or(git),
            Some("using nearest git repository (not drive root)".into()),
        ));
    }

    // 2) Env override
    for var in ["META_CWD", "MUSE_CWD"] {
        if let Ok(v) = std::env::var(var) {
            let p = PathBuf::from(v.trim());
            if p.is_dir() && !is_dangerous_workspace(&p) {
                return Ok((
                    p.canonicalize().map(|c| strip_verbatim(&c)).unwrap_or(p),
                    Some(format!("using {var}")),
                ));
            }
        }
    }

    // 3) Last session cwd from ~/.meta/latest_session.json
    if let Some(p) = last_session_cwd() {
        if p.is_dir() && !is_dangerous_workspace(&p) {
            return Ok((
                p.canonicalize().map(|c| strip_verbatim(&c)).unwrap_or(p),
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
                        meta_cli
                            .canonicalize()
                            .map(|c| strip_verbatim(&c))
                            .unwrap_or(meta_cli),
                        Some(format!("using ~\\{name}\\meta-cli (started from drive root)")),
                    ));
                }
                return Ok((
                    p.canonicalize().map(|c| strip_verbatim(&c)).unwrap_or(p),
                    Some(format!("using ~\\{name} (started from drive root)")),
                ));
            }
        }
        // 5) Home directory itself
        if !is_dangerous_workspace(&home) {
            return Ok((
                home.canonicalize()
                    .map(|c| strip_verbatim(&c))
                    .unwrap_or(home),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn rejects_parent_escape() {
        let root = std::env::temp_dir().join("meta-sandbox-test-root");
        let _ = fs::create_dir_all(&root);
        let root = root.canonicalize().unwrap();
        let err = resolve_in_workspace(&root, "../outside.txt");
        assert!(err.is_err(), "expected escape to fail");
    }

    #[test]
    fn allows_nested() {
        let root = std::env::temp_dir().join("meta-sandbox-test-root2");
        let _ = fs::create_dir_all(root.join("a"));
        let root = root.canonicalize().unwrap();
        let p = resolve_in_workspace(&root, "a/b.txt").unwrap();
        assert!(p.starts_with(strip_verbatim(&root)) || path_is_within(&p, &root));
    }

    #[test]
    fn allows_nonexistent_nested_target() {
        let root = std::env::temp_dir().join("meta-sandbox-test-root3");
        let _ = fs::create_dir_all(&root);
        let root = root.canonicalize().unwrap();
        assert!(resolve_in_workspace(&root, "new_dir/new_file.txt").is_ok());
    }

    #[test]
    fn rejects_absolute_outside() {
        let root = std::env::temp_dir().join("meta-sandbox-test-root4");
        let _ = fs::create_dir_all(&root);
        let root = root.canonicalize().unwrap();
        #[cfg(windows)]
        let outside = r"C:\Windows\System32\drivers\etc\hosts";
        #[cfg(not(windows))]
        let outside = "/etc/hosts";
        assert!(resolve_in_workspace(&root, outside).is_err());
    }

    #[test]
    fn detects_drive_root() {
        assert!(is_dangerous_workspace(Path::new(r"C:\")));
        assert!(is_dangerous_workspace(Path::new("/")));
    }
}

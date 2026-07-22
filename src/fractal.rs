//! fractal integration — hierarchical recursive loops in git worktrees
//!
//! Fractal (https://github.com/plasma-ai/fractal) spawns child nodes in isolated
//! git worktrees, each running its own autonomous agent loop. You are one node;
//! spawn a child to own a subtask that is well-defined, separable, large enough
//! for its own iteration cycle, and able to be run in parallel.
//!
//! This module mirrors the penecho/t3code pattern:
//! - Probe binary on PATH with Windows extension handling
//! - Detect fractal repo (`.fractal` folder at repo root, `.worktrees`)
//! - Version check via `fractal --version`
//! - Repo root discovery via walking up for `.git`
//! - Doctor checks: binary, git, fractal folder, worktrees

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{MuseError, Result};

/// fractal data dir at repo root (from fractal/constants.py)
pub const FRACTAL_FOLDER: &str = ".fractal";
pub const WORKTREES_FOLDER: &str = ".worktrees";

/// Reuse robust find_on_path from penecho (handles .exe/.cmd/.bat/.js wrappers on Windows)
pub fn find_on_path(name: &str) -> Option<PathBuf> {
    crate::penecho::find_on_path(name)
}

#[derive(Debug, Clone)]
pub struct FractalProbe {
    pub binary: Option<PathBuf>,
    pub version: Option<String>,
    pub repo_root: Option<PathBuf>,
    pub fractal_dir: Option<PathBuf>,
    pub fractal_dir_exists: bool,
    pub worktrees_exist: bool,
    pub is_git_repo: bool,
    pub is_fractal_repo: bool,
}

fn run_cmd_output(bin: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new(bin).args(args).output().ok()?;
    if !out.status.success() {
        if out.stdout.is_empty() && out.stderr.is_empty() {
            return None;
        }
    }
    let s = if !out.stdout.is_empty() {
        String::from_utf8_lossy(&out.stdout).to_string()
    } else {
        String::from_utf8_lossy(&out.stderr).to_string()
    };
    let trimmed = s.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn find_git_root(mut cwd: &Path) -> Option<PathBuf> {
    loop {
        if cwd.join(".git").exists() {
            return Some(cwd.to_path_buf());
        }
        match cwd.parent() {
            Some(p) => cwd = p,
            None => break,
        }
    }
    None
}

fn find_fractal_root(mut cwd: &Path) -> Option<PathBuf> {
    loop {
        if cwd.join(FRACTAL_FOLDER).exists() {
            return Some(cwd.to_path_buf());
        }
        match cwd.parent() {
            Some(p) => cwd = p,
            None => break,
        }
    }
    None
}

fn is_valid_node_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 64 {
        return false;
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return false;
    }
    name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[allow(dead_code)]
pub fn is_valid_fractal_node_name(name: &str) -> bool {
    is_valid_node_name(name)
}

/// Probe fractal installation and current repo state.
pub fn probe_at(cwd: &Path) -> FractalProbe {
    let binary = find_on_path("fractal");
    let version = binary.as_ref().and_then(|b| {
        run_cmd_output(b, &["--version"]).or_else(|| {
            run_cmd_output(b, &["--help"]).and_then(|h| {
                let first = h.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
                if first.is_empty() {
                    None
                } else {
                    Some(first)
                }
            })
        })
    });

    let repo_root = find_git_root(cwd);
    let is_git_repo = repo_root.is_some();
    let fractal_root = find_fractal_root(cwd);
    let fractal_dir = fractal_root
        .as_ref()
        .map(|r| r.join(FRACTAL_FOLDER))
        .or_else(|| repo_root.as_ref().map(|r| r.join(FRACTAL_FOLDER)));
    let fractal_dir_exists = fractal_dir.as_ref().map(|p| p.exists()).unwrap_or(false);
    let worktrees_exist = fractal_root
        .as_ref()
        .or(repo_root.as_ref())
        .map(|r| {
            let wt = r.join(WORKTREES_FOLDER);
            wt.exists() || r.join(FRACTAL_FOLDER).join(WORKTREES_FOLDER).exists()
        })
        .unwrap_or(false);
    let is_fractal_repo = fractal_dir_exists;

    FractalProbe {
        binary,
        version,
        repo_root,
        fractal_dir,
        fractal_dir_exists,
        worktrees_exist,
        is_git_repo,
        is_fractal_repo,
    }
}

#[allow(dead_code)]
pub fn probe() -> FractalProbe {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    probe_at(&cwd)
}

/// Run fractal CLI and capture output.
pub fn run_fractal_args(cwd: &Path, args: &[String]) -> Result<String> {
    let bin = find_on_path("fractal").ok_or_else(|| {
        MuseError::Other(
            "fractal binary not found on PATH. Install via pipx: `pipx install fractal` or `pip install fractal` (requires Python 3.10+). Repo: https://github.com/plasma-ai/fractal"
                .into(),
        )
    })?;
    let mut cmd = Command::new(&bin);
    cmd.args(args);
    cmd.current_dir(cwd);
    let out = cmd
        .output()
        .map_err(|e| MuseError::Other(format!("failed to spawn fractal: {e}")))?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let combined = if stdout.trim().is_empty() {
        stderr
    } else if stderr.trim().is_empty() {
        stdout
    } else {
        format!("{stdout}\n{stderr}")
    };
    if out.status.success() {
        Ok(combined)
    } else {
        Err(MuseError::Other(combined))
    }
}

/// Doctor report.
#[derive(Debug, Clone)]
pub struct Doctor {
    pub binary_present: bool,
    pub version: Option<String>,
    pub git_repo: bool,
    pub fractal_repo: bool,
    pub fractal_dir: Option<PathBuf>,
    pub worktrees_present: bool,
    pub python_present: bool,
}

pub fn doctor_at(cwd: &Path) -> Doctor {
    let probe = probe_at(cwd);
    let python = find_on_path("python")
        .or_else(|| find_on_path("python3"))
        .is_some();
    Doctor {
        binary_present: probe.binary.is_some(),
        version: probe.version,
        git_repo: probe.is_git_repo,
        fractal_repo: probe.is_fractal_repo,
        fractal_dir: probe.fractal_dir,
        worktrees_present: probe.worktrees_exist,
        python_present: python,
    }
}

#[allow(dead_code)]
pub fn doctor() -> Doctor {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    doctor_at(&cwd)
}

/// List nodes via `fractal node list` if available; fallback to reading .fractal dir.
pub fn list_nodes(cwd: &Path) -> Result<String> {
    match run_fractal_args(cwd, &["node".into(), "list".into()]) {
        Ok(s) => Ok(s),
        Err(e) => {
            let probe = probe_at(cwd);
            if let Some(root) = probe.repo_root.or_else(|| {
                probe_at(cwd)
                    .fractal_dir
                    .as_ref()
                    .and_then(|d| d.parent().map(|p| p.to_path_buf()))
            }) {
                let candidates = [
                    root.join(WORKTREES_FOLDER),
                    root.join(FRACTAL_FOLDER).join(WORKTREES_FOLDER),
                ];
                for wt in candidates {
                    if wt.exists() {
                        let entries = fs::read_dir(&wt)
                            .map(|rd| {
                                let mut names = Vec::new();
                                for ent in rd.flatten() {
                                    if ent.path().is_dir() {
                                        if let Some(n) = ent.file_name().to_str() {
                                            names.push(n.to_string());
                                        }
                                    }
                                }
                                names.join("\n")
                            })
                            .unwrap_or_else(|_| "(cannot read worktrees dir)".into());
                        return Ok(format!(
                            "(fractal CLI failed: {e})\nFallback worktrees in {}:\n{entries}",
                            wt.display()
                        ));
                    }
                }
            }
            Err(e)
        }
    }
}

/// Open node dir path for a given node name.
pub fn node_path(cwd: &Path, node_name: &str) -> Option<PathBuf> {
    if !is_valid_node_name(node_name) {
        return None;
    }
    let probe = probe_at(cwd);
    let root = probe.repo_root?;
    let candidates = [
        root.join(WORKTREES_FOLDER).join(node_name),
        root.join(FRACTAL_FOLDER).join(WORKTREES_FOLDER).join(node_name),
    ];
    for wt_path in candidates {
        if wt_path.exists() {
            return Some(wt_path);
        }
    }
    None
}

/// Check if this repo can init fractal.
#[allow(dead_code)]
pub fn can_init(cwd: &Path) -> bool {
    let probe = probe_at(cwd);
    probe.is_git_repo && !probe.is_fractal_repo
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_does_not_panic() {
        let p = probe();
        let _ = format!("{:?}", p);
    }

    #[test]
    fn doctor_does_not_panic() {
        let d = doctor();
        let _ = format!("{:?}", d);
    }

    #[test]
    fn valid_name_rejects_traversal() {
        assert!(!is_valid_node_name("../../etc"));
        assert!(!is_valid_node_name("a/b"));
        assert!(!is_valid_node_name("a\\b"));
        assert!(is_valid_node_name("my_node_123"));
        assert!(!is_valid_node_name("my-node")); // dash not allowed
    }
}

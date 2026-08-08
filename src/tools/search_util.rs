//! Fast path helpers for grep/glob — mirror what Claude Code / Cursor / Codex do:
//! prefer **ripgrep** when available; otherwise walk with hard excludes + size caps.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Directories almost never useful for agent code search (always skip, even without .gitignore).
pub const HARD_EXCLUDES: &[&str] = &[
    "node_modules",
    "target",
    ".git",
    "dist",
    "build",
    "out",
    ".next",
    ".nuxt",
    ".turbo",
    ".cache",
    "coverage",
    "__pycache__",
    ".venv",
    "venv",
    ".tox",
    ".mypy_cache",
    ".pytest_cache",
    ".idea",
    ".vscode",
    "vendor",
    "Pods",
    ".gradle",
    "bin",
    "obj",
    ".terraform",
    "storybook-static",
    ".parcel-cache",
    ".yarn",
    "bower_components",
];

/// Skip files larger than this when using the pure-Rust fallback (bytes).
pub const MAX_FILE_BYTES: u64 = 1_048_576; // 1 MiB

/// Soft wall-clock budget so a runaway walk cannot hang a turn.
pub const SEARCH_BUDGET: Duration = Duration::from_secs(8);

pub fn is_hard_excluded(path: &Path) -> bool {
    for c in path.components() {
        if let Some(name) = c.as_os_str().to_str() {
            if HARD_EXCLUDES.contains(&name) {
                return true;
            }
        }
    }
    false
}

/// Locate `rg` on PATH (Windows includes .exe via Command).
pub fn find_rg() -> Option<PathBuf> {
    which_bin("rg")
}

/// Threads for rg parallelism (cap so we don't oversubscribe tiny searches).
fn rg_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(2, 16)
}

fn which_bin(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
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

/// Run ripgrep; return stdout lines or None if rg missing/failed hard.
/// Optimized for speed: no config, explicit globs, parallel (-j), deterministic
/// sorted output, per-file -m cap plus a global line cap, and a filesize bound.
pub fn rg_grep(
    cwd: &Path,
    pattern: &str,
    search_path: &Path,
    case_insensitive: bool,
    max_matches: usize,
) -> Option<String> {
    let rg = find_rg()?;
    let j = rg_threads().to_string();
    let mut cmd = Command::new(rg);
    cmd.current_dir(cwd)
        .arg("--no-config")
        .arg("--line-number")
        .arg("--no-heading")
        .arg("--color=never")
        .arg("--hidden")
        .arg("-j")
        .arg(&j)
        .arg("--sort")
        .arg("path")
        .arg("--glob=!.git/*")
        .arg("--max-filesize=1M")
        .arg("-m")
        .arg(max_matches.to_string());
    if case_insensitive {
        cmd.arg("-i");
    }
    // Keep the noise out even without a gitignore (mirror HARD_EXCLUDES).
    cmd.arg("--glob=!node_modules/**")
        .arg("--glob=!target/**")
        .arg("--glob=!dist/**")
        .arg("--glob=!build/**")
        .arg("--glob=!.next/**")
        .arg("--glob=!__pycache__/**")
        .arg("--glob=!.venv/**")
        .arg("--glob=!vendor/**");
    cmd.arg("-e").arg(pattern).arg(search_path);

    let output = cmd.output().ok()?;
    if !output.status.success() && output.stdout.is_empty() {
        // exit 1 = no matches for rg — treat as empty success
        if output.status.code() == Some(1) {
            return Some(String::new());
        }
        return None;
    }
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    // Enforce global max lines
    let lines: Vec<&str> = text.lines().take(max_matches).collect();
    text = lines.join("\n");
    Some(text)
}

/// List files with ripgrep --files + glob.
pub fn rg_files(cwd: &Path, pattern: &str, search_path: &Path, max: usize) -> Option<String> {
    let rg = find_rg()?;
    let mut cmd = Command::new(rg);
    cmd.current_dir(cwd)
        .arg("--files")
        .arg("--color=never")
        .arg("--hidden")
        .arg("--glob=!.git/*")
        .arg("--glob=!node_modules/**")
        .arg("--glob=!target/**")
        .arg("--glob=!dist/**")
        .arg("--glob=!build/**")
        .arg("--glob=!.next/**")
        .arg("--glob=!__pycache__/**")
        .arg("--glob=!.venv/**")
        .arg("--glob=!vendor/**");

    // Map simple patterns to rg globs
    let g = if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
        pattern.to_string()
    } else if pattern.starts_with('.') {
        format!("*{pattern}")
    } else {
        format!("*{pattern}*")
    };
    cmd.arg("--glob").arg(&g);
    cmd.arg(search_path);

    let output = cmd.output().ok()?;
    if !output.status.success() && output.stdout.is_empty() {
        if output.status.code() == Some(1) {
            return Some(String::new());
        }
        return None;
    }
    let mut lines: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .take(max)
        .map(|s| s.replace('\\', "/"))
        .collect();
    lines.sort();
    Some(lines.join("\n"))
}

pub fn walk_builder(root: &Path) -> ignore::WalkBuilder {
    let mut b = ignore::WalkBuilder::new(root);
    b.hidden(true) // respect hidden; .gitignore still applied
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .ignore(true)
        .parents(true)
        .require_git(false)
        .follow_links(false)
        .max_filesize(Some(MAX_FILE_BYTES));
    // Parallel walk when ignore supports it
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(8);
    b.threads(threads);
    // Custom filter for hard excludes (node_modules etc. even if not gitignored)
    b.filter_entry(|entry| {
        let name = entry.file_name().to_string_lossy();
        if HARD_EXCLUDES.iter().any(|e| *e == name) {
            return false;
        }
        true
    });
    b
}

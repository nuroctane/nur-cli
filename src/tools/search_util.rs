//! Fast path helpers for grep/glob — mirror what Claude Code / Cursor / Codex do:
//! prefer **ripgrep** when available; otherwise walk with hard excludes + size caps.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

/// Directories almost never useful for agent code search (always skip, even
/// without .gitignore). `src/bin` is a real source directory in Rust and Go
/// projects, so `bin` is deliberately absent.
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

/// Longest line ripgrep prints before eliding (minified bundles, lockfiles).
const RG_MAX_COLUMNS: &str = "500";

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

/// Locate `rg`: `NUR_RG_BIN`, then PATH, then the usual install locations
/// (cargo, scoop, winget, choco, Homebrew, distro). Probed once per process -
/// grep and glob run on every turn and a PATH scan per call adds up.
pub fn find_rg() -> Option<PathBuf> {
    static RG: OnceLock<Option<PathBuf>> = OnceLock::new();
    RG.get_or_init(|| {
        if let Some(p) = std::env::var_os("NUR_RG_BIN") {
            let p = PathBuf::from(p);
            if p.is_file() {
                return Some(p);
            }
        }
        if let Some(p) = which_bin("rg") {
            return Some(p);
        }
        rg_candidates().into_iter().find(|p| p.is_file())
    })
    .clone()
}

fn rg_candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    let home = dirs::home_dir();
    #[cfg(windows)]
    {
        if let Some(h) = &home {
            v.push(h.join(".cargo").join("bin").join("rg.exe"));
            v.push(h.join("scoop").join("shims").join("rg.exe"));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            v.push(
                PathBuf::from(local)
                    .join("Microsoft")
                    .join("WinGet")
                    .join("Links")
                    .join("rg.exe"),
            );
        }
        v.push(PathBuf::from(r"C:\ProgramData\chocolatey\bin\rg.exe"));
        v.push(PathBuf::from(r"C:\Program Files\Git\usr\bin\rg.exe"));
    }
    #[cfg(not(windows))]
    {
        if let Some(h) = &home {
            v.push(h.join(".cargo").join("bin").join("rg"));
            v.push(h.join(".local").join("bin").join("rg"));
        }
        v.push(PathBuf::from("/opt/homebrew/bin/rg"));
        v.push(PathBuf::from("/usr/local/bin/rg"));
        v.push(PathBuf::from("/usr/bin/rg"));
    }
    v
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

/// `--glob=!**/<dir>/**` for every hard exclude, so nested copies
/// (`packages/x/node_modules`) are skipped too, not only the top-level one.
fn exclude_globs() -> impl Iterator<Item = String> {
    HARD_EXCLUDES.iter().map(|d| format!("--glob=!**/{d}/**"))
}

/// Path handed to rg: relative to `cwd` when the root sits inside it (shorter
/// output, no drive-letter colons to confuse the `path:line:` split), else
/// absolute.
fn rg_search_arg(cwd: &Path, root: &Path) -> PathBuf {
    match root.strip_prefix(cwd) {
        Ok(rel) if rel.as_os_str().is_empty() => PathBuf::from("."),
        Ok(rel) => rel.to_path_buf(),
        Err(_) => root.to_path_buf(),
    }
}

/// How a content search runs; shared by the rg and fallback paths.
#[derive(Debug, Clone, Default)]
pub struct GrepOptions {
    pub case_insensitive: bool,
    /// Treat the pattern as a literal string, not a regex.
    pub fixed_strings: bool,
    /// Only search files matching this glob (`*.rs`, `src/**/*.ts`).
    pub glob: Option<String>,
    /// ripgrep file type (`rust`, `py`, `js`); fallback maps common ones.
    pub file_type: Option<String>,
    /// Lines of context around each match.
    pub context: usize,
    /// Let `.` and anchors span lines (ripgrep only).
    pub multiline: bool,
    pub max_matches: usize,
}

/// Where ripgrep stands for one call.
pub enum RgOutcome {
    /// rg ran; matched lines (possibly empty) plus whether the cap was hit.
    Hits { text: String, truncated: bool },
    /// rg is not installed or could not be launched: use the fallback.
    Unavailable,
    /// rg ran and rejected the request (bad regex, unknown type, …).
    Error(String),
}

/// Run ripgrep. Fast by construction: no config file, hard excludes as globs,
/// parallel (`-j`), per-file `-m` plus a global line cap, a filesize bound and
/// a column cap. Output is sorted here instead of with `--sort path`, which
/// forces rg single-threaded.
pub fn rg_grep(cwd: &Path, pattern: &str, search_path: &Path, opts: &GrepOptions) -> RgOutcome {
    let Some(rg) = find_rg() else {
        return RgOutcome::Unavailable;
    };
    let max_matches = opts.max_matches.max(1);
    let mut cmd = Command::new(rg);
    cmd.current_dir(cwd)
        .arg("--no-config")
        .arg("--line-number")
        .arg("--no-heading")
        .arg("--color=never")
        .arg("--hidden")
        .arg("-j")
        .arg(rg_threads().to_string())
        .arg("--max-filesize=1M")
        .arg("--max-columns")
        .arg(RG_MAX_COLUMNS)
        .arg("--max-columns-preview")
        .arg("-m")
        .arg(max_matches.to_string());
    if opts.case_insensitive {
        cmd.arg("-i");
    }
    if opts.fixed_strings {
        cmd.arg("-F");
    }
    if opts.multiline {
        cmd.arg("-U").arg("--multiline-dotall");
    }
    if opts.context > 0 {
        cmd.arg("-C").arg(opts.context.to_string());
        // Context blocks cannot be re-sorted after the fact; let rg order them.
        cmd.arg("--sort").arg("path");
    }
    if let Some(t) = opts.file_type.as_deref().filter(|t| !t.trim().is_empty()) {
        cmd.arg("--type").arg(t.trim());
    }
    cmd.args(exclude_globs());
    if let Some(g) = opts.glob.as_deref().filter(|g| !g.trim().is_empty()) {
        cmd.arg("--glob").arg(g.trim());
    }
    cmd.arg("-e")
        .arg(pattern)
        .arg(rg_search_arg(cwd, search_path));

    let output = match cmd.output() {
        Ok(o) => o,
        Err(_) => return RgOutcome::Unavailable,
    };
    if !output.status.success() && output.stdout.is_empty() {
        // exit 1 = no matches for rg — treat as empty success
        if output.status.code() == Some(1) {
            return RgOutcome::Hits {
                text: String::new(),
                truncated: false,
            };
        }
        let err = String::from_utf8_lossy(&output.stderr);
        let first = err
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("ripgrep failed");
        return RgOutcome::Error(first.to_string());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines: Vec<String> = text.lines().map(normalize_rg_line).collect();
    let truncated = if opts.context == 0 {
        // Deterministic order without `--sort path`: by file, then line.
        lines.sort_by_key(|line| rg_sort_key(line));
        let truncated = lines.len() > max_matches;
        lines.truncate(max_matches);
        truncated
    } else {
        // With context the cap bounds match lines only, so the model keeps
        // whole blocks; `--` separators and context lines are not matches.
        let mut kept = Vec::new();
        let mut matches = 0usize;
        let mut truncated = false;
        for l in lines {
            if is_rg_match_line(&l) {
                if matches >= max_matches {
                    truncated = true;
                    break;
                }
                matches += 1;
            }
            kept.push(l);
        }
        lines = kept;
        truncated
    };
    RgOutcome::Hits {
        text: lines.join("\n"),
        truncated,
    }
}

/// `path:line:text` (match) or `path-line-text` (context) → sort key.
fn rg_sort_key(line: &str) -> (String, u64) {
    match split_rg_line(line) {
        Some((path, sep, rest)) => {
            let n = rest
                .split(sep)
                .next()
                .and_then(|d| d.parse::<u64>().ok())
                .unwrap_or(0);
            (path.to_string(), n)
        }
        None => (line.to_string(), 0),
    }
}

pub(crate) fn is_rg_match_line(line: &str) -> bool {
    matches!(split_rg_line(line), Some((_, ':', _)))
}

/// Split an rg output line into (path, separator, remainder). The path runs
/// to the first `:` or `-` that is followed by digits and the same separator;
/// a Windows drive prefix (`C:\`) is skipped so its colon is not the split.
fn split_rg_line(line: &str) -> Option<(&str, char, &str)> {
    let bytes = line.as_bytes();
    let start = if bytes.len() > 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
    {
        3
    } else {
        0
    };
    // Match lines (`:`) first: file names contain `-digits-` far more often
    // than `:digits:`, so a hyphenated name must not be split as context.
    for &sep in b":-" {
        let mut i = start;
        while i < bytes.len() {
            if bytes[i] == sep {
                let mut j = i + 1;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if j > i + 1 && j < bytes.len() && bytes[j] == sep {
                    return Some((&line[..i], sep as char, &line[i + 1..]));
                }
            }
            i += 1;
        }
    }
    None
}

/// Forward slashes in the path part only; the matched text is left alone.
fn normalize_rg_line(line: &str) -> String {
    match split_rg_line(line) {
        Some((path, sep, rest)) if path.contains('\\') => {
            format!("{}{sep}{rest}", path.replace('\\', "/"))
        }
        _ => line.to_string(),
    }
}

/// List files with ripgrep --files + glob. `Unavailable` and `Error` as for
/// [`rg_grep`]; `Hits.text` is one path per line, sorted, forward slashes.
pub fn rg_files(cwd: &Path, pattern: &str, search_path: &Path, max: usize) -> RgOutcome {
    let Some(rg) = find_rg() else {
        return RgOutcome::Unavailable;
    };
    let mut cmd = Command::new(rg);
    cmd.current_dir(cwd)
        .arg("--files")
        .arg("--color=never")
        .arg("--hidden")
        .arg("-j")
        .arg(rg_threads().to_string());
    cmd.args(exclude_globs());
    cmd.arg("--glob").arg(glob_for_pattern(pattern));
    cmd.arg(rg_search_arg(cwd, search_path));

    let output = match cmd.output() {
        Ok(o) => o,
        Err(_) => return RgOutcome::Unavailable,
    };
    if !output.status.success() && output.stdout.is_empty() {
        if output.status.code() == Some(1) {
            return RgOutcome::Hits {
                text: String::new(),
                truncated: false,
            };
        }
        let err = String::from_utf8_lossy(&output.stderr);
        let first = err
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("ripgrep failed");
        return RgOutcome::Error(first.to_string());
    }
    let mut lines: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|s| s.replace('\\', "/"))
        .collect();
    lines.sort();
    let truncated = lines.len() > max;
    lines.truncate(max);
    RgOutcome::Hits {
        text: lines.join("\n"),
        truncated,
    }
}

/// Map the loose patterns models send to one rg glob:
/// `*.rs` / `**/mod.rs` → as is; `.env` → `*.env`; `src/` → everything under a
/// `src` directory; `mod.rs` → that file name anywhere; `main` → `*main*`.
pub fn glob_for_pattern(pattern: &str) -> String {
    let p = pattern.trim().replace('\\', "/");
    if p.contains('*') || p.contains('?') || p.contains('[') || p.contains('{') {
        return p;
    }
    if let Some(dir) = p.strip_suffix('/') {
        let dir = dir.trim_start_matches("./");
        return format!("**/{dir}/**");
    }
    if p.starts_with('.') && !p.contains('/') {
        return format!("*{p}");
    }
    if p.contains('/') {
        return format!("**/{}", p.trim_start_matches("./"));
    }
    if p.contains('.') {
        // Looks like a file name: match it exactly, anywhere.
        return format!("**/{p}");
    }
    format!("*{p}*")
}

pub fn walk_builder(root: &Path) -> ignore::WalkBuilder {
    let mut b = ignore::WalkBuilder::new(root);
    // Hidden files are searched (like the rg path: `--hidden`); `.git` and the
    // other junk directories are dropped by the filter below.
    b.hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .ignore(true)
        .parents(true)
        .require_git(false)
        .follow_links(false)
        .max_filesize(Some(MAX_FILE_BYTES));
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

/// Fallback-side file filter for `glob` / `type`, built on the same globset
/// engine ripgrep uses. `None` when nothing was requested.
pub fn fallback_file_filter(
    root: &Path,
    glob: Option<&str>,
    file_type: Option<&str>,
) -> std::result::Result<Option<ignore::overrides::Override>, String> {
    let mut ov = ignore::overrides::OverrideBuilder::new(root);
    let mut any = false;
    if let Some(g) = glob.map(str::trim).filter(|g| !g.is_empty()) {
        ov.add(g).map_err(|e| format!("invalid glob `{g}`: {e}"))?;
        any = true;
    }
    if let Some(t) = file_type.map(str::trim).filter(|t| !t.is_empty()) {
        let exts = extensions_for_type(t)
            .ok_or_else(|| format!("unknown file type `{t}` (fallback knows the common ones; install ripgrep for the full list)"))?;
        for e in exts {
            ov.add(&format!("*.{e}"))
                .map_err(|e2| format!("type `{t}`: {e2}"))?;
        }
        any = true;
    }
    if !any {
        return Ok(None);
    }
    ov.build().map(Some).map_err(|e| e.to_string())
}

/// Common ripgrep type names → extensions, for the pure-Rust fallback.
fn extensions_for_type(t: &str) -> Option<&'static [&'static str]> {
    Some(match t {
        "rust" | "rs" => &["rs"],
        "py" | "python" => &["py", "pyi"],
        "js" => &["js", "mjs", "cjs", "jsx"],
        "ts" => &["ts", "tsx", "mts", "cts"],
        "go" => &["go"],
        "java" => &["java"],
        "kotlin" | "kt" => &["kt", "kts"],
        "c" => &["c", "h"],
        "cpp" | "cxx" => &["cpp", "cc", "cxx", "hpp", "hh", "hxx", "h"],
        "cs" | "csharp" => &["cs"],
        "rb" | "ruby" => &["rb"],
        "php" => &["php"],
        "swift" => &["swift"],
        "sh" | "shell" => &["sh", "bash", "zsh"],
        "ps1" | "powershell" => &["ps1", "psm1", "psd1"],
        "md" | "markdown" => &["md", "markdown"],
        "json" => &["json"],
        "yaml" | "yml" => &["yaml", "yml"],
        "toml" => &["toml"],
        "html" => &["html", "htm"],
        "css" => &["css", "scss", "sass", "less"],
        "sql" => &["sql"],
        "txt" => &["txt"],
        "xml" => &["xml"],
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Failure modes: a drive-letter colon taken as the separator, a `-` in a
    // file name taken as a context separator, backslashes rewritten inside the
    // matched text.
    #[test]
    fn rg_lines_split_on_the_real_separator() {
        assert_eq!(
            split_rg_line(r"C:\src\a-b.rs:12:let x = 1;"),
            Some((r"C:\src\a-b.rs", ':', "12:let x = 1;"))
        );
        assert_eq!(
            split_rg_line("src/my-mod.rs-7-// context"),
            Some(("src/my-mod.rs", '-', "7-// context"))
        );
        assert_eq!(split_rg_line("--"), None);
        assert_eq!(
            normalize_rg_line(r#"src\tools\x.rs:3:path = r"C:\\tmp";"#),
            r#"src/tools/x.rs:3:path = r"C:\\tmp";"#
        );
        assert!(is_rg_match_line("a.rs:1:x"));
        assert!(!is_rg_match_line("a.rs-1-x"));
    }

    #[test]
    fn sort_key_orders_by_path_then_numeric_line() {
        let mut v = vec!["b.rs:2:x", "a.rs:10:x", "a.rs:9:x"];
        v.sort_by_key(|line| rg_sort_key(line));
        assert_eq!(v, vec!["a.rs:9:x", "a.rs:10:x", "b.rs:2:x"]);
    }

    // Failure modes: `src/` matching any path containing "src", a plain file
    // name widened to a substring, an explicit glob rewritten.
    #[test]
    fn loose_patterns_become_precise_globs() {
        assert_eq!(glob_for_pattern("src/"), "**/src/**");
        assert_eq!(glob_for_pattern("./src/"), "**/src/**");
        assert_eq!(glob_for_pattern("mod.rs"), "**/mod.rs");
        assert_eq!(glob_for_pattern("src/tools/mod.rs"), "**/src/tools/mod.rs");
        assert_eq!(glob_for_pattern(".env"), "*.env");
        assert_eq!(glob_for_pattern("main"), "*main*");
        assert_eq!(glob_for_pattern("**/*.rs"), "**/*.rs");
        assert_eq!(glob_for_pattern(r"src\**\*.ts"), "src/**/*.ts");
    }

    #[test]
    fn nested_junk_directories_are_excluded() {
        let globs: Vec<String> = exclude_globs().collect();
        assert!(globs.iter().any(|g| g == "--glob=!**/node_modules/**"));
        assert!(globs.iter().any(|g| g == "--glob=!**/.git/**"));
        assert!(!globs.iter().any(|g| g.contains("/bin/")));
    }

    #[test]
    fn search_arg_is_relative_inside_cwd() {
        let cwd = Path::new("/w/proj");
        assert_eq!(rg_search_arg(cwd, Path::new("/w/proj")), PathBuf::from("."));
        assert_eq!(
            rg_search_arg(cwd, Path::new("/w/proj/src")),
            PathBuf::from("src")
        );
        assert_eq!(
            rg_search_arg(cwd, Path::new("/other")),
            PathBuf::from("/other")
        );
    }
}

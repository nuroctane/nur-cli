//! Fast path helpers for grep/glob — mirror what Claude Code / Cursor / Codex do:
//! prefer **ripgrep** when available; otherwise walk with hard excludes + size caps.

use super::ToolContext;
use std::borrow::Cow;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

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

/// Skip files larger than this when searching contents (bytes). Applies to
/// grep only: a name lookup must still list `package-lock.json`.
pub const MAX_FILE_BYTES: u64 = 1_048_576; // 1 MiB

/// Soft wall-clock budget so a runaway walk or rg run cannot hang a turn.
pub const SEARCH_BUDGET: Duration = Duration::from_secs(8);

/// Longest result line (bytes) before eliding; a minified bundle must not
/// turn one hit into a megabyte of tool output. rg gets the same number.
pub const MAX_LINE_BYTES: usize = 500;

/// Most rg stdout read before the run is cut short and marked truncated.
const RG_OUTPUT_BUDGET_BYTES: usize = 8 * 1024 * 1024;

/// Unsorted rg output is read a few times past the match cap, then sorted
/// and cut, so the kept set is the alphabetically first one whenever the
/// total fits the window. `--sort path` would make it exact but forces rg
/// single-threaded, which costs more than it saves on content search.
const RG_OVERREAD_FACTOR: usize = 8;
const RG_OVERREAD_MIN_LINES: usize = 1000;

const RG_STDERR_CAP: u64 = 64 * 1024;

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
    let home = crate::config::home_dir();
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

/// `--glob=!**/<dir>/` for every hard exclude, so nested copies
/// (`packages/x/node_modules`) are skipped too, not only the top-level one.
/// The trailing slash is what prunes: gitignore semantics turn `**/d/**` into
/// `**/d/**/*`, which matches the files but never the directory, so rg would
/// still descend into `.git` and `target` and only discard what it found.
fn exclude_globs() -> impl Iterator<Item = String> {
    HARD_EXCLUDES.iter().map(|d| format!("--glob=!**/{d}/"))
}

fn strip_verbatim(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(p) => PathBuf::from(p),
        None => path.to_path_buf(),
    }
}

/// `cwd` as the sandbox spells resolved paths (canonical, no `\\?\`), so a
/// resolved root can be stripped against it.
fn canonical_cwd(cwd: &Path) -> PathBuf {
    cwd.canonicalize()
        .map(|p| strip_verbatim(&p))
        .unwrap_or_else(|_| cwd.to_path_buf())
}

/// A path for tool output: relative to `cwd` when inside it, forward
/// slashes, no `./`.
pub fn display_path(cwd: &Path, path: &Path) -> String {
    let rel = path
        .strip_prefix(canonical_cwd(cwd))
        .or_else(|_| path.strip_prefix(cwd))
        .unwrap_or(path);
    let s = rel.to_string_lossy().replace('\\', "/");
    match s.strip_prefix("./") {
        Some(rest) => rest.to_string(),
        None => s,
    }
}

/// Prefix put back in front of the paths rg prints when it runs inside
/// `dir`: empty for the cwd itself, `src/` for a subdirectory, the absolute
/// directory otherwise.
fn display_prefix(cwd: &Path, dir: &Path) -> String {
    let s = display_path(cwd, dir);
    if s.is_empty() || s == "." {
        String::new()
    } else {
        format!("{}/", s.trim_end_matches('/'))
    }
}

/// Where rg runs and what it searches: `(process cwd, path argument, display
/// prefix)`. rg runs *inside* the search root so `--glob` is anchored where
/// the fallback anchors its override (the `path` argument), not at the
/// process cwd; the prefix restores cwd-relative output paths.
fn rg_target(cwd: &Path, root: &Path) -> (PathBuf, PathBuf, String) {
    let (dir, target) = if root.is_file() {
        let dir = root.parent().unwrap_or(root).to_path_buf();
        let name = root.file_name().map(PathBuf::from).unwrap_or_default();
        (dir, name)
    } else {
        (root.to_path_buf(), PathBuf::from("."))
    };
    let prefix = display_prefix(cwd, &dir);
    (dir, target, prefix)
}

/// Clean up a glob a model typed: backslash separators, a `./` prefix, and
/// the directory form `src/` (which as a plain glob matches no file) become
/// what the globset engine expects. A leading `!` negation is preserved.
pub fn normalize_user_glob(glob: &str) -> String {
    let g = glob.trim();
    let (neg, body) = match g.strip_prefix('!') {
        Some(b) => ("!", b.trim()),
        None => ("", g),
    };
    let mut body = body.replace('\\', "/");
    while let Some(rest) = body.strip_prefix("./") {
        body = rest.to_string();
    }
    if let Some(dir) = body.strip_suffix('/') {
        let dir = dir.trim_end_matches('/');
        body = if dir.is_empty() {
            "**".to_string()
        } else if has_glob_meta(dir) {
            format!("{dir}/**")
        } else {
            format!("**/{dir}/**")
        };
    }
    format!("{neg}{body}")
}

fn has_glob_meta(s: &str) -> bool {
    s.contains(['*', '?', '[', '{'])
}

/// How a content search runs; shared by the rg and fallback paths.
#[derive(Debug, Clone, Default)]
pub struct GrepOptions {
    pub case_insensitive: bool,
    /// Treat the pattern as a literal string, not a regex.
    pub fixed_strings: bool,
    /// Only search files matching this glob (`*.rs`, `src/**/*.ts`),
    /// already passed through [`normalize_user_glob`].
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
    /// rg ran; matched lines (possibly empty), whether the cap was hit, and
    /// an optional note for the model (budget, cancellation, unreadable
    /// entries).
    Hits {
        text: String,
        truncated: bool,
        note: Option<String>,
    },
    /// rg is not installed or could not be launched: use the fallback.
    Unavailable,
    /// rg ran and rejected the request (bad regex, unknown type, …).
    Error(String),
}

/// Why reading rg's output ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    /// rg closed stdout on its own.
    Eof,
    /// The caller's line consumer asked to stop (result cap reached).
    Cap,
    /// [`SEARCH_BUDGET`] elapsed.
    Budget,
    /// More than [`RG_OUTPUT_BUDGET_BYTES`] arrived.
    Output,
    /// `ctx.cancel` fired.
    Cancelled,
}

struct RgRun {
    stop: Stop,
    /// `None` when the child was killed because of an early stop.
    status: Option<ExitStatus>,
    stderr: String,
}

/// Spawn rg and feed its stdout lines (line terminator removed) to `keep`
/// until it returns `false`, the output or time budget runs out, or the turn
/// is cancelled; the child is killed on every early exit. `None` when rg
/// could not be launched.
fn run_rg(
    cmd: &mut Command,
    ctx: &ToolContext,
    mut keep: impl FnMut(String) -> bool,
) -> Option<RgRun> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().ok()?;
    let stdout = child.stdout.take()?;
    let mut stderr = child.stderr.take()?;

    // A reader thread plus a bounded channel lets the loop below poll the
    // cancel token and deadline while rg is silent (a long search with few
    // hits), which a blocking read on this thread could not.
    let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(1024);
    std::thread::spawn(move || {
        let mut rd = BufReader::new(stdout);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match rd.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if buf.last() == Some(&b'\n') {
                        buf.pop();
                    }
                    if buf.last() == Some(&b'\r') {
                        buf.pop();
                    }
                    if tx.send(std::mem::take(&mut buf)).is_err() {
                        break;
                    }
                }
            }
        }
    });
    // Drain stderr fully (not only the kept head) so a chatty rg can never
    // block on a full pipe.
    let err_thread = std::thread::spawn(move || {
        let mut head = Vec::new();
        let _ = (&mut stderr).take(RG_STDERR_CAP).read_to_end(&mut head);
        let _ = std::io::copy(&mut stderr, &mut std::io::sink());
        String::from_utf8_lossy(&head).into_owned()
    });

    let deadline = Instant::now() + SEARCH_BUDGET;
    let mut bytes = 0usize;
    let stop = loop {
        if ctx.cancel.is_cancelled() {
            break Stop::Cancelled;
        }
        if Instant::now() >= deadline {
            break Stop::Budget;
        }
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(raw) => {
                bytes += raw.len() + 1;
                let line = String::from_utf8(raw)
                    .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
                if !keep(line) {
                    break Stop::Cap;
                }
                if bytes > RG_OUTPUT_BUDGET_BYTES {
                    break Stop::Output;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break Stop::Eof,
        }
    };
    drop(rx);
    let status = if stop == Stop::Eof {
        child.wait().ok()
    } else {
        let _ = child.kill();
        let _ = child.wait();
        None
    };
    let stderr = err_thread.join().unwrap_or_default();
    Some(RgRun {
        stop,
        status,
        stderr,
    })
}

/// Turn a finished run into an outcome, given the lines kept and how the
/// caller judged truncation. Exit code 2 with nothing printed is only an
/// error when rg named a real problem (`--no-messages` already silences
/// per-file read errors); otherwise it means some entry was unreadable.
fn rg_finish(run: &RgRun, text: String, truncated: bool) -> RgOutcome {
    let stderr_lines: Vec<&str> = run
        .stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take(4)
        .collect();
    let mut note = match run.stop {
        Stop::Eof | Stop::Cap => None,
        Stop::Budget => Some(format!(
            "search stopped after {}ms budget - narrow the pattern or path",
            SEARCH_BUDGET.as_millis()
        )),
        Stop::Output => {
            Some("search stopped after 8 MiB of output - narrow the pattern or path".into())
        }
        Stop::Cancelled => Some("search cancelled".into()),
    };
    if text.is_empty() {
        if let Some(status) = run.status {
            if !status.success() && status.code() != Some(1) {
                if !stderr_lines.is_empty() {
                    return RgOutcome::Error(stderr_lines.join("\n"));
                }
                note = Some("some entries could not be read (ripgrep exit code 2)".into());
            }
        }
    } else if let Some(first) = stderr_lines.first() {
        note.get_or_insert_with(|| format!("ripgrep: {first}"));
    }
    RgOutcome::Hits {
        text,
        truncated,
        note,
    }
}

/// Run ripgrep. Fast by construction: no config file, hard excludes as globs,
/// parallel (`-j`), per-file `-m` plus a global line cap, a filesize bound and
/// a column cap. Output is sorted here instead of with `--sort path`, which
/// forces rg single-threaded.
pub fn rg_grep(ctx: &ToolContext, pattern: &str, root: &Path, opts: &GrepOptions) -> RgOutcome {
    let Some(rg) = find_rg() else {
        return RgOutcome::Unavailable;
    };
    let max_matches = opts.max_matches.max(1);
    let (dir, target, prefix) = rg_target(&ctx.cwd, root);
    let mut cmd = Command::new(rg);
    cmd.current_dir(&dir)
        .arg("--no-config")
        .arg("--no-messages")
        .arg("--line-number")
        .arg("--no-heading")
        .arg("--with-filename")
        .arg("--null")
        .arg("--color=never")
        .arg("--hidden")
        .arg("-j")
        .arg(rg_threads().to_string())
        .arg("--max-filesize=1M")
        .arg("--max-columns")
        .arg(MAX_LINE_BYTES.to_string())
        .arg("--max-columns-preview")
        // One past the cap per file, so a single file holding more matches
        // than the cap still produces the `[truncated …]` trailer.
        .arg("-m")
        .arg((max_matches + 1).to_string());
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
        cmd.arg("--glob").arg(normalize_user_glob(g));
    }
    cmd.arg("-e").arg(pattern).arg("--").arg(&target);

    let mut lines: Vec<String> = Vec::new();
    let mut truncated = false;
    let run = if opts.context == 0 {
        let window = max_matches
            .saturating_mul(RG_OVERREAD_FACTOR)
            .max(RG_OVERREAD_MIN_LINES);
        run_rg(&mut cmd, ctx, |line| {
            lines.push(line);
            lines.len() < window
        })
    } else {
        // With context the cap bounds match lines only, so the model keeps
        // whole blocks; `--` separators and context lines are not matches.
        let mut matches = 0usize;
        run_rg(&mut cmd, ctx, |line| {
            if parse_rg_line(&line).is_some_and(|l| l.sep == ':') {
                if matches >= max_matches {
                    truncated = true;
                    return false;
                }
                matches += 1;
            }
            lines.push(line);
            true
        })
    };
    let Some(run) = run else {
        return RgOutcome::Unavailable;
    };
    if opts.context == 0 {
        // Deterministic order without `--sort path`: by file, then line.
        lines.sort_by_cached_key(|line| rg_sort_key(line));
        truncated = lines.len() > max_matches;
        lines.truncate(max_matches);
    }
    truncated |= !matches!(run.stop, Stop::Eof | Stop::Cap) && !lines.is_empty();
    let text = lines
        .iter()
        .map(|l| render_rg_line(l, &prefix))
        .collect::<Vec<_>>()
        .join("\n");
    rg_finish(&run, text, truncated)
}

/// One `--null --no-heading --line-number` output line, split.
struct RgLine<'a> {
    path: &'a str,
    /// `:` for a match, `-` for a context line.
    sep: char,
    line_no: u64,
    text: &'a str,
}

/// Parse `path\0N:text` / `path\0N-text`. `None` for the `--` separator and
/// anything else rg prints without a path (binary-file notices). Splitting on
/// NUL means a `12:30:45` inside the matched text can never be mistaken for
/// the separator.
fn parse_rg_line(line: &str) -> Option<RgLine<'_>> {
    let (path, rest) = line.split_once('\0')?;
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let line_no = rest[..digits].parse().ok()?;
    let mut after = rest[digits..].chars();
    let sep = after.next().filter(|c| matches!(c, ':' | '-'))?;
    Some(RgLine {
        path,
        sep,
        line_no,
        text: after.as_str(),
    })
}

/// `(path, line)` for sorting; lines without a path sort by their text.
fn rg_sort_key(line: &str) -> (String, u64) {
    match parse_rg_line(line) {
        Some(l) => (l.path.to_string(), l.line_no),
        None => (line.to_string(), 0),
    }
}

/// `path:N:text` with the path forward-slashed, `./`-stripped and prefixed;
/// the matched text is left alone. Lines without a path pass through.
fn render_rg_line(line: &str, prefix: &str) -> String {
    match parse_rg_line(line) {
        Some(l) => {
            let path = l.path.replace('\\', "/");
            let path = path.strip_prefix("./").unwrap_or(&path);
            format!("{prefix}{path}{}{}{}{}", l.sep, l.line_no, l.sep, l.text)
        }
        None => line.to_string(),
    }
}

/// Cut a result line at [`MAX_LINE_BYTES`] (on a char boundary) with a
/// marker, like rg's `--max-columns-preview`.
pub fn clip_line(line: &str) -> Cow<'_, str> {
    if line.len() <= MAX_LINE_BYTES {
        return Cow::Borrowed(line);
    }
    let mut cut = MAX_LINE_BYTES;
    while !line.is_char_boundary(cut) {
        cut -= 1;
    }
    Cow::Owned(format!(
        "{} [... {} more bytes]",
        &line[..cut],
        line.len() - cut
    ))
}

/// List files with ripgrep --files + glob. `Unavailable` and `Error` as for
/// [`rg_grep`]; `Hits.text` is one path per line, sorted, forward slashes.
/// `--sort path` costs rg its parallelism, but a sorted stream can be cut at
/// exactly `max` and the kept set is the same on every run.
pub fn rg_files(ctx: &ToolContext, pattern: &str, root: &Path, max: usize) -> RgOutcome {
    let Some(rg) = find_rg() else {
        return RgOutcome::Unavailable;
    };
    let (dir, target, prefix) = rg_target(&ctx.cwd, root);
    let mut cmd = Command::new(rg);
    cmd.current_dir(&dir)
        .arg("--files")
        .arg("--no-config")
        .arg("--no-messages")
        .arg("--color=never")
        .arg("--hidden")
        .arg("--sort")
        .arg("path");
    cmd.args(exclude_globs());
    cmd.arg("--glob").arg(glob_for_pattern(root, pattern));
    cmd.arg("--").arg(&target);

    let mut lines: Vec<String> = Vec::new();
    let mut truncated = false;
    let run = run_rg(&mut cmd, ctx, |line| {
        if lines.len() >= max {
            truncated = true;
            return false;
        }
        let p = line.replace('\\', "/");
        let p = p.strip_prefix("./").unwrap_or(&p);
        lines.push(format!("{prefix}{p}"));
        true
    });
    let Some(run) = run else {
        return RgOutcome::Unavailable;
    };
    truncated |= !matches!(run.stop, Stop::Eof | Stop::Cap) && !lines.is_empty();
    rg_finish(&run, lines.join("\n"), truncated)
}

/// Map the loose patterns models send to one glob, anchored at `root`:
/// `*.rs` / `**/mod.rs` → as is; a path naming an existing directory (`src`,
/// `src/tools/`) → everything under it; `src/` otherwise → everything under
/// any `src` directory; `.env` → `*.env`; `mod.rs` → that file name anywhere;
/// `main` → `*main*`.
pub fn glob_for_pattern(root: &Path, pattern: &str) -> String {
    let mut p = pattern.trim().replace('\\', "/");
    while let Some(rest) = p.strip_prefix("./") {
        p = rest.to_string();
    }
    if has_glob_meta(&p) {
        return p;
    }
    let bare = p.trim_end_matches('/');
    if bare.is_empty() || bare == "." {
        return "**".to_string();
    }
    let plain_relative = !Path::new(bare).is_absolute() && !bare.split('/').any(|c| c == "..");
    if plain_relative && root.join(bare).is_dir() {
        return format!("{bare}/**");
    }
    if p.ends_with('/') {
        return format!("**/{bare}/**");
    }
    if p.starts_with('.') && !p.contains('/') {
        return format!("*{p}");
    }
    if p.contains('/') || p.contains('.') {
        // A path or a file name: match it exactly, anywhere.
        return format!("**/{p}");
    }
    format!("*{p}*")
}

/// Walker for the pure-Rust fallback. `max_filesize` is the content-search
/// cap ([`MAX_FILE_BYTES`]); pass `None` when listing names. Entries come in
/// file-name order so the results, and which ones a cap keeps, are the same
/// on every run.
pub fn walk_builder(root: &Path, max_filesize: Option<u64>) -> ignore::WalkBuilder {
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
        .sort_by_file_name(|a, b| a.cmp(b))
        .max_filesize(max_filesize);
    // Hard excludes (node_modules etc. even if not gitignored). The walker
    // never applies this to the root itself, so a project that lives under a
    // directory called `build` or `out` is still searchable.
    b.filter_entry(|entry| {
        let name = entry.file_name().to_string_lossy();
        !HARD_EXCLUDES.iter().any(|e| *e == name)
    });
    b
}

/// Fallback-side file filter for `glob` / `type`, built on the same globset
/// engine ripgrep uses and anchored at `root` like rg's `--glob` is (rg runs
/// inside the root). `None` when nothing was requested.
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

    fn tmp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nur-search-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src").join("tools")).unwrap();
        dir
    }

    // Failure modes: a `12:30:45` in the text taken as the separator, a
    // hyphenated file name taken as a context line, the `--` separator parsed
    // as a hit, a Windows path printed with backslashes and `.\`.
    #[test]
    fn rg_lines_split_on_the_nul_separator() {
        let l = parse_rg_line("src/a-b.rs\x00120-at 12:30:45 nothing matched").unwrap();
        assert_eq!((l.path, l.sep, l.line_no), ("src/a-b.rs", '-', 120));
        assert_eq!(l.text, "at 12:30:45 nothing matched");
        let l = parse_rg_line("C:\\w\\x-1-y.rs\x0012:let x = 1;").unwrap();
        assert_eq!((l.path, l.sep, l.line_no), ("C:\\w\\x-1-y.rs", ':', 12));
        assert!(parse_rg_line("--").is_none());
        assert!(parse_rg_line("Binary file x matches").is_none());
        assert_eq!(
            render_rg_line(".\\src\\tools\\x.rs\x003:path = r\"C:\\\\tmp\";", ""),
            "src/tools/x.rs:3:path = r\"C:\\\\tmp\";"
        );
        assert_eq!(render_rg_line("x.rs\x007-ctx", "lib/"), "lib/x.rs-7-ctx");
        assert_eq!(render_rg_line("--", "lib/"), "--");
    }

    #[test]
    fn sort_key_orders_by_path_then_numeric_line() {
        let mut v = vec!["b.rs\x002:x", "a.rs\x0010:x", "a.rs\x009:x"];
        v.sort_by_cached_key(|line| rg_sort_key(line));
        assert_eq!(v, vec!["a.rs\x009:x", "a.rs\x0010:x", "b.rs\x002:x"]);
    }

    // Failure modes: `src/` matching any path containing "src", a plain file
    // name widened to a substring, an explicit glob rewritten, a pattern that
    // names a real directory matching nothing.
    #[test]
    fn loose_patterns_become_precise_globs() {
        let root = tmp_root("glob");
        assert_eq!(glob_for_pattern(&root, "src"), "src/**");
        assert_eq!(glob_for_pattern(&root, "src/"), "src/**");
        assert_eq!(glob_for_pattern(&root, "./src/tools/"), "src/tools/**");
        assert_eq!(glob_for_pattern(&root, "src\\tools"), "src/tools/**");
        assert_eq!(glob_for_pattern(&root, "lib/"), "**/lib/**");
        assert_eq!(glob_for_pattern(&root, "mod.rs"), "**/mod.rs");
        assert_eq!(
            glob_for_pattern(&root, "src/tools/mod.rs"),
            "**/src/tools/mod.rs"
        );
        assert_eq!(glob_for_pattern(&root, ".env"), "*.env");
        assert_eq!(glob_for_pattern(&root, "main"), "*main*");
        assert_eq!(glob_for_pattern(&root, "**/*.rs"), "**/*.rs");
        assert_eq!(glob_for_pattern(&root, r"src\**\*.ts"), "src/**/*.ts");
        assert_eq!(glob_for_pattern(&root, "."), "**");
        let _ = std::fs::remove_dir_all(root);
    }

    // Failure modes: backslashes matching nothing, `./` anchoring at a
    // non-existent component, a directory form matching no file, a negation
    // lost.
    #[test]
    fn user_globs_are_normalized() {
        assert_eq!(normalize_user_glob(r"src\tools\*.rs"), "src/tools/*.rs");
        assert_eq!(normalize_user_glob("./src/**/*.ts"), "src/**/*.ts");
        assert_eq!(normalize_user_glob("src/"), "**/src/**");
        assert_eq!(normalize_user_glob("src/*/"), "src/*/**");
        assert_eq!(normalize_user_glob("!tests/"), "!**/tests/**");
        assert_eq!(normalize_user_glob("! *.test.js"), "!*.test.js");
        assert_eq!(normalize_user_glob("*.rs"), "*.rs");
    }

    #[test]
    fn nested_junk_directories_are_pruned_not_just_filtered() {
        let globs: Vec<String> = exclude_globs().collect();
        assert!(globs.iter().any(|g| g == "--glob=!**/node_modules/"));
        assert!(globs.iter().any(|g| g == "--glob=!**/.git/"));
        assert!(!globs.iter().any(|g| g.contains("/bin/")));
    }

    #[test]
    fn display_prefix_is_relative_inside_cwd() {
        let cwd = Path::new("/w/proj");
        assert_eq!(display_prefix(cwd, Path::new("/w/proj")), "");
        assert_eq!(display_prefix(cwd, Path::new("/w/proj/src")), "src/");
        assert_eq!(
            display_prefix(cwd, Path::new("/w/proj/src/tools")),
            "src/tools/"
        );
        assert_eq!(display_prefix(cwd, Path::new("/other")), "/other/");
        assert_eq!(display_path(cwd, Path::new("/w/proj/src/a.rs")), "src/a.rs");
    }

    // Failure modes: a cut inside a multi-byte char (panic), a line exactly at
    // the cap clipped, the marker missing the dropped byte count.
    #[test]
    fn long_lines_are_clipped_on_char_boundaries() {
        let exact = "x".repeat(MAX_LINE_BYTES);
        assert_eq!(clip_line(&exact), exact.as_str());
        let mut s = "a".repeat(MAX_LINE_BYTES - 1);
        s.push('é'); // 2 bytes straddling the cap
        s.push_str("tail");
        let clipped = clip_line(&s);
        assert!(clipped.starts_with(&"a".repeat(MAX_LINE_BYTES - 1)));
        assert!(clipped.ends_with("[... 6 more bytes]"), "{clipped}");
    }
}

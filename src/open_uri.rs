//! Open http(s) URLs and local paths with the OS default handler.
//!
//! Used by the TUI (click links), browser setup, and tools (excalidraw export).

use std::path::Path;
use std::process::Command;
use unicode_width::UnicodeWidthStr;

/// Open a URL or path with the system default application (best-effort).
pub fn open(target: &str) -> Result<(), String> {
    if target.trim().is_empty() {
        return Err("empty target".into());
    }
    #[cfg(windows)]
    {
        // Empty window title is required so `start` does not treat a
        // quoted URL (with # or &) as the title.
        let status = Command::new("cmd.exe")
            .args(["/C", "start", "", target])
            .spawn()
            .map_err(|e| e.to_string())?
            .wait()
            .map_err(|e| e.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("start exited {status}"))
        }
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(target)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(target)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Open a path with the system default handler, without going through a shell
/// where the OS allows it (a shell mangles paths containing spaces or `&`).
pub fn open_path(path: &Path) -> Result<(), String> {
    file_command(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Command that opens `path` with the OS default **application**.
///
/// Split out from [`open_path`] so the exact program and arguments can be
/// asserted in a test without launching anything on the user's desktop.
fn file_command(path: &Path) -> Command {
    #[cfg(windows)]
    {
        // `start` needs the empty title first, or a quoted path is read as the
        // window title. A shell is required here: the file association lives in
        // the shell, and Explorer would only reveal the file, not open it.
        let mut c = Command::new("cmd.exe");
        c.args(["/C", "start", ""]).arg(path);
        c
    }
    #[cfg(target_os = "macos")]
    {
        let mut c = Command::new("open");
        c.arg(path);
        c
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut c = Command::new("xdg-open");
        c.arg(path);
        c
    }
}

/// Command that reveals `path` in the OS **file manager**.
fn dir_command(path: &Path) -> Command {
    #[cfg(windows)]
    {
        // Launched directly (no shell) so the path survives verbatim; Explorer
        // exits non-zero even on success, so callers must not check status.
        let mut c = Command::new("explorer.exe");
        c.arg(path);
        c
    }
    #[cfg(target_os = "macos")]
    {
        let mut c = Command::new("open");
        c.arg(path);
        c
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut c = Command::new("xdg-open");
        c.arg(path);
        c
    }
}

/// Reveal a directory in the OS file manager (Explorer / Finder / xdg-open).
pub fn open_dir(path: &Path) -> Result<(), String> {
    if !path.is_dir() {
        return Err(format!("not a directory: {}", path.display()));
    }
    dir_command(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// What a path token turned out to be on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    Dir,
    File,
}

impl PathKind {
    /// Open it the way a double-click in the file manager would.
    pub fn open(self, path: &Path) -> Result<(), String> {
        match self {
            PathKind::Dir => open_dir(path),
            PathKind::File => open_path(path),
        }
    }
}

/// How long an existence probe stays warm. Long enough that a transcript full
/// of paths costs a handful of syscalls per render, short enough that a
/// directory created a moment ago becomes clickable without a restart.
const DIR_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(10);
/// Hard cap on cached probes; cleared wholesale when exceeded (transcripts
/// reference far fewer distinct paths than this).
const DIR_CACHE_MAX: usize = 512;

static DIR_CACHE: std::sync::Mutex<
    Option<std::collections::HashMap<String, (Option<PathKind>, std::time::Instant)>>,
> = std::sync::Mutex::new(None);

/// Cached "what is this path" probe: `Some(Dir)` / `Some(File)` / `None`.
fn kind_cached(key: &str, path: &Path) -> Option<PathKind> {
    let now = std::time::Instant::now();
    {
        let guard = DIR_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cache) = guard.as_ref() {
            if let Some((hit, at)) = cache.get(key) {
                if now.duration_since(*at) < DIR_CACHE_TTL {
                    return *hit;
                }
            }
        }
    }
    let hit = match std::fs::metadata(path) {
        Ok(m) if m.is_dir() => Some(PathKind::Dir),
        Ok(_) => Some(PathKind::File),
        Err(_) => None,
    };
    let mut guard = DIR_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = guard.get_or_insert_with(std::collections::HashMap::new);
    if cache.len() >= DIR_CACHE_MAX {
        cache.clear();
    }
    cache.insert(key.to_string(), (hit, now));
    hit
}

/// Find file and directory paths in a single visual line.
///
/// Returns `(display_col_start, display_col_end, resolved_path, kind)` - end is
/// exclusive, columns match [`find_url_spans`] so mouse hit-testing works the
/// same way. Only tokens that **exist** are returned, so a clickable path is
/// always one that opens.
///
/// `base` resolves relative paths (the session's working directory).
pub fn find_path_spans(
    plain: &str,
    base: &Path,
) -> Vec<(usize, usize, std::path::PathBuf, PathKind)> {
    let mut out = Vec::new();
    // Cheap gate: a path candidate always carries a separator or `~`.
    if !plain.contains(['/', '\\', '~']) {
        return out;
    }
    let mut byte = 0usize;
    while byte < plain.len() {
        let c = plain[byte..].chars().next().unwrap_or('\0');
        // Path characters only; everything else ends the token.
        let is_path_char = !c.is_whitespace()
            && !matches!(
                c,
                '"' | '\''
                    | '`'
                    | '<'
                    | '>'
                    | '('
                    | ')'
                    | '['
                    | ']'
                    | '{'
                    | '}'
                    | '|'
                    | '*'
                    | ','
                    | ';'
            );
        if !is_path_char {
            byte += c.len_utf8();
            continue;
        }
        let start = byte;
        let mut end = byte;
        while end < plain.len() {
            let ch = plain[end..].chars().next().unwrap_or('\0');
            if ch.is_whitespace()
                || matches!(
                    ch,
                    '"' | '\''
                        | '`'
                        | '<'
                        | '>'
                        | '('
                        | ')'
                        | '['
                        | ']'
                        | '{'
                        | '}'
                        | '|'
                        | '*'
                        | ','
                        | ';'
                )
            {
                break;
            }
            end += ch.len_utf8();
        }
        byte = end.max(start + 1);
        let raw = &plain[start..end];
        let Some(token) = trim_path_token(raw) else {
            continue;
        };
        if !token.contains(['/', '\\']) {
            continue;
        }
        let resolved = resolve_path(&token, base);
        let Some(kind) = kind_cached(&resolved.to_string_lossy(), &resolved) else {
            continue;
        };
        let start_col =
            UnicodeWidthStr::width(&plain[..start + (raw.len() - raw.trim_start().len())]);
        let end_col = start_col + UnicodeWidthStr::width(token.as_str());
        out.push((start_col, end_col, resolved, kind));
    }
    out
}

/// Trim the punctuation prose wraps a path in, keeping Windows drive colons and
/// a trailing separator (which is meaningful for directories).
fn trim_path_token(raw: &str) -> Option<String> {
    let t = raw.trim();
    let t = t.trim_end_matches(['.', '!', '?', ';', ',', ')']);
    if t.is_empty() || t.starts_with("http://") || t.starts_with("https://") {
        return None;
    }
    if t.contains("://") {
        return None;
    }
    Some(t.to_string())
}

/// Resolve `~`, absolute paths, and path tokens relative to the session cwd.
fn resolve_path(token: &str, base: &Path) -> std::path::PathBuf {
    if let Some(rest) = token
        .strip_prefix("~/")
        .or_else(|| token.strip_prefix("~\\"))
    {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    let p = std::path::Path::new(token);
    let out = if p.is_absolute() {
        p.to_path_buf()
    } else if base.as_os_str().is_empty() {
        // A relative path only means something against a real base; without
        // one, do not guess (a stray stat in the process cwd would be a lie).
        p.to_path_buf()
    } else {
        base.join(p)
    };
    // A trailing separator is display, not identity: `src/tui/` and `src/tui`
    // are one directory, so they share a probe (and never resolve to "" for a
    // root like `/` or `C:\`).
    let s = out.to_string_lossy();
    let trimmed = s.trim_end_matches(['/', '\\']);
    if !trimmed.is_empty() && !trimmed.ends_with(':') {
        std::path::PathBuf::from(trimmed)
    } else {
        out
    }
}

/// Find `http://` / `https://` spans in a single visual line.
/// Returns `(display_col_start, display_col_end, url)` — end is exclusive.
///
/// Display columns count Unicode scalar values (same as ratatui span layout
/// for BMP text). URLs split across wraps are not joined (best-effort).
pub fn find_url_spans(plain: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let bytes = plain.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let rest = &plain[i..];
        // Take the earliest scheme match — prefer min offset, not https-first
        // (otherwise "http://… https://…" only finds the second URL).
        let https = rest.find("https://").map(|p| (p, 8usize));
        let http = rest.find("http://").map(|p| (p, 7usize));
        let rel = match (https, http) {
            (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
        let Some((off, scheme_len)) = rel else {
            break;
        };
        let start_byte = i + off;
        let mut end_byte = start_byte + scheme_len;
        while end_byte < bytes.len() {
            let c = plain[end_byte..].chars().next().unwrap_or('\0');
            if c.is_whitespace()
                || c == '<'
                || c == '>'
                || c == '"'
                || c == '\''
                || c == '`'
                || c == ')'
                || c == ']'
                || c == '}'
                || c == '|'
            {
                break;
            }
            // Strip trailing punctuation common in prose.
            end_byte += c.len_utf8();
        }
        // Trim trailing .,;: from the URL itself.
        while end_byte > start_byte {
            let last = plain[..end_byte].chars().last().unwrap_or('\0');
            if matches!(last, '.' | ',' | ';' | ':' | '!' | '?') {
                end_byte -= last.len_utf8();
            } else {
                break;
            }
        }
        if end_byte > start_byte + scheme_len {
            let url = plain[start_byte..end_byte].to_string();
            // DISPLAY columns, not char counts: the caller compares these
            // against a mouse column, so any wide glyph (CJK, emoji) earlier in
            // the line shifted the clickable region left of the painted link.
            let start_col = UnicodeWidthStr::width(&plain[..start_byte]);
            let end_col = start_col + UnicodeWidthStr::width(url.as_str());
            out.push((start_col, end_col, url));
        }
        i = end_byte.max(start_byte + 1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_simple_https() {
        let spans = find_url_spans("see https://excalidraw.com/#json=abc,key please");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].2, "https://excalidraw.com/#json=abc,key");
        assert_eq!(spans[0].0, 4);
    }

    /// The returned span is compared against a mouse column, so it has to be in
    /// display columns. With char counts, any wide glyph earlier in the line
    /// shifted the clickable region left of where the link was painted - so
    /// clicking the link did nothing and clicking beside it opened a browser.
    #[test]
    fn spans_are_display_columns_not_char_counts() {
        // "日本語 " is 3 wide glyphs + a space = 7 columns, but only 4 chars.
        let spans = find_url_spans("日本語 https://example.com ok");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].0, 7, "start column must clear the wide glyphs");
        assert_eq!(spans[0].1, 7 + "https://example.com".len());

        // Emoji are width 2 as well.
        let spans = find_url_spans("🌕 https://a.test");
        assert_eq!(spans[0].0, 3);

        // Pure ASCII is unchanged - char count and column agree there.
        let spans = find_url_spans("see https://a.test");
        assert_eq!(spans[0].0, 4);
    }

    #[test]
    fn strips_trailing_punct() {
        let spans = find_url_spans("go to https://example.com/path.");
        assert_eq!(spans[0].2, "https://example.com/path");
    }

    #[test]
    fn multiple_urls() {
        let spans = find_url_spans("a http://example.com/x b https://example.org/y");
        assert_eq!(spans.len(), 2, "{spans:?}");
        assert!(spans[0].2.contains("example.com"));
        assert!(spans[1].2.contains("example.org"));
    }

    /// Paths are only advertised when they exist, resolved against the session
    /// base - so a clickable path is always one that opens.
    #[test]
    fn finds_existing_files_and_directories() {
        let base = std::env::temp_dir().join(format!("nur-pathspans-{}", std::process::id()));
        let nested = base.join("src").join("tui");
        std::fs::create_dir_all(&nested).expect("temp tree");
        std::fs::write(base.join("notes.md"), "x").expect("temp file");
        std::fs::write(nested.join("markdown.rs"), "// x").expect("temp nested file");

        let plain = "  results: src/tui/ and notes.md here";
        let spans = find_path_spans(plain, &base);
        let found: Vec<(String, PathKind)> = spans
            .iter()
            .map(|(_, _, p, k)| (p.display().to_string(), *k))
            .collect();
        assert_eq!(
            found.len(),
            1,
            "only the directory carries a separator: {found:?}"
        );
        assert!(
            found[0].0.ends_with("tui") && found[0].1 == PathKind::Dir,
            "{found:?}"
        );

        // A file path is clickable too, and reports itself as a file.
        let spans = find_path_spans("see src/tui/markdown.rs", &base);
        assert_eq!(spans.len(), 1, "{spans:?}");
        assert_eq!(spans[0].3, PathKind::File, "files are clickable paths");
        assert!(spans[0].2.ends_with("markdown.rs"));

        // Something that does not exist is not a link.
        assert!(find_path_spans("no/such/path", &base).is_empty());

        // Absolute paths work, and the span is in display columns.
        let abs = format!("see {}", nested.display());
        let spans = find_path_spans(&abs, &base);
        assert_eq!(spans.len(), 1, "{spans:?}");
        assert_eq!(spans[0].0, 4, "span starts after 'see '");

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn path_tokens_ignore_urls_and_prose_punctuation() {
        assert!(trim_path_token("https://example.com/src").is_none());
        assert!(trim_path_token("git://host/src").is_none());
        assert_eq!(trim_path_token("src/tui/.").unwrap(), "src/tui/");
        assert_eq!(trim_path_token("src/tui/),").unwrap(), "src/tui/");
        // A drive root keeps its colon.
        assert_eq!(trim_path_token(r"C:\work\x").unwrap(), r"C:\work\x");
    }

    #[test]
    fn lines_without_a_separator_cost_nothing() {
        assert!(find_path_spans("just some words here", &std::env::temp_dir()).is_empty());
    }

    /// The click has to hand the OS the real path, un-shelled where possible:
    /// a directory goes to the file manager, a file to its associated app.
    #[test]
    fn open_commands_target_the_file_manager_and_the_default_app() {
        let dir = Path::new("/tmp/example dir");
        let file = Path::new("/tmp/example file.txt");
        let d = dir_command(dir);
        let f = file_command(file);
        assert!(d.get_args().any(|a| a == dir));
        assert!(f.get_args().any(|a| a == file));

        #[cfg(windows)]
        {
            assert_eq!(d.get_program(), "explorer.exe");
            #[cfg(windows)]
            assert_eq!(f.get_program(), "cmd.exe");
            // `start` reads a quoted path as the window title without the empty
            // title argument first.
            let args: Vec<String> = f
                .get_args()
                .map(|a| a.to_string_lossy().to_string())
                .collect();
            assert_eq!(args[0], "/C");
            assert_eq!(args[1], "start");
            assert_eq!(args[2], "");
        }
        #[cfg(not(windows))]
        {
            assert_eq!(d.get_program(), "open");
            assert_eq!(f.get_program(), "open");
        }
    }
}

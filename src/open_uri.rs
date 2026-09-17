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

pub fn open_path(path: &Path) -> Result<(), String> {
    open(&path.to_string_lossy())
}

/// Reveal a directory in the OS file manager (Explorer / Finder / xdg-open).
///
/// Deliberately not routed through [`open`]: `cmd /C start ""` mangles paths
/// containing spaces, and Explorer must be launched without a shell so the path
/// survives verbatim. Explorer also exits non-zero on success, so its status is
/// not treated as a failure.
pub fn open_dir(path: &Path) -> Result<(), String> {
    if !path.is_dir() {
        return Err(format!("not a directory: {}", path.display()));
    }
    #[cfg(windows)]
    {
        std::process::Command::new("explorer.exe")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
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
    Option<std::collections::HashMap<String, (bool, std::time::Instant)>>,
> = std::sync::Mutex::new(None);

/// Cached `is_dir` for a resolved path string.
fn is_dir_cached(key: &str, path: &Path) -> bool {
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
    let hit = path.is_dir();
    let mut guard = DIR_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = guard.get_or_insert_with(std::collections::HashMap::new);
    if cache.len() >= DIR_CACHE_MAX {
        cache.clear();
    }
    cache.insert(key.to_string(), (hit, now));
    hit
}

/// Find directory paths in a single visual line.
///
/// Returns `(display_col_start, display_col_end, resolved_path)` - end is
/// exclusive, columns match [`find_url_spans`] so mouse hit-testing works the
/// same way. Only tokens that actually resolve to an existing directory are
/// returned, so the colour never advertises something you cannot open.
///
/// `base` resolves relative paths (the session's working directory).
pub fn find_dir_spans(plain: &str, base: &Path) -> Vec<(usize, usize, std::path::PathBuf)> {
    let mut out = Vec::new();
    // Cheap gate: a directory candidate always carries a separator or `~`.
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
        if !is_dir_cached(&resolved.to_string_lossy(), &resolved) {
            continue;
        }
        let start_col =
            UnicodeWidthStr::width(&plain[..start + (raw.len() - raw.trim_start().len())]);
        let end_col = start_col + UnicodeWidthStr::width(token.as_str());
        out.push((start_col, end_col, resolved));
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

    /// Directories are only advertised when they exist, resolved against the
    /// session base - the colour is a promise that clicking opens something.
    #[test]
    fn finds_existing_directories_only() {
        let base = std::env::temp_dir().join(format!("nur-dirspans-{}", std::process::id()));
        let nested = base.join("src").join("tui");
        std::fs::create_dir_all(&nested).expect("temp tree");
        std::fs::write(base.join("notes.md"), "x").expect("temp file");

        let plain = "  results: src/tui/ and src tui/markdown.rs done";
        let spans = find_dir_spans(plain, &base);
        let found: Vec<String> = spans
            .iter()
            .map(|(_, _, p)| p.display().to_string())
            .collect();
        assert_eq!(found.len(), 1, "only the real directory: {found:?}");
        assert!(found[0].ends_with("tui"), "{found:?}");

        // A file of the same shape is not a directory.
        assert!(find_dir_spans("src/tui/markdown.rs", &base).is_empty());
        // Absolute paths work too, and the span is in display columns.
        let abs = format!("see {}", nested.display());
        let spans = find_dir_spans(&abs, &base);
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
        assert_eq!(trim_path_token("C:\\work\\x").unwrap(), "C:\\work\\x");
    }

    #[test]
    fn lines_without_a_separator_cost_nothing() {
        assert!(find_dir_spans("just some words here", &std::env::temp_dir()).is_empty());
    }
}

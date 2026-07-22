//! Open http(s) URLs and local paths with the OS default handler.
//!
//! Used by the TUI (click links), browser setup, and tools (excalidraw export).

use std::path::Path;
use unicode_width::UnicodeWidthStr;
use std::process::Command;

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
}

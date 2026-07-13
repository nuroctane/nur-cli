//! Cap tool results that re-enter model context. Oversized output is written
//! to disk; the model receives a short preview + path (use `read_file` for more).

use crate::config::{atomic_write, muse_home};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Default max chars of tool output kept inline in the transcript / API items.
/// Keep in sync with `config::default_tool_result_max_chars`.
pub const DEFAULT_TOOL_RESULT_MAX_CHARS: usize = 12_000;

/// How many leading characters of the full body to show in the preview.
const PREVIEW_CHARS: usize = 2_000;

pub fn tool_results_dir() -> PathBuf {
    muse_home().join("tool-results")
}

/// If `body` exceeds `max_chars`, spill the full text and return a compact
/// substitute for the model. Errors and tiny results pass through unchanged.
///
/// `max_chars == 0` disables spilling (unlimited).
pub fn maybe_spill(session_id: &str, tool: &str, body: String, max_chars: usize) -> String {
    if max_chars == 0 || body.chars().count() <= max_chars {
        return body;
    }
    // Never spill obvious auth payloads into a world-readable spill dir.
    let lower = body.to_ascii_lowercase();
    if lower.contains("api_key") && lower.contains("sk-") {
        return truncate_only(&body, max_chars);
    }

    let dir = tool_results_dir();
    let _ = std::fs::create_dir_all(&dir);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let safe_tool: String = tool
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let sid: String = session_id
        .chars()
        .take(8)
        .collect();
    let path = dir.join(format!("{sid}_{safe_tool}_{ts}.txt"));
    if atomic_write(&path, body.as_bytes()).is_err() {
        return truncate_only(&body, max_chars);
    }

    let total = body.chars().count();
    let preview: String = body.chars().take(PREVIEW_CHARS).collect();
    format!(
        "[tool result truncated — {total} chars, spilled to disk]\n\
         full path: {}\n\
         use read_file on that path if you need more than the preview below.\n\
         --- preview (first {PREVIEW_CHARS} chars) ---\n\
         {preview}\n\
         --- end preview ---",
        path.display()
    )
}

fn truncate_only(body: &str, max_chars: usize) -> String {
    let total = body.chars().count();
    let keep = max_chars.saturating_sub(80).max(200);
    let preview: String = body.chars().take(keep).collect();
    format!(
        "{preview}\n\n… [truncated {total} → {keep} chars; spill failed or disabled]"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_body_unchanged() {
        let s = "hello".to_string();
        assert_eq!(maybe_spill("abc", "bash", s.clone(), 100), s);
    }

    #[test]
    fn large_body_spills_or_truncates() {
        let big = "x".repeat(20_000);
        let out = maybe_spill("deadbeef-session", "bash", big, 1000);
        assert!(out.len() < 20_000);
        assert!(
            out.contains("truncated") || out.contains("spilled") || out.contains("preview"),
            "got: {}",
            &out[..out.len().min(200)]
        );
    }

    #[test]
    fn zero_max_disables() {
        let big = "y".repeat(5_000);
        let out = maybe_spill("id", "grep", big.clone(), 0);
        assert_eq!(out, big);
    }
}

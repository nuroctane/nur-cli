//! Cap tool results that re-enter model context. Oversized output is written
//! to disk; the model receives a short preview + path (use `read_file` for more).

use super::sensitive::body_looks_sensitive;
use crate::config::{atomic_write, nur_home};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Default max chars of tool output kept inline in the transcript / API items.
/// Keep in sync with `config::default_tool_result_max_chars`.
#[allow(dead_code)] // canonical default; config supplies the live limit
pub const DEFAULT_TOOL_RESULT_MAX_CHARS: usize = 12_000;

/// How many leading characters of the full body to show in the preview.
const PREVIEW_CHARS: usize = 2_000;

pub fn tool_results_dir() -> PathBuf {
    nur_home().join("tool-results")
}

/// True when `path` resolves under the nur tool-results spill directory.
pub fn is_under_tool_results(path: &Path) -> bool {
    let Ok(root) = tool_results_dir().canonicalize().or_else(|_| {
        let _ = std::fs::create_dir_all(tool_results_dir());
        tool_results_dir().canonicalize()
    }) else {
        return false;
    };
    let Ok(cand) = path.canonicalize() else {
        // Non-existent: lexical check against normalized root.
        let lex = crate::tools::sandbox::normalize_path(path);
        let root_n = crate::tools::sandbox::normalize_path(&root);
        return path_prefix(&lex, &root_n);
    };
    let root = strip_verbatim(&root);
    let cand = strip_verbatim(&cand);
    path_prefix(&cand, &root)
}

fn strip_verbatim(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

fn path_prefix(path: &Path, root: &Path) -> bool {
    let p: Vec<_> = path.components().collect();
    let r: Vec<_> = root.components().collect();
    if p.len() < r.len() {
        return false;
    }
    #[cfg(windows)]
    {
        p.iter()
            .zip(r.iter())
            .all(|(a, b)| a.as_os_str().eq_ignore_ascii_case(b.as_os_str()))
    }
    #[cfg(not(windows))]
    {
        p.iter().zip(r.iter()).all(|(a, b)| a == b)
    }
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
    if body_looks_sensitive(&body) {
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
    let sid: String = session_id.chars().take(8).collect();
    let uniq = uuid::Uuid::new_v4().simple().to_string();
    let path = dir.join(format!("{sid}_{safe_tool}_{ts}_{uniq}.txt"));
    if atomic_write(&path, body.as_bytes()).is_err() {
        return truncate_only(&body, max_chars);
    }

    let total = body.chars().count();
    let preview: String = body.chars().take(PREVIEW_CHARS).collect();
    format!(
        "[tool result truncated - {total} chars, spilled to disk]\n\
         full path: {}\n\
         use read_file on that absolute path (nur tool-results are readable) if you need more than the preview below.\n\
         --- preview (first {PREVIEW_CHARS} chars) ---\n\
         {preview}\n\
         --- end preview ---",
        path.display()
    )
}

fn truncate_only(body: &str, max_chars: usize) -> String {
    let total = body.chars().count();
    let keep = max_chars.saturating_sub(80).max(1).min(max_chars.max(1));
    let preview: String = body.chars().take(keep).collect();
    format!("{preview}\n\n… [truncated {total} -> {keep} chars; spill failed or disabled]")
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
        assert!(!out.contains('\u{2014}'));
    }

    #[test]
    fn sensitive_body_does_not_spill_to_disk() {
        let sid = format!("sens-{}", uuid::Uuid::new_v4().simple());
        let big = format!(
            "Authorization: Bearer {}\n{}",
            "a".repeat(40),
            "x".repeat(20_000)
        );
        let out = maybe_spill(&sid, "bash", big, 1000);
        assert!(out.contains("truncated"));
        assert!(!out.contains("spilled to disk"));
        // No spill file for this session prefix.
        let prefix = format!("{}_bash_", &sid[..8.min(sid.len())]);
        let leaked = std::fs::read_dir(tool_results_dir())
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .any(|e| e.file_name().to_string_lossy().starts_with(&prefix))
            })
            .unwrap_or(false);
        assert!(!leaked, "sensitive spill must not create a new file");
    }

    #[test]
    fn zero_max_disables() {
        let big = "y".repeat(5_000);
        let out = maybe_spill("id", "grep", big.clone(), 0);
        assert_eq!(out, big);
    }

    #[test]
    fn tool_results_dir_is_recognized() {
        let dir = tool_results_dir();
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("probe_spill_allow.txt");
        let _ = std::fs::write(&f, "hi");
        assert!(is_under_tool_results(&f));
        let _ = std::fs::remove_file(&f);
    }
}

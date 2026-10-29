//! Cap tool results that re-enter model context. Oversized output is written
//! to disk; the model receives a short preview + path (use `read_file` for more).

use super::sensitive::body_looks_sensitive;
use crate::config::{atomic_write, nur_home};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Preview budget (chars) for a spilled body, split ~70% head / ~30% tail.
const PREVIEW_CHARS: usize = 2_000;

/// Tools whose results arrive already bounded (head/tail within the budget
/// the model asked for, full text spilled). Bounding them again would hand
/// the model a short preview instead of what it requested.
pub fn tool_bounds_own_output(tool: &str) -> bool {
    tool == "bash"
}

pub fn tool_results_dir() -> PathBuf {
    nur_home().join("tool-results")
}

/// Shared, content-addressed bodies.  Context-store variables and ordinary tool
/// result spills deliberately use this one namespace: registering a result must
/// not make a second copy of the same large payload.
pub fn shared_blob_dir() -> PathBuf {
    tool_results_dir().join("blobs")
}

/// Store a non-sensitive body once, keyed by its SHA-256 digest.  The returned
/// path stays below `tool-results`, so it remains readable by the normal
/// read-file sandbox exception.
pub fn write_content_addressed_blob(body: &str) -> Result<PathBuf, String> {
    let path = content_addressed_blob_path(body);
    write_blob(&path, body)?;
    Ok(path)
}

fn content_addressed_blob_path(body: &str) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(body.as_bytes());
    let digest = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    shared_blob_dir().join(format!("sha256-{digest}.txt"))
}

fn write_blob(path: &Path, body: &str) -> Result<(), String> {
    if path.is_file() {
        return Ok(());
    }
    atomic_write(path, body.as_bytes()).map_err(|e| format!("write blob: {e}"))
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
pub fn maybe_spill(_session_id: &str, _tool: &str, body: String, max_chars: usize) -> String {
    if max_chars == 0 {
        return body;
    }
    let total_chars = body.chars().count();
    if total_chars <= max_chars {
        return body;
    }
    // Never spill obvious auth payloads into a world-readable spill dir.
    if body_looks_sensitive(&body) {
        return truncate_only(&body, max_chars);
    }
    let path = content_addressed_blob_path(&body);
    // The path can be much longer than a fixed reserve. Count the complete
    // marker first, using the full byte count as an upper bound for its digits.
    let reserve = format!(
        "...{} bytes truncated; complete output in {}...",
        body.len(),
        path.display()
    )
    .chars()
    .count();
    if reserve >= max_chars || write_blob(&path, &body).is_err() {
        return truncate_only(&body, max_chars);
    }
    let budget = PREVIEW_CHARS.min(max_chars - reserve);
    let (head_end, tail_start) = head_tail_bounds(&body, total_chars, budget);
    format!(
        "{}...{} bytes truncated; complete output in {}...{}",
        &body[..head_end],
        tail_start - head_end,
        path.display(),
        &body[tail_start..]
    )
}

/// Byte offsets `(head_end, tail_start)` of a `budget`-char preview split
/// ~70% head / ~30% tail, so the start (what ran) and the end (how it ended)
/// both survive. Scans only the chars it keeps; `total_chars` is counted once
/// by the caller.
fn head_tail_bounds(body: &str, total_chars: usize, budget: usize) -> (usize, usize) {
    if total_chars <= budget {
        return (body.len(), body.len());
    }
    let head_n = budget * 7 / 10;
    let tail_n = budget - head_n;
    let head_end = body
        .char_indices()
        .nth(head_n)
        .map(|(i, _)| i)
        .unwrap_or(body.len());
    let tail_start = if tail_n == 0 {
        body.len()
    } else {
        body.char_indices()
            .rev()
            .nth(tail_n - 1)
            .map(|(i, _)| i)
            .unwrap_or(0)
    };
    (head_end, tail_start.max(head_end))
}

/// Head-only cut for bodies that must not reach disk (or when the spill
/// failed). The marker shares the budget with the retained text.
fn truncate_only(body: &str, max_chars: usize) -> String {
    let reserve = format!("...{} bytes truncated; not spilled...", body.len())
        .chars()
        .count();
    if reserve >= max_chars {
        return body.chars().take(max_chars).collect();
    }
    let keep = PREVIEW_CHARS.min(max_chars - reserve);
    let head_end = body
        .char_indices()
        .nth(keep)
        .map(|(i, _)| i)
        .unwrap_or(body.len());
    format!(
        "{}...{} bytes truncated; not spilled...",
        &body[..head_end],
        body.len() - head_end
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
        assert!(out.contains("truncated; not spilled"), "{out}");
        assert!(!out.contains("complete output in"));
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
    fn spill_preview_keeps_the_tail() {
        let body = format!("{}\nERROR: last line", "x".repeat(20_000));
        let out = maybe_spill("s", "bash", body, 1000);
        assert!(out.ends_with("ERROR: last line"), "{out}");
        assert!(out.contains("bytes truncated; complete output in"), "{out}");
    }

    // Failure modes: a `max_chars` below the preview budget yields a result
    // longer than the limit, or the marker misreports what was cut (the byte
    // count must add up to the body with multi-byte chars at both cuts).
    #[test]
    fn spill_preview_respects_a_small_max_and_counts_bytes() {
        let body = format!("{}MID{}", "é".repeat(3_000), "ü".repeat(3_000));
        let out = maybe_spill("s", "bash", body.clone(), 1000);
        assert!(out.chars().count() <= 1000, "{} chars", out.chars().count());
        assert!(out.starts_with("éé"));
        assert!(out.ends_with("üü"));
        let kept = out
            .split("...")
            .filter(|s| !s.contains("bytes truncated"))
            .map(|s| s.len())
            .sum::<usize>();
        let cut: usize = out
            .split("...")
            .find(|s| s.contains("bytes truncated"))
            .and_then(|m| m.split(" bytes").next())
            .and_then(|n| n.parse().ok())
            .unwrap();
        assert_eq!(kept + cut, body.len(), "{out}");
    }

    // Even the truncation marker may not fit. Tiny caps must remain bounded
    // and UTF-8 safe, and cannot create an unreadable spill without a path.
    #[test]
    fn tiny_limits_and_sensitive_output_stay_bounded() {
        for body in [
            "é".repeat(3000),
            format!("Authorization: Bearer {}", "ü".repeat(3000)),
        ] {
            for limit in [1, 2, 10, 40, 100] {
                let out = maybe_spill("s", "read_file", body.clone(), limit);
                assert!(out.chars().count() <= limit, "limit {limit}: {out}");
                assert!(!out.contains("complete output in"));
                if out.contains("bytes truncated") {
                    let (kept, marker) = out.split_once("...").unwrap();
                    let cut: usize = marker.split(" bytes").next().unwrap().parse().unwrap();
                    assert_eq!(kept.len() + cut, body.len());
                }
            }
        }
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

    #[test]
    fn identical_bodies_share_one_content_addressed_blob() {
        let body = format!("blob-{}", uuid::Uuid::new_v4());
        let a = write_content_addressed_blob(&body).unwrap();
        let b = write_content_addressed_blob(&body).unwrap();
        assert_eq!(a, b);
        assert!(is_under_tool_results(&a));
        let _ = std::fs::remove_file(a);
    }
}

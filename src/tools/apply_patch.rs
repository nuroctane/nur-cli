//! Minimal unified-diff apply (V4A-style / git-style hunks) for safer edits
//! than full-file rewrites — addresses the "no apply_patch" assessment gap.

use super::sandbox;
use super::{arg_str, Tool, ToolContext};
use crate::error::{NurError, Result};
use serde_json::Value;
use std::fs;
use std::path::Path;

pub struct ApplyPatch;

impl Tool for ApplyPatch {
    fn name(&self) -> &str {
        "apply_patch"
    }

    fn description(&self) -> &str {
        "Apply a unified diff patch to a file under the workspace. \
         Prefer this for multi-hunk edits. path is the file to patch; \
         patch is a unified diff (---/+++ optional, @@ hunks required)."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File to patch (workspace-relative)"},
                "patch": {"type": "string", "description": "Unified diff text"}
            },
            "required": ["path", "patch"]
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let path = arg_str(args, "path")?;
        let patch = arg_str(args, "patch")?;
        let full = sandbox::resolve_in_workspace(&ctx.cwd, &path)?;
        super::guard_unchanged_since_read(&full)?;

        let original = if full.exists() {
            fs::read_to_string(&full)
                .map_err(|e| NurError::Tool(format!("read {}: {e}", full.display())))?
        } else {
            String::new()
        };

        let updated = apply_unified_diff(&original, &patch)?;
        if let Some(staged) = super::commit_file_write(ctx, &path, &full, &updated, "apply_patch")?
        {
            return Ok(staged);
        }

        Ok(format!(
            "patched {} ({} -> {} bytes)",
            display_rel(&ctx.cwd, &full),
            original.len(),
            updated.len()
        ))
    }
}

fn display_rel(cwd: &Path, full: &Path) -> String {
    full.strip_prefix(cwd)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| full.display().to_string())
}

/// Apply unified diff hunks to `original`. Supports standard `@@ -a,b +c,d @@` hunks.
///
/// Lines are split with `lines()` (so CRLF and LF both parse) and joined back
/// with the file's dominant ending; a trailing newline is kept only if the
/// file had one. Hunk positions are interpreted against the *original*
/// numbering, as in a real diff, so a cumulative offset tracks how earlier
/// hunks grew or shrank the file.
fn apply_unified_diff(original: &str, patch: &str) -> Result<String> {
    let mut lines: Vec<String> = if original.is_empty() {
        Vec::new()
    } else {
        original.lines().map(|l| l.to_string()).collect()
    };
    let eol = super::edit_file::dominant_eol(original);
    // A brand-new file gets a terminator; an existing file keeps whatever it had.
    let want_trailing_nl = original.is_empty() || original.ends_with('\n');

    let patch_lines: Vec<&str> = patch.lines().collect();
    let mut i = 0usize;
    let mut hunks = 0usize;
    let mut offset: isize = 0;

    while i < patch_lines.len() {
        let line = patch_lines[i];
        if line.starts_with("@@") {
            let (old_start, old_count) = parse_hunk_header(line)?;
            i += 1;
            let mut old_lines_in_hunk: Vec<&str> = Vec::new();
            let mut new_lines_in_hunk: Vec<String> = Vec::new();

            while i < patch_lines.len() {
                let l = patch_lines[i];
                if l.starts_with("@@")
                    || l.starts_with("diff ")
                    || l.starts_with("---")
                        && i + 1 < patch_lines.len()
                        && patch_lines[i + 1].starts_with("+++")
                {
                    break;
                }
                if l.starts_with("---")
                    || l.starts_with("+++")
                    || l.starts_with("index ")
                    || l.starts_with("diff ")
                {
                    i += 1;
                    continue;
                }
                if l.starts_with('\\') {
                    // "\ No newline at end of file"
                    i += 1;
                    continue;
                }
                if l.is_empty() {
                    // treat empty as context empty line sometimes missing prefix
                    old_lines_in_hunk.push("");
                    new_lines_in_hunk.push(String::new());
                    i += 1;
                    continue;
                }
                let (tag, rest) = l.split_at(1);
                match tag {
                    " " => {
                        old_lines_in_hunk.push(rest);
                        new_lines_in_hunk.push(rest.to_string());
                    }
                    "-" => {
                        old_lines_in_hunk.push(rest);
                    }
                    "+" => {
                        new_lines_in_hunk.push(rest.to_string());
                    }
                    _ => {
                        // line without prefix - context
                        old_lines_in_hunk.push(l);
                        new_lines_in_hunk.push(l.to_string());
                    }
                }
                i += 1;
            }

            // old_start is 1-based in the original file; shift it by what the
            // hunks before this one already added or removed. 0 means "empty
            // file" (create).
            let shifted = |n: usize| -> usize {
                let v = n as isize + offset;
                (v.max(0) as usize).min(lines.len())
            };
            let start = if old_start == 0 {
                0
            } else {
                shifted(old_start.saturating_sub(1))
            };
            let delta = new_lines_in_hunk.len() as isize - old_lines_in_hunk.len() as isize;

            // Verify old lines match (fuzzy: allow if file empty and old is empty)
            if !old_lines_in_hunk.is_empty() {
                if start + old_lines_in_hunk.len() > lines.len()
                    && !(lines.is_empty() && old_start <= 1)
                {
                    // Fall back to a search — but only accept a UNIQUE match.
                    let found = find_slice_unique(&lines, &old_lines_in_hunk, old_start)?;
                    apply_at(
                        &mut lines,
                        found,
                        old_lines_in_hunk.len(),
                        &new_lines_in_hunk,
                    );
                } else if lines.is_empty() && old_lines_in_hunk.iter().all(|l| l.is_empty()) {
                    lines = new_lines_in_hunk;
                } else if start + old_lines_in_hunk.len() > lines.len() {
                    // Reachable on an empty file whose hunk carries non-empty old
                    // lines: both branches above decline, and indexing here would
                    // panic. Treat it as a mismatch and let the unique search
                    // report a clean error.
                    let found = find_slice_unique(&lines, &old_lines_in_hunk, old_start)?;
                    apply_at(
                        &mut lines,
                        found,
                        old_lines_in_hunk.len(),
                        &new_lines_in_hunk,
                    );
                } else {
                    let slice = &lines[start..start + old_lines_in_hunk.len()];
                    let matches = slice
                        .iter()
                        .zip(old_lines_in_hunk.iter())
                        .all(|(a, b)| a.as_str() == *b);
                    if !matches {
                        let found = find_slice_unique(&lines, &old_lines_in_hunk, old_start)?;
                        apply_at(
                            &mut lines,
                            found,
                            old_lines_in_hunk.len(),
                            &new_lines_in_hunk,
                        );
                    } else {
                        apply_at(
                            &mut lines,
                            start,
                            old_lines_in_hunk.len(),
                            &new_lines_in_hunk,
                        );
                    }
                }
            } else {
                // Pure addition. `@@ -N,0 +M,k @@` means "insert after line N"
                // in unified-diff terms, so the insertion index is N itself
                // (0-based), not N-1 - the old code put the new lines one line
                // too early. A header without the `,0` is malformed for a
                // hunk with no old lines; treat it as "before line N".
                let at = if old_count == 0 {
                    shifted(old_start)
                } else {
                    start
                };
                for (j, nl) in new_lines_in_hunk.into_iter().enumerate() {
                    lines.insert(at + j, nl);
                }
            }
            offset += delta;
            hunks += 1;
        } else {
            i += 1;
        }
    }

    if hunks == 0 {
        return Err(NurError::Tool(
            "no unified-diff hunks found (need lines starting with @@)".into(),
        ));
    }

    let mut out = lines.join(eol);
    if want_trailing_nl && !out.is_empty() {
        out.push_str(eol);
    }
    Ok(out)
}

fn parse_hunk_header(line: &str) -> Result<(usize, usize)> {
    // @@ -12,5 +12,7 @@
    let rest = line.trim_start_matches('@').trim();
    let rest = rest.trim_start_matches('@').trim();
    let minus = rest
        .split_whitespace()
        .next()
        .ok_or_else(|| NurError::Tool(format!("bad hunk header: {line}")))?;
    let minus = minus.trim_start_matches('-');
    let start = minus
        .split(',')
        .next()
        .unwrap_or("0")
        .parse::<usize>()
        .map_err(|_| NurError::Tool(format!("bad hunk header: {line}")))?;
    let count = minus
        .split(',')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    Ok((start, count))
}

/// Search for the hunk's old lines; refuse ambiguous (multi-site) matches so a
/// fuzzy relocation can never edit the wrong copy of repeated code.
fn find_slice_unique(lines: &[String], needle: &[&str], hunk_line: usize) -> Result<usize> {
    if needle.is_empty() {
        return Ok(0);
    }
    let hits: Vec<usize> = lines
        .windows(needle.len())
        .enumerate()
        .filter(|(_, w)| w.iter().zip(needle.iter()).all(|(a, b)| a.as_str() == *b))
        .map(|(i, _)| i)
        .collect();
    match hits.len() {
        1 => Ok(hits[0]),
        0 => Err(NurError::Tool(format!(
            "hunk context mismatch at line {hunk_line} — file may have changed; re-read and retry"
        ))),
        n => Err(NurError::Tool(format!(
            "hunk context at line {hunk_line} is ambiguous ({n} matches) — \
             include more surrounding context lines in the diff"
        ))),
    }
}

fn apply_at(lines: &mut Vec<String>, start: usize, old_len: usize, new_lines: &[String]) {
    let end = (start + old_len).min(lines.len());
    lines.splice(start..end, new_lines.iter().cloned());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A CRLF file came back LF: `lines()` ate the `\r` and the join used `\n`.
    #[test]
    fn crlf_file_keeps_crlf() {
        let original = "a\r\nb\r\nc\r\n";
        let patch = "@@ -2,1 +2,1 @@\n-b\n+B\n";
        assert_eq!(
            apply_unified_diff(original, patch).unwrap(),
            "a\r\nB\r\nc\r\n"
        );
    }

    /// Patches written on Windows carry CRLF themselves; the `\r` must not
    /// leak into the context comparison or the inserted text.
    #[test]
    fn crlf_patch_against_lf_file_stays_lf() {
        let original = "a\nb\nc\n";
        let patch = "@@ -2,1 +2,1 @@\r\n-b\r\n+B\r\n";
        assert_eq!(apply_unified_diff(original, patch).unwrap(), "a\nB\nc\n");
    }

    #[test]
    fn file_without_trailing_newline_does_not_gain_one() {
        let original = "a\nb";
        let patch = "@@ -2,1 +2,1 @@\n-b\n+B\n";
        assert_eq!(apply_unified_diff(original, patch).unwrap(), "a\nB");
        // ...and one with a trailing blank line keeps it
        assert_eq!(
            apply_unified_diff("a\n\n", "@@ -1,1 +1,1 @@\n-a\n+A\n").unwrap(),
            "A\n\n"
        );
    }

    /// `@@ -2,0 +3,1 @@` inserts *after* line 2; the result used to land
    /// before it.
    #[test]
    fn zero_count_hunk_inserts_after_the_named_line() {
        let original = "a\nb\nc\n";
        let patch = "@@ -2,0 +3,1 @@\n+X\n";
        assert_eq!(apply_unified_diff(original, patch).unwrap(), "a\nb\nX\nc\n");
        // after the last line
        assert_eq!(
            apply_unified_diff(original, "@@ -3,0 +4,1 @@\n+X\n").unwrap(),
            "a\nb\nc\nX\n"
        );
        // at the very top
        assert_eq!(
            apply_unified_diff(original, "@@ -0,0 +1,1 @@\n+X\n").unwrap(),
            "X\na\nb\nc\n"
        );
    }

    /// Hunk positions refer to the original file; a second hunk after an
    /// earlier insertion must land on the right line without context search.
    #[test]
    fn later_hunks_account_for_earlier_growth() {
        let original = "a\nb\nc\nd\n";
        let patch = "@@ -1,0 +2,2 @@\n+1\n+2\n@@ -3,0 +6,1 @@\n+X\n";
        assert_eq!(
            apply_unified_diff(original, patch).unwrap(),
            "a\n1\n2\nb\nc\nX\nd\n"
        );
    }

    #[test]
    fn mismatched_context_is_an_error_not_a_guess() {
        let original = "a\nb\nc\n";
        let err = apply_unified_diff(original, "@@ -2,1 +2,1 @@\n-zzz\n+B\n").unwrap_err();
        assert!(err.to_string().contains("context mismatch"), "{err}");
        let err = apply_unified_diff("x\ny\nx\ny\n", "@@ -9,1 +9,1 @@\n-x\n+X\n").unwrap_err();
        assert!(err.to_string().contains("ambiguous"), "{err}");
    }

    #[test]
    fn new_file_gets_content_and_terminator() {
        assert_eq!(
            apply_unified_diff("", "@@ -0,0 +1,2 @@\n+a\n+b\n").unwrap(),
            "a\nb\n"
        );
    }
}

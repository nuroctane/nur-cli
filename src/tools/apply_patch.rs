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
    // Each line's own ending, so a mixed-ending file comes back with only the
    // hunks' lines touched; joining everything with the dominant ending
    // rewrote every other line of such a file.
    let mut endings: Vec<&'static str> = if original.is_empty() {
        Vec::new()
    } else {
        original
            .split_inclusive('\n')
            .map(|segment| {
                if segment.ends_with("\r\n") {
                    "\r\n"
                } else if segment.ends_with('\n') {
                    "\n"
                } else {
                    ""
                }
            })
            .collect()
    };
    // A brand-new file gets a terminator; an existing file keeps whatever it had.
    let want_trailing_nl = original.is_empty() || original.ends_with('\n');

    let patch_lines: Vec<&str> = patch.lines().collect();
    let mut i = 0usize;
    let mut hunks = 0usize;
    let mut offset: isize = 0;

    while i < patch_lines.len() {
        let line = patch_lines[i];
        if line.starts_with("@@") {
            let (old_start, old_count, new_count) = parse_hunk_header(line)?;
            i += 1;
            let mut old_lines_in_hunk: Vec<&str> = Vec::new();
            let mut new_lines_in_hunk: Vec<String> = Vec::new();

            while i < patch_lines.len() {
                let l = patch_lines[i];
                // Removing `-- note` gives the row `--- note`, adding `++x`
                // gives `+++x`: while the header's counts still expect lines,
                // such rows are content, not the next file's header.
                let hunk_done =
                    old_lines_in_hunk.len() >= old_count && new_lines_in_hunk.len() >= new_count;
                if l.starts_with("@@")
                    || l.starts_with("diff ")
                    || hunk_done
                        && l.starts_with("---")
                        && i + 1 < patch_lines.len()
                        && patch_lines[i + 1].starts_with("+++")
                {
                    break;
                }
                if hunk_done
                    && (l.starts_with("---") || l.starts_with("+++") || l.starts_with("index "))
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
                        &mut endings,
                        eol,
                        found,
                        old_lines_in_hunk.len(),
                        &new_lines_in_hunk,
                    );
                } else if lines.is_empty() && old_lines_in_hunk.iter().all(|l| l.is_empty()) {
                    endings = vec![eol; new_lines_in_hunk.len()];
                    lines = new_lines_in_hunk;
                } else if start + old_lines_in_hunk.len() > lines.len() {
                    // Reachable on an empty file whose hunk carries non-empty old
                    // lines: both branches above decline, and indexing here would
                    // panic. Treat it as a mismatch and let the unique search
                    // report a clean error.
                    let found = find_slice_unique(&lines, &old_lines_in_hunk, old_start)?;
                    apply_at(
                        &mut lines,
                        &mut endings,
                        eol,
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
                            &mut endings,
                            eol,
                            found,
                            old_lines_in_hunk.len(),
                            &new_lines_in_hunk,
                        );
                    } else {
                        apply_at(
                            &mut lines,
                            &mut endings,
                            eol,
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
                    endings.insert((at + j).min(endings.len()), eol);
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

    let mut out = String::with_capacity(original.len());
    let last = lines.len().saturating_sub(1);
    for (n, line) in lines.iter().enumerate() {
        out.push_str(line);
        let ending = endings.get(n).copied().unwrap_or("");
        // Every line but the last ends; the last one only if the file did.
        if n < last || want_trailing_nl {
            out.push_str(if ending.is_empty() { eol } else { ending });
        }
    }
    Ok(out)
}

/// `(old_start, old_count, new_count)` from `@@ -12,5 +12,7 @@`. A missing
/// count is 1, as in unified diff; a missing `+` side counts as 0.
fn parse_hunk_header(line: &str) -> Result<(usize, usize, usize)> {
    let rest = line.trim_start_matches('@').trim();
    let rest = rest.trim_start_matches('@').trim();
    let mut fields = rest.split_whitespace();
    let minus = fields
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
    let new_count = fields
        .next()
        .and_then(|plus| plus.strip_prefix('+'))
        .map(|plus| {
            plus.split(',')
                .nth(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(1)
        })
        .unwrap_or(0);
    Ok((start, count, new_count))
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

/// Replace `old_len` lines at `start`. Line n of the replacement keeps the
/// ending line n of the replaced span had; extra lines take the span's last
/// ending, and `eol` only when the span had none.
fn apply_at(
    lines: &mut Vec<String>,
    endings: &mut Vec<&'static str>,
    eol: &'static str,
    start: usize,
    old_len: usize,
    new_lines: &[String],
) {
    let end = (start + old_len).min(lines.len());
    let replaced: Vec<&'static str> = endings
        .get(start..end.min(endings.len()))
        .map(<[&str]>::to_vec)
        .unwrap_or_default();
    let nearby = replaced
        .iter()
        .rev()
        .copied()
        .find(|e| !e.is_empty())
        .unwrap_or(eol);
    let new_endings: Vec<&'static str> = (0..new_lines.len())
        .map(|n| {
            replaced
                .get(n)
                .copied()
                .filter(|e| !e.is_empty())
                .unwrap_or(nearby)
        })
        .collect();
    lines.splice(start..end, new_lines.iter().cloned());
    let e_start = start.min(endings.len());
    let e_end = end.min(endings.len());
    endings.splice(e_start..e_end, new_endings);
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

    /// A mixed-ending file came back with every line on the dominant ending:
    /// one hunk turned into a whole-file diff.
    #[test]
    fn mixed_endings_survive_outside_the_hunk() {
        let original = "a\r\nb\nc\r\nd\n";
        let patch = "@@ -3,1 +3,2 @@\n-c\n+C\n+C2\n";
        assert_eq!(
            apply_unified_diff(original, patch).unwrap(),
            "a\r\nb\nC\r\nC2\r\nd\n"
        );
    }

    /// Patches written on Windows carry CRLF themselves; the `\r` must not
    /// leak into the context comparison or the inserted text.
    /// Removing a `-- note` line yields the row `--- note` and adding `++x`
    /// yields `+++x`; both were skipped as file headers, so the deletion never
    /// happened and the hunk matched one line short.
    #[test]
    fn rows_that_look_like_file_headers_inside_a_hunk_are_content() {
        let original = "select 1;\n-- note\nselect 2;\n";
        let patch = "@@ -1,3 +1,3 @@\n select 1;\n--- note\n+++x\n select 2;\n";
        assert_eq!(
            apply_unified_diff(original, patch).unwrap(),
            "select 1;\n++x\nselect 2;\n"
        );
        // A real header for the next file still ends the hunk once its
        // counts are used up.
        let two_files =
            "--- a/f\n+++ b/f\n@@ -1,1 +1,1 @@\n-select 1;\n+select 9;\n--- a/g\n+++ b/g\n";
        assert!(apply_unified_diff(original, two_files)
            .unwrap()
            .starts_with("select 9;\n"));
    }

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

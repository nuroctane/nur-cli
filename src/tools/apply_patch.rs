//! Minimal unified-diff apply (V4A-style / git-style hunks) for safer edits
//! than full-file rewrites — addresses the "no apply_patch" assessment gap.

use super::sandbox;
use super::{arg_str, Tool, ToolContext};
use crate::error::{MuseError, Result};
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

        let original = if full.exists() {
            fs::read_to_string(&full)
                .map_err(|e| MuseError::Tool(format!("read {}: {e}", full.display())))?
        } else {
            String::new()
        };

        let updated = apply_unified_diff(&original, &patch)?;
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| MuseError::Tool(format!("mkdir {}: {e}", parent.display())))?;
        }
        fs::write(&full, &updated)
            .map_err(|e| MuseError::Tool(format!("write {}: {e}", full.display())))?;

        Ok(format!(
            "patched {} ({} → {} bytes)",
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
fn apply_unified_diff(original: &str, patch: &str) -> Result<String> {
    let mut lines: Vec<String> = if original.is_empty() {
        Vec::new()
    } else {
        original.lines().map(|l| l.to_string()).collect()
    };
    // Preserve trailing newline semantics loosely
    let had_trailing_nl = original.ends_with('\n');

    let patch_lines: Vec<&str> = patch.lines().collect();
    let mut i = 0usize;
    let mut hunks = 0usize;

    while i < patch_lines.len() {
        let line = patch_lines[i];
        if line.starts_with("@@") {
            let (old_start, _old_count) = parse_hunk_header(line)?;
            i += 1;
            // Collect hunk body
            let mut old_chunk: Vec<Option<&str>> = Vec::new(); // None = added-only context skip
            let mut new_chunk: Vec<String> = Vec::new();
            // We'll rebuild by matching old lines from old_start
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
                        // line without prefix — context
                        old_lines_in_hunk.push(l);
                        new_lines_in_hunk.push(l.to_string());
                    }
                }
                let _ = (&mut old_chunk, &mut new_chunk); // silence
                i += 1;
            }

            // old_start is 1-based; 0 means empty file create
            let start = if old_start == 0 {
                0
            } else {
                old_start.saturating_sub(1)
            };

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
                // pure addition
                let at = start.min(lines.len());
                for (j, nl) in new_lines_in_hunk.into_iter().enumerate() {
                    lines.insert(at + j, nl);
                }
            }
            hunks += 1;
        } else {
            i += 1;
        }
    }

    if hunks == 0 {
        return Err(MuseError::Tool(
            "no unified-diff hunks found (need lines starting with @@)".into(),
        ));
    }

    let mut out = lines.join("\n");
    if had_trailing_nl || out.is_empty() {
        if !out.ends_with('\n') && !out.is_empty() {
            out.push('\n');
        }
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
        .ok_or_else(|| MuseError::Tool(format!("bad hunk header: {line}")))?;
    let minus = minus.trim_start_matches('-');
    let start = minus
        .split(',')
        .next()
        .unwrap_or("0")
        .parse::<usize>()
        .map_err(|_| MuseError::Tool(format!("bad hunk header: {line}")))?;
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
        0 => Err(MuseError::Tool(format!(
            "hunk context mismatch at line {hunk_line} — file may have changed; re-read and retry"
        ))),
        n => Err(MuseError::Tool(format!(
            "hunk context at line {hunk_line} is ambiguous ({n} matches) — \
             include more surrounding context lines in the diff"
        ))),
    }
}

fn apply_at(lines: &mut Vec<String>, start: usize, old_len: usize, new_lines: &[String]) {
    let end = (start + old_len).min(lines.len());
    lines.splice(start..end, new_lines.iter().cloned());
}

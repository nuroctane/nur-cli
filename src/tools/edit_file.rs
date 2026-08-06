use super::{arg_str, resolve_path, Tool, ToolContext};
use crate::error::{NurError, Result};
use serde_json::Value;
use std::fs;
use std::path::Path;

/// Write edited contents, honoring Shepherd-style proposal mode when enabled.
fn commit_edit(ctx: &ToolContext, path_arg: &str, full: &Path, updated: String) -> Result<String> {
    if crate::config::load_config()
        .map(|c| c.proposal_mode)
        .unwrap_or(false)
    {
        let sid = std::env::var("NUR_SESSION_ID").unwrap_or_else(|_| "default".into());
        let entry =
            crate::agent::proposal::stage_write(&sid, &ctx.cwd, path_arg, &updated, "edit_file")
                .map_err(NurError::Tool)?;
        return Ok(format!(
            "proposal staged `{}` ({} bytes) — not written to workspace. \
             Use tool `proposal` action=list|apply|discard.",
            entry.rel_path, entry.bytes
        ));
    }
    fs::write(full, updated)
        .map_err(|e| NurError::Tool(format!("write {}: {e}", full.display())))?;
    Ok(String::new())
}

pub struct EditFile;

impl Tool for EditFile {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn description(&self) -> &str {
        "Replace an exact string in a file. old_string must match uniquely unless replace_all is true. Always read the file first and copy old_string exactly including whitespace."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "old_string": {"type": "string"},
                "new_string": {"type": "string"},
                "replace_all": {"type": "boolean", "default": false}
            },
            "required": ["path", "old_string", "new_string"]
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let path = arg_str(args, "path")?;
        let old = arg_str(args, "old_string")?;
        let new = arg_str(args, "new_string")?;
        let replace_all = args
            .get("replace_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let full = resolve_path(&ctx.cwd, &path)?;
        let file_content = fs::read_to_string(&full)
            .map_err(|e| NurError::Tool(format!("read {}: {e}", full.display())))?;

        // First try exact match (fast path)
        let count = file_content.matches(&old).count();
        if count > 0 {
            if count > 1 && !replace_all {
                return Err(NurError::Tool(format!(
                    "old_string matched {count} times; set replace_all=true or make old_string unique"
                )));
            }
            let updated = if replace_all {
                file_content.replace(&old, &new)
            } else {
                file_content.replacen(&old, &new, 1)
            };
            let staged = commit_edit(ctx, &path, &full, updated)?;
            if !staged.is_empty() {
                return Ok(staged);
            }
            return Ok(format!(
                "edited {} ({} replacement{})",
                full.display(),
                if replace_all { count } else { 1 },
                if replace_all && count != 1 { "s" } else { "" }
            ));
        }

        // Fallback 1: normalize line endings CRLF -> LF for both sides.
        // Models sometimes emit LF while file is CRLF or vice versa.
        let content_n = file_content.replace("\r\n", "\n");
        let old_n = old.replace("\r\n", "\n");
        let count_n = content_n.matches(&old_n).count();
        if count_n > 0 {
            if count_n > 1 && !replace_all {
                return Err(NurError::Tool(format!(
                    "old_string matched {count_n} times after normalizing line endings; make it unique"
                )));
            }
            // Splice into the ORIGINAL text at the mapped offsets rather than
            // re-emitting the normalized copy. The old code rebuilt the whole
            // file from `content_n` (already LF-only) and then, because
            // `updated_n` could never contain "\r\n", unconditionally converted
            // every "\n" back to "\r\n" — silently rewriting every bare-LF line
            // in a mixed-ending file and turning a one-line edit into a
            // whole-file diff.
            let updated = splice_normalized(&file_content, &content_n, &old_n, &new, replace_all);
            let staged = commit_edit(ctx, &path, &full, updated)?;
            if !staged.is_empty() {
                return Ok(staged);
            }
            return Ok(format!(
                "edited {} ({} replacement{} via line-ending normalization)",
                full.display(),
                if replace_all { count_n } else { 1 },
                if replace_all && count_n != 1 { "s" } else { "" }
            ));
        }

        // Fallback 2: trimmed old_string search — model often copies with extra
        // leading/trailing newline or spaces. If trimmed version occurs uniquely,
        // replace that exact slice.
        let old_trim = old.trim();
        // The surrounding whitespace we trimmed off `old` is still present in
        // the file around the match, so the replacement must be trimmed the
        // same way. Substituting the untrimmed `new` re-inserted that
        // whitespace a second time — a 4-space indent silently became 8, which
        // is a semantic change in Python/YAML, and the tool still reported
        // plain success.
        let new_trim = new.trim();
        if !old_trim.is_empty() {
            let count_trim = file_content.matches(old_trim).count();
            if count_trim == 1 && !replace_all {
                let updated = file_content.replacen(old_trim, new_trim, 1);
                let staged = commit_edit(ctx, &path, &full, updated)?;
                if !staged.is_empty() {
                    return Ok(staged);
                }
                return Ok(format!(
                    "edited {} (1 replacement — FUZZY: matched after trimming surrounding \
                     whitespace from old_string, and trimmed new_string to match. Verify the \
                     indentation is what you intended, then copy exact whitespace next time)",
                    full.display()
                ));
            }
            // Also try trimmed with normalized line endings
            let old_trim_n = old_trim.replace("\r\n", "\n");
            if content_n.matches(&old_trim_n).count() == 1 && !replace_all {
                let updated =
                    splice_normalized(&file_content, &content_n, &old_trim_n, new_trim, false);
                let staged = commit_edit(ctx, &path, &full, updated)?;
                if !staged.is_empty() {
                    return Ok(staged);
                }
                return Ok(format!(
                    "edited {} (1 replacement — FUZZY: matched after trimming whitespace and \
                     normalizing line endings. Verify the result)",
                    full.display()
                ));
            }
        }

        // Still not found — build a helpful diagnostic. Previous message
        // "old_string not found in file" left the model blind and the transcript
        // showed a correct-looking diff with a failing footer on every expand.
        // Now we include file length and nearby context.
        let file_len = file_content.len();
        let line_count = file_content.lines().count();
        let mut hint = format!(
            "old_string not found in file (file {} chars, {} lines). ",
            file_len, line_count
        );

        if let Some(first_line) = old
            .lines()
            .next()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
        {
            if let Some(pos) = file_content.find(first_line) {
                let start = file_content[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
                let end = file_content[pos..]
                    .find('\n')
                    .map(|i| pos + i)
                    .unwrap_or(file_content.len());
                // These are arbitrary byte offsets; slicing a `str` off a char
                // boundary panics, and this is the *recovery* path — any file
                // with a multi-byte char (·, —, …) within 200 bytes of the
                // match blew up instead of producing the diagnostic.
                let ctx_start = floor_char_boundary(&file_content, start.saturating_sub(200));
                let ctx_end = ceil_char_boundary(&file_content, end.saturating_add(200));
                let snippet: String = file_content[ctx_start..ctx_end].chars().take(500).collect();
                hint.push_str(&format!(
                    "First line of old_string '{}' found near byte {}. Nearby snippet:\n---\n{}\n---\nTip: read the file with read_file and copy old_string exactly including whitespace.",
                    first_line, pos, snippet
                ));
            } else {
                hint.push_str(&format!(
                    "First line '{}' not found in file. Tip: read the file first with read_file, then copy the exact block you want to replace.",
                    first_line
                ));
            }
        } else {
            hint.push_str("old_string appears empty or whitespace-only after trimming — provide a non-empty unique block and read the file first.");
        }

        Err(NurError::Tool(hint))
    }
}

/// Largest index `<= i` that lies on a UTF-8 char boundary.
fn floor_char_boundary(s: &str, i: usize) -> usize {
    let mut i = i.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Smallest index `>= i` that lies on a UTF-8 char boundary.
fn ceil_char_boundary(s: &str, i: usize) -> usize {
    let mut i = i.min(s.len());
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// Which line ending dominates `s` — used only for the text we *insert*.
fn dominant_eol(s: &str) -> &'static str {
    let crlf = s.matches("\r\n").count();
    let lf = s.matches('\n').count() - crlf;
    if crlf > lf {
        "\r\n"
    } else {
        "\n"
    }
}

/// Re-apply the line-ending pattern of `span` onto LF-normalized `new_lf`.
///
/// The n-th newline of the replacement takes the n-th line ending of the text
/// it replaces; replacements with more lines than the span fall back to the
/// file's dominant ending. This keeps a one-line edit from converting the
/// endings of the lines it touches.
fn apply_eol_pattern(new_lf: &str, span: &str, fallback: &str) -> String {
    let mut endings: Vec<&str> = Vec::new();
    let b = span.as_bytes();
    for i in 0..b.len() {
        if b[i] == b'\n' {
            endings.push(if i > 0 && b[i - 1] == b'\r' {
                "\r\n"
            } else {
                "\n"
            });
        }
    }
    let mut out = String::with_capacity(new_lf.len() + endings.len());
    let mut seen = 0usize;
    for ch in new_lf.chars() {
        if ch == '\n' {
            out.push_str(endings.get(seen).copied().unwrap_or(fallback));
            seen += 1;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Map a byte offset in the CRLF→LF normalized text back to the original text.
///
/// Normalization only ever deletes a `\r` that precedes a `\n`, so walking both
/// in lockstep recovers the original offset exactly.
fn denormalized_offset(original: &str, norm_off: usize) -> usize {
    let bytes = original.as_bytes();
    let (mut oi, mut ni) = (0usize, 0usize);
    while oi < bytes.len() && ni < norm_off {
        if bytes[oi] == b'\r' && bytes.get(oi + 1) == Some(&b'\n') {
            oi += 1; // the \r vanished in the normalized copy
            continue;
        }
        oi += 1;
        ni += 1;
    }
    oi
}

/// Replace `old_n` (a CRLF→LF-normalized needle) inside `original`, editing
/// **only** the matched span.
///
/// Finding the match in the normalized copy tolerates the model sending LF for
/// a CRLF file, but the surrounding bytes must be preserved verbatim: rebuilding
/// the file from the normalized copy and re-adding `\r` everywhere rewrites
/// every line ending the file happened not to share.
fn splice_normalized(
    original: &str,
    content_n: &str,
    old_n: &str,
    new: &str,
    replace_all: bool,
) -> String {
    let fallback_eol = dominant_eol(original);
    let new_lf = new.replace("\r\n", "\n");

    let mut out = String::with_capacity(original.len());
    let mut cursor_norm = 0usize; // position in content_n already copied
    let mut search_from = 0usize;
    while let Some(rel) = content_n[search_from..].find(old_n) {
        let hit_n = search_from + rel;
        let hit_o = denormalized_offset(original, hit_n);
        let end_o = denormalized_offset(original, hit_n + old_n.len());
        let copy_from_o = denormalized_offset(original, cursor_norm);
        out.push_str(&original[copy_from_o..hit_o]);
        // Line n of the replacement inherits the ending that line n of the
        // replaced span actually had, so a CRLF line stays CRLF and an LF line
        // stays LF even inside a mixed-ending file.
        out.push_str(&apply_eol_pattern(
            &new_lf,
            &original[hit_o..end_o],
            fallback_eol,
        ));
        cursor_norm = hit_n + old_n.len();
        search_from = cursor_norm;
        if !replace_all {
            break;
        }
        if old_n.is_empty() {
            break;
        }
    }
    let tail_o = denormalized_offset(original, cursor_norm);
    out.push_str(&original[tail_o..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mixed-ending file must keep every line ending it already had. The old
    /// code rebuilt the file from the LF-normalized copy and re-CRLF'd all of
    /// it, so one edit rewrote every bare-LF line in the file.
    #[test]
    fn crlf_fallback_only_touches_the_matched_span() {
        let original = "alpha\r\nbeta\ngamma\r\ndelta\nepsilon\n";
        let content_n = original.replace("\r\n", "\n");
        let out = splice_normalized(
            original,
            &content_n,
            "beta\ngamma\n",
            "beta\nGAMMA\n",
            false,
        );
        let bare_lf_before = original.matches('\n').count() - original.matches("\r\n").count();
        let bare_lf_after = out.matches('\n').count() - out.matches("\r\n").count();
        assert_eq!(
            bare_lf_before, bare_lf_after,
            "line endings outside the edit were rewritten:\n{out:?}"
        );
        assert!(out.contains("GAMMA"), "edit did not apply: {out:?}");
        assert!(out.starts_with("alpha\r\n"), "prefix disturbed: {out:?}");
        assert!(out.ends_with("epsilon\n"), "suffix disturbed: {out:?}");
    }

    #[test]
    fn splice_preserves_a_pure_lf_file() {
        let original = "one\ntwo\nthree\n";
        let content_n = original.to_string();
        let out = splice_normalized(original, &content_n, "two\n", "TWO\n", false);
        assert_eq!(out, "one\nTWO\nthree\n");
        assert!(!out.contains('\r'));
    }

    #[test]
    fn splice_replace_all_hits_every_occurrence() {
        let original = "x\r\nx\r\nx\r\n";
        let content_n = original.replace("\r\n", "\n");
        let out = splice_normalized(original, &content_n, "x\n", "y\n", true);
        assert_eq!(out, "y\r\ny\r\ny\r\n");
    }

    /// Offsets must survive multi-byte characters before the match.
    #[test]
    fn splice_is_correct_after_multibyte_text() {
        let original = "héllo · 日本\r\ntarget\r\ntail\r\n";
        let content_n = original.replace("\r\n", "\n");
        let out = splice_normalized(original, &content_n, "target\n", "REPLACED\n", false);
        assert_eq!(out, "héllo · 日本\r\nREPLACED\r\ntail\r\n");
    }

    /// The diagnostic path must not panic when a multi-byte char lands inside
    /// the ±200-byte context window.
    #[test]
    fn context_window_never_splits_a_char() {
        for pad in 185..215usize {
            let mut s = String::new();
            s.push_str(&"a".repeat(10));
            s.push('·'); // 2 bytes
            s.push_str(&"b".repeat(pad));
            s.push_str("\nneedle\n");
            s.push_str(&"c".repeat(pad));
            let pos = s.find("needle").expect("needle present");
            let start = s[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let end = s[pos..].find('\n').map(|i| pos + i).unwrap_or(s.len());
            let a = floor_char_boundary(&s, start.saturating_sub(200));
            let b = ceil_char_boundary(&s, end.saturating_add(200));
            let _ = &s[a..b]; // must not panic
        }
    }

    #[test]
    fn char_boundary_helpers_clamp_to_len() {
        let s = "ab·cd";
        assert_eq!(ceil_char_boundary(s, 9999), s.len());
        assert_eq!(floor_char_boundary(s, 9999), s.len());
        // byte 3 is inside the 2-byte '·' (bytes 2..4)
        assert_eq!(floor_char_boundary(s, 3), 2);
        assert_eq!(ceil_char_boundary(s, 3), 4);
    }

    #[test]
    fn dominant_eol_picks_the_majority() {
        assert_eq!(dominant_eol("a\r\nb\r\nc\n"), "\r\n");
        assert_eq!(dominant_eol("a\nb\nc\r\n"), "\n");
        assert_eq!(dominant_eol("no newlines"), "\n");
    }
}

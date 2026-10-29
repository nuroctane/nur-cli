use super::sandbox;
use super::{arg_str, Tool, ToolContext};
use crate::error::{NurError, Result};
use serde_json::Value;
use std::fs;

pub struct MultiEdit;

impl Tool for MultiEdit {
    fn name(&self) -> &str {
        "multi_edit"
    }

    fn description(&self) -> &str {
        "Apply multiple exact search/replace edits to one file in order. \
         Each edit needs unique old_string unless replace_all is true for that edit."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "edits": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "old_string": {"type": "string"},
                            "new_string": {"type": "string"},
                            "replace_all": {"type": "boolean", "default": false}
                        },
                        "required": ["old_string", "new_string"]
                    }
                }
            },
            "required": ["path", "edits"]
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let path = arg_str(args, "path")?;
        let full = sandbox::resolve_in_workspace(&ctx.cwd, &path)?;
        let edits = args
            .get("edits")
            .and_then(|v| v.as_array())
            .ok_or_else(|| NurError::Tool("edits array required".into()))?;
        if edits.is_empty() {
            return Err(NurError::Tool("edits empty".into()));
        }
        super::guard_unchanged_since_read(&full)?;
        let original = fs::read_to_string(&full)
            .map_err(|e| NurError::Tool(format!("read {}: {e}", full.display())))?;
        let (content, total) = apply_edits(&original, edits)?;
        if let Some(staged) = super::commit_file_write(ctx, &path, &full, &content, "multi_edit")? {
            return Ok(staged);
        }
        Ok(format!(
            "multi_edit {} - {total} replacement(s) in {} edit(s)",
            full.display(),
            edits.len()
        ))
    }
}

/// Every edit is validated against the running text before anything is
/// written, so a failing edit leaves the file exactly as it was.
fn apply_edits(original: &str, edits: &[Value]) -> Result<(String, usize)> {
    let mut content = original.to_string();
    let mut total = 0usize;
    for (i, ed) in edits.iter().enumerate() {
        let old = ed
            .get("old_string")
            .and_then(|v| v.as_str())
            .ok_or_else(|| NurError::Tool(format!("edits[{i}].old_string required")))?;
        if old.is_empty() {
            // `content.replace("", new)` inserts `new` between every character
            // of the file; `edit_file` refuses the same call for the same reason.
            return Err(NurError::Tool(format!(
                "edits[{i}]: old_string must not be empty - include the text to replace \
                 (use write_file to create a file's contents); no edits in this call were written"
            )));
        }
        let new = ed
            .get("new_string")
            .and_then(|v| v.as_str())
            .ok_or_else(|| NurError::Tool(format!("edits[{i}].new_string required")))?;
        let replace_all = ed
            .get("replace_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        // CRLF files match line-ending-insensitively, like `edit_file`:
        // read_file shows lines without `\r`, so the model sends LF, and each
        // replaced line keeps the ending it had.
        let crlf = content.contains("\r\n");
        let (haystack, needle) = if crlf {
            (content.replace("\r\n", "\n"), old.replace("\r\n", "\n"))
        } else {
            (content.clone(), old.to_string())
        };
        let count = haystack.matches(needle.as_str()).count();
        if count == 0 {
            return Err(NurError::Tool(format!(
                "edits[{i}]: old_string not found (earlier edits in this call were not written)"
            )));
        }
        if count > 1 && !replace_all {
            return Err(NurError::Tool(format!(
                "edits[{i}]: old_string matched {count} times; set replace_all=true or make unique"
            )));
        }
        content = if crlf {
            super::edit_file::splice_normalized(&content, &haystack, &needle, new, replace_all)
        } else if replace_all {
            content.replace(old, new)
        } else {
            content.replacen(old, new, 1)
        };
        total += if replace_all { count } else { 1 };
    }
    Ok((content, total))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edits(list: &[(&str, &str, bool)]) -> Vec<Value> {
        list.iter()
            .map(|(old, new, all)| {
                serde_json::json!({ "old_string": old, "new_string": new, "replace_all": all })
            })
            .collect()
    }

    /// read_file shows CRLF lines without `\r`, so a multi-line old_string
    /// arrives LF-only: it must still match, and the result must stay CRLF.
    #[test]
    fn lf_edits_apply_to_a_crlf_file_and_keep_its_endings() {
        let (out, n) = apply_edits(
            "fn a() {\r\n    1\r\n}\r\n",
            &edits(&[("fn a() {\n    1\n", "fn a() {\n    2\n    3\n", false)]),
        )
        .unwrap();
        assert_eq!(out, "fn a() {\r\n    2\r\n    3\r\n}\r\n");
        assert_eq!(n, 1);
    }

    /// `"".replace` would interleave `new` between every character.
    #[test]
    fn an_empty_old_string_is_refused() {
        let err = apply_edits("abc\n", &edits(&[("", "X", true)])).unwrap_err();
        assert!(err.to_string().contains("must not be empty"), "{err}");
    }

    #[test]
    fn edits_apply_in_order_and_see_each_other() {
        let (out, n) =
            apply_edits("a b c\n", &edits(&[("a", "b", false), ("b", "z", true)])).unwrap();
        assert_eq!(out, "z z c\n");
        assert_eq!(n, 3);
    }

    /// A miss in the second edit must not leave the first one applied: the
    /// caller writes nothing on `Err`.
    #[test]
    fn a_failing_edit_yields_no_text_at_all() {
        let err =
            apply_edits("a\n", &edits(&[("a", "b", false), ("nope", "c", false)])).unwrap_err();
        assert!(err.to_string().contains("edits[1]"), "{err}");
    }

    #[test]
    fn crlf_endings_pass_through_untouched() {
        let (out, _) = apply_edits("one\r\ntwo\r\n", &edits(&[("two", "TWO", false)])).unwrap();
        assert_eq!(out, "one\r\nTWO\r\n");
    }
}

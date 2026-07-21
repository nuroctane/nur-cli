use super::sandbox;
use super::{arg_str, Tool, ToolContext};
use crate::error::{MuseError, Result};
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
            .ok_or_else(|| MuseError::Tool("edits array required".into()))?;
        if edits.is_empty() {
            return Err(MuseError::Tool("edits empty".into()));
        }
        let mut content = fs::read_to_string(&full)
            .map_err(|e| MuseError::Tool(format!("read {}: {e}", full.display())))?;
        let mut total = 0usize;
        for (i, ed) in edits.iter().enumerate() {
            let old = ed
                .get("old_string")
                .and_then(|v| v.as_str())
                .ok_or_else(|| MuseError::Tool(format!("edits[{i}].old_string required")))?;
            let new = ed
                .get("new_string")
                .and_then(|v| v.as_str())
                .ok_or_else(|| MuseError::Tool(format!("edits[{i}].new_string required")))?;
            let replace_all = ed
                .get("replace_all")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let count = content.matches(old).count();
            if count == 0 {
                return Err(MuseError::Tool(format!("edits[{i}]: old_string not found")));
            }
            if count > 1 && !replace_all {
                return Err(MuseError::Tool(format!(
                    "edits[{i}]: old_string matched {count} times; set replace_all=true or make unique"
                )));
            }
            content = if replace_all {
                content.replace(old, new)
            } else {
                content.replacen(old, new, 1)
            };
            total += if replace_all { count } else { 1 };
        }
        fs::write(&full, content)
            .map_err(|e| MuseError::Tool(format!("write {}: {e}", full.display())))?;
        Ok(format!(
            "multi_edit {} — {total} replacement(s) in {} edit(s)",
            full.display(),
            edits.len()
        ))
    }
}

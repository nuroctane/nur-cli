use super::{arg_str, resolve_path, Tool, ToolContext};
use crate::error::{MuseError, Result};
use serde_json::Value;
use std::fs;

pub struct EditFile;

impl Tool for EditFile {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn description(&self) -> &str {
        "Replace an exact string in a file. old_string must match uniquely unless replace_all is true."
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
        let content = fs::read_to_string(&full)
            .map_err(|e| MuseError::Tool(format!("read {}: {e}", full.display())))?;

        let count = content.matches(&old).count();
        if count == 0 {
            return Err(MuseError::Tool("old_string not found in file".into()));
        }
        if count > 1 && !replace_all {
            return Err(MuseError::Tool(format!(
                "old_string matched {count} times; set replace_all=true or make old_string unique"
            )));
        }

        let updated = if replace_all {
            content.replace(&old, &new)
        } else {
            content.replacen(&old, &new, 1)
        };
        fs::write(&full, updated)
            .map_err(|e| MuseError::Tool(format!("write {}: {e}", full.display())))?;
        Ok(format!(
            "edited {} ({} replacement{})",
            full.display(),
            if replace_all { count } else { 1 },
            if replace_all && count != 1 { "s" } else { "" }
        ))
    }
}

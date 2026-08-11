use super::{arg_str, resolve_path, Tool, ToolContext};
use crate::error::{NurError, Result};
use serde_json::Value;
use std::fs;
use std::path::Path;

pub struct WriteFile;

impl Tool for WriteFile {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Create or overwrite a file with the given contents. Creates parent directories as needed."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "contents": {"type": "string"}
            },
            "required": ["path", "contents"]
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let path = arg_str(args, "path")?;
        let contents = arg_str(args, "contents")?;
        // Shepherd retained-output mode: stage instead of mutating workspace.
        if crate::config::load_config()
            .map(|c| c.proposal_mode)
            .unwrap_or(false)
        {
            let sid = std::env::var("NUR_SESSION_ID").unwrap_or_else(|_| "default".into());
            let entry =
                crate::agent::proposal::stage_write(&sid, &ctx.cwd, &path, &contents, "write_file")
                    .map_err(NurError::Tool)?;
            return Ok(format!(
                "proposal staged `{}` ({} bytes) at {} — not written to workspace. \
                 Use tool `proposal` action=list|apply|discard.",
                entry.rel_path, entry.bytes, entry.staged_path
            ));
        }
        let full = resolve_path(&ctx.cwd, &path)?;
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| NurError::Tool(format!("mkdir {}: {e}", parent.display())))?;
        }
        fs::write(&full, contents)
            .map_err(|e| NurError::Tool(format!("write {}: {e}", full.display())))?;
        Ok(format!("wrote {}", display_rel(&ctx.cwd, &full)))
    }
}

fn display_rel(cwd: &Path, full: &Path) -> String {
    full.strip_prefix(cwd)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| full.display().to_string())
}

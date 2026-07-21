use super::{arg_str, arg_u64, resolve_path, Tool, ToolContext};
use crate::error::{MuseError, Result};
use serde_json::Value;
use std::fs;

pub struct ReadFile;

impl Tool for ReadFile {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read a file from the workspace. Optionally limit to a line range (1-indexed)."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path relative to cwd or absolute"},
                "offset": {"type": "integer", "description": "Start line (1-indexed)"},
                "limit": {"type": "integer", "description": "Max lines to return"}
            },
            "required": ["path"]
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let path = arg_str(args, "path")?;
        let full = resolve_path(&ctx.cwd, &path)?;
        if !full.exists() {
            return Err(MuseError::Tool(format!(
                "file not found: {}",
                full.display()
            )));
        }
        let content = fs::read_to_string(&full)
            .map_err(|e| MuseError::Tool(format!("read {}: {e}", full.display())))?;

        // Cap very large files
        const MAX_CHARS: usize = 200_000;
        let content = if content.len() > MAX_CHARS {
            format!(
                "{}\n\n… truncated ({} bytes total, showing first {})",
                &content[..MAX_CHARS],
                content.len(),
                MAX_CHARS
            )
        } else {
            content
        };

        let offset = arg_u64(args, "offset").unwrap_or(1).max(1) as usize;
        let limit = arg_u64(args, "limit").map(|l| l as usize);

        let lines: Vec<&str> = content.lines().collect();
        let start = offset.saturating_sub(1).min(lines.len());
        let end = match limit {
            Some(l) => (start + l).min(lines.len()),
            None => lines.len(),
        };

        let mut out = String::new();
        for (i, line) in lines[start..end].iter().enumerate() {
            out.push_str(&format!("{:>6}|{}\n", start + i + 1, line));
        }
        if out.is_empty() {
            out = String::from("(empty file)");
        }
        Ok(out)
    }
}

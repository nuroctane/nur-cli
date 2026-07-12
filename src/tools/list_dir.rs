//! Directory listing — cheaper and more precise than shelling out to `ls`.

use super::{arg_str, Tool, ToolContext};
use crate::error::{MuseError, Result};
use serde_json::Value;

pub struct ListDir;

const MAX_ENTRIES: usize = 500;

impl Tool for ListDir {
    fn name(&self) -> &str {
        "list_dir"
    }

    fn description(&self) -> &str {
        "List a directory (non-recursive): name, kind, size. \
         Directories first, then files, alphabetical. Defaults to the workspace root."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Directory (workspace-relative; default \".\")"}
            }
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let path = arg_str(args, "path").unwrap_or_else(|_| ".".into());
        let full = super::resolve_path(&ctx.cwd, &path)?;
        if !full.is_dir() {
            return Err(MuseError::Tool(format!(
                "not a directory: {}",
                full.display()
            )));
        }

        let mut dirs: Vec<String> = Vec::new();
        let mut files: Vec<(String, u64)> = Vec::new();
        let entries =
            std::fs::read_dir(&full).map_err(|e| MuseError::Tool(format!("read_dir: {e}")))?;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            match entry.file_type() {
                Ok(t) if t.is_dir() => dirs.push(name),
                Ok(_) => {
                    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    files.push((name, size));
                }
                Err(_) => files.push((name, 0)),
            }
        }
        dirs.sort_unstable();
        files.sort_unstable_by(|a, b| a.0.cmp(&b.0));

        let total = dirs.len() + files.len();
        let mut out = format!("{} — {} entries\n", full.display(), total);
        let mut shown = 0usize;
        for d in &dirs {
            if shown >= MAX_ENTRIES {
                break;
            }
            out.push_str(&format!("  {d}/\n"));
            shown += 1;
        }
        for (f, size) in &files {
            if shown >= MAX_ENTRIES {
                break;
            }
            out.push_str(&format!("  {f}  ({})\n", fmt_size(*size)));
            shown += 1;
        }
        if total > shown {
            out.push_str(&format!("  … +{} more entries\n", total - shown));
        }
        Ok(out)
    }
}

fn fmt_size(n: u64) -> String {
    if n >= 1_048_576 {
        format!("{:.1} MB", n as f64 / 1_048_576.0)
    } else if n >= 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

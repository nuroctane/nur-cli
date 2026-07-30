use super::{arg_str, arg_u64, Tool, ToolContext};
use crate::error::{MuseError, Result};
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader};

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
        let full = super::sandbox::resolve_for_read(&ctx.cwd, &path)?;
        if !full.exists() {
            return Err(MuseError::Tool(format!(
                "file not found: {}",
                full.display()
            )));
        }

        let offset = arg_u64(args, "offset").unwrap_or(1).max(1) as usize;
        let limit = arg_u64(args, "limit").map(|l| l as usize);
        // Cap returned body size (chars/bytes of joined lines), not the whole file first.
        const MAX_OUT_BYTES: usize = 200_000;

        let file = fs::File::open(&full)
            .map_err(|e| MuseError::Tool(format!("read {}: {e}", full.display())))?;
        let reader = BufReader::new(file);

        let start = offset.saturating_sub(1);
        let end = match limit {
            Some(l) => start.saturating_add(l),
            None => usize::MAX,
        };

        let mut out = String::new();
        let mut truncated = false;
        for (i, line_res) in reader.lines().enumerate() {
            if i < start {
                continue;
            }
            if i >= end {
                break;
            }
            let line = line_res
                .map_err(|e| MuseError::Tool(format!("read {}: {e}", full.display())))?;
            let numbered = format!("{:>6}|{}\n", i + 1, line);
            if out.len().saturating_add(numbered.len()) > MAX_OUT_BYTES {
                truncated = true;
                break;
            }
            out.push_str(&numbered);
        }

        if out.is_empty() && !truncated {
            out = String::from("(empty file)");
        } else if truncated {
            out.push_str(
                "\n… truncated (hit 200k output cap; raise offset or lower limit)\n",
            );
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolContext;
    use tokio_util::sync::CancellationToken;

    #[test]
    fn large_file_with_multibyte_at_cut_does_not_panic() {
        let dir = std::env::temp_dir().join(format!("nur_readfile_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("big.txt");
        let mut content = String::new();
        while content.len() < 200_050 {
            content.push('─');
        }
        std::fs::write(&file, &content).unwrap();

        let tool = ReadFile;
        let args = serde_json::json!({ "path": file.to_string_lossy() });
        let ctx = ToolContext {
            cwd: dir.clone(),
            cancel: CancellationToken::new(),
        };
        let out = tool
            .execute(&args, &ctx)
            .expect("read_file must not panic on multibyte cut");
        assert!(out.contains("truncated") || out.contains('─'));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn offset_reads_beyond_first_chunk() {
        let dir = std::env::temp_dir().join(format!("nur_readfile_off_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("lines.txt");
        let mut body = String::new();
        for i in 1..=50 {
            body.push_str(&format!("line-{i}\n"));
        }
        std::fs::write(&file, &body).unwrap();
        let tool = ReadFile;
        let args = serde_json::json!({ "path": file.to_string_lossy(), "offset": 40, "limit": 5 });
        let ctx = ToolContext {
            cwd: dir.clone(),
            cancel: CancellationToken::new(),
        };
        let out = tool.execute(&args, &ctx).unwrap();
        assert!(out.contains("line-40"), "{out}");
        assert!(out.contains("line-44"), "{out}");
        assert!(!out.contains("line-1\n") && !out.contains("|line-1\n"), "{out}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

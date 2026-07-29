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

        // Cap very large files. Slice on a CHAR boundary, not a raw byte index —
        // `content.len()` is bytes, and a fixed byte cut can land inside a
        // multi-byte UTF-8 sequence (e.g. `─` is 3 bytes), which panics the
        // whole tool worker thread with "not a char boundary".
        const MAX_BYTES: usize = 200_000;
        let content = if content.len() > MAX_BYTES {
            // Walk back to the nearest char boundary at or before MAX_BYTES.
            let mut cut = MAX_BYTES;
            while cut > 0 && !content.is_char_boundary(cut) {
                cut -= 1;
            }
            format!(
                "{}\n\n… truncated ({} bytes total, showing first {})",
                &content[..cut],
                content.len(),
                cut
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolContext;
    use tokio_util::sync::CancellationToken;

    /// Regression: a file larger than MAX_BYTES whose byte-200000 boundary lands
    /// inside a multi-byte UTF-8 char (e.g. `─`, 3 bytes) must not panic. Before
    /// the fix, `&content[..200_000]` panicked the whole tool worker thread with
    /// "not a char boundary", bleeding stderr over the TUI.
    #[test]
    fn large_file_with_multibyte_at_cut_does_not_panic() {
        let dir = std::env::temp_dir().join(format!("nur_readfile_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("big.txt");
        // Fill past 200_000 bytes with a 3-byte char so *some* char straddles
        // the byte-200000 boundary regardless of exact alignment.
        let mut content = String::new();
        while content.len() < 200_050 {
            content.push('─'); // U+2500, 3 bytes
        }
        std::fs::write(&file, &content).unwrap();

        let tool = ReadFile;
        let args = serde_json::json!({ "path": file.to_string_lossy() });
        let ctx = ToolContext {
            cwd: dir.clone(),
            cancel: CancellationToken::new(),
        };
        // Must return Ok — the whole point is that it does not panic.
        let out = tool
            .execute(&args, &ctx)
            .expect("read_file must not panic on multibyte cut");
        assert!(out.contains("truncated"), "large file should be truncated");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

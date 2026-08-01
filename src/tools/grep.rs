use super::search_util::{is_hard_excluded, rg_grep, walk_builder, SEARCH_BUDGET};
use super::{arg_str, arg_u64, resolve_path, Tool, ToolContext};
use crate::error::{NurError, Result};
use regex::RegexBuilder;
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::time::Instant;

pub struct Grep;

impl Tool for Grep {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Search file contents with a regex (uses ripgrep when available; respects .gitignore; skips node_modules/target/etc.)."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string"},
                "path": {"type": "string", "description": "Directory or file to search (default cwd)"},
                "case_insensitive": {"type": "boolean", "default": false},
                "max_matches": {"type": "integer", "default": 50}
            },
            "required": ["pattern"]
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let pattern = arg_str(args, "pattern")?;
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let case_insensitive = args
            .get("case_insensitive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let max_matches = arg_u64(args, "max_matches").unwrap_or(50) as usize;
        let max_matches = max_matches.clamp(1, 500);

        let root = resolve_path(&ctx.cwd, path)?;

        // Fast path: system ripgrep (what Claude Code / Cursor / Codex rely on).
        if let Some(out) = rg_grep(&ctx.cwd, &pattern, &root, case_insensitive, max_matches) {
            if out.is_empty() {
                return Ok("no matches".into());
            }
            return Ok(out);
        }

        // Fallback: parallel ignore walk + line scan, hard excludes + budget.
        let re = RegexBuilder::new(&pattern)
            .case_insensitive(case_insensitive)
            .build()
            .map_err(|e| NurError::Tool(format!("invalid regex: {e}")))?;

        let start = Instant::now();
        let mut matches = Vec::new();

        if root.is_file() {
            search_file(&root, &re, &mut matches, max_matches)?;
        } else {
            let walker = walk_builder(&root).build();
            for entry in walker.flatten() {
                if start.elapsed() > SEARCH_BUDGET {
                    matches.push(format!(
                        "[search stopped after {}ms budget — narrow path or install ripgrep]",
                        SEARCH_BUDGET.as_millis()
                    ));
                    break;
                }
                if matches.len() >= max_matches {
                    break;
                }
                let p = entry.path();
                if !p.is_file() || is_hard_excluded(p) {
                    continue;
                }
                let _ = search_file(p, &re, &mut matches, max_matches);
            }
        }

        if matches.is_empty() {
            return Ok("no matches".into());
        }
        Ok(matches.join("\n"))
    }
}

fn search_file(
    path: &Path,
    re: &regex::Regex,
    matches: &mut Vec<String>,
    max: usize,
) -> Result<()> {
    // Skip huge / binary-ish files quickly
    if let Ok(meta) = fs::metadata(path) {
        if meta.len() > super::search_util::MAX_FILE_BYTES {
            return Ok(());
        }
    }
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(_) => return Ok(()),
    };
    // NUL byte ⇒ binary
    if bytes.contains(&0) {
        return Ok(());
    }
    let content = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return Ok(()),
    };
    for (i, line) in content.lines().enumerate() {
        if matches.len() >= max {
            break;
        }
        if re.is_match(line) {
            matches.push(format!("{}:{}:{}", path.display(), i + 1, line));
        }
    }
    Ok(())
}

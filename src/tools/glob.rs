use super::search_util::{
    fallback_file_filter, glob_for_pattern, is_hard_excluded, rg_files, walk_builder, RgOutcome,
    SEARCH_BUDGET,
};
use super::{arg_str, arg_u64, resolve_path, Tool, ToolContext};
use crate::error::{NurError, Result};
use serde_json::Value;
use std::time::Instant;

pub struct GlobTool;

const DEFAULT_MAX_RESULTS: usize = 200;
const MAX_MAX_RESULTS: usize = 1000;

impl Tool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        "Find files by name or glob (ripgrep --files when available; respects .gitignore; skips \
         node_modules/target/etc. at any depth; hidden files included). Paths are sorted, \
         forward-slashed. `src/` lists everything under a src directory, `mod.rs` finds that \
         file anywhere, `*.rs` / `**/*.test.ts` are globs. A '[truncated …]' trailer means more \
         files matched."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "e.g. *.rs, **/mod.rs, src/, Cargo.toml"},
                "path": {"type": "string", "description": "Root directory (default cwd)"},
                "max_results": {"type": "integer", "default": DEFAULT_MAX_RESULTS, "description": format!("Cap on paths returned (max {MAX_MAX_RESULTS})")}
            },
            "required": ["pattern"]
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let pattern = arg_str(args, "pattern")?;
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let max = (arg_u64(args, "max_results").unwrap_or(DEFAULT_MAX_RESULTS as u64) as usize)
            .clamp(1, MAX_MAX_RESULTS);
        let root = resolve_path(&ctx.cwd, path)?;

        match rg_files(&ctx.cwd, &pattern, &root, max) {
            RgOutcome::Hits { text, truncated } => {
                if text.is_empty() {
                    return Ok("no files matched".into());
                }
                return Ok(finish(text, truncated, max));
            }
            RgOutcome::Error(msg) => return Err(NurError::Tool(format!("glob: {msg}"))),
            RgOutcome::Unavailable => {}
        }

        // Fallback walk with the same glob the rg path would have used, so the
        // two agree on what `src/` or `mod.rs` means.
        let glob = glob_for_pattern(&pattern);
        let filter = fallback_file_filter(&root, Some(&glob), None).map_err(NurError::Tool)?;
        let start = Instant::now();
        let mut hits = Vec::new();
        let mut note: Option<String> = None;
        let mut truncated = false;
        let mut b = walk_builder(&root);
        if let Some(ov) = filter {
            b.overrides(ov);
        }
        for entry in b.build().flatten() {
            if start.elapsed() > SEARCH_BUDGET {
                note = Some(format!(
                    "search stopped after {}ms budget - install ripgrep for speed",
                    SEARCH_BUDGET.as_millis()
                ));
                break;
            }
            let p = entry.path();
            if !p.is_file() || is_hard_excluded(p) {
                continue;
            }
            if hits.len() >= max {
                truncated = true;
                break;
            }
            hits.push(p.to_string_lossy().replace('\\', "/"));
        }

        if hits.is_empty() {
            return Ok(match note {
                Some(n) => format!("no files matched\n[{n}]"),
                None => "no files matched".into(),
            });
        }
        hits.sort();
        let mut out = finish(hits.join("\n"), truncated, max);
        if let Some(n) = note {
            out.push_str(&format!("\n[{n}]"));
        }
        Ok(out)
    }
}

fn finish(text: String, truncated: bool, max: usize) -> String {
    if truncated {
        format!("{text}\n[truncated at {max} files - narrow the pattern/path or raise max_results]")
    } else {
        text
    }
}

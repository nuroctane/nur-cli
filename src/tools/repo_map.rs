//! `repo_map`: a ranked, symbol-dense map of the repository.
//!
//! Aider's repo map ranks files by the symbols the model's next edit is likely
//! to touch. Without a tree-sitter dependency this uses a conservative line
//! heuristic (declaration keywords) over the same hard exclusions the search
//! tools apply, ranks files by symbol count, and prints each file's
//! signatures. It is a triage view: enough to decide where to read, never a
//! substitute for `read_file`.

use super::{arg_u64, Tool, ToolContext};
use crate::error::{NurError, Result};
use serde_json::Value;
use std::path::Path;

const SCAN_CAP: usize = 4000;
const MAX_FILES_CAP: u64 = 200;
const PER_FILE_CAP: u64 = 20;
const LINE_CAP: usize = 160;

const CODE_EXTS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "py", "go", "c", "h", "cc", "cpp", "hpp", "java", "kt", "rb",
    "php", "cs", "swift", "scala", "lua", "sh", "ps1",
];

pub struct RepoMap;

impl Tool for RepoMap {
    fn name(&self) -> &str {
        "repo_map"
    }

    fn description(&self) -> &str {
        "Ranked map of the repository: files with the most declarations first, each with its \
         function/type signatures. Use it to triage where to read before editing; it is not a \
         substitute for read_file. Skips node_modules/vendor/target and other build trees."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Subdirectory to map (default: cwd)"},
                "max_files": {"type": "integer", "default": 60},
                "per_file": {"type": "integer", "default": 8}
            }
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let rel = args
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let root = if rel.is_empty() {
            ctx.cwd.clone()
        } else {
            super::resolve_path(&ctx.cwd, &rel)?
        };
        let max_files = arg_u64(args, "max_files")
            .unwrap_or(60)
            .clamp(1, MAX_FILES_CAP) as usize;
        let per_file = arg_u64(args, "per_file")
            .unwrap_or(8)
            .clamp(1, PER_FILE_CAP) as usize;

        let decl = regex::Regex::new(
            r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?(?:fn|def|class|struct|enum|trait|impl|interface|type|const|static|function|module|macro_rules!)\b",
        )
        .map_err(|e| NurError::Tool(format!("repo_map: {e}")))?;

        let mut files: Vec<(usize, usize, std::path::PathBuf, Vec<String>)> = Vec::new();
        let mut scanned = 0usize;
        for entry in ignore::WalkBuilder::new(&root)
            .hidden(false)
            .git_ignore(true)
            .build()
            .flatten()
        {
            scanned += 1;
            if scanned > SCAN_CAP * 4 {
                break;
            }
            let path = entry.path();
            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if !CODE_EXTS.contains(&ext.as_str()) {
                continue;
            }
            // Hard excludes even when not gitignored.
            if path.components().any(|c| {
                super::search_util::HARD_EXCLUDES
                    .iter()
                    .any(|d| c.as_os_str() == *d)
            }) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(path) else {
                continue;
            };
            let mut sigs: Vec<String> = Vec::new();
            for line in text.lines() {
                if sigs.len() >= per_file {
                    break;
                }
                if decl.is_match(line) {
                    let trimmed = line.trim_end();
                    if trimmed.len() > LINE_CAP {
                        let cut = trimmed
                            .char_indices()
                            .take_while(|(i, _)| *i < LINE_CAP)
                            .last()
                            .map(|(i, c)| i + c.len_utf8())
                            .unwrap_or(LINE_CAP);
                        sigs.push(format!("{}…", &trimmed[..cut]));
                    } else {
                        sigs.push(trimmed.to_string());
                    }
                }
            }
            if sigs.is_empty() {
                continue;
            }
            let rel_path = path
                .strip_prefix(&ctx.cwd)
                .map(Path::to_path_buf)
                .unwrap_or_else(|_| path.to_path_buf());
            files.push((sigs.len(), text.len(), rel_path, sigs));
            if files.len() >= SCAN_CAP {
                break;
            }
        }
        if files.is_empty() {
            return Ok(format!(
                "no code files with declarations found under {}",
                root.display()
            ));
        }
        files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.2.cmp(&b.2)));
        let total = files.len();
        let mut out = format!(
            "{} file(s) with declarations under {} (top {} by symbol count):\n",
            total,
            root.display(),
            total.min(max_files)
        );
        for (symbols, bytes, path, sigs) in files.iter().take(max_files) {
            out.push_str(&format!(
                "\n{} - {symbols} symbol(s), {:.1} KB\n",
                path.display(),
                *bytes as f64 / 1024.0
            ));
            for sig in sigs {
                out.push_str(&format!("  {sig}\n"));
            }
        }
        Ok(out.trim_end().to_string())
    }
}

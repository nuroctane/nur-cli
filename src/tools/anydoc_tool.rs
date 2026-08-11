//! Document → markdown via Firecrawl anydoc (Rust crate) or CLI fallback.
//!
//! Primary source: https://github.com/firecrawl/anydoc (nickscamara / Firecrawl).
//! Converts office formats to GFM for RLM context_store ingestion.

use super::{arg_str, Tool, ToolContext};
use crate::agent::context_store;
use crate::error::{NurError, Result};
use serde_json::Value;
use std::path::Path;
use std::process::Command;

pub struct AnydocTool;

impl Tool for AnydocTool {
    fn name(&self) -> &str {
        "anydoc"
    }

    fn description(&self) -> &str {
        "Convert PDF/DOCX/PPTX/XLSX/ODT/RTF/EPUB/CSV (and more) to clean GitHub-Flavored \
         Markdown via Firecrawl anydoc (local, fast). Optionally register the result in \
         the RLM context store. Actions: convert (default). Scanned-image PDFs may need \
         Firecrawl hosted /parse OCR - this tool is local text extraction only."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Workspace-relative or absolute document path"},
                "register": {
                    "type": "boolean",
                    "description": "If true, register markdown in context store (default true for large docs)",
                    "default": true
                },
                "name": {"type": "string", "description": "Optional context var name when registering"},
                "max_chars": {
                    "type": "integer",
                    "description": "Max markdown chars to return inline (default 12000); full text still registered when register=true"
                }
            },
            "required": ["path"]
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let path = arg_str(args, "path")?;
        let resolved = super::resolve_path(&ctx.cwd, &path)?;
        if !resolved.is_file() {
            return Err(NurError::Tool(format!(
                "not a file: {}",
                resolved.display()
            )));
        }
        let md = convert_path(&resolved)?;
        let register = args
            .get("register")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let max_chars = args
            .get("max_chars")
            .and_then(|v| v.as_u64())
            .unwrap_or(12_000) as usize;

        let session_id = std::env::var("NUR_SESSION_ID").unwrap_or_else(|_| "default".into());
        let mut header = format!(
            "anydoc converted {} → {} markdown chars\n",
            resolved.display(),
            md.chars().count()
        );

        if register && !md.is_empty() {
            let name = args
                .get("name")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| {
                    let stem = resolved
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("doc");
                    format!("doc_{stem}")
                });
            match context_store::register(&session_id, &name, &md, "document", "anydoc") {
                Ok(v) => header.push_str(&format!(
                    "registered context var `{}` ({} chars). Use context peek/slice/search.\n",
                    v.name, v.char_count
                )),
                Err(e) => header.push_str(&format!("context register skipped: {e}\n")),
            }
        }

        let body: String = md.chars().take(max_chars).collect();
        let more = if md.chars().count() > max_chars {
            format!(
                "\n… [inline truncated {} -> {} chars; full text in context store if registered]",
                md.chars().count(),
                max_chars
            )
        } else {
            String::new()
        };
        Ok(format!("{header}---\n{body}{more}"))
    }
}

fn convert_path(path: &Path) -> Result<String> {
    #[cfg(feature = "anydoc")]
    {
        match convert_with_crate(path) {
            Ok(md) => return Ok(md),
            Err(e) => {
                // Fall through to CLI - crate may lack a format.
                tracing::debug!(error = %e, "anydoc crate convert failed; trying CLI");
            }
        }
    }
    convert_with_cli(path)
}

#[cfg(feature = "anydoc")]
fn convert_with_crate(path: &Path) -> Result<String> {
    // anydoc 0.1.x public API (firecrawl/anydoc lib.rs):
    // `pub fn to_markdown(path) -> Result<String, ConvertError>`
    // Format detected from bytes; extension is fallback for CSV.
    // PDF uses pdf-inspector; scanned/OCR PDFs error as unsupported.
    anydoc::to_markdown(path).map_err(|e| NurError::Tool(format!("anydoc: {e}")))
}

fn convert_with_cli(path: &Path) -> Result<String> {
    // Try common CLI entrypoints.
    for bin in ["anydoc", "firecrawl-anydoc"] {
        let out = Command::new(bin).arg(path.as_os_str()).output();
        if let Ok(o) = out {
            if o.status.success() {
                let s = String::from_utf8_lossy(&o.stdout).to_string();
                if !s.trim().is_empty() {
                    return Ok(s);
                }
            }
        }
    }
    #[cfg(feature = "anydoc")]
    {
        Err(NurError::Tool(format!(
            "anydoc failed for {} (crate + CLI). Is the format supported?",
            path.display()
        )))
    }
    #[cfg(not(feature = "anydoc"))]
    {
        Err(NurError::Tool(format!(
            "anydoc not available: rebuild nur with --features anydoc, or install the anydoc CLI on PATH. path={}",
            path.display()
        )))
    }
}

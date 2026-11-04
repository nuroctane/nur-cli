//! The `lsp` tool: one tool, one op enum (diagnostics | hover | definition |
//! references | symbols | stop) over the native LSP client in `crate::lsp`.

use super::{arg_str, Tool, ToolContext};
use crate::error::Result;
use serde_json::Value;

pub struct Lsp;

impl Tool for Lsp {
    fn name(&self) -> &str {
        "lsp"
    }

    fn description(&self) -> &str {
        "Language-server operations for a file: diagnostics, hover, definition, references, \
         document symbols, or stop the server. Servers spawn lazily on first use for a \
         language (rust-analyzer, typescript-language-server, pyright, gopls - detected on \
         PATH; [lsp] servers overrides) and die when stopped. After edits, diagnostics are \
         appended automatically when a server is already running. Cold starts can be slow \
         (rust-analyzer ~30s); retry once on a timeout."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "op": {
                    "type": "string",
                    "enum": ["diagnostics", "hover", "definition", "references", "symbols", "stop"],
                    "description": "Operation to run. Most ops need line/character (0-based)."
                },
                "path": {"type": "string", "description": "File to operate on (workspace-relative)"},
                "line": {"type": "integer", "description": "0-based line for position ops"},
                "character": {"type": "integer", "description": "0-based column for position ops"},
                "wait_ms": {
                    "type": "integer",
                    "default": 1500,
                    "description": "How long diagnostics waits for the server to publish (max 10000)"
                }
            },
            "required": ["op", "path"]
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let op = arg_str(args, "op")?;
        let path = arg_str(args, "path")?;
        let full = super::sandbox::resolve_in_workspace(&ctx.cwd, &path)?;
        crate::lsp::run_op(&op, &full, args)
    }
}

/// Register `[lsp] servers` from config at startup (called once from main).
pub fn init_from_config(cfg: &crate::config::LspConfig) {
    crate::lsp::set_prewarm(cfg.prewarm);
    let mut map = std::collections::HashMap::new();
    for (ext, command) in &cfg.servers {
        // Simple whitespace split: LSP server commands are plain argv
        // ("rust-analyzer", "pyright-langserver --stdio"). Quoting is not
        // supported and not needed for these commands.
        let words: Vec<String> = command.split_whitespace().map(str::to_string).collect();
        map.insert(ext.clone(), words);
    }
    crate::lsp::set_config_overrides(if map.is_empty() { None } else { Some(map) });
}

/// Where the edit tools should report diagnostics from, given the edited
/// file's path. Thin wrapper so tools do not reach into crate::lsp directly.
pub fn post_edit_hook(full: &std::path::Path) -> String {
    crate::lsp::diagnostics_after_edit(full)
}

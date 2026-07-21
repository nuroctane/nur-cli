//! Oh My Pi backend delegation — https://omp.sh · https://github.com/can1357/oh-my-pi
//!
//! omp is a coding agent (fork of Mario Zechner's Pi) with a ~55k-line Rust
//! core: LSP-wired edits, a real debugger (DAP), AST rewrites, hashline
//! patches, and 25-provider web search. We integrate the *backend* entry
//! point — headless one-shot runs (`omp -p …`) in the workspace — not the
//! IDE/ACP surface. The CLI is auto-provisioned by `nur ecosystem ensure`
//! when Bun is available.

use super::{arg_str, Tool, ToolContext};
use crate::ecosystem;
use crate::error::{MuseError, Result};
use serde_json::Value;

pub struct OmpTool;

/// Only `status` / `version` are read-only; `run` hands the workspace to a
/// full coding agent that can edit files and execute commands.
pub fn is_read_only_action(args: &str) -> bool {
    let action = serde_json::from_str::<Value>(args)
        .ok()
        .and_then(|v| v.get("action")?.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "run".into());
    matches!(action.as_str(), "status" | "version")
}

impl Tool for OmpTool {
    fn name(&self) -> &str {
        "omp"
    }

    fn description(&self) -> &str {
        "Delegate a focused coding task to the Oh My Pi agent backend (omp.sh) as a \
         headless one-shot run in the workspace. Strongest at LSP-backed refactors \
         (rename with re-exports), debugger-driven diagnosis (DAP), AST rewrites, and \
         deep web research with citations. action=run|status|version. `run` executes \
         code and edits files — treat it like a write tool. Requires the omp CLI \
         (auto-provisioned when Bun is installed)."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["run", "status", "version"],
                    "default": "run"
                },
                "prompt": {
                    "type": "string",
                    "description": "For run: the complete task for the omp agent"
                },
                "model": {
                    "type": "string",
                    "description": "For run: optional omp model override (e.g. provider/model-id)"
                }
            }
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let bin = ecosystem::find_bin("omp").ok_or_else(|| {
            MuseError::Tool(
                "omp CLI not found. Install Bun (bun.sh) then `nur ecosystem ensure`, \
                 or install directly: bun install -g @oh-my-pi/pi-coding-agent \
                 (Windows: irm https://omp.sh/install.ps1 | iex)"
                    .into(),
            )
        })?;

        let action = arg_str(args, "action").unwrap_or_else(|_| "run".into());
        match action.as_str() {
            "status" | "version" => ecosystem::run_capture(&bin, &["--version"], None, 30_000)
                .map(|v| format!("omp {v}"))
                .map_err(MuseError::Tool),
            "run" => {
                let prompt = arg_str(args, "prompt")?;
                let mut argv: Vec<String> = vec!["-p".into(), prompt];
                if let Ok(model) = arg_str(args, "model") {
                    if !model.trim().is_empty() {
                        argv.push("--model".into());
                        argv.push(model);
                    }
                }
                let refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
                // Full agent runs take a while — generous timeout, workspace cwd.
                ecosystem::run_capture(&bin, &refs, Some(&ctx.cwd), 600_000)
                    .map_err(MuseError::Tool)
            }
            other => Err(MuseError::Tool(format!("unknown omp action '{other}'"))),
        }
    }
}

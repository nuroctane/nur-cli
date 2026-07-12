//! Executor MCP gateway wrapper — https://executor.sh/docs

use super::{arg_str, Tool, ToolContext};
use crate::ecosystem;
use crate::error::{MuseError, Result};
use serde_json::Value;

pub struct ExecutorTool;

pub fn is_read_only_action(args: &str) -> bool {
    let action = serde_json::from_str::<Value>(args)
        .ok()
        .and_then(|v| v.get("action")?.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "status".into());
    matches!(
        action.as_str(),
        "status" | "tools" | "search" | "sources" | "help"
    )
}

impl Tool for ExecutorTool {
    fn name(&self) -> &str {
        "executor"
    }

    fn description(&self) -> &str {
        "Executor MCP gateway (executor.sh): unified catalog of OpenAPI/GraphQL/MCP \
         integrations. action=status|sources|search|call|install. \
         Prefer for external SaaS/APIs; use native Meta tools for local repo work. \
         Auto-installed with meta."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["status", "sources", "search", "call", "install", "help"],
                    "default": "status"
                },
                "query": {
                    "type": "string",
                    "description": "For search: natural-language tool query"
                },
                "namespace": {
                    "type": "string",
                    "description": "For call: integration namespace"
                },
                "tool": {
                    "type": "string",
                    "description": "For call: tool name within namespace"
                },
                "args_json": {
                    "type": "string",
                    "description": "For call: JSON object string of arguments"
                }
            }
        })
    }

    fn execute(&self, args: &Value, _ctx: &ToolContext) -> Result<String> {
        let bin = ecosystem::find_bin("executor").ok_or_else(|| {
            MuseError::Tool(
                "executor CLI not found. Meta auto-installs it — npm i -g executor \
                 then meta ecosystem ensure"
                    .into(),
            )
        })?;

        let action = arg_str(args, "action").unwrap_or_else(|_| "status".into());
        let argv: Vec<String> = match action.as_str() {
            "status" | "help" => vec!["--help".into()],
            "install" => vec!["install".into()],
            "sources" => vec!["tools".into(), "sources".into()],
            "search" => {
                let q = arg_str(args, "query")?;
                vec!["tools".into(), "search".into(), q]
            }
            "call" => {
                let ns = arg_str(args, "namespace")?;
                let tool = arg_str(args, "tool")?;
                let mut a = vec!["call".into(), ns, tool];
                if let Ok(json) = arg_str(args, "args_json") {
                    a.push(json);
                }
                a
            }
            other => {
                return Err(MuseError::Tool(format!(
                    "unknown executor action '{other}'"
                )));
            }
        };
        let refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
        ecosystem::run_capture(&bin, &refs, None, 120_000).map_err(MuseError::Tool)
    }
}

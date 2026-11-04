//! `mcp` tool: local MCP servers over stdio.
//!
//! `action=list` shows the configured servers and the tools each one exposes
//! (spawning a server lazily on first ask). `action=call` runs one tool.
//! Server definitions live under `[mcp.servers.<name>]` in config.toml.

use super::{arg_str, Tool, ToolContext};
use crate::error::{NurError, Result};
use serde_json::Value;

pub struct McpTool;

impl Tool for McpTool {
    fn name(&self) -> &str {
        "mcp"
    }

    fn description(&self) -> &str {
        "MCP servers over stdio or HTTP. action=list shows configured servers and tools; \
         action=call runs a tool; action=login opens OAuth for a remote server. Configure \
         [mcp.servers.<name>] with command/args/env or url/bearer_env in config.toml. \
         A server with read_only = true has its calls treated as read-only."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["list", "call", "login"]},
                "server": {"type": "string", "description": "Server name for action=call"},
                "url": {"type": "string", "description": "MCP server URL for action=login"},
                "tool": {"type": "string", "description": "Tool name for action=call"},
                "arguments": {"type": "object", "description": "Tool arguments for action=call"}
            },
            "required": ["action"]
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        match arg_str(args, "action")?.as_str() {
            "list" => {
                let servers = crate::mcp_stdio::configured_for(&ctx.session_id);
                if servers.is_empty() {
                    return Ok(
                        "no MCP servers configured. Add [mcp.servers.<name>] with a \
                         `command` (and `args`) to config.toml."
                            .into(),
                    );
                }
                let mut out = format!("{} MCP server(s):\n", servers.len());
                for (name, read_only, internal) in servers {
                    let tag = if read_only { " (read-only)" } else { "" };
                    match crate::mcp_stdio::tools(&internal, &ctx.cancel) {
                        Ok(tools) => {
                            out.push_str(&format!("\n{name}{tag} - {} tool(s)\n", tools.len()));
                            for tool in tools.iter().take(40) {
                                let tname = tool.get("name").and_then(Value::as_str).unwrap_or("?");
                                let desc = tool
                                    .get("description")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .lines()
                                    .next()
                                    .unwrap_or_default();
                                out.push_str(&format!("  {tname} - {desc}\n"));
                            }
                        }
                        Err(e) => out.push_str(&format!("\n{name}{tag} - unavailable: {e}\n")),
                    }
                }
                Ok(out.trim_end().to_string())
            }
            "call" => {
                let server = arg_str(args, "server")?;
                let tool = arg_str(args, "tool")?;
                let arguments = args
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));
                let internal = crate::mcp_stdio::resolve_for(&ctx.session_id, &server)
                    .ok_or_else(|| NurError::Tool(format!("mcp: unknown server '{server}'")))?;
                crate::mcp_stdio::call(&internal, &tool, &arguments, &ctx.cancel)
                    .map_err(NurError::Tool)
            }
            "login" => {
                let server = arg_str(args, "server")?;
                let url = arg_str(args, "url")?;
                crate::mcp_oauth::login(&server, &url).map_err(NurError::Tool)
            }
            other => Err(NurError::Tool(format!(
                "mcp: unknown action '{other}' - use list, call or login"
            ))),
        }
    }
}

//! `message` tool - agent-to-agent messaging (Prime agent_message port).

use super::{arg_str, Tool, ToolContext};
use crate::agent::mailbox;
use crate::error::{NurError, Result};
use serde_json::Value;

pub struct MessageTool;

fn agent_self() -> String {
    std::env::var("NUR_SESSION_ID")
        .ok()
        .filter(|s| !s.is_empty())
        .map(|s| format!("session-{}", &s[..s.len().min(8)]))
        .unwrap_or_else(|| "agent".into())
}

fn scope_from(args: &Value, ctx: &ToolContext) -> String {
    if let Some(s) = args
        .get("scope")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return s.to_string();
    }
    let proj = ctx
        .cwd
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace");
    format!("{proj}")
}

impl Tool for MessageTool {
    fn name(&self) -> &str {
        "message"
    }

    fn description(&self) -> &str {
        "Agent-to-agent messaging (Prime agent_message / Connectome): exchange \
         durable messages with other agents sharing a project scope. Actions: \
         send (to=<agent|all>) | recv (read + mark delivered) | status. Useful \
         for orchestrating siblings or leaving notes for a later session on the \
         same project."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["send", "recv", "status"]},
                "to": {"type": "string", "description": "Recipient agent name, or 'all' to broadcast"},
                "text": {"type": "string", "description": "Message body (send)"},
                "scope": {"type": "string", "description": "Optional mailbox scope (defaults to project)"}
            },
            "required": ["action"]
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action")?;
        let scope = scope_from(args, ctx);
        let me = agent_self();
        match action.as_str() {
            "send" => {
                let to = arg_str(args, "to")?;
                let text = arg_str(args, "text")?;
                let m = mailbox::send(&scope, &to, &me, &text).map_err(NurError::Tool)?;
                Ok(format!(
                    "sent to `{}` from `{}` (id {})\n{}",
                    m.to,
                    m.from,
                    m.id,
                    m.text
                ))
            }
            "recv" => match mailbox::receive(&scope, &me, true) {
                items if items.is_empty() => Ok("no messages for you in this scope".into()),
                items => {
                    let mut s = format!("{} message(s):", items.len());
                    for m in items {
                        s.push_str(&format!("\n{}", mailbox::render(&m)));
                    }
                    Ok(s)
                }
            },
            "status" => Ok(mailbox::mailbox_status(&scope)),
            other => Err(NurError::Tool(format!(
                "unknown message action `{other}`; use send|recv|status"
            ))),
        }
    }
}

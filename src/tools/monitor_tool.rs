//! Monitor tool: event-streaming background processes, modelled on
//! Claude Code's Monitor tool.
//!
//! `monitor(action=start, command=…, description=…)` runs a shell command
//! in the background and each stdout line is delivered to the model as an
//! event, the moment it lands. Use it for a live stream (build/deploy
//! logs, `tail -F` of a log file, a long test run) where polling `bg`
//! would miss the middle. `bg` stays the tool for fire-and-forget jobs
//! where one notification on completion is enough.
//!
//! Same permission rules as `bash`: the command runs under the shell.
//! A monitor is never restored on resume.

use super::{arg_str, Tool, ToolContext};
use crate::error::{NurError, Result};
use serde_json::Value;

pub fn is_read_only_action(args_json: &str) -> bool {
    if let Ok(v) = serde_json::from_str::<Value>(args_json) {
        let action = v.get("action").and_then(|a| a.as_str()).unwrap_or("list");
        return matches!(action, "list" | "status");
    }
    false
}

pub struct MonitorTool;

impl Tool for MonitorTool {
    fn name(&self) -> &str {
        "monitor"
    }

    fn description(&self) -> &str {
        "Stream a background command's stdout to the model as events, line by line. \
         action=start: run a shell command in the workspace whose stdout lines arrive as events (batched within 200ms); \
         stderr goes to a file and never triggers events (merge with 2>&1 to see failures). action=list|status|events|stop. \
         Use for live streams (build/deploy logs, tail -F, long test runs); use bg for fire-and-forget jobs. \
         Same permission rules as bash. Never restored on resume."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "description": "start|list|status|events|stop",
                    "default": "list"
                },
                "command": {
                    "type": "string",
                    "description": "For start: shell command, run under the same shell as bash"
                },
                "description": {
                    "type": "string",
                    "description": "For start: human label for the monitor"
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "For start: watch length in ms (default 300000 = 5 minutes)"
                },
                "persistent": {
                    "type": "boolean",
                    "description": "For start: no timeout, run until stopped or nur exits (default false)"
                },
                "id": {
                    "type": "integer",
                    "description": "Monitor id for status/events/stop"
                }
            }
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action").unwrap_or_else(|_| "list".into());
        match action.as_str() {
            "list" => {
                let monitors = crate::monitors::list();
                if monitors.is_empty() {
                    return Ok("no monitors".into());
                }
                let mut s = String::from("monitors:\n");
                for m in monitors {
                    s.push_str(&format!(
                        "  #{} · {} · {} · {} · timeout {}s{}\n",
                        m.id,
                        m.label,
                        m.state.as_str(),
                        m.command,
                        m.timeout_ms / 1000,
                        if m.persistent { " · persistent" } else { "" }
                    ));
                }
                Ok(s)
            }
            "status" => {
                let id = args
                    .get("id")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| NurError::Tool("status requires id=".into()))?;
                match crate::monitors::get(id) {
                    Some(m) => Ok(format!(
                        "monitor #{id}\n  label: {}\n  command: {}\n  state: {}\n  persistent: {}\n  timeout: {}ms\n  exit code: {}\n  stderr: {}\n",
                        m.label,
                        m.command,
                        m.state.as_str(),
                        m.persistent,
                        m.timeout_ms,
                        m.exit_code
                            .map(|c| c.to_string())
                            .unwrap_or_else(|| "-".into()),
                        m.stderr_path
                    )),
                    None => Err(NurError::Tool(format!("unknown monitor {id}"))),
                }
            }
            "events" => {
                let id = args
                    .get("id")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| NurError::Tool("events requires id=".into()))?;
                let events = crate::monitors::take_events(id)?;
                if events.is_empty() {
                    return Ok(format!("monitor #{id}: no new events"));
                }
                let mut s = format!("monitor #{id} events:\n");
                for batch in events {
                    for line in batch {
                        s.push_str(&format!("  {line}\n"));
                    }
                }
                Ok(s)
            }
            "stop" => {
                let id = args
                    .get("id")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| NurError::Tool("stop requires id=".into()))?;
                crate::monitors::stop(id)
            }
            "start" => {
                if crate::agent::context_store::is_child(&ctx.session_id) {
                    return Err(NurError::Tool("child agents start background commands with bash background=true, so the command keeps the child's workspace and cancellation scope".into()));
                }
                let command = arg_str(args, "command")
                    .map_err(|_| NurError::Tool("start requires command=".into()))?;
                let label = arg_str(args, "description")
                    .or_else(|_| arg_str(args, "label"))
                    .unwrap_or_else(|_| "monitor".into());
                let timeout_ms = args.get("timeout_ms").and_then(|v| v.as_u64());
                let persistent = args
                    .get("persistent")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let id = crate::monitors::start(
                    &label,
                    &command,
                    Some(&ctx.cwd),
                    timeout_ms,
                    persistent,
                    Some(ctx.session_id.clone()),
                )?;
                Ok(format!(
                    "monitor #{id} started · {label}\n  $ {command}\n  \
                     stdout lines arrive as events; keep working, they stream in. \
                     stop: monitor(action=stop, id={id})\n"
                ))
            }
            other => Err(NurError::Tool(format!(
                "unknown monitor action '{other}', use start|list|status|events|stop"
            ))),
        }
    }
}

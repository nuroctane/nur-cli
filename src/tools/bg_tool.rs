//! Background jobs tool — push long work off the agent turn.
//!
//! Agents call `bg(action=run, …)` for shell-style work, or diagram tools pass
//! `background=true` (handled by those tools via this module's helpers).

use super::{arg_str, Tool, ToolContext};
use crate::error::{NurError, Result};
use serde_json::Value;

pub fn is_read_only_action(args_json: &str) -> bool {
    if let Ok(v) = serde_json::from_str::<Value>(args_json) {
        let action = v.get("action").and_then(|a| a.as_str()).unwrap_or("list");
        return matches!(action, "list" | "status" | "result" | "chip");
    }
    false
}

pub struct Bg;

impl Tool for Bg {
    fn name(&self) -> &str {
        "bg"
    }

    fn description(&self) -> &str {
        "Background jobs — push long-running work off the agent turn so the CLI stays interactive. \
         action=run: spawn a shell command in the background (returns job id immediately). \
         action=spawn_label: register a labeled job already started by another tool. \
         action=list|status|result|cancel|chip. \
         Diagram tools (penecho install, tldraw install, long exports) accept background=true too. \
         Use when a task would block the agent for more than a few seconds. \
         TUI: /bg · status chip shows running jobs."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "description": "run|list|status|result|cancel|chip",
                    "default": "list"
                },
                "command": {
                    "type": "string",
                    "description": "For run: shell command string (Windows: cmd /C, else sh -c)"
                },
                "program": {
                    "type": "string",
                    "description": "For run: program path (alternative to command=)"
                },
                "args": {
                    "description": "For run with program=: JSON array of args"
                },
                "label": {
                    "type": "string",
                    "description": "Human label for the job"
                },
                "id": {
                    "type": "integer",
                    "description": "Job id for status/result/cancel"
                }
            }
        })
    }

    fn execute(&self, args: &Value, _ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action").unwrap_or_else(|_| "list".into());
        match action.as_str() {
            "list" | "chip" => Ok(crate::bg_jobs::report()),
            "status" => {
                let id = args
                    .get("id")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| NurError::Tool("status requires id=".into()))?;
                match crate::bg_jobs::get(id) {
                    Some(j) => Ok(format!(
                        "bg #{id}\n  label: {}\n  kind:  {}\n  state: {}\n  preview: {}\n  error: {}\n",
                        j.label,
                        j.kind,
                        j.state.as_str(),
                        j.result_preview.unwrap_or_default(),
                        j.error.unwrap_or_default()
                    )),
                    None => Err(NurError::Tool(format!("unknown bg job {id}"))),
                }
            }
            "result" => {
                let id = args
                    .get("id")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| NurError::Tool("result requires id=".into()))?;
                crate::bg_jobs::result(id)
            }
            "cancel" => {
                let id = args
                    .get("id")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| NurError::Tool("cancel requires id=".into()))?;
                crate::bg_jobs::cancel(id)
            }
            "run" => {
                let label = arg_str(args, "label").unwrap_or_else(|_| "bg command".into());
                if let Ok(program) = arg_str(args, "program") {
                    let argv: Vec<String> = args
                        .get("args")
                        .and_then(|a| a.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_default();
                    let id = crate::bg_jobs::spawn_command(&label, &program, &argv);
                    return Ok(format!(
                        "bg job #{id} started · {label}\n  program: {program}\n  \
                         continue working — later: bg(action=result, id={id})\n"
                    ));
                }
                let command = arg_str(args, "command").map_err(|_| {
                    NurError::Tool("run requires command= or program= + args=".into())
                })?;
                #[cfg(windows)]
                let (prog, argv) = ("cmd.exe".to_string(), vec!["/C".into(), command.clone()]);
                #[cfg(not(windows))]
                let (prog, argv) = ("sh".to_string(), vec!["-c".into(), command.clone()]);
                let id = crate::bg_jobs::spawn_command(&label, &prog, &argv);
                Ok(format!(
                    "bg job #{id} started · {label}\n  $ {command}\n  \
                     continue working — later: bg(action=result, id={id})  or  /bg {id}\n"
                ))
            }
            other => Err(NurError::Tool(format!(
                "unknown bg action '{other}' — use run|list|status|result|cancel"
            ))),
        }
    }
}

//! Persistent goal tool - Prime `/goal` surface.

use super::{arg_str, arg_u64, Tool, ToolContext};
use crate::agent::goal;
use crate::error::{NurError, Result};
use serde_json::Value;

pub struct GoalTool;

impl Tool for GoalTool {
    fn name(&self) -> &str {
        "goal"
    }

    fn description(&self) -> &str {
        "Persistent goal (Prime Agent pattern): durable objective across turns. \
         Actions: get | set | complete | pause | resume | clear. \
         Only goal.complete marks successful completion. Optional token_budget on set."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["get", "set", "complete", "pause", "resume", "clear"],
                },
                "text": {"type": "string", "description": "For set: goal objective"},
                "note": {"type": "string", "description": "For complete: optional note"},
                "token_budget": {"type": "integer", "description": "For set: optional token budget"},
                "session_id": {"type": "string"}
            },
            "required": ["action"]
        })
    }

    fn execute(&self, args: &Value, _ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action")?;
        let session_id = args
            .get("session_id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| std::env::var("NUR_SESSION_ID").ok())
            .unwrap_or_else(|| "default".into());

        match action.as_str() {
            "get" => match goal::load(&session_id) {
                Some(g) => Ok(goal::format_status(&g)),
                None => Ok("no goal set for this session".into()),
            },
            "set" => {
                let text = arg_str(args, "text")?;
                let budget = arg_u64(args, "token_budget");
                let g = goal::set(&session_id, &text, budget).map_err(NurError::Tool)?;
                Ok(format!("goal set (active)\n{}", goal::format_status(&g)))
            }
            "complete" => {
                let note = args
                    .get("note")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let g = goal::complete(&session_id, note).map_err(NurError::Tool)?;
                Ok(format!("goal completed\n{}", goal::format_status(&g)))
            }
            "pause" => {
                let g = goal::pause(&session_id).map_err(NurError::Tool)?;
                Ok(format!("goal paused\n{}", goal::format_status(&g)))
            }
            "resume" => {
                let g = goal::resume(&session_id).map_err(NurError::Tool)?;
                Ok(format!("goal resumed\n{}", goal::format_status(&g)))
            }
            "clear" => {
                goal::clear(&session_id).map_err(NurError::Tool)?;
                Ok("goal cleared".into())
            }
            other => Err(NurError::Tool(format!("unknown goal action `{other}`"))),
        }
    }
}

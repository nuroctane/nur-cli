//! `admission` tool - retrieve results of asynchronously admitted subagents
//! (Prime `rlm()` admission-handle model, RLM paper).

use super::{arg_str, arg_u64, Tool, ToolContext};
use crate::agent::admission;
use crate::error::{NurError, Result};
use serde_json::Value;

pub struct AdmissionTool;

fn session_id() -> String {
    std::env::var("NUR_SESSION_ID")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "default".into())
}

impl Tool for AdmissionTool {
    fn name(&self) -> &str {
        "admission"
    }

    fn description(&self) -> &str {
        "Get results of asynchronously admitted subagents (Prime rlm() / RLM \
         admission handles). A child spawned with agent async=true returns a handle \
         id immediately and runs in the background; poll here. Actions: get <id> | \
         list | all. get blocks-until-done only if you poll repeatedly; returns \
         state=running while the child works."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["get", "list"]},
                "id": {"type": "integer", "description": "Admission handle id (for get)"}
            },
            "required": ["action"]
        })
    }

    fn execute(&self, args: &Value, _ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action")?;
        let sid = session_id();
        match action.as_str() {
            "list" => {
                let list = admission::list(&sid);
                if list.is_empty() {
                    return Ok("no admitted subagents in this session".into());
                }
                let mut lines = vec![format!("{} admission(s):", list.len())];
                for a in &list {
                    let st = match a.state {
                        admission::AdmissionState::Running => "running",
                        admission::AdmissionState::Done => "done",
                        admission::AdmissionState::Failed => "failed",
                    };
                    lines.push(format!("#{} · {st} · {}", a.id, a.desc));
                }
                Ok(lines.join("\n"))
            }
            "get" => {
                let id = arg_u64(args, "id")
                    .ok_or_else(|| NurError::Tool("id required for get".into()))?;
                match admission::get(&sid, id) {
                    Some(a) => Ok(admission::render(&a)),
                    None => Err(NurError::Tool(format!(
                        "no admission #{id} in this session (list to see handles)"
                    ))),
                }
            }
            other => Err(NurError::Tool(format!(
                "unknown admission action `{other}`; use get or list"
            ))),
        }
    }
}

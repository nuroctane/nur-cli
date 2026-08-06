//! Continual harness refine / rollback tool.

use super::{arg_str, Tool, ToolContext};
use crate::agent::harness;
use crate::error::{NurError, Result};
use serde_json::Value;

pub struct HarnessTool;

impl Tool for HarnessTool {
    fn name(&self) -> &str {
        "harness"
    }

    fn description(&self) -> &str {
        "Continual harness (Prime Agent /refine lite): append evidence-backed \
         operating lessons to session supplemental state (never rewrites the base \
         system prompt). Actions: status | refine | rollback. Snapshots support rollback."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["status", "refine", "rollback"],
                },
                "lesson": {"type": "string", "description": "For refine: short lesson"},
                "evidence": {"type": "string", "description": "For refine: what trajectory supports it"},
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
            "status" => Ok(harness::status(&session_id)),
            "refine" => {
                let lesson = arg_str(args, "lesson")?;
                let evidence = args
                    .get("evidence")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let s = harness::refine(&session_id, &lesson, evidence).map_err(NurError::Tool)?;
                Ok(format!(
                    "refined harness to rev {}\n{}",
                    s.revision,
                    s.supplemental.chars().rev().take(800).collect::<String>().chars().rev().collect::<String>()
                ))
            }
            "rollback" => {
                let s = harness::rollback(&session_id).map_err(NurError::Tool)?;
                Ok(format!(
                    "rolled back harness; now rev {} ({} chars supplemental)",
                    s.revision,
                    s.supplemental.chars().count()
                ))
            }
            other => Err(NurError::Tool(format!("unknown harness action `{other}`"))),
        }
    }
}

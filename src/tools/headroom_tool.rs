use super::{arg_str, Tool, ToolContext};
use crate::error::{MuseError, Result};
use crate::headroom;
use serde_json::Value;

pub struct Headroom;

pub fn is_read_only_action(args: &str) -> bool {
    let action = serde_json::from_str::<Value>(args)
        .ok()
        .and_then(|v| v.get("action")?.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "status".into());
    matches!(
        action.as_str(),
        "status" | "doctor" | "probe" | "stats"
    )
}

impl Tool for Headroom {
    fn name(&self) -> &str {
        "headroom"
    }

    fn description(&self) -> &str {
        "Headroom context compression (https://github.com/headroomlabs-ai/headroom). \
         Inline-compresses large tool results before they hit the LLM when \
         [headroom] enabled=true (default). actions: status|doctor|compress|stats. \
         Disable with headroom.enabled=false in ~/.nur/config.toml."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["status", "doctor", "probe", "compress", "stats"],
                    "default": "status"
                },
                "text": {
                    "type": "string",
                    "description": "For compress: text to compress"
                },
                "label": {
                    "type": "string",
                    "description": "Optional label for compress (default tool)"
                }
            }
        })
    }

    fn execute(&self, args: &Value, _ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action").unwrap_or_else(|_| "status".into());
        match action.as_str() {
            "status" | "doctor" | "probe" | "stats" => Ok(headroom::doctor_report()),
            "compress" => {
                let text = arg_str(args, "text").map_err(|_| {
                    MuseError::Tool("compress requires text=".into())
                })?;
                let label = arg_str(args, "label").unwrap_or_else(|_| "tool".into());
                let cfg = crate::config::load_config()
                    .map(|c| c.headroom)
                    .unwrap_or_default();
                let model = crate::config::load_config()
                    .map(|c| c.model)
                    .unwrap_or_default();
                match headroom::compress_text(&cfg, &label, &text, &model) {
                    Some(c) => Ok(c),
                    None => Ok(
                        "compress skipped or unavailable (install headroom-ai, ensure python, \
                         or text too small / no savings)"
                            .into(),
                    ),
                }
            }
            other => Ok(format!(
                "unknown headroom action '{other}' - status|doctor|compress|stats"
            )),
        }
    }
}

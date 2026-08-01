use super::{arg_str, Tool, ToolContext};
use crate::error::{NurError, Result};
use crate::optmem;
use serde_json::Value;

pub struct OptMem;

pub fn is_read_only_action(args: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(args) else {
        return false;
    };
    let action = v.get("action").and_then(|a| a.as_str()).unwrap_or("status");
    match action {
        "status" | "doctor" | "wake" | "recall" | "zoom" => true,
        "config" => v
            .get("config_kv")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim()
            .is_empty(),
        _ => false,
    }
}

impl Tool for OptMem {
    fn name(&self) -> &str {
        "optmem"
    }

    fn description(&self) -> &str {
        "OptMem permanent memory (https://github.com/VictorTaelin/OptMem). \
         Upstream-pure under ~/.optmem. actions: status|doctor|wake|note|nap|recall|zoom|forget|config. \
         Root agents: wake at session start (auto). Subagents must not use memo. \
         note text max 280 chars; if note asks a merge, nap before continuing."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["status", "doctor", "wake", "note", "nap", "recall", "zoom", "forget", "config"],
                    "default": "status"
                },
                "text": { "type": "string", "description": "For note: one line, max 280 chars" },
                "query": { "type": "string", "description": "For recall: regex/search" },
                "range": { "type": "string", "description": "For zoom/forget: a-b node id" },
                "config_kv": { "type": "string", "description": "For config: e.g. WAKE_LINES=300" }
            }
        })
    }

    fn execute(&self, args: &Value, _ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action").unwrap_or_else(|_| "status".into());
        match action.as_str() {
            "status" | "doctor" => Ok(optmem::doctor_report()),
            "wake" => optmem::run_memo(&["wake"], 15_000).map_err(NurError::Tool),
            "note" => {
                let text = arg_str(args, "text")
                    .map_err(|_| NurError::Tool("note requires text=".into()))?;
                optmem::note(&text).map_err(NurError::Tool)
            }
            "nap" => {
                let out = optmem::run_memo(&["nap"], 120_000).map_err(NurError::Tool)?;
                optmem::invalidate_wake_cache();
                Ok(out)
            }
            "recall" => {
                let q = arg_str(args, "query")
                    .map_err(|_| NurError::Tool("recall requires query=".into()))?;
                optmem::run_memo(&["recall", &q], 30_000).map_err(NurError::Tool)
            }
            "zoom" => {
                let r = arg_str(args, "range")
                    .map_err(|_| NurError::Tool("zoom requires range=a-b".into()))?;
                optmem::run_memo(&["zoom", &r], 15_000).map_err(NurError::Tool)
            }
            "forget" => {
                let r = arg_str(args, "range")
                    .map_err(|_| NurError::Tool("forget requires range=a-b".into()))?;
                let out = optmem::run_memo(&["forget", &r], 15_000).map_err(NurError::Tool)?;
                optmem::invalidate_wake_cache();
                Ok(out)
            }
            "config" => {
                if let Ok(kv) = arg_str(args, "config_kv") {
                    let out = optmem::run_memo(&["config", &kv], 10_000).map_err(NurError::Tool)?;
                    optmem::invalidate_wake_cache();
                    Ok(out)
                } else {
                    optmem::run_memo(&["config"], 10_000).map_err(NurError::Tool)
                }
            }
            other => Ok(format!(
                "unknown optmem action '{other}' - status|wake|note|nap|recall|zoom|forget|config"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_read_only_action;

    #[test]
    fn config_read_is_ro_write_is_not() {
        assert!(is_read_only_action(r#"{"action":"config"}"#));
        assert!(!is_read_only_action(
            r#"{"action":"config","config_kv":"WAKE_LINES=10"}"#
        ));
        assert!(!is_read_only_action(r#"{"action":"note","text":"x"}"#));
        assert!(!is_read_only_action(r#"{"action":"forget","range":"a-b"}"#));
        assert!(is_read_only_action(r#"{"action":"wake"}"#));
    }
}

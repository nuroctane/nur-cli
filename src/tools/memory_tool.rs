use super::{arg_str, Tool, ToolContext};
use crate::agent::memory::{append_memory, memory_path, read_memory};
use crate::error::{NurError, Result};
use serde_json::Value;

pub struct MemoryTool;

impl Tool for MemoryTool {
    fn name(&self) -> &str {
        "memory"
    }

    fn description(&self) -> &str {
        "Cross-session memory stored in ~/.nur/memory.md. action=read|append. \
         Use for durable user preferences and project facts (never secrets)."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["read", "append"]},
                "note": {"type": "string", "description": "Required for append"}
            },
            "required": ["action"]
        })
    }

    fn execute(&self, args: &Value, _ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action")?;
        match action.as_str() {
            "read" => Ok(format!(
                "memory path: {}\n\n{}",
                memory_path().display(),
                read_memory()
            )),
            "append" => {
                let note = arg_str(args, "note")?;
                if note.trim().is_empty() {
                    return Err(NurError::Tool("note required for append".into()));
                }
                // Refuse obvious secrets
                let lower = note.to_ascii_lowercase();
                for bad in ["api_key", "password", "secret", "bearer ", "sk-", "llm_"] {
                    if lower.contains(bad) {
                        return Err(NurError::Tool(
                            "refused to store possible secret in memory".into(),
                        ));
                    }
                }
                append_memory(&note).map_err(|e| NurError::Tool(e.to_string()))?;
                Ok(format!("appended to {}", memory_path().display()))
            }
            _ => Err(NurError::Tool("action must be read or append".into())),
        }
    }
}

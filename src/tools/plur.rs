//! PLUR — local-first shared agent memory (engrams + episodes).
//! Wraps the `plur` CLI from `@plur-ai/cli`.

use super::{arg_str, Tool, ToolContext};
use crate::ecosystem;
use crate::error::{MuseError, Result};
use serde_json::Value;

pub struct Plur;

/// Read-only (or free in plan mode) PLUR actions.
pub fn is_read_only_action(args: &str) -> bool {
    let action = serde_json::from_str::<Value>(args)
        .ok()
        .and_then(|v| v.get("action")?.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "status".into());
    matches!(
        action.as_str(),
        "status" | "recall" | "inject" | "list" | "timeline"
    )
}

impl Tool for Plur {
    fn name(&self) -> &str {
        "plur"
    }

    fn description(&self) -> &str {
        "PLUR shared agent memory (local YAML engrams under ~/.plur/). \
         Persist corrections, preferences, conventions; recall/inject across sessions. \
         action=status|learn|recall|inject|list|capture|timeline|feedback|forget|ingest. \
         Prefer over ephemeral chat memory. Never store secrets. Auto-installed with meta."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["status", "learn", "recall", "inject", "list", "capture", "timeline", "feedback", "forget", "ingest"],
                    "default": "status"
                },
                "statement": {
                    "type": "string",
                    "description": "For learn: the engram text"
                },
                "query": {
                    "type": "string",
                    "description": "For recall / timeline / inject task text"
                },
                "task": {
                    "type": "string",
                    "description": "For inject: current task description"
                },
                "id": {
                    "type": "string",
                    "description": "Engram id for forget / feedback"
                },
                "signal": {
                    "type": "string",
                    "enum": ["positive", "negative", "neutral"],
                    "description": "For feedback"
                },
                "summary": {
                    "type": "string",
                    "description": "For capture: episode summary"
                },
                "content": {
                    "type": "string",
                    "description": "For ingest: free text to extract engrams from"
                },
                "scope": {
                    "type": "string",
                    "description": "Optional scope e.g. global or project:myapp"
                },
                "fast": {
                    "type": "boolean",
                    "description": "BM25-only search (default true for speed)",
                    "default": true
                }
            }
        })
    }

    fn execute(&self, args: &Value, _ctx: &ToolContext) -> Result<String> {
        let bin = ecosystem::find_bin("plur").ok_or_else(|| {
            MuseError::Tool(
                "plur CLI not found. Meta normally auto-installs it — run: \
                 npm install -g @plur-ai/cli @plur-ai/mcp"
                    .into(),
            )
        })?;

        let action = arg_str(args, "action").unwrap_or_else(|_| "status".into());
        let fast = args.get("fast").and_then(|v| v.as_bool()).unwrap_or(true);

        let mut argv: Vec<String> = Vec::new();
        match action.as_str() {
            "status" => {
                argv.push("status".into());
                argv.push("--json".into());
            }
            "learn" => {
                let statement = arg_str(args, "statement")?;
                argv.push("learn".into());
                argv.push(statement);
                if let Ok(scope) = arg_str(args, "scope") {
                    argv.push("--scope".into());
                    argv.push(scope);
                }
            }
            "recall" => {
                let q = arg_str(args, "query")?;
                argv.push("recall".into());
                argv.push(q);
                if fast {
                    argv.push("--fast".into());
                }
                argv.push("--json".into());
            }
            "inject" => {
                let task = arg_str(args, "task")
                    .or_else(|_| arg_str(args, "query"))
                    .unwrap_or_else(|_| "coding task".into());
                argv.push("inject".into());
                argv.push(task);
                if fast {
                    argv.push("--fast".into());
                }
            }
            "list" => {
                argv.push("list".into());
                argv.push("--json".into());
            }
            "capture" => {
                let summary = arg_str(args, "summary").or_else(|_| arg_str(args, "statement"))?;
                argv.push("capture".into());
                argv.push(summary);
            }
            "timeline" => {
                argv.push("timeline".into());
                if let Ok(q) = arg_str(args, "query") {
                    argv.push(q);
                }
            }
            "feedback" => {
                let id = arg_str(args, "id")?;
                let signal = arg_str(args, "signal").unwrap_or_else(|_| "positive".into());
                argv.push("feedback".into());
                argv.push(id);
                argv.push(signal);
            }
            "forget" => {
                let id = arg_str(args, "id")?;
                argv.push("forget".into());
                argv.push(id);
            }
            "ingest" => {
                let content = arg_str(args, "content").or_else(|_| arg_str(args, "statement"))?;
                argv.push("ingest".into());
                argv.push(content);
            }
            other => {
                return Err(MuseError::Tool(format!(
                    "unknown plur action '{other}' — status|learn|recall|inject|list|capture|timeline|feedback|forget|ingest"
                )));
            }
        }

        let refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
        ecosystem::run_capture(&bin, &refs, None, 120_000).map_err(MuseError::Tool)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_gate() {
        assert!(is_read_only_action(r#"{"action":"status"}"#));
        assert!(is_read_only_action(r#"{"action":"recall","query":"x"}"#));
        assert!(is_read_only_action(r#"{"action":"inject","task":"x"}"#));
        assert!(!is_read_only_action(
            r#"{"action":"learn","statement":"x"}"#
        ));
        assert!(!is_read_only_action(
            r#"{"action":"capture","summary":"x"}"#
        ));
        assert!(!is_read_only_action(r#"{"action":"forget","id":"ENG-1"}"#));
    }
}

//! RLM context store tool - programmatic examine of prompt-as-variable.

use super::{arg_str, arg_u64, Tool, ToolContext};
use crate::agent::context_store;
use crate::error::{NurError, Result};
use serde_json::Value;

pub struct ContextTool;

/// Room for the `var=... chars=... showing=...` header inside the inline
/// tool-result budget.
const HEADER_RESERVE: usize = 200;

/// The largest read that still reaches the model whole. `tool_result_max_chars = 0`
/// means no inline cap, so fall back to the store's own window limit.
fn inline_read_limit() -> usize {
    let cap = crate::config::load_config()
        .map(|c| c.tool_result_max_chars as usize)
        .unwrap_or(0);
    if cap == 0 {
        context_store::MAX_READ_CHARS
    } else {
        cap.saturating_sub(HEADER_RESERVE)
            .clamp(1, context_store::MAX_READ_CHARS)
    }
}

impl Tool for ContextTool {
    fn name(&self) -> &str {
        "context"
    }

    fn description(&self) -> &str {
        "RLM context store (Recursive Language Models / Prime Agent pattern): \
         large working context as named variables outside the model window. \
         Actions: list | peek | slice | search | register | delete | inventory. \
         Use when handling long docs, big tool dumps, or multi-file corpora - \
         variables survive compaction. Prefer register+peek over re-pasting."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "peek", "slice", "search", "register", "delete", "inventory"],
                    "description": "list/inventory: show vars; peek/slice/search: read; register: store text; delete: drop"
                },
                "name": {"type": "string", "description": "Variable name"},
                "content": {"type": "string", "description": "For register: full text to store"},
                "kind": {"type": "string", "description": "For register: kind label (doc, tool_result, …)"},
                "offset": {"type": "integer", "description": "For peek: char offset"},
                "max_chars": {"type": "integer", "description": "For peek/slice: window size in chars (default 2000; capped at the inline tool-result limit, the header gives the next offset)"},
                "start": {"type": "integer", "description": "For slice: start char"},
                "end": {"type": "integer", "description": "For slice: end char (exclusive)"},
                "pattern": {"type": "string", "description": "For search: substring (case-insensitive)"},
                "max_hits": {"type": "integer", "description": "For search: max lines (default 20)"}
            },
            "required": ["action"]
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action")?;
        // Always the calling session's store. Registration keys on the same id,
        // so a `/new`, `/resume`, fork or subagent reads what it registered;
        // and no argument can point a read at another session's variables.
        let session_id = ctx.session_id.as_str();
        // Reads are bounded by the inline tool-result limit so a large window
        // reaches the model intact instead of being cut to a preview; the
        // header says where to continue.
        let inline_limit = inline_read_limit();

        match action.as_str() {
            "list" | "inventory" => {
                let inv = context_store::prompt_inventory(session_id);
                if inv.is_empty() {
                    Ok("context store empty for this session".into())
                } else {
                    Ok(inv)
                }
            }
            "peek" => {
                let name = arg_str(args, "name")?;
                let offset = arg_u64(args, "offset").unwrap_or(0) as usize;
                let max = arg_u64(args, "max_chars").unwrap_or(context_store::DEFAULT_PEEK as u64)
                    as usize;
                context_store::peek(session_id, &name, offset, max.min(inline_limit))
                    .map_err(NurError::Tool)
            }
            "slice" => {
                let name = arg_str(args, "name")?;
                let start = arg_u64(args, "start").unwrap_or(0) as usize;
                // `end` wins when given; otherwise `max_chars` sizes the window
                // (the schema advertised it for peek only, and models sent it
                // to slice too, where it was ignored).
                let window = arg_u64(args, "max_chars")
                    .map(|n| n as usize)
                    .unwrap_or(context_store::DEFAULT_PEEK);
                let end = arg_u64(args, "end")
                    .map(|n| n as usize)
                    .unwrap_or_else(|| start.saturating_add(window));
                let end = end.min(start.saturating_add(inline_limit));
                context_store::slice(session_id, &name, start, end).map_err(NurError::Tool)
            }
            "search" => {
                let name = arg_str(args, "name")?;
                let pattern = arg_str(args, "pattern")?;
                let max = arg_u64(args, "max_hits").unwrap_or(20) as usize;
                context_store::search(session_id, &name, &pattern, max).map_err(NurError::Tool)
            }
            "register" => {
                let name = arg_str(args, "name")?;
                let content = arg_str(args, "content")?;
                let kind = args
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("manual");
                let v = context_store::register(session_id, &name, &content, kind, "context_tool")
                    .map_err(NurError::Tool)?;
                Ok(format!(
                    "registered `{}` id={} chars={} kind={}",
                    v.name, v.id, v.char_count, v.kind
                ))
            }
            "delete" => {
                let name = arg_str(args, "name")?;
                context_store::delete(session_id, &name).map_err(NurError::Tool)
            }
            other => Err(NurError::Tool(format!(
                "unknown context action `{other}`; use list|peek|slice|search|register|delete"
            ))),
        }
    }
}

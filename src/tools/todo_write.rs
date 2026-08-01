use super::{arg_str, Tool, ToolContext};
use crate::agent::todos::{TodoItem, TodoList, TodoStatus};
use crate::error::{NurError, Result};
use serde_json::Value;
use std::sync::{Arc, Mutex};

pub struct TodoWrite {
    pub todos: Arc<Mutex<TodoList>>,
}

impl Tool for TodoWrite {
    fn name(&self) -> &str {
        "todo_write"
    }

    fn description(&self) -> &str {
        "Update the session task list. Use for multi-step work: track pending/in_progress/completed items. \
         Prefer merge=true to update by id; merge=false replaces the whole list."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {"type": "string"},
                            "content": {"type": "string"},
                            "status": {"type": "string", "enum": ["pending","in_progress","completed","cancelled"]}
                        },
                        "required": ["id", "content", "status"]
                    }
                },
                "merge": {"type": "boolean", "default": true}
            },
            "required": ["items"]
        })
    }

    fn execute(&self, args: &Value, _ctx: &ToolContext) -> Result<String> {
        let arr = args
            .get("items")
            .and_then(|v| v.as_array())
            .ok_or_else(|| NurError::Tool("items array required".into()))?;
        let merge = args.get("merge").and_then(|v| v.as_bool()).unwrap_or(true);
        let mut items = Vec::new();
        for v in arr {
            let id = v
                .get("id")
                .and_then(|x| x.as_str())
                .ok_or_else(|| NurError::Tool("item.id required".into()))?
                .to_string();
            let content = v
                .get("content")
                .and_then(|x| x.as_str())
                .ok_or_else(|| NurError::Tool("item.content required".into()))?
                .to_string();
            let status_s = v
                .get("status")
                .and_then(|x| x.as_str())
                .unwrap_or("pending");
            let status = TodoStatus::parse(status_s)
                .ok_or_else(|| NurError::Tool(format!("bad status: {status_s}")))?;
            items.push(TodoItem {
                id,
                content,
                status,
            });
        }
        let mut g = self
            .todos
            .lock()
            .map_err(|_| NurError::Tool("todos lock".into()))?;
        g.apply(items, merge);
        Ok(format!("todos updated\n{}", g.render()))
    }
}

// silence unused import if any
#[allow(dead_code)]
fn _arg(args: &Value) -> Result<String> {
    arg_str(args, "x")
}

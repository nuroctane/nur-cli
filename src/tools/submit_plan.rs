use super::{arg_str, Tool, ToolContext};
use crate::error::{MuseError, Result};
use serde_json::Value;
use std::fs;
use std::sync::{Arc, Mutex};

/// Last submitted plan text for the TUI to surface.
pub type SharedPlan = Arc<Mutex<Option<String>>>;

pub struct SubmitPlan {
    pub plan: SharedPlan,
}

impl Tool for SubmitPlan {
    fn name(&self) -> &str {
        "submit_plan"
    }

    fn description(&self) -> &str {
        "Submit a structured implementation plan (markdown). In plan mode this is the primary deliverable. \
         User can approve and switch to manual/auto to execute."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "title": {"type": "string"},
                "plan": {"type": "string", "description": "Full markdown plan"}
            },
            "required": ["plan"]
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let plan = arg_str(args, "plan")?;
        let title = args
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Plan");
        let text = format!("# {title}\n\n{plan}\n");
        if let Ok(mut g) = self.plan.lock() {
            *g = Some(text.clone());
        }
        // Persist under workspace .meta/plan.md when possible
        let dir = ctx.cwd.join(".meta");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("plan.md");
        fs::write(&path, &text).map_err(|e| MuseError::Tool(e.to_string()))?;
        Ok(format!(
            "plan submitted and written to {}\n\nUser can Shift+Tab to manual/auto and say \"implement the plan\".",
            path.display()
        ))
    }
}

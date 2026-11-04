use super::{arg_str, Tool, ToolContext};
use crate::error::{NurError, Result};
use serde_json::{json, Value};

pub struct Worktree;

impl Tool for Worktree {
    fn name(&self) -> &str {
        "worktree"
    }
    fn description(&self) -> &str {
        "Create or list managed Git worktrees for isolated writers. Create starts from committed HEAD and does not copy uncommitted changes. Pass the name as agent.worktree to run a child there."
    }
    fn parameters_schema(&self) -> Value {
        json!({"type":"object", "required":["action"], "properties": {
            "action":{"type":"string", "enum":["create","list"]},
            "name":{"type":"string", "description":"Managed worktree name for create"}
        }})
    }
    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        match arg_str(args, "action")?.as_str() {
            "list" => crate::worktree::list(&ctx.cwd, &ctx.cancel),
            "create" => crate::worktree::create(&ctx.cwd, &arg_str(args, "name")?, &ctx.cancel),
            action => Err(NurError::Tool(format!("unknown worktree action: {action}"))),
        }
    }
}

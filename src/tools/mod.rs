mod apply_patch;
mod bash;
mod edit_file;
mod git_diff;
mod git_status;
mod glob;
pub mod executor_tool;
pub use executor_tool::is_read_only_action as executor_is_read_only;
pub mod graphify;
mod grep;
mod list_dir;
mod memory_tool;
mod multi_edit;
pub mod plur;
mod read_file;
pub mod ruflo;
mod sandbox;
mod search_util;
mod shell;
mod skill_tool;
mod submit_plan;
mod todo_write;
mod web_fetch;
mod web_search;
mod write_file;

use crate::agent::todos::{shared_empty, SharedTodos, TodoList};
use crate::api::types::ToolDef;
use crate::error::{MuseError, Result};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub use sandbox::{is_dangerous_workspace, resolve_safe_workspace};
pub use shell::shell_backend;
pub use submit_plan::{SharedPlan, SubmitPlan};

pub struct ToolContext {
    pub cwd: PathBuf,
    /// Cooperative cancellation — long-running tools (shell) poll this and
    /// kill their child processes when the user hits Esc.
    pub cancel: tokio_util::sync::CancellationToken,
}

pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> Value;
    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String>;
}

/// Stateful tool host (todos/plan share with TUI).
pub struct ToolHost {
    pub todos: SharedTodos,
    pub plan: SharedPlan,
}

impl Default for ToolHost {
    fn default() -> Self {
        Self {
            todos: shared_empty(),
            plan: Arc::new(Mutex::new(None)),
        }
    }
}

impl ToolHost {
    fn boxed_tools(&self) -> Vec<Box<dyn Tool>> {
        vec![
            Box::new(read_file::ReadFile),
            Box::new(list_dir::ListDir),
            Box::new(write_file::WriteFile),
            Box::new(edit_file::EditFile),
            Box::new(multi_edit::MultiEdit),
            Box::new(apply_patch::ApplyPatch),
            Box::new(bash::Bash),
            Box::new(grep::Grep),
            Box::new(glob::GlobTool),
            Box::new(web_fetch::WebFetch),
            Box::new(web_search::WebSearch),
            Box::new(git_status::GitStatus),
            Box::new(git_diff::GitDiff),
            Box::new(graphify::Graphify),
            Box::new(plur::Plur),
            Box::new(ruflo::Ruflo),
            Box::new(executor_tool::ExecutorTool),
            Box::new(skill_tool::SkillTool),
            Box::new(memory_tool::MemoryTool),
            Box::new(todo_write::TodoWrite {
                todos: self.todos.clone(),
            }),
            Box::new(SubmitPlan {
                plan: self.plan.clone(),
            }),
            // `agent` is handled asynchronously in the agent loop (nested runner).
            Box::new(AgentStub),
        ]
    }

    pub fn tool_defs(&self) -> Vec<ToolDef> {
        self.boxed_tools()
            .into_iter()
            .map(|t| ToolDef {
                type_: "function".into(),
                name: t.name().into(),
                description: Some(t.description().into()),
                parameters: Some(t.parameters_schema()),
            })
            .collect()
    }

    pub fn dispatch(&self, name: &str, arguments: &str, ctx: &ToolContext) -> Result<String> {
        if name == "agent" {
            return Err(MuseError::Tool(
                "agent tool must be executed by the runtime (internal error)".into(),
            ));
        }
        if is_dangerous_workspace(&ctx.cwd) && name != "web_fetch" && name != "memory" {
            return Err(MuseError::Tool(
                "workspace is filesystem root — refuse tools. Re-run from a project dir or pass --cwd"
                    .into(),
            ));
        }
        let args: Value = serde_json::from_str(arguments).unwrap_or_else(|_| serde_json::json!({}));
        for tool in self.boxed_tools() {
            if tool.name() == name {
                return tool.execute(&args, ctx);
            }
        }
        Err(MuseError::Tool(format!("unknown tool: {name}")))
    }

    pub fn todos_snapshot(&self) -> TodoList {
        self.todos.lock().map(|g| g.clone()).unwrap_or_default()
    }
}

/// Placeholder so the model sees the agent tool schema; execution is in loop.rs.
struct AgentStub;

impl Tool for AgentStub {
    fn name(&self) -> &str {
        "agent"
    }

    fn description(&self) -> &str {
        "Spawn a subagent for a focused subtask. \
         subagent_type: explore (read-only research) | general (same tools as parent). \
         Returns a text report. Use for parallel research or isolated investigation."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "description": {"type": "string", "description": "Short 3-7 word label"},
                "prompt": {"type": "string", "description": "Full task for the subagent"},
                "subagent_type": {
                    "type": "string",
                    "enum": ["explore", "general"],
                    "default": "explore"
                }
            },
            "required": ["prompt"]
        })
    }

    fn execute(&self, _args: &Value, _ctx: &ToolContext) -> Result<String> {
        Err(MuseError::Tool("agent is runtime-handled".into()))
    }
}

pub(crate) fn resolve_path(cwd: &PathBuf, path: &str) -> Result<PathBuf> {
    sandbox::resolve_in_workspace(cwd, path)
}

pub(crate) fn arg_str(args: &Value, key: &str) -> Result<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| MuseError::Tool(format!("missing string arg: {key}")))
}

pub(crate) fn arg_u64(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(|v| {
        v.as_u64()
            .or_else(|| v.as_i64().map(|i| i as u64))
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
    })
}

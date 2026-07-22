pub mod akarso;
pub mod t3code_tool;
pub mod penecho_tool;
pub mod fractal_tool;
mod apply_patch;
mod bash;
pub mod browser;
pub use browser::is_read_only_action as browser_is_read_only;
pub mod capabilities;
mod edit_file;
pub mod executor_tool;
mod git_diff;
mod git_status;
mod glob;
pub use executor_tool::is_read_only_action as executor_is_read_only;
pub mod excalidraw;
pub mod graphify;
pub mod graphjin;
mod grep;
mod list_dir;
pub mod media;
mod memory_tool;
mod multi_edit;
pub mod omp;
pub mod plur;
mod read_file;
pub mod ruflo;
mod sandbox;
mod search_util;
mod shell;
mod skill_tool;
pub mod spill;
mod submit_plan;
pub mod tldraw;
mod todo_write;
pub mod undo;
mod web_fetch;
mod web_search;
mod write_file;

use crate::agent::todos::{shared_empty, SharedTodos, TodoList};
use crate::api::types::ToolDef;
use crate::error::{MuseError, Result};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[allow(unused_imports)] // full capability surface re-exported; loop uses a subset
pub use capabilities::{
    classify as classify_tool, is_concurrency_safe, is_destructive_call, is_parallel_safe,
    is_read_only_call, ToolCaps,
};
pub use sandbox::{is_dangerous_workspace, resolve_safe_workspace};
pub use shell::shell_backend;
pub use submit_plan::{SharedPlan, SubmitPlan};

/// Shared mid-turn "steer" queue. The TUI pushes user messages here while a
/// turn is running; the agent loop drains it at each round boundary and injects
/// them into the live conversation **without cancelling** the turn (unlike the
/// interrupt path). Empty for headless / subagent runs.
pub type SharedSteer = Arc<Mutex<std::collections::VecDeque<String>>>;

/// A fresh, empty steer queue.
pub fn shared_steer() -> SharedSteer {
    Arc::new(Mutex::new(std::collections::VecDeque::new()))
}

pub struct ToolContext {
    pub cwd: PathBuf,
    /// Cooperative cancellation — long-running tools (shell) poll this and
    /// kill their child processes when the user hits Esc.
    pub cancel: tokio_util::sync::CancellationToken,
}

/// Tool contract. Capability methods are **fail-closed** by default
/// (not free, not parallel, not destructive). Override or rely on the
/// central classifier in [`capabilities`] via the default impls.
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> Value;
    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String>;

    /// Approval-free in manual when true; plan mode allows freely when true.
    /// (Capability surface on the trait; the agent loop currently classifies
    /// via the free `capabilities::*` fns by name+args.)
    #[allow(dead_code)]
    fn is_read_only(&self, args: &Value) -> bool {
        capabilities::is_read_only(self.name(), args)
    }

    /// May join a concurrent batch. Must imply [`is_read_only`].
    #[allow(dead_code)]
    fn is_concurrency_safe(&self, args: &Value) -> bool {
        capabilities::classify_value(self.name(), args).concurrency_safe
    }

    /// High-impact / irreversible mutator (writes, shell, agent, …).
    #[allow(dead_code)]
    fn is_destructive(&self, args: &Value) -> bool {
        capabilities::is_destructive(self.name(), args)
    }
}

/// Stateful tool host (todos/plan/steer share with TUI).
pub struct ToolHost {
    pub todos: SharedTodos,
    pub plan: SharedPlan,
    /// Mid-turn steering messages, drained by the agent loop each round.
    pub steer: SharedSteer,
}

impl Default for ToolHost {
    fn default() -> Self {
        Self {
            todos: shared_empty(),
            plan: Arc::new(Mutex::new(None)),
            steer: shared_steer(),
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
            Box::new(browser::BrowserTool),
            Box::new(media::Look),
            Box::new(media::ExtractFrames),
            Box::new(git_status::GitStatus),
            Box::new(git_diff::GitDiff),
            Box::new(graphify::Graphify),
            Box::new(graphjin::GraphJin),
            Box::new(excalidraw::Excalidraw),
            Box::new(tldraw::Tldraw),
            Box::new(plur::Plur),
            Box::new(ruflo::Ruflo),
            Box::new(akarso::Akarso),
            Box::new(t3code_tool::T3Code),
            Box::new(penecho_tool::Penecho),
            Box::new(fractal_tool::Fractal),
            Box::new(executor_tool::ExecutorTool),
            Box::new(omp::OmpTool),
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
        // Cache the static tool defs — todo_write/ submit_plan schema doesn't depend on state
        static CACHE: std::sync::OnceLock<Vec<ToolDef>> = std::sync::OnceLock::new();
        CACHE
            .get_or_init(|| {
                let host = ToolHost::default();
                host.boxed_tools()
                    .into_iter()
                    .map(|t| ToolDef {
                        type_: "function".into(),
                        name: t.name().into(),
                        description: Some(t.description().into()),
                        parameters: Some(t.parameters_schema()),
                    })
                    .collect()
            })
            .clone()
    }

    /// Execute a tool by name. Deliberately a direct `match` (not a
    /// `Vec<Box<dyn Tool>>` lookup): `ToolHost` is a throwaway built per call
    /// from two `Arc` clones, so a stored registry would re-allocate every
    /// boxed tool on each dispatch — a hot-path regression. The trade-off is
    /// that the arm roster here must mirror [`Self::boxed_tools`]; the
    /// `roster_stays_in_sync` test locks that invariant.
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

        // Direct match dispatch — no Vec<Box> allocation per call
        match name {
            "read_file" => read_file::ReadFile.execute(&args, ctx),
            "list_dir" => list_dir::ListDir.execute(&args, ctx),
            "write_file" => write_file::WriteFile.execute(&args, ctx),
            "edit_file" => edit_file::EditFile.execute(&args, ctx),
            "multi_edit" => multi_edit::MultiEdit.execute(&args, ctx),
            "apply_patch" => apply_patch::ApplyPatch.execute(&args, ctx),
            "bash" => bash::Bash.execute(&args, ctx),
            "grep" => grep::Grep.execute(&args, ctx),
            "glob" => glob::GlobTool.execute(&args, ctx),
            "web_fetch" => web_fetch::WebFetch.execute(&args, ctx),
            "web_search" => web_search::WebSearch.execute(&args, ctx),
            "browser" => browser::BrowserTool.execute(&args, ctx),
            "look" => media::Look.execute(&args, ctx),
            "extract_frames" => media::ExtractFrames.execute(&args, ctx),
            "git_status" => git_status::GitStatus.execute(&args, ctx),
            "git_diff" => git_diff::GitDiff.execute(&args, ctx),
            "graphify" => graphify::Graphify.execute(&args, ctx),
            "graphjin" => graphjin::GraphJin.execute(&args, ctx),
            "excalidraw" => excalidraw::Excalidraw.execute(&args, ctx),
            "tldraw" => tldraw::Tldraw.execute(&args, ctx),
            "plur" => plur::Plur.execute(&args, ctx),
            "ruflo" => ruflo::Ruflo.execute(&args, ctx),
            "akarso" => akarso::Akarso.execute(&args, ctx),
            "t3code" => t3code_tool::T3Code.execute(&args, ctx),
            "penecho" => penecho_tool::Penecho.execute(&args, ctx),
            "fractal" => fractal_tool::Fractal.execute(&args, ctx),
            "executor" => executor_tool::ExecutorTool.execute(&args, ctx),
            "omp" => omp::OmpTool.execute(&args, ctx),
            "skill" => skill_tool::SkillTool.execute(&args, ctx),
            "memory" => memory_tool::MemoryTool.execute(&args, ctx),
            "todo_write" => todo_write::TodoWrite {
                todos: self.todos.clone(),
            }
            .execute(&args, ctx),
            "submit_plan" => SubmitPlan {
                plan: self.plan.clone(),
            }
            .execute(&args, ctx),
            _ => Err(MuseError::Tool(format!("unknown tool: {name}"))),
        }
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
         Returns a text report. \
         **Issue several agent calls in one response to fan them out — they run \
         concurrently** (up to 4 at a time), so independent investigations cost \
         roughly one investigation's wall time. Split work that does not share \
         state: one subagent per subsystem, per hypothesis, or per file cluster. \
         Watch them live in the TUI with /swarm."
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every tool the model can see (a schema in `tool_defs`) must have a
    /// `dispatch` arm, and vice-versa. Because the two rosters live in separate
    /// functions (see the note on `dispatch`), this test is the guardrail: it
    /// locks the exact set so adding/removing a tool in only one place fails CI.
    #[test]
    fn roster_stays_in_sync() {
        let mut got: Vec<String> = ToolHost::default()
            .tool_defs()
            .iter()
            .map(|d| d.name.clone())
            .collect();
        got.sort();

        let mut want = vec![
            "read_file",
            "list_dir",
            "write_file",
            "edit_file",
            "multi_edit",
            "apply_patch",
            "bash",
            "grep",
            "glob",
            "web_fetch",
            "web_search",
            "browser",
            "look",
            "extract_frames",
            "git_status",
            "git_diff",
            "graphify",
            "graphjin",
            "excalidraw",
            "tldraw",
            "plur",
            "ruflo",
            "akarso",
            "t3code",
            "penecho",
            "fractal",
            "executor",
            "omp",
            "skill",
            "memory",
            "todo_write",
            "submit_plan",
            "agent",
        ];
        want.sort();

        assert_eq!(
            got, want,
            "tool roster drift: update BOTH boxed_tools() and the dispatch match \
             (and this list) when adding/removing a tool"
        );
    }

    /// The unknown-tool fallthrough must actually reject unregistered names
    /// (so a schema/dispatch mismatch surfaces as a clear error, not a panic).
    #[test]
    fn unknown_tool_is_rejected() {
        let host = ToolHost::default();
        let ctx = ToolContext {
            cwd: std::env::temp_dir(),
            cancel: tokio_util::sync::CancellationToken::new(),
        };
        let err = host
            .dispatch("definitely_not_a_real_tool", "{}", &ctx)
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown tool"), "got: {err}");
    }
}

pub mod akarso;
mod apply_patch;
pub mod anydoc_tool;
mod bash;
pub mod browser;
pub mod egaki_tool;
pub mod terminal_browser;
pub use terminal_browser::is_read_only_action as terminal_browser_is_read_only;
pub mod bg_tool;
mod connectome_tool;
mod context_tool;
pub mod fractal_tool;
mod goal_tool;
mod harness_tool;
mod mem_tool;
mod message_tool;
mod admission_tool;
mod ipython_tool;
pub mod headroom_tool;
pub mod optmem_tool;
pub mod penecho_tool;
mod proposal_tool;
pub mod t3code_tool;
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
pub(crate) mod sensitive;
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
use crate::error::{NurError, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
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

/// Focused tool surface for child agents.
///
/// Children receive the repo and web primitives needed to investigate or edit,
/// but not nested delegation, OMP, memory systems, ecosystem gateways, or
/// presentation tools. Besides preventing accidental fallback recursion, this
/// avoids re-sending a large schema catalog on every child model round.
pub const SUBAGENT_TOOL_NAMES: &[&str] = &[
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
    "terminal_browser",
    "look",
    "extract_frames",
    "git_status",
    "git_diff",
    // RLM / Prime / Shepherd / AnyDoc surfaces children need for long context
    "context",
    "anydoc",
    "goal",
    "proposal",
    "connectome",
    "repl",
    "mem",
];

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

    /// May join a concurrent batch. Must imply `is_read_only`.
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
            Box::new(terminal_browser::TerminalBrowser),
            Box::new(media::Look),
            Box::new(media::ExtractFrames),
            Box::new(git_status::GitStatus),
            Box::new(git_diff::GitDiff),
            Box::new(context_tool::ContextTool),
            Box::new(anydoc_tool::AnydocTool),
            Box::new(goal_tool::GoalTool),
            Box::new(proposal_tool::ProposalTool),
            Box::new(harness_tool::HarnessTool),
            Box::new(connectome_tool::ConnectomeTool),
            Box::new(ipython_tool::ReplTool),
            Box::new(admission_tool::AdmissionTool),
            Box::new(message_tool::MessageTool),
            Box::new(mem_tool::MemTool),
            Box::new(graphify::Graphify),
            Box::new(graphjin::GraphJin),
            Box::new(excalidraw::Excalidraw),
            Box::new(tldraw::Tldraw),
            Box::new(plur::Plur),
            Box::new(ruflo::Ruflo),
            Box::new(akarso::Akarso),
            Box::new(t3code_tool::T3Code),
            Box::new(penecho_tool::Penecho),
            Box::new(bg_tool::Bg),
            Box::new(fractal_tool::Fractal),
            Box::new(headroom_tool::Headroom),
            Box::new(optmem_tool::OptMem),
            Box::new(egaki_tool::Egaki),
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

    pub fn subagent_tool_defs(&self) -> Vec<ToolDef> {
        static CACHE: std::sync::OnceLock<Vec<ToolDef>> = std::sync::OnceLock::new();
        CACHE
            .get_or_init(|| {
                self.tool_defs()
                    .into_iter()
                    .filter(|tool| SUBAGENT_TOOL_NAMES.contains(&tool.name.as_str()))
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
            return Err(NurError::Tool(
                "agent tool must be executed by the runtime (internal error)".into(),
            ));
        }
        if is_dangerous_workspace(&ctx.cwd) && name != "web_fetch" && name != "memory" {
            return Err(NurError::Tool(
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
            "terminal_browser" => terminal_browser::TerminalBrowser.execute(&args, ctx),
            "look" => media::Look.execute(&args, ctx),
            "extract_frames" => media::ExtractFrames.execute(&args, ctx),
            "git_status" => git_status::GitStatus.execute(&args, ctx),
            "git_diff" => git_diff::GitDiff.execute(&args, ctx),
            "context" => context_tool::ContextTool.execute(&args, ctx),
            "anydoc" => anydoc_tool::AnydocTool.execute(&args, ctx),
            "goal" => goal_tool::GoalTool.execute(&args, ctx),
            "proposal" => proposal_tool::ProposalTool.execute(&args, ctx),
            "harness" => harness_tool::HarnessTool.execute(&args, ctx),
            "connectome" => connectome_tool::ConnectomeTool.execute(&args, ctx),
            "repl" => ipython_tool::ReplTool.execute(&args, ctx),
            "admission" => admission_tool::AdmissionTool.execute(&args, ctx),
            "message" => message_tool::MessageTool.execute(&args, ctx),
            "mem" => mem_tool::MemTool.execute(&args, ctx),
            "graphify" => graphify::Graphify.execute(&args, ctx),
            "graphjin" => graphjin::GraphJin.execute(&args, ctx),
            "excalidraw" => excalidraw::Excalidraw.execute(&args, ctx),
            "tldraw" => tldraw::Tldraw.execute(&args, ctx),
            "plur" => plur::Plur.execute(&args, ctx),
            "ruflo" => ruflo::Ruflo.execute(&args, ctx),
            "akarso" => akarso::Akarso.execute(&args, ctx),
            "t3code" => t3code_tool::T3Code.execute(&args, ctx),
            "penecho" => penecho_tool::Penecho.execute(&args, ctx),
            "bg" => bg_tool::Bg.execute(&args, ctx),
            "fractal" => fractal_tool::Fractal.execute(&args, ctx),
            "headroom" => headroom_tool::Headroom.execute(&args, ctx),
            "optmem" => optmem_tool::OptMem.execute(&args, ctx),
            "egaki" => egaki_tool::Egaki.execute(&args, ctx),
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
            _ => Err(NurError::Tool(format!("unknown tool: {name}"))),
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
         **Issue several agent calls in one response to fan them out - they run \
         concurrently** (up to 4 at a time), so independent investigations cost \
         roughly one investigation's wall time. Split work that does not share \
         state: one subagent per subsystem, per hypothesis, or per file cluster. \
         \n\nCROSS-PROVIDER DEPLOYMENT: set `provider` to run this subagent on a \
         DIFFERENT model provider than yours - this is how you 'deploy a subagent \
         on gemini', 'use grok', 'spawn a claude subagent', etc. When the user's \
         request names a provider or model, you MUST pass it in `provider` (and \
         optionally `model`); do not just describe it. `provider` accepts a catalog \
         id, a display name, or a natural-language alias: \
         provider:\"anthropic\" / \"claude\" / \"sonnet\" / \"opus\", \
         provider:\"openai\" / \"gpt\" / \"chatgpt\", \
         provider:\"xai\" / \"grok\", \
         provider:\"google\" / \"gemini\" / \"flash\" / \"pro\", \
         provider:\"antigravity\" (Google via agy CLI - its OWN provider, not google), \
         provider:\"deepseek\", \"mistral\", \"kimi\", \"moonshot\", \"qwen\", \"groq\", \
         \"openrouter\", \"ollama\" (local). `model` is optional - omit it and the \
         subagent uses that provider's default model (recommended unless you know \
         a valid model id for that provider). The subagent uses that \
         provider's stored credentials, importing a logged-in vendor CLI session \
         (Claude Code, Codex, agy, gemini, …) automatically when there is no key \
         on disk. If you are NOT signed in to the requested provider, the spawn is \
         BLOCKED - it does NOT fall back to your provider - and the TUI pops /login \
         pre-selected to that provider. In that case tell the user to finish /login \
         (or run /login <provider>); nur then hands you the exact re-deploy call. \
         Never re-run that task on your own provider and report it as the \
         requested one. Omit `provider` and `model` to inherit yours. Watch runs \
         live with /swarm - each pane shows the provider its child ran on."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "description": {"type": "string", "description": "Short 3-7 word label"},
                "prompt": {
                    "type": "string",
                    "maxLength": crate::agent::r#loop::MAX_SUBAGENT_PROMPT_CHARS,
                    "description": "Bounded focused task for the subagent; include only the context needed for this delegation"
                },
                "subagent_type": {
                    "type": "string",
                    "enum": ["explore", "general"],
                    "default": "explore"
                },
                "provider": {
                    "type": "string",
                    "description": "Optional: deploy this subagent on a DIFFERENT provider than yours. Pass this whenever the request names a provider/model. Accepts a catalog id, display name, or natural-language alias: anthropic/claude/sonnet/opus, openai/gpt/chatgpt, xai/grok, google/gemini/flash/pro, antigravity (distinct from google), deepseek, mistral, kimi, moonshot, qwen, groq, openrouter, ollama. Uses that provider's stored credentials; if missing, the TUI pops /login pre-selected to it. Omit to inherit the parent provider."
                },
                "model": {
                    "type": "string",
                    "description": "Optional: exact model id for the chosen provider. Omit to use that provider's default model (recommended). If you supply a model id it is used as-is, so only pass one you know is valid for that provider."
                },
                "reason": {
                    "type": "string",
                    "description": "Optional handoff reason (OpenAI Agents SDK handoff pattern) - why this specialist is needed"
                },
                "handoff_role": {
                    "type": "string",
                    "description": "Optional specialist role label for the handoff packet (e.g. security-reviewer, docs)"
                },
                "context_files": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Optional handoff input filter (OpenAI Agents SDK): workspace files to load into the child's prompt so it gets exactly the context it needs. Paths are workspace-relative."
                },
                "async": {
                    "type": "boolean",
                    "description": "Optional RLM async admission (Prime rlm()): when true, return a handle id immediately and run the subagent in the background; the model keeps working. Retrieve the result with tool `admission` action=get. Default false (blocking report)."
                }
            },
            "required": ["prompt"]
        })
    }

    fn execute(&self, _args: &Value, _ctx: &ToolContext) -> Result<String> {
        Err(NurError::Tool("agent is runtime-handled".into()))
    }
}

pub(crate) fn resolve_path(cwd: &Path, path: &str) -> Result<PathBuf> {
    sandbox::resolve_in_workspace(cwd, path)
}

pub(crate) fn arg_str(args: &Value, key: &str) -> Result<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| NurError::Tool(format!("missing string arg: {key}")))
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
            "terminal_browser",
            "look",
            "extract_frames",
            "git_status",
            "git_diff",
            "context",
            "anydoc",
            "goal",
            "proposal",
            "harness",
            "connectome",
            "repl",
            "admission",
            "message",
            "mem",
            "graphify",
            "graphjin",
            "excalidraw",
            "tldraw",
            "plur",
            "ruflo",
            "akarso",
            "t3code",
            "penecho",
            "bg",
            "fractal",
            "headroom",
            "optmem",
            "egaki",
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

    #[test]
    fn subagent_surface_is_focused_and_cannot_recurse() {
        let host = ToolHost::default();
        let defs = host.subagent_tool_defs();
        let names: Vec<&str> = defs.iter().map(|tool| tool.name.as_str()).collect();
        assert_eq!(names, SUBAGENT_TOOL_NAMES);
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"write_file"));
        assert!(!names.contains(&"agent"));
        assert!(!names.contains(&"omp"));
        assert!(defs.len() < host.tool_defs().len());
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

use super::mode::{PermissionMode, SharedMode};
use super::prompt::PromptContext;
use super::session::Session;
use super::subagent;
use crate::api::types::{
    function_call_output_item, replay_output_items, user_text_item, FunctionCallRef,
    ReasoningConfig, ResponseRequest,
};
use crate::api::{ApiResponse, MetaClient, StreamEvent};
use crate::config::Config;
use crate::error::{MuseError, Result};
use crate::tools::{ToolContext, ToolHost};
use crate::usage::{TokenUsage, UsageTracker};
use serde_json::Value;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

/// Events emitted while an agent turn runs.
pub enum AgentEvent {
    Status(String),
    ReasoningDelta(String),
    TextDelta(String),
    AssistantMessage(String),
    ToolStart { id: u64, name: String, args: String },
    ToolEnd {
        id: u64,
        name: String,
        result: String,
        ok: bool,
    },
    /// Todo list changed — TUI should refresh.
    TodosChanged(String),
    /// Plan written via submit_plan.
    PlanSubmitted(String),
    ApprovalRequest {
        name: String,
        args: String,
        respond: oneshot::Sender<ApprovalDecision>,
    },
    Usage { session: TokenUsage, last: TokenUsage },
    Done {
        session: Box<Session>,
        usage: Box<UsageTracker>,
        result: std::result::Result<String, String>,
        interrupted: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    Approve,
    ApproveAlways,
    Deny,
}

/// Tools that never mutate the workspace (approval-free in manual; allowed in plan).
/// Note: `memory` is special-cased — only action=read is free; append is mutating.
pub const READ_ONLY_TOOLS: &[&str] = &[
    "read_file",
    "list_dir",
    "grep",
    "glob",
    "web_fetch",
    "web_search",
    "git_status",
    "git_diff",
    "skill",
    "todo_write",
    "submit_plan",
];

pub struct AgentRunner {
    pub client: MetaClient,
    pub config: Config,
    pub cwd: PathBuf,
    pub permission_mode: SharedMode,
    #[allow(dead_code)]
    pub verbose: bool,
    pub approved_tools: Arc<Mutex<HashSet<String>>>,
    pub tools: ToolHost,
    /// Nested subagents cannot spawn further agents (depth limit 1).
    pub is_subagent: bool,
}

pub fn spawn_turn(
    runner: Arc<AgentRunner>,
    mut session: Session,
    mut usage: UsageTracker,
    prompt: String,
    tx: mpsc::UnboundedSender<AgentEvent>,
    cancel: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let res = runner
            .run_turn_events(&mut session, &prompt, &mut usage, &tx, &cancel)
            .await;
        usage.set_state("idle");
        let _ = session.save();
        let interrupted = matches!(res, Err(MuseError::Interrupted));
        let result = res.map_err(|e| e.to_string());
        let _ = tx.send(AgentEvent::Done {
            session: Box::new(session),
            usage: Box::new(usage),
            result,
            interrupted,
        });
    })
}

impl AgentRunner {
    pub async fn run_turn_events(
        &self,
        session: &mut Session,
        user_text: &str,
        usage: &mut UsageTracker,
        tx: &mpsc::UnboundedSender<AgentEvent>,
        cancel: &CancellationToken,
    ) -> Result<String> {
        session.push_user(user_text);
        session.input_items.push(user_text_item(user_text));

        let tools = self.tools.tool_defs();
        // Disk-backed prompt parts (skills, MUSE.md, memory, shell) — read once
        // per user turn, not once per model request.
        let prompt_ctx = PromptContext::build(&self.cwd, self.is_subagent);
        let mut turns = 0u32;
        let mut tool_seq: u64 = 0;
        // Prevent compact→still-hot→compact infinite loop within one user turn.
        let mut did_auto_compact = false;

        loop {
            if cancel.is_cancelled() {
                return Err(MuseError::Interrupted);
            }
            turns += 1;
            if turns > self.config.max_turns {
                return Err(MuseError::MaxTurns(self.config.max_turns));
            }

            // Auto-compact at most once per user turn (Claude-style pressure relief).
            if !did_auto_compact && should_auto_compact(usage, &self.config) {
                let _ = tx.send(AgentEvent::Status("auto-compacting context…".into()));
                match compact_session(self, session, usage).await {
                    Ok(_) => {
                        did_auto_compact = true;
                        let _ = tx.send(AgentEvent::Status(
                            "context compacted — continuing".into(),
                        ));
                    }
                    Err(e) => {
                        did_auto_compact = true; // don't spin on repeated failures
                        let _ = tx.send(AgentEvent::Status(format!(
                            "auto-compact skipped: {e}"
                        )));
                    }
                }
            }

            let mode_now = self.permission_mode.get();
            let instructions =
                prompt_ctx.render(mode_now, &self.tools.todos_snapshot().render());

            usage.set_state(format!("thinking (turn {turns})"));
            let _ = tx.send(AgentEvent::Status(format!(
                "thinking · turn {turns} · {}",
                mode_now.label()
            )));

            let req = ResponseRequest {
                model: self.config.model.clone(),
                input: Value::Array(session.input_items.clone()),
                instructions: Some(instructions),
                tools: Some(tools.clone()),
                tool_choice: Some("auto".into()),
                store: Some(false),
                include: Some(vec!["reasoning.encrypted_content".into()]),
                reasoning: Some(ReasoningConfig {
                    effort: Some(self.config.reasoning_effort.clone()),
                    summary: Some("auto".into()),
                }),
                stream: Some(self.config.stream && !self.is_subagent),
                parallel_tool_calls: Some(true),
            };

            let mut text_deltas = 0usize;
            let resp: ApiResponse = if req.stream == Some(true) {
                self.client
                    .create_response_stream(
                        &req,
                        |ev| match ev {
                            StreamEvent::TextDelta(d) => {
                                text_deltas += 1;
                                let _ = tx.send(AgentEvent::TextDelta(d));
                            }
                            StreamEvent::ReasoningDelta(d) => {
                                let _ = tx.send(AgentEvent::ReasoningDelta(d));
                            }
                            StreamEvent::Completed(_) => {}
                        },
                        cancel,
                    )
                    .await?
            } else {
                tokio::select! {
                    _ = cancel.cancelled() => return Err(MuseError::Interrupted),
                    r = self.client.create_response(&req) => r?,
                }
            };

            if let Some(u) = &resp.usage {
                let tu: TokenUsage = u.into();
                usage.record_request(tu.clone(), resp.id.clone());
                session.usage.add(&tu);
                let _ = tx.send(AgentEvent::Usage {
                    session: usage.session_usage().clone(),
                    last: tu,
                });
            }

            let replayed = replay_output_items(&resp.output);
            session.input_items.extend(replayed);

            let calls = resp.function_calls();
            let text = resp.output_text();

            if text_deltas == 0 && !text.is_empty() {
                let _ = tx.send(AgentEvent::AssistantMessage(text.clone()));
            }

            if calls.is_empty() {
                usage.set_state("idle");
                session.push_assistant(&text);
                let _ = session.save();
                return Ok(text);
            }

            // Execute tools **in original model order** (required for call_id pairing).
            // Contiguous parallel-safe reads may run concurrently, results emitted in order.
            let mut idx = 0usize;
            while idx < calls.len() {
                if cancel.is_cancelled() {
                    pair_interrupted(&mut session.input_items, &calls);
                    let _ = session.save();
                    return Err(MuseError::Interrupted);
                }

                // Contiguous parallel-safe batch
                if is_parallel_safe(&calls[idx].name, &calls[idx].arguments) {
                    let mut batch_end = idx + 1;
                    while batch_end < calls.len()
                        && is_parallel_safe(&calls[batch_end].name, &calls[batch_end].arguments)
                    {
                        batch_end += 1;
                    }
                    let batch = &calls[idx..batch_end];
                    let mut handles = Vec::new();
                    let mut meta: Vec<(u64, String, String)> = Vec::new(); // id, call_id, name

                    for call in batch {
                        // Parallel-safe tools are always free — no approval (keeps output order simple).
                        tool_seq += 1;
                        let id = tool_seq;
                        let _ = tx.send(AgentEvent::ToolStart {
                            id,
                            name: call.name.clone(),
                            args: call.arguments.clone(),
                        });
                        let host = ToolHost {
                            todos: self.tools.todos.clone(),
                            plan: self.tools.plan.clone(),
                        };
                        let cwd = self.cwd.clone();
                        let name = call.name.clone();
                        let args = call.arguments.clone();
                        let call_id = call.call_id.clone();
                        let cancel_t = cancel.clone();
                        meta.push((id, call_id.clone(), name.clone()));
                        handles.push(tokio::task::spawn_blocking(move || {
                            let res = host.dispatch(
                                &name,
                                &args,
                                &ToolContext {
                                    cwd,
                                    cancel: cancel_t,
                                },
                            );
                            (call_id, name, res)
                        }));
                    }

                    // Collect in submission order (handles order matches meta)
                    for (handle, (id, call_id, name)) in
                        handles.into_iter().zip(meta.into_iter())
                    {
                        let (_, _, res) = tokio::select! {
                            _ = cancel.cancelled() => {
                                // Fills this call, the rest of the batch, and every
                                // post-batch call — whatever has not answered yet.
                                pair_interrupted(&mut session.input_items, &calls);
                                // Note: other in-flight blocking tasks keep running until drop
                                let _ = session.save();
                                return Err(MuseError::Interrupted);
                            }
                            r = handle => r.map_err(|e| MuseError::Other(e.to_string()))?,
                        };
                        let (body, ok) = match res {
                            Ok(s) => (s, true),
                            Err(e) => (format!("error: {e}"), false),
                        };
                        emit_side_effects(tx, &name, &body);
                        let _ = tx.send(AgentEvent::ToolEnd {
                            id,
                            name,
                            result: body.clone(),
                            ok,
                        });
                        session
                            .input_items
                            .push(function_call_output_item(&call_id, &body));
                    }
                    idx = batch_end;
                    continue;
                }

                // Single sequential tool (mutating / agent / memory append)
                let call = &calls[idx];
                tool_seq += 1;
                let id = tool_seq;
                let _ = tx.send(AgentEvent::ToolStart {
                    id,
                    name: call.name.clone(),
                    args: call.arguments.clone(),
                });

                let mode_at_gate = self.permission_mode.get();
                let approved = self.check_approval(&call.name, &call.arguments, tx).await;
                if !approved {
                    let plan_block = mode_at_gate.is_read_only_enforced()
                        && !is_read_only_call(&call.name, &call.arguments);
                    let (msg, result_label) = if plan_block {
                        (
                            format!(
                                "blocked: plan mode. Switch to manual/auto (Shift+Tab) for {}.",
                                call.name
                            ),
                            "blocked · plan mode".into(),
                        )
                    } else {
                        (
                            "user denied this tool call".into(),
                            "denied by user".into(),
                        )
                    };
                    let _ = tx.send(AgentEvent::ToolEnd {
                        id,
                        name: call.name.clone(),
                        result: result_label,
                        ok: false,
                    });
                    session
                        .input_items
                        .push(function_call_output_item(&call.call_id, &msg));
                    idx += 1;
                    continue;
                }

                usage.set_state(format!("tool:{}", call.name));

                let (body, ok) = if call.name == "agent" {
                    if self.is_subagent {
                        (
                            "error: nested subagents are not allowed (depth limit)".into(),
                            false,
                        )
                    } else {
                        match run_agent_tool(self, call, cancel, tx).await {
                            Ok((s, spent)) => {
                                // Roll subagent tokens into the parent session so
                                // totals + the Orca status stay honest.
                                usage.add_external(&spent);
                                session.usage.add(&spent);
                                let _ = tx.send(AgentEvent::Usage {
                                    session: usage.session_usage().clone(),
                                    last: spent,
                                });
                                (s, true)
                            }
                            Err(MuseError::Interrupted) => {
                                pair_interrupted(&mut session.input_items, &calls);
                                let _ = session.save();
                                return Err(MuseError::Interrupted);
                            }
                            Err(e) => (format!("error: {e}"), false),
                        }
                    }
                } else {
                    let host = ToolHost {
                        todos: self.tools.todos.clone(),
                        plan: self.tools.plan.clone(),
                    };
                    let cwd = self.cwd.clone();
                    let name = call.name.clone();
                    let args = call.arguments.clone();
                    let cancel_t = cancel.clone();
                    let exec = tokio::task::spawn_blocking(move || {
                        host.dispatch(
                            &name,
                            &args,
                            &ToolContext {
                                cwd,
                                cancel: cancel_t,
                            },
                        )
                    });
                    tokio::select! {
                        _ = cancel.cancelled() => {
                            pair_interrupted(&mut session.input_items, &calls);
                            let _ = session.save();
                            return Err(MuseError::Interrupted);
                        }
                        r = exec => match r {
                            Ok(Ok(s)) => (s, true),
                            Ok(Err(e)) => (format!("error: {e}"), false),
                            Err(e) => (format!("error: tool panicked: {e}"), false),
                        },
                    }
                };

                emit_side_effects(tx, &call.name, &body);
                let _ = tx.send(AgentEvent::ToolEnd {
                    id,
                    name: call.name.clone(),
                    result: body.clone(),
                    ok,
                });
                session
                    .input_items
                    .push(function_call_output_item(&call.call_id, &body));
                idx += 1;
            }

            let _ = session.save();
        }
    }

    async fn check_approval(
        &self,
        name: &str,
        args: &str,
        tx: &mpsc::UnboundedSender<AgentEvent>,
    ) -> bool {
        let mode = self.permission_mode.get();
        let read_only = is_read_only_call(name, args);

        match mode {
            PermissionMode::Auto => true,
            PermissionMode::Plan => {
                if read_only && name != "agent" {
                    true
                } else {
                    let _ = tx.send(AgentEvent::Status(format!("plan mode blocked · {name}")));
                    false
                }
            }
            PermissionMode::Manual => {
                if read_only {
                    return true;
                }
                if let Ok(set) = self.approved_tools.lock() {
                    if set.contains(name) {
                        return true;
                    }
                }
                let (otx, orx) = oneshot::channel();
                if tx
                    .send(AgentEvent::ApprovalRequest {
                        name: name.to_string(),
                        args: args.to_string(),
                        respond: otx,
                    })
                    .is_err()
                {
                    return false;
                }
                match orx.await {
                    Ok(ApprovalDecision::Approve) => true,
                    Ok(ApprovalDecision::ApproveAlways) => {
                        if let Ok(mut set) = self.approved_tools.lock() {
                            set.insert(name.to_string());
                        }
                        true
                    }
                    Ok(ApprovalDecision::Deny) => false,
                    Err(_) => self.permission_mode.get().auto_approves(),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(id: &str, name: &str) -> FunctionCallRef {
        FunctionCallRef {
            call_id: id.into(),
            name: name.into(),
            arguments: "{}".into(),
        }
    }

    /// Every function_call must end up with exactly one output — the Responses
    /// API 400s otherwise. This is the invariant the cancel paths must hold.
    fn assert_fully_paired(items: &[Value], calls: &[FunctionCallRef]) {
        for c in calls {
            let n = items
                .iter()
                .filter(|v| {
                    v.get("type").and_then(|t| t.as_str()) == Some("function_call_output")
                        && v.get("call_id").and_then(|i| i.as_str()) == Some(c.call_id.as_str())
                })
                .count();
            assert_eq!(n, 1, "call {} has {n} outputs, expected exactly 1", c.call_id);
        }
    }

    #[test]
    fn cancel_before_any_tool_pairs_every_call() {
        let calls = vec![call("a", "read_file"), call("b", "bash"), call("c", "grep")];
        let mut items: Vec<Value> = Vec::new();
        assert_eq!(pair_interrupted(&mut items, &calls), 3);
        assert_fully_paired(&items, &calls);
    }

    #[test]
    fn cancel_mid_parallel_batch_pairs_only_the_unanswered() {
        // Batch of 3 reads; the first two answered before the user hit Esc.
        let calls = vec![
            call("a", "read_file"),
            call("b", "grep"),
            call("c", "glob"),
            call("d", "bash"), // post-batch, never started
        ];
        let mut items = vec![
            function_call_output_item("a", "contents"),
            function_call_output_item("b", "matches"),
        ];
        assert_eq!(pair_interrupted(&mut items, &calls), 2); // only c and d
        assert_fully_paired(&items, &calls);
        // Answered calls keep their real results — not overwritten by the interrupt.
        let a = items
            .iter()
            .find(|v| v.get("call_id").and_then(|i| i.as_str()) == Some("a"))
            .unwrap();
        assert_eq!(a.get("output").and_then(|o| o.as_str()), Some("contents"));
    }

    #[test]
    fn pairing_is_idempotent() {
        let calls = vec![call("a", "bash")];
        let mut items: Vec<Value> = Vec::new();
        pair_interrupted(&mut items, &calls);
        assert_eq!(pair_interrupted(&mut items, &calls), 0, "must not duplicate");
        assert_fully_paired(&items, &calls);
    }

    /// Parallel batches skip the approval gate, so anything parallel-safe MUST
    /// be read-only — otherwise a write could run without asking.
    #[test]
    fn parallel_safe_implies_approval_free() {
        for name in [
            "read_file", "list_dir", "grep", "glob", "web_fetch", "web_search", "git_status",
            "git_diff", "skill", "write_file", "edit_file", "multi_edit", "apply_patch", "bash",
            "agent", "memory", "todo_write", "submit_plan",
        ] {
            if is_parallel_safe(name, "{}") {
                assert!(
                    is_read_only_call(name, "{}"),
                    "{name} is parallel-safe but not read-only — it would bypass approval"
                );
            }
        }
    }

    #[test]
    fn mutating_tools_are_never_parallel_safe() {
        for name in ["write_file", "edit_file", "multi_edit", "apply_patch", "bash", "agent"] {
            assert!(!is_parallel_safe(name, "{}"), "{name} must run sequentially");
            assert!(!is_read_only_call(name, "{}"), "{name} must need approval");
        }
    }

    #[test]
    fn memory_read_is_free_but_append_needs_approval() {
        assert!(is_read_only_call("memory", r#"{"action":"read"}"#));
        assert!(!is_read_only_call("memory", r#"{"action":"append","text":"x"}"#));
        assert!(!is_read_only_call("memory", "{}"), "unspecified action must not be free");
        // …and memory never rides a parallel batch (it can mutate).
        assert!(!is_parallel_safe("memory", r#"{"action":"read"}"#));
    }
}

pub(crate) const INTERRUPT_OUTPUT: &str = "[interrupted by user]";

/// Pair every function_call in `calls` that has no `function_call_output` yet
/// with an interrupt output.
///
/// Invariant: the Responses API rejects a request in which a `function_call`
/// has no matching `function_call_output`, so a cancel must never leave a gap —
/// including mid-parallel-batch, where some calls have already answered.
/// Idempotent and order-independent: safe to call at any cancel site with the
/// full call list. Returns how many were filled.
pub(crate) fn pair_interrupted(items: &mut Vec<Value>, calls: &[FunctionCallRef]) -> usize {
    let answered: std::collections::HashSet<&str> = items
        .iter()
        .filter(|v| v.get("type").and_then(|t| t.as_str()) == Some("function_call_output"))
        .filter_map(|v| v.get("call_id").and_then(|c| c.as_str()))
        .collect();
    let missing: Vec<String> = calls
        .iter()
        .filter(|c| !answered.contains(c.call_id.as_str()))
        .map(|c| c.call_id.clone())
        .collect();
    let n = missing.len();
    for call_id in missing {
        items.push(function_call_output_item(&call_id, INTERRUPT_OUTPUT));
    }
    n
}

fn is_read_only_call(name: &str, args: &str) -> bool {
    if name == "memory" {
        return serde_json::from_str::<Value>(args)
            .ok()
            .and_then(|v| v.get("action")?.as_str().map(|s| s == "read"))
            .unwrap_or(false);
    }
    if name == "agent" {
        return false;
    }
    READ_ONLY_TOOLS.contains(&name)
}

fn is_parallel_safe(name: &str, args: &str) -> bool {
    // Never parallelize agent or anything that needs approval / mutates.
    if !is_read_only_call(name, args) {
        return false;
    }
    matches!(
        name,
        "read_file"
            | "list_dir"
            | "grep"
            | "glob"
            | "web_fetch"
            | "web_search"
            | "git_status"
            | "git_diff"
            | "skill"
    )
}

fn should_auto_compact(usage: &UsageTracker, cfg: &Config) -> bool {
    let last = usage.last_usage();
    // Prefer input tokens (what pressures the next request window).
    let used = if last.input_tokens > 0 {
        last.input_tokens
    } else {
        last.total_tokens
    };
    let window = cfg.context_window.max(1);
    used > (window as f64 * 0.55) as u64 && used > 40_000
}

fn emit_side_effects(tx: &mpsc::UnboundedSender<AgentEvent>, name: &str, body: &str) {
    if name == "todo_write" {
        let _ = tx.send(AgentEvent::TodosChanged(body.to_string()));
    }
    if name == "submit_plan" {
        let _ = tx.send(AgentEvent::PlanSubmitted(body.to_string()));
    }
}

async fn run_agent_tool(
    runner: &AgentRunner,
    call: &FunctionCallRef,
    cancel: &CancellationToken,
    tx: &mpsc::UnboundedSender<AgentEvent>,
) -> Result<(String, TokenUsage)> {
    let v: Value = serde_json::from_str(&call.arguments).unwrap_or(serde_json::json!({}));
    let prompt = v
        .get("prompt")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    if prompt.is_empty() {
        return Err(MuseError::Tool("agent.prompt required".into()));
    }
    let kind = v
        .get("subagent_type")
        .and_then(|x| x.as_str())
        .unwrap_or("explore");
    let desc = v
        .get("description")
        .and_then(|x| x.as_str())
        .unwrap_or(kind);
    let _ = tx.send(AgentEvent::Status(format!("subagent · {desc}")));

    subagent::run_subagent(
        runner.client.clone(),
        runner.config.clone(),
        runner.cwd.clone(),
        runner.permission_mode.clone(),
        &prompt,
        kind,
        cancel,
    )
    .await
}

pub async fn compact_session(
    runner: &AgentRunner,
    session: &mut Session,
    usage: &mut UsageTracker,
) -> Result<String> {
    let mut items = session.input_items.clone();
    items.push(user_text_item(
        "Summarize this conversation for a fresh context window. Capture: goals, decisions, \
         files touched, current state, pending next steps. Dense bullets.",
    ));
    let req = ResponseRequest {
        model: runner.config.model.clone(),
        input: Value::Array(items),
        instructions: Some("You compress agent conversations into handoff summaries.".into()),
        tools: None,
        tool_choice: None,
        store: Some(false),
        include: Some(vec!["reasoning.encrypted_content".into()]),
        reasoning: Some(ReasoningConfig {
            effort: Some("low".into()),
            summary: None,
        }),
        stream: Some(false),
        parallel_tool_calls: None,
    };
    let resp = runner.client.create_response(&req).await?;
    if let Some(u) = &resp.usage {
        let tu: TokenUsage = u.into();
        usage.record_request(tu.clone(), resp.id.clone());
        session.usage.add(&tu);
    }
    let summary = resp.output_text();
    if summary.is_empty() {
        return Err(MuseError::Other("compaction produced no summary".into()));
    }
    session.input_items = vec![user_text_item(&format!(
        "[Context compacted. Summary of the conversation so far:]\n\n{summary}"
    ))];
    let _ = session.save();
    Ok(summary)
}

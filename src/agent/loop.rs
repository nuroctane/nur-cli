use super::hooks::HooksConfig;
use super::mode::{PermissionMode, SharedMode};
use super::permissions::{RuleDecision, SharedPermissions};
use super::prompt::PromptContext;
use super::receipt;
use super::session::Session;
use super::subagent;
use crate::api::types::{
    function_call_output_item, replay_output_items, user_multimodal_item, user_text_item,
    FunctionCallRef, ReasoningConfig, ResponseRequest,
};
use crate::api::{ApiResponse, ApiClient, StreamEvent};
use crate::config::Config;
use crate::error::{MuseError, Result};
use crate::tools::media::{self, MediaAttach};
use crate::tools::{
    is_parallel_safe, is_read_only_call, spill, ToolContext, ToolHost,
};
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

// Tool capability classification (read-only / parallel / destructive) lives in
// `crate::tools::capabilities` — single source of truth for the agent loop.

pub struct AgentRunner {
    pub client: ApiClient,
    pub config: Config,
    pub cwd: PathBuf,
    pub permission_mode: SharedMode,
    #[allow(dead_code)]
    pub verbose: bool,
    pub approved_tools: Arc<Mutex<HashSet<String>>>,
    pub tools: ToolHost,
    /// Optional allow/deny/ask patterns (`permissions.toml`). Empty = no change.
    pub permissions: SharedPermissions,
    /// Optional pre/post tool hooks (`hooks.toml`). Inactive when file missing.
    pub hooks: HooksConfig,
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

/// Run one turn to completion **off the UI** and return the final answer text
/// with the (restored) session + usage. Used by headless integrations — the
/// Telegram gateway and `bench` — that need the answer, not a live stream.
///
/// Auto-approval is the caller's responsibility: build the runner with a
/// permission mode of `Auto`, otherwise any tool that needs approval is denied
/// (there is no interactive approver here).
pub async fn run_collect(
    runner: Arc<AgentRunner>,
    session: Session,
    usage: UsageTracker,
    prompt: String,
    cancel: CancellationToken,
) -> (
    Box<Session>,
    Box<UsageTracker>,
    std::result::Result<String, String>,
    bool,
) {
    let (tx, mut rx) = mpsc::unbounded_channel();
    spawn_turn(runner, session, usage, prompt, tx, cancel);
    let mut acc = String::new();
    while let Some(ev) = rx.recv().await {
        match ev {
            AgentEvent::TextDelta(d) => acc.push_str(&d),
            AgentEvent::AssistantMessage(m) => {
                if acc.trim().is_empty() {
                    acc = m;
                }
            }
            // No interactive approver in headless integrations — deny anything
            // that slips through (shouldn't happen: callers run in Auto mode).
            AgentEvent::ApprovalRequest { respond, .. } => {
                let _ = respond.send(ApprovalDecision::Deny);
            }
            AgentEvent::Done {
                session,
                usage,
                result,
                interrupted,
            } => {
                let result =
                    result.map(|t| if t.trim().is_empty() { acc.clone() } else { t });
                return (session, usage, result, interrupted);
            }
            _ => {}
        }
    }
    // spawn_turn always emits Done as its last act, so the channel never closes
    // before it — but stay honest if that invariant ever breaks.
    unreachable!("agent turn ended without a Done event")
}

/// Which provider/model actually served a model request (for the receipt).
struct Served {
    provider: String,
    model: String,
    failover: bool,
}

impl AgentRunner {
    /// Run one model request against `client`, forwarding stream events to the
    /// UI. Returns `(response, text_deltas_emitted)` on success, or
    /// `(error, text_deltas_emitted)` so the caller can tell whether failing
    /// over is safe (only when nothing was streamed yet).
    async fn stream_one(
        &self,
        client: &ApiClient,
        req: &ResponseRequest,
        tx: &mpsc::UnboundedSender<AgentEvent>,
        cancel: &CancellationToken,
    ) -> std::result::Result<(ApiResponse, usize), (MuseError, usize)> {
        let mut deltas = 0usize;
        if req.stream == Some(true) {
            let r = client
                .create_response_stream(
                    req,
                    |ev| match ev {
                        StreamEvent::TextDelta(d) => {
                            deltas += 1;
                            let _ = tx.send(AgentEvent::TextDelta(d));
                        }
                        StreamEvent::ReasoningDelta(d) => {
                            let _ = tx.send(AgentEvent::ReasoningDelta(d));
                        }
                        StreamEvent::Completed(_) => {}
                    },
                    cancel,
                )
                .await;
            match r {
                Ok(resp) => Ok((resp, deltas)),
                Err(e) => Err((e, deltas)),
            }
        } else {
            tokio::select! {
                _ = cancel.cancelled() => Err((MuseError::Interrupted, 0)),
                r = client.create_response(req) => match r {
                    Ok(resp) => Ok((resp, 0)),
                    Err(e) => Err((e, 0)),
                },
            }
        }
    }

    /// One model request with opt-in cross-provider failover. Tries the active
    /// provider first; on a retryable server error **before any text streamed**,
    /// retries the same request against each configured fallback provider in
    /// turn. Never fails over once output has begun, so the transcript never
    /// shows duplicated text.
    async fn request_with_failover(
        &self,
        req: &ResponseRequest,
        tx: &mpsc::UnboundedSender<AgentEvent>,
        cancel: &CancellationToken,
    ) -> Result<(ApiResponse, usize, Served)> {
        let primary_err = match self.stream_one(&self.client, req, tx, cancel).await {
            Ok((resp, deltas)) => {
                return Ok((
                    resp,
                    deltas,
                    Served {
                        provider: self.config.provider.clone(),
                        model: self.config.model.clone(),
                        failover: false,
                    },
                ))
            }
            Err((e, emitted)) => {
                if emitted > 0 || !crate::api::failover::should_failover(&e) {
                    return Err(e);
                }
                e
            }
        };

        // Privacy floor: never fail over to a weaker data-privacy tier than the
        // active provider unless explicitly allowed (see `providers::Privacy`).
        let active_privacy =
            crate::providers::effective_privacy(&self.config.provider_privacy, &self.config.provider);
        let allowed: Vec<String> = self
            .config
            .fallback_providers
            .iter()
            .filter(|id| {
                let r = crate::providers::effective_privacy(&self.config.provider_privacy, id).rank();
                crate::api::failover::privacy_allowed(
                    active_privacy.rank(),
                    r,
                    self.config.failover_allow_downgrade,
                )
            })
            .cloned()
            .collect();
        let dropped = self.config.fallback_providers.len() - allowed.len();

        let targets = crate::api::failover::plan_targets(
            &self.config.provider,
            &allowed,
            crate::api::failover::resolve_target_key,
        );
        if targets.is_empty() {
            if dropped > 0 {
                let _ = tx.send(AgentEvent::Status(format!(
                    "failover skipped {dropped} provider(s) weaker than your {} tier — \
                     enable failover_allow_downgrade or raise their privacy tags to allow",
                    active_privacy.as_str()
                )));
            } else if self.config.fallback_providers.is_empty() {
                let _ = tx.send(AgentEvent::Status(
                    "no failover chain — /failover to add backups (or set fallback_providers); \
                     primary is the only route"
                        .into(),
                ));
            } else {
                let _ = tx.send(AgentEvent::Status(
                    "failover chain has no usable credentials — save a key/OAuth for each \
                     fallback via /failover (or that provider's env key)"
                        .into(),
                ));
            }
            return Err(primary_err);
        }

        let mut last = primary_err;
        for t in targets {
            let _ = tx.send(AgentEvent::Status(format!(
                "provider error ({last}) — failing over to {} · {}",
                t.provider_id, t.model
            )));
            let client = match ApiClient::for_provider(&t.base_url, &t.api_key, &t.provider_id) {
                Ok(c) => c.with_style(t.style),
                Err(e) => {
                    last = e;
                    continue;
                }
            };
            let mut req2 = req.clone();
            req2.model = t.model.clone();
            match self.stream_one(&client, &req2, tx, cancel).await {
                Ok((resp, deltas)) => {
                    return Ok((
                        resp,
                        deltas,
                        Served {
                            provider: t.provider_id.clone(),
                            model: t.model.clone(),
                            failover: true,
                        },
                    ))
                }
                Err((e, emitted)) => {
                    if emitted > 0 || !crate::api::failover::should_failover(&e) {
                        return Err(e);
                    }
                    last = e;
                }
            }
        }
        Err(last)
    }

    pub async fn run_turn_events(
        &self,
        session: &mut Session,
        user_text: &str,
        usage: &mut UsageTracker,
        tx: &mpsc::UnboundedSender<AgentEvent>,
        cancel: &CancellationToken,
    ) -> Result<String> {
        // Discard any media a prior turn queued but never flushed (e.g. `look`
        // ran, then the turn was cancelled before the attach) so a stale image
        // can't bleed onto this unrelated prompt.
        let _ = media::take_pending_media();
        session.push_user(user_text);
        // Auto-attach media paths mentioned in the user prompt (png/mp4/…).
        let auto_notes = media::auto_attach_from_text(&self.cwd, user_text);
        let pending = media::take_pending_media();
        if pending.is_empty() {
            session.input_items.push(user_text_item(user_text));
        } else {
            let mut text = user_text.to_string();
            if !auto_notes.is_empty() {
                text.push_str("\n\n[media auto-attached]\n");
                text.push_str(&auto_notes.join("\n"));
            }
            session
                .input_items
                .push(multimodal_user_item(&text, &pending));
            let _ = tx.send(AgentEvent::Status(format!(
                "vision · attached {} media file(s) from prompt",
                pending.len()
            )));
        }

        let tools = self.tools.tool_defs();
        // Disk-backed prompt parts (skills, NUR.md, memory, shell) — read once
        // per user turn, not once per model request. Pass user_text so natural
        // language (e.g. "think like fable") can auto-activate skills.
        let provider_label = crate::config::active_provider_label(&self.config);
        let prompt_ctx = PromptContext::build_with_opts(
            &self.cwd,
            self.is_subagent,
            &self.config.model,
            &provider_label,
            self.config.poor_mode,
            Some(user_text),
        );
        if prompt_ctx.has_skill_activation() {
            let label = prompt_ctx
                .skill_activation_label()
                .unwrap_or("skill");
            let _ = tx.send(AgentEvent::Status(format!(
                "{label} · activated from your wording (no slash command needed)"
            )));
        }
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
            if let Some(msg) = session_budget_exceeded(&self.config, usage) {
                let _ = tx.send(AgentEvent::Status(msg.clone()));
                return Err(MuseError::Budget(msg));
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
                // One cache key per session so system instructions + tools can be
                // prefix-cached across multi-turn agent loops.
                prompt_cache_key: Some(session.id.clone()),
            };

            let (resp, text_deltas, served): (ApiResponse, usize, Served) =
                self.request_with_failover(&req, tx, cancel).await?;

            let (in_tok, out_tok) = if let Some(u) = &resp.usage {
                let tu: TokenUsage = u.into();
                usage.record_request(tu.clone(), resp.id.clone());
                session.usage.add(&tu);
                let toks = (tu.input_tokens, tu.output_tokens);
                let _ = tx.send(AgentEvent::Usage {
                    session: usage.session_usage().clone(),
                    last: tu,
                });
                toks
            } else {
                (0, 0)
            };

            // Session receipt: record where this request actually went.
            receipt::record(
                &session.id,
                receipt::Event::Model {
                    provider: served.provider.clone(),
                    model: served.model.clone(),
                    privacy: crate::providers::effective_privacy(
                        &self.config.provider_privacy,
                        &served.provider,
                    )
                    .as_str()
                    .to_string(),
                    failover: served.failover,
                    input_tokens: in_tok,
                    output_tokens: out_tok,
                },
            );

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
                        let body = spill::maybe_spill(
                            &session.id,
                            &name,
                            body,
                            self.config.tool_result_max_chars as usize,
                        );
                        receipt::record(
                            &session.id,
                            receipt::Event::Tool {
                                name: name.clone(),
                                args_sha256: None,
                                result_sha256: receipt::sha256_hex(body.as_bytes()),
                                ok,
                            },
                        );
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
                    flush_pending_media(&mut session.input_items, tx);
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
                                "blocked in plan mode — {} needs manual/auto (Shift+Tab). \
                                 Plan allows reads, analysis, and non-mutating shell (incl. \
                                 ffmpeg/scratch work); it blocks code edits and repo/VCS commits. \
                                 Describe the change instead, or ask the user to switch mode.",
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
                    // Pre-tool hook (optional) — blocks on non-zero exit.
                    if let Err(e) = self.hooks.run_pre(
                        &call.name,
                        &call.arguments,
                        &self.cwd,
                        &session.id,
                    ) {
                        let msg = format!("error: {e}");
                        let _ = tx.send(AgentEvent::ToolEnd {
                            id,
                            name: call.name.clone(),
                            result: msg.clone(),
                            ok: false,
                        });
                        session
                            .input_items
                            .push(function_call_output_item(&call.call_id, &msg));
                        idx += 1;
                        continue;
                    }
                    // Snapshot the target before a single-file mutating tool so
                    // `/undo` can restore it. Best-effort; never blocks the tool.
                    if matches!(call.name.as_str(), "write_file" | "edit_file" | "multi_edit") {
                        if let Ok(v) = serde_json::from_str::<Value>(&call.arguments) {
                            if let Some(p) = v.get("path").and_then(|p| p.as_str()) {
                                if let Ok(abs) = crate::tools::resolve_path(&self.cwd, p) {
                                    crate::tools::undo::record(&session.id, &abs);
                                }
                            }
                        }
                    }
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

                let body = if ok {
                    spill::maybe_spill(
                        &session.id,
                        &call.name,
                        body,
                        self.config.tool_result_max_chars as usize,
                    )
                } else {
                    // Keep error messages intact (usually short).
                    body
                };
                receipt::record(
                    &session.id,
                    receipt::Event::Tool {
                        name: call.name.clone(),
                        args_sha256: Some(receipt::sha256_hex(call.arguments.as_bytes())),
                        result_sha256: receipt::sha256_hex(body.as_bytes()),
                        ok,
                    },
                );
                self.hooks.run_post(
                    &call.name,
                    &call.arguments,
                    &self.cwd,
                    &session.id,
                );
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
                flush_pending_media(&mut session.input_items, tx);
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

        // 1) Explicit deny always wins (including auto mode).
        if self.permissions.decide(name, args) == Some(RuleDecision::Deny) {
            let _ = tx.send(AgentEvent::Status(format!(
                "denied by permissions.toml · {name}"
            )));
            return false;
        }

        // 2) Plan-mode structural gates (cannot be overridden by allow rules).
        if mode == PermissionMode::Plan {
            let plan_ok = plan_mode_allows(name, args, read_only, tx);
            if !plan_ok {
                return false;
            }
            // Plan allowed — still honor ask rules (force a prompt).
            if self.permissions.decide(name, args) == Some(RuleDecision::Ask) {
                return self.prompt_approval(name, args, tx).await;
            }
            return true;
        }

        // 3) Allow rule skips approval (manual) / short-circuits auto.
        if self.permissions.decide(name, args) == Some(RuleDecision::Allow) {
            return true;
        }

        // 4) Ask rule forces a prompt even in auto.
        if self.permissions.decide(name, args) == Some(RuleDecision::Ask) {
            return self.prompt_approval(name, args, tx).await;
        }

        // 5) Mode default.
        match mode {
            PermissionMode::Auto => true,
            PermissionMode::Plan => true, // handled above
            PermissionMode::Manual => {
                if read_only {
                    return true;
                }
                if let Ok(set) = self.approved_tools.lock() {
                    if set.contains(name) {
                        return true;
                    }
                }
                self.prompt_approval(name, args, tx).await
            }
        }
    }

    async fn prompt_approval(
        &self,
        name: &str,
        args: &str,
        tx: &mpsc::UnboundedSender<AgentEvent>,
    ) -> bool {
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

/// Plan-mode structural allow/deny (same rules as before permissions.toml).
fn plan_mode_allows(
    name: &str,
    args: &str,
    read_only: bool,
    tx: &mpsc::UnboundedSender<AgentEvent>,
) -> bool {
    if read_only && name != "agent" {
        return true;
    }
    if name == "extract_frames" {
        return true;
    }
    if name == "browser" && crate::tools::browser::is_plan_safe_action(args) {
        return true;
    }
    if name == "bash" {
        let cmd = serde_json::from_str::<Value>(args)
            .ok()
            .and_then(|v| v.get("command").and_then(|c| c.as_str()).map(String::from))
            .unwrap_or_default();
        return match plan_blocks_shell(&cmd) {
            None => true,
            Some(reason) => {
                let _ = tx.send(AgentEvent::Status(format!("plan mode · {reason}")));
                false
            }
        };
    }
    let _ = tx.send(AgentEvent::Status(format!("plan mode blocked · {name}")));
    false
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
            "read_file", "list_dir", "grep", "glob", "web_fetch", "web_search", "look",
            "extract_frames", "git_status", "git_diff", "skill", "write_file", "edit_file",
            "multi_edit", "apply_patch", "bash", "agent", "memory", "todo_write", "submit_plan",
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
        for name in [
            "write_file",
            "edit_file",
            "multi_edit",
            "apply_patch",
            "bash",
            "agent",
            "extract_frames",
        ] {
            assert!(!is_parallel_safe(name, "{}"), "{name} must run sequentially");
            assert!(!is_read_only_call(name, "{}"), "{name} must need approval");
        }
        assert!(is_read_only_call("look", r#"{"path":"x.png"}"#));
        assert!(is_parallel_safe("look", r#"{"path":"x.png"}"#));
    }

    #[test]
    fn memory_read_is_free_but_append_needs_approval() {
        assert!(is_read_only_call("memory", r#"{"action":"read"}"#));
        assert!(!is_read_only_call("memory", r#"{"action":"append","text":"x"}"#));
        assert!(!is_read_only_call("memory", "{}"), "unspecified action must not be free");
        // …and memory never rides a parallel batch (it can mutate).
        assert!(!is_parallel_safe("memory", r#"{"action":"read"}"#));
    }

    #[test]
    fn graphify_query_is_free_but_extract_needs_approval() {
        assert!(is_read_only_call(
            "graphify",
            r#"{"action":"query","question":"auth flow"}"#
        ));
        assert!(is_read_only_call("graphify", r#"{"action":"status"}"#));
        assert!(is_read_only_call(
            "graphify",
            r#"{"action":"path","from":"A","to":"B"}"#
        ));
        assert!(!is_read_only_call("graphify", r#"{"action":"extract"}"#));
        assert!(!is_read_only_call("graphify", r#"{"action":"update"}"#));
        assert!(is_parallel_safe(
            "graphify",
            r#"{"action":"query","question":"x"}"#
        ));
        assert!(!is_parallel_safe("graphify", r#"{"action":"extract"}"#));
    }

    #[test]
    fn excalidraw_status_is_free_but_create_needs_approval() {
        assert!(is_read_only_call("excalidraw", r#"{"action":"status"}"#));
        assert!(is_read_only_call("excalidraw", r#"{"action":"reference"}"#));
        assert!(is_read_only_call(
            "excalidraw",
            r#"{"action":"checkpoint","checkpoint_action":"list"}"#
        ));
        assert!(!is_read_only_call(
            "excalidraw",
            r#"{"action":"create","output":"x.excalidraw"}"#
        ));
        assert!(!is_read_only_call(
            "excalidraw",
            r#"{"action":"export","path":"x.excalidraw"}"#
        ));
        assert!(is_parallel_safe("excalidraw", r#"{"action":"status"}"#));
        assert!(!is_parallel_safe(
            "excalidraw",
            r#"{"action":"create","output":"x.excalidraw"}"#
        ));
    }

    #[test]
    fn plan_shell_allows_analysis_blocks_repo_mutation() {
        // Reading / parsing / scratch / media compute — all free in plan mode.
        for ok in [
            "ls -la",
            "cat src/main.rs",
            "grep -rn TODO src",
            "rg 'fn main' -n",
            "python analyze.py --report",
            "cargo build",
            "cargo test",
            "npm run build",
            "ffmpeg -i demo.mp4 -vf fps=1 /tmp/f%02d.jpg",
            "cp demo.mp4 /tmp/clip.mp4",
            "git status",
            "git diff HEAD~1",
            "git log --oneline",
            "git fetch origin",
        ] {
            assert!(plan_blocks_shell(ok).is_none(), "should allow: {ok}");
        }
        // Repo/VCS mutation, publishing, and installs — blocked.
        for bad in [
            "git commit -m 'x'",
            "git push origin main",
            "git add -A",
            "git checkout main",
            "git reset --hard",
            "git restore src/x.rs",
            "git rebase -i HEAD~3",
            "git pull",
            "gh pr create --fill",
            "gh pr merge 12",
            "npm install",
            "npm i react",
            "pnpm add lodash",
            "yarn add axios",
            "pip install requests",
            "cargo add serde",
            "cargo install ripgrep",
            "cargo update",
        ] {
            assert!(plan_blocks_shell(bad).is_some(), "should block: {bad}");
        }
    }

    #[test]
    fn plur_and_ruflo_gates() {
        assert!(is_read_only_call("plur", r#"{"action":"recall","query":"x"}"#));
        assert!(is_read_only_call("plur", r#"{"action":"status"}"#));
        assert!(!is_read_only_call(
            "plur",
            r#"{"action":"learn","statement":"prefer tabs"}"#
        ));
        assert!(is_read_only_call(
            "ruflo",
            r#"{"action":"memory_search","query":"auth"}"#
        ));
        assert!(!is_read_only_call(
            "ruflo",
            r#"{"action":"memory_store","key":"k","value":"v"}"#
        ));
        assert!(!is_read_only_call("ruflo", r#"{"action":"swarm_init"}"#));
    }

    #[test]
    fn omp_run_is_write_class() {
        // status/version probes are free; a run drives a full coding agent.
        assert!(is_read_only_call("omp", r#"{"action":"status"}"#));
        assert!(is_read_only_call("omp", r#"{"action":"version"}"#));
        assert!(!is_read_only_call("omp", r#"{"action":"run","prompt":"x"}"#));
        assert!(
            !is_read_only_call("omp", "{}"),
            "default action=run must not be free"
        );
        assert!(!is_parallel_safe("omp", r#"{"action":"status"}"#));
    }

    #[test]
    fn session_budget_trips_on_cost_and_tokens() {
        use crate::usage::TokenUsage;
        let mut cfg = Config::default();
        let mut usage = UsageTracker::new("t".into(), "m".into(), PathBuf::from("."));
        assert!(session_budget_exceeded(&cfg, &usage).is_none());
        cfg.max_session_cost_usd = Some(0.01);
        // Seed enough tokens that estimated cost exceeds $0.01 at default prices.
        let mut u = TokenUsage::default();
        u.input_tokens = 50_000;
        u.total_tokens = 50_000;
        usage.seed_session(u.clone());
        assert!(session_budget_exceeded(&cfg, &usage).is_some());
        cfg.max_session_cost_usd = None;
        cfg.max_session_tokens = Some(10_000);
        assert!(session_budget_exceeded(&cfg, &usage).is_some());
        cfg.max_session_tokens = Some(1_000_000);
        assert!(session_budget_exceeded(&cfg, &usage).is_none());
    }

    #[test]
    fn browser_perception_is_free_control_is_gated() {
        for free in ["tabs", "scan", "snapshot", "tabtree", "status", "console", "network"] {
            let a = format!(r#"{{"action":"{free}"}}"#);
            assert!(is_read_only_call("browser", &a), "{free} should be free");
        }
        for gated in ["open", "click", "fill", "send_keys", "exec", "close", "screenshot"] {
            let a = format!(r#"{{"action":"{gated}"}}"#);
            assert!(!is_read_only_call("browser", &a), "{gated} must need approval");
        }
        // Screenshot is plan-safe perception (writes an image, like extract_frames).
        assert!(crate::tools::browser::is_plan_safe_action(
            r#"{"action":"screenshot"}"#
        ));
        assert!(!crate::tools::browser::is_plan_safe_action(
            r#"{"action":"exec","js":"x"}"#
        ));
        assert!(!is_parallel_safe("browser", r#"{"action":"tabs"}"#));
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

/// Returns a human-readable reason when the session has hit a configured
/// cost or token ceiling (checked before each API call).
pub fn session_budget_exceeded(cfg: &Config, usage: &UsageTracker) -> Option<String> {
    let u = usage.session_usage();
    if let Some(max) = cfg.max_session_cost_usd {
        let cost = u.estimated_cost_usd();
        if cost >= max {
            return Some(format!(
                "session cost ${cost:.4} ≥ budget ${max:.4} — raise with /budget cost <n> or max_session_cost_usd in config"
            ));
        }
    }
    if let Some(max) = cfg.max_session_tokens {
        if u.total_tokens >= max {
            return Some(format!(
                "session tokens {} ≥ budget {} — raise with /budget tokens <n> or max_session_tokens in config",
                u.total_tokens, max
            ));
        }
    }
    None
}

/// In PLAN mode, shell runs freely for reading, parsing, analysis, and scratch
/// or media work (ffmpeg keyframes, copying a clip to temp, analysis scripts).
/// It is refused only when it would change the repository's committed state or
/// install dependencies — i.e. "no submitting changes / no code input", while
/// non-mutating compute stays free. Returns a short reason when blocked.
pub fn plan_blocks_shell(command: &str) -> Option<&'static str> {
    let c = format!(" {} ", command.to_ascii_lowercase().replace(['\t', '\n'], " "));
    // Git working-tree / index / publish mutations (fetch is read-only, allowed).
    const GIT_MUT: &[&str] = &[
        "git commit", "git push", "git add", "git reset", "git checkout", "git restore",
        "git stash", "git merge", "git rebase", "git cherry-pick", "git revert", "git rm",
        "git mv", "git clean", "git apply", "git tag ", "git pull", "git switch",
    ];
    if GIT_MUT.iter().any(|p| c.contains(p)) {
        return Some("git repo/VCS mutation is blocked in plan mode — Shift+Tab to manual/auto to commit or change tracked files");
    }
    // PR / release publishing via gh.
    const GH_MUT: &[&str] = &[
        "gh pr create", "gh pr merge", "gh pr close", "gh pr edit", "gh pr ready",
        "gh pr comment", "gh pr reopen", "gh release create", "gh release edit",
        "gh release delete", "gh repo create", "gh repo delete", "gh repo edit",
        "gh issue create", "gh issue edit", "gh issue close",
    ];
    if GH_MUT.iter().any(|p| c.contains(p)) {
        return Some("publishing (gh) is blocked in plan mode");
    }
    // Dependency installs mutate lockfiles / the environment.
    const DEP_MUT: &[&str] = &[
        "npm install", "npm i ", "npm ci", "npm add", "npm uninstall", "npm remove",
        "pnpm add", "pnpm install", "pnpm remove", "yarn add", "yarn install", "yarn remove",
        "bun add", "bun install", "pip install", "pip uninstall", "pip3 install",
        "pip3 uninstall", "poetry add", "poetry install", "poetry remove", "cargo add",
        "cargo install", "cargo remove", "cargo publish", "cargo update", "gem install",
        "bundle install", "bundle update", "go get ", "go install", "apt install",
        "apt-get install", "brew install", "dnf install", "yum install",
    ];
    if DEP_MUT.iter().any(|p| c.contains(p)) {
        return Some("dependency install/mutation is blocked in plan mode");
    }
    None
}

/// Attach any media queued by `look` / `extract_frames` as a multimodal user item.
fn flush_pending_media(items: &mut Vec<Value>, tx: &mpsc::UnboundedSender<AgentEvent>) {
    let pending = media::take_pending_media();
    if pending.is_empty() {
        return;
    }
    let n = pending.len();
    let label = pending
        .iter()
        .map(|m| {
            format!(
                "{} ({})",
                PathBuf::from(&m.path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&m.path),
                m.kind.api_type()
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    items.push(multimodal_user_item(
        &format!(
            "[tool media attached for vision — {n} file(s): {label}]\n\
             Inspect the attached image(s)/video carefully. For UI/design work: extract \
             palette, type scale, spacing, radius, shadows, motion cues; then implement."
        ),
        &pending,
    ));
    let _ = tx.send(AgentEvent::Status(format!("vision · {n} attachment(s) ready")));
}

fn multimodal_user_item(text: &str, media: &[MediaAttach]) -> Value {
    let parts: Vec<(&str, &str, &str)> = media
        .iter()
        .map(|m| (m.kind.api_type(), m.kind.url_field(), m.data_url.as_str()))
        .collect();
    user_multimodal_item(text, &parts)
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
    // Snapshot full session before rewrite (never-lose-context; beside .json.bak).
    {
        let path = session.path();
        if path.is_file() {
            let pre = path.with_extension("precompact.bak");
            let _ = std::fs::copy(&path, &pre);
        }
    }

    // Thin old tool bodies for the summarizer so we don't re-pay huge dumps.
    let mut items = session.input_items.clone();
    let thinned = thin_tool_bodies_for_compact(
        &mut items,
        runner.config.compact_tool_body_max_chars as usize,
        runner.config.compact_keep_user_turns as usize,
    );
    items.push(user_text_item(
        "Summarize this conversation for a fresh context window. Capture: goals, decisions, \
         files touched, current state, pending next steps. Prefer decisions over raw tool dumps. \
         Dense bullets.",
    ));
    let req = ResponseRequest {
        model: runner.config.model.clone(),
        input: Value::Array(items),
        instructions: Some(
            "You compress agent conversations into handoff summaries. \
             Preserve goals, decisions, file paths, and next steps; drop redundant tool noise."
                .into(),
        ),
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
        prompt_cache_key: Some(format!("compact:{}", session.id)),
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

    // New context: summary + last N user/assistant display messages (not full tool stream).
    let keep_n = runner.config.compact_keep_user_turns.max(1) as usize;
    let mut new_items = vec![user_text_item(&format!(
        "[Context compacted. Summary of the conversation so far:]\n\n{summary}"
    ))];
    let recent = recent_dialogue_items(&session.messages, keep_n);
    let kept = recent.len();
    new_items.extend(recent);
    session.input_items = new_items;
    let _ = session.save();
    Ok(format!(
        "{summary}\n\n[compact: thinned {thinned} tool bodies · kept {kept} recent dialogue items · \
         precompact bak written]"
    ))
}

/// Truncate oversized `function_call_output` bodies outside the last `keep_user_turns`
/// user messages. Returns how many bodies were thinned.
fn thin_tool_bodies_for_compact(items: &mut [Value], max_chars: usize, keep_user_turns: usize) -> usize {
    if max_chars == 0 {
        return 0;
    }
    let user_idxs: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, it)| it.get("role").and_then(|r| r.as_str()) == Some("user"))
        .map(|(i, _)| i)
        .collect();
    let protect_from = if user_idxs.len() > keep_user_turns.max(1) {
        user_idxs[user_idxs.len() - keep_user_turns.max(1)]
    } else {
        0
    };

    let mut n = 0usize;
    for (i, it) in items.iter_mut().enumerate() {
        if i >= protect_from {
            continue;
        }
        if it.get("type").and_then(|t| t.as_str()) != Some("function_call_output") {
            continue;
        }
        let Some(out) = it.get("output").and_then(|o| o.as_str()) else {
            continue;
        };
        if out.chars().count() <= max_chars {
            continue;
        }
        let preview: String = out.chars().take(max_chars).collect();
        let total = out.chars().count();
        if let Some(m) = it.as_object_mut() {
            m.insert(
                "output".into(),
                Value::String(format!(
                    "{preview}\n… [thinned for compact: {total} → {max_chars} chars]"
                )),
            );
        }
        n += 1;
    }
    n
}

/// Last `keep_user_turns` user messages and any assistant reply immediately after each,
/// as Responses-style user text items (lossy but preserves recent intent).
fn recent_dialogue_items(
    messages: &[crate::agent::session::SessionMessage],
    keep_user_turns: usize,
) -> Vec<Value> {
    let user_idxs: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role == "user")
        .map(|(i, _)| i)
        .collect();
    if user_idxs.is_empty() || keep_user_turns == 0 {
        return Vec::new();
    }
    let start_u = user_idxs.len().saturating_sub(keep_user_turns);
    let from = user_idxs[start_u];
    let mut out = Vec::new();
    for m in &messages[from..] {
        if m.role == "user" {
            out.push(user_text_item(&m.content));
        } else if m.role == "assistant" && !m.content.is_empty() {
            // Fold assistant text as a user-visible note so the model still sees it
            // (Responses multi-turn uses input items; assistant turns live in store/API).
            out.push(user_text_item(&format!(
                "[prior assistant]\n{}",
                m.content.chars().take(4000).collect::<String>()
            )));
        }
    }
    out
}

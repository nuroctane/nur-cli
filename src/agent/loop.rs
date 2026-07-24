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
use crate::api::{ApiClient, ApiResponse, StreamEvent};
use crate::config::Config;
use crate::error::{MuseError, Result};
use crate::tools::media::{self, MediaAttach};
use crate::tools::{is_parallel_safe, is_read_only_call, spill, ToolContext, ToolHost};
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
    ToolStart {
        id: u64,
        name: String,
        args: String,
    },
    ToolEnd {
        id: u64,
        name: String,
        result: String,
        ok: bool,
    },
    /// Todo list changed — TUI should refresh.
    TodosChanged(String),
    /// A subagent was requested on a provider with no stored credentials. The
    /// TUI turns this into a pre-selected `/login` prompt so the user can
    /// authenticate and activate that provider. The subagent does **not** run
    /// on the parent provider — the tool result is a blocked message, and the
    /// TUI re-deploys after login with a structured steer.
    LoginRequired {
        provider_id: String,
        provider_name: String,
        /// The original subagent request that was blocked, so the TUI can
        /// faithfully re-deploy it verbatim once the user completes login
        /// (rather than relying on the model to reconstruct it from context).
        retry_prompt: Option<String>,
        retry_desc: Option<String>,
        /// explore | general (defaults to explore on retry if missing).
        retry_kind: Option<String>,
        /// Optional exact model id the original call requested.
        retry_model: Option<String>,
    },
    /// Plan written via submit_plan.
    PlanSubmitted(String),
    ApprovalRequest {
        name: String,
        args: String,
        respond: oneshot::Sender<ApprovalDecision>,
    },
    Usage {
        session: TokenUsage,
        last: TokenUsage,
    },
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
        if !runner.is_subagent {
            let _ = session.save();
        }
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
                let result = result.map(|t| if t.trim().is_empty() { acc.clone() } else { t });
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
    fn persist_session(&self, session: &Session) {
        if !self.is_subagent {
            let _ = session.save();
        }
    }

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
        // Saturation is a pause, not a failure. Providers that queue per worker
        // (NVIDIA NIM, vLLM, Triton, local servers) refuse admission *mid-stream*
        // on an HTTP 200 — NIM answers `ResourceExhausted: Worker local total
        // request limit reached (90/32)` as an SSE error event. The transport
        // retry in `ApiClient` never sees that, because the response itself
        // succeeded, so a blip that clears in a second used to kill the turn.
        //
        // Only retried when the stream emitted nothing: replaying a request that
        // already wrote text would duplicate it into the transcript.
        let mut attempt: u32 = 0;
        let primary_err = loop {
            match self.stream_one(&self.client, req, tx, cancel).await {
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
                    if emitted == 0
                        && attempt < CAPACITY_RETRIES
                        && crate::api::failover::is_transient_capacity(&e)
                        && !cancel.is_cancelled()
                    {
                        attempt += 1;
                        let wait_ms = CAPACITY_BACKOFF_BASE_MS * 2u64.pow(attempt - 1);
                        let _ = tx.send(AgentEvent::Status(format!(
                            "{} is at capacity — waiting {}s, retry {attempt}/{CAPACITY_RETRIES}",
                            self.config.provider,
                            wait_ms / 1000
                        )));
                        tokio::select! {
                            _ = cancel.cancelled() => return Err(MuseError::Interrupted),
                            _ = tokio::time::sleep(std::time::Duration::from_millis(wait_ms)) => {}
                        }
                        continue;
                    }
                    if emitted > 0
                        || !crate::api::failover::should_failover_for(&e, &self.config.provider)
                    {
                        return Err(e);
                    }
                    break e;
                }
            }
        };

        // Privacy floor: never fail over to a weaker data-privacy tier than the
        // active provider unless explicitly allowed (see `providers::Privacy`).
        let active_privacy = crate::providers::effective_privacy(
            &self.config.provider_privacy,
            &self.config.provider,
        );
        let allowed: Vec<String> = self
            .config
            .fallback_providers
            .iter()
            .filter(|id| {
                let r =
                    crate::providers::effective_privacy(&self.config.provider_privacy, id).rank();
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
                    if emitted > 0 || !crate::api::failover::should_failover_for(&e, &t.provider_id)
                    {
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
        // GitHub Models free tier caps request bodies (~8k tokens for gpt-4o).
        // Force compact prompts so the full skill catalog does not 413.
        let limited_ctx = matches!(
            self.config.provider.as_str(),
            "github-models" | "github-copilot"
        );
        let prompt_ctx = PromptContext::build_with_opts(
            &self.cwd,
            self.is_subagent,
            &self.config.model,
            &provider_label,
            self.config.poor_mode || limited_ctx,
            Some(user_text),
        );
        if prompt_ctx.has_skill_activation() {
            let label = prompt_ctx.skill_activation_label().unwrap_or("skill");
            let _ = tx.send(AgentEvent::Status(format!(
                "{label} · activated from your wording (no slash command needed)"
            )));
        }
        let mut turns = 0u32;
        let mut tool_seq: u64 = 0;
        // Compaction pressure relief. This was a single bool: one compaction per
        // user turn, latched even when the attempt *failed*. A long run therefore
        // got exactly one release valve, and one transient failure removed it for
        // the rest of the run — after which context grew until the provider
        // rejected the request outright. Track counts instead, and require the
        // context to have actually grown before compacting again so a compaction
        // that frees nothing cannot spin.
        let mut compactions: u8 = 0;
        let mut compact_failures: u8 = 0;
        let mut last_compact_input: u64 = 0;
        // Codex/ChatGPT free (and some hosts) sometimes emit only a reasoning
        // summary and zero tool calls / zero answer text. Retry once with a
        // hard nudge + tool_choice=required before giving up.
        let mut empty_tool_stalls: u8 = 0;
        let mut truncation_continuations: u8 = 0;
        let mut truncation_giving_up = false;
        let mut force_tool_choice = false;

        loop {
            if cancel.is_cancelled() {
                return Err(MuseError::Interrupted);
            }
            turns += 1;
            // max_turns == 0 → unlimited (overnight / long agent loops).
            if self.config.max_turns > 0 && turns > self.config.max_turns {
                return Err(MuseError::MaxTurns(self.config.max_turns));
            }
            if let Some(msg) = session_budget_exceeded(&self.config, usage) {
                let _ = tx.send(AgentEvent::Status(msg.clone()));
                return Err(MuseError::Budget(msg));
            }

            // Auto-compact whenever the window is under pressure — as often as a
            // long run needs, not once. `last_compact_input` is the guard against
            // spinning: a second attempt only happens once the context has grown
            // past where the previous one ran.
            let input_now = {
                let last = usage.last_usage();
                if last.input_tokens > 0 {
                    last.input_tokens
                } else {
                    last.total_tokens
                }
            };
            if compactions < MAX_AUTO_COMPACTIONS
                && compact_failures < MAX_AUTO_COMPACT_FAILURES
                && input_now > last_compact_input
                && should_auto_compact(usage, &self.config)
            {
                last_compact_input = input_now;
                let _ = tx.send(AgentEvent::Status("auto-compacting context…".into()));
                match compact_session(self, session, usage).await {
                    Ok(_) => {
                        compactions += 1;
                        let _ =
                            tx.send(AgentEvent::Status("context compacted — continuing".into()));
                    }
                    Err(e) => {
                        // Count the failure but stay eligible: a later attempt,
                        // after more growth, may well succeed.
                        compact_failures += 1;
                        let _ = tx.send(AgentEvent::Status(format!("auto-compact skipped: {e}")));
                    }
                }
            }

            // Steering: fold in any messages the user pushed mid-turn *without*
            // cancelling. Drained here (after auto-compact, before the request)
            // so injected guidance rides the very next model round with full
            // prior context instead of aborting and restarting the turn.
            let steered: Vec<String> = self
                .tools
                .steer
                .lock()
                .map(|mut q| q.drain(..).collect())
                .unwrap_or_default();
            for msg in steered {
                session.input_items.push(user_text_item(&msg));
                self.persist_session(session);
                let preview: String = msg.chars().take(80).collect();
                let ellip = if msg.chars().count() > 80 { "…" } else { "" };
                let _ = tx.send(AgentEvent::Status(format!(
                    "steered · injected mid-turn: {preview}{ellip}"
                )));
            }

            let mode_now = self.permission_mode.get();
            let instructions = prompt_ctx.render(mode_now, &self.tools.todos_snapshot().render());

            usage.set_state(format!("thinking (turn {turns})"));
            let _ = tx.send(AgentEvent::Status(format!(
                "thinking · turn {turns} · {}",
                mode_now.label()
            )));

            let tool_choice = if force_tool_choice {
                // Reset after one attempt so later normal turns stay "auto".
                force_tool_choice = false;
                "required"
            } else {
                "auto"
            };

            // Lazy /models resolution for local placeholder (llama.cpp proof).
            // If cfg still holds `local-model`, attempt to resolve to a real id
            // from the live local server before we POST.
            let effective_model = if crate::providers::is_placeholder_local_model(&self.config.model)
            {
                let resolved = self.client.resolve_local_model(&self.config.model).await;
                if resolved != self.config.model {
                    let _ = tx.send(AgentEvent::Status(format!(
                        "local model placeholder → resolved to `{resolved}` via /models"
                    )));
                }
                resolved
            } else {
                self.config.model.clone()
            };

            let req = ResponseRequest {
                model: effective_model,
                input: Value::Array(session.input_items.clone()),
                instructions: Some(instructions),
                tools: Some(tools.clone()),
                tool_choice: Some(tool_choice.into()),
                store: Some(false),
                include: Some(vec!["reasoning.encrypted_content".into()]),
                // Effort rungs differ per provider and keep being added. Send
                // what this one actually accepts — clamped to its nearest rung,
                // or omitted entirely for thinking-budget providers, which
                // reject an unexpected `effort` string outright.
                reasoning: Some(ReasoningConfig {
                    effort: crate::providers::nearest_effort(
                        &self.config.provider,
                        &self.config.reasoning_effort,
                    ),
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

            let mut replayed = replay_output_items(&resp.output);
            let mut calls = resp.function_calls();
            // Some gateways number tool calls per *response* (`read_file_5`), so
            // an id can repeat in a later turn. A repeat makes the older
            // `function_call_output` look like this call's answer — the pairing
            // scan then skips it and the request goes out with a `function_call`
            // that has no output, which strict providers reject outright.
            // Rewrite collisions (and blank ids) before anything is appended.
            let renamed = normalize_call_ids(&session.input_items, &mut replayed, &mut calls);
            session.input_items.extend(replayed);
            if renamed > 0 {
                let _ = tx.send(AgentEvent::Status(format!(
                    "history · {renamed} duplicate tool-call id(s) renamed to keep results paired"
                )));
            }

            let text = resp.output_text();
            let unknown_items = resp
                .output
                .iter()
                .filter(|i| matches!(i, crate::api::types::OutputItem::Other))
                .count();

            // ── truncation detection (finish_reason: length) ──────────────
            // A response truncated at max_tokens must not end the run quietly
            // with a partial answer. Detect it via the mapped status field
            // (chat completions / anthropic set status="length").
            //
            // CRITICAL: only inject a continuation when there are NO tool calls.
            // When the model was truncated mid tool-call, `replay_output_items`
            // has already appended that `function_call` to history; the normal
            // path below runs `execute_calls` and appends the paired
            // `function_call_output`. Injecting a bare `[harness]` user message
            // and `continue`-ing here instead would leave that `function_call`
            // unpaired — which strict providers reject on the next request —
            // and persist the poisoned history to disk. So for the tool-call
            // case we fall through and let the real tool run; the continuation
            // nudge is reserved for text-only truncation.
            //
            // Guarded by MAX_TRUNCATION_CONTINUATIONS. Once the limit is hit the
            // guard is TERMINAL for the turn (a `truncation_giving_up` latch),
            // so a model that truncates every round cannot rearm the allowance
            // by processing one tool call and looping forever.
            let mut truncated_this_round = false;
            if resp.status.as_deref() == Some("length") {
                truncated_this_round = true;
                if truncation_giving_up || truncation_continuations >= MAX_TRUNCATION_CONTINUATIONS {
                    // Terminal: surface partial output, never continue again.
                    if !truncation_giving_up {
                        truncation_giving_up = true;
                        let _ = tx.send(AgentEvent::Status(format!(
                            "model response truncated {truncation_continuations}× at max_tokens — giving up and surfacing partial output (limit {MAX_TRUNCATION_CONTINUATIONS})"
                        )));
                    }
                    // Fall through: if there are tool calls, run them; if not,
                    // the `calls.is_empty()` branch returns the partial text.
                } else if calls.is_empty() {
                    // Text-only truncation: safe to inject a continuation nudge
                    // (no unpaired function_call in history).
                    truncation_continuations += 1;
                    let _ = tx.send(AgentEvent::Status(format!(
                        "model response truncated at max_tokens (finish_reason: length) — asking to continue… ({truncation_continuations}/{MAX_TRUNCATION_CONTINUATIONS})"
                    )));
                    let nudge = if !text.trim().is_empty() {
                        "[harness] Your previous response was cut off by the provider's max_tokens limit (finish_reason: length). The user saw a truncated, incomplete answer. Continue exactly where you left off, without repeating the preamble. Finish the answer."
                    } else {
                        "[harness] Your previous response was truncated at max_tokens (finish_reason: length) with no usable output. Retry the last step, possibly with smaller chunks, or summarize and continue."
                    };
                    session.input_items.push(crate::api::types::user_text_item(nudge));
                    self.persist_session(session);
                    continue;
                } else {
                    // Truncated mid tool-call: count it, but DO NOT inject a
                    // continuation. Fall through so `execute_calls` pairs the
                    // function_call output; the model naturally continues next
                    // round with the tool result in context.
                    truncation_continuations += 1;
                    let _ = tx.send(AgentEvent::Status(format!(
                        "model truncated mid tool-call (finish_reason: length) — running the tool, then continuing ({truncation_continuations}/{MAX_TRUNCATION_CONTINUATIONS})"
                    )));
                }
            }
            if resp.status.as_deref() == Some("content_filter") {
                let _ = tx.send(AgentEvent::Status(
                    "model stopped for content_filter — surfacing partial output and ending turn".into(),
                ));
            }
            if !truncated_this_round {
                truncation_continuations = 0;
                truncation_giving_up = false;
            }

            if text_deltas == 0 && !text.is_empty() {
                let _ = tx.send(AgentEvent::AssistantMessage(text.clone()));
            }

            if calls.is_empty() {
                // Reasoning-only / empty completion: model "planned" but never
                // answered or called tools. Common on ChatGPT free + Codex OAuth
                // with some gpt-5.* models. Retry once before surfacing a note.
                let emptyish = text.trim().is_empty();
                if emptyish && empty_tool_stalls < MAX_EMPTY_TOOL_STALLS {
                    empty_tool_stalls += 1;
                    force_tool_choice = true;
                    let note = if unknown_items > 0 {
                        format!(
                            "model returned no usable tools (and {unknown_items} unparsed output item(s)) — \
                             retrying with required tool use…"
                        )
                    } else {
                        "model returned only a planning thought (no tools, no answer) — \
                         retrying with required tool use…"
                            .into()
                    };
                    let _ = tx.send(AgentEvent::Status(note));
                    session.input_items.push(user_text_item(
                        "[harness] You ended with only internal reasoning and zero tool calls \
                         and zero user-visible text. That is not done.\n\
                         Immediately call tools to inspect the workspace (list_dir on `.`, \
                         grep, read_file on README/Cargo.toml/package.json). \
                         Do not only plan. Do not reply with an empty message.",
                    ));
                    self.persist_session(session);
                    continue;
                }

                let text = if emptyish {
                    let hint = empty_turn_hint(&self.config.provider, &self.config.model);
                    let msg = format!(
                        "I only produced a short planning thought and never called tools or \
                         wrote an answer (nothing to show).\n\n{hint}"
                    );
                    let _ = tx.send(AgentEvent::AssistantMessage(msg.clone()));
                    msg
                } else {
                    text
                };

                usage.set_state("idle");
                session.push_assistant(&text);
                self.persist_session(session);
                return Ok(text);
            }

            // Reaching here means the model produced real tool calls, so any
            // earlier empty round was a hiccup, not a pattern. Without this
            // reset the allowance was spent once per turn and never returned:
            // a stall at round 3 left the next one — twenty rounds later —
            // terminating the run and reporting it as a normal completion.
            empty_tool_stalls = 0;

            // Every `function_call` just appended must leave this turn with a
            // matching `function_call_output`, whatever happens inside — cancel,
            // a panicking tool task, a subagent error. `execute_calls` owns the
            // happy path; this guard backstops *every* way out of it, so no
            // early return can strand a call in the persisted history.
            if let Err(e) = self
                .execute_calls(&calls, &mut tool_seq, session, usage, tx, cancel)
                .await
            {
                let filled = pair_unanswered(&mut session.input_items, &calls, &abort_output(&e));
                if filled > 0 && !matches!(e, MuseError::Interrupted) {
                    let _ = tx.send(AgentEvent::Status(format!(
                        "history · {filled} tool call(s) closed out after: {e}"
                    )));
                }
                self.persist_session(session);
                return Err(e);
            }

            self.persist_session(session);
        }
    }

    /// Execute one response's tool calls **in the model's original order**
    /// (required for `call_id` pairing), appending a `function_call_output` for
    /// each. Contiguous parallel-safe reads run concurrently, results emitted in
    /// order.
    ///
    /// Callers must treat any `Err` as "pairing unknown" and close out the
    /// remaining calls — see the guard in `run_turn_events`.
    async fn execute_calls(
        &self,
        calls: &[FunctionCallRef],
        tool_seq: &mut u64,
        session: &mut Session,
        usage: &mut UsageTracker,
        tx: &mpsc::UnboundedSender<AgentEvent>,
        cancel: &CancellationToken,
    ) -> Result<()> {
        let mut idx = 0usize;
        while idx < calls.len() {
            if cancel.is_cancelled() {
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
                    *tool_seq += 1;
                    let id = *tool_seq;
                    let _ = tx.send(AgentEvent::ToolStart {
                        id,
                        name: call.name.clone(),
                        args: call.arguments.clone(),
                    });
                    let host = ToolHost {
                        todos: self.tools.todos.clone(),
                        plan: self.tools.plan.clone(),
                        steer: self.tools.steer.clone(),
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
                for (handle, (id, call_id, name)) in handles.into_iter().zip(meta.into_iter()) {
                    let joined = tokio::select! {
                        // The caller's guard fills this call, the rest of the
                        // batch, and every post-batch call.
                        // Note: other in-flight blocking tasks keep running until drop
                        _ = cancel.cancelled() => return Err(MuseError::Interrupted),
                        r = handle => r,
                    };
                    // A panicking tool must not abort the turn mid-batch —
                    // that would strand every remaining call. Report it as
                    // this call's result and keep going.
                    let (body, ok) = match joined {
                        Ok((_, _, Ok(s))) => (s, true),
                        Ok((_, _, Err(e))) => (format!("error: {e}"), false),
                        Err(e) => (format!("error: tool panicked: {e}"), false),
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
                idx = batch_end;
                continue;
            }

            // Contiguous `agent` calls fan out concurrently. Subagents are
            // whole agent turns — running them one after another wastes the
            // wall time that made the model ask for several in the first
            // place. Approval is still collected up front, one prompt at a
            // time, so the user is never raced by parallel children.
            if calls[idx].name == "agent" && !self.is_subagent {
                let mut batch_end = idx + 1;
                while batch_end < calls.len() && calls[batch_end].name == "agent" {
                    batch_end += 1;
                }
                if batch_end - idx > 1 {
                    // Any error here (including cancel) leaves part of the
                    // fan-out unanswered — the caller's guard closes it out.
                    self.run_agent_fanout(
                        &calls[idx..batch_end],
                        tool_seq,
                        session,
                        usage,
                        tx,
                        cancel,
                    )
                    .await?;
                    idx = batch_end;
                    continue;
                }
            }

            // Single sequential tool (mutating / agent / memory append)
            let call = &calls[idx];
            *tool_seq += 1;
            let id = *tool_seq;
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
                    ("user denied this tool call".into(), "denied by user".into())
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
                        Err(MuseError::Interrupted) => return Err(MuseError::Interrupted),
                        Err(e) => (format!("error: {e}"), false),
                    }
                }
            } else {
                // Pre-tool hook (optional) — blocks on non-zero exit.
                if let Err(e) =
                    self.hooks
                        .run_pre(&call.name, &call.arguments, &self.cwd, &session.id)
                {
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
                if matches!(
                    call.name.as_str(),
                    "write_file" | "edit_file" | "multi_edit"
                ) {
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
                    steer: self.tools.steer.clone(),
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
                    _ = cancel.cancelled() => return Err(MuseError::Interrupted),
                    r = exec => match r {
                        Ok(Ok(s)) => (s, true),
                        Ok(Err(e)) => (format!("error: {e}"), false),
                        Err(e) => (format!("error: tool panicked: {e}"), false),
                    },
                }
            };

            if ok && call.name == "omp" {
                if let Some(spent) = crate::tools::omp::delegated_usage(&body) {
                    usage.add_external(&spent);
                    session.usage.add(&spent);
                    let _ = tx.send(AgentEvent::Usage {
                        session: usage.session_usage().clone(),
                        last: spent,
                    });
                }
            }

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
            self.hooks
                .run_post(&call.name, &call.arguments, &self.cwd, &session.id);
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
        // Media rides a *user* item, so it can only be appended once every call
        // in this response is answered — slipping it between a call and its
        // output splits the pair and strict providers reject the history.
        flush_pending_media(&mut session.input_items, tx);
        Ok(())
    }

    /// Run a contiguous run of `agent` calls concurrently, emitting their
    /// results into `session` in the model's original order (`call_id` pairing
    /// depends on it).
    ///
    /// Approval is collected for the whole batch first, sequentially — the UI
    /// has one approval slot, and a user should decide about a fan-out before
    /// any of it starts, not while three children race to ask.
    async fn run_agent_fanout(
        &self,
        batch: &[FunctionCallRef],
        tool_seq: &mut u64,
        session: &mut Session,
        usage: &mut UsageTracker,
        tx: &mpsc::UnboundedSender<AgentEvent>,
        cancel: &CancellationToken,
    ) -> Result<()> {
        // Phase 1 — announce and gate, in order.
        let mut gated: Vec<(u64, Option<String>)> = Vec::with_capacity(batch.len());
        for call in batch {
            *tool_seq += 1;
            let id = *tool_seq;
            let _ = tx.send(AgentEvent::ToolStart {
                id,
                name: call.name.clone(),
                args: call.arguments.clone(),
            });
            let mode_at_gate = self.permission_mode.get();
            let denial = if self.check_approval(&call.name, &call.arguments, tx).await {
                None
            } else if mode_at_gate.is_read_only_enforced()
                && !is_read_only_call(&call.name, &call.arguments)
            {
                Some(
                    "blocked in plan mode — subagents may edit; switch to manual/auto (Shift+Tab)"
                        .to_string(),
                )
            } else {
                Some("user denied this tool call".to_string())
            };
            gated.push((id, denial));
        }

        // Phase 2 — fan out the approved ones, capped.
        let permits = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_SUBAGENTS));
        let approved = gated.iter().filter(|(_, d)| d.is_none()).count();
        if approved > 1 {
            let _ = tx.send(AgentEvent::Status(format!(
                "fan-out · {approved} subagents (max {MAX_CONCURRENT_SUBAGENTS} at once)"
            )));
        }
        usage.set_state("tool:agent");

        let mut handles: Vec<Option<SubagentHandle>> = Vec::with_capacity(batch.len());
        for (call, (_, denial)) in batch.iter().zip(gated.iter()) {
            if denial.is_some() {
                handles.push(None);
                continue;
            }
            let parsed = parse_agent_call(call);
            let client = self.client.clone();
            let config = self.config.clone();
            let cwd = self.cwd.clone();
            let mode = self.permission_mode.clone();
            let tx_child = tx.clone();
            let cancel_child = cancel.clone();
            let permits = permits.clone();
            handles.push(Some(tokio::spawn(async move {
                let (prompt, kind, desc, provider_override, model_override) = parsed?;
                // Held for the whole child run: this is the concurrency cap.
                let _permit = permits
                    .acquire()
                    .await
                    .map_err(|e| MuseError::Other(e.to_string()))?;
                let _ = tx_child.send(AgentEvent::Status(format!("subagent · {desc}")));
                // Cross-provider: if the call named a different provider, build a
                // client + config for it from that provider's stored credentials.
                // Missing creds → LoginRequired + hard block (no silent parent run).
                match resolve_subagent_target(
                    &client,
                    &config,
                    provider_override.as_deref(),
                    model_override.as_deref(),
                    Some(prompt.as_str()),
                    Some(desc.as_str()),
                    Some(kind.as_str()),
                    &tx_child,
                ) {
                    SubagentTarget::Ready {
                        client: child_client,
                        config: child_config,
                    } => {
                        subagent::run_subagent(
                            child_client,
                            child_config,
                            cwd,
                            mode,
                            &prompt,
                            &kind,
                            &cancel_child,
                            &tx_child,
                        )
                        .await
                    }
                    SubagentTarget::AwaitingLogin { message, .. } => {
                        Err(MuseError::Other(message))
                    }
                }
            })));
        }

        // Phase 3 — collect in submission order so `call_id` pairing holds.
        for (call, ((id, denial), handle)) in batch.iter().zip(gated.into_iter().zip(handles)) {
            let (body, ok) = match (denial, handle) {
                (Some(msg), _) => {
                    let _ = tx.send(AgentEvent::ToolEnd {
                        id,
                        name: call.name.clone(),
                        result: msg.clone(),
                        ok: false,
                    });
                    session
                        .input_items
                        .push(function_call_output_item(&call.call_id, &msg));
                    continue;
                }
                (None, Some(handle)) => {
                    let joined = tokio::select! {
                        _ = cancel.cancelled() => return Err(MuseError::Interrupted),
                        r = handle => r,
                    };
                    match joined {
                        Ok(Ok((text, spent))) => {
                            usage.add_external(&spent);
                            session.usage.add(&spent);
                            let _ = tx.send(AgentEvent::Usage {
                                session: usage.session_usage().clone(),
                                last: spent,
                            });
                            (text, true)
                        }
                        Ok(Err(MuseError::Interrupted)) => return Err(MuseError::Interrupted),
                        Ok(Err(e)) => (format!("error: {e}"), false),
                        Err(e) => (format!("error: subagent task failed: {e}"), false),
                    }
                }
                (None, None) => ("error: subagent was never started".to_string(), false),
            };
            let body = spill::maybe_spill(
                &session.id,
                &call.name,
                body,
                self.config.tool_result_max_chars as usize,
            );
            receipt::record(
                &session.id,
                receipt::Event::Tool {
                    name: call.name.clone(),
                    args_sha256: None,
                    result_sha256: receipt::sha256_hex(body.as_bytes()),
                    ok,
                },
            );
            let _ = tx.send(AgentEvent::ToolEnd {
                id,
                name: call.name.clone(),
                result: body.clone(),
                ok,
            });
            session
                .input_items
                .push(function_call_output_item(&call.call_id, &body));
        }
        // Media is flushed by `execute_calls` once *all* calls are answered —
        // a user item here would land between a later call and its output.
        Ok(())
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
            assert_eq!(
                n, 1,
                "call {} has {n} outputs, expected exactly 1",
                c.call_id
            );
        }
    }

    /// The guard `run_turn_events` runs on every non-Ok exit from `execute_calls`.
    fn close_out(items: &mut Vec<Value>, calls: &[FunctionCallRef], err: &MuseError) -> usize {
        pair_unanswered(items, calls, &abort_output(err))
    }

    #[test]
    fn cancel_before_any_tool_pairs_every_call() {
        let calls = vec![call("a", "read_file"), call("b", "bash"), call("c", "grep")];
        let mut items: Vec<Value> = Vec::new();
        assert_eq!(pair_unanswered(&mut items, &calls, INTERRUPT_OUTPUT), 3);
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
        // only c and d
        assert_eq!(
            close_out(&mut items, &calls, &MuseError::Interrupted),
            2,
            "cancel mid-batch must close out the unanswered calls"
        );
        assert_fully_paired(&items, &calls);
        // Answered calls keep their real results — not overwritten by the interrupt.
        let a = items
            .iter()
            .find(|v| v.get("call_id").and_then(|i| i.as_str()) == Some("a"))
            .unwrap();
        assert_eq!(a.get("output").and_then(|o| o.as_str()), Some("contents"));
        let c = items
            .iter()
            .find(|v| v.get("call_id").and_then(|i| i.as_str()) == Some("c"))
            .unwrap();
        assert_eq!(
            c.get("output").and_then(|o| o.as_str()),
            Some(INTERRUPT_OUTPUT)
        );
    }

    #[test]
    fn errored_tool_run_still_leaves_history_paired() {
        // A tool task panicked (JoinError) after the first call answered: the
        // turn bails with a non-Interrupted error and the rest must still close.
        let calls = vec![call("a", "read_file"), call("b", "bash"), call("c", "grep")];
        let mut items = vec![
            serde_json::json!({"type":"function_call","call_id":"a","name":"read_file","arguments":"{}"}),
            serde_json::json!({"type":"function_call","call_id":"b","name":"bash","arguments":"{}"}),
            serde_json::json!({"type":"function_call","call_id":"c","name":"grep","arguments":"{}"}),
            function_call_output_item("a", "contents"),
        ];
        let err = MuseError::Other("tool task panicked".into());
        assert_eq!(close_out(&mut items, &calls, &err), 2);
        assert_fully_paired(&items, &calls);
        let b = items
            .iter()
            .find(|v| {
                v.get("type").and_then(|t| t.as_str()) == Some("function_call_output")
                    && v.get("call_id").and_then(|i| i.as_str()) == Some("b")
            })
            .unwrap();
        assert!(
            b.get("output")
                .and_then(|o| o.as_str())
                .unwrap_or_default()
                .contains("panicked"),
            "the synthetic output should say why the call never ran"
        );
    }

    #[test]
    fn denied_and_errored_calls_in_one_batch_stay_paired() {
        // Mixed batch: one real result, one permission denial, one never run.
        let calls = vec![
            call("a", "read_file"),
            call("b", "write_file"),
            call("c", "bash"),
        ];
        let mut items = vec![
            function_call_output_item("a", "contents"),
            function_call_output_item("b", "user denied this tool call"),
        ];
        assert_eq!(close_out(&mut items, &calls, &MuseError::Interrupted), 1);
        assert_fully_paired(&items, &calls);
    }

    #[test]
    fn pairing_is_idempotent() {
        let calls = vec![call("a", "bash")];
        let mut items: Vec<Value> = Vec::new();
        pair_unanswered(&mut items, &calls, INTERRUPT_OUTPUT);
        assert_eq!(
            pair_unanswered(&mut items, &calls, INTERRUPT_OUTPUT),
            0,
            "must not duplicate"
        );
        assert_fully_paired(&items, &calls);
    }

    fn fc_item(id: &str, name: &str) -> Value {
        serde_json::json!({
            "type": "function_call", "call_id": id, "name": name, "arguments": "{}"
        })
    }

    #[test]
    fn call_id_reused_from_an_earlier_turn_is_renamed() {
        // Gateways that number calls per response (`read_file_5`) repeat ids
        // across turns; the old output would otherwise "answer" the new call.
        let history = vec![
            fc_item("read_file_5", "read_file"),
            function_call_output_item("read_file_5", "old contents"),
        ];
        let mut replayed = vec![fc_item("read_file_5", "read_file")];
        let mut calls = vec![call("read_file_5", "read_file")];
        assert_eq!(normalize_call_ids(&history, &mut replayed, &mut calls), 1);
        assert_ne!(calls[0].call_id, "read_file_5");
        assert_eq!(
            replayed[0].get("call_id").and_then(|c| c.as_str()),
            Some(calls[0].call_id.as_str()),
            "the replayed item and the call must agree on the new id"
        );

        // With the rename, the stale output no longer counts as an answer.
        let mut items = history;
        items.extend(replayed);
        assert_eq!(pair_unanswered(&mut items, &calls, INTERRUPT_OUTPUT), 1);
        assert_fully_paired(&items, &calls);
    }

    #[test]
    fn duplicate_and_blank_ids_within_one_response_are_made_unique() {
        let mut replayed = vec![
            fc_item("dup", "read_file"),
            serde_json::json!({"type":"reasoning","summary":[]}),
            fc_item("dup", "grep"),
            fc_item("", "glob"),
        ];
        let mut calls = vec![
            call("dup", "read_file"),
            call("dup", "grep"),
            call("", "glob"),
        ];
        assert_eq!(normalize_call_ids(&[], &mut replayed, &mut calls), 2);
        let ids: Vec<&str> = calls.iter().map(|c| c.call_id.as_str()).collect();
        assert_eq!(ids[0], "dup");
        assert_ne!(ids[1], "dup");
        assert!(!ids[2].is_empty());
        let unique: std::collections::HashSet<&&str> = ids.iter().collect();
        assert_eq!(unique.len(), 3, "every call needs its own id: {ids:?}");
        // Items were rewritten in lockstep (skipping the reasoning item).
        assert_eq!(
            replayed[2].get("call_id").and_then(|c| c.as_str()),
            Some(ids[1])
        );
        assert_eq!(
            replayed[3].get("call_id").and_then(|c| c.as_str()),
            Some(ids[2])
        );
    }

    #[test]
    fn unique_call_ids_are_left_alone() {
        let history = vec![
            fc_item("c1", "read_file"),
            function_call_output_item("c1", "x"),
        ];
        let mut replayed = vec![fc_item("c2", "grep"), fc_item("c3", "bash")];
        let mut calls = vec![call("c2", "grep"), call("c3", "bash")];
        assert_eq!(normalize_call_ids(&history, &mut replayed, &mut calls), 0);
        assert_eq!(calls[0].call_id, "c2");
        assert_eq!(calls[1].call_id, "c3");
    }

    fn agent_call(id: &str, prompt: &str, kind: &str) -> FunctionCallRef {
        FunctionCallRef {
            call_id: id.into(),
            name: "agent".into(),
            arguments: serde_json::json!({"prompt": prompt, "subagent_type": kind}).to_string(),
        }
    }

    #[test]
    fn agent_calls_parse_into_prompt_kind_and_label() {
        let (prompt, kind, desc, _prov, _model) =
            parse_agent_call(&FunctionCallRef {
                call_id: "a".into(),
                name: "agent".into(),
                arguments:
                    r#"{"prompt":"map auth","subagent_type":"general","description":"auth map"}"#
                        .into(),
            })
            .expect("valid call");
        assert_eq!(
            (prompt.as_str(), kind.as_str(), desc.as_str()),
            ("map auth", "general", "auth map")
        );

        // Defaults: explore, and the label falls back to the kind.
        let (_, kind, desc, _prov, _model) = parse_agent_call(&agent_call("b", "look around", "explore")).unwrap();
        assert_eq!((kind.as_str(), desc.as_str()), ("explore", "explore"));

        // A missing prompt is a tool error, not a spawned no-op subagent.
        assert!(parse_agent_call(&call("c", "agent")).is_err());
    }

    /// Cross-provider: the agent call parses optional provider/model overrides,
    /// and natural-language provider names resolve to the right catalog entry.
    #[test]
    fn agent_call_parses_cross_provider_overrides() {
        let (_, _, _, prov, model) = parse_agent_call(&FunctionCallRef {
            call_id: "a".into(),
            name: "agent".into(),
            arguments: r#"{"prompt":"audit auth","provider":"grok","model":"grok-4"}"#.into(),
        })
        .unwrap();
        // Alias "grok" is preserved as the raw field value; resolve happens later.
        assert_eq!(prov.as_deref(), Some("grok"));
        assert_eq!(model.as_deref(), Some("grok-4"));

        // Omitted overrides are None (inherit parent).
        let (_, _, _, prov2, model2) =
            parse_agent_call(&agent_call("b", "look", "explore")).unwrap();
        assert!(prov2.is_none() && model2.is_none());
    }

    /// Models often forget the structured `provider` field but put the target
    /// in the description or prompt. Recovery must still route correctly.
    #[test]
    fn agent_call_infers_provider_from_description_and_prompt() {
        // Description is a short NL label naming the provider.
        let (_, _, _, prov, _) = parse_agent_call(&FunctionCallRef {
            call_id: "a".into(),
            name: "agent".into(),
            arguments: r#"{"prompt":"review the auth module","description":"claude review","subagent_type":"general"}"#.into(),
        })
        .unwrap();
        assert_eq!(prov.as_deref(), Some("anthropic"));

        // Routing phrase in the prompt.
        let (_, _, _, prov2, _) = parse_agent_call(&FunctionCallRef {
            call_id: "b".into(),
            name: "agent".into(),
            arguments: r#"{"prompt":"Deploy this on grok and check the failover path.","description":"failover check"}"#.into(),
        })
        .unwrap();
        assert_eq!(prov2.as_deref(), Some("xai"));

        // Explicit provider:value form.
        let (_, _, _, prov3, model3) = parse_agent_call(&FunctionCallRef {
            call_id: "c".into(),
            name: "agent".into(),
            arguments: r#"{"prompt":"task\nprovider:antigravity model:gemini-2.5-flash","description":"agy"}"#.into(),
        })
        .unwrap();
        assert_eq!(prov3.as_deref(), Some("antigravity"));
        assert_eq!(model3.as_deref(), Some("gemini-2.5-flash"));

        // Bare task text that merely mentions a product must NOT hijack routing.
        let (_, _, _, prov4, _) = parse_agent_call(&FunctionCallRef {
            call_id: "d".into(),
            name: "agent".into(),
            arguments: r#"{"prompt":"Document how Claude Code stores sessions on disk.","description":"session docs"}"#.into(),
        })
        .unwrap();
        assert!(
            prov4.is_none(),
            "incidental product mention must not force a provider: {prov4:?}"
        );

        // "via antigravity" / "using gemini" routing cues.
        let (_, _, _, prov5, _) = parse_agent_call(&FunctionCallRef {
            call_id: "e".into(),
            name: "agent".into(),
            arguments: r#"{"prompt":"Ship the patch via antigravity","description":"ship"}"#.into(),
        })
        .unwrap();
        assert_eq!(prov5.as_deref(), Some("antigravity"));

        let (_, _, _, prov6, _) = parse_agent_call(&FunctionCallRef {
            call_id: "f".into(),
            name: "agent".into(),
            arguments: r#"{"prompt":"Research using gemini","description":"research"}"#.into(),
        })
        .unwrap();
        assert_eq!(prov6.as_deref(), Some("google"));

        // Structured provider wins over incidental description text.
        let (_, _, _, prov7, _) = parse_agent_call(&FunctionCallRef {
            call_id: "g".into(),
            name: "agent".into(),
            arguments: r#"{"prompt":"compare notes","description":"claude vs grok","provider":"xai"}"#.into(),
        })
        .unwrap();
        assert_eq!(
            prov7.as_deref(),
            Some("xai"),
            "explicit provider field must not be overwritten by description inference"
        );
    }

    /// A task prompt that merely *starts* with a product name is describing its
    /// subject, not requesting a backend. Reading it as routing blocks the spawn
    /// behind a /login modal the user never asked for.
    #[test]
    fn a_provider_name_leading_the_prompt_is_a_subject_not_a_route() {
        for prompt in [
            "Claude Code session import path — map how it works",
            "Gemini response parsing has a bug, find it",
            "GPT-style tool schemas: audit our converter",
        ] {
            let (_, _, _, prov, _) = parse_agent_call(&FunctionCallRef {
                call_id: "a".into(),
                name: "agent".into(),
                arguments: serde_json::json!({ "prompt": prompt, "description": "audit" })
                    .to_string(),
            })
            .unwrap();
            assert!(
                prov.is_none(),
                "prompt subject must not route: {prompt:?} → {prov:?}"
            );
        }

        // The same leading name in the *description* is still routing — that
        // field is a label the model wrote about the spawn itself.
        let (_, _, _, prov, _) = parse_agent_call(&FunctionCallRef {
            call_id: "b".into(),
            name: "agent".into(),
            arguments: r#"{"prompt":"audit the failover path","description":"grok audit"}"#.into(),
        })
        .unwrap();
        assert_eq!(prov.as_deref(), Some("xai"));

        // And an explicit cue inside the prompt still routes.
        let (_, _, _, prov, _) = parse_agent_call(&FunctionCallRef {
            call_id: "c".into(),
            name: "agent".into(),
            arguments: r#"{"prompt":"Claude subagent: review auth for races","description":"review"}"#.into(),
        })
        .unwrap();
        assert_eq!(
            prov.as_deref(),
            Some("anthropic"),
            "'<provider> subagent' at the head of a prompt is an explicit route"
        );
    }

    /// "gemini" resolves to catalog id `google` (see `natural_language_provider_names_resolve`),
    /// a *different* id from an active `antigravity` session — but both share the
    /// same Google OAuth login. A subagent saying "gemini" while the parent is
    /// already on `antigravity` (or vice versa) must be treated as the provider
    /// the user is *already using*: reuse the parent client verbatim, never
    /// rebuild one or touch credential resolution. Regression test for the hang
    /// where this fell through to the cross-provider branch instead.
    #[test]
    fn subagent_target_short_circuits_within_the_google_family() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let parent_client = ApiClient::new("https://example.invalid", "parent-key").unwrap();
        let mut parent_config = Config::default();
        parent_config.provider = "antigravity".into();
        parent_config.model = "gemini-2.5-pro".into();

        let target = resolve_subagent_target(
            &parent_client,
            &parent_config,
            Some("gemini"),
            None,
            None,
            None,
            None,
            &tx,
        );
        match target {
            SubagentTarget::Ready { config, .. } => {
                assert_eq!(
                    config.provider, "antigravity",
                    "same-family request must keep the parent's actual provider id, \
                     not rebuild against the unrelated `google` catalog entry"
                );
                assert_eq!(
                    config.model, "gemini-2.5-pro",
                    "no explicit model override was given, so it must inherit the parent's"
                );
            }
            SubagentTarget::AwaitingLogin { .. } => {
                panic!("same-provider-family request must never require login");
            }
        }

        // The reverse direction (parent already on `google`, request "antigravity")
        // must also short-circuit.
        let mut parent_config_google = Config::default();
        parent_config_google.provider = "google".into();
        let target2 = resolve_subagent_target(
            &parent_client,
            &parent_config_google,
            Some("antigravity"),
            None,
            None,
            None,
            None,
            &tx,
        );
        match target2 {
            SubagentTarget::Ready { config, .. } => {
                assert_eq!(config.provider, "google");
            }
            SubagentTarget::AwaitingLogin { .. } => {
                panic!("same-provider-family request must never require login");
            }
        }
    }

    #[test]
    fn natural_language_provider_names_resolve() {
        assert_eq!(resolve_provider_alias("grok").map(|p| p.id), Some("xai"));
        assert_eq!(resolve_provider_alias("gemini").map(|p| p.id), Some("google"));
        assert_eq!(resolve_provider_alias("claude").map(|p| p.id), Some("anthropic"));
        assert_eq!(resolve_provider_alias("chatgpt").map(|p| p.id), Some("openai"));
        assert_eq!(resolve_provider_alias("deepseek").map(|p| p.id), Some("deepseek"));
        // Direct id passes through.
        assert_eq!(resolve_provider_alias("anthropic").map(|p| p.id), Some("anthropic"));
        // Antigravity is its OWN provider — must NOT collapse to google.
        assert_eq!(
            resolve_provider_alias("antigravity").map(|p| p.id),
            Some("antigravity")
        );
        assert_eq!(
            resolve_provider_alias("google antigravity").map(|p| p.id),
            Some("antigravity")
        );
        // Filler words are stripped, so full NL phrases still resolve.
        assert_eq!(
            resolve_provider_alias("antigravity subagent").map(|p| p.id),
            Some("antigravity")
        );
        assert_eq!(resolve_provider_alias("use grok").map(|p| p.id), Some("xai"));
        assert_eq!(
            resolve_provider_alias("the gemini provider").map(|p| p.id),
            Some("google")
        );
        // Model-family nicknames route to the serving provider.
        assert_eq!(resolve_provider_alias("sonnet").map(|p| p.id), Some("anthropic"));
        assert_eq!(resolve_provider_alias("opus").map(|p| p.id), Some("anthropic"));
        assert_eq!(resolve_provider_alias("flash").map(|p| p.id), Some("google"));
        assert_eq!(resolve_provider_alias("gpt").map(|p| p.id), Some("openai"));
        // Distinct kimi vs moonshot catalog ids.
        assert_eq!(resolve_provider_alias("kimi").map(|p| p.id), Some("kimi"));
        assert_eq!(resolve_provider_alias("moonshot").map(|p| p.id), Some("moonshot"));
        // "meta" must not over-match every display name via naive substring.
        assert_eq!(resolve_provider_alias("meta").map(|p| p.id), Some("meta"));
        // Unknown name → None (caller falls back to parent).
        assert!(resolve_provider_alias("nonesuch-xyz").is_none());
    }

    /// The fan-out path must only ever claim a run of `agent` calls — grouping
    /// anything else would run a mutating tool concurrently and out of order.
    #[test]
    fn only_contiguous_agent_calls_form_a_fanout_batch() {
        let calls = [
            agent_call("a", "one", "explore"),
            agent_call("b", "two", "explore"),
            call("c", "write_file"),
            agent_call("d", "three", "explore"),
        ];
        let mut idx = 0usize;
        let mut batch_end = idx + 1;
        while batch_end < calls.len() && calls[batch_end].name == "agent" {
            batch_end += 1;
        }
        assert_eq!(batch_end - idx, 2, "the batch stops at the write");

        // A lone trailing agent call is not a fan-out — it takes the plain path.
        idx = 3;
        batch_end = idx + 1;
        while batch_end < calls.len() && calls[batch_end].name == "agent" {
            batch_end += 1;
        }
        assert_eq!(batch_end - idx, 1);
    }

    #[test]
    fn the_concurrency_cap_is_a_real_bound() {
        assert!(
            (1..=8).contains(&MAX_CONCURRENT_SUBAGENTS),
            "cap must throttle fan-out without serialising it"
        );
    }

    /// Executable spec for the fan-out shape in `run_agent_fanout`: the permit
    /// is acquired *inside* the spawned task and held across the whole run, and
    /// results are collected in submission order regardless of finish order.
    ///
    /// The classic ways to get this wrong — acquiring before spawn (serialises
    /// everything) or dropping the permit early (no bound at all) — both fail
    /// this test.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fanout_respects_the_cap_and_preserves_order() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        const CAP: usize = 3;
        const JOBS: usize = 9;
        let permits = Arc::new(tokio::sync::Semaphore::new(CAP));
        let in_flight = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for i in 0..JOBS {
            let permits = permits.clone();
            let in_flight = in_flight.clone();
            let peak = peak.clone();
            handles.push(tokio::spawn(async move {
                let _permit = permits.acquire().await.unwrap();
                let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                // Later jobs finish sooner, so ordering cannot come for free.
                tokio::time::sleep(std::time::Duration::from_millis((JOBS - i) as u64 * 8)).await;
                in_flight.fetch_sub(1, Ordering::SeqCst);
                i
            }));
        }

        let mut collected = Vec::new();
        for handle in handles {
            collected.push(handle.await.unwrap());
        }

        assert_eq!(
            collected,
            (0..JOBS).collect::<Vec<_>>(),
            "results must arrive in submission order — call_id pairing depends on it"
        );
        assert!(
            peak.load(Ordering::SeqCst) <= CAP,
            "peak concurrency {} exceeded the cap {CAP}",
            peak.load(Ordering::SeqCst)
        );
        assert!(
            peak.load(Ordering::SeqCst) > 1,
            "the batch must actually run concurrently, not one at a time"
        );
    }

    /// Concurrent children must not race the parent's single approval slot.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_child_approvals_are_serialised() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let (parent_tx, mut parent_rx) = mpsc::unbounded_channel();
        let concurrent = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));

        // Parent side: answer prompts one at a time, as the TUI does.
        let seen = Arc::new(AtomicUsize::new(0));
        let seen_bg = seen.clone();
        let parent = tokio::spawn(async move {
            while let Some(ev) = parent_rx.recv().await {
                if let AgentEvent::ApprovalRequest { respond, .. } = ev {
                    seen_bg.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    let _ = respond.send(ApprovalDecision::Approve);
                }
            }
        });

        let mut children = Vec::new();
        for _ in 0..4 {
            let tx = parent_tx.clone();
            let concurrent = concurrent.clone();
            let peak = peak.clone();
            children.push(tokio::spawn(async move {
                let (child_tx, child_rx) = tokio::sync::oneshot::channel();
                let ask = tokio::spawn(async move {
                    super::subagent::relay_approval_for_test(
                        &tx,
                        "bash".into(),
                        "{}".into(),
                        child_tx,
                    )
                    .await;
                });
                let decision = child_rx.await;
                let n = concurrent.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(n, Ordering::SeqCst);
                concurrent.fetch_sub(1, Ordering::SeqCst);
                ask.await.unwrap();
                decision
            }));
        }
        for c in children {
            assert_eq!(c.await.unwrap().unwrap(), ApprovalDecision::Approve);
        }
        drop(parent_tx);
        let _ = parent.await;

        assert_eq!(
            seen.load(Ordering::SeqCst),
            4,
            "every child must get an answer"
        );
    }

    /// Parallel batches skip the approval gate, so anything parallel-safe MUST
    /// be read-only — otherwise a write could run without asking.
    #[test]
    fn parallel_safe_implies_approval_free() {
        for name in [
            "read_file",
            "list_dir",
            "grep",
            "glob",
            "web_fetch",
            "web_search",
            "look",
            "extract_frames",
            "git_status",
            "git_diff",
            "skill",
            "write_file",
            "edit_file",
            "multi_edit",
            "apply_patch",
            "bash",
            "agent",
            "memory",
            "todo_write",
            "submit_plan",
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
            assert!(
                !is_parallel_safe(name, "{}"),
                "{name} must run sequentially"
            );
            assert!(!is_read_only_call(name, "{}"), "{name} must need approval");
        }
        assert!(is_read_only_call("look", r#"{"path":"x.png"}"#));
        assert!(is_parallel_safe("look", r#"{"path":"x.png"}"#));
    }

    #[test]
    fn memory_read_is_free_but_append_needs_approval() {
        assert!(is_read_only_call("memory", r#"{"action":"read"}"#));
        assert!(!is_read_only_call(
            "memory",
            r#"{"action":"append","text":"x"}"#
        ));
        assert!(
            !is_read_only_call("memory", "{}"),
            "unspecified action must not be free"
        );
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
        assert!(is_read_only_call(
            "plur",
            r#"{"action":"recall","query":"x"}"#
        ));
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
        assert!(!is_read_only_call(
            "omp",
            r#"{"action":"run","prompt":"x"}"#
        ));
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
        for free in [
            "tabs", "scan", "snapshot", "tabtree", "status", "console", "network",
        ] {
            let a = format!(r#"{{"action":"{free}"}}"#);
            assert!(is_read_only_call("browser", &a), "{free} should be free");
        }
        for gated in [
            "open",
            "click",
            "fill",
            "send_keys",
            "exec",
            "close",
            "screenshot",
        ] {
            let a = format!(r#"{{"action":"{gated}"}}"#);
            assert!(
                !is_read_only_call("browser", &a),
                "{gated} must need approval"
            );
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
/// with `output` (an interrupt or error note).
///
/// Invariant: providers reject a request in which a `function_call` has no
/// matching `function_call_output` — Anthropic hardest ("`tool_use` ids were
/// found without `tool_result` blocks") — so an aborted turn must never leave a
/// gap, including mid-parallel-batch, where some calls have already answered.
/// Idempotent and order-independent: safe to call at any bail-out site with the
/// full call list. Returns how many were filled.
pub(crate) fn pair_unanswered(
    items: &mut Vec<Value>,
    calls: &[FunctionCallRef],
    output: &str,
) -> usize {
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
        items.push(function_call_output_item(&call_id, output));
    }
    n
}

/// Synthetic result recorded for calls that never ran because the turn aborted.
fn abort_output(err: &MuseError) -> String {
    match err {
        MuseError::Interrupted => INTERRUPT_OUTPUT.to_string(),
        e => format!("[error: {e}]"),
    }
}

/// Make every `function_call` id in `replayed` unique — across `history` and
/// within the response itself — rewriting `calls` in lockstep. Returns how many
/// ids were replaced.
///
/// `replayed` and `calls` come from the same response in the same order, so the
/// n-th `function_call` item describes the n-th call. Blank ids (providers that
/// omit `call_id`) and ids that collide with something already in history both
/// break pairing: the *older* output answers the *newer* call, leaving a
/// `function_call` with nothing after it. Rewriting is safe because the id only
/// ever has to match inside the history we send back.
fn normalize_call_ids(
    history: &[Value],
    replayed: &mut [Value],
    calls: &mut [FunctionCallRef],
) -> usize {
    let mut used: HashSet<String> = history
        .iter()
        .filter(|v| {
            matches!(
                v.get("type").and_then(|t| t.as_str()),
                Some("function_call") | Some("function_call_output")
            )
        })
        .filter_map(|v| v.get("call_id").and_then(|c| c.as_str()))
        .map(str::to_string)
        .collect();

    let mut renamed = 0usize;
    let mut calls = calls.iter_mut();
    for item in replayed.iter_mut() {
        if item.get("type").and_then(|t| t.as_str()) != Some("function_call") {
            continue;
        }
        let Some(call) = calls.next() else { break };
        let id = item
            .get("call_id")
            .and_then(|c| c.as_str())
            .unwrap_or_default()
            .to_string();
        if !id.is_empty() && used.insert(id) {
            continue; // fresh and unique — the normal case
        }
        let base = if call.call_id.is_empty() {
            format!("call_{}", call.name)
        } else {
            call.call_id.clone()
        };
        let mut n = 2usize;
        let mut fresh = format!("{base}-{n}");
        while !used.insert(fresh.clone()) {
            n += 1;
            fresh = format!("{base}-{n}");
        }
        if let Some(obj) = item.as_object_mut() {
            obj.insert("call_id".into(), Value::String(fresh.clone()));
        }
        call.call_id = fresh;
        renamed += 1;
    }
    renamed
}

/// Returns a human-readable reason when the session has hit a configured
/// cost or token ceiling (checked before each API call).
pub fn session_budget_exceeded(cfg: &Config, usage: &UsageTracker) -> Option<String> {
    let u = usage.session_usage();
    if let Some(max) = cfg.max_session_cost_usd {
        let cost = u.estimated_cost_usd();
        if cost >= max {
            return Some(format!(
                "session cost ${cost:.4} ≥ budget ${max:.4} — raise with /budget cost <n> (or 0/off) · /budget clear"
            ));
        }
    }
    if let Some(max) = cfg.max_session_tokens {
        if u.total_tokens >= max {
            return Some(format!(
                "session tokens {} ≥ budget {} — raise with /budget tokens <n> (or 0/off) · /budget clear",
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
    let c = format!(
        " {} ",
        command.to_ascii_lowercase().replace(['\t', '\n'], " ")
    );
    // Git working-tree / index / publish mutations (fetch is read-only, allowed).
    const GIT_MUT: &[&str] = &[
        "git commit",
        "git push",
        "git add",
        "git reset",
        "git checkout",
        "git restore",
        "git stash",
        "git merge",
        "git rebase",
        "git cherry-pick",
        "git revert",
        "git rm",
        "git mv",
        "git clean",
        "git apply",
        "git tag ",
        "git pull",
        "git switch",
    ];
    if GIT_MUT.iter().any(|p| c.contains(p)) {
        return Some("git repo/VCS mutation is blocked in plan mode — Shift+Tab to manual/auto to commit or change tracked files");
    }
    // PR / release publishing via gh.
    const GH_MUT: &[&str] = &[
        "gh pr create",
        "gh pr merge",
        "gh pr close",
        "gh pr edit",
        "gh pr ready",
        "gh pr comment",
        "gh pr reopen",
        "gh release create",
        "gh release edit",
        "gh release delete",
        "gh repo create",
        "gh repo delete",
        "gh repo edit",
        "gh issue create",
        "gh issue edit",
        "gh issue close",
    ];
    if GH_MUT.iter().any(|p| c.contains(p)) {
        return Some("publishing (gh) is blocked in plan mode");
    }
    // Dependency installs mutate lockfiles / the environment.
    const DEP_MUT: &[&str] = &[
        "npm install",
        "npm i ",
        "npm ci",
        "npm add",
        "npm uninstall",
        "npm remove",
        "pnpm add",
        "pnpm install",
        "pnpm remove",
        "yarn add",
        "yarn install",
        "yarn remove",
        "bun add",
        "bun install",
        "pip install",
        "pip uninstall",
        "pip3 install",
        "pip3 uninstall",
        "poetry add",
        "poetry install",
        "poetry remove",
        "cargo add",
        "cargo install",
        "cargo remove",
        "cargo publish",
        "cargo update",
        "gem install",
        "bundle install",
        "bundle update",
        "go get ",
        "go install",
        "apt install",
        "apt-get install",
        "brew install",
        "dnf install",
        "yum install",
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
    let _ = tx.send(AgentEvent::Status(format!(
        "vision · {n} attachment(s) ready"
    )));
}

fn multimodal_user_item(text: &str, media: &[MediaAttach]) -> Value {
    let parts: Vec<(&str, &str, &str)> = media
        .iter()
        .map(|m| (m.kind.api_type(), m.kind.url_field(), m.data_url.as_str()))
        .collect();
    user_multimodal_item(text, &parts)
}

/// User-facing hint when the model ends a turn with no tools and no text.
fn empty_turn_hint(provider: &str, model: &str) -> String {
    let openai_oauth = provider == "openai"
        || std::env::var("NUR_PROVIDER")
            .or_else(|_| std::env::var("META_PROVIDER"))
            .map(|p| p.eq_ignore_ascii_case("openai"))
            .unwrap_or(false);
    // ChatGPT free OAuth often returns reasoning-only on Codex backend.
    if openai_oauth || model.contains("sol") || model.starts_with("gpt-5") {
        return format!(
            "Likely causes:\n\
             • **ChatGPT OAuth / free plan** on the Codex backend — some models emit only a \
               reasoning summary and skip tool calls. Paid ChatGPT / an **OpenAI API key** \
               (`/login` → OpenAI key) is more reliable for agent tools.\n\
             • Model `{model}` may not be fully tool-capable on this endpoint — try \
               `/model` and pick another, or switch provider (`/login`).\n\
             • Retry the same prompt once; nur already auto-retried with required tool use."
        );
    }
    format!(
        "The model (`{model}` via `{provider}`) returned no tools and no answer after a \
         forced retry. Try `/model`, another provider via `/login`, or rephrase the request."
    )
}

/// Empty (reasoning-only, no tools, no text) rounds retried before giving up.
/// Reset after any round that produces real tool calls.
const MAX_EMPTY_TOOL_STALLS: u8 = 3;

/// Consecutive truncation continuations before we surface partial output and stop.
/// Guards against an infinite loop if the provider keeps returning finish_reason=length.
const MAX_TRUNCATION_CONTINUATIONS: u8 = 5;

/// Ceiling on automatic compactions inside one user turn. High enough that a
/// long agent run never runs out of relief, low enough to bound the cost.
const MAX_AUTO_COMPACTIONS: u8 = 8;
/// Consecutive-ish compaction failures tolerated before giving up on the turn.
const MAX_AUTO_COMPACT_FAILURES: u8 = 3;

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

/// A spawned subagent run: its report text plus the tokens it spent.
type SubagentHandle = tokio::task::JoinHandle<Result<(String, TokenUsage)>>;

/// How many times to re-offer a turn to a provider that reported saturation
/// before giving up on it and falling over. Three attempts spans ~7s of
/// backoff, which clears a per-worker queue without stalling a real outage.
const CAPACITY_RETRIES: u32 = 3;
/// First capacity backoff; doubles per attempt (1s → 2s → 4s).
const CAPACITY_BACKOFF_BASE_MS: u64 = 1000;

/// Most subagents to keep in flight at once.
///
/// Each one is a full agent turn against the same provider, so this is a
/// rate-limit and context-budget guard as much as a CPU one. The rest of the
/// batch queues behind the semaphore and starts as slots free up.
const MAX_CONCURRENT_SUBAGENTS: usize = 4;

/// `{prompt, subagent_type, description, provider?, model?}` out of an `agent`
/// tool call. Provider/model are optional cross-provider overrides. When the
/// model forgets `provider` but names one in the description/prompt (common),
/// we recover it via [`infer_provider_from_agent_text`].
fn parse_agent_call(call: &FunctionCallRef) -> Result<(String, String, String, Option<String>, Option<String>)> {
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
        .unwrap_or("explore")
        .to_string();
    let desc = v
        .get("description")
        .and_then(|x| x.as_str())
        .unwrap_or(&kind)
        .to_string();
    let mut provider = v
        .get("provider")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let mut model = v
        .get("model")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    // Robust NL recovery: models often put "on claude" / "grok review" in the
    // description or prompt and omit the structured `provider` field.
    if provider.is_none() {
        if let Some((pid, maybe_model)) = infer_provider_from_agent_text(&desc, &prompt) {
            provider = Some(pid);
            if model.is_none() {
                model = maybe_model;
            }
        }
    }
    Ok((prompt, kind, desc, provider, model))
}

/// Infer a target provider (and optional model id) from free-text the model
/// wrote into `description` / `prompt` when it forgot the structured fields.
///
/// Prefers explicit routing phrases (`on claude`, `using grok`, `provider:xai`,
/// `deploy … antigravity`) over bare name mentions, so ordinary task text that
/// merely discusses a provider does not hijack routing.
fn infer_provider_from_agent_text(
    desc: &str,
    prompt: &str,
) -> Option<(String, Option<String>)> {
    // 1) Explicit key=value / key:value in either field.
    for text in [desc, prompt] {
        if let Some(hit) = extract_explicit_provider_kv(text) {
            return Some(hit);
        }
    }
    // 2) Short description that is itself a provider alias ("claude review",
    //    "grok audit", "antigravity").
    if let Some(p) = resolve_provider_alias(desc.trim()) {
        return Some((p.id.to_string(), None));
    }
    // 3) Routing phrases. The description is a label the model wrote *about the
    //    spawn*, so a provider name at its head ("claude review") is routing.
    //    The prompt is the task itself, where a leading provider name is usually
    //    just the subject ("Claude Code session import path") — misreading that
    //    as routing blocks the spawn behind a /login the user never asked for,
    //    so the prompt only counts with an explicit cue ("… on claude").
    if let Some(hit) = extract_provider_routing_phrase(desc, true) {
        return Some(hit);
    }
    extract_provider_routing_phrase(prompt, false)
}

/// `provider:claude`, `provider=xai`, `model:grok-4` pairs in free text.
fn extract_explicit_provider_kv(text: &str) -> Option<(String, Option<String>)> {
    let lower = text.to_ascii_lowercase();
    let mut found_provider: Option<String> = None;
    let mut found_model: Option<String> = None;
    for (key, out) in [("provider", &mut found_provider), ("model", &mut found_model)] {
        for sep in [':', '='] {
            let needle = format!("{key}{sep}");
            if let Some(idx) = lower.find(&needle) {
                let rest = text[idx + needle.len()..].trim_start();
                let token: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
                    .collect();
                if !token.is_empty() {
                    *out = Some(token);
                }
            }
        }
    }
    let prov = found_provider.and_then(|raw| resolve_provider_alias(&raw).map(|p| p.id.to_string()))?;
    // If model was set but looks like a provider alias, drop it — keep real ids.
    let model = found_model.filter(|m| resolve_provider_alias(m).is_none() || m.contains('-') || m.contains('/'));
    Some((prov, model))
}

/// Phrases like "on claude", "using gemini", "via antigravity", "with grok",
/// "spawn a claude subagent", "deploy on xai".
///
/// `lead_counts` allows a bare hit at position 0 to route on its own. True for
/// the short description label, false for task prose.
fn extract_provider_routing_phrase(
    text: &str,
    lead_counts: bool,
) -> Option<(String, Option<String>)> {
    let lower = text.to_ascii_lowercase();
    // Prefer longer / more specific multi-word hits first.
    const PHRASES: &[&str] = &[
        "google antigravity",
        "claude sonnet",
        "claude opus",
        "claude haiku",
        "gemini flash",
        "gemini pro",
        "chatgpt",
        "antigravity",
        "deepseek",
        "openrouter",
        "moonshot",
        "anthropic",
        "openai",
        "gemini",
        "claude",
        "sonnet",
        "opus",
        "haiku",
        "grok",
        "xai",
        "mistral",
        "kimi",
        "qwen",
        "ollama",
        "flash",
        "gpt",
    ];
    // Only accept a hit when preceded by a routing cue or "subagent"/"agent".
    const CUES: &[&str] = &[
        " on ",
        " using ",
        " via ",
        " with ",
        " through ",
        " against ",
        " onto ",
        " deploy ",
        " deploy on ",
        " spawn ",
        " run on ",
        " routed to ",
        " target ",
        " provider ",
        " subagent on ",
        " subagent ",
        " agent on ",
        " agent ",
    ];
    for phrase in PHRASES {
        let Some(idx) = lower.find(phrase) else { continue };
        // Whole-word-ish: char before should be boundary.
        if idx > 0 {
            let prev = lower.as_bytes()[idx - 1] as char;
            if prev.is_alphanumeric() {
                continue;
            }
        }
        let after = idx + phrase.len();
        if after < lower.len() {
            let next = lower.as_bytes()[after] as char;
            if next.is_alphanumeric() || next == '-' {
                continue;
            }
        }
        // Window before the match for a routing cue.
        let window_start = idx.saturating_sub(24);
        let window = &lower[window_start..idx];
        // A hit at the head of the text routes on its own only where a leading
        // provider name means routing (the description label), never in prose.
        let leads = idx == 0 || window.trim().is_empty();
        let cued = (leads && lead_counts)
            || CUES.iter().any(|c| window.ends_with(c.trim_start()) || window.contains(c));
        if !cued {
            // Also allow "…claude subagent" / "…grok agent" immediately after.
            let tail = &lower[after..];
            let tail_ok = tail.trim_start().starts_with("subagent")
                || tail.trim_start().starts_with("agent")
                || tail.trim_start().starts_with("reviewer")
                || tail.trim_start().starts_with("review");
            if !tail_ok {
                continue;
            }
        }
        if let Some(p) = resolve_provider_alias(phrase) {
            return Some((p.id.to_string(), None));
        }
    }
    None
}

async fn run_agent_tool(
    runner: &AgentRunner,
    call: &FunctionCallRef,
    cancel: &CancellationToken,
    tx: &mpsc::UnboundedSender<AgentEvent>,
) -> Result<(String, TokenUsage)> {
    let (prompt, kind, desc, provider_override, model_override) = parse_agent_call(call)?;
    let _ = tx.send(AgentEvent::Status(format!("subagent · {desc}")));

    match resolve_subagent_target(
        &runner.client,
        &runner.config,
        provider_override.as_deref(),
        model_override.as_deref(),
        Some(prompt.as_str()),
        Some(desc.as_str()),
        Some(kind.as_str()),
        tx,
    ) {
        SubagentTarget::Ready { client, config } => {
            subagent::run_subagent(
                client,
                config,
                runner.cwd.clone(),
                runner.permission_mode.clone(),
                &prompt,
                &kind,
                cancel,
                tx,
            )
            .await
        }
        SubagentTarget::AwaitingLogin { message, .. } => {
            // Surface as a tool error so the parent model does not treat a
            // parent-provider run as success. LoginRequired was already emitted.
            Err(MuseError::Other(message))
        }
    }
}

/// Outcome of resolving where a subagent should run.
enum SubagentTarget {
    Ready {
        client: ApiClient,
        config: Config,
    },
    /// Explicit cross-provider request, but no credentials yet. Do not run.
    AwaitingLogin {
        #[allow(dead_code)]
        provider_id: String,
        #[allow(dead_code)]
        provider_name: String,
        message: String,
    },
}

/// Resolve a subagent's client + config, honoring an optional cross-provider
/// override. When `provider` names a DIFFERENT provider than the parent, build a
/// client from that provider's stored credentials + catalog base/model.
///
/// Unknown provider names fall back to the parent with a status note. A **known**
/// provider with **no credentials** does **not** fall back — it emits
/// [`AgentEvent::LoginRequired`] and returns [`SubagentTarget::AwaitingLogin`]
/// so the TUI can open `/login` and the model is told the spawn was blocked.
fn resolve_subagent_target(
    parent_client: &ApiClient,
    parent_config: &Config,
    provider: Option<&str>,
    model: Option<&str>,
    retry_prompt: Option<&str>,
    retry_desc: Option<&str>,
    retry_kind: Option<&str>,
    tx: &mpsc::UnboundedSender<AgentEvent>,
) -> SubagentTarget {
    let Some(requested) = provider else {
        // No override: inherit parent, but still allow a model-only override.
        if let Some(m) = model {
            let mut cfg = parent_config.clone();
            cfg.model = m.to_string();
            return SubagentTarget::Ready {
                client: parent_client.clone(),
                config: cfg,
            };
        }
        return SubagentTarget::Ready {
            client: parent_client.clone(),
            config: parent_config.clone(),
        };
    };
    let Some(prov) = resolve_provider_alias(requested) else {
        let _ = tx.send(AgentEvent::Status(format!(
            "subagent · unknown provider '{requested}' — using parent provider instead"
        )));
        return SubagentTarget::Ready {
            client: parent_client.clone(),
            config: parent_config.clone(),
        };
    };
    // Same provider as parent — or same account family (google / antigravity /
    // google-oauth all share one Google OAuth session) — means the model is
    // calling a provider the user is *already using*. Skip every bit of the
    // cross-provider machinery below (credential re-resolution, vendor-CLI
    // probing, client rebuild) and just reuse the parent client verbatim, the
    // same way subagents worked before cross-provider routing existed. A bare
    // `prov.id == parent_config.provider` check missed this: "gemini" resolves
    // to catalog id `google`, a different id from an active `antigravity`
    // session, even though it's the same provider/account.
    let same_provider = prov.id == parent_config.provider
        || (crate::providers::is_google_family(prov.id)
            && crate::providers::is_google_family(&parent_config.provider));
    if same_provider {
        let mut cfg = parent_config.clone();
        if let Some(m) = model {
            cfg.model = m.to_string();
        }
        return SubagentTarget::Ready {
            client: parent_client.clone(),
            config: cfg,
        };
    }
    // Different provider: resolve its credential and build a client.
    let key = match crate::auth::resolve_api_key_for(Some(prov.id)) {
        Ok(k) if !k.trim().is_empty() => k,
        _ => {
            // Try the per-provider failover stores (key or OAuth token).
            crate::auth::load_provider_key(prov.id)
                .or_else(|| crate::auth::load_provider_oauth_token(prov.id))
                .unwrap_or_default()
        }
    };
    let mut key = key;
    if key.trim().is_empty() && !prov.key_optional {
        // No stored credential yet. Before popping a /login modal, try to
        // auto-import the vendor CLI (t3code driver) session for this provider,
        // persist it into the per-provider OAuth store, and re-resolve. If that
        // works the subagent "just works" with no user input.
        if let Some(driver) = driver_for_provider(prov.id) {
            // Both probe_driver and import_existing_session can shell out
            // (reading a vendor CLI's credential file/store) — isolate via
            // run_blocking so a slow probe can't stall the whole runtime.
            let has_credentials =
                crate::oauth::run_blocking(|| crate::t3code::probe_driver(driver)).has_credentials;
            if has_credentials {
                match crate::oauth::run_blocking(|| crate::oauth::import_existing_session(prov.id)) {
                    Ok(Some(tokens)) if !tokens.access_token.trim().is_empty() => {
                        // Persist so load_provider_oauth_token can re-resolve it.
                        let _ = crate::auth::save_provider_oauth(
                            prov.id,
                            &tokens.access_token,
                            tokens.refresh_token.clone(),
                            tokens.expires_at,
                            tokens.meta.clone(),
                        );
                        // Re-resolve the credential now that a session exists.
                        let reresolved = match crate::auth::resolve_api_key_for(Some(prov.id)) {
                            Ok(k) if !k.trim().is_empty() => k,
                            _ => crate::auth::load_provider_key(prov.id)
                                .or_else(|| crate::auth::load_provider_oauth_token(prov.id))
                                .unwrap_or_else(|| tokens.access_token.trim().to_string()),
                        };
                        if !reresolved.trim().is_empty() {
                            let _ = tx.send(AgentEvent::Status(format!(
                                "subagent · imported {} session from vendor CLI ({})",
                                prov.name,
                                driver.as_str()
                            )));
                            key = reresolved;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    if key.trim().is_empty() && !prov.key_optional {
        // No stored credential and no importable vendor CLI session. Open /login
        // pre-selected to this provider and BLOCK the spawn — never silently run
        // on the parent (that made models think cross-provider "worked").
        let _ = tx.send(AgentEvent::LoginRequired {
            provider_id: prov.id.to_string(),
            provider_name: prov.name.to_string(),
            retry_prompt: retry_prompt.map(str::to_string),
            retry_desc: retry_desc.map(str::to_string),
            retry_kind: retry_kind.map(str::to_string),
            retry_model: model.map(str::to_string),
        });
        let _ = tx.send(AgentEvent::Status(format!(
            "subagent · not signed in to {} — opening /login {} (spawn blocked until you authenticate)",
            prov.name, prov.id
        )));
        let message = format!(
            "blocked: not signed in to {name} (provider id `{id}`). \
             A /login modal was opened pre-selected to `{id}`. \
             Do NOT re-run this subagent on the parent provider. \
             After the user finishes /login, nur will inject a mandatory re-deploy \
             instruction with the exact `agent` tool call (provider=\"{id}\"). \
             Original task is preserved for that retry.",
            name = prov.name,
            id = prov.id,
        );
        return SubagentTarget::AwaitingLogin {
            provider_id: prov.id.to_string(),
            provider_name: prov.name.to_string(),
            message,
        };
    }
    // The catalog row describes the API-key endpoint. When the credential we just
    // resolved is an OAuth access token the provider answers somewhere else
    // entirely (ChatGPT → Codex backend, Grok Build → CLI proxy in Responses
    // shape, Google `ya29.` → Cloud Code), on a different default model. Aiming
    // the token at the key-only host is an immediate 401, so resolve the real
    // endpoint the same way failover does.
    let is_oauth = crate::auth::oauth_request_context(prov.id, &key).is_some();
    let (base_url, style, default_model) =
        crate::providers::endpoint_for_credential(prov, is_oauth);
    // key_optional local providers may have an empty key.
    let client = match ApiClient::for_provider(base_url, &key, prov.id) {
        Ok(c) => c.with_style(style),
        Err(e) => {
            let _ = tx.send(AgentEvent::Status(format!(
                "subagent · could not build {} client ({e}) — using parent",
                prov.name
            )));
            return SubagentTarget::Ready {
                client: parent_client.clone(),
                config: parent_config.clone(),
            };
        }
    };
    let mut cfg = parent_config.clone();
    cfg.provider = prov.id.to_string();
    cfg.base_url = base_url.trim_end_matches('/').to_string();
    cfg.model = model
        .map(str::to_string)
        .unwrap_or_else(|| default_model.to_string());
    // A failover chain configured for the parent's account is not this child's:
    // it would quietly move a "run this on grok" subagent onto some other
    // vendor and report the answer as grok's. Explicit routing means explicit.
    cfg.fallback_providers.clear();
    let _ = tx.send(AgentEvent::Status(format!(
        "subagent · routed to {} · {}",
        prov.name, cfg.model
    )));
    SubagentTarget::Ready {
        client,
        config: cfg,
    }
}

/// Map a natural-language provider name to a catalog provider. Thin wrapper over
/// [`crate::providers::resolve_provider_alias`] - the single source of truth
/// shared with `/login <provider>` in the TUI - so cross-provider subagent
/// deployment and the login modal accept the exact same aliases.
fn resolve_provider_alias(raw: &str) -> Option<&'static crate::providers::Provider> {
    crate::providers::resolve_provider_alias(raw)
}

/// Map a nur provider id to the vendor CLI (t3code) driver that can supply its
/// credentials via `import_existing_session`. Used to auto-import a logged-in
/// vendor CLI session before falling back to a `/login` prompt so cross-provider
/// subagents "just work" when the user is already signed in to the vendor CLI.
/// Returns `None` for providers with no direct vendor CLI (e.g. cursor has no
/// nur provider of its own).
fn driver_for_provider(provider_id: &str) -> Option<crate::t3code::DriverId> {
    use crate::t3code::DriverId;
    match provider_id {
        "anthropic" => Some(DriverId::Claude),
        "openai" => Some(DriverId::Codex),
        "xai" => Some(DriverId::Grok),
        "antigravity" => Some(DriverId::Antigravity),
        "google" => Some(DriverId::Gemini),
        "opencode" => Some(DriverId::OpenCode),
        _ => None,
    }
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

    // New context: summary + last N user/assistant display messages + the tail of
    // the live working items.
    //
    // That last part is not optional. `session.messages` only gains an assistant
    // entry when the turn *returns*, so mid-run it holds the user prompt and
    // nothing else. Rebuilding from it alone handed the model a summary and the
    // original request with no trace of the work in flight — so it answered as
    // if starting fresh, produced no tool calls, and the loop treated that as a
    // completed turn. A silent stop, right at the token volume where compaction
    // first fires.
    let keep_n = runner.config.compact_keep_user_turns.max(1) as usize;
    let mut new_items = vec![user_text_item(&format!(
        "[Context compacted. Summary of the conversation so far:]\n\n{summary}"
    ))];
    let recent = recent_dialogue_items(&session.messages, keep_n);
    let kept = recent.len();
    new_items.extend(recent);
    let tail = safe_tail_after_compact(&session.input_items, COMPACT_KEEP_WORKING_ITEMS);
    let tail_kept = tail.len();
    new_items.extend(tail);
    session.input_items = new_items;
    runner.persist_session(session);
    Ok(format!(
        "{summary}\n\n[compact: thinned {thinned} tool bodies · kept {kept} recent dialogue items · \
         {tail_kept} working items · precompact bak written]"
    ))
}

/// Working items carried across a compaction so the model can see the task it
/// was mid-way through.
const COMPACT_KEEP_WORKING_ITEMS: usize = 12;

/// Take the tail of the working items, keeping only *complete* tool pairs.
///
/// A `function_call_output` whose `function_call` was summarised away is a hard
/// 400 on both wire formats — OpenAI rejects a `tool` message with no preceding
/// declaration, Anthropic rejects a `tool_result` with no matching `tool_use`.
/// Dropping either half of a split pair keeps the slice valid for both.
fn safe_tail_after_compact(items: &[Value], want: usize) -> Vec<Value> {
    let start = items.len().saturating_sub(want);
    let tail = &items[start..];
    fn kind(v: &Value) -> &str {
        v.get("type").and_then(|t| t.as_str()).unwrap_or("")
    }
    fn call_id(v: &Value) -> &str {
        v.get("call_id").and_then(|c| c.as_str()).unwrap_or("")
    }
    let calls: std::collections::HashSet<&str> = tail
        .iter()
        .filter(|v| kind(v) == "function_call")
        .map(call_id)
        .collect();
    let outputs: std::collections::HashSet<&str> = tail
        .iter()
        .filter(|v| kind(v) == "function_call_output")
        .map(call_id)
        .collect();
    tail.iter()
        .filter(|v| match kind(v) {
            "function_call" => outputs.contains(call_id(v)),
            "function_call_output" => calls.contains(call_id(v)),
            _ => true,
        })
        .cloned()
        .collect()
}

/// Truncate oversized `function_call_output` bodies outside the last `keep_user_turns`
/// user messages. Returns how many bodies were thinned.
fn thin_tool_bodies_for_compact(
    items: &mut [Value],
    max_chars: usize,
    keep_user_turns: usize,
) -> usize {
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

#[cfg(test)]
mod compact_tail_tests {
    use super::*;
    use serde_json::json;

    fn call(id: &str) -> Value {
        json!({"type":"function_call","call_id":id,"name":"read","arguments":"{}"})
    }
    fn output(id: &str) -> Value {
        json!({"type":"function_call_output","call_id":id,"output":"ok"})
    }
    fn text() -> Value {
        json!({"role":"user","content":[{"type":"input_text","text":"hi"}]})
    }

    /// A result whose call was summarised away is a hard 400 on both wire
    /// formats. Whatever the cut point, the kept slice must be self-consistent.
    #[test]
    fn tail_never_keeps_a_result_without_its_call() {
        let items: Vec<Value> = vec![
            text(),
            call("a"),
            output("a"),
            call("b"),
            output("b"),
            call("c"),
            output("c"),
        ];
        for want in 0..=items.len() + 2 {
            let tail = safe_tail_after_compact(&items, want);
            let calls: std::collections::HashSet<&str> = tail
                .iter()
                .filter(|v| v["type"] == "function_call")
                .map(|v| v["call_id"].as_str().unwrap())
                .collect();
            for v in &tail {
                if v["type"] == "function_call_output" {
                    assert!(
                        calls.contains(v["call_id"].as_str().unwrap()),
                        "want={want} kept an orphaned result: {tail:?}"
                    );
                }
            }
        }
    }

    /// A call whose result was cut is equally invalid (Anthropic requires every
    /// tool_use to be answered).
    #[test]
    fn tail_never_keeps_a_call_without_its_result() {
        let items = vec![text(), call("a"), output("a"), call("b"), output("b")];
        for want in 0..=items.len() {
            let tail = safe_tail_after_compact(&items, want);
            let outs: std::collections::HashSet<&str> = tail
                .iter()
                .filter(|v| v["type"] == "function_call_output")
                .map(|v| v["call_id"].as_str().unwrap())
                .collect();
            for v in &tail {
                if v["type"] == "function_call" {
                    assert!(
                        outs.contains(v["call_id"].as_str().unwrap()),
                        "want={want} kept a dangling call: {tail:?}"
                    );
                }
            }
        }
    }

    /// The point of carrying a tail at all: after compaction the model must
    /// still see it was mid-task, or it answers as if starting fresh and the
    /// loop reads that as a finished turn.
    #[test]
    fn tail_carries_recent_work_forward() {
        let items = vec![text(), call("a"), output("a")];
        let tail = safe_tail_after_compact(&items, COMPACT_KEEP_WORKING_ITEMS);
        assert!(
            tail.iter().any(|v| v["type"] == "function_call"),
            "compaction must not erase the in-flight work: {tail:?}"
        );
    }

    #[test]
    fn empty_items_are_safe() {
        assert!(safe_tail_after_compact(&[], 12).is_empty());
    }
}

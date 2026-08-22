use super::hooks::HooksConfig;
use super::mode::{PermissionMode, SharedMode};
use super::permissions::{RuleDecision, SharedPermissions};
use super::prompt::{attribute_request, PromptContext};
use super::receipt;
use super::session::Session;
use super::subagent;
use crate::api::types::{
    function_call_output_item, replay_output_items, user_multimodal_item, user_text_item,
    FunctionCallRef, ReasoningConfig, ResponseRequest,
};
use crate::api::{ApiClient, ApiResponse, StreamEvent};
use crate::config::Config;
use crate::error::{NurError, Result};
use crate::tools::media::{self, MediaAttach};
use crate::tools::{is_parallel_safe, is_read_only_call, ToolContext, ToolHost};
use crate::usage::{TokenUsage, UsageTracker};
use serde_json::Value;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
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
    /// `/fusion` named a provider that has no usable saved, environment, local,
    /// vendor-CLI, or OMP credential. The TUI opens the same provider auth
    /// window and replays the exact fusion request after authentication.
    FusionLoginRequired {
        provider_id: String,
        provider_name: String,
        question: String,
        panel_ids: Vec<String>,
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
    /// Nested subagents can spawn further agents up to `config.subagent_depth`
    /// (default 1 = children only, no grandchildren). This field is the *current*
    /// runner's nesting level (0 = root). Enables budgeted RLM-style recursion.
    pub is_subagent: bool,
    /// Current recursion depth of this runner (0 = root). Incremented when a
    /// subagent spawns children; bounded by `config.subagent_depth`.
    pub subagent_depth: u32,
    /// OMP-style prewalk: once fired, later turns in this TUI session use this
    /// model (shared Arc so `make_runner` each turn still sees the switch).
    pub prewalk_override: Arc<Mutex<Option<String>>>,
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
        let interrupted = matches!(res, Err(NurError::Interrupted));
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
    mut session: Session,
    mut usage: UsageTracker,
    prompt: String,
    cancel: CancellationToken,
) -> (
    Box<Session>,
    Box<UsageTracker>,
    std::result::Result<String, String>,
    bool,
) {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let collector = tokio::spawn(async move {
        let mut acc = String::new();
        while let Some(ev) = rx.recv().await {
            match ev {
                AgentEvent::TextDelta(d) => acc.push_str(&d),
                AgentEvent::AssistantMessage(m) => {
                    if acc.trim().is_empty() {
                        acc = m;
                    }
                }
                // No interactive approver in headless integrations - deny
                // anything that slips through. Callers should use Auto mode.
                AgentEvent::ApprovalRequest { respond, .. } => {
                    let _ = respond.send(ApprovalDecision::Deny);
                }
                _ => {}
            }
        }
        acc
    });

    // Run headless turns directly so ownership of the session and usage never
    // depends on receiving a final channel event. The old detached-task path
    // used `unreachable!` when a task panicked or the channel closed, turning a
    // recoverable backend failure into a parent-process panic.
    let result = runner
        .run_turn_events(&mut session, &prompt, &mut usage, &tx, &cancel)
        .await;
    usage.set_state("idle");
    if !runner.is_subagent {
        let _ = session.save();
    }
    drop(tx);
    let acc = collector.await.unwrap_or_default();
    let interrupted = matches!(result, Err(NurError::Interrupted));
    let result = result
        .map(|text| if text.trim().is_empty() { acc } else { text })
        .map_err(|error| error.to_string());
    (Box::new(session), Box::new(usage), result, interrupted)
}

/// Which provider/model actually served a model request (for the receipt).
struct Served {
    provider: String,
    model: String,
    failover: bool,
}

fn provider_turn_timeout_from(value: Option<&str>) -> std::time::Duration {
    value
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .map(std::time::Duration::from_secs)
        .unwrap_or_else(|| std::time::Duration::from_secs(300))
}

fn provider_turn_timeout() -> std::time::Duration {
    let value = std::env::var("NUR_PROVIDER_TURN_TIMEOUT_SECS").ok();
    provider_turn_timeout_from(value.as_deref())
}

impl AgentRunner {
    fn persist_session(&self, session: &Session) {
        if !self.is_subagent {
            let _ = session.save();
        }
    }

    /// Session id used for async subagent admissions. The runner does not own a
    /// session; prefer the ambient `NUR_SESSION_ID`, else a process-global id.
    fn session_id_for_admission(&self) -> String {
        if let Ok(sid) = std::env::var("NUR_SESSION_ID") {
            if !sid.is_empty() {
                return sid;
            }
        }
        static PROC: OnceLock<String> = OnceLock::new();
        PROC.get_or_init(|| format!("proc-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]))
            .clone()
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
    ) -> std::result::Result<(ApiResponse, usize), (NurError, usize)> {
        let delta_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let turn_cancel = cancel.child_token();
        let timeout = provider_turn_timeout();
        // Cursor Agent CLI must honor Esc cancel; always take the streaming path
        // so the CancellationToken reaches `cursor-agent` (even for subagents).
        if req.stream == Some(true) || client.uses_cursor_cli() {
            let streamed_count = delta_count.clone();
            let request = client.create_response_stream(
                req,
                move |ev| match ev {
                    StreamEvent::TextDelta(d) => {
                        streamed_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let _ = tx.send(AgentEvent::TextDelta(d));
                    }
                    StreamEvent::ReasoningDelta(d) => {
                        let _ = tx.send(AgentEvent::ReasoningDelta(d));
                    }
                    StreamEvent::Completed(_) => {}
                },
                &turn_cancel,
            );
            tokio::pin!(request);
            let r = tokio::select! {
                _ = cancel.cancelled() => {
                    turn_cancel.cancel();
                    return Err((
                        NurError::Interrupted,
                        delta_count.load(std::sync::atomic::Ordering::Relaxed),
                    ));
                }
                _ = tokio::time::sleep(timeout) => {
                    turn_cancel.cancel();
                    Err(NurError::Other(format!(
                        "provider turn exceeded the {}s timeout (set NUR_PROVIDER_TURN_TIMEOUT_SECS to adjust)",
                        timeout.as_secs()
                    )))
                }
                result = &mut request => result,
            };
            let deltas = delta_count.load(std::sync::atomic::Ordering::Relaxed);
            match r {
                Ok(resp) => Ok((resp, deltas)),
                Err(e) => Err((e, deltas)),
            }
        } else {
            tokio::select! {
                _ = cancel.cancelled() => {
                    turn_cancel.cancel();
                    Err((NurError::Interrupted, 0))
                },
                _ = tokio::time::sleep(timeout) => {
                    turn_cancel.cancel();
                    Err((NurError::Other(format!(
                        "provider turn exceeded the {}s timeout (set NUR_PROVIDER_TURN_TIMEOUT_SECS to adjust)",
                        timeout.as_secs()
                    )), 0))
                },
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
        usage: &UsageTracker,
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
                            model: req.model.clone(),
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
                            _ = cancel.cancelled() => return Err(NurError::Interrupted),
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
            let preflight =
                match crate::api::failover::preflight_target(&t, req, &self.config, usage) {
                    Ok(summary) => summary,
                    Err(reason) => {
                        let _ = tx.send(AgentEvent::Status(format!(
                            "failover skipped {} · {}",
                            t.provider_id, reason
                        )));
                        continue;
                    }
                };
            let _ = tx.send(AgentEvent::Status(format!(
                "provider error ({last}) — failing over to {} · {}",
                t.provider_id, t.model
            )));
            let _ = tx.send(AgentEvent::Status(format!(
                "failover preflight · {preflight}"
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
            if let Some(reasoning) = req2.reasoning.as_mut() {
                reasoning.effort =
                    crate::providers::nearest_effort(&t.provider_id, &self.config.reasoning_effort);
            }
            let target_caps = crate::api::failover::route_capabilities(&t.provider_id, t.style);
            if !target_caps.parallel_tools {
                req2.parallel_tool_calls = Some(false);
            }
            if !client.supports_output_limit(&req2.model) {
                req2.max_output_tokens = None;
            }
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
        // Peer lifecycle presence (pi-peer port): make this session addressable
        // by other sessions and mark it working for the turn. Best-effort and
        // non-fatal - a registry we cannot write is a discovery problem only.
        let mut peer_mail = String::new();
        if !self.is_subagent {
            let sid = std::env::var("NUR_SESSION_ID").unwrap_or_else(|_| session.id.clone());
            let cwd = self.cwd.to_string_lossy().into_owned();
            // Persistent watch lives across turns and drains within the sender's
            // 1.5s receipt window. Re-assert Working after watch registration.
            crate::agent::mailbox::ensure_live_watch(&cwd, &sid);
            crate::agent::mailbox::register_session(
                &cwd,
                &sid,
                None,
                crate::agent::mailbox::PeerState::Working,
            );
            // Existing queued mail may race the just-started watcher: combine a
            // direct turn-start drain with anything the watcher already queued.
            peer_mail = crate::agent::mailbox::drain_inbound_for_prompt();
            peer_mail.push_str(&crate::agent::mailbox::take_pending_peer_prompt());
            if !peer_mail.is_empty() {
                let _ = tx.send(AgentEvent::Status(
                    "peer messages arrived from another session".into(),
                ));
            }
        }
        // Discard any media a prior turn queued but never flushed (e.g. `look`
        // ran, then the turn was cancelled before the attach) so a stale image
        // can't bleed onto this unrelated prompt.
        let _ = media::take_pending_media();
        // Portable input guardrails (OpenAI Agents SDK pattern) - all providers.
        match super::guardrails::check_input(user_text) {
            super::guardrails::GuardDecision::Block(msg) => {
                let _ = tx.send(AgentEvent::Status(format!(
                    "guardrail blocked input · {msg}"
                )));
                return Err(NurError::Other(format!("input guardrail: {msg}")));
            }
            super::guardrails::GuardDecision::Warn(w) => {
                let _ = tx.send(AgentEvent::Status(format!("guardrail · {w}")));
            }
            super::guardrails::GuardDecision::Allow => {}
        }
        // Track tokens toward a persistent goal when present.
        session.push_user(user_text);
        // Peer mail from other sessions (real receipt + authority boundary) is
        // prepended so it arrives mid-turn; see the registration block above.
        let prompt_text = if peer_mail.is_empty() {
            user_text.to_string()
        } else {
            format!("{user_text}{peer_mail}")
        };
        // Auto-attach media paths mentioned in the user prompt (png/mp4/…).
        let auto_notes = media::auto_attach_from_text(&self.cwd, &prompt_text);
        let pending = media::take_pending_media();
        if pending.is_empty() {
            session.input_items.push(user_text_item(&prompt_text));
        } else {
            let mut text = prompt_text.clone();
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
        let mut emergency_compactions: u8 = 0;
        // Codex/ChatGPT free (and some hosts) sometimes emit only a reasoning
        // summary and zero tool calls / zero answer text. Retry once with a
        // hard nudge + tool_choice=required before giving up.
        let mut empty_tool_stalls: u8 = 0;
        let mut truncation_continuations: u8 = 0;
        let mut truncation_giving_up = false;
        let mut force_tool_choice = false;
        let mut recovered_default_model: Option<String> = None;
        // OMP contextPromotion: try a larger-window sibling once before compact.
        let mut context_promoted = false;

        loop {
            if cancel.is_cancelled() {
                return Err(NurError::Interrupted);
            }
            turns += 1;
            // max_turns == 0 → unlimited (overnight / long agent loops).
            if self.config.max_turns > 0 && turns > self.config.max_turns {
                return Err(NurError::MaxTurns(self.config.max_turns));
            }
            if let Some(msg) = session_budget_exceeded(&self.config, usage) {
                let _ = tx.send(AgentEvent::Status(msg.clone()));
                return Err(NurError::Budget(msg));
            }
            if turns == 1
                && self.config.max_turns == 0
                && self.config.max_session_tokens.is_none()
                && self.config.max_session_cost_usd.is_none()
            {
                let _ = tx.send(AgentEvent::Status(
                    "task completion is unlimited; spend is not capped - set /budget tokens or /budget cost for a hard stop"
                        .into(),
                ));
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
                && should_auto_compact(usage, &self.config, &session.input_items)
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

            // pi-peer live delivery: the persistent inbox watcher drains letters
            // while this turn runs and queues authority-framed text here. Inject
            // before the next provider request, just like user steering, so the
            // recipient model observes mail mid-task and the sender gets a true
            // consumed-file receipt within its 1.5s window.
            if !self.is_subagent {
                let peer_steer = crate::agent::mailbox::take_pending_peer_prompt();
                if !peer_steer.is_empty() {
                    session.input_items.push(user_text_item(&peer_steer));
                    self.persist_session(session);
                    let _ = tx.send(AgentEvent::Status(
                        "peer message · injected mid-turn from another session".into(),
                    ));
                }
            }

            // OMP-style dynamic context pruning (supersedeReads + dropUseless):
            // when the model reads the same target again, the newer observation
            // supersedes the old body; empty/error tool bodies outside the live
            // suffix are collapsed so they stop paying rent every turn.
            let superseded = prune_superseded_observations(&mut session.input_items);
            let dropped = prune_useless_observations(&mut session.input_items);
            if superseded + dropped > 0 {
                self.persist_session(session);
                let mut parts = Vec::new();
                if superseded > 0 {
                    parts.push(format!("{superseded} superseded read/search"));
                }
                if dropped > 0 {
                    parts.push(format!("{dropped} uneventful empty tool body"));
                }
                let _ = tx.send(AgentEvent::Status(format!(
                    "context · pruned {}",
                    parts.join(" · ")
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
            // Precedence: prewalk override → recovery/promotion → config.model.
            let prewalk_held = self.prewalk_override.lock().ok().and_then(|g| g.clone());
            let configured_model = prewalk_held
                .as_deref()
                .or(recovered_default_model.as_deref())
                .unwrap_or(&self.config.model);
            let effective_model = if crate::providers::is_placeholder_local_model(configured_model)
            {
                let resolved = self.client.resolve_local_model(configured_model).await;
                if resolved != configured_model {
                    let _ = tx.send(AgentEvent::Status(format!(
                        "local model placeholder → resolved to `{resolved}` via /models"
                    )));
                }
                resolved
            } else {
                configured_model.to_string()
            };

            let tools = if self.is_subagent {
                self.tools.subagent_tool_defs()
            } else {
                self.tools.root_tool_defs_for_task(user_text, turns > 1)
            };
            let attribution = attribute_request(&instructions, &tools, &session.input_items);
            let output_reserve = self.config.request_output_reserve_tokens;
            if let Some(msg) = preflight_request_budget(
                &self.config,
                usage,
                &self.config.provider,
                &effective_model,
                attribution.estimated_input_tokens(),
                output_reserve,
            ) {
                let _ = tx.send(AgentEvent::Status(msg.clone()));
                return Err(NurError::Budget(msg));
            }
            let _ = tx.send(AgentEvent::Status(format!(
                "prompt ~{} tok (system {} · tools {} · dialogue {} · tool output {}) · stable-prefix {} · output reserve {}",
                attribution.estimated_input_tokens(),
                attribution.system_tokens,
                attribution.tool_schema_tokens,
                attribution.dialogue_tokens,
                attribution.tool_output_tokens,
                attribution.stable_prefix_fingerprint,
                output_reserve,
            )));
            let primary_style = crate::providers::by_id(&self.config.provider)
                .map(|provider| provider.style)
                .unwrap_or(crate::providers::ApiStyle::Responses);
            let primary_caps =
                crate::api::failover::route_capabilities(&self.config.provider, primary_style);
            let primary_output_limit = self.client.supports_output_limit(&effective_model);
            if !primary_caps.parallel_tools || !primary_output_limit {
                let mut limits = Vec::new();
                if !primary_caps.parallel_tools {
                    limits.push("serial tool calls");
                }
                if !primary_output_limit {
                    limits.push("provider output limit unavailable");
                }
                let _ = tx.send(AgentEvent::Status(format!(
                    "provider capability profile · {}",
                    limits.join(" · ")
                )));
            }

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
                // Native subagents must inherit streaming. ChatGPT/Codex OAuth
                // rejects Responses requests with `stream:false` (HTTP 400:
                // "Stream must be set to true"), so force it on that route even
                // when the user's global stream display preference is off.
                stream: Some(self.config.stream || self.client.requires_streaming_responses()),
                parallel_tool_calls: Some(primary_caps.parallel_tools),
                // One cache key per session so system instructions + tools can be
                // prefix-cached across multi-turn agent loops.
                prompt_cache_key: Some(session.id.clone()),
                max_output_tokens: (output_reserve > 0 && primary_output_limit)
                    .then_some(output_reserve),
            };

            let (resp, text_deltas, served): (ApiResponse, usize, Served) = match self
                .request_with_failover(&req, usage, tx, cancel)
                .await
            {
                Ok(response) => response,
                Err(error) if is_context_limit_error(&error) => {
                    // OMP contextPromotion: switch to a larger sibling *before*
                    // compaction when the API rejects the window (e.g. *-spark).
                    if !context_promoted {
                        context_promoted = true;
                        let current = recovered_default_model
                            .as_deref()
                            .unwrap_or(&self.config.model);
                        if let Ok(models) = self.client.live_model_ids().await {
                            if let Some(model) = pick_context_promotion_target(&models, current) {
                                let _ = tx.send(AgentEvent::Status(format!(
                                    "context overflow · promoting `{current}` → `{model}` \
                                         (OMP-style contextPromotion before compact)"
                                )));
                                recovered_default_model = Some(model);
                                continue;
                            }
                        }
                    }
                    if emergency_compactions >= MAX_EMERGENCY_COMPACTIONS {
                        return Err(error);
                    }
                    emergency_compactions += 1;
                    let _ = tx.send(AgentEvent::Status(format!(
                        "provider rejected the context window - recovering and retrying \
                             ({emergency_compactions}/{MAX_EMERGENCY_COMPACTIONS})"
                    )));

                    match compact_session(self, session, usage).await {
                        Ok(_) => {
                            compactions = compactions.saturating_add(1);
                            let _ = tx.send(AgentEvent::Status(
                                "emergency context compaction succeeded - continuing".into(),
                            ));
                        }
                        Err(compact_error) => {
                            // A model-assisted summary can itself exceed the
                            // provider window. Keep a valid recent working
                            // set locally so the turn continues instead of
                            // dying at exactly the point compaction is needed.
                            let kept = emergency_compact_session(self, session);
                            compactions = compactions.saturating_add(1);
                            let _ = tx.send(AgentEvent::Status(format!(
                                "model compaction failed ({compact_error}); recovered a \
                                     valid {kept}-item recent context locally - continuing"
                            )));
                        }
                    }
                    continue;
                }
                Err(error)
                    if recovered_default_model.is_none()
                        && is_model_unavailable_error(&error)
                        && is_catalog_default_model(&self.config) =>
                {
                    match self.client.live_model_ids().await {
                        Ok(models) => {
                            if let Some(model) = pick_replacement_model(&models, &self.config.model)
                            {
                                let _ = tx.send(AgentEvent::Status(format!(
                                    "provider no longer serves default model `{}` - \
                                         retrying this turn with live model `{model}`",
                                    self.config.model
                                )));
                                recovered_default_model = Some(model);
                                continue;
                            }
                            return Err(error);
                        }
                        Err(discovery_error) => {
                            let _ = tx.send(AgentEvent::Status(format!(
                                "default model is unavailable and live model discovery \
                                     failed: {discovery_error}"
                            )));
                            return Err(error);
                        }
                    }
                }
                Err(error) => return Err(error),
            };

            // Every completed request gets a ledger row. `accounting_usage`
            // prefers provider telemetry and falls back to a local estimate,
            // so missing usage is never interpreted as free usage.
            let raw = resp.accounting_usage();
            usage.record_request_for_route(&served.provider, &served.model, raw, resp.id.clone());
            let tu = usage.last_usage().clone();
            session.usage.add(&tu);
            let (in_tok, out_tok) = (tu.input_tokens, tu.output_tokens);
            let _ = tx.send(AgentEvent::Usage {
                session: usage.session_usage().clone(),
                last: tu,
            });

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

            let mut calls = resp.function_calls();
            let text = resp.output_text();
            let suppress_duplicate_preamble =
                !calls.is_empty() && is_duplicate_tool_preamble(&session.input_items, &text);
            let mut replayed = replay_output_items(&resp.output);
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
                if truncation_giving_up || truncation_continuations >= MAX_TRUNCATION_CONTINUATIONS
                {
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
                    session
                        .input_items
                        .push(crate::api::types::user_text_item(nudge));
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
                    "model stopped for content_filter — surfacing partial output and ending turn"
                        .into(),
                ));
            }
            // ── unknown / network_error finish reasons: continue, don't stop ──
            // Upstream OpenCode #43892 ("loop continues when finish is
            // unknown") + #43813 ("retry raw network finish errors"). A
            // gateway hiccup (OpenCode Zen free routes especially) can end a
            // 200 stream with finish_reason `network_error` or an unrecognized
            // value right after visible text. Treating that as a completed
            // turn is exactly the "ox-alpha stops mid-run" symptom. When the
            // round produced text but no tool calls, nudge the model to pick
            // up where it stopped — bounded by the same guard as truncation.
            if matches!(
                resp.status.as_deref(),
                Some("network_error") | Some("unknown")
            ) && calls.is_empty()
            {
                let reason = resp.status.clone().unwrap_or_default();
                if truncation_giving_up || truncation_continuations >= MAX_TRUNCATION_CONTINUATIONS
                {
                    if !truncation_giving_up {
                        truncation_giving_up = true;
                        let _ = tx.send(AgentEvent::Status(format!(
                            "stream ended early (finish_reason: {reason}) {truncation_continuations}× — giving up and surfacing partial output"
                        )));
                    }
                    // Fall through to the normal completion path with whatever
                    // text arrived; never loop forever on a flapping route.
                } else {
                    truncation_continuations += 1;
                    let _ = tx.send(AgentEvent::Status(format!(
                        "stream ended early (finish_reason: {reason}) — asking the model to continue… ({}/{MAX_TRUNCATION_CONTINUATIONS})",
                        truncation_continuations
                    )));
                    let nudge = if text.trim().is_empty() {
                        "[harness] The stream ended before you produced anything usable \
                         (transport error, not a real stop). Retry your last step now."
                    } else {
                        "[harness] The stream was cut off by a transport error right after the \
                         partial answer above (finish_reason marked it unreliable — this was NOT \
                         a natural stop). Continue exactly where you left off. Do not repeat or \
                         re-preamble; finish the response."
                    };
                    session
                        .input_items
                        .push(crate::api::types::user_text_item(nudge));
                    self.persist_session(session);
                    continue;
                }
            }
            if !truncated_this_round {
                truncation_continuations = 0;
                truncation_giving_up = false;
            }

            if text_deltas == 0 && !text.is_empty() && !suppress_duplicate_preamble {
                let _ = tx.send(AgentEvent::AssistantMessage(text.clone()));
            } else if suppress_duplicate_preamble {
                let _ = tx.send(AgentEvent::Status(
                    "suppressed duplicate tool rationale in display; durable request transcript retained".into(),
                ));
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

                // Output guardrails - warn (do not hard-block final answers).
                if let super::guardrails::GuardDecision::Warn(w) =
                    super::guardrails::check_output(&text)
                {
                    let _ = tx.send(AgentEvent::Status(format!("guardrail · {w}")));
                }
                // Attribute tokens toward persistent goal if any.
                let spent = usage.last_usage().total_tokens;
                if spent > 0 {
                    let _ = super::goal::add_tokens(&session.id, spent);
                }
                // M2 light extraction + Connectome chronicle (agent-native memory).
                if self.config.native_memory && !self.is_subagent && !text.trim().is_empty() {
                    let mem_scope = {
                        let proj = std::path::Path::new(&session.cwd)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("workspace");
                        format!("{proj}:{}", session.id)
                    };
                    let _ = super::chronicle::append(
                        &mem_scope,
                        "assistant",
                        &text.chars().take(500).collect::<String>(),
                        None,
                    );
                    for cand in super::native_memory::extract_candidates(&text) {
                        let _ = super::native_memory::remember(
                            &mem_scope,
                            &cand,
                            super::native_memory::Tier::Recent,
                            super::native_memory::Voice::Observed,
                            &["auto".into()],
                            0.5,
                            "turn_end",
                        );
                    }
                    // Model-assisted extraction (Mem0-class, paper M2) - opt-in.
                    if self.config.memory_model_extract {
                        let prompt = super::native_memory::model_extract_prompt(&text, 8_000);
                        let extract_input_estimate = (prompt.chars().count() as u64).div_ceil(3);
                        if let Some(msg) = preflight_request_budget(
                            &self.config,
                            usage,
                            &self.config.provider,
                            &self.config.model,
                            extract_input_estimate,
                            self.config.request_output_reserve_tokens,
                        ) {
                            let _ = tx.send(AgentEvent::Status(format!(
                                "memory extraction skipped - {msg}"
                            )));
                        } else {
                            let req = crate::api::types::ResponseRequest {
                                model: self.config.model.clone(),
                                input: serde_json::Value::Array(vec![
                                    crate::api::types::user_text_item(&prompt),
                                ]),
                                instructions: None,
                                tools: None,
                                tool_choice: None,
                                store: Some(false),
                                include: None,
                                reasoning: Some(crate::api::types::ReasoningConfig {
                                    effort: Some("minimal".into()),
                                    summary: None,
                                }),
                                stream: Some(false),
                                parallel_tool_calls: None,
                                prompt_cache_key: Some(format!(
                                    "native-mem-extract:{}",
                                    session.id
                                )),
                                max_output_tokens: (self.config.request_output_reserve_tokens > 0)
                                    .then_some(self.config.request_output_reserve_tokens),
                            };
                            let resp = self.client.create_response(&req).await;
                            if let Ok(resp) = resp {
                                let raw = resp.accounting_usage();
                                usage.record_request_for_route(
                                    &self.config.provider,
                                    &self.config.model,
                                    raw,
                                    resp.id.clone(),
                                );
                                let metered = usage.last_usage().clone();
                                session.usage.add(&metered);
                                let _ = tx.send(AgentEvent::Usage {
                                    session: usage.session_usage().clone(),
                                    last: metered,
                                });
                                let _ = tx.send(AgentEvent::Status(
                                    "memory extraction · metered auxiliary model request".into(),
                                ));
                                let output = resp.output_text();
                                for (fact, voice) in
                                    super::native_memory::parse_model_extraction(&output)
                                {
                                    let _ = super::native_memory::remember(
                                        &mem_scope,
                                        &fact,
                                        super::native_memory::Tier::L1,
                                        voice,
                                        &["model".into()],
                                        0.7,
                                        "turn_end_model",
                                    );
                                }
                            }
                        }
                    }
                }

                usage.set_state("idle");
                session.push_assistant(&text);
                // Settle back to idle so peers see this session as idle (not wedged).
                if !self.is_subagent {
                    let sid =
                        std::env::var("NUR_SESSION_ID").unwrap_or_else(|_| session.id.clone());
                    let cwd = self.cwd.to_string_lossy().into_owned();
                    crate::agent::mailbox::heartbeat(
                        &cwd,
                        &sid,
                        Some(crate::agent::mailbox::PeerState::Idle),
                    );
                }
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
            let exec_result = self
                .execute_calls(&calls, &mut tool_seq, session, usage, tx, cancel)
                .await;
            // Tool runs shell out to child processes (omp, egaki, graphjin
            // serve, …) that rewrite the terminal title with their own branding
            // (omp sets it to its π mark). Put our provider-branded title back
            // after every batch so the tab always reflects who is serving.
            crate::ade::reassert_after_child(&session_window_prompt(session));
            match exec_result {
                Ok(()) => {
                    self.persist_session(session);
                }
                Err(e) => {
                    let filled =
                        pair_unanswered(&mut session.input_items, &calls, &abort_output(&e));
                    if filled > 0 && !matches!(e, NurError::Interrupted) {
                        let _ = tx.send(AgentEvent::Status(format!(
                            "history · {filled} tool call(s) closed out after: {e}"
                        )));
                    }
                    self.persist_session(session);
                    return Err(e);
                }
            }
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
                return Err(NurError::Interrupted);
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
                for (handle, (id, call_id, name)) in handles.into_iter().zip(meta) {
                    let joined = tokio::select! {
                        // The caller's guard fills this call, the rest of the
                        // batch, and every post-batch call.
                        // Note: other in-flight blocking tasks keep running until drop
                        _ = cancel.cancelled() => return Err(NurError::Interrupted),
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
                    let hr = self.config.headroom.clone();
                    let sid = session.id.clone();
                    let tname = name.clone();
                    let model = self.config.model.clone();
                    let spill_max = self.config.tool_result_max_chars as usize;
                    let body = tokio::task::spawn_blocking(move || {
                        crate::headroom::prepare_tool_body(
                            &hr, &sid, &tname, body, ok, spill_max, &model,
                        )
                    })
                    .await
                    .unwrap_or_else(|e| format!("error: headroom task failed: {e}"));
                    record_auxiliary_telemetry(&session.id);
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
            let approved = self
                .check_approval(&call.name, &call.arguments, tx, cancel)
                .await;
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
                        Err(NurError::Interrupted) => return Err(NurError::Interrupted),
                        Err(e) => (format!("error: {e}"), false),
                    }
                }
            } else {
                // Portable tool-arg guardrails (OpenAI Agents SDK pattern) before hooks.
                match super::guardrails::check_tool_args(&call.name, &call.arguments) {
                    super::guardrails::GuardDecision::Block(msg) => {
                        let msg = format!("error: {msg}");
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
                    super::guardrails::GuardDecision::Warn(w) => {
                        let _ = tx.send(AgentEvent::Status(format!("guardrail · {w}")));
                    }
                    super::guardrails::GuardDecision::Allow => {}
                }
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
                // If the tool fails, drop the checkpoint so `/undo` does not
                // revert an edit that never landed.
                let mut recorded_undo = false;
                if matches!(
                    call.name.as_str(),
                    "write_file" | "edit_file" | "multi_edit" | "apply_patch"
                ) {
                    if let Ok(v) = serde_json::from_str::<Value>(&call.arguments) {
                        if let Some(p) = v.get("path").and_then(|p| p.as_str()) {
                            if let Ok(abs) = crate::tools::resolve_path(&self.cwd, p) {
                                crate::tools::undo::record(&session.id, &abs);
                                recorded_undo = true;
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
                let (body, ok) = tokio::select! {
                    _ = cancel.cancelled() => return Err(NurError::Interrupted),
                    r = exec => match r {
                        Ok(Ok(s)) => (s, true),
                        Ok(Err(e)) => (format!("error: {e}"), false),
                        Err(e) => (format!("error: tool panicked: {e}"), false),
                    },
                };
                if recorded_undo && !ok {
                    let _ = crate::tools::undo::drop_last(&session.id);
                }
                (body, ok)
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
                let hr = self.config.headroom.clone();
                let sid = session.id.clone();
                let tname = call.name.clone();
                let model = self.config.model.clone();
                let spill_max = self.config.tool_result_max_chars as usize;
                tokio::task::spawn_blocking(move || {
                    crate::headroom::prepare_tool_body(
                        &hr, &sid, &tname, body, true, spill_max, &model,
                    )
                })
                .await
                .unwrap_or_else(|e| format!("error: headroom task failed: {e}"))
            } else {
                // Keep error messages intact (usually short).
                body
            };
            record_auxiliary_telemetry(&session.id);
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
            // OMP prewalk: after todos exist, first successful write/edit hands
            // off to the cheap/smol model for the rest of the session.
            // `prewalk_override` on self is enough — next turn's model pick
            // reads it (same effect as OMP's recoveredDefaultModel).
            if ok {
                if let Some(into) = maybe_fire_prewalk(self, &call.name) {
                    let _ = tx.send(AgentEvent::Status(format!(
                        "prewalk · todos ready · first `{name}` → switching to `{into}` \
                         (OMP-style; /prewalk off to disable)",
                        name = call.name
                    )));
                }
            }
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
            let denial = if self
                .check_approval(&call.name, &call.arguments, tx, cancel)
                .await
            {
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
            // RLM recursion depth: children may recurse up to the config budget.
            let depth = self.subagent_depth;
            handles.push(Some(tokio::spawn(async move {
                let (prompt, kind, desc, provider_override, model_override) = parsed?;
                let cfg_limit = config.subagent_depth.max(1);
                if depth >= cfg_limit {
                    return Err(NurError::Other(format!(
                        "subagent recursion limit reached (depth {depth}, max {cfg_limit}) - \
                         raise config.subagent_depth to allow deeper RLM-style recursion"
                    )));
                }
                // Held for the whole child run: this is the concurrency cap.
                let _permit = tokio::select! {
                    _ = cancel_child.cancelled() => return Err(NurError::Interrupted),
                    permit = permits.acquire() => {
                        permit.map_err(|e| NurError::Other(e.to_string()))?
                    }
                };
                if cancel_child.is_cancelled() {
                    return Err(NurError::Interrupted);
                }
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
                            *child_config,
                            cwd,
                            mode,
                            &prompt,
                            &kind,
                            depth,
                            &cancel_child,
                            &tx_child,
                        )
                        .await
                    }
                    SubagentTarget::AwaitingLogin { message, .. }
                    | SubagentTarget::Unavailable { message } => Err(NurError::Other(message)),
                }
            })));
        }

        // Phase 3 — collect in submission order so `call_id` pairing holds.
        for index in 0..batch.len() {
            let call = &batch[index];
            let (id, denial) = &gated[index];
            let id = *id;
            let denial = denial.clone();
            let (body, ok) = match (denial, handles[index].take()) {
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
                (None, Some(mut handle)) => {
                    let joined = tokio::select! {
                        _ = cancel.cancelled() => {
                            handle.abort();
                            abort_subagent_handles(&mut handles);
                            return Err(NurError::Interrupted);
                        },
                        r = &mut handle => r,
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
                        Ok(Err(NurError::Interrupted)) => {
                            abort_subagent_handles(&mut handles);
                            return Err(NurError::Interrupted);
                        }
                        Ok(Err(e)) => (format!("error: {e}"), false),
                        Err(e) => (format!("error: subagent task failed: {e}"), false),
                    }
                }
                (None, None) => ("error: subagent was never started".to_string(), false),
            };
            let hr = self.config.headroom.clone();
            let sid = session.id.clone();
            let tname = call.name.clone();
            let model = self.config.model.clone();
            let spill_max = self.config.tool_result_max_chars as usize;
            let body = tokio::task::spawn_blocking(move || {
                crate::headroom::prepare_tool_body(&hr, &sid, &tname, body, ok, spill_max, &model)
            })
            .await
            .unwrap_or_else(|e| format!("error: headroom task failed: {e}"));
            record_auxiliary_telemetry(&session.id);
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
        cancel: &CancellationToken,
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
                return self.prompt_approval(name, args, tx, cancel).await;
            }
            return true;
        }

        // 3) Allow rule skips approval (manual) / short-circuits auto.
        if self.permissions.decide(name, args) == Some(RuleDecision::Allow) {
            return true;
        }

        // 4) Ask rule forces a prompt even in auto.
        if self.permissions.decide(name, args) == Some(RuleDecision::Ask) {
            return self.prompt_approval(name, args, tx, cancel).await;
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
                self.prompt_approval(name, args, tx, cancel).await
            }
        }
    }

    async fn prompt_approval(
        &self,
        name: &str,
        args: &str,
        tx: &mpsc::UnboundedSender<AgentEvent>,
        cancel: &CancellationToken,
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
        let decision = tokio::select! {
            _ = cancel.cancelled() => return false,
            decision = orx => decision,
        };
        match decision {
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
    if name == "terminal_browser" && crate::tools::terminal_browser::is_plan_safe_action(args) {
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

    #[test]
    fn provider_turn_timeout_is_bounded_and_configurable() {
        assert_eq!(provider_turn_timeout_from(None).as_secs(), 300);
        assert_eq!(provider_turn_timeout_from(Some("17")).as_secs(), 17);
        assert_eq!(provider_turn_timeout_from(Some("0")).as_secs(), 300);
        assert_eq!(provider_turn_timeout_from(Some("invalid")).as_secs(), 300);
    }

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
    fn close_out(items: &mut Vec<Value>, calls: &[FunctionCallRef], err: &NurError) -> usize {
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
            close_out(&mut items, &calls, &NurError::Interrupted),
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
        let err = NurError::Other("tool task panicked".into());
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
        assert_eq!(close_out(&mut items, &calls, &NurError::Interrupted), 1);
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
        let (_, kind, desc, _prov, _model) =
            parse_agent_call(&agent_call("b", "look around", "explore")).unwrap();
        assert_eq!((kind.as_str(), desc.as_str()), ("explore", "explore"));

        // A missing prompt is a tool error, not a spawned no-op subagent.
        assert!(parse_agent_call(&call("c", "agent")).is_err());
    }

    #[test]
    fn agent_calls_reject_unbounded_prompts_and_unknown_privilege_classes() {
        let oversized = FunctionCallRef {
            call_id: "large".into(),
            name: "agent".into(),
            arguments: serde_json::json!({
                "prompt": "x".repeat(MAX_SUBAGENT_PROMPT_CHARS + 1),
            })
            .to_string(),
        };
        assert!(parse_agent_call(&oversized)
            .unwrap_err()
            .to_string()
            .contains("keep delegated context"));

        let unknown = FunctionCallRef {
            call_id: "kind".into(),
            name: "agent".into(),
            arguments: r#"{"prompt":"audit","subagent_type":"unrestricted"}"#.into(),
        };
        assert!(parse_agent_call(&unknown)
            .unwrap_err()
            .to_string()
            .contains("unsupported subagent_type"));
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
        // A bare provider label with NO explicit spawn ask ("claude review")
        // is NOT routing - it names the subject. The child inherits the parent.
        let (_, _, _, prov, _) = parse_agent_call(&FunctionCallRef {
            call_id: "a".into(),
            name: "agent".into(),
            arguments: r#"{"prompt":"review the auth module","description":"claude review","subagent_type":"general"}"#.into(),
        })
        .unwrap();
        assert!(
            prov.is_none(),
            "bare description label must not route without an explicit spawn ask: {prov:?}"
        );

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
            arguments:
                r#"{"prompt":"compare notes","description":"claude vs grok","provider":"xai"}"#
                    .into(),
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
    fn topical_research_descriptions_never_route_cross_provider() {
        // These are EXACTLY the cases that bit us: research/naming tasks whose
        // description or prompt merely *contains* a provider word. They describe
        // the subject, not a backend, and must yield NO provider override (the
        // child then inherits the parent client verbatim).
        for (desc, prompt) in [
            // "x" is a single-char xAI alias — must NOT fire from prose.
            ("Resolve nicopreme X post content", "resolve the X post"),
            // "claude" mid-description is the subject, not a backend.
            (
                "Research Claude cross-session messaging",
                "fetch the docs and summarize",
            ),
            // "grok" in the description of a research task on grok.
            (
                "Research cross-provider subagent grok setup",
                "report how it works",
            ),
            // Leading provider name in the prompt is a subject, not routing.
            (
                "audit",
                "Claude Code session import path - map how it works",
            ),
        ] {
            let (_, _, _, prov, _) = parse_agent_call(&FunctionCallRef {
                call_id: "a".into(),
                name: "agent".into(),
                arguments: serde_json::json!({ "prompt": prompt, "description": desc }).to_string(),
            })
            .unwrap();
            assert!(
                prov.is_none(),
                "topical prose must NOT route to a provider: desc={desc:?} prompt={prompt:?} → {prov:?}"
            );
        }

        // Explicit asks still route (provider:kv, and verb-cued routing phrases).
        let (_, _, _, prov_a, _) = parse_agent_call(&FunctionCallRef {
            call_id: "b".into(),
            name: "agent".into(),
            arguments: r#"{"prompt":"x","description":"audit","provider":"grok"}"#.into(),
        })
        .unwrap();
        // parse_agent_call returns the RAW provider field ("grok"); alias
        // resolution (grok → xai) happens later in resolve_subagent_target.
        assert_eq!(prov_a.as_deref(), Some("grok"));

        let (_, _, _, prov_b, _) = parse_agent_call(&FunctionCallRef {
            call_id: "c".into(),
            name: "agent".into(),
            arguments: r#"{"prompt":"run this review on claude","description":"review the auth"}"#
                .into(),
        })
        .unwrap();
        assert_eq!(prov_b.as_deref(), Some("anthropic"));
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
            // Regression: topical "X agent …" prose used to match the bare
            // "agent" tail cue and force a cross-provider spawn onto OpenAI
            // with a stale sk- key (401 Incorrect API key).
            "OpenAI agent strategies — research official docs for portable patterns",
            "Study the Claude agent architecture and summarize it",
            "Compare Gemini agent frameworks with our loop",
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

        // A leading provider name in the DESCRIPTION is ALSO not routing on its
        // own — the provider must be tied to an explicit spawn ask ("spawn a
        // grok agent", "run via claude"). A bare "grok audit" label describes
        // the subject, not a backend.
        let (_, _, _, prov, _) = parse_agent_call(&FunctionCallRef {
            call_id: "b".into(),
            name: "agent".into(),
            arguments: r#"{"prompt":"audit the failover path","description":"grok audit"}"#.into(),
        })
        .unwrap();
        assert!(
            prov.is_none(),
            "bare description label must not route without an explicit spawn ask: {prov:?}"
        );

        // "<provider> subagent" with no spawn verb/preposition is still a bare
        // mention, not an explicit ask — route only on a real cue.
        let (_, _, _, prov, _) = parse_agent_call(&FunctionCallRef {
            call_id: "c".into(),
            name: "agent".into(),
            arguments:
                r#"{"prompt":"Claude subagent: review auth for races","description":"review"}"#
                    .into(),
        })
        .unwrap();
        assert!(
            prov.is_none(),
            "'<provider> subagent' alone must not route: {prov:?}"
        );

        // Explicit spawn-verb asks really route.
        for (prompt, expected) in [
            ("spawn a grok agent to audit failover", "xai"),
            ("run a claude agent for the auth review", "anthropic"),
            (
                "deploy a gemini reviewer to map the graphify module",
                "google",
            ),
        ] {
            let (_, _, _, prov, _) = parse_agent_call(&FunctionCallRef {
                call_id: "d".into(),
                name: "agent".into(),
                arguments: serde_json::json!({ "prompt": prompt, "description": "task" })
                    .to_string(),
            })
            .unwrap();
            assert_eq!(
                prov.as_deref(),
                Some(expected),
                "explicit spawn-verb ask must route: {prompt:?}"
            );
        }
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
        let parent_config = Config {
            provider: "antigravity".into(),
            model: "gemini-2.5-pro".into(),
            ..Default::default()
        };

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
            SubagentTarget::Unavailable { message } => panic!("{message}"),
        }

        // The reverse direction (parent already on `google`, request "antigravity")
        // must also short-circuit.
        let parent_config_google = Config {
            provider: "google".into(),
            ..Default::default()
        };
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
            SubagentTarget::Unavailable { message } => panic!("{message}"),
        }
    }

    #[test]
    fn natural_language_provider_names_resolve() {
        assert_eq!(resolve_provider_alias("grok").map(|p| p.id), Some("xai"));
        assert_eq!(
            resolve_provider_alias("gemini").map(|p| p.id),
            Some("google")
        );
        assert_eq!(
            resolve_provider_alias("claude").map(|p| p.id),
            Some("anthropic")
        );
        assert_eq!(
            resolve_provider_alias("chatgpt").map(|p| p.id),
            Some("openai")
        );
        assert_eq!(
            resolve_provider_alias("deepseek").map(|p| p.id),
            Some("deepseek")
        );
        // Direct id passes through.
        assert_eq!(
            resolve_provider_alias("anthropic").map(|p| p.id),
            Some("anthropic")
        );
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
        assert_eq!(
            resolve_provider_alias("use grok").map(|p| p.id),
            Some("xai")
        );
        assert_eq!(
            resolve_provider_alias("the gemini provider").map(|p| p.id),
            Some("google")
        );
        // Model-family nicknames route to the serving provider.
        assert_eq!(
            resolve_provider_alias("sonnet").map(|p| p.id),
            Some("anthropic")
        );
        assert_eq!(
            resolve_provider_alias("opus").map(|p| p.id),
            Some("anthropic")
        );
        assert_eq!(
            resolve_provider_alias("flash").map(|p| p.id),
            Some("google")
        );
        assert_eq!(resolve_provider_alias("gpt").map(|p| p.id), Some("openai"));
        // Distinct kimi vs moonshot catalog ids.
        assert_eq!(resolve_provider_alias("kimi").map(|p| p.id), Some("kimi"));
        assert_eq!(
            resolve_provider_alias("moonshot").map(|p| p.id),
            Some("moonshot")
        );
        // "meta" must not over-match every display name via naive substring.
        assert_eq!(resolve_provider_alias("meta").map(|p| p.id), Some("meta"));
        // Unknown name -> None (explicit routing fails closed).
        assert!(resolve_provider_alias("nonesuch-xyz").is_none());
    }

    #[test]
    fn explicit_unknown_provider_never_impersonates_with_parent_client() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let parent_client = ApiClient::new("https://example.invalid", "parent-key").unwrap();
        let target = resolve_subagent_target(
            &parent_client,
            &Config::default(),
            Some("definitely-not-a-provider"),
            None,
            None,
            None,
            None,
            &tx,
        );
        match target {
            SubagentTarget::Unavailable { message } => {
                assert!(message.contains("unknown provider"));
            }
            _ => panic!("unknown explicit provider must fail closed"),
        }
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
        let u = TokenUsage {
            input_tokens: 50_000,
            total_tokens: 50_000,
            ..Default::default()
        };
        usage.seed_session(u.clone());
        assert!(session_budget_exceeded(&cfg, &usage).is_some());
        cfg.max_session_cost_usd = None;
        cfg.max_session_tokens = Some(10_000);
        assert!(session_budget_exceeded(&cfg, &usage).is_some());
        cfg.max_session_tokens = Some(1_000_000);
        assert!(session_budget_exceeded(&cfg, &usage).is_none());
    }

    #[test]
    fn request_preflight_reserves_output_before_dispatch() {
        let mut cfg = Config {
            max_session_tokens: Some(10_000),
            ..Default::default()
        };
        let usage = UsageTracker::new("preflight".into(), "m".into(), PathBuf::from("."));
        let err = preflight_request_budget(&cfg, &usage, "meta", "m", 3_000, 8_192)
            .expect("input plus reserved completion must be checked before dispatch");
        assert!(err.contains("reserved output"));

        cfg.max_session_tokens = Some(20_000);
        assert!(preflight_request_budget(&cfg, &usage, "meta", "m", 3_000, 8_192).is_none());
    }

    #[test]
    fn duplicate_tool_preamble_only_matches_short_equivalent_text() {
        let prior = vec![serde_json::json!({
            "type":"message",
            "content":[{"type":"output_text","text":"I will inspect the config first."}]
        })];
        assert!(is_duplicate_tool_preamble(
            &prior,
            "I will inspect the config first!"
        ));
        assert!(!is_duplicate_tool_preamble(
            &prior,
            "A genuinely different rationale."
        ));
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
/// The prompt fragment the TUI uses in its window title — the last user
/// message, so title re-asserts keep showing what this session is about.
fn session_window_prompt(session: &Session) -> String {
    session
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.clone())
        .unwrap_or_else(|| "ready".to_string())
}

fn abort_output(err: &NurError) -> String {
    match err {
        NurError::Interrupted => INTERRUPT_OUTPUT.to_string(),
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

/// Guard a request before it leaves the machine. Providers often report usage
/// only after completion (or not at all), so checking the already-spent total
/// alone leaves one unbounded completion able to cross the user budget.
pub fn preflight_request_budget(
    cfg: &Config,
    usage: &UsageTracker,
    provider: &str,
    model: &str,
    estimated_input_tokens: u64,
    reserved_output_tokens: u64,
) -> Option<String> {
    let requested = estimated_input_tokens.saturating_add(reserved_output_tokens);
    let context_window =
        crate::pricing::context_window_for(provider, model).unwrap_or(cfg.context_window);
    if requested > context_window {
        return Some(format!(
            "request preflight blocked: estimated input {estimated_input_tokens} + reserved output {reserved_output_tokens} = {requested} tokens exceeds {provider}/{model} context window {context_window}"
        ));
    }
    let session = usage.session_usage();
    if let Some(max) = cfg.max_session_tokens {
        let projected = session.total_tokens.saturating_add(requested);
        if projected > max {
            return Some(format!(
                "request preflight blocked: session tokens {} + estimated input {estimated_input_tokens} + reserved output {reserved_output_tokens} = {projected} would exceed budget {max}",
                session.total_tokens
            ));
        }
    }
    if let Some(max) = cfg.max_session_cost_usd {
        let reserve = TokenUsage {
            input_tokens: estimated_input_tokens,
            output_tokens: reserved_output_tokens,
            total_tokens: requested,
            ..Default::default()
        };
        let projected = session.estimated_cost_usd()
            + crate::pricing::rates_for(provider, model).cost_for(&reserve);
        if projected > max {
            return Some(format!(
                "request preflight blocked: estimated session cost ${projected:.4} (including this request's ${:.4} reserve) would exceed budget ${max:.4}",
                crate::pricing::rates_for(provider, model).cost_for(&reserve)
            ));
        }
    }
    None
}

/// A display-only suppression predicate for repeated model tool preambles.
/// The original response remains in `input_items` (including every function
/// call and id), so no provider transcript or tool pairing is altered.
fn is_duplicate_tool_preamble(prior_items: &[Value], text: &str) -> bool {
    let normalized = normalize_tool_preamble(text);
    if normalized.is_empty() || normalized.len() > 240 {
        return false;
    }
    prior_items.iter().rev().any(|item| {
        item.get("type").and_then(Value::as_str) == Some("message")
            && item
                .pointer("/content/0/text")
                .and_then(Value::as_str)
                .map(normalize_tool_preamble)
                .as_deref()
                == Some(normalized.as_str())
    })
}

fn normalize_tool_preamble(text: &str) -> String {
    text.to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || c.is_ascii_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
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
/// Reactive recoveries after a provider rejects the request as too large.
/// This is separate from proactive compaction because usage metadata can be
/// absent or inaccurate on gateways and OAuth-backed compatibility routes.
const MAX_EMERGENCY_COMPACTIONS: u8 = 2;

fn is_context_limit_error(error: &NurError) -> bool {
    let message = match error {
        NurError::Api { message, .. } | NurError::Other(message) => message,
        _ => return false,
    }
    .to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "context length",
        "context window",
        "maximum context",
        "max context",
        "input is too long",
        "input too long",
        "prompt is too long",
        "prompt too long",
        "too many input tokens",
        "input token count exceeds",
        "request too large",
        "request_too_large",
        "tokens exceed",
    ];
    NEEDLES.iter().any(|needle| message.contains(needle))
}

fn is_model_unavailable_error(error: &NurError) -> bool {
    let message = match error {
        NurError::Api { message, .. } | NurError::Other(message) => message,
        _ => return false,
    }
    .to_ascii_lowercase();
    let names_model = message.contains("model") || message.contains("deployment");
    names_model
        && [
            "not found",
            "does not exist",
            "not available",
            "unsupported model",
            "unknown model",
            "retired",
            "deprecated",
        ]
        .iter()
        .any(|needle| message.contains(needle))
}

fn is_catalog_default_model(config: &Config) -> bool {
    crate::providers::by_id(&config.provider)
        .is_some_and(|provider| provider.default_model == config.model)
}

fn pick_replacement_model(models: &[String], unavailable: &str) -> Option<String> {
    const NON_CHAT: &[&str] = &[
        "embedding",
        "moderation",
        "realtime",
        "transcribe",
        "whisper",
        "tts",
        "image",
        "video",
        "audio",
        "veo",
        "imagen",
    ];
    const ROLE_HINTS: &[&str] = &[
        "sonnet",
        "opus",
        "haiku",
        "pro",
        "flash",
        "mini",
        "nano",
        "terra",
        "sol",
        "luna",
        "reasoning",
    ];
    let unavailable_lower = unavailable.to_ascii_lowercase();
    let family = unavailable_lower
        .split(['-', '/', ':'])
        .next()
        .unwrap_or_default();
    let wanted_roles: Vec<&str> = ROLE_HINTS
        .iter()
        .copied()
        .filter(|hint| unavailable_lower.contains(hint))
        .collect();

    let mut candidates: Vec<&String> = models
        .iter()
        .filter(|model| !model.eq_ignore_ascii_case(unavailable))
        .filter(|model| {
            let lower = model.to_ascii_lowercase();
            !NON_CHAT.iter().any(|needle| lower.contains(needle))
                && (family.is_empty() || lower.contains(family))
        })
        .collect();
    candidates.sort_by(|left, right| {
        let left_lower = left.to_ascii_lowercase();
        let right_lower = right.to_ascii_lowercase();
        let left_role = wanted_roles
            .iter()
            .filter(|hint| left_lower.contains(**hint))
            .count();
        let right_role = wanted_roles
            .iter()
            .filter(|hint| right_lower.contains(**hint))
            .count();
        right_role.cmp(&left_role).then_with(|| right.cmp(left))
    });
    candidates.first().map(|model| (*model).clone())
}

const DEFAULT_COMPACTION_RESERVE_TOKENS: u64 = 16_384;

/// Match OMP's reserve-based default instead of compacting at a fixed 55%.
/// Large windows keep 15% free; smaller windows keep the 16k response/tool
/// reserve unless that would leave no practical prompt budget.
fn compaction_threshold_tokens(context_window: u64) -> u64 {
    let window = context_window.max(1);
    let proportional = ((window as f64 * 0.15).floor() as u64).max(1);
    let requested = proportional.max(DEFAULT_COMPACTION_RESERVE_TOKENS);
    // OMP recovers to the proportional reserve when its default would make the
    // budget effectively impossible. Nur also applies that recovery when the
    // fixed reserve would consume more than half of a small provider window;
    // otherwise a 20k model would compact at only 3.6k tokens.
    let reserve = if requested >= window.saturating_sub(proportional) || requested >= window / 2 {
        proportional
    } else {
        requested
    };
    window.saturating_sub(reserve).min(window.saturating_sub(1))
}

/// Conservative local context estimate used only as a floor when provider
/// usage is absent or under-reported. Inline media payloads are represented by
/// a bounded image allowance rather than charging their base64 byte length as
/// text tokens.
fn estimate_context_tokens(items: &[Value]) -> u64 {
    fn chars(value: &Value, key: Option<&str>) -> u64 {
        match value {
            Value::Null => 4,
            Value::Bool(_) => 5,
            Value::Number(number) => number.to_string().len() as u64,
            Value::String(text)
                if matches!(key, Some("image_url" | "image" | "data"))
                    && text.starts_with("data:") =>
            {
                4_096
            }
            Value::String(text) => text.chars().count() as u64,
            Value::Array(values) => values
                .iter()
                .map(|value| chars(value, None).saturating_add(1))
                .sum(),
            Value::Object(values) => values
                .iter()
                .map(|(key, value)| {
                    (key.len() as u64)
                        .saturating_add(chars(value, Some(key)))
                        .saturating_add(2)
                })
                .sum(),
        }
    }

    let estimated_chars: u64 = items.iter().map(|item| chars(item, None)).sum();
    estimated_chars.saturating_add(3) / 4
}

fn should_auto_compact(usage: &UsageTracker, cfg: &Config, items: &[Value]) -> bool {
    let last = usage.last_usage();
    let provider_used = if last.input_tokens > 0 {
        last.input_tokens
    } else {
        last.total_tokens
    };
    let window = cfg.context_window.max(1);
    let used = provider_used.max(estimate_context_tokens(items));
    used > compaction_threshold_tokens(window)
}

/// Replace stale bodies from repeated identical observations while preserving
/// the function-call/result pair required by strict provider protocols.
///
/// This mirrors OMP's `supersedeReads`: a second read/grep of the same target is
/// the current truth, so carrying the earlier full body forward only wastes
/// context. Mutating and delegated tools are deliberately never touched.
/// Collapse empty / hard-error tool bodies that sit outside the live suffix.
/// Mirrors OMP `compaction.dropUseless` without rewriting recent cacheable turns.
fn prune_useless_observations(items: &mut [Value]) -> usize {
    const CACHE_AWARE_SUFFIX_CHARS: usize = 32_000;
    const NOTICE: &str = "[uneventful empty tool result elided to save context]";
    let item_chars: Vec<usize> = items
        .iter()
        .map(|item| serde_json::to_string(item).map_or(0, |text| text.len()))
        .collect();
    let mut suffix_chars = vec![0usize; items.len()];
    let mut running = 0usize;
    for index in (0..items.len()).rev() {
        suffix_chars[index] = running;
        running = running.saturating_add(item_chars[index]);
    }
    let mut pruned = 0;
    for index in 0..items.len() {
        // A prefix cache survives mutations near the live suffix. Rewriting
        // older entries would invalidate far more cached context than this
        // tiny optimization can save.
        if suffix_chars[index] > CACHE_AWARE_SUFFIX_CHARS {
            continue;
        }
        let item = &mut items[index];
        if item.get("type").and_then(Value::as_str) != Some("function_call_output") {
            continue;
        }
        let Some(output) = item.get_mut("output") else {
            continue;
        };
        let Some(body) = output.as_str() else {
            continue;
        };
        if body == NOTICE {
            continue;
        }
        let trimmed = body.trim();
        // Nur's current Tool result type has no explicit `useless` bit. Never
        // infer uselessness from error text: OMP's invariant is that errors
        // always win and remain available for recovery. Only successful empty
        // payload shapes qualify, and only when replacing them actually saves.
        let useless = trimmed.is_empty() || trimmed == "{}" || trimmed == "null";
        if !useless {
            continue;
        }
        if body.chars().count() <= NOTICE.chars().count() {
            continue;
        }
        *output = Value::String(NOTICE.into());
        pruned += 1;
    }
    pruned
}

/// Prefer a larger-context sibling before compaction (OMP `contextPromotion`).
///
/// Heuristics when the live catalog has no window sizes: drop `-spark` /
/// size-shrink suffixes (`mini`, `nano`, `flash`, `haiku`, `luna`) within the
/// same family, preferring the longest remaining id match.
fn pick_context_promotion_target(models: &[String], current: &str) -> Option<String> {
    let current_lower = current.to_ascii_lowercase();
    let stem = current_lower
        .strip_suffix("-spark")
        .or_else(|| current_lower.strip_suffix("_spark"))
        .unwrap_or(&current_lower);
    let shrink = [
        "-mini", "-nano", "-flash", "-haiku", "-luna", "-lite", "-small",
    ];
    let mut targets = Vec::new();
    if stem != current_lower {
        targets.push(stem.to_string());
    }
    for suffix in shrink {
        if let Some(base) = stem.strip_suffix(suffix) {
            targets.push(base.to_string());
            // Also try common full-size siblings on the same provider prefix.
            if let Some((provider, rest)) = base.rsplit_once('/') {
                targets.push(format!("{provider}/{rest}"));
            }
        }
    }
    if targets.is_empty() {
        return None;
    }
    let mut best: Option<&String> = None;
    for model in models {
        let lower = model.to_ascii_lowercase();
        if lower == current_lower {
            continue;
        }
        // Never "promote" into an even smaller shrink tier.
        if shrink.iter().any(|s| lower.ends_with(s)) || lower.ends_with("-spark") {
            continue;
        }
        let hits = targets
            .iter()
            .any(|t| lower == *t || lower.starts_with(t) || t.starts_with(&lower));
        if !hits {
            // Family match: share first path segment + major version token.
            let cur_family = stem
                .split(['/', '-', ':'])
                .take(2)
                .collect::<Vec<_>>()
                .join("-");
            let cand_family = lower
                .split(['/', '-', ':'])
                .take(2)
                .collect::<Vec<_>>()
                .join("-");
            if cur_family.is_empty() || cur_family != cand_family {
                continue;
            }
        }
        best = match best {
            None => Some(model),
            Some(prev) if model.len() >= prev.len() => Some(model),
            Some(prev) => Some(prev),
        };
    }
    best.cloned()
}

fn prune_superseded_observations(items: &mut [Value]) -> usize {
    const OBSERVATION_TOOLS: &[&str] = &[
        "read_file",
        "list_dir",
        "grep",
        "glob",
        "git_status",
        "git_diff",
        "web_fetch",
        "web_search",
    ];
    const MIN_BODY_CHARS: usize = 512;

    let mut seen = HashSet::new();
    let mut superseded: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for item in items.iter().rev() {
        if item.get("type").and_then(Value::as_str) != Some("function_call") {
            continue;
        }
        let name = item.get("name").and_then(Value::as_str).unwrap_or_default();
        if !OBSERVATION_TOOLS.contains(&name) {
            continue;
        }
        let call_id = item
            .get("call_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let arguments = item
            .get("arguments")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if call_id.is_empty() {
            continue;
        }
        let key = format!("{name}\0{}", canonical_tool_arguments(arguments));
        if !seen.insert(key) {
            superseded.insert(call_id.to_string(), name.to_string());
        }
    }

    let mut pruned = 0;
    let item_chars: Vec<usize> = items
        .iter()
        .map(|item| serde_json::to_string(item).map_or(0, |text| text.len()))
        .collect();
    let mut suffix_chars = vec![0usize; items.len()];
    let mut running = 0usize;
    for index in (0..items.len()).rev() {
        suffix_chars[index] = running;
        running = running.saturating_add(item_chars[index]);
    }
    for index in 0..items.len() {
        // Rewriting deep history can discard a much larger provider prompt
        // cache than it saves. Match OMP's cache-aware policy: prune while the
        // changed entry is still near the live suffix (roughly <=8k tokens).
        if suffix_chars[index] > 32_000 {
            continue;
        }
        let item = &mut items[index];
        if item.get("type").and_then(Value::as_str) != Some("function_call_output") {
            continue;
        }
        let Some(call_id) = item.get("call_id").and_then(Value::as_str) else {
            continue;
        };
        let Some(name) = superseded.get(call_id) else {
            continue;
        };
        let Some(output) = item.get_mut("output") else {
            continue;
        };
        let Some(body) = output.as_str() else {
            continue;
        };
        if body.chars().count() < MIN_BODY_CHARS || body.starts_with("[superseded by newer") {
            continue;
        }
        *output = Value::String(format!(
            "[superseded by newer identical `{name}` call; stale body removed to save context]"
        ));
        pruned += 1;
    }
    pruned
}

/// Tool-call JSON object ordering is not semantic. Providers can reorder the
/// same arguments between rounds, so use a recursively sorted representation
/// when deciding whether a read/search observation supersedes an older one.
fn canonical_tool_arguments(arguments: &str) -> String {
    fn sort_value(value: Value) -> Value {
        match value {
            Value::Array(values) => Value::Array(values.into_iter().map(sort_value).collect()),
            Value::Object(values) => {
                let mut entries: Vec<_> = values.into_iter().collect();
                entries.sort_by(|left, right| left.0.cmp(&right.0));
                let mut sorted = serde_json::Map::new();
                for (key, value) in entries {
                    sorted.insert(key, sort_value(value));
                }
                Value::Object(sorted)
            }
            scalar => scalar,
        }
    }

    serde_json::from_str(arguments)
        .map(sort_value)
        .and_then(|value| serde_json::to_string(&value))
        .unwrap_or_else(|_| arguments.trim().to_string())
}

fn emit_side_effects(tx: &mpsc::UnboundedSender<AgentEvent>, name: &str, body: &str) {
    if name == "todo_write" {
        let _ = tx.send(AgentEvent::TodosChanged(body.to_string()));
    }
    if name == "submit_plan" {
        let _ = tx.send(AgentEvent::PlanSubmitted(body.to_string()));
    }
}

/// Drain auxiliary inference receipts created while a tool was running. These
/// calls are deliberately separate from the primary model ledger because their
/// provider, model, privacy boundary, and billing provenance can differ.
fn record_auxiliary_telemetry(session_id: &str) {
    for event in super::embed::take_embedding_telemetry() {
        receipt::record(
            session_id,
            receipt::Event::AuxiliaryInference {
                purpose: "embedding".into(),
                route: event.route,
                model: event.model,
                processing: event.processing,
                input_tokens: event
                    .input_tokens_reported
                    .unwrap_or(event.input_tokens_estimate),
                output_tokens: 0,
                cost_usd: event.cost_usd_estimate,
                cost_provenance: event.cost_provenance,
                outcome: event.outcome,
            },
        );
    }
    for event in crate::headroom::take_headroom_telemetry() {
        receipt::record(
            session_id,
            receipt::Event::AuxiliaryInference {
                purpose: "headroom compression".into(),
                route: event.backend,
                model: event.model,
                processing: event.processing,
                input_tokens: event
                    .input_tokens
                    .unwrap_or_else(|| (event.input_chars as u64).div_ceil(4)),
                output_tokens: event
                    .output_tokens
                    .unwrap_or_else(|| (event.output_chars as u64).div_ceil(4)),
                cost_usd: event.cost_usd,
                cost_provenance: event.cost_provenance,
                outcome: "compressed".into(),
            },
        );
    }
}

/// A spawned subagent run: its report text plus the tokens it spent.
type SubagentHandle = tokio::task::JoinHandle<Result<(String, TokenUsage)>>;

/// Dropping a Tokio join handle detaches the task. Fan-out cancellation must
/// abort every still-running child explicitly so no queued child can acquire a
/// permit later and begin resolving credentials or editing after the turn ended.
fn abort_subagent_handles(handles: &mut [Option<SubagentHandle>]) {
    for handle in handles.iter_mut().filter_map(Option::take) {
        handle.abort();
    }
}

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
pub const MAX_SUBAGENT_PROMPT_CHARS: usize = 20_000;

/// `{prompt, subagent_type, description, provider?, model?}` out of an `agent`
/// tool call. Provider/model are optional cross-provider overrides. When the
/// model forgets `provider` but names one in the description/prompt (common),
/// we recover it via [`infer_provider_from_agent_text`].
type ParsedAgentCall = (String, String, String, Option<String>, Option<String>);
fn parse_agent_call(call: &FunctionCallRef) -> Result<ParsedAgentCall> {
    let v: Value = serde_json::from_str(&call.arguments).unwrap_or(serde_json::json!({}));
    let mut prompt = v
        .get("prompt")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    if prompt.is_empty() {
        return Err(NurError::Tool("agent.prompt required".into()));
    }
    // OpenAI Agents SDK-style handoff packet fields (portable).
    let reason = v
        .get("reason")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let handoff_role = v
        .get("handoff_role")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if reason.is_some() || handoff_role.is_some() {
        let mut packet = String::from("\n\n[handoff packet]");
        if let Some(r) = handoff_role {
            packet.push_str(&format!("\nrole: {r}"));
        }
        if let Some(r) = reason {
            packet.push_str(&format!("\nreason: {r}"));
        }
        prompt.push_str(&packet);
    }
    // Handoff input filter (OpenAI Agents SDK context-filter port): a list of
    // workspace files the child should be given directly. Resolved here so the
    // packet stays usable by `run_agent_tool`; the loop prepends them.
    if let Some(files) = v.get("context_files").and_then(|x| x.as_array()) {
        let mut resolved = Vec::new();
        for f in files.iter().filter_map(|x| x.as_str()).take(16) {
            if !f.trim().is_empty() {
                resolved.push(f.trim().to_string());
            }
        }
        if !resolved.is_empty() {
            prompt.push_str(&format!("\ncontext_files: {}", resolved.join(", ")));
        }
    }
    let prompt_chars = prompt.chars().count();
    if prompt_chars > MAX_SUBAGENT_PROMPT_CHARS {
        return Err(NurError::Tool(format!(
            "agent.prompt is {prompt_chars} characters; keep delegated context under {MAX_SUBAGENT_PROMPT_CHARS}"
        )));
    }
    let kind = v
        .get("subagent_type")
        .and_then(|x| x.as_str())
        .unwrap_or("explore")
        .to_string();
    if !matches!(
        kind.as_str(),
        "explore" | "research" | "readonly" | "general"
    ) {
        return Err(NurError::Tool(format!(
            "unsupported subagent_type `{kind}`; use explore or general"
        )));
    }
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
fn infer_provider_from_agent_text(desc: &str, prompt: &str) -> Option<(String, Option<String>)> {
    // 1) Explicit key=value / key:value in either field ("provider:claude",
    //    "model:grok-4"). This is the unambiguously-explicit ask.
    for text in [desc, prompt] {
        if let Some(hit) = extract_explicit_provider_kv(text) {
            return Some(hit);
        }
    }
    // 2) Routing phrases with an explicit spawn ask — "spawn a claude subagent",
    //    "run on grok", "deploy through openai". A provider name alone, whether
    //    in the description label or the prompt subject, is NEVER routing: it
    //    usually names the thing being discussed, not a backend. Routing on a
    //    bare mention ("Resolve nicopreme X post content", "Research Claude
    //    cross-session messaging", "I love gpt 5.6 sol") throws spawns at a
    //    /login nobody asked for. Omit `provider` → always inherit the parent.
    if let Some(hit) = extract_provider_routing_phrase(desc) {
        return Some(hit);
    }
    extract_provider_routing_phrase(prompt)
}

/// `provider:claude`, `provider=xai`, `model:grok-4` pairs in free text.
fn extract_explicit_provider_kv(text: &str) -> Option<(String, Option<String>)> {
    let lower = text.to_ascii_lowercase();
    let mut found_provider: Option<String> = None;
    let mut found_model: Option<String> = None;
    for (key, out) in [
        ("provider", &mut found_provider),
        ("model", &mut found_model),
    ] {
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
    let prov =
        found_provider.and_then(|raw| resolve_provider_alias(&raw).map(|p| p.id.to_string()))?;
    // If model was set but looks like a provider alias, drop it — keep real ids.
    let model = found_model
        .filter(|m| resolve_provider_alias(m).is_none() || m.contains('-') || m.contains('/'));
    Some((prov, model))
}

/// Phrases like "spawn a claude subagent", "run via grok", "deploy through
/// openai", "subagent on xai".
///
/// STRICT RULE: a provider routes ONLY with an explicit routing/spawn ask —
/// a preposition cue immediately before it ("on/via/through/using/…") OR a
/// spawn-verb cue ("spawn", "deploy", "run") somewhere in the preceding window.
/// A bare mention of a provider ("I love gpt", "Research Claude cross-session
/// messaging") never routes. Omit `provider` → always inherit the parent.
fn extract_provider_routing_phrase(text: &str) -> Option<(String, Option<String>)> {
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
    // Preposition immediately before the provider ("subagent through openai").
    // These are the explicit route signals (the user's rule: "spawn a sol
    // subagent THROUGH openai"). No bare topic words here.
    const PREP_CUES: &[&str] = &["on", "using", "via", "with", "through", "onto"];
    // Spawn verbs that, anywhere in the window, mark it as an explicit ask
    // ("spawn a claude subagent", "deploy a grok subagent", "run a gemini
    // subagent"). Deliberately NO "subagent"/"agent" keywords here: those are
    // ordinary topic words ("Research cross-provider subagent grok setup") and
    // would route on mere mention. Only a real ask-verb routes.
    const SPAWN_CUES: &[&str] = &["spawn", "deploy", "run ", "invoke", "launch", "route"];

    for phrase in PHRASES {
        // First occurrence; a later one with a cue is fine too on a second scan
        // below, but one cue-hit is enough for strict routing.
        let mut search_from = 0;
        while let Some(idx) = lower[search_from..].find(phrase) {
            let idx = search_from + idx;
            if idx > 0 {
                let prev = lower.as_bytes()[idx - 1] as char;
                if prev.is_alphanumeric() {
                    search_from = idx + phrase.len();
                    continue;
                }
            }
            let after = idx + phrase.len();
            if after < lower.len() {
                let next = lower.as_bytes()[after] as char;
                if next.is_alphanumeric() || next == '-' {
                    search_from = idx + phrase.len();
                    continue;
                }
            }
            let window_start = idx.saturating_sub(24);
            let window = &lower[window_start..idx];
            // Immediate preposition ("… through openai") is the strongest signal.
            let prep_fire = PREP_CUES.iter().any(|c| {
                window.ends_with(c)
                    || window.ends_with(&format!(" {c}"))
                    || window.ends_with(&format!("{c} "))
            });
            // Spawn verb anywhere in the window ("spawn a claude subagent").
            let spawn_fire = SPAWN_CUES.iter().any(|c| window.contains(c));
            if !(prep_fire || spawn_fire) {
                search_from = after;
                continue;
            }
            if let Some(p) = resolve_provider_alias(phrase) {
                return Some((p.id.to_string(), None));
            }
            search_from = after;
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
    // Async admission (Prime rlm() / RLM model): if the model asked for a
    // non-blocking child, admit + spawn in the background, return the handle.
    let want_async = serde_json::from_str::<Value>(&call.arguments)
        .map(|v| v.get("async").and_then(Value::as_bool).unwrap_or(false))
        .unwrap_or(false);
    if want_async && !runner.is_subagent {
        let sid = runner.session_id_for_admission();
        let id = crate::agent::admission::admit(&sid, &desc);
        let client = runner.client.clone();
        let config = runner.config.clone();
        let cwd = runner.cwd.clone();
        let mode = runner.permission_mode.clone();
        let prompt = prompt.clone();
        let kind = kind.clone();
        let desc_for_spawn = desc.clone();
        let tx2 = tx.clone();
        let accept = cancel.clone();
        let depth = runner.subagent_depth;
        tokio::spawn(async move {
            // Resolve target like the sync path, then run in background.
            let outcome = match resolve_subagent_target(
                &client,
                &config,
                provider_override.as_deref(),
                model_override.as_deref(),
                Some(prompt.as_str()),
                Some(desc_for_spawn.as_str()),
                Some(kind.as_str()),
                &tx2,
            ) {
                SubagentTarget::Ready {
                    client: c,
                    config: cc,
                } => {
                    crate::agent::subagent::run_subagent(
                        c, *cc, cwd, mode, &prompt, &kind, depth, &accept, &tx2,
                    )
                    .await
                }
                SubagentTarget::AwaitingLogin { message, .. }
                | SubagentTarget::Unavailable { message } => {
                    Err(crate::error::NurError::Other(message))
                }
            };
            match outcome {
                Ok((text, _)) => crate::agent::admission::finish(&sid, id, &text, true),
                Err(e) => crate::agent::admission::finish(&sid, id, &e.to_string(), false),
            }
        });
        let _ = tx.send(AgentEvent::Status(format!(
            "admitted subagent #{id} · {desc} (async; agent can keep working)"
        )));
        return Ok((
            format!(
                "subagent admitted asynchronously (#{id}). It runs in the background; \
                 the model can keep working. Get the result with tool `admission` \
                 action=get id={id} or action=list. This call returns immediately and \
                 does NOT wait for the child."
            ),
            TokenUsage::default(),
        ));
    }
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
                *config,
                runner.cwd.clone(),
                runner.permission_mode.clone(),
                &prompt,
                &kind,
                runner.subagent_depth,
                cancel,
                tx,
            )
            .await
        }
        SubagentTarget::AwaitingLogin { message, .. } | SubagentTarget::Unavailable { message } => {
            // Surface as a tool error so the parent model does not treat a
            // parent-provider run as success. LoginRequired was already emitted.
            Err(NurError::Other(message))
        }
    }
}

/// Outcome of resolving where a subagent should run.
enum SubagentTarget {
    Ready {
        client: ApiClient,
        config: Box<Config>,
    },
    /// Explicit cross-provider request, but no credentials yet. Do not run.
    AwaitingLogin {
        #[allow(dead_code)]
        provider_id: String,
        #[allow(dead_code)]
        provider_name: String,
        message: String,
    },
    /// Explicit routing was understood, but the target could not be built.
    /// This stays a failure rather than impersonating the requested provider
    /// with the parent's client.
    Unavailable { message: String },
}

/// Resolve a subagent's client + config, honoring an optional cross-provider
/// override. When `provider` names a DIFFERENT provider than the parent, build a
/// client from that provider's stored credentials + catalog base/model.
///
/// Unknown provider names fail closed. A **known** provider with **no credentials**
/// also does **not** fall back - it emits
/// [`AgentEvent::LoginRequired`] and returns [`SubagentTarget::AwaitingLogin`]
/// so the TUI can open `/login` and the model is told the spawn was blocked.
#[allow(clippy::too_many_arguments)] // Cohesive routing boundary; every input affects target resolution.
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
                config: Box::new(cfg),
            };
        }
        return SubagentTarget::Ready {
            client: parent_client.clone(),
            config: Box::new(parent_config.clone()),
        };
    };
    let Some(prov) = resolve_provider_alias(requested) else {
        let message = format!(
            "subagent routing failed: unknown provider `{requested}`. Use a provider id from \
             `/login` or omit `provider` to inherit the parent explicitly."
        );
        let _ = tx.send(AgentEvent::Status(message.clone()));
        return SubagentTarget::Unavailable { message };
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
            config: Box::new(cfg),
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
    if key.trim().is_empty() && !prov.key_optional && crate::auth::t3_fallback_allowed(prov.id) {
        // No stored credential yet. Before popping a /login modal, try vendor
        // CLI and OMP (universal last resort). Saved nur keys were already
        // attempted above via resolve_api_key_for / load_provider_*. Persist
        // whatever we import so the next spawn hits the store first.
        // import_existing_session can shell out - isolate via run_blocking.
        match crate::oauth::run_blocking(|| crate::oauth::import_existing_session(prov.id)) {
            Ok(Some(tokens)) if !tokens.access_token.trim().is_empty() => {
                let _ = crate::auth::save_provider_oauth(
                    prov.id,
                    &tokens.access_token,
                    tokens.refresh_token.clone(),
                    tokens.expires_at,
                    tokens.meta.clone(),
                );
                let reresolved = match crate::auth::resolve_api_key_for(Some(prov.id)) {
                    Ok(k) if !k.trim().is_empty() => k,
                    _ => crate::auth::load_provider_key(prov.id)
                        .or_else(|| crate::auth::load_provider_oauth_token(prov.id))
                        .unwrap_or_else(|| tokens.access_token.trim().to_string()),
                };
                if !reresolved.trim().is_empty() {
                    let source = if crate::oauth::omp_bridge::is_omp_import(&tokens) {
                        "OMP".to_string()
                    } else if let Some(driver) = driver_for_provider(prov.id) {
                        format!("vendor CLI ({})", driver.as_str())
                    } else {
                        "imported session".into()
                    };
                    let _ = tx.send(AgentEvent::Status(format!(
                        "subagent · imported {} session from {source}",
                        prov.name
                    )));
                    key = reresolved;
                }
            }
            _ => {}
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
    //
    // Prefer the store-linked OAuth context, but also accept a JWT-shaped bearer
    // for providers that have a dedicated OAuth host (xAI / OpenAI / Kimi). A
    // cross-provider rebuild that holds a fresh Grok JWT must still hit
    // cli-chat-proxy, not api.x.ai with Chat Completions.
    let is_oauth = crate::auth::oauth_request_context(prov.id, &key).is_some()
        || (crate::providers::oauth_base_url(prov.id).is_some() && key_looks_like_jwt(&key));
    let (base_url, style, default_model) =
        crate::providers::endpoint_for_credential(prov, is_oauth);
    // key_optional local providers may have an empty key.
    let client = match ApiClient::for_provider(base_url, &key, prov.id) {
        Ok(c) => c.with_style(style),
        Err(e) => {
            let message = format!(
                "subagent routing failed: could not build the explicitly requested {} client: {e}",
                prov.name
            );
            let _ = tx.send(AgentEvent::Status(message.clone()));
            return SubagentTarget::Unavailable { message };
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
        config: Box::new(cfg),
    }
}

/// Map a natural-language provider name to a catalog provider. Thin wrapper over
/// [`crate::providers::resolve_provider_alias`] - the single source of truth
/// shared with `/login <provider>` in the TUI - so cross-provider subagent
/// deployment and the login modal accept the exact same aliases.
fn resolve_provider_alias(raw: &str) -> Option<&'static crate::providers::Provider> {
    crate::providers::resolve_provider_alias(raw)
}

/// JWT-shaped bearer (`eyJ…`.`…`.`…`) — used to pick OAuth endpoints when the
/// store-linked OAuth context is briefly unavailable during a cross-provider rebuild.
fn key_looks_like_jwt(token: &str) -> bool {
    let t = token.trim();
    if !t.starts_with("eyJ") {
        return false;
    }
    let mut parts = t.split('.');
    matches!(
        (parts.next(), parts.next(), parts.next(), parts.next()),
        (Some(h), Some(p), Some(s), None) if !h.is_empty() && !p.is_empty() && !s.is_empty()
    )
}

/// Map a nur provider id to the vendor CLI (t3code) driver that can supply its
/// credentials via `import_existing_session`. Used to auto-import a logged-in
/// vendor CLI session before falling back to a `/login` prompt so cross-provider
/// subagents "just work" when the user is already signed in to the vendor CLI.
/// Returns `None` for providers with no direct vendor CLI.
fn driver_for_provider(provider_id: &str) -> Option<crate::t3code::DriverId> {
    use crate::t3code::DriverId;
    match provider_id {
        "anthropic" => Some(DriverId::Claude),
        "openai" => Some(DriverId::Codex),
        "xai" => Some(DriverId::Grok),
        "antigravity" => Some(DriverId::Antigravity),
        "google" => Some(DriverId::Gemini),
        "opencode" => Some(DriverId::OpenCode),
        "cursor" => Some(DriverId::Cursor),
        _ => None,
    }
}

/// Tools that count as "starting implementation" for OMP prewalk.
fn is_prewalk_mutate_tool(name: &str) -> bool {
    matches!(
        name,
        "write_file" | "edit_file" | "multi_edit" | "apply_patch"
    )
}

/// Resolve the cheap/smol target for prewalk (config → env → OMP role).
pub fn resolve_prewalk_into(cfg: &Config) -> Option<String> {
    let configured = cfg.prewalk.into.trim();
    if !configured.is_empty() {
        return Some(configured.to_string());
    }
    for var in ["NUR_PREWALK_MODEL", "OMP_SMOL_MODEL", "PI_SMOL_MODEL"] {
        if let Ok(v) = std::env::var(var) {
            let v = v.trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    crate::oauth::omp_bridge::omp_model_role("smol")
}

fn maybe_fire_prewalk(runner: &AgentRunner, tool_name: &str) -> Option<String> {
    if !runner.config.prewalk.enabled || runner.is_subagent {
        return None;
    }
    if !is_prewalk_mutate_tool(tool_name) {
        return None;
    }
    if runner.tools.todos_snapshot().items.is_empty() {
        return None;
    }
    {
        let guard = runner.prewalk_override.lock().ok()?;
        if guard.is_some() {
            return None; // already switched this session
        }
    }
    let into = resolve_prewalk_into(&runner.config)?;
    if into.eq_ignore_ascii_case(&runner.config.model) {
        return None;
    }
    if let Ok(mut guard) = runner.prewalk_override.lock() {
        *guard = Some(into.clone());
    }
    Some(into)
}

/// Resolve OMP-style remote compact URL. Opt-in only:
/// - `compaction.remote_endpoint` in config.toml (implies want remote), or
/// - `compaction.remote_enabled = true` plus endpoint / `NUR_COMPACT_REMOTE_ENDPOINT`, or
/// - `NUR_COMPACT_REMOTE=1` plus `NUR_COMPACT_REMOTE_ENDPOINT`.
fn remote_compact_endpoint(cfg: &Config) -> Option<String> {
    let cfg_ep = cfg.compaction.remote_endpoint.trim();
    let env_ep = std::env::var("NUR_COMPACT_REMOTE_ENDPOINT")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let env_force = std::env::var("NUR_COMPACT_REMOTE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if !cfg_ep.is_empty() {
        // OMP: setting remoteEndpoint is enough.
        return Some(cfg_ep.to_string());
    }
    if cfg.compaction.remote_enabled || env_force {
        return env_ep;
    }
    None
}

/// OMP `compaction.remoteEndpoint` protocol: POST `{systemPrompt,prompt}` → `{summary}`.
struct RemoteCompactSummary {
    summary: String,
    endpoint_origin: String,
    input_tokens_estimate: u64,
    output_tokens: u64,
    usage_reported: bool,
}

async fn try_remote_compact_summary(
    runner: &AgentRunner,
    system_prompt: &str,
    items: &[Value],
) -> Option<RemoteCompactSummary> {
    let endpoint = remote_compact_endpoint(&runner.config)?;

    // Preserve both the original goal and the newest in-flight work. The old
    // `take(80)` retained only the beginning of long sessions, and appended the
    // summary instruction a second time even though it is already in `items`.
    let serialized = bounded_remote_compact_transcript(items, 120_000);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .ok()?;
    let openai_compatible = endpoint
        .split('?')
        .next()
        .unwrap_or(&endpoint)
        .trim_end_matches('/')
        .ends_with("/chat/completions");
    let body = if openai_compatible {
        serde_json::json!({
            "model": runner.config.model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": serialized},
            ],
            "stream": false,
        })
    } else {
        serde_json::json!({
            "systemPrompt": system_prompt,
            "prompt": serialized,
        })
    };
    let resp = client.post(&endpoint).json(&body).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: Value = resp.json().await.ok()?;
    let summary = v
        .get("summary")
        .and_then(Value::as_str)
        .or_else(|| {
            v.pointer("/choices/0/message/content")
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let input_tokens_estimate = (serialized.chars().count() as u64).div_ceil(3);
    let reported_input = v
        .pointer("/usage/input_tokens")
        .and_then(Value::as_u64)
        .or_else(|| v.pointer("/usage/prompt_tokens").and_then(Value::as_u64));
    let reported_output = v
        .pointer("/usage/output_tokens")
        .and_then(Value::as_u64)
        .or_else(|| {
            v.pointer("/usage/completion_tokens")
                .and_then(Value::as_u64)
        });
    let usage_reported = reported_input.is_some() || reported_output.is_some();
    Some(RemoteCompactSummary {
        summary: summary.to_string(),
        endpoint_origin: compact_endpoint_origin(&endpoint),
        input_tokens_estimate: reported_input.unwrap_or(input_tokens_estimate),
        output_tokens: reported_output
            .unwrap_or_else(|| (summary.chars().count() as u64).div_ceil(3)),
        usage_reported,
    })
}

fn compact_endpoint_origin(endpoint: &str) -> String {
    let without_scheme = endpoint
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(endpoint);
    without_scheme
        .split('/')
        .next()
        .unwrap_or("remote-endpoint")
        .split('?')
        .next()
        .unwrap_or("remote-endpoint")
        .to_string()
}

fn bounded_remote_compact_transcript(items: &[Value], max_chars: usize) -> String {
    let full = items
        .iter()
        .filter_map(|item| serde_json::to_string(item).ok())
        .collect::<Vec<_>>()
        .join("\n");
    if full.chars().count() <= max_chars {
        return full;
    }

    const MARKER: &str = "\n...[middle omitted for bounded remote compaction]...\n";
    let content_budget = max_chars.saturating_sub(MARKER.chars().count());
    let head_budget = content_budget / 4;
    let tail_budget = content_budget.saturating_sub(head_budget);
    let head: String = full.chars().take(head_budget).collect();
    let tail: String = full
        .chars()
        .rev()
        .take(tail_budget)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{head}{MARKER}{tail}")
}

pub async fn compact_session(
    runner: &AgentRunner,
    session: &mut Session,
    usage: &mut UsageTracker,
) -> Result<String> {
    snapshot_before_compact(session);

    // Thin old tool bodies for the summarizer so we don't re-pay huge dumps.
    let mut items = session.input_items.clone();
    let thinned = thin_tool_bodies_for_compact(
        &mut items,
        runner.config.compact_tool_body_max_chars as usize,
        runner.config.compact_keep_user_turns as usize,
    );
    let user_prompt =
        "Summarize this conversation for a fresh context window. Capture: goals, decisions, \
         files touched, current state, pending next steps. Prefer decisions over raw tool dumps. \
         Dense bullets.";
    let system_prompt = "You compress agent conversations into handoff summaries. \
             Preserve goals, decisions, file paths, and next steps; drop redundant tool noise.";
    items.push(user_text_item(user_prompt));
    let compact_input_estimate = serde_json::to_string(&items)
        .map(|text| (text.chars().count() as u64).div_ceil(3))
        .unwrap_or(0)
        .saturating_add((system_prompt.chars().count() as u64).div_ceil(3));
    if let Some(msg) = preflight_request_budget(
        &runner.config,
        usage,
        &runner.config.provider,
        &runner.config.model,
        compact_input_estimate,
        runner.config.request_output_reserve_tokens,
    ) {
        return Err(NurError::Budget(format!("compaction {msg}")));
    }

    // OMP-compatible remote summarization (compaction.remoteEndpoint). Opt-in;
    // any failure falls through to the local model path below.
    let remote_summary = try_remote_compact_summary(runner, system_prompt, &items).await;
    let summary = if let Some(remote) = remote_summary {
        let remote_usage =
            TokenUsage::estimated(remote.input_tokens_estimate, remote.output_tokens);
        // The endpoint is an explicit remote boundary, not the active route.
        // Pricing is necessarily an estimate unless the endpoint reports a
        // native cost, but tokens still count toward session safeguards.
        usage.record_request_for_route(
            "remote-compaction",
            &remote.endpoint_origin,
            remote_usage,
            None,
        );
        session.usage.add(usage.last_usage());
        receipt::record(
            &session.id,
            receipt::Event::RemoteCompaction {
                endpoint_origin: remote.endpoint_origin.clone(),
                input_tokens_estimate: remote.input_tokens_estimate,
                output_tokens: remote.output_tokens,
                usage_reported: remote.usage_reported,
            },
        );
        remote.summary
    } else {
        let req = ResponseRequest {
            model: runner.config.model.clone(),
            input: Value::Array(items),
            instructions: Some(system_prompt.into()),
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
            max_output_tokens: (runner.config.request_output_reserve_tokens > 0)
                .then_some(runner.config.request_output_reserve_tokens),
        };
        let resp = runner.client.create_response(&req).await?;
        let raw = resp.accounting_usage();
        usage.record_request(raw, resp.id.clone());
        session.usage.add(usage.last_usage());
        let summary = resp.output_text();
        if summary.is_empty() {
            return Err(NurError::Other("compaction produced no summary".into()));
        }
        summary
    };

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
    // RLM invariant (Prime): compaction summarizes chat but context_store vars remain.
    // Connectome: live edge is compacted; deep memory is maintained *localized* (paper M4).
    let mem_scope = {
        let proj = std::path::Path::new(&session.cwd)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("workspace");
        format!("{proj}:{}", session.id)
    };
    if runner.config.native_memory {
        let _ = super::native_memory::consolidate_localized(&mem_scope, 32);
        // closes the tier-ladder gap: age-progress recent→l1 and l2→l3 so deep
        // tiers are produced automatically, not only by manual remember/consolidate.
        let _ = super::native_memory::promote_aged(&mem_scope);
        let _ = super::chronicle::append(
            &mem_scope,
            "compact",
            "context compacted - recent edge summarized; native memory tiers preserved",
            None,
        );
    }
    let store_inv = super::context_store::prompt_inventory(&session.id);
    let mem_inv = if runner.config.native_memory {
        super::native_memory::prompt_block(&mem_scope, "", 1_200)
    } else {
        String::new()
    };
    let store_note = {
        let mut parts = Vec::new();
        if !store_inv.is_empty() {
            parts.push(format!(
                "[RLM context store preserved — tool `context`]\n{store_inv}"
            ));
        }
        if !mem_inv.is_empty() {
            parts.push(mem_inv);
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("\n\n{}", parts.join("\n"))
        }
    };
    let mut new_items = vec![user_text_item(&format!(
        "[Context compacted. Summary of the conversation so far:]\n\n{summary}{store_note}"
    ))];
    let recent = recent_dialogue_items(&session.messages, keep_n);
    let kept = recent.len();
    let tail = safe_tail_after_compact(&session.input_items, COMPACT_KEEP_WORKING_ITEMS);
    let (tail_kept, strategy) = if runner.config.kv_stable_compact {
        // KV-stable: carry the recent edge verbatim and preserve its ordering,
        // maximizing a reusable prefix at providers that cache it.
        new_items.extend(recent);
        (
            extend_unique_items(&mut new_items, tail),
            "kv-stable: recent edge carried verbatim",
        )
    } else {
        // Classic rebuild: put the in-flight work directly after the new
        // summary, then reconstruct recent dialogue. This deliberately opts
        // out of retaining the former cacheable edge while preserving valid
        // tool-call/result pairs for stateless provider loops.
        let tail_kept = extend_unique_items(&mut new_items, tail);
        extend_unique_items(&mut new_items, recent);
        (tail_kept, "classic rebuild: working tail reconstructed")
    };
    session.input_items = new_items;
    runner.persist_session(session);
    Ok(format!(
        "{summary}\n\n[compact: thinned {thinned} tool bodies · kept {kept} recent dialogue items · \
         {tail_kept} working items · precompact bak written · context_store vars preserved · \
         {strategy}]"
    ))
}

fn extend_unique_items(
    items: &mut Vec<Value>,
    additions: impl IntoIterator<Item = Value>,
) -> usize {
    let before = items.len();
    for item in additions {
        if !items.contains(&item) {
            items.push(item);
        }
    }
    items.len() - before
}

/// Keep a request-valid recent context when the provider refuses both the
/// original request and the model-assisted compaction request as too large.
///
/// The full persisted transcript is copied to `*.precompact.bak` first. The
/// live request retains recent dialogue and complete tool-call/result pairs,
/// which is enough for the agent to continue or reconstruct details with tools.
fn emergency_compact_session(runner: &AgentRunner, session: &mut Session) -> usize {
    snapshot_before_compact(session);
    let keep_turns = runner.config.compact_keep_user_turns.max(2) as usize;
    let mut new_items = vec![user_text_item(
        "[Emergency context recovery: the provider rejected the prior request as larger than its \
         context window. Older details remain in the persisted session and precompact backup. \
         Continue the current task from the recent dialogue and complete tool results below. \
         Re-read workspace files when an omitted detail is needed.]",
    )];
    new_items.extend(recent_dialogue_items(&session.messages, keep_turns));
    new_items.extend(safe_tail_after_compact(
        &session.input_items,
        EMERGENCY_KEEP_WORKING_ITEMS,
    ));
    session.input_items = new_items;
    runner.persist_session(session);
    session.input_items.len()
}

fn snapshot_before_compact(session: &Session) {
    let path = session.path();
    if path.is_file() {
        let pre = path.with_extension("precompact.bak");
        let _ = std::fs::copy(path, pre);
    }
}

/// Working items carried across a compaction so the model can see the task it
/// was mid-way through.
const COMPACT_KEEP_WORKING_ITEMS: usize = 12;
const EMERGENCY_KEEP_WORKING_ITEMS: usize = 16;

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

    #[test]
    fn context_promotion_prefers_non_spark_sibling() {
        assert_eq!(
            pick_context_promotion_target(
                &[
                    "openai-codex/gpt-5.3-codex-spark".into(),
                    "openai-codex/gpt-5.3-codex".into(),
                ],
                "openai-codex/gpt-5.3-codex-spark"
            )
            .as_deref(),
            Some("openai-codex/gpt-5.3-codex")
        );
    }

    #[test]
    fn repeated_observations_drop_only_the_stale_body_and_keep_pairs() {
        let large = "old ".repeat(200);
        let mut items = vec![
            json!({"type":"function_call","call_id":"old","name":"read_file","arguments":"{\"path\":\"a.rs\"}"}),
            json!({"type":"function_call_output","call_id":"old","output":large}),
            json!({"type":"function_call","call_id":"write","name":"write_file","arguments":"{\"path\":\"a.rs\"}"}),
            json!({"type":"function_call_output","call_id":"write","output":"wrote file"}),
            json!({"type":"function_call","call_id":"new","name":"read_file","arguments":"{\"path\":\"a.rs\"}"}),
            json!({"type":"function_call_output","call_id":"new","output":"current contents"}),
        ];
        assert_eq!(prune_superseded_observations(&mut items), 1);
        assert!(items[1]["output"]
            .as_str()
            .unwrap()
            .starts_with("[superseded by newer"));
        assert_eq!(items[3]["output"], "wrote file");
        assert_eq!(items[5]["output"], "current contents");
        assert_eq!(
            items
                .iter()
                .filter(|item| item["type"] == "function_call")
                .count(),
            3,
            "tool declarations remain paired"
        );
        assert_eq!(
            items
                .iter()
                .filter(|item| item["type"] == "function_call_output")
                .count(),
            3,
            "tool results remain paired"
        );
        assert_eq!(
            prune_superseded_observations(&mut items),
            0,
            "pruning is idempotent"
        );
    }

    #[test]
    fn reordered_json_arguments_still_supersede_the_same_observation() {
        let large = "old ".repeat(200);
        let mut items = vec![
            json!({"type":"function_call","call_id":"old","name":"grep","arguments":"{\"path\":\"src\",\"pattern\":\"auth\"}"}),
            json!({"type":"function_call_output","call_id":"old","output":large}),
            json!({"type":"function_call","call_id":"new","name":"grep","arguments":"{ \"pattern\": \"auth\", \"path\": \"src\" }"}),
            json!({"type":"function_call_output","call_id":"new","output":"current"}),
        ];

        assert_eq!(prune_superseded_observations(&mut items), 1);
        assert!(items[1]["output"]
            .as_str()
            .unwrap()
            .starts_with("[superseded by newer"));
    }

    #[test]
    fn useless_pruning_never_discards_errors_or_grows_short_results() {
        let bodies = [
            "error: provider timed out",
            "Error: permission denied",
            "FAILED validation",
            "{}",
            "null",
            "",
        ];
        let mut items: Vec<Value> = bodies
            .iter()
            .enumerate()
            .map(|(index, body)| {
                json!({
                    "type":"function_call_output",
                    "call_id":format!("call-{index}"),
                    "output":body,
                })
            })
            .collect();

        assert_eq!(prune_useless_observations(&mut items), 0);
        for (item, body) in items.iter().zip(bodies) {
            assert_eq!(item["output"], body);
        }
    }

    #[test]
    fn reserve_based_compaction_threshold_matches_omp_defaults() {
        assert_eq!(compaction_threshold_tokens(8_000), 6_800);
        assert_eq!(compaction_threshold_tokens(16_000), 13_600);
        assert_eq!(compaction_threshold_tokens(20_000), 17_000);
        assert_eq!(compaction_threshold_tokens(32_000), 27_200);
        assert_eq!(compaction_threshold_tokens(64_000), 47_616);
        assert_eq!(compaction_threshold_tokens(128_000), 108_800);
        assert_eq!(compaction_threshold_tokens(1_000_000), 850_000);
    }

    #[test]
    fn context_estimate_does_not_count_inline_base64_as_text() {
        let items = vec![json!({
            "role":"user",
            "content":[{
                "type":"input_image",
                "image_url":format!("data:image/png;base64,{}", "a".repeat(200_000)),
            }]
        })];
        assert!(estimate_context_tokens(&items) < 2_000);
    }

    #[test]
    fn context_limit_errors_are_detected_without_retrying_validation_errors() {
        for message in [
            "maximum context length is 128000 tokens",
            "input token count exceeds the model limit",
            "request_too_large",
            "Prompt is too long for this context window",
        ] {
            assert!(
                is_context_limit_error(&NurError::Api {
                    status: 400,
                    message: message.into(),
                }),
                "{message}"
            );
        }
        assert!(!is_context_limit_error(&NurError::Api {
            status: 400,
            message: "invalid tool call id".into(),
        }));
        assert!(!is_context_limit_error(&NurError::Api {
            status: 401,
            message: "invalid api key".into(),
        }));
    }

    #[test]
    fn retired_default_recovery_picks_a_same_family_chat_model() {
        let models = vec![
            "text-embedding-3-large".into(),
            "gpt-image-1".into(),
            "gpt-5.4-mini".into(),
            "gpt-5.6-terra".into(),
        ];
        assert_eq!(
            pick_replacement_model(&models, "gpt-5.5").as_deref(),
            Some("gpt-5.6-terra")
        );
        assert!(is_model_unavailable_error(&NurError::Api {
            status: 404,
            message: "model gpt-5.5 does not exist".into(),
        }));
        assert!(!is_model_unavailable_error(&NurError::Api {
            status: 404,
            message: "file does not exist".into(),
        }));
    }
}

#[cfg(test)]
mod prewalk_remote_compact_tests {
    use super::*;
    use crate::config::{CompactionConfig, Config, PrewalkConfig};

    #[test]
    fn prewalk_mutate_tools_match_omp() {
        assert!(is_prewalk_mutate_tool("write_file"));
        assert!(is_prewalk_mutate_tool("edit_file"));
        assert!(is_prewalk_mutate_tool("multi_edit"));
        assert!(is_prewalk_mutate_tool("apply_patch"));
        assert!(!is_prewalk_mutate_tool("read_file"));
        assert!(!is_prewalk_mutate_tool("bash"));
    }

    #[test]
    fn remote_compact_off_by_default() {
        let cfg = Config {
            compaction: CompactionConfig::default(),
            ..Config::default()
        };
        assert!(remote_compact_endpoint(&cfg).is_none());
    }

    #[test]
    fn remote_compact_endpoint_in_config_opts_in() {
        let cfg = Config {
            compaction: CompactionConfig {
                remote_enabled: false,
                remote_endpoint: "https://example.test/compact".into(),
            },
            ..Config::default()
        };
        assert_eq!(
            remote_compact_endpoint(&cfg).as_deref(),
            Some("https://example.test/compact")
        );
    }

    #[test]
    fn prewalk_into_from_config() {
        let cfg = Config {
            prewalk: PrewalkConfig {
                enabled: true,
                into: "gpt-5.4-mini".into(),
            },
            ..Config::default()
        };
        assert_eq!(resolve_prewalk_into(&cfg).as_deref(), Some("gpt-5.4-mini"));
    }

    #[test]
    fn bounded_remote_transcript_preserves_goal_and_live_tail() {
        let items = vec![
            serde_json::json!({"role":"user","content":"ORIGINAL-GOAL"}),
            serde_json::json!({"type":"function_call_output","output":"x".repeat(5_000)}),
            serde_json::json!({"role":"user","content":"LATEST-WORK"}),
        ];
        let transcript = bounded_remote_compact_transcript(&items, 1_000);
        assert!(transcript.contains("ORIGINAL-GOAL"));
        assert!(transcript.contains("LATEST-WORK"));
        assert!(transcript.contains("middle omitted"));
        assert!(transcript.chars().count() <= 1_000);
    }

    #[test]
    fn compact_tail_does_not_duplicate_an_identical_recent_user_item() {
        let duplicate = user_text_item("same task");
        let mut new_items = vec![duplicate.clone()];
        let added = extend_unique_items(
            &mut new_items,
            vec![duplicate, user_text_item("new detail")],
        );
        assert_eq!(added, 1);
        assert_eq!(new_items.len(), 2);
    }
}

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
use crate::tools::{is_concurrency_safe, is_read_only_call, ToolContext, ToolHost};
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
    ChildStatus { parent_session: String, status: String, finished: bool },
    ReasoningDelta(String),
    TextDelta(String),
    /// The model stream dropped after `discard` bytes of text had already been
    /// sent as [`AgentEvent::TextDelta`]s, and the request is being replayed.
    /// Consumers remove that tail so the replayed answer does not land on top
    /// of the partial one ([`discard_streamed_tail`]).
    /// The advisor (`agent::advisor`) left a note for the agent.
    Advice {
        model: String,
        note: String,
    },
    StreamRestart {
        discard: usize,
    },
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
        parent_session: Option<String>,
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
    /// The model asked for a credential (`secret` action request). The TUI
    /// shows the masked secret modal and answers with the saved name, or
    /// `None` when the user declines; a dropped sender means no user.
    SecretRequest {
        name: String,
        purpose: String,
        respond: oneshot::Sender<Option<String>>,
    },
    ApprovalRequest {
        name: String,
        args: String,
        respond: oneshot::Sender<ApprovalDecision>,
        parent_session: Option<String>,
    },
    /// The model asked the user a close-ended question (`question` tool).
    /// Interactive clients answer; noninteractive runners dismiss it.
    QuestionRequest {
        turn_cancel: CancellationToken,
        question: String,
        header: String,
        options: Vec<(String, String)>,
        multi_select: bool,
        respond: oneshot::Sender<QuestionAnswer>,
        parent_session: Option<String>,
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
        /// Process exit code for headless callers (`crate::error::exit`);
        /// `result` is a string by then, so the class travels separately.
        exit_code: i32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    Approve,
    ApproveAlways,
    Deny,
}

/// The user's answer to a `question` tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionAnswer {
    /// Labels picked in the modal (at most one unless multi-select).
    pub selected: Vec<String>,
    /// Free-form response typed by the user when the offered options do not fit.
    pub typed: Option<String>,
    /// True when the question was dismissed, redirected, or asked somewhere
    /// with no interactive user: proceed, do not re-ask.
    pub dismissed: bool,
    /// Short reason shown when dismissed (`"dismissed"`, `"headless"`, ...).
    pub reason: &'static str,
}

impl QuestionAnswer {
    pub fn picked(selected: Vec<String>) -> Self {
        Self {
            selected,
            typed: None,
            dismissed: false,
            reason: "",
        }
    }

    pub fn typed(text: String) -> Self {
        Self {
            selected: Vec::new(),
            typed: Some(text),
            dismissed: false,
            reason: "",
        }
    }

    pub fn dismissed() -> Self {
        Self {
            selected: Vec::new(),
            typed: None,
            dismissed: true,
            reason: "dismissed",
        }
    }

    pub fn unavailable() -> Self {
        Self {
            selected: Vec::new(),
            typed: None,
            dismissed: true,
            reason: "no interactive user",
        }
    }
}

// Tool capability classification (read-only / parallel / destructive) lives in
// `crate::tools::capabilities` — single source of truth for the agent loop.

/// True the first time this exact notice is seen in the process. For status
/// lines that describe a standing condition (a route's capabilities, a missing
/// layer) rather than an event: say it once, not before every request.
pub fn first_time(notice: &str) -> bool {
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SEEN.get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .map(|mut seen| seen.insert(notice.to_string()))
        .unwrap_or(true)
}

pub struct AgentRunner {
    pub client: ApiClient,
    pub config: Config,
    pub cwd: PathBuf,
    pub permission_mode: SharedMode,
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
    /// The caller can receive and answer questions from delegated agents.
    pub interactive_children: bool,
    /// Current recursion depth of this runner (0 = root). Incremented when a
    /// subagent spawns children; bounded by `config.subagent_depth`.
    pub subagent_depth: u32,
    /// OMP-style prewalk: once fired, later turns in this TUI session use this
    /// model (shared Arc so `make_runner` each turn still sees the switch).
    pub prewalk_override: Arc<Mutex<Option<String>>>,
    /// Domains the user approved for `web_fetch` this session. A fetch to a
    /// new domain asks first (a page's instructions could otherwise pull the
    /// agent onto any host); Auto mode skips the gate by definition.
    pub approved_domains: Arc<Mutex<HashSet<String>>>,
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
        let exit_code = res
            .as_ref()
            .map_or_else(NurError::exit_code, |_| crate::error::exit::OK);
        let result = res.map_err(|e| e.to_string());
        let _ = tx.send(AgentEvent::Done {
            session: Box::new(session),
            usage: Box::new(usage),
            result,
            interrupted,
            exit_code,
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
                AgentEvent::StreamRestart { discard } => discard_streamed_tail(&mut acc, discard),
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
                // No interactive user either - the model proceeds or defers.
                AgentEvent::QuestionRequest { respond, .. } => {
                    let _ = respond.send(QuestionAnswer::unavailable());
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
    let result = Box::pin(runner.run_turn_events(&mut session, &prompt, &mut usage, &tx, &cancel)).await;
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

/// Drop the last `discard` bytes of streamed text after an
/// [`AgentEvent::StreamRestart`]. Falls back to the nearest char boundary
/// below, so a consumer that saw extra text never panics on a split char.
pub fn discard_streamed_tail(text: &mut String, discard: usize) {
    let mut keep = text.len().saturating_sub(discard);
    while !text.is_char_boundary(keep) {
        keep -= 1;
    }
    text.truncate(keep);
}

/// Backoff before each reconnect after a transport failure (see
/// [`NurError::is_transport`]). The first retry is almost immediate: the most
/// common cause is a pooled socket the server had already closed, and a fresh
/// connection fixes it at once. The rest ride out a network blip or a provider
/// edge restart (~30s in all) before the failover chain or the error takes over.
const TRANSPORT_BACKOFF_MS: &[u64] = &[250, 1_000, 2_000, 4_000, 8_000, 15_000];

impl AgentRunner {
    fn persist_session(&self, session: &Session) {
        if !self.is_subagent {
            let _ = session.save();
        }
    }

    /// Append the AGENTS.md / NUR.md of directories below the cwd that this
    /// tool call touched for the first time this session. They ride in the
    /// tool result, never the system prompt, so the cached prefix holds.
    fn with_nested_instructions(
        &self,
        session_id: &str,
        tool: &str,
        args: &str,
        body: String,
    ) -> String {
        match super::instructions::nested_for_tool(
            &self.cwd,
            session_id,
            tool,
            args,
            self.config.instructions.max_chars,
        ) {
            Some(extra) => body + &extra,
            None => body,
        }
    }

    /// Run one model request against `client`, forwarding stream events to the
    /// UI. Returns `(response, text_bytes_streamed)` on success, or
    /// `(error, text_bytes_streamed)` so the caller can tell whether failing
    /// over is safe (only when nothing was streamed yet) and how much streamed
    /// text a replay must retract.
    ///
    /// No wall clock bounds the request unless the user set
    /// `NUR_PROVIDER_TURN_TIMEOUT_SECS`: a long answer that keeps streaming is
    /// never cut off. A silent connection is caught by the HTTP client's idle
    /// timeout and comes back as a transport error.
    async fn stream_one(
        &self,
        client: &ApiClient,
        req: &ResponseRequest,
        tx: &mpsc::UnboundedSender<AgentEvent>,
        cancel: &CancellationToken,
        rules: &super::stream_rules::RuleWatch,
    ) -> std::result::Result<(ApiResponse, usize), (NurError, usize)> {
        let delta_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let turn_cancel = cancel.child_token();
        let cap = crate::api::client::provider_turn_cap();
        let capped = async {
            match cap {
                Some(cap) => tokio::time::sleep(cap).await,
                None => std::future::pending::<()>().await,
            }
        };
        tokio::pin!(capped);
        let cap_error = || {
            NurError::Other(format!(
                "provider request exceeded your {}s cap ({})",
                cap.unwrap_or_default().as_secs(),
                crate::api::client::TURN_TIMEOUT_ENV
            ))
        };
        // Agent CLIs (cursor-agent, droid) must honor Esc cancel; always take
        // the streaming path so the CancellationToken reaches the child
        // process (even for subagents).
        if req.stream == Some(true) || client.uses_agent_cli() {
            let streamed_count = delta_count.clone();
            // Stream rules watch the answer as it arrives; a match stops the
            // request here and the caller asks again with the rule added.
            let streamed_text = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
            let rule_hit: std::sync::Arc<std::sync::Mutex<Option<String>>> = Default::default();
            let (watch_text, watch_hit, stop) =
                (streamed_text.clone(), rule_hit.clone(), turn_cancel.clone());
            let request = client.create_response_stream(
                req,
                move |ev| match ev {
                    StreamEvent::TextDelta(d) => {
                        streamed_count.fetch_add(d.len(), std::sync::atomic::Ordering::Relaxed);
                        if !rules.is_empty() {
                            let mut text = watch_text.lock().unwrap_or_else(|e| e.into_inner());
                            text.push_str(&d);
                            let mut hit = watch_hit.lock().unwrap_or_else(|e| e.into_inner());
                            if hit.is_none() {
                                if let Some(rule) = rules.check(&text) {
                                    *hit = Some(rule.name.clone());
                                    stop.cancel();
                                }
                            }
                        }
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
                _ = &mut capped => {
                    turn_cancel.cancel();
                    Err(cap_error())
                }
                result = &mut request => result,
            };
            let deltas = delta_count.load(std::sync::atomic::Ordering::Relaxed);
            if let Some(name) = rule_hit.lock().unwrap_or_else(|e| e.into_inner()).take() {
                return Err((
                    NurError::StreamRule {
                        name,
                        discard: deltas,
                    },
                    deltas,
                ));
            }
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
                _ = &mut capped => {
                    turn_cancel.cancel();
                    Err((cap_error(), 0))
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
        rules: &super::stream_rules::RuleWatch,
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
        //
        // A dropped connection is a pause too. Reset sockets, a keep-alive the
        // server had already closed, an edge restart or a silent stall all
        // mean the provider never finished answering, not that the request is
        // wrong, so it is replayed on the same provider with backoff. If text
        // had already streamed, consumers are told to retract it first
        // (`StreamRestart`); tool calls only run from a completed response, so
        // a replay cannot repeat one.
        let mut attempt: u32 = 0;
        let mut reconnects: usize = 0;
        let primary_err = loop {
            match self.stream_one(&self.client, req, tx, cancel, rules).await {
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
                    if e.is_transport()
                        && reconnects < TRANSPORT_BACKOFF_MS.len()
                        && !cancel.is_cancelled()
                    {
                        let wait_ms = TRANSPORT_BACKOFF_MS[reconnects];
                        reconnects += 1;
                        if emitted > 0 {
                            let _ = tx.send(AgentEvent::StreamRestart { discard: emitted });
                        }
                        let wait = if wait_ms < 1_000 {
                            format!("{wait_ms}ms")
                        } else {
                            format!("{}s", wait_ms / 1_000)
                        };
                        let _ = tx.send(AgentEvent::Status(format!(
                            "connection to {} dropped ({e}) - reconnecting in {wait}, retry {reconnects}/{}",
                            self.config.provider,
                            TRANSPORT_BACKOFF_MS.len()
                        )));
                        tokio::select! {
                            _ = cancel.cancelled() => return Err(NurError::Interrupted),
                            _ = tokio::time::sleep(std::time::Duration::from_millis(wait_ms)) => {}
                        }
                        continue;
                    }
                    // Another key for the same provider is the cheapest fix
                    // for a rate limit, an empty quota or a refused key: no
                    // wait, no provider switch. Each rotation blocks the key
                    // that failed, so this ends once every key has been tried.
                    if emitted == 0 && !cancel.is_cancelled() {
                        if let Some(note) = self.client.rotate_key_after(&e) {
                            let _ = tx.send(AgentEvent::Status(note));
                            continue;
                        }
                    }
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

        // Billing guard: a subscription route must not fail over to a
        // per-token billed route unless the user opted in - an outage would
        // otherwise quietly start spending money on a meter the user never
        // picked (the mirror of the privacy floor above).
        let primary_is_subscription =
            crate::api::failover::active_route_is_subscription(&self.config);

        let mut last = primary_err;
        for t in targets {
            if primary_is_subscription
                && !t.via_oauth
                && !self.config.failover_allow_paid
                && !crate::providers::by_id(&t.provider_id).is_some_and(|p| p.key_optional)
            {
                let _ = tx.send(AgentEvent::Status(format!(
                    "failover skipped {} - it bills per token while this session runs on a \
                     subscription; set failover_allow_paid = true to allow",
                    t.provider_id
                )));
                continue;
            }
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
                reasoning.effort = crate::providers::nearest_effort(
                    &t.provider_id,
                    &t.model,
                    &self.config.reasoning_effort,
                );
            }
            let target_caps = crate::api::failover::route_capabilities(&t.provider_id, t.style);
            if !target_caps.parallel_tools {
                req2.parallel_tool_calls = Some(false);
            }
            if !client.supports_output_limit(&req2.model) {
                req2.max_output_tokens = None;
            }
            match self.stream_one(&client, &req2, tx, cancel, rules).await {
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
        let effective_text = if self.config.ignore_project_tuning {
            None
        } else {
            crate::self_tune::active_instruction(&self.cwd, &self.config.provider, &self.config.model)
                .map(|instruction| crate::self_tune::apply_instruction(&instruction, user_text))
        };
        let user_text = effective_text.as_deref().unwrap_or(user_text);
        crate::typesafe::context::scope(session.id.clone(), cancel.clone(), async {
            let result = self
                .run_turn_scoped(session, user_text, usage, tx, cancel)
                .await;
            record_auxiliary_telemetry(&session.id);
            result
        })
        .await
    }

    async fn run_turn_scoped(
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
            // Policy `ask` letters are held for a user decision right here;
            // with nobody answering (headless EOF, continuous) they refuse.
            let items = crate::agent::mailbox::drain_inbound_items();
            let mut approved_lines: Vec<String> = Vec::new();
            for item in items {
                match item {
                    crate::agent::mailbox::InboundItem::Deliver(line) => approved_lines.push(line),
                    crate::agent::mailbox::InboundItem::Held(letter, reply) => {
                        let shown = letter.from_name.clone();
                        let chars = letter.text.chars().count();
                        if ask_peer_mail(tx, &shown, chars, cancel).await {
                            approved_lines
                                .push(crate::agent::mailbox::format_delivery(&letter, &reply));
                        } else {
                            let _ = tx.send(AgentEvent::Status(format!(
                                "peer mail from {shown} refused"
                            )));
                        }
                    }
                }
            }
            peer_mail = crate::agent::mailbox::wrap_peer_block(&approved_lines);
            peer_mail.push_str(&crate::agent::mailbox::take_pending_peer_prompt());
            if !peer_mail.is_empty() {
                let _ = tx.send(AgentEvent::Status(
                    "peer messages arrived from another session".into(),
                ));
            }
        }
        // Drop stale TOOL-queued media (e.g. `look` ran but the turn was
        // cancelled before the attach) so it can't bleed onto this unrelated
        // prompt. USER-pasted media (Ctrl+V, /image) is kept: the paste flow
        // promises "queued for vision on your next message", and a blanket
        // take_pending_media() here silently ate every pasted image before it
        // could be attached.
        media::drop_tool_pending_media();
        // Oversized input never stops a turn: auto-register the bulk with
        // context_store (RLM prompt-as-variable, done for the user) and run
        // with a pointer + preview instead of blocking. Runs before the
        // guardrails so the size check never fires.
        let user_text: String =
            match super::context_store::maybe_spill_oversized_prompt(&session.id, user_text) {
                Some((name, replacement)) => {
                    let _ = tx.send(AgentEvent::Status(format!(
                        "input · over the inline limit - auto-registered as context var `{name}`, \
                     turn continues with a pointer + preview"
                    )));
                    replacement
                }
                None => user_text.to_string(),
            };
        // Portable input guardrails (OpenAI Agents SDK pattern) - all providers.
        // Size is handled above; what remains here are safety refusals.
        match super::guardrails::check_input(&user_text) {
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
        session.push_user(&user_text);
        // Peer mail from other sessions (real receipt + authority boundary) is
        // prepended so it arrives mid-turn; see the registration block above.
        let prompt_text = if peer_mail.is_empty() {
            user_text.clone()
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
        let prompt_ctx = PromptContext::build_with_opts_async(
            &self.cwd,
            self.is_subagent,
            &self.config.model,
            &provider_label,
            self.config.poor_mode || limited_ctx,
            Some(user_text.as_str()),
            &self.config.reasoning_effort,
        )
        .await
        .map_err(|e| NurError::Other(format!("prompt context worker failed: {e}")))?;
        // `ultrathink` in the user's prose: the strongest effort this route
        // accepts, for this turn only (an `ultracode` setting already is).
        let mut rules = super::stream_rules::RuleWatch::load(&self.cwd, &self.config.stream_rules);
        let advisor = if self.is_subagent {
            None
        } else {
            match super::advisor::Advisor::for_session(&self.config, &self.client) {
                Ok(advisor) => advisor
                    .map(|a| a.with_review_rules(&self.cwd, self.config.instructions.max_chars)),
                Err(error) => {
                    let _ = tx.send(AgentEvent::Status(format!(
                        "advisor off this turn · {error}"
                    )));
                    None
                }
            }
        };
        // The advisor pushes back on an answer once per turn.
        let mut advisor_reviewed_answer = false;
        if !rules.invalid.is_empty() {
            let _ = tx.send(AgentEvent::Status(format!(
                "stream rules skipped · {}",
                rules.invalid.join(" · ")
            )));
        }
        let ultrathink = !self.is_subagent
            && super::keywords::has_keyword(&user_text, super::keywords::ULTRATHINK)
            && crate::providers::canonical_effort(&self.config.reasoning_effort) != "ultracode";
        let turn_effort = if ultrathink {
            let _ = tx.send(AgentEvent::Status(
                "ultrathink · strongest reasoning effort for this turn".into(),
            ));
            "max".to_string()
        } else {
            self.config.reasoning_effort.clone()
        };
        if prompt_ctx.has_skill_activation() {
            let label = prompt_ctx.skill_activation_label().unwrap_or("skill");
            let _ = tx.send(AgentEvent::Status(format!(
                "{label} · activated from your wording (no slash command needed)"
            )));
        }
        // The activated skill body joins the conversation once, right after
        // the user's message, so it is cached with the history instead of
        // re-read on every round as part of a changed system prompt.
        if let Some(item) = prompt_ctx.activation_item() {
            session.input_items.push(item);
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
        let mut last_compact_input: u64 = 0;
        // Staged context-limit recovery markers (promotion → model
        // compaction → local minimal trim) so a size rejection never ends
        // the turn before every stage has run.
        let mut emergency_compact_attempted = false;
        let mut emergency_local_trim_done = false;
        let mut recovery_window: Option<u64> = None;
        let mut minimum_window_stalls: u32 = 0;
        // Preflight context-window recoveries (compact + retry instead of
        // dying) for this user turn.
        let mut preflight_recoveries: u8 = 0;
        // Codex/ChatGPT free (and some hosts) sometimes emit only a reasoning
        // summary and zero tool calls / zero answer text. Retry with a
        // nudge and tool_choice=required, backing off on repeated stalls.
        let mut empty_tool_stalls: u64 = 0;
        // How many times the skill layer has corrected this turn. Bounded so a
        // triggered skill can steer the work, not nag it.
        let mut skill_nudges: u8 = 0;
        let mut force_tool_choice = false;
        let mut recovered_default_model: Option<String> = None;
        // OMP contextPromotion: try a larger-window sibling once before compact.
        let mut context_promoted = false;
        // A text-only model rejecting replayed images must not end the turn.
        let mut media_stripped = false;

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
                let reported = if last.input_tokens > 0 {
                    last.input_tokens
                } else {
                    last.total_tokens
                };
                reported.max(estimate_context_tokens(&session.input_items))
            };
            if input_now > last_compact_input
                && should_auto_compact(
                    usage,
                    &self.config,
                    recovered_default_model
                        .as_deref()
                        .unwrap_or(&self.config.model),
                    self.client.uses_oauth_session(),
                    &session.input_items,
                )
            {
                last_compact_input = input_now;
                let before_compact = estimate_context_tokens(&session.input_items);
                let _ = tx.send(AgentEvent::Status("auto-compacting context…".into()));
                match compact_session_with_mode(self, session, usage, CompactionMode::Pressure)
                    .await
                {
                    Ok(_) => {
                        if estimate_context_tokens(&session.input_items) < before_compact {
                            last_compact_input = 0;
                        }
                        let _ =
                            tx.send(AgentEvent::Status("context compacted — continuing".into()));
                    }
                    Err(e) => {
                        // Count the failure but stay eligible: a later attempt,
                        // after more growth, may well succeed.
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

            // Background jobs (shell handoffs, bg run) that finished since the
            // last round are announced once here, so the model learns about
            // them without polling and can pull the full result when it wants.
            // Subagents leave the drain to the parent so a completion is not
            // consumed by a child that never asked for the job.
            if !self.is_subagent {
                if let Some(note) = crate::bg_jobs::completion_note_for(Some(&session.id)) {
                    session.input_items.push(user_text_item(&note));
                    self.persist_session(session);
                    let _ = tx.send(AgentEvent::Status(
                        "bg · job completion injected mid-turn".into(),
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
            let rendered =
                prompt_ctx.render(mode_now, &self.tools.todos_snapshot().render(), &session.id);
            let turn_state_item = rendered.turn_state_item();
            let mut instructions = rendered.system;

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

            // The same tool roster on every request of the session: schemas
            // sit at the head of the cached prefix, so a set that differs
            // between two requests re-bills them and everything after them.
            //
            // The lean profile is the one exception, and only by growing: a
            // tool loaded through `tool_search` joins the end of the roster
            // from the next request on and stays for the rest of the session.
            let mut tools = if self.is_subagent {
                let mut defs = self.tools.subagent_tool_defs();
                if self.interactive_children {
                    if let Some(question) = self.tools.full_tool_defs().into_iter().find(|tool| tool.name == "question") {
                        defs.push(question);
                    }
                }
                defs
            } else if self.config.tools.is_core() {
                self.tools.core_tool_defs()
            } else if self.config.tools.is_lean() {
                let loaded = crate::tools::tool_search::loaded_from_history(&session.input_items);
                self.tools.lean_tool_defs(&loaded)
            } else {
                self.tools.full_tool_defs()
            };
            // Opt-in narrowing, first round only: later rounds keep the full
            // surface, so a tool once offered is never withdrawn mid-turn.
            if turns <= 1 {
                tools = self.typesafe_narrow_tools(tools, &user_text, tx).await;
            }
            let output_reserve = if let Some(window) = recovery_window {
                fit_recovery_request(
                    session,
                    &mut instructions,
                    &mut tools,
                    window,
                    self.config.request_output_reserve_tokens,
                )
            } else {
                self.config.request_output_reserve_tokens
            };
            // The live state (mode, todos, inventories) is the last input item
            // of this request only: it is not persisted, so the next request's
            // history is still an exact extension of this one's cached prefix.
            let mut request_items = session.input_items.clone();
            if let Some(item) = turn_state_item {
                request_items.push(item);
            }
            let attribution = attribute_request(&instructions, &tools, &request_items);
            if let Some(block) = preflight_request_budget(
                &self.config,
                usage,
                &self.config.provider,
                &effective_model,
                attribution.estimated_input_tokens(),
                output_reserve,
            ) {
                match block {
                    // Over the model's context window: this is recoverable,
                    // not a budget stop. Compact (model-assisted, falling
                    // back to a local bounded trim) and retry the request
                    // - never kill the session for something compaction can
                    // fix, on any provider. Bounded so a pathological
                    // estimate cannot spin the loop.
                    PreflightBlock::ContextWindow(_) if recovery_window.is_some() => {
                        // The fitted envelope is already minimal. Send it so a
                        // provider rejection can reduce the recovery target;
                        // never repeat the same local estimate forever.
                    }
                    PreflightBlock::ContextWindow(reason)
                        if preflight_recoveries < MAX_PREFLIGHT_RECOVERIES =>
                    {
                        preflight_recoveries += 1;
                        let _ = tx.send(AgentEvent::Status(format!(
                            "request preflight: {reason} - compacting before sending"
                        )));
                        match compact_session_with_mode(
                            self,
                            session,
                            usage,
                            CompactionMode::Pressure,
                        )
                        .await
                        {
                            Ok(_) => {
                                let _ = tx.send(AgentEvent::Status(
                                    "context compacted - retrying request".into(),
                                ));
                            }
                            Err(compact_error) => {
                                let kept = emergency_compact_session(self, session);
                                let _ = tx.send(AgentEvent::Status(format!(
                                    "model compaction failed ({compact_error}); recovered a \
                                         valid {kept}-item recent context locally - retrying"
                                )));
                            }
                        }
                        continue;
                    }
                    // Model-assisted recoveries exhausted (tiny window or a
                    // stubborn estimate). Hard-trim to a minimal recent
                    // working set and send anyway: a size estimate is never
                    // a turn-stopper. If the provider still rejects, the
                    // context-limit recovery path below takes over.
                    PreflightBlock::ContextWindow(reason) => {
                        emergency_compact_session(self, session);
                        emergency_local_trim_done = true;
                        recovery_window = Some(self.config.context_window.saturating_mul(3) / 4);
                        let _ = tx.send(AgentEvent::Status(format!("request preflight: {reason} - fitting retained context and output reservation")));
                        continue;
                    }
                    PreflightBlock::Budget(msg) => {
                        // The only preflight that ends a turn: an explicit
                        // user-set budget cap.
                        let _ = tx.send(AgentEvent::Status(msg.clone()));
                        return Err(NurError::Budget(msg));
                    }
                }
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
                let notice = format!("provider capability profile · {}", limits.join(" · "));
                // The profile is a property of the route, not the request: a
                // headless log was getting it before every call.
                if first_time(&notice) {
                    let _ = tx.send(AgentEvent::Status(notice));
                }
            }

            // Effort rungs differ per provider (and, for Claude, per model) and
            // keep being added. Send what this route actually accepts: clamped
            // to its nearest rung, or omitted where there is no effort control.
            let effort = crate::providers::nearest_effort(
                &self.config.provider,
                &effective_model,
                &turn_effort,
            );
            let req = ResponseRequest {
                model: effective_model,
                input: Value::Array(request_items),
                instructions: Some(instructions),
                tools: Some(tools.clone()),
                tool_choice: Some(tool_choice.into()),
                store: Some(false),
                include: Some(vec!["reasoning.encrypted_content".into()]),
                reasoning: Some(ReasoningConfig {
                    effort,
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
                prompt_cache_key: Some(session.provider_session_id().to_string()),
                max_output_tokens: (output_reserve > 0 && primary_output_limit)
                    .then_some(output_reserve),
            };

            let (resp, text_deltas, served): (ApiResponse, usize, Served) = match self
                .request_with_failover(&req, usage, tx, cancel, &rules)
                .await
            {
                Ok(response) => response,
                Err(NurError::StreamRule { name, discard }) => {
                    if discard > 0 {
                        let _ = tx.send(AgentEvent::StreamRestart { discard });
                    }
                    if let Some(rule) = rules.rule(&name) {
                        session
                            .input_items
                            .push(super::stream_rules::injection_item(rule));
                    }
                    rules.fired.insert(name.clone());
                    let _ = tx.send(AgentEvent::Status(format!(
                        "stream rule `{name}` fired - answering again with it"
                    )));
                    continue;
                }
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
                    // Provider rejected the request as over-window.
                    // Recovery is staged and a size rejection never ends
                    // the turn on its own:
                    //   1. OMP contextPromotion to a larger sibling (once,
                    //      handled above)
                    //   2. model-assisted compaction (once)
                    //   3. local trim to a minimal recent working set
                    // Repeated size rejections shrink the full envelope and
                    // output reservation; cancellation and explicit budgets
                    // remain available throughout recovery.
                    if emergency_local_trim_done {
                        let previous = recovery_window.unwrap_or(self.config.context_window);
                        recovery_window = Some((previous / 2).max(256));
                        let _ = tx.send(AgentEvent::Status(
                            "context recovery - reducing retained payload and output reservation"
                                .into(),
                        ));
                        if previous <= 256 {
                            minimum_window_stalls = minimum_window_stalls.saturating_add(1);
                            let seconds = (2_u64 << minimum_window_stalls.saturating_sub(1).min(5)).min(60);
                            tokio::select! { _ = cancel.cancelled() => return Err(NurError::Interrupted), _ = tokio::time::sleep(std::time::Duration::from_secs(seconds)) => {} }
                        }
                        continue;
                    }
                    if !emergency_compact_attempted {
                        emergency_compact_attempted = true;
                        let _ = tx.send(AgentEvent::Status(
                            "provider rejected the context window - compacting and retrying".into(),
                        ));
                        match compact_session_with_mode(
                            self,
                            session,
                            usage,
                            CompactionMode::Pressure,
                        )
                        .await
                        {
                            Ok(_) => {
                                let _ = tx.send(AgentEvent::Status(
                                    "emergency context compaction succeeded - continuing".into(),
                                ));
                            }
                            Err(compact_error) => {
                                // A model-assisted summary can itself exceed
                                // the provider window. Keep a valid recent
                                // working set locally so the turn continues
                                // instead of dying at exactly the point
                                // compaction is needed.
                                let kept = emergency_compact_session(self, session);
                                emergency_local_trim_done = true;
                                recovery_window =
                                    Some(self.config.context_window.saturating_mul(3) / 4);
                                let _ = tx.send(AgentEvent::Status(format!(
                                    "model compaction failed ({compact_error}); recovered a \
                                         valid {kept}-item recent context locally - continuing"
                                )));
                            }
                        }
                    } else {
                        let kept = emergency_compact_session(self, session);
                        emergency_local_trim_done = true;
                        recovery_window = Some(self.config.context_window.saturating_mul(3) / 4);
                        let _ = tx.send(AgentEvent::Status(format!(
                            "context still over the window after compaction - recovered a \
                                 minimal {kept}-item recent context locally - continuing"
                        )));
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
                // Vision rejection is transport-agnostic: only the Chat
                // Completions adapter retries on its own, so the Responses,
                // Anthropic and Gemini routes would end the turn here. History
                // carries the images, so dropping them once and retrying costs
                // one round trip and keeps the turn alive.
                Err(error)
                    if !media_stripped
                        && crate::api::chat::is_media_unsupported_error(&error.to_string()) =>
                {
                    media_stripped = true;
                    let dropped: usize = session
                        .input_items
                        .iter_mut()
                        .map(strip_inline_images)
                        .sum();
                    if dropped == 0 {
                        return Err(error);
                    }
                    let _ = tx.send(AgentEvent::Status(format!(
                        "model has no vision - retrying this turn with {dropped} inline image(s) \
                         replaced by a text note"
                    )));
                    continue;
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
            // Make the prefix cache visible per request: this is the number
            // that tells whether the stable system prompt + tool roster is
            // actually paying off on this provider.
            if in_tok > 0 && tu.cached_tokens > 0 {
                let cached = tu.cached_tokens.min(in_tok);
                let _ = tx.send(AgentEvent::Status(format!(
                    "prompt cache · {}% of input read from cache ({} of {} tok)",
                    cached.saturating_mul(100) / in_tok,
                    cached,
                    in_tok
                )));
            }
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
            // Tool-call arguments never stream, so a rule is checked against
            // the finished response too; a match drops it before any tool runs.
            if !rules.is_empty() {
                let haystack = calls
                    .iter()
                    .map(|call| call.arguments.as_str())
                    .chain(std::iter::once(text.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n");
                if let Some(rule) = rules.check_whole(&haystack) {
                    let name = rule.name.clone();
                    session
                        .input_items
                        .push(super::stream_rules::injection_item(rule));
                    rules.fired.insert(name.clone());
                    if text_deltas > 0 {
                        let _ = tx.send(AgentEvent::StreamRestart {
                            discard: text_deltas,
                        });
                    }
                    let _ = tx.send(AgentEvent::Status(format!(
                        "stream rule `{name}` fired - answering again with it"
                    )));
                    continue;
                }
            }
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
            if resp.status.as_deref() == Some("length") {
                if calls.is_empty() {
                    // Text-only truncation: safe to inject a continuation nudge
                    // (no unpaired function_call in history).
                    let _ = tx.send(AgentEvent::Status(
                        "model response truncated at max_tokens - asking to continue".into(),
                    ));
                    let nudge = if !text.trim().is_empty() {
                        "[harness] Your previous response was cut off by the provider's max_tokens limit (finish_reason: length). The user saw a truncated, incomplete answer. Continue exactly where you left off, without repeating the preamble. Finish the answer."
                    } else {
                        tokio::select! {
                            _ = cancel.cancelled() => return Err(NurError::Interrupted),
                            _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
                        }
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
                    let _ = tx.send(AgentEvent::Status(
                        "model truncated mid tool-call - running the tool, then continuing".into(),
                    ));
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
            // up where it stopped without turning a transient failure into a completion.
            if matches!(
                resp.status.as_deref(),
                Some("network_error") | Some("unknown")
            ) && calls.is_empty()
            {
                let reason = resp.status.clone().unwrap_or_default();
                let _ = tx.send(AgentEvent::Status(format!(
                    "stream ended early (finish_reason: {reason}) - asking the model to continue"
                )));
                let nudge = if text.trim().is_empty() {
                    tokio::select! {
                        _ = cancel.cancelled() => return Err(NurError::Interrupted),
                            _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
                    }
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

            if text_deltas == 0 && !text.is_empty() && !suppress_duplicate_preamble {
                let _ = tx.send(AgentEvent::AssistantMessage(text.clone()));
            } else if suppress_duplicate_preamble {
                let _ = tx.send(AgentEvent::Status(
                    "suppressed duplicate tool rationale in display; durable request transcript retained".into(),
                ));
            }

            if calls.is_empty() {
                // Skill layer: a triggered skill states requirements. Before the
                // turn is accepted as finished, the skill machinery itself
                // checks the work against them - one Jev request over this
                // turn's tool trace. A confident shortfall steers once or twice
                // with the specific rule that was skipped; it never blocks the
                // turn and never rewrites anything.
                if skill_nudges < MAX_SKILL_NUDGES
                    && self.typesafe_skill_nudge(&prompt_ctx, session, tx).await
                {
                    skill_nudges += 1;
                    self.persist_session(session);
                    continue;
                }
                if let Some(advisor) = advisor
                    .as_ref()
                    .filter(|_| !advisor_reviewed_answer && !text.trim().is_empty())
                {
                    advisor_reviewed_answer = true;
                    if let Some(note) = self
                        .consult_advisor(advisor, session, usage, tx, cancel, Some(text.as_str()))
                        .await
                    {
                        session
                            .input_items
                            .push(super::advisor::note_item(&advisor.model, &note));
                        self.persist_session(session);
                        continue;
                    }
                }
                // Reasoning-only / empty completion: model "planned" but never
                // answered or called tools. Common on ChatGPT free + Codex OAuth
                // with some gpt-5.* models. Keep retrying until there is a
                // real response, cancellation, or an explicit budget stop.
                let emptyish = text.trim().is_empty();
                if emptyish {
                    empty_tool_stalls = empty_tool_stalls.saturating_add(1);
                    if empty_tool_stalls > 3 {
                        tokio::select! {
                            _ = cancel.cancelled() => return Err(NurError::Interrupted),
                            _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
                        }
                    }
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
                        if let Some(block) = preflight_request_budget(
                            &self.config,
                            usage,
                            &self.config.provider,
                            &self.config.model,
                            extract_input_estimate,
                            self.config.request_output_reserve_tokens,
                        ) {
                            let _ = tx.send(AgentEvent::Status(format!(
                                "memory extraction skipped - {}",
                                block.message()
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
            // Rendered from ade's shared title state so a running turn keeps
            // the animated marker (no busy/idle flip-flop); subagents never
            // touch the parent session's tab.
            if !self.is_subagent {
                crate::ade::reassert_after_child();
            }
            match exec_result {
                Ok(()) => {
                    if let Some(advisor) = advisor.as_ref().filter(|advisor| advisor.every_round) {
                        if let Some(note) = self
                            .consult_advisor(advisor, session, usage, tx, cancel, None)
                            .await
                        {
                            session
                                .input_items
                                .push(super::advisor::note_item(&advisor.model, &note));
                        }
                    }
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
        let mut compact_requests = Vec::new();
        // Jev's tool gate runs once per response, over the whole call list: one
        // request in, `call_id -> skip reason` out. Everything it decides is
        // recorded here so the post-execution judge can reuse the same facts.
        let ts_skip = self.typesafe_pre_gate(calls, session, tx).await;
        let mut ts_results: Vec<TsToolResult> = Vec::new();
        while idx < calls.len() {
            if cancel.is_cancelled() {
                return Err(NurError::Interrupted);
            }

            // Contiguous parallel-safe batch. The batch skips the approval
            // gate, so a call that a deny or ask rule names takes the gated
            // path instead: a rule must hold for reads as well as writes. The
            // same goes for a first-time web_fetch domain - its prompt lives
            // in check_approval, so the call must reach it.
            let parallel = |call: &FunctionCallRef| {
                is_concurrency_safe(&call.name, &call.arguments)
                    && !matches!(
                        self.permissions.decide(&call.name, &call.arguments),
                        Some(RuleDecision::Deny | RuleDecision::Ask)
                    )
                    && !self.web_fetch_needs_domain_gate(&call.name, &call.arguments)
            };
            if parallel(&calls[idx]) {
                let mut batch_end = idx + 1;
                while batch_end < calls.len() && parallel(&calls[batch_end]) {
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
                    if let Some(reason) = ts_skip.get(&call.call_id) {
                        // Jev says this call is a confident repeat of one whose
                        // result is still in context: answer it from that
                        // knowledge instead of paying for the tool again.
                        let skip = reason.clone();
                        handles.push(crate::typesafe::context::spawn_blocking(move || {
                            (call_id, name, Ok(skip))
                        }));
                        continue;
                    }
                    let sid = session.id.clone();
                    handles.push(crate::typesafe::context::spawn_blocking(move || {
                        let res = host.dispatch(
                            &name,
                            &args,
                            &ToolContext {
                                cwd,
                                cancel: cancel_t,
                                session_id: sid,
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
                    let body = crate::typesafe::context::spawn_blocking(move || {
                        crate::headroom::prepare_tool_body(
                            &hr, &sid, &tname, body, ok, spill_max, &model,
                        )
                    })
                    .await
                    .unwrap_or_else(|e| format!("error: headroom task failed: {e}"));
                    let body = if ok {
                        self.with_nested_instructions(
                            &session.id,
                            &name,
                            &call_arguments(calls, &call_id),
                            body,
                        )
                    } else {
                        body
                    };
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
                    // A call the gate answered from context was never run, so
                    // there is no result to judge: sending our own skip note to
                    // Jev would spend tokens to grade a sentence we wrote.
                    if !ts_skip.contains_key(&call_id) {
                        ts_results.push(TsToolResult {
                            tool: name.clone(),
                            args: call_arguments(calls, &call_id),
                            body: body.clone(),
                            ok,
                        });
                    }
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
            if calls[idx].name == "agent" && self.subagent_depth < self.config.subagent_depth.max(1)
            {
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

            // `question` is answered by the user through a modal, not by
            // dispatch: asking permission to ask is absurd, and it must never
            // ride the parallel batch (it is not parallel-safe, so it lands
            // here) or the Jev pre-gate (it is a meta tool).
            // `secret` requests are the same: the user answers in a masked modal.
            let secret_request = call.name == "secret"
                && serde_json::from_str::<serde_json::Value>(&call.arguments)
                    .ok()
                    .and_then(|v| v.get("action").and_then(|a| a.as_str()).map(String::from))
                    .as_deref()
                    == Some("request");
            if call.name == "question" || secret_request {
                *tool_seq += 1;
                let id = *tool_seq;
                let _ = tx.send(AgentEvent::ToolStart {
                    id,
                    name: call.name.clone(),
                    args: call.arguments.clone(),
                });
                let (body, ok) = if secret_request {
                    self.run_secret_request(call, tx, cancel).await?
                } else {
                    self.run_question_call(call, session, tx, cancel).await?
                };
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
                continue;
            }

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
                let plan_block = mode_at_gate == PermissionMode::Plan
                    && matches!(
                        plan_gate_classify(&call.name, &call.arguments, {
                            is_read_only_call(&call.name, &call.arguments)
                        })
                        .0,
                        PlanGate::Deny
                    );
                let (msg, result_label) = if plan_block {
                    (
                        format!(
                            "blocked in plan mode — {} needs manual/auto (Shift+Tab). \
                             Plan allows reads and a known read-only shell set (other shell \
                             asks first); it blocks code edits and repo/VCS commits. \
                             Describe the change instead, or ask the user to switch mode.",
                            call.name
                        ),
                        "blocked · plan mode".into(),
                    )
                } else if self.permissions.decide(&call.name, &call.arguments)
                    == Some(RuleDecision::Deny)
                {
                    (
                        "denied by a permissions.toml rule - this call is off limits; \
                         continue without it"
                            .into(),
                        "denied · permissions.toml".into(),
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

            // A transcript can only be compacted after every call from this
            // response has an output. Defer it until the entire batch is paired.
            let requests_compact = call.name == "typesafe"
                && serde_json::from_str::<Value>(&call.arguments)
                    .ok()
                    .and_then(|v| v.get("action").and_then(Value::as_str).map(str::to_owned))
                    .as_deref()
                    == Some("compact");
            if requests_compact {
                let body = "Compaction requested; it will run after this tool batch finishes.";
                compact_requests.push(call.call_id.clone());
                let _ = tx.send(AgentEvent::ToolEnd {
                    id,
                    name: call.name.clone(),
                    result: body.into(),
                    ok: true,
                });
                session
                    .input_items
                    .push(function_call_output_item(&call.call_id, body));
                idx += 1;
                continue;
            }

            usage.set_state(format!("tool:{}", call.name));

            let (body, ok) = if let Some(reason) = ts_skip.get(&call.call_id) {
                // Gated by Jev: an answer already in context, so the call is
                // not dispatched. Reported as the tool's own output.
                (reason.clone(), true)
            } else if call.name == "agent" {
                if self.subagent_depth >= self.config.subagent_depth.max(1) {
                    (
                        format!(
                            "error: nested subagents are not allowed (depth {} >= max {}) - raise config.subagent_depth to allow deeper recursion",
                            self.subagent_depth,
                            self.config.subagent_depth.max(1)
                        ),
                        false,
                    )
                } else {
                    let context = route_context(&self.config, &session.input_items);
                    match run_agent_tool(self, call, &session.id, context, cancel, tx).await {
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
                let mut recorded_undo: Option<std::path::PathBuf> = None;
                if matches!(
                    call.name.as_str(),
                    "write_file" | "edit_file" | "multi_edit" | "apply_patch" | "hashline_edit"
                ) {
                    if let Ok(v) = serde_json::from_str::<Value>(&call.arguments) {
                        if let Some(p) = v.get("path").and_then(|p| p.as_str()) {
                            if let Ok(abs) = crate::tools::resolve_path(&self.cwd, p) {
                                crate::tools::undo::record(&session.id, &abs);
                                recorded_undo = Some(abs);
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
                let sid = session.id.clone();
                let exec = crate::typesafe::context::spawn_blocking(move || {
                    host.dispatch(
                        &name,
                        &args,
                        &ToolContext {
                            cwd,
                            cancel: cancel_t,
                            session_id: sid,
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
                if let Some(abs) = recorded_undo {
                    if ok {
                        crate::tools::undo::seal(&session.id, &abs);
                    } else {
                        let _ = crate::tools::undo::drop_last(&session.id);
                    }
                }
                (body, ok)
            };

            let body = if ok {
                let hr = self.config.headroom.clone();
                let sid = session.id.clone();
                let tname = call.name.clone();
                let model = self.config.model.clone();
                let spill_max = self.config.tool_result_max_chars as usize;
                let body = crate::typesafe::context::spawn_blocking(move || {
                    crate::headroom::prepare_tool_body(
                        &hr, &sid, &tname, body, true, spill_max, &model,
                    )
                })
                .await
                .unwrap_or_else(|e| format!("error: headroom task failed: {e}"));
                self.with_nested_instructions(&session.id, &call.name, &call.arguments, body)
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
            if !ts_skip.contains_key(&call.call_id) {
                ts_results.push(TsToolResult {
                    tool: call.name.clone(),
                    args: call.arguments.clone(),
                    body: body.clone(),
                    ok,
                });
            }
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
        // One Jev request judges every result from this response: did it do
        // what the call was for? Reported, never rewritten - the bodies stay
        // verbatim.
        self.typesafe_judge_results(&ts_results, session, tx).await;
        // A `goal` action=complete claim in this response is checked part by
        // part against the transcript before it counts.
        self.maybe_verify_goal_completion(&ts_results, session, tx)
            .await;
        if !compact_requests.is_empty() {
            let result =
                compact_session_with_mode(self, session, usage, CompactionMode::Model).await;
            let report = match result {
                Ok(s) => compact_receipt_detail(&s),
                Err(e @ (NurError::Budget(_) | NurError::Interrupted)) => return Err(e),
                Err(e) => format!("compaction failed: {e}"),
            };
            let report = crate::secrets::redact(&report).into_owned();
            let _ = tx.send(AgentEvent::Status(report.clone()));
            let mut found = false;
            for item in &mut session.input_items {
                if item.get("type").and_then(Value::as_str) == Some("function_call_output")
                    && item
                        .get("call_id")
                        .and_then(Value::as_str)
                        .is_some_and(|id| compact_requests.iter().any(|requested| requested == id))
                {
                    item["output"] = Value::String(report.clone());
                    found = true;
                }
            }
            // Summary compaction replaces the call pair. Tell the model the
            // outcome without creating an orphaned tool output.
            if !found {
                session
                    .input_items
                    .push(user_text_item(&format!("[harness] {report}")));
            }
        }
        Ok(())
    }

    /// Verify a `goal` action=complete claim against transcript evidence.
    ///
    /// Fires only when this response actually completed the tracked goal: one
    /// batched Jev request (hosted or local engine, whichever is configured)
    /// judges every part, and only a *confident* not-done reopens the goal
    /// with the verdict fed back as a harness message. No judgment layer, a
    /// failed request, or an uncertain verdict accepts the claim - the same
    /// confidence policy the rest of the harness uses.
    async fn maybe_verify_goal_completion(
        &self,
        results: &[TsToolResult],
        session: &mut Session,
        tx: &mpsc::UnboundedSender<AgentEvent>,
    ) {
        let completed_here = results.iter().any(|r| {
            r.tool == "goal"
                && r.ok
                && super::goal::args_action(&r.args).as_deref() == Some("complete")
        });
        if !completed_here {
            return;
        }
        let goal = match super::goal::load(&session.id) {
            Some(g) if g.status == super::goal::GoalStatus::Completed => g,
            _ => return,
        };
        let cfg = self.config.typesafe.clone();
        if !cfg.enabled || crate::typesafe::harness::ready(&cfg).is_none() {
            let _ = tx.send(AgentEvent::Status(
                "goal completed - no judgment layer active, claim accepted \
                 unverified (set a Jev key or start a local engine for verified completion)"
                    .into(),
            ));
            return;
        }
        let parts = super::goal::split_parts(&goal.text);
        let evidence = super::goal::completion_evidence(&session.input_items, 12_000);
        let state = serde_json::json!({ "goal": goal.text, "evidence": evidence });
        let verdict = crate::typesafe::context::spawn_blocking(move || {
            super::goal::verify_completion(&cfg, &parts, &state)
        })
        .await
        .unwrap_or_else(|_| super::goal::GoalVerification { parts: Vec::new() });
        if verdict.unavailable() || verdict.parts.is_empty() {
            let _ = tx.send(AgentEvent::Status(
                "goal completed - verification request failed, claim accepted unverified".into(),
            ));
            return;
        }
        let failures = verdict.confident_failures();
        if verdict.passed() {
            if !verdict.verified() {
                let _ = tx.send(AgentEvent::Status(format!(
                    "goal completed - claim accepted unverified ({}; uncertain or missing judgments)",
                    verdict.summary()
                )));
                return;
            }
            let _ = tx.send(AgentEvent::Status(format!(
                "goal completed - Jev verified {}",
                verdict.summary()
            )));
            // Ledger entry (AgentRun-style trace): what was verified, against
            // what evidence, so later runs can learn from this one.
            let scope = Self::goal_ledger_scope(session);
            let _ = super::chronicle::append(
                &scope,
                "goal_complete",
                &format!(
                    "goal completed ({}) · evidence: {} tool item(s) in transcript",
                    verdict.summary(),
                    session.input_items.len(),
                ),
                None,
            );
            // Learning note: file the win as an evidence-backed lesson draft
            // (code-built facts only, never model prose), unless this goal is
            // already noted. Rollback via /refine management if it misleads.
            let tools = super::goal::tools_used(&session.input_items);
            let (lesson, lesson_evidence) = super::goal::completion_lesson(
                &goal.text,
                &verdict.summary(),
                &tools,
                goal.tokens_used,
                goal.continuation_count,
            );
            let already_noted = super::harness::load(&session.id)
                .supplemental
                .contains(&goal.text.chars().take(40).collect::<String>());
            if !already_noted
                && super::harness::refine(&session.id, &lesson, &lesson_evidence).is_ok()
            {
                let _ = tx.send(AgentEvent::Status(
                    "lesson filed from this goal (/refine management rolls it back)".into(),
                ));
            }
            return;
        }
        // Confidently not done: reopen and show exactly which parts failed,
        // so the next round works the gap instead of re-claiming.
        let detail = failures
            .iter()
            .map(|(part, conf)| format!("- {part} (confidently not done, conf {conf:.2})"))
            .collect::<Vec<_>>()
            .join("\n");
        let _ = super::goal::reopen(&session.id);
        let scope = Self::goal_ledger_scope(session);
        let _ = super::chronicle::append(
            &scope,
            "goal_rejected",
            &format!(
                "completion claim rejected ({} failed) · evidence: {} tool item(s)",
                failures.len(),
                session.input_items.len(),
            ),
            None,
        );
        session.input_items.push(user_text_item(&format!(
            "[harness] Completion rejected: Jev judged these goal parts NOT done \
             against the transcript evidence:\n{detail}\nKeep working the failed \
             parts with your tools. Call `goal` action=complete again only when \
             every part is done and verified, or end with BLOCKED: <what you need> \
             if only the user can unblock you."
        )));
        let _ = tx.send(AgentEvent::Status(format!(
            "goal completion rejected by Jev verification ({} failed) - goal reopened",
            failures.len()
        )));
    }

    /// Chronicle scope for goal ledger entries: same project:session namespacing
    /// the memory tiers use, so a later run can read what past goals verified.
    fn goal_ledger_scope(session: &Session) -> String {
        let proj = std::path::Path::new(&session.cwd)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("workspace");
        format!("{proj}:{}", session.id)
    }

    /// Drop specialist tools the task confidently does not need.
    ///
    /// One Noul per specialist ("does this task need tool X?"), batched into a
    /// single request. The direction of the bet matters: a wrong *keep* costs
    /// schema tokens, a wrong *drop* costs a round trip and can strand the turn -
    /// so a tool is removed only when the answer is confident it is NOT needed,
    /// and everything core, everything already offered, and every escape hatch is
    /// kept unconditionally.
    ///
    /// Returns `tools` untouched when the feature is off, no judgment layer is
    /// available, or nothing could be placed confidently.
    async fn typesafe_narrow_tools(
        &self,
        tools: Vec<crate::api::types::ToolDef>,
        task: &str,
        tx: &mpsc::UnboundedSender<AgentEvent>,
    ) -> Vec<crate::api::types::ToolDef> {
        let cfg = self.config.typesafe.clone();
        if !cfg.enabled || !cfg.tools.subset || tools.len() <= 8 {
            return tools;
        }
        if crate::typesafe::harness::ready(&cfg).is_none() {
            return tools;
        }
        // Candidates: the specialists beyond the always-present core. The list is
        // built here, so the answer can only select from real, offered tools.
        let specialists: Vec<String> = tools
            .iter()
            .filter(|t| !crate::tools::ROOT_CORE_TOOL_NAMES.contains(&t.name.as_str()))
            .map(|t| t.name.clone())
            .collect();
        if specialists.is_empty() {
            return tools;
        }
        let keep_floor = cfg.tools.keep_probability.clamp(0.5, 0.99);
        let cfg_for_task = cfg.clone();
        let task_owned = task.to_string();
        // Name plus a one-line description: picking from a named list measured
        // 90% on names alone and 100% with one line each (jev-playground fit.md).
        let listed: Vec<Value> = tools
            .iter()
            .filter(|t| specialists.contains(&t.name))
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "does": one_line_description(t.description.as_deref().unwrap_or("")),
                })
            })
            .collect();
        let asked = specialists.clone();
        let judged = match crate::typesafe::context::blocking(move || {
            let state = serde_json::json!({
                "task": crate::typesafe::harness::judge_preview(&task_owned, 2_000),
                "note": "Each question is about one tool below; answer yes when the task needs it",
                "tools": listed,
            });
            crate::typesafe::harness::tool_need(&cfg_for_task, &state, &asked)
        })
        .await
        {
            Ok(j) => j,
            Err(_) => return tools,
        };
        if judged.is_empty() {
            return tools;
        }
        let mut dropped: Vec<String> = Vec::new();
        for (name, needed) in &judged {
            // `None` from the policy layer means "no confident answer" (escalate
            // or confirm band) - keep the tool.
            if let Some(p) = needed {
                // `tool_need` only ever hands back a banded probability, so a call
                // it *needed* can arrive as 0.93. Comparing that against a raised
                // `keep_floor` inverted the rule and dropped tools the judgment had
                // just said were needed; only the low mode may drop.
                if *p <= 1.0 - keep_floor {
                    dropped.push(name.clone());
                }
            }
        }
        if dropped.is_empty() {
            return tools;
        }
        let before = tools.len();
        let kept: Vec<crate::api::types::ToolDef> = tools
            .into_iter()
            .filter(|t| !dropped.contains(&t.name))
            .collect();
        let _ = tx.send(AgentEvent::Status(format!(
            "typesafe · tool surface {} → {} for this round (dropped {})",
            before,
            kept.len(),
            dropped.join(", ")
        )));
        crate::typesafe::telemetry::record_decision(
            Some(keep_floor),
            true,
            cfg.escalate_confidence,
        );
        kept
    }

    /// Ask Jev about this response's tool calls **before** any of them run.
    ///
    /// Returns `call_id -> reason` for calls the harness should not dispatch.
    /// Everything else - risk, "a person should see this" - is surfaced in the
    /// transcript and never enforced: the gate removes work that is provably
    /// redundant, and nothing more.
    async fn typesafe_pre_gate(
        &self,
        calls: &[FunctionCallRef],
        session: &Session,
        tx: &mpsc::UnboundedSender<AgentEvent>,
    ) -> std::collections::HashMap<String, String> {
        let cfg = self.config.typesafe.clone();
        if !cfg.enabled || !cfg.tool_gate.enabled || calls.is_empty() {
            return std::collections::HashMap::new();
        }
        if crate::typesafe::harness::ready(&cfg).is_none() {
            return std::collections::HashMap::new();
        }
        let facts = typesafe_call_facts(calls, &session.input_items);
        let preview_chars = cfg.tool_gate.preview_chars;
        // Only the calls worth judging. `judged` maps a question index back to
        // its position in `calls`, so a filtered list cannot misalign answers.
        let judged_calls: Vec<(usize, &FunctionCallRef)> = calls
            .iter()
            .enumerate()
            .filter(|(_, c)| typesafe_judges_tool(&c.name))
            .collect();
        if judged_calls.is_empty() {
            return std::collections::HashMap::new();
        }
        let items: Vec<crate::typesafe::harness::ToolCallItem> = judged_calls
            .iter()
            .enumerate()
            .map(|(n, (i, c))| crate::typesafe::harness::ToolCallItem {
                index: n,
                tool: c.name.clone(),
                args_preview: crate::typesafe::harness::judge_preview(&c.arguments, preview_chars),
                intent: None,
                result_preview: None,
                duplicate_of: facts[*i].0,
                prior_failures: facts[*i].1,
            })
            .collect();
        let goal = typesafe_goal(session, 3);
        let cfg_for_task = cfg.clone();
        let judged = match crate::typesafe::context::spawn_blocking(move || {
            let state =
                serde_json::json!({ "goal": goal, "pending": "these calls have not run yet" });
            crate::typesafe::harness::judge_calls(
                &cfg_for_task,
                &state,
                &items,
                crate::typesafe::harness::JudgeScope::Pre,
            )
        })
        .await
        {
            Ok(j) => j,
            Err(_) => return std::collections::HashMap::new(),
        };
        let mut skip = std::collections::HashMap::new();
        for (n, j) in judged.iter().enumerate() {
            let Some((_, call)) = judged_calls.get(n) else {
                continue;
            };
            if cfg.tool_gate.flag_risky {
                // Surfacing only: these use the confirm band, because a notice
                // the user can ignore must not require near-certainty. Nothing
                // here blocks or changes the call.
                if let Some(band) = j.risk_notice() {
                    let _ = tx.send(AgentEvent::Status(format!(
                        "typesafe · {} risk {} ({})",
                        call.name,
                        band.as_str(),
                        j.risk
                            .confidence
                            .map(|c| format!("{c:.2}"))
                            .unwrap_or_else(|| "-".into())
                    )));
                }
                if j.wants_human_notice() {
                    let _ = tx.send(AgentEvent::Status(format!(
                        "typesafe · {} may need you (p={}) - flagging, not blocking",
                        call.name,
                        j.needs_human
                            .confidence
                            .map(|c| format!("{c:.2}"))
                            .unwrap_or_else(|| "-".into())
                    )));
                }
            }
            let reason = if cfg.tool_gate.skip_redundant && j.gate_skip() {
                // Higher-stakes calls need more certainty before the harness
                // answers them from memory: thresholds scale with the risk band.
                let base = crate::typesafe::policy::Thresholds::from_config(&cfg);
                let stakes = j.risk.raw().map(|b| b.stakes()).unwrap_or(0.0);
                let scaled = base.scaled(stakes);
                let confident = j
                    .redundant
                    .confidence
                    .map(|c| c >= scaled.act)
                    .unwrap_or(false);
                confident.then(|| {
                    format!(
                        "skipped by typesafe: this call repeats an identical earlier call whose \
                         result is still in this conversation, and the judgment was confident \
                         (p={:.2}). Re-read that earlier result, or change the arguments if \
                         something has changed.",
                        j.redundant.confidence.unwrap_or(0.0)
                    )
                })
            } else if cfg.tool_gate.skip_repeated_failures && j.stuck() {
                Some(
                    "skipped by typesafe: this is the same call that already failed here, and \
                     the judgment says nothing relevant changed. Something else has to change \
                     first."
                        .to_string(),
                )
            } else {
                None
            };
            if let Some(reason) = reason {
                let _ = tx.send(AgentEvent::Status(format!(
                    "typesafe · skipping {} · {}",
                    call.name,
                    reason.lines().next().unwrap_or("")
                )));
                crate::typesafe::telemetry::record_gated_call();
                skip.insert(call.call_id.clone(), reason);
            }
        }
        skip
    }

    /// The skill layer's own usage check.
    ///
    /// A triggered skill is supposed to be followed. This asks Jev one question
    /// per activated skill - was it carried out, judged only from this turn's
    /// tool trace - and on a shortfall asks which of the skill's own stated
    /// requirements was skipped, then injects that as a mid-turn steer.
    ///
    /// Returns `true` when a steer was injected (the caller re-runs the model).
    /// Nothing is ever blocked or rewritten: the skill machinery corrects the
    /// work inside the loop, which is where it belongs.
    /// One advisor review, metered under its own route; the note, if it left
    /// one. A failed request is a status line and no note.
    async fn consult_advisor(
        &self,
        advisor: &super::advisor::Advisor,
        session: &mut Session,
        usage: &mut UsageTracker,
        tx: &mpsc::UnboundedSender<AgentEvent>,
        cancel: &CancellationToken,
        answer: Option<&str>,
    ) -> Option<String> {
        let request = advisor.request(&typesafe_goal(session, 2), &session.input_items, answer);
        let _ = tx.send(AgentEvent::Status(format!(
            "advisor · {} reviewing",
            advisor.model
        )));
        let response = tokio::select! {
            response = advisor.client.create_response(&request) => response,
            _ = cancel.cancelled() => return None,
        };
        match response {
            Ok(response) => {
                usage.record_request_as(
                    &advisor.provider,
                    &advisor.model,
                    response.accounting_usage(),
                    response.id.clone(),
                    "advisor",
                );
                let metered = usage.last_usage().clone();
                session.usage.add(&metered);
                let _ = tx.send(AgentEvent::Usage {
                    session: usage.session_usage().clone(),
                    last: metered,
                });
                let note = super::advisor::parse_note(&response.output_text())?;
                let _ = tx.send(AgentEvent::Advice {
                    model: advisor.model.clone(),
                    note: note.clone(),
                });
                Some(note)
            }
            Err(error) => {
                let _ = tx.send(AgentEvent::Status(format!("advisor skipped · {error}")));
                None
            }
        }
    }

    async fn typesafe_skill_nudge(
        &self,
        prompt_ctx: &super::prompt::PromptContext,
        session: &mut Session,
        tx: &mpsc::UnboundedSender<AgentEvent>,
    ) -> bool {
        let cfg = self.config.typesafe.clone();
        if !cfg.enabled || !cfg.skills.enabled || !cfg.skills.check_usage {
            return false;
        }
        let Some((skill, requirements, path)) = prompt_ctx.active_skill() else {
            return false;
        };
        let path = path.to_string();
        if requirements.is_empty() || crate::typesafe::harness::ready(&cfg).is_none() {
            return false;
        }
        let skill = skill.to_string();
        let requirements: Vec<String> = requirements.to_vec();
        let request = typesafe_goal(session, 2);
        let evidence = typesafe_trace_evidence(&session.input_items, 24);
        let cfg_for_task = cfg.clone();
        let skill_for_task = skill.clone();
        let (verdict, missed) = crate::typesafe::context::blocking(move || {
            let v = crate::typesafe::harness::judge_skill_use(
                &cfg_for_task,
                &request,
                &skill_for_task,
                &requirements,
                &evidence,
            );
            let m = if v.usable().is_some_and(|c| c.is_shortfall()) {
                crate::typesafe::harness::missed_requirement(
                    &cfg_for_task,
                    &request,
                    &skill_for_task,
                    &requirements,
                    &evidence,
                )
                .usable()
                .cloned()
            } else {
                None
            };
            (v, m)
        })
        .await
        .unwrap_or_else(|_| {
            (
                crate::typesafe::policy::Judgment::unavailable("skill_use", "task failed"),
                None,
            )
        });
        let Some(compliance) = verdict.usable().copied() else {
            return false;
        };
        if !compliance.is_shortfall() {
            return false;
        }
        let missed_line = missed
            .map(|m| format!("\nThe requirement that was not carried out:\n> {m}"))
            .unwrap_or_default();
        let steer = format!(
            "[harness · skill check] **{skill}** is active for this request, and the work so \
             far does not follow it ({compliance}{missed_line}).\n\
             Do the skill's required steps now, in its stated order, before answering. \
             If a rule genuinely does not apply to this request, say which and why.",
            compliance = compliance.as_str(),
        );
        let _ = tx.send(AgentEvent::Status(format!(
            "typesafe · skill {skill} {} ({path}) - steering once, not blocking",
            compliance.as_str()
        )));
        crate::typesafe::telemetry::record_decision(
            verdict.confidence,
            true,
            cfg.escalate_confidence,
        );
        session.input_items.push(user_text_item(&steer));
        true
    }

    /// Ask Jev about this response's completed tool results. Verdicts are shown
    /// and counted; nothing is rewritten and nothing is blocked.
    async fn typesafe_judge_results(
        &self,
        results: &[TsToolResult],
        session: &Session,
        tx: &mpsc::UnboundedSender<AgentEvent>,
    ) {
        let cfg = self.config.typesafe.clone();
        if !cfg.enabled
            || !cfg.tool_gate.enabled
            || !cfg.tool_gate.judge_results
            || results.is_empty()
        {
            return;
        }
        if crate::typesafe::harness::ready(&cfg).is_none() {
            return;
        }
        let preview_chars = cfg.tool_gate.preview_chars;
        // Only results with something to judge: failures, and anything long
        // enough to be worth a second opinion. Judging every tiny confirmation
        // would spend Jev tokens for nothing.
        let worth: Vec<usize> = results
            .iter()
            .enumerate()
            .filter(|(_, r)| typesafe_judges_tool(&r.tool))
            .filter(|(_, r)| !r.ok || r.body.chars().count() >= 200)
            .map(|(i, _)| i)
            .collect();
        if worth.is_empty() {
            return;
        }
        let items: Vec<crate::typesafe::harness::ToolCallItem> = worth
            .iter()
            .enumerate()
            .filter_map(|(n, i)| {
                results
                    .get(*i)
                    .map(|r| crate::typesafe::harness::ToolCallItem {
                        index: n,
                        tool: r.tool.clone(),
                        args_preview: crate::typesafe::harness::judge_preview(
                            &r.args,
                            preview_chars,
                        ),
                        intent: None,
                        result_preview: Some(crate::typesafe::harness::judge_preview(
                            &r.body,
                            preview_chars,
                        )),
                        duplicate_of: None,
                        prior_failures: 0,
                    })
            })
            .collect();
        let goal = typesafe_goal(session, 3);
        let cfg_for_task = cfg.clone();
        let judged = match crate::typesafe::context::spawn_blocking(move || {
            let state = serde_json::json!({ "goal": goal });
            crate::typesafe::harness::judge_calls(
                &cfg_for_task,
                &state,
                &items,
                crate::typesafe::harness::JudgeScope::Verify,
            )
        })
        .await
        {
            Ok(j) => j,
            Err(_) => return,
        };
        for (n, j) in judged.iter().enumerate() {
            let Some(source) = worth.get(n).and_then(|i| results.get(*i)) else {
                continue;
            };
            if j.failed() {
                let _ = tx.send(AgentEvent::Status(format!(
                    "typesafe · {} result looks {} ({}) - verify before relying on it",
                    source.tool,
                    j.verification
                        .raw()
                        .map(|v| v.as_str())
                        .unwrap_or("unclear"),
                    j.verification
                        .confidence
                        .map(|c| format!("{c:.2}"))
                        .unwrap_or_else(|| "-".into())
                )));
            }
        }
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

        let context = route_context(&self.config, &session.input_items);
        let mut handles: Vec<Option<SubagentHandle>> = Vec::with_capacity(batch.len());
        for (call, (_, denial)) in batch.iter().zip(gated.iter()) {
            if denial.is_some() {
                handles.push(None);
                continue;
            }
            let parsed = parse_agent_call(call);
            let spawn = ChildSpawn::for_call(self, &session.id, call);
            let tx_child = tx.clone();
            let cancel_child = cancel.clone();
            let permits = permits.clone();
            let context = context.clone();
            handles.push(Some(tokio::spawn(async move {
                let parsed = parsed?;
                let spawn = spawn?;
                // RLM recursion depth: children may recurse up to the config budget.
                let depth = spawn.depth;
                let cfg_limit = spawn.config.subagent_depth.max(1);
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
                let desc = &parsed.2;
                let _ = tx_child.send(AgentEvent::Status(format!("subagent · {desc}")));
                spawn.run(parsed, context, &cancel_child, &tx_child).await
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
            let body = crate::typesafe::context::spawn_blocking(move || {
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

    /// Run one `question` tool call: validate, apply the no-interrupt rules,
    /// optionally let Jev resolve it without bothering the user, else show
    /// the modal and wait for the answer. Returns the tool result + ok.
    /// Cancellation aborts the whole turn (Err), never the question alone.
    async fn run_question_call(
        &self,
        call: &FunctionCallRef,
        session: &Session,
        tx: &mpsc::UnboundedSender<AgentEvent>,
        cancel: &CancellationToken,
    ) -> Result<(String, bool)> {
        use crate::tools::question_tool::{self, UserAnswer};
        let parsed = match question_tool::parse_args(&call.arguments) {
            Ok(q) => q,
            Err(e) => return Ok((format!("error: invalid question call: {e}"), false)),
        };
        // Goal-driven turns never interrupt the user: the BLOCKED: protocol
        // is the way to ask. (Also covers /goal auto-continuations.)
        if let Some(g) = super::goal::load(&session.id) {
            if g.status == super::goal::GoalStatus::Active {
                return Ok((
                    question_tool::format_answer(
                        &UserAnswer::Dismissed {
                            reason: "goal-driven turn",
                        },
                        &parsed.options,
                    ) + " - goal-driven turns do not interrupt the user; end with \
                         BLOCKED: <what you need> instead",
                    true,
                ));
            }
        }
        if self.is_subagent && !self.interactive_children {
            return Ok((
                question_tool::format_answer(
                    &UserAnswer::Dismissed {
                        reason: "subagent turn",
                    },
                    &parsed.options,
                ) + " - subagents cannot prompt the user; proceed with your best \
                     judgment or declare BLOCKED:",
                true,
            ));
        }
        // Jev first: a question resolvable from context, tools, or a safe
        // default should never pop a modal.
        if self.question_needs_user(&parsed, session).await == Some(false) {
            return Ok((
                "Jev judged this question resolvable without interrupting the user \
                 (answerable from context, tools, or a safe default) - resolve it \
                 yourself and keep working; do not ask."
                    .to_string(),
                true,
            ));
        }
        if cancel.is_cancelled() {
            return Err(NurError::Interrupted);
        }
        let (otx, orx) = oneshot::channel();
        let _ = tx.send(AgentEvent::QuestionRequest {
            turn_cancel: cancel.clone(),
            question: parsed.question.clone(),
            header: parsed.header.clone(),
            options: parsed.options.clone(),
            multi_select: parsed.multi_select,
            respond: otx,
            parent_session: None,
        });
        let answer = tokio::select! {
            _ = cancel.cancelled() => return Err(NurError::Interrupted),
            r = orx => r.unwrap_or_else(|_| QuestionAnswer::dismissed()),
        };
        let user_answer = if answer.dismissed {
            UserAnswer::Dismissed {
                reason: answer.reason,
            }
        } else if let Some(text) = answer.typed {
            UserAnswer::Typed(text)
        } else {
            UserAnswer::Picked(answer.selected)
        };
        Ok((
            question_tool::format_answer(&user_answer, &parsed.options),
            true,
        ))
    }

    /// A `secret` request: the TUI shows its masked modal and answers with the
    /// saved name, never the value. Subagents and runs with no interactive
    /// user (the sender is dropped) get the `nur secrets set` instruction.
    async fn run_secret_request(
        &self,
        call: &FunctionCallRef,
        tx: &mpsc::UnboundedSender<AgentEvent>,
        cancel: &CancellationToken,
    ) -> Result<(String, bool)> {
        use crate::tools::secret_tool;
        let args: serde_json::Value =
            serde_json::from_str(&call.arguments).unwrap_or(serde_json::Value::Null);
        let (name, purpose) = match secret_tool::parse_request(&args) {
            Ok(parsed) => parsed,
            Err(e) => return Ok((e.to_string(), false)),
        };
        if self.is_subagent {
            return Ok((
                format!("subagents cannot prompt the user; {}", secret_tool::NO_USER),
                false,
            ));
        }
        let (otx, orx) = oneshot::channel();
        let _ = tx.send(AgentEvent::SecretRequest {
            name,
            purpose,
            respond: otx,
        });
        let answer = tokio::select! {
            _ = cancel.cancelled() => return Err(NurError::Interrupted),
            r = orx => r,
        };
        Ok(match answer {
            Ok(saved) => (secret_tool::answer(saved.as_deref()), true),
            Err(_) => (secret_tool::NO_USER.to_string(), false),
        })
    }

    /// Does this question need the user? `Some(false)` only on a confident
    /// Jev "resolvable without interrupting" - `None` (no layer) and
    /// `Some(true)` both show the modal, since interrupting is the safe
    /// default when unsure.
    async fn question_needs_user(
        &self,
        parsed: &crate::tools::question_tool::ParsedQuestion,
        session: &Session,
    ) -> Option<bool> {
        let cfg = self.config.typesafe.clone();
        if !cfg.enabled || crate::typesafe::harness::ready(&cfg).is_none() {
            return None;
        }
        let labels: Vec<String> = parsed.options.iter().map(|(l, _)| l.clone()).collect();
        let state = serde_json::json!({
            "task": typesafe_goal(session, 3),
            "question": parsed.question,
            "options": labels,
        });
        let cfg_for_task = cfg.clone();
        let judged = crate::typesafe::context::blocking(move || {
            crate::typesafe::harness::noul(
                &cfg_for_task,
                &state,
                "question_needs_user",
                "Can this question be resolved WITHOUT asking the user - from the \
                 conversation context, available tools, or a safe default the user \
                 would accept? Answer yes when interrupting is unnecessary; no only \
                 when the user's answer is genuinely required to proceed.",
                None,
                0.5,
            )
        })
        .await
        .ok()?;
        match judged.usable() {
            Some(true) => Some(false),
            _ => Some(true),
        }
    }

    /// True when a `web_fetch` call must take the gated path for its domain
    /// prompt: not Auto mode, no allow rule, and the host is not yet approved
    /// this session. An unparsable url is treated as unapproved.
    fn web_fetch_needs_domain_gate(&self, name: &str, args: &str) -> bool {
        if name != "web_fetch" || self.permission_mode.get() == PermissionMode::Auto {
            return false;
        }
        if self.permissions.decide(name, args) == Some(RuleDecision::Allow) {
            return false;
        }
        let host = fetch_host(args);
        host.is_empty()
            || !self
                .approved_domains
                .lock()
                .map(|set| set.contains(&host))
                .unwrap_or(false)
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

        // 1.5) A fetched page can carry instructions, and exfiltration is one
        // url away: the first fetch to a domain asks, in every prompting mode.
        // Auto mode is the user's blanket yes; an explicit allow rule is too.
        if self.web_fetch_needs_domain_gate(name, args) {
            let host = fetch_host(args);
            let _ = tx.send(AgentEvent::Status(format!(
                "web_fetch · first fetch to {host} asks once per session"
            )));
            if !self.prompt_approval(name, args, tx, cancel).await {
                return false;
            }
            if !host.is_empty() {
                if let Ok(mut set) = self.approved_domains.lock() {
                    set.insert(host);
                }
            }
        }

        // 2) Plan-mode structural gates (deny cannot be overridden by allow rules).
        if mode == PermissionMode::Plan {
            match plan_mode_gate(name, args, read_only, tx) {
                PlanGate::Deny => return false,
                PlanGate::Ask => {
                    // An explicit allow rule is the user's standing answer;
                    // anything else asks once per action.
                    if self.permissions.decide(name, args) == Some(RuleDecision::Allow) {
                        return true;
                    }
                    let _ = tx.send(AgentEvent::Status(format!(
                        "plan mode · {name} needs approval"
                    )));
                    return self.prompt_approval(name, args, tx, cancel).await;
                }
                PlanGate::Allow => {
                    // Plan allowed — still honor ask rules (force a prompt).
                    if self.permissions.decide(name, args) == Some(RuleDecision::Ask) {
                        return self.prompt_approval(name, args, tx, cancel).await;
                    }
                    return true;
                }
            }
        }

        // Accepting security findings as noise is the user's call, never the
        // agent's: `secscan baseline` asks in every mode, allow rules included.
        if name == "secscan" && crate::tools::secscan::always_asks(args) {
            return self.prompt_approval(name, args, tx, cancel).await;
        }

        // 3) Allow rule skips approval (manual) / short-circuits auto.
        if self.permissions.decide(name, args) == Some(RuleDecision::Allow) {
            return true;
        }

        // 4) Ask rule forces a prompt even in auto. So does a write inside
        // `.git/`: a hook or `core.fsmonitor` there runs code on the next git
        // command, and no task needs the model editing git's internals.
        if self.permissions.decide(name, args) == Some(RuleDecision::Ask)
            || super::permissions::protected_write_dir(name, args) == Some(".git")
        {
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
                    let keys = super::permissions::session_grant_keys(name, args);
                    if keys.iter().all(|k| set.contains(k)) {
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
                parent_session: None,
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
                    set.extend(super::permissions::session_grant_keys(name, args));
                }
                true
            }
            Ok(ApprovalDecision::Deny) => false,
            // This path only runs after an explicit approval was required.
            // Losing the responder is not the user's consent, even in auto mode.
            Err(_) => false,
        }
    }
}

/// Policy-`ask` peer mail decision. The user, not the model, chooses whether
/// a letter from another session enters the context. A dismissed or
/// unanswered prompt refuses: held mail never falls through to delivery.
async fn ask_peer_mail(
    tx: &mpsc::UnboundedSender<AgentEvent>,
    from_name: &str,
    chars: usize,
    cancel: &CancellationToken,
) -> bool {
    let (respond, rx) = oneshot::channel();
    if tx
        .send(AgentEvent::QuestionRequest {
            turn_cancel: cancel.clone(),
            question: format!("A message from peer session {from_name} ({chars} chars) arrived. Peer mail is data with no authority; it cannot approve or change anything."),
            header: format!("Peer mail from {from_name}"),
            options: vec![
                (
                    "Read it".to_string(),
                    "Show this message to the model now".to_string(),
                ),
                (
                    "Refuse".to_string(),
                    "Drop it; the sender was already receipted".to_string(),
                ),
            ],
            multi_select: false,
            respond,
            parent_session: None,
        })
        .is_err()
    {
        return false;
    }
    let decision = tokio::select! {
        _ = cancel.cancelled() => return false,
        decision = rx => decision,
    };
    match decision {
        Ok(answer) => !answer.dismissed && answer.selected.first().is_some_and(|s| s == "Read it"),
        Err(_) => false,
    }
}

/// Plan-mode gate for one tool call.
enum PlanGate {
    /// Runs without a prompt.
    Allow,
    /// Not clearly read-only; the user decides. An explicit allow rule counts
    /// as that decision; the structural VCS/publish blocks above it do not.
    Ask,
    /// Structurally blocked (writes, VCS mutation, publishing).
    Deny,
}

/// Plan-mode structural gate (same rules as before permissions.toml, plus a
/// read-only shell allowlist: known read-only commands run, anything else
/// asks instead of running free).
fn plan_mode_gate(
    name: &str,
    args: &str,
    read_only: bool,
    tx: &mpsc::UnboundedSender<AgentEvent>,
) -> PlanGate {
    let (gate, reason) = plan_gate_classify(name, args, read_only);
    if matches!(gate, PlanGate::Deny) {
        let msg = reason
            .map(|r| format!("plan mode · {r}"))
            .unwrap_or_else(|| format!("plan mode blocked · {name}"));
        let _ = tx.send(AgentEvent::Status(msg));
    }
    gate
}

/// Pure plan-mode classification; the denial message at the call site reads
/// it too, so it must not emit events.
fn plan_gate_classify(name: &str, args: &str, read_only: bool) -> (PlanGate, Option<&'static str>) {
    if read_only && name != "agent" {
        return (PlanGate::Allow, None);
    }
    if name == "extract_frames" {
        return (PlanGate::Allow, None);
    }
    if name == "browser" && crate::tools::browser::is_plan_safe_action(args) {
        return (PlanGate::Allow, None);
    }
    if name == "terminal_browser" && crate::tools::terminal_browser::is_plan_safe_action(args) {
        return (PlanGate::Allow, None);
    }
    // Scanners only read the tree (their reports land in .nur/scratch), so a
    // security review in plan mode asks; `baseline` writes and stays denied.
    if name == "secscan" && !crate::tools::secscan::always_asks(args) {
        return (PlanGate::Ask, None);
    }
    if name == "bash" {
        let cmd = serde_json::from_str::<Value>(args)
            .ok()
            .and_then(|v| v.get("command").and_then(|c| c.as_str()).map(String::from))
            .unwrap_or_default();
        if let Some(reason) = plan_blocks_shell(&cmd) {
            return (PlanGate::Deny, Some(reason));
        }
        return if plan_shell_read_only(&cmd) {
            (PlanGate::Allow, None)
        } else {
            (PlanGate::Ask, None)
        };
    }
    (PlanGate::Deny, None)
}

/// Host of a `web_fetch` call's url argument, lowercased; "" when the args
/// or url do not parse (an unparsable url never counts as approved).
fn fetch_host(args: &str) -> String {
    serde_json::from_str::<Value>(args)
        .ok()
        .and_then(|v| v.get("url").and_then(|u| u.as_str()).map(String::from))
        .and_then(|u| reqwest::Url::parse(&u).ok())
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
        .unwrap_or_default()
}

/// True when every simple command in the line is a known read-only program.
/// Opaque lines (command substitution, output redirected into a file) are not
/// read-only: they ask instead.
fn plan_shell_read_only(command: &str) -> bool {
    let parts = super::permissions::shell_parts(command);
    if parts.opaque || parts.commands.is_empty() {
        return false;
    }
    parts.commands.iter().all(|c| plan_command_read_only(c))
}

/// One simple command is read-only when its program is in the plan-mode set
/// and its arguments do not turn it into a writer (`git diff --output=…`,
/// `find -delete`, `env FOO=x bar`, …).
fn plan_command_read_only(command: &str) -> bool {
    let words: Vec<&str> = command.split_whitespace().collect();
    // Skip leading VAR=value assignments; the program is the first other word.
    let prog_at = words
        .iter()
        .position(|w| !(w.contains('=') && !w.starts_with('=') && !w.starts_with('-')));
    let Some(prog_at) = prog_at else {
        return true; // bare assignments set shell vars; no program runs
    };
    let program = super::permissions::program(command);
    let Some(program) = program else {
        return false;
    };
    let args: Vec<&str> = words[prog_at + 1..]
        .iter()
        .map(|w| w.trim_matches(['"', '\'']))
        .collect();
    let positionals = || args.iter().filter(|a| !a.starts_with('-')).count();
    let has_flag = |flags: &[&str]| {
        args.iter().any(|a| {
            flags
                .iter()
                .any(|f| a == f || a.starts_with(&format!("{f}=")))
        })
    };
    // A PowerShell script block runs arbitrary code inside a reading cmdlet
    // (`Select-Object @{e={Remove-Item x}}`, `ForEach-Object {…}`).
    if program.contains('-') && command.contains('{') {
        return false;
    }
    match program.as_str() {
        // Pure readers on both shells. `env` and `printenv` are absent: one
        // runs whatever follows the assignments, the other prints every
        // provider key into the transcript. `sed`/`awk`/`xargs` are absent:
        // they carry `-i` / `system()` / a nested command.
        "ls" | "pwd" | "cat" | "head" | "tail" | "wc" | "stat" | "grep" | "egrep" | "fgrep"
        | "rg" | "sgrep" | "tr" | "cut" | "diff" | "comm" | "jq" | "echo" | "printf" | "true"
        | "false" | "which" | "where" | "whereis" | "uname" | "whoami" | "hostname" | "date"
        | "id" | "df" | "du" | "ps" | "basename" | "dirname" | "realpath" | "readlink" | "od"
        | "sha256sum" | "md5sum" | "cd" | "type" | "dir" | "findstr" | "select-string"
        | "select-object" | "sort-object" | "measure-object" | "compare-object"
        | "format-table" | "format-list" | "out-string" => true,
        // Readers with a writing form that needs no shell redirect.
        "sort" => !has_flag(&["-o", "--output"]),
        "tree" => !has_flag(&["-o"]),
        "file" => !has_flag(&["-C", "--compile"]),
        "uniq" => positionals() <= 1, // `uniq IN OUT` writes OUT
        "xxd" => !has_flag(&["-r", "-revert"]) && positionals() <= 1,
        // PowerShell's Get-* verb reads by convention.
        p if p.starts_with("get-") => true,
        "find" => !args.iter().any(|a| {
            matches!(
                a.to_ascii_lowercase().as_str(),
                "-delete" | "-exec" | "-execdir" | "-ok" | "-okdir" | "-fprintf" | "-fls"
            )
        }),
        "git" => plan_git_read_only(&args),
        _ => false,
    }
}

/// The read-only side of git: inspection subcommands, plus the list/read
/// forms of the dual-purpose ones (`git branch` reads, `git branch x` writes).
fn plan_git_read_only(args: &[&str]) -> bool {
    // `--output=<file>` writes a patch without any shell redirect, and these
    // run a program the arguments name: `-c core.fsmonitor=<cmd> status`,
    // `-c diff.external=<cmd>` / `--ext-diff`, `grep -O<cmd>`, `--upload-pack`.
    if args.iter().any(|a| {
        a.starts_with("--output")
            || matches!(*a, "-c" | "--ext-diff")
            || a.starts_with("--config-env")
            || a.starts_with("--exec-path")
            || a.starts_with("--upload-pack")
            || a.starts_with("--open-files-in-pager")
            || a.starts_with("-O")
    }) {
        return false;
    }
    // Skip git's global options (`-C dir`, `-c k=v`, `--git-dir=…`,
    // `--no-pager`, …) so `git -C ../other log` reads as `git log`.
    let mut sub_at: Option<usize> = None;
    let mut i = 0;
    while i < args.len() {
        let a = args[i];
        if a.starts_with('-') {
            if matches!(a, "-c" | "-C" | "--git-dir" | "--work-tree" | "--namespace") {
                i += 1; // these take the next word as their value
            }
            i += 1;
            continue;
        }
        sub_at = Some(i);
        break;
    }
    let Some(sub_at) = sub_at else {
        return true; // bare `git` (help text)
    };
    let sub = args[sub_at].to_ascii_lowercase();
    if matches!(
        sub.as_str(),
        "status"
            | "log"
            | "diff"
            | "show"
            | "blame"
            | "rev-parse"
            | "rev-list"
            | "ls-files"
            | "ls-tree"
            | "ls-remote"
            | "describe"
            | "shortlog"
            | "grep"
            | "count-objects"
            | "show-branch"
            | "show-ref"
            | "whatchanged"
            | "reflog"
            | "name-rev"
    ) {
        return true;
    }
    let rest: Vec<&str> = args[sub_at + 1..].to_vec();
    match sub.as_str() {
        // `git branch` / `git tag` with flags only list; a positional name writes.
        "branch" | "tag" => rest.iter().all(|a| a.starts_with('-')),
        "remote" => {
            rest.is_empty()
                || rest.iter().all(|a| a.starts_with('-'))
                || rest
                    .first()
                    .is_some_and(|a| a.eq_ignore_ascii_case("get-url"))
        }
        // `git config key` reads; `git config key value` writes.
        "config" => {
            !rest.iter().any(|a| {
                matches!(
                    a.to_ascii_lowercase().as_str(),
                    "--add" | "--unset" | "--unset-all" | "--replace-all"
                )
            }) && rest.iter().filter(|a| !a.starts_with('-')).count() <= 1
        }
        "stash" => rest
            .first()
            .is_some_and(|a| matches!(a.to_ascii_lowercase().as_str(), "list" | "show")),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_fits_large_items_and_preserves_schema_and_retrieval() {
        const CHILD: &str = "NUR_RECOVERY_FIT_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let root =
                std::env::temp_dir().join(format!("nur-recovery-fit-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            let output = std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact", "agent::r#loop::tests::recovery_fits_large_items_and_preserves_schema_and_retrieval", "--nocapture"]).env(CHILD,"1").env("NUR_HOME",&root).output().unwrap();
            std::fs::remove_dir_all(root).unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        let mut session = Session::new_for_provider("fake", "vllm", ".");
        let body = "日本語 example result 🔎 ".repeat(2000);
        session.input_items = vec![
            user_text_item("Please finish the current task"),
            serde_json::json!({"type":"function_call","call_id":"owned","name":"read_file","arguments":"{\"path\":\"notes.md\"}"}),
            function_call_output_item("owned", &body),
        ];
        let mut instructions = "Detailed instructions ".repeat(3000);
        let mut tools = vec![crate::api::types::ToolDef {
            type_: "function".into(),
            name: "read_file".into(),
            description: Some("File reader ".repeat(100)),
            parameters: Some(
                serde_json::json!({"type":"object","required":["path"],"properties":{"path":{"type":"string","description":"a path"}},"additionalProperties":false}),
            ),
        }];
        let reserve =
            fit_recovery_request(&mut session, &mut instructions, &mut tools, 4096, 100_000);
        assert!(
            attribute_request(&instructions, &tools, &session.input_items).estimated_input_tokens()
                + reserve
                <= 4096
        );
        assert!(reserve <= 512);
        assert_eq!(
            tools[0].parameters.as_ref().unwrap()["required"],
            serde_json::json!(["path"])
        );
        assert_eq!(
            tools[0].parameters.as_ref().unwrap()["properties"]["path"]["type"],
            "string"
        );
        let stored = super::super::context_store::list(&session.id);
        assert!(stored
            .iter()
            .any(
                |variable| super::super::context_store::get(&session.id, &variable.name)
                    .is_some_and(|var| var.body.as_deref() == Some(body.as_str()))
            ));
        for item in &session.input_items {
            if item["type"] == "function_call_output" {
                assert!(session
                    .input_items
                    .iter()
                    .any(|call| call["type"] == "function_call"
                        && call["call_id"] == item["call_id"]));
            }
        }
    }

    #[test]
    fn explicit_compact_honors_jev_even_without_large_savings() {
        for reduction in [0.0, 0.01, 0.24, 0.25, 0.9] {
            assert!(CompactionMode::Manual.jev_wins(true, reduction, 0.25));
            assert!(!CompactionMode::Manual.jev_wins(false, reduction, 0.25));
        }
        assert!(!CompactionMode::Pressure.jev_wins(true, 0.24, 0.25));
        assert!(!CompactionMode::Pressure.jev_wins(true, 0.0, 0.0));
        assert!(CompactionMode::Pressure.jev_wins(true, 0.25, 0.25));
        assert!(!CompactionMode::Pressure.jev_wins(false, 0.9, 0.25));
    }

    #[test]
    fn compaction_receipt_excludes_generated_summary_contents() {
        let status = "[compact: thinned 2 tool bodies · jev pre-pass: no usable judgments; summary fallback]";
        assert_eq!(
            compact_receipt_detail(&format!("private conversation [compact: fake]\n\n{status}")),
            status
        );
        assert_eq!(
            compact_receipt_detail("[compact: jev unchanged - no candidates]"),
            "[compact: jev unchanged - no candidates]"
        );
        assert_eq!(
            compact_receipt_detail("private content"),
            "compaction completed"
        );
    }

    #[tokio::test]
    async fn manual_compact_does_not_summarize_when_jev_cannot_prune() {
        const CHILD: &str = "NUR_COMPACT_NO_JUDGMENT_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let root = std::env::temp_dir().join(format!("nur-compact-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "agent::r#loop::tests::manual_compact_does_not_summarize_when_jev_cannot_prune",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .env("NUR_HOME", &root)
                .env_remove("NUR_JEV_LOCAL_URL")
                .output()
                .unwrap();
            let resolved = root.canonicalize().unwrap();
            assert!(resolved.starts_with(std::env::temp_dir().canonicalize().unwrap()));
            std::fs::remove_dir_all(resolved).unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stdout)
            );
            return;
        }
        let mut config = Config {
            native_memory: false,
            ..Config::default()
        };
        config.typesafe.enabled = true;
        config.typesafe.api_key = "test-only".into();
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        config.typesafe.base_url = format!(
            "http://127.0.0.1:{}/v1/systemone",
            closed.local_addr().unwrap().port()
        );
        drop(closed);
        config.typesafe.retries = 0;
        config.typesafe.compaction.enabled = true;
        config.typesafe.compaction.replace_summary = true;
        config.typesafe.compaction.preserve_recent = 0;
        let cwd = PathBuf::from(".");
        let runner = compaction_test_runner(config);
        for with_tool in [false, true] {
            let mut session = Session::new("test", ".");
            session.input_items = vec![user_text_item("preserve my goal")];
            if with_tool {
                session.input_items.push(serde_json::json!({
                    "type": "function_call", "call_id": "c1", "name": "read_file", "arguments": "{}"
                }));
                session
                    .input_items
                    .push(function_call_output_item("c1", "original result"));
                session
                    .input_items
                    .push(user_text_item("latest working note"));
            }
            let before = session.input_items.clone();
            let mut usage = UsageTracker::new(session.id.clone(), "test".into(), cwd.clone());
            let report = compact_session(&runner, &mut session, &mut usage)
                .await
                .unwrap();
            assert!(report.starts_with("[compact: jev unchanged"), "{report}");
            assert!(
                report.contains(if with_tool {
                    "no usable retention judgments"
                } else {
                    "no completed tool pairs"
                }),
                "{report}"
            );
            assert_eq!(session.input_items, before);
            let recorded = std::fs::read_to_string(receipt::path(&session.id)).unwrap();
            assert!(recorded.contains("jev_unchanged"));
            assert!(!recorded.contains("preserve my goal"));
            let _ = std::fs::remove_file(receipt::path(&session.id));
        }
    }

    fn compaction_test_runner(config: Config) -> AgentRunner {
        let cwd = PathBuf::from(".");
        AgentRunner {
            client: ApiClient::new("invalid://primary-must-not-run", "test-only").unwrap(),
            config,
            cwd: cwd.clone(),
            permission_mode: SharedMode::new(PermissionMode::Auto),
            approved_tools: Arc::new(Mutex::new(HashSet::new())),
            tools: ToolHost::default(),
            permissions: SharedPermissions::load(&cwd),
            hooks: HooksConfig::default(),
            is_subagent: true, // Do not update the user's latest-session pointer.
            interactive_children: false,
            subagent_depth: 0,
            prewalk_override: Arc::new(Mutex::new(None)),
            approved_domains: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    #[tokio::test]
    async fn lost_forced_approval_denies_in_auto_mode() {
        if crate::agent::admission::isolated_test(
            "agent::r#loop::tests::lost_forced_approval_denies_in_auto_mode",
        ) {
            return;
        }
        let runner = compaction_test_runner(Config::default());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let receiver = tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                if let AgentEvent::ApprovalRequest { respond, .. } = event {
                    drop(respond);
                    break;
                }
            }
        });
        let allowed = runner
            .check_approval(
                "secscan",
                r#"{"action":"baseline","ids":["fixture"]}"#,
                &tx,
                &CancellationToken::new(),
            )
            .await;
        receiver.await.unwrap();
        assert!(!allowed, "a dropped approval response authorized the baseline");
    }

    #[test]
    fn automatic_route_fallback_does_not_request_login() {
        if crate::agent::admission::isolated_test(
            "agent::r#loop::tests::automatic_route_fallback_does_not_request_login",
        ) {
            return;
        }
        std::env::remove_var("DEEPSEEK_API_KEY");
        let config = Config { provider: "vllm".into(), model: "fixture".into(), ..Config::default() };
        let runner = compaction_test_runner(config);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let result = resolve_planned_target(true, &runner.client, &runner.config, Some("deepseek"), None, None, None, None, &tx);
        assert!(matches!(result, SubagentTarget::Ready { .. }));
        while let Ok(event) = rx.try_recv() {
            assert!(!matches!(event, AgentEvent::LoginRequired { .. }), "automatic fallback requested an unnecessary login");
        }
    }

    #[tokio::test]
    #[ignore = "uses the configured Jev endpoint and credentials; opt-in live smoke"]
    async fn manual_compact_live_jev() {
        let mut config = crate::config::load_config().expect("load configured Jev");
        assert!(config.typesafe.enabled && config.typesafe.compaction.enabled);
        config.native_memory = false;
        config.typesafe.compaction.replace_summary = true;
        config.typesafe.compaction.min_reduction = 1.0;
        config.typesafe.compaction.preserve_recent = 1;
        let runner = compaction_test_runner(config);
        let mut session = Session::new("compaction-smoke", ".");
        session.input_items = vec![
            user_text_item("The temporary scratch file has been deleted. The task is complete."),
            serde_json::json!({"type":"function_call", "call_id":"old", "name":"read_file", "arguments":"{\"path\":\"scratch.txt\"}"}),
            function_call_output_item("old", &"obsolete scratch data ".repeat(80)),
            user_text_item(
                "Everything from scratch.txt is obsolete; preserve this final instruction.",
            ),
        ];
        let mut usage = UsageTracker::new(
            session.id.clone(),
            "compaction-smoke".into(),
            PathBuf::from("."),
        );
        let report = compact_session(&runner, &mut session, &mut usage)
            .await
            .unwrap();
        assert!(
            report.starts_with("[compact: jev-scored")
                || report.contains("judgments kept the transcript"),
            "actual usable Jev judgment required: {report}"
        );
        let recorded = std::fs::read_to_string(receipt::path(&session.id)).unwrap();
        assert!(recorded.contains("auxiliary_inference"));
        assert!(recorded.contains("jev_pruned") || recorded.contains("jev_unchanged"));
        println!(
            "{report}\nreceipt: {}",
            receipt::path(&session.id).display()
        );
    }

    #[test]
    fn discarding_a_streamed_tail_never_splits_a_char() {
        let mut text = String::from("kept · partial");
        discard_streamed_tail(&mut text, "partial".len());
        assert_eq!(text, "kept · ");
        // A consumer that missed the start of the failed attempt can be told
        // to drop more than it holds: it empties rather than panicking.
        let mut short = String::from("ab");
        discard_streamed_tail(&mut short, 10);
        assert_eq!(short, "");
        // Landing inside the two-byte middle dot moves down to its start.
        let mut mid = String::from("a·b");
        discard_streamed_tail(&mut mid, 2);
        assert_eq!(mid, "a");
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
        // Cline routes by its own id/name and the vendor-CLI spelling.
        assert_eq!(resolve_provider_alias("cline").map(|p| p.id), Some("cline"));
        assert_eq!(
            resolve_provider_alias("cline-bot").map(|p| p.id),
            Some("cline")
        );
    }

    /// "cline" is a whole word inside larger English words. Routing must never
    /// fire there — a spawn verb next to "decline" must not target Cline.
    #[test]
    fn provider_routing_ignores_cline_inside_english_words() {
        assert_eq!(
            extract_provider_routing_phrase("spawn a subagent, then decline the offer"),
            None,
            "decline must not route to cline"
        );
        assert_eq!(
            extract_provider_routing_phrase("deploy the fix once you incline the plane"),
            None
        );
        // …while a real ask still routes.
        assert_eq!(
            extract_provider_routing_phrase("spawn a cline subagent to audit this")
                .map(|(id, _)| id),
            Some("cline".to_string())
        );
        assert_eq!(
            extract_provider_routing_phrase("run this through cline").map(|(id, _)| id),
            Some("cline".to_string())
        );
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
            "sgrep",
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
            if is_concurrency_safe(name, "{}") {
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
                !is_concurrency_safe(name, "{}"),
                "{name} must run sequentially"
            );
            assert!(!is_read_only_call(name, "{}"), "{name} must need approval");
        }
        assert!(is_read_only_call("look", r#"{"path":"x.png"}"#));
        assert!(is_concurrency_safe("look", r#"{"path":"x.png"}"#));
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
        assert!(!is_concurrency_safe("memory", r#"{"action":"read"}"#));
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
        assert!(is_concurrency_safe(
            "graphify",
            r#"{"action":"query","question":"x"}"#
        ));
        assert!(!is_concurrency_safe("graphify", r#"{"action":"extract"}"#));
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
        assert!(is_concurrency_safe("excalidraw", r#"{"action":"status"}"#));
        assert!(!is_concurrency_safe(
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
            "git -C ../other log --oneline",
            "git --no-pager diff",
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
            // Spacing and git's global options must not hide the subcommand.
            "git  commit -m x",
            "git\tpush",
            "git -C . commit -am x",
            "git -c user.name=x commit -m y",
            "git --git-dir=.git --work-tree . push",
            "git --no-pager stash",
            "/usr/bin/git add .",
            "npm  install",
        ] {
            assert!(plan_blocks_shell(bad).is_some(), "should block: {bad}");
        }
    }

    #[test]
    fn fetch_host_reads_the_url_arg() {
        // Failure modes: a host with different case bypasses the approved
        // set; an unparsable url counts as approved and never asks.
        assert_eq!(
            fetch_host(r#"{"url":"https://Docs.Example.com/x?y=1"}"#),
            "docs.example.com"
        );
        assert_eq!(fetch_host(r#"{"url":"not a url"}"#), "");
        assert_eq!(fetch_host(r#"{}"#), "");
        assert_eq!(fetch_host("garbage"), "");
    }

    #[test]
    fn web_fetch_domain_gate_only_for_new_hosts_outside_auto() {
        // Failure modes: a first-time host rides the free parallel batch and
        // the domain prompt never fires; an already-approved host asks again;
        // Auto mode (a blanket yes) gets prompted anyway.
        let runner = compaction_test_runner(Config::default());
        let args = r#"{"url":"https://example.com/"}"#;

        runner.permission_mode.set(PermissionMode::Auto);
        assert!(!runner.web_fetch_needs_domain_gate("web_fetch", args));

        runner.permission_mode.set(PermissionMode::Manual);
        assert!(runner.web_fetch_needs_domain_gate("web_fetch", args));
        assert!(!runner.web_fetch_needs_domain_gate("read_file", args));

        runner
            .approved_domains
            .lock()
            .unwrap()
            .insert("example.com".to_string());
        assert!(!runner.web_fetch_needs_domain_gate("web_fetch", args));
    }

    #[test]
    fn plan_shell_read_only_covers_known_readers_only() {
        // Failure modes: a writer or code runner slipping through the
        // read-only set (plan mode runs it with no prompt), or a plain reader
        // falling out (every exploration step nags the user).
        for ro in [
            "ls -la",
            "cat src/main.rs",
            "rg 'fn main' -n",
            "grep -rn TODO src",
            "git status",
            "git -C ../other log --oneline",
            "git --no-pager diff",
            "git branch -a",
            "git tag -l",
            "git config user.name",
            "git remote -v",
            "git remote get-url origin",
            "git stash list",
            "cat a.rs | grep foo | wc -l",
            "Get-ChildItem src",
            "Select-String foo bar.rs",
            "Get-Process",
            "echo hello",
            "FOO=1 rg bar",
            "find src -name '*.rs'",
            "jq .package.json package.json",
            "sort names.txt",
            "uniq -c names.txt",
            "xxd blob.bin",
            "tree src",
        ] {
            assert!(plan_shell_read_only(ro), "read-only: {ro}");
        }
        for ask in [
            // Interpreters and build/test runners are not read-only.
            "python analyze.py --report",
            "cargo test",
            "npm run build",
            // Plain writers.
            "cp demo.mp4 /tmp/clip.mp4",
            "rm -rf target",
            "echo hi > out.txt",
            // Substitution hides the real command.
            "cat $(ls)",
            // Dual-purpose forms that write.
            "git diff --output=patch.diff",
            "git branch new-feature",
            "git tag v1.0",
            "git config user.name bob",
            "git config --add alias.x y",
            "git stash pop",
            "git remote add upstream u",
            // find/xargs/env carry a nested command or a delete flag.
            "find . -delete",
            "find . -exec rm {} +",
            "env FOO=1 rm x",
            "xargs rm",
            // Git options that run a program the arguments name.
            "git -c core.fsmonitor=evil status",
            "git -c diff.external=evil diff",
            "git diff --ext-diff",
            "git grep -Oevil foo",
            // Readers with a redirect-free writing form.
            "sort -o ~/.ssh/authorized_keys payload.txt",
            "sort --output=out.txt in.txt",
            "uniq in.txt out.txt",
            "tree -o out.txt",
            "xxd -r dump.hex out.bin",
            // Prints every provider key into the transcript.
            "printenv",
            // A script block runs code inside a reading cmdlet.
            "Get-ChildItem | Select-Object @{e={Remove-Item x}}",
        ] {
            assert!(!plan_shell_read_only(ask), "should ask: {ask}");
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
        assert!(
            matches!(err, PreflightBlock::Budget(ref msg) if msg.contains("reserved output")),
            "unexpected block: {err:?}"
        );

        cfg.max_session_tokens = Some(20_000);
        assert!(preflight_request_budget(&cfg, &usage, "meta", "m", 3_000, 8_192).is_none());
    }

    #[test]
    fn default_config_has_no_budgets_and_preflight_never_hard_stops() {
        let cfg = Config::default();
        // No budget of any kind is on by default.
        assert!(cfg.max_session_tokens.is_none());
        assert!(cfg.max_session_cost_usd.is_none());
        assert_eq!(cfg.max_turns, 0);
        // Without explicit budgets the preflight can never classify a block
        // as a hard Budget stop - window overflow is recoverable and is
        // handled by compact-and-retry in the loop.
        let usage = UsageTracker::new("t".into(), "m".into(), PathBuf::from("."));
        let block = preflight_request_budget(
            &cfg,
            &usage,
            "test-no-provider",
            "zz-no-model",
            2_000_000,
            8_192,
        );
        assert!(matches!(
            block,
            Some(PreflightBlock::ContextWindow(_)) | None
        ));
        assert!(!matches!(block, Some(PreflightBlock::Budget(_))));
    }

    #[test]
    fn request_preflight_classifies_window_overflow_as_recoverable() {
        // 3000 estimated input + 8192 output reserve does not fit a 4096
        // window: that must surface as the RECOVERABLE ContextWindow block
        // (compact and retry), not as a hard session-budget stop. The grok
        // bug was exactly this shape at a 500k window.
        let cfg = Config {
            context_window: 4_096,
            ..Default::default()
        };
        let usage = UsageTracker::new("preflight-window".into(), "m".into(), PathBuf::from("."));
        // A local provider carries no catalog window, so the configured
        // context_window is authoritative — deterministic, no cache reads.
        let block =
            preflight_request_budget(&cfg, &usage, "llamacpp", "zz-local-model", 3_000, 8_192)
                .expect("over-window request must be blocked");
        match block {
            PreflightBlock::ContextWindow(msg) => {
                assert!(msg.contains("context window"), "message: {msg}");
            }
            PreflightBlock::Budget(msg) => {
                panic!("window overflow misclassified as budget stop: {msg}")
            }
        }
        // Under a wide window the same request passes.
        let cfg = Config::default();
        assert!(
            preflight_request_budget(&cfg, &usage, "llamacpp", "zz-local-model", 3_000, 8_192)
                .is_none()
        );
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
        assert!(!is_concurrency_safe("browser", r#"{"action":"tabs"}"#));
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

/// Why a request was refused before it left the machine. The two cases need
/// opposite treatments: an over-window request is recoverable (compaction
/// frees real context and the request can be retried), while a session
/// budget cap is a hard stop the user asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreflightBlock {
    /// The request would not fit the model's context window.
    ContextWindow(String),
    /// The session token or cost budget would be crossed.
    Budget(String),
}

impl PreflightBlock {
    pub fn message(&self) -> String {
        match self {
            PreflightBlock::ContextWindow(msg) | PreflightBlock::Budget(msg) => msg.clone(),
        }
    }
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
) -> Option<PreflightBlock> {
    let requested = estimated_input_tokens.saturating_add(reserved_output_tokens);
    let context_window =
        crate::pricing::context_window_for(provider, model).unwrap_or(cfg.context_window);
    if requested > context_window {
        return Some(PreflightBlock::ContextWindow(format!(
            "estimated input {estimated_input_tokens} + reserved output {reserved_output_tokens} = {requested} tokens exceeds {provider}/{model} context window {context_window}"
        )));
    }
    let session = usage.session_usage();
    if let Some(max) = cfg.max_session_tokens {
        let projected = session.total_tokens.saturating_add(requested);
        if projected > max {
            return Some(PreflightBlock::Budget(format!(
                "request preflight blocked: session tokens {} + estimated input {estimated_input_tokens} + reserved output {reserved_output_tokens} = {projected} would exceed budget {max}",
                session.total_tokens
            )));
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
            return Some(PreflightBlock::Budget(format!(
                "request preflight blocked: estimated session cost ${projected:.4} (including this request's ${:.4} reserve) would exceed budget ${max:.4}",
                crate::pricing::rates_for(provider, model).cost_for(&reserve)
            )));
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
    let c = format!(" {} ", plan_shell_words(command));
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

/// The command as plan mode's phrase lists read it: lowercase, one space
/// between words, and git's global options (`-C <dir>`, `-c k=v`,
/// `--git-dir=…`, `--no-pager`, …) dropped, so `git  commit` and
/// `git -C . commit` read as `git commit`.
fn plan_shell_words(command: &str) -> String {
    let lower = command.to_ascii_lowercase();
    let mut words = lower.split_whitespace().peekable();
    let mut out: Vec<&str> = Vec::new();
    while let Some(word) = words.next() {
        out.push(word);
        if word != "git" && !word.ends_with("/git") && !word.ends_with("\\git.exe") {
            continue;
        }
        while let Some(&option) = words.peek() {
            if !option.starts_with('-') {
                break;
            }
            words.next();
            // `-C`/`-c` (one spelling once lowercased) and the long forms
            // without `=` take the next word as their value.
            if matches!(option, "-c" | "--git-dir" | "--work-tree" | "--namespace") {
                words.next();
            }
        }
        out.pop();
        out.push("git");
    }
    out.join(" ")
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

/// Cap on preflight context-window recoveries (compact + retry) per user
/// turn, so a pathological estimate cannot spin the loop forever. After the
/// cap the request is sent anyway from a minimal locally-trimmed context.
const MAX_PREFLIGHT_RECOVERIES: u8 = 3;

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
            // `droid exec` rejects ids outside its catalog this way.
            "invalid model",
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
    items
        .iter()
        .map(crate::agent::prompt::estimate_item_tokens)
        .fold(0u64, u64::saturating_add)
}

/// Placeholder left where an inline image used to be.
pub(crate) const MEDIA_DROPPED_NOTE: &str =
    "[image omitted to fit the context window - re-attach it if it is still needed]";

/// Replace every inline `input_image` part (data URL) in a message item with a
/// text note. Returns how many were dropped. Items without images are untouched.
pub(crate) fn strip_inline_images(item: &mut Value) -> usize {
    let Some(parts) = item.get_mut("content").and_then(Value::as_array_mut) else {
        return 0;
    };
    let mut dropped = 0usize;
    for part in parts.iter_mut() {
        let is_inline_image = part.get("type").and_then(Value::as_str) == Some("input_image")
            && part
                .get("image_url")
                .and_then(Value::as_str)
                .is_some_and(|u| u.starts_with("data:"));
        if is_inline_image {
            *part = serde_json::json!({"type": "input_text", "text": MEDIA_DROPPED_NOTE});
            dropped += 1;
        }
    }
    dropped
}

fn should_auto_compact(
    usage: &UsageTracker,
    cfg: &Config,
    model: &str,
    subscription: bool,
    items: &[Value],
) -> bool {
    let last = usage.last_usage();
    let provider_used = if last.input_tokens > 0 {
        last.input_tokens
    } else {
        last.total_tokens
    };
    // OpenAI bills the entire GPT-5.6 Sol request at a higher rate above
    // 272k input tokens. This is a proactive compaction target, not a hard
    // request limit: failed compaction must never stop a turn.
    let has_price_cliff = cfg.avoid_long_context_pricing
        && !subscription
        && matches!(cfg.provider.as_str(), "openai" | "openai-cc")
        && (model == "gpt-5.6" || model.starts_with("gpt-5.6-sol"));
    let window = if has_price_cliff {
        cfg.context_window.min(272_000)
    } else {
        cfg.context_window
    }
    .max(1);
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
        "sgrep",
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
    // TypeSafe/Jev judgments are billed work outside the primary chat call, and
    // they buy back tokens the primary call does not have to carry. Recorded as a
    // delta so the session tally the TUI chip shows stays cumulative.
    for jev in crate::typesafe::telemetry::take_for_session(session_id) {
        if !jev.is_empty() {
            // Where it really came from: a local bridge and the hosted API must never
            // be confused in the audit trail.
            let ts_cfg = crate::config::load_config()
                .map(|c| c.typesafe)
                .unwrap_or_default();
            let route = if jev.route.is_empty() {
                crate::typesafe::client::effective_base_url(&ts_cfg)
            } else {
                jev.route.clone()
            };
            let model = if !jev.model.is_empty() {
                jev.model.clone()
            } else if ts_cfg.model.trim().is_empty() {
                crate::typesafe::client::DEFAULT_MODEL.to_string()
            } else {
                ts_cfg.model.trim().to_string()
            };
            receipt::record(
                session_id,
                receipt::Event::AuxiliaryInference {
                    purpose: "typesafe system one judgments".into(),
                    route: route.clone(),
                    model,
                    processing: if crate::typesafe::client::is_loopback_endpoint(&route) {
                        "local"
                    } else {
                        "remote"
                    }
                    .into(),
                    input_tokens: jev.input_tokens,
                    output_tokens: jev.output_tokens,
                    cost_usd: None,
                    cost_provenance: "typesafe: provider-reported usage; pricing not published by \
                                  the endpoint"
                        .into(),
                    outcome: format!(
                        "requests {} · questions {} · decisions {} · escalations {} · gated {} · \
                     pruned {} call(s)/{} result(s) · ~{} tokens kept out of context · \
                     frontier compaction calls avoided {} · failures {}",
                        jev.requests,
                        jev.questions,
                        jev.decisions,
                        jev.escalations,
                        jev.gated_calls,
                        jev.pruned_calls,
                        jev.pruned_results,
                        jev.tokens_saved(),
                        jev.frontier_calls_avoided,
                        jev.failures,
                    ),
                },
            );
        }
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
        "commandcode",
        // Whole-word matched below, so "decline" / "incline" never fire.
        "cline",
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

/// What one `agent` call needs to run its child, captured from the parent so
/// the blocking, async and fan-out paths route every child the same way:
/// cache-aware routing, then target resolution, then the nested runner.
struct ChildSpawn {
    client: ApiClient,
    config: Config,
    cwd: PathBuf,
    mode: SharedMode,
    domains: Arc<Mutex<HashSet<String>>>,
    depth: u32,
    interactive_children: bool,
    /// The parent's session id, and the variables of its context store the
    /// call named in `context_vars`: all the child may read of it.
    parent_session: String,
    context_vars: Vec<String>,
    output_schema: Option<Value>,
    worktree: Option<String>,
    /// `cost_mode: economy`: a cheap model, low effort (see [`economy_model`]).
    economy: bool,
}

/// Most `context_vars` one `agent` call may hand over.
const MAX_HANDED_OVER_VARS: usize = 16;

impl ChildSpawn {
    /// The spawn for one `agent` call. Fails when `context_vars` names a
    /// variable the parent does not have, so a typo is not a silent gap.
    fn for_call(
        runner: &AgentRunner,
        parent_session: &str,
        call: &FunctionCallRef,
    ) -> Result<Self> {
        // The `agent` tool never reaches ToolHost::dispatch (the loop
        // intercepts it), so the core-profile dispatch refusal cannot catch
        // it. Refuse it here, in the one place both the sequential and
        // fan-out paths pass through, with the same message dispatch uses.
        if runner.config.tools.is_core() {
            return Err(NurError::Tool(
                "tool 'agent' is not part of the core tool profile ([tools] profile = \"full\" enables it)".into(),
            ));
        }
        let args: Value = serde_json::from_str(&call.arguments).unwrap_or(Value::Null);
        let worktree = match args.get("worktree") {
            None => None,
            Some(Value::String(name)) if !name.is_empty() => Some(name.clone()),
            Some(_) => return Err(NurError::Tool("agent.worktree must be a nonempty managed worktree name".into())),
        };
        let output_schema = args.get("output_schema").cloned();
        if let Some(schema) = &output_schema {
            if !schema.is_object() {
                return Err(NurError::Tool("agent.output_schema must be a JSON Schema object".into()));
            }
            jsonschema::validator_for(schema).map_err(|error| NurError::Tool(format!("invalid agent.output_schema: {error}")))?;
        }
        let mut context_vars: Vec<String> = Vec::new();
        for name in args
            .get("context_vars")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            if !context_vars.iter().any(|known| known == name) {
                context_vars.push(name.to_string());
            }
        }
        if context_vars.len() > MAX_HANDED_OVER_VARS {
            return Err(NurError::Tool(format!(
                "agent.context_vars names {} variables; at most {MAX_HANDED_OVER_VARS}",
                context_vars.len()
            )));
        }
        let missing: Vec<&str> = context_vars
            .iter()
            .filter(|name| super::context_store::get(parent_session, name).is_none())
            .map(String::as_str)
            .collect();
        if !missing.is_empty() {
            return Err(NurError::Tool(format!(
                "agent.context_vars: no context variable named `{}` (`context` action=list shows them)",
                missing.join("`, `")
            )));
        }
        Ok(Self {
            client: runner.client.clone(),
            config: runner.config.clone(),
            cwd: runner.cwd.clone(),
            mode: runner.permission_mode.clone(),
            domains: runner.approved_domains.clone(),
            depth: runner.subagent_depth,
            interactive_children: runner.interactive_children && !super::goal::load(parent_session).is_some_and(|goal| goal.status == super::goal::GoalStatus::Active),
            parent_session: parent_session.to_string(),
            context_vars,
            output_schema,
            worktree,
            economy: args.get("cost_mode").and_then(Value::as_str) == Some("economy"),
        })
    }

    async fn run(
        self,
        parsed: ParsedAgentCall,
        context: Arc<Vec<String>>,
        cancel: &CancellationToken,
        tx: &mpsc::UnboundedSender<AgentEvent>,
    ) -> Result<(String, TokenUsage)> {
        let (prompt, kind, desc, provider_override, model_override) = parsed;
        let (cwd, _worktree_lease) = if let Some(name) = self.worktree {
            let root = self.cwd.clone();
            let token = cancel.clone();
            let task = tokio::task::spawn_blocking(move || crate::worktree::claim(&root, &name, &token));
            let lease = tokio::select! {
                _ = cancel.cancelled() => return Err(NurError::Interrupted),
                result = task => result.map_err(|error| NurError::Tool(format!("worktree lookup failed: {error}")))??,
            };
            let lease = Arc::new(lease);
            (lease.path.clone(), Some(lease))
        } else {
            (self.cwd, None)
        };
        // Every event from child routing is scoped, including events emitted
        // before the child runner exists. An async child can outlive this
        // turn, so an unscoped status or login notice could reach a new session.
        let (child_tx, mut child_rx) = tokio::sync::mpsc::unbounded_channel();
        let parent_tx = tx.clone();
        let parent_scope = self.parent_session.clone();
        let child_cancel = cancel.clone();
        let forward = tokio::spawn(async move {
            while let Some(event) = child_rx.recv().await {
                if child_cancel.is_cancelled() { break; }
                let event = match event {
                    AgentEvent::LoginRequired { provider_id, provider_name, retry_prompt, retry_desc, retry_kind, retry_model, .. } => AgentEvent::LoginRequired {
                        provider_id, provider_name, retry_prompt, retry_desc, retry_kind, retry_model,
                        parent_session: Some(parent_scope.clone()),
                    },
                    AgentEvent::Status(status) => AgentEvent::ChildStatus {
                        parent_session: parent_scope.clone(), status, finished: false,
                    },
                    other => other,
                };
                if parent_tx.send(event).is_err() { break; }
            }
        });
        let result = async {
            let tx = &child_tx;
            let (prompt, provider_override, model_override, hopped) = plan_unrouted_child(
                &self.config,
                prompt,
                context,
                provider_override,
                model_override,
                tx,
            )
            .await;
            // Cross-provider: a named provider gets a client from its own stored
            // credentials. Missing creds → LoginRequired + hard block, never a
            // silent run on the parent.
            let target = resolve_planned_target(
                hopped,
                &self.client,
                &self.config,
                provider_override.as_deref(),
                model_override.as_deref(),
                Some(prompt.as_str()),
                Some(desc.as_str()),
                Some(kind.as_str()),
                tx,
            );
            match target {
                SubagentTarget::Ready { client, mut config } => {
                    // A named or routed model is a choice; economy fills the gap.
                    if self.economy && model_override.is_none() {
                        let status = match economy_model(&self.config, &client, &config.provider).await
                        {
                            Some(model) => {
                                config.model = model;
                                format!("subagent · economy · {}", config.model)
                            }
                            None => format!(
                                "subagent · economy · no priced cheaper model, staying on {}",
                                config.model
                            ),
                        };
                        config.reasoning_effort = "low".into();
                        let _ = tx.send(AgentEvent::Status(status));
                    }
                    // Handed-over context leaves the parent's provider: say so.
                    if !self.context_vars.is_empty() && config.provider != self.config.provider {
                        let name = crate::providers::by_id(&config.provider)
                            .map(|provider| provider.name)
                            .unwrap_or(config.provider.as_str());
                        let n = self.context_vars.len();
                        let _ = tx.send(AgentEvent::Status(format!(
                            "subagent · sharing {n} context variable{} with {name}",
                            if n == 1 { "" } else { "s" }
                        )));
                    }
                    let (report, usage) = subagent::run_subagent(
                        client,
                        *config,
                        cwd,
                        self.mode,
                        self.domains,
                        &prompt,
                        &kind,
                        self.depth,
                        self.interactive_children,
                        &self.parent_session,
                        &self.context_vars,
                        self.output_schema.as_ref(),
                        _worktree_lease.clone(),
                        cancel,
                        tx,
                    )
                    .await?;
                    if let Some(schema) = &self.output_schema {
                        let value: Value = serde_json::from_str(&report).map_err(|_| NurError::Tool("subagent output does not match output_schema: expected a JSON value".into()))?;
                        let validator = jsonschema::validator_for(schema).map_err(|error| NurError::Tool(format!("invalid agent.output_schema: {error}")))?;
                        validator.validate(&value).map_err(|error| NurError::Tool(format!("subagent output does not match output_schema: {error}")))?;
                    }
                    Ok((report, usage))
                }
                // A tool error, so the parent model does not take a parent-provider
                // run for success. LoginRequired was already emitted.
                SubagentTarget::AwaitingLogin { message, .. }
                | SubagentTarget::Unavailable { message } => Err(NurError::Other(message)),
            }
        }
        .await;
        drop(child_tx);
        let _ = forward.await;
        result
    }
}

/// The model an economy child runs on, without launching anything: the
/// configured `economy_model` when the child stays on the parent's provider,
/// else the cheapest tool-capable model this account lists that models.dev
/// prices. `None` leaves the child on the model it resolved to.
async fn economy_model(parent: &Config, client: &ApiClient, provider: &str) -> Option<String> {
    let configured = parent.economy_model.trim();
    if !configured.is_empty() && provider == parent.provider {
        return Some(configured.to_string());
    }
    let ids = client.live_model_ids().await.ok()?;
    let provider = provider.to_string();
    crate::typesafe::context::blocking(move || crate::pricing::cheapest_tool_model(&provider, &ids))
        .await
        .ok()
        .flatten()
}

async fn run_agent_tool(
    runner: &AgentRunner,
    call: &FunctionCallRef,
    parent_session: &str,
    context: Arc<Vec<String>>,
    cancel: &CancellationToken,
    tx: &mpsc::UnboundedSender<AgentEvent>,
) -> Result<(String, TokenUsage)> {
    let parsed = parse_agent_call(call)?;
    let desc = parsed.2.clone();
    let spawn = ChildSpawn::for_call(runner, parent_session, call)?;
    // Async admission (Prime rlm() / RLM model): if the model asked for a
    // non-blocking child, admit + spawn in the background, return the handle.
    let want_async = serde_json::from_str::<Value>(&call.arguments)
        .map(|v| v.get("async").and_then(Value::as_bool).unwrap_or(false))
        .unwrap_or(false);
    if want_async && !runner.is_subagent {
        let sid = parent_session.to_string();
        let id = crate::agent::admission::admit(&sid, &desc);
        let tx2 = tx.clone();
        let accept = cancel.clone();
        tokio::spawn(async move {
            let outcome = spawn.run(parsed, context, &accept, &tx2).await;
            match outcome {
                Ok((text, _)) => crate::agent::admission::finish(&sid, id, &text, true),
                Err(e) => crate::agent::admission::finish(&sid, id, &e.to_string(), false),
            }
            let _ = tx2.send(AgentEvent::ChildStatus { parent_session: sid, status: String::new(), finished: true });
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
    // Boxed: the child's routing state (a whole Config and client) would
    // otherwise sit inside the turn loop's future, which `main` holds on its
    // stack; an unoptimized build overflowed Windows' 1 MB main stack before
    // parsing arguments.
    Box::pin(spawn.run(parsed, context, cancel, tx)).await
}

/// Parent context a routed child may be shown. Built only when routing is on,
/// so a disabled router costs nothing per `agent` call.
fn route_context(config: &Config, items: &[Value]) -> Arc<Vec<String>> {
    let ts = &config.typesafe;
    if ts.enabled && ts.routing.enabled {
        Arc::new(crate::typesafe::route::child_context(items))
    } else {
        Arc::new(Vec::new())
    }
}

/// Cache-aware routing for an `agent` call that named no provider or model
/// ([`crate::typesafe::route::plan_child`]). The judgments are blocking HTTP,
/// so they run on the blocking pool. Returns the prompt the child receives,
/// the (possibly routed) provider/model overrides, and whether routing chose
/// them. An explicit provider or model is always obeyed as written.
async fn plan_unrouted_child(
    config: &Config,
    prompt: String,
    context: Arc<Vec<String>>,
    provider: Option<String>,
    model: Option<String>,
    tx: &mpsc::UnboundedSender<AgentEvent>,
) -> (String, Option<String>, Option<String>, bool) {
    let ts = &config.typesafe;
    if provider.is_some() || model.is_some() || !ts.enabled || !ts.routing.enabled {
        return (prompt, provider, model, false);
    }
    let cfg = config.clone();
    let task = prompt.clone();
    let planned = crate::typesafe::context::blocking(move || {
        crate::typesafe::route::plan_child(&cfg, &task, &context)
    })
    .await
    .ok()
    .flatten();
    let Some(plan) = planned else {
        return (prompt, None, None, false);
    };
    if let Some(note) = plan.note.as_ref().filter(|_| ts.routing.suggest) {
        let _ = tx.send(AgentEvent::Status(note.clone()));
    }
    match plan.hop {
        Some(hop) => (plan.prompt, Some(hop.provider), Some(hop.model), true),
        None => (plan.prompt, None, None, false),
    }
}

/// [`resolve_subagent_target`], except that a target routing chose on its own
/// falls back to the parent's model when it cannot be built: an automatic hop
/// is an optimization, never a reason for the child to fail.
#[allow(clippy::too_many_arguments)]
fn resolve_planned_target(
    hopped: bool,
    parent_client: &ApiClient,
    parent_config: &Config,
    provider: Option<&str>,
    model: Option<&str>,
    retry_prompt: Option<&str>,
    retry_desc: Option<&str>,
    retry_kind: Option<&str>,
    tx: &mpsc::UnboundedSender<AgentEvent>,
) -> SubagentTarget {
    // An automatic route is only a candidate. Do not surface its login or
    // failure notices if the parent route can be used instead.
    let (candidate_tx, mut candidate_rx) = tokio::sync::mpsc::unbounded_channel();
    let candidate_events = if hopped { &candidate_tx } else { tx };
    let target = resolve_subagent_target(
        parent_client,
        parent_config,
        provider,
        model,
        retry_prompt,
        retry_desc,
        retry_kind,
        candidate_events,
    );
    drop(candidate_tx);
    if hopped && !matches!(target, SubagentTarget::Ready { .. }) {
        let _ = tx.send(AgentEvent::Status(
            "subagent · routed model unavailable - running on the parent model".into(),
        ));
        return resolve_subagent_target(
            parent_client,
            parent_config,
            None,
            None,
            retry_prompt,
            retry_desc,
            retry_kind,
            tx,
        );
    }
    while let Ok(event) = candidate_rx.try_recv() {
        let _ = tx.send(event);
    }
    target
}

/// Outcome of resolving where a subagent should run.
enum SubagentTarget {
    Ready {
        client: ApiClient,
        config: Box<Config>,
    },
    /// Explicit cross-provider request, but no credentials yet. Do not run.
    AwaitingLogin { message: String },
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
    // A sidecar (TypeSafe · Jev, Enclave) has a credential like a provider but
    // cannot serve a chat request. Point the caller at the thing that actually
    // works instead of failing upstream.
    if crate::providers::is_sidecar_provider(prov.id) {
        let role = crate::providers::sidecar_role(prov.id);
        let message = format!(
            "`{}` is not a chat provider - it is a {}, and it has no model to run a subagent \
             on. Use {} ({}), `route` to pick a model, or name a real provider here.",
            prov.name, role.kind, role.surface, role.serves
        );
        let _ = tx.send(AgentEvent::Status(message.clone()));
        return SubagentTarget::Unavailable { message };
    }
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
                // Persist with the same credential-kind gating as failover:
                // plain API-key imports (cursor/opencode env keys, harness
                // keys) must not be stored as OAuth sessions.
                if crate::oauth::imported_as_oauth_session(&tokens) {
                    let _ = crate::auth::save_provider_oauth(
                        prov.id,
                        &tokens.access_token,
                        tokens.refresh_token.clone(),
                        tokens.expires_at,
                        tokens.meta.clone(),
                    );
                } else {
                    let _ = crate::auth::save_provider_key(prov.id, tokens.access_token.trim());
                }
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
            parent_session: None,
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
             Do NOT re-run this subagent on the parent provider. \
             The user signs in with `/login {id}` in the TUI (nur opens it pre-selected \
             there and, once it finishes, hands you the exact re-deploy call) or with \
             `nur auth login --provider {id}` from a shell; then deploy again with \
             provider=\"{id}\".",
            name = prov.name,
            id = prov.id,
        );
        return SubagentTarget::AwaitingLogin { message };
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
        // Cline ships its own CLI (`npm i -g cline`); `cline auth cline` mints
        // the account session `oauth::cline::import_cline_cli` reads.
        "cline" => Some(DriverId::Cline),
        _ => None,
    }
}

/// Tools that count as "starting implementation" for OMP prewalk.
fn is_prewalk_mutate_tool(name: &str) -> bool {
    matches!(
        name,
        "write_file" | "edit_file" | "multi_edit" | "apply_patch" | "hashline_edit"
    )
}

/// Resolve the cheap target for prewalk (`[prewalk] into` → `NUR_PREWALK_MODEL`
/// → `economy_model`). Asking an
/// installed `omp` for its `smol` role launched that CLI mid-turn, in front of
/// the next model request, and made nur's routing depend on another harness's
/// config; nur's own `[prewalk] into` is the setting.
pub fn resolve_prewalk_into(cfg: &Config) -> Option<String> {
    let configured = cfg.prewalk.into.trim();
    if !configured.is_empty() {
        return Some(configured.to_string());
    }
    std::env::var("NUR_PREWALK_MODEL")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .or_else(|| Some(cfg.economy_model.trim().to_string()).filter(|v| !v.is_empty()))
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

#[path = "compaction.rs"]
mod compaction;
use compaction::*;
pub use compaction::compact_session;

/// Most times the skill layer may correct a single turn. A triggered skill
/// should steer the work, not nag it.
const MAX_SKILL_NUDGES: u8 = 2;

/// Tools whose gate/verdict judgment would be noise rather than signal.
///
/// These are bookkeeping and self-description: a judgment tool call cannot be
/// "risky" or "needs a human", a skill load cannot fail in an informative way,
/// and a todo update is not work. Skipping them saves Jev questions without
/// touching the cases that matter (repeated reads, retried commands, long
/// result bodies). Deduplication still applies to real tools.
const TYPESAFE_META_TOOLS: &[&str] = &["typesafe", "skill", "todo_write", "question", "secret"];

/// Should the gate/verdict judgment cover this tool?
/// First sentence of a tool description, bounded, for a judge's option list.
fn one_line_description(description: &str) -> String {
    let flat = description.split_whitespace().collect::<Vec<_>>().join(" ");
    let first = flat
        .split_inclusive(". ")
        .next()
        .unwrap_or("")
        .trim_end()
        .trim_end_matches('.');
    first.chars().take(160).collect()
}

fn typesafe_judges_tool(tool: &str) -> bool {
    !TYPESAFE_META_TOOLS.contains(&tool)
}

fn call_arguments(calls: &[FunctionCallRef], call_id: &str) -> String {
    calls
        .iter()
        .find(|c| c.call_id == call_id)
        .map(|c| c.arguments.clone())
        .unwrap_or_default()
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
mod jev_compaction_tests {
    use super::*;
    use serde_json::json;

    fn call(id: &str) -> Value {
        json!({"type":"function_call","call_id":id,"name":"read_file","arguments":"{\"path\":\"a.rs\"}"})
    }
    fn call_args(id: &str, tool: &str, args: &str) -> Value {
        json!({"type":"function_call","call_id":id,"name":tool,"arguments":args})
    }
    fn output(id: &str, body: &str) -> Value {
        json!({"type":"function_call_output","call_id":id,"output":body})
    }

    /// With no key the Jev pre-pass must be a pure no-op: the caller keeps its
    /// normal path, and nothing is dropped.
    #[test]
    fn jev_compaction_is_absent_without_a_key() {
        let cfg = crate::config::TypesafeConfig {
            enabled: true,
            api_key: String::new(),
            ..crate::config::TypesafeConfig::default()
        };
        // Force the ambient environment out of the way so this is deterministic.
        for var in crate::typesafe::client::KEY_ENVS {
            std::env::remove_var(var);
        }
        let items = vec![call("c1"), output("c1", "contents of a.rs")];
        let opts = crate::typesafe::compact::CompactOptions::from_config(&cfg.compaction);
        if crate::typesafe::client::api_key(&cfg).is_none() {
            assert!(
                crate::typesafe::compact::try_compact_items(&cfg, &items, &opts)
                    .ok()
                    .is_none(),
                "no key must never prune"
            );
        }
    }

    /// Meta tools are not worth a judgment: gating a judgment call cannot find
    /// risk, and grading a skill load cannot find a failure. Real tools - reads,
    /// shell, edits - always are.
    #[test]
    fn meta_tools_are_left_out_of_the_judgments() {
        for t in ["typesafe", "skill", "todo_write"] {
            assert!(!typesafe_judges_tool(t), "{t} is bookkeeping");
        }
        for t in [
            "read_file",
            "bash",
            "grep",
            "edit_file",
            "web_fetch",
            "agent",
        ] {
            assert!(typesafe_judges_tool(t), "{t} must still be judged");
        }
    }

    /// Compaction's candidate facts are built in code, from the live transcript.
    #[test]
    fn call_facts_find_duplicates_and_repeated_failures() {
        let items = vec![
            // The shell call comes first: any state-changing call after the
            // read would (correctly) make the earlier contents stale.
            call_args("c2", "bash", "{\"command\":\"cargo test\"}"),
            output("c2", "error: test failed"),
            call_args("c1", "read_file", "{\"path\":\"a.rs\"}"),
            output("c1", "first contents"),
        ];
        let pending = vec![
            // Same file as c1 -> a duplicate with a usable result.
            FunctionCallRef {
                call_id: "n1".into(),
                name: "read_file".into(),
                // Structurally identical, formatted differently.
                arguments: "{ \"path\" : \"a.rs\" }".into(),
            },
            // Same command as c2, which failed -> a repeat of a failure.
            FunctionCallRef {
                call_id: "n2".into(),
                name: "bash".into(),
                arguments: "{\"command\":\"cargo test\"}".into(),
            },
            FunctionCallRef {
                call_id: "n3".into(),
                name: "read_file".into(),
                arguments: "{\"path\":\"unseen.rs\"}".into(),
            },
        ];
        let facts = typesafe_call_facts(&pending, &items);
        assert!(
            facts[0].0.is_some(),
            "identical earlier call is a candidate"
        );
        assert_eq!(facts[0].1, 0, "that earlier call succeeded");
        assert_eq!(facts[1].1, 1, "one prior identical failure");
        assert!(
            facts[1].0.is_none(),
            "a failed call is not a usable duplicate"
        );
        assert!(facts[2].0.is_none(), "nothing to compare against");
        assert_eq!(facts[2].1, 0);
    }

    /// "Did anything change since?" is a fact, not a judgment. A read repeated
    /// after an edit must never be offered to the gate as redundant: a
    /// confident yes would skip the read and leave the model on stale contents.
    #[test]
    fn call_facts_reset_after_an_intervening_change() {
        let edit = |id: &str| {
            call_args(
                id,
                "edit_file",
                "{\"path\":\"a.rs\",\"old\":\"x\",\"new\":\"y\"}",
            )
        };
        let read = |id: &str| call_args(id, "read_file", "{\"path\":\"a.rs\"}");
        let test = |id: &str| call_args(id, "bash", "{\"command\":\"cargo test\"}");
        let pending = |name: &str, args: &str| FunctionCallRef {
            call_id: "n".into(),
            name: name.into(),
            arguments: args.into(),
        };
        let reread = pending("read_file", "{\"path\":\"a.rs\"}");
        let retest = pending("bash", "{\"command\":\"cargo test\"}");

        // read, edit, read again -> the earlier contents are stale.
        let items = vec![
            read("c1"),
            output("c1", "old"),
            edit("c2"),
            output("c2", "ok"),
        ];
        assert!(
            typesafe_call_facts(std::slice::from_ref(&reread), &items)[0]
                .0
                .is_none()
        );

        // An edit earlier in the same pending batch counts as a change too.
        let items = vec![read("c1"), output("c1", "old")];
        let batch = [
            pending(
                "edit_file",
                "{\"path\":\"a.rs\",\"old\":\"x\",\"new\":\"y\"}",
            ),
            reread.clone(),
        ];
        assert!(typesafe_call_facts(&batch, &items)[1].0.is_none());

        // The latest unchanged read is the duplicate, not the oldest.
        let items = vec![
            read("c1"),
            output("c1", "old"),
            edit("c2"),
            output("c2", "ok"),
            read("c3"),
            output("c3", "new"),
        ];
        assert_eq!(
            typesafe_call_facts(std::slice::from_ref(&reread), &items)[0].0,
            Some(2)
        );

        // A failure before a fix is not "the same failure again".
        let items = vec![
            test("c1"),
            output("c1", "error: failed"),
            edit("c2"),
            output("c2", "ok"),
        ];
        assert_eq!(
            typesafe_call_facts(std::slice::from_ref(&retest), &items)[0].1,
            0
        );
        // Re-running the identical failing command changes nothing relevant.
        let items = vec![
            test("c1"),
            output("c1", "error: failed"),
            test("c2"),
            output("c2", "error: failed"),
        ];
        assert_eq!(
            typesafe_call_facts(std::slice::from_ref(&retest), &items)[0].1,
            2
        );

        // A result Jev compaction cut down is no longer in context in full.
        let items = vec![
            read("c1"),
            output("c1", "head\n[… 900 chars dropped by Jev-scored compaction from a tool result; the call kept above is unchanged, and the tool can be re-run …]"),
        ];
        assert!(
            typesafe_call_facts(std::slice::from_ref(&reread), &items)[0]
                .0
                .is_none()
        );
    }

    #[test]
    fn the_same_call_is_recognised_through_formatting() {
        let items = vec![
            json!({"type":"function_call","call_id":"c1","name":"bash","arguments":"{\"command\":\"cargo test\",\"timeout\":10}"}),
            output("c1", "error: test failed"),
            // Same call, key order and spacing differ: still the same call.
            json!({"type":"function_call","call_id":"c2","name":"bash","arguments":"{ \"timeout\": 10, \"command\": \"cargo test\" }"}),
            output("c2", "error: test failed"),
        ];
        let pending = vec![FunctionCallRef {
            call_id: "n1".into(),
            name: "bash".into(),
            arguments: "{\"command\":\"cargo test\",\"timeout\":10}".into(),
        }];
        let facts = typesafe_call_facts(&pending, &items);
        assert_eq!(facts[0].1, 2, "both earlier attempts count as failures");
        assert!(
            facts[0].0.is_none(),
            "a failed call is not a usable duplicate"
        );
    }

    /// The trace evidence handed to the skill check is bounded and one-lined.
    #[test]
    fn trace_evidence_is_bounded() {
        let mut items = Vec::new();
        for i in 0..40 {
            items.push(call(&format!("c{i}")));
            items.push(output(&format!("c{i}"), &"x".repeat(5_000)));
        }
        let evidence = typesafe_trace_evidence(&items, 24);
        assert_eq!(evidence.len(), 24);
        assert!(
            evidence.iter().all(|e| e.chars().count() < 600),
            "every line is bounded"
        );
    }
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

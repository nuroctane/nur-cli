//! Nested subagent runner — Claude Code Task-tool style.

use super::mode::{PermissionMode, SharedMode};
use super::session::Session;
use super::swarm::{self, RunState};
use super::{AgentEvent, AgentRunner, ApprovalDecision};
use crate::api::ApiClient;
use crate::config::Config;
use crate::error::{MuseError, Result};
use crate::tools::ToolHost;
use crate::usage::{TokenUsage, UsageTracker};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[allow(clippy::too_many_arguments)] // Cohesive spawn boundary; these inputs define one child run.
pub async fn run_subagent(
    client: ApiClient,
    config: Config,
    cwd: PathBuf,
    parent_mode: SharedMode,
    prompt: &str,
    subagent_type: &str,
    cancel: &CancellationToken,
    parent_tx: &mpsc::UnboundedSender<AgentEvent>,
) -> Result<(String, TokenUsage)> {
    let explore = matches!(
        subagent_type.to_ascii_lowercase().as_str(),
        "explore" | "research" | "readonly"
    );

    let mode = if explore {
        SharedMode::new(PermissionMode::Plan)
    } else {
        // General inherits parent mode but never upgrades beyond parent auto.
        SharedMode::new(parent_mode.get())
    };

    let mut cfg = config;
    // Subagents inherit the parent's turn budget verbatim — no extra ceiling.
    // A hidden `min(20)` here meant anyone who set an explicit limit silently
    // got 20 for every swarm child, and the child's death surfaced only as the
    // tool-result string "error: max turns reached (20)".
    if explore {
        cfg.reasoning_effort = "medium".into();
    }

    let host = ToolHost::default();
    let runner = Arc::new(AgentRunner {
        client,
        config: cfg.clone(),
        cwd: cwd.clone(),
        permission_mode: mode,
        verbose: false,
        approved_tools: Arc::new(Mutex::new(HashSet::new())),
        tools: host,
        permissions: super::SharedPermissions::load(&cwd),
        hooks: super::hooks::HooksConfig::load(),
        is_subagent: true,
        prewalk_override: Arc::new(Mutex::new(None)),
    });

    let session = Session::new(&cfg.model, &cwd.display().to_string());
    // Scoped: don't clobber the global status.json / Orca display.
    let mut usage = UsageTracker::scoped(session.id.clone(), cfg.model.clone(), cwd);
    usage.set_provider(cfg.provider.clone());

    let (tx, mut rx) = mpsc::unbounded_channel::<AgentEvent>();
    let cancel = cancel.clone();
    let task = prompt.to_string();
    let prompt = format!(
        "[SUBAGENT:{subagent_type}] {prompt}\n\n\
         When finished, respond with a concise report: findings, files touched/read, next steps. \
         Do not ask the user questions."
    );

    let handle = super::spawn_turn(runner, session, usage, prompt, tx, cancel);

    // Publish this run to the shared table the inline `/swarm` card reads,
    // including where it was routed — the whole point of a cross-provider
    // fan-out is being able to see that it actually went elsewhere.
    let run_id = swarm::begin_on(subagent_type, &task, &cfg.provider, &cfg.model);

    let mut last_text = String::new();
    while let Some(ev) = rx.recv().await {
        match ev {
            AgentEvent::Status(status) => {
                swarm::activity(run_id, &status);
                let _ = parent_tx.send(AgentEvent::Status(format!("subagent · {status}")));
            }
            AgentEvent::ToolStart { name, args, .. } => {
                swarm::tool_start_with(run_id, &name, &args);
            }
            AgentEvent::ToolEnd {
                name: _,
                result,
                ok,
                ..
            } => {
                swarm::tool_end_with(run_id, ok, &result);
            }
            // A nested subagent asked for a provider with no creds — relay the
            // signal up so the top-level TUI can pop the pre-selected /login.
            AgentEvent::LoginRequired {
                provider_id,
                provider_name,
                retry_prompt,
                retry_desc,
                retry_kind,
                retry_model,
            } => {
                let _ = parent_tx.send(AgentEvent::LoginRequired {
                    provider_id,
                    provider_name,
                    retry_prompt,
                    retry_desc,
                    retry_kind,
                    retry_model,
                });
            }
            AgentEvent::ApprovalRequest {
                name,
                args,
                respond,
            } => {
                relay_approval(parent_tx, name, args, respond).await;
            }
            AgentEvent::TextDelta(d) => {
                swarm::thinking(run_id);
                last_text.push_str(&d);
            }
            AgentEvent::AssistantMessage(m) => {
                if !m.is_empty() {
                    last_text = m;
                }
            }
            AgentEvent::Done {
                result,
                interrupted,
                usage,
                ..
            } => {
                let _ = handle.await;
                let spent = usage.session_usage().clone();
                let tokens = spent.input_tokens + spent.output_tokens;
                if interrupted {
                    swarm::finish(run_id, RunState::Cancelled, tokens);
                    return Err(MuseError::Interrupted);
                }
                return match result {
                    Ok(s) => {
                        swarm::finish(run_id, RunState::Done, tokens);
                        Ok((if s.trim().is_empty() { last_text } else { s }, spent))
                    }
                    Err(e) => {
                        swarm::finish(run_id, RunState::Failed, tokens);
                        Err(subagent_failure(&e, &last_text))
                    }
                };
            }
            _ => {}
        }
    }
    let _ = handle.await;
    swarm::finish(run_id, RunState::Failed, 0);
    Err(subagent_failure(
        "event channel closed before the child reported completion",
        &last_text,
    ))
}

/// A child failure stays a failure even when it streamed useful partial text.
///
/// Treating partial output as `Ok` made the parent believe the delegated task
/// completed, so it would continue from an unverified half-result instead of
/// retrying or reporting the provider/runtime failure.
fn subagent_failure(error: &str, partial: &str) -> MuseError {
    let partial = partial.trim();
    if partial.is_empty() {
        MuseError::Other(format!("subagent failed: {error}"))
    } else {
        MuseError::Other(format!(
            "subagent failed: {error}\n\nPartial output before failure:\n{partial}"
        ))
    }
}

/// One approval prompt at a time, across every concurrent subagent.
///
/// The parent has a single approval slot (TUI dialogue / terminal prompt), so
/// two children asking at once would clobber each other — the loser's channel
/// drops and it silently reads as a denial. Children queue here instead.
fn approval_turnstile() -> &'static tokio::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(Default::default)
}

#[cfg(test)]
/// Exposes the turnstile-guarded relay to the loop's concurrency tests.
pub async fn relay_approval_for_test(
    parent_tx: &mpsc::UnboundedSender<AgentEvent>,
    name: String,
    args: String,
    respond: tokio::sync::oneshot::Sender<ApprovalDecision>,
) {
    relay_approval(parent_tx, name, args, respond).await
}

/// Proxy a child approval through the parent event loop, which is the only
/// runner that has a terminal prompt or TUI approval surface.
async fn relay_approval(
    parent_tx: &mpsc::UnboundedSender<AgentEvent>,
    name: String,
    args: String,
    respond: tokio::sync::oneshot::Sender<ApprovalDecision>,
) {
    let _turn = approval_turnstile().lock().await;
    let (proxy_tx, proxy_rx) = tokio::sync::oneshot::channel();
    if parent_tx
        .send(AgentEvent::ApprovalRequest {
            name,
            args,
            respond: proxy_tx,
        })
        .is_err()
    {
        let _ = respond.send(ApprovalDecision::Deny);
    } else {
        let decision = proxy_rx.await.unwrap_or(ApprovalDecision::Deny);
        let _ = respond.send(decision);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_child_output_never_turns_a_failure_into_success() {
        let error = subagent_failure("provider disconnected", "inspected auth.rs");
        let message = error.to_string();
        assert!(message.starts_with("subagent failed: provider disconnected"));
        assert!(message.contains("Partial output before failure"));
        assert!(message.contains("inspected auth.rs"));
    }

    #[test]
    fn child_failure_without_output_is_still_actionable() {
        assert_eq!(
            subagent_failure("event channel closed", "").to_string(),
            "subagent failed: event channel closed"
        );
    }

    #[tokio::test]
    async fn child_approval_is_proxied_to_parent() {
        let (parent_tx, mut parent_rx) = mpsc::unbounded_channel();
        let (child_tx, child_rx) = tokio::sync::oneshot::channel();
        let parent_for_relay = parent_tx.clone();
        let relay = tokio::spawn(async move {
            relay_approval(
                &parent_for_relay,
                "write_file".into(),
                "{}".into(),
                child_tx,
            )
            .await;
        });

        let event = parent_rx.recv().await.expect("parent approval event");
        let AgentEvent::ApprovalRequest { respond, .. } = event else {
            panic!("expected approval request");
        };
        respond.send(ApprovalDecision::Approve).unwrap();
        assert_eq!(child_rx.await.unwrap(), ApprovalDecision::Approve);
        relay.await.unwrap();
    }
}

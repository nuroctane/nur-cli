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
    // Soft cap subagent depth only when the parent has an explicit positive
    // turn limit. Parent unlimited (0) must stay unlimited for nested work.
    if cfg.max_turns > 0 {
        cfg.max_turns = cfg.max_turns.min(20);
    }
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

    // Publish this run to the shared table the inline `/swarm` card reads.
    let run_id = swarm::begin(subagent_type, &task);

    let mut last_text = String::new();
    while let Some(ev) = rx.recv().await {
        match ev {
            AgentEvent::Status(status) => {
                swarm::activity(run_id, &status);
                let _ = parent_tx.send(AgentEvent::Status(format!(
                    "subagent · {status}"
                )));
            }
            AgentEvent::ToolStart { name, .. } => swarm::tool_start(run_id, &name),
            AgentEvent::ToolEnd { ok, .. } => swarm::tool_end(run_id, ok),
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
                        if !last_text.trim().is_empty() {
                            Ok((format!("{last_text}\n\n(subagent ended: {e})"), spent))
                        } else {
                            Err(MuseError::Other(e))
                        }
                    }
                };
            }
            _ => {}
        }
    }
    let _ = handle.await;
    if last_text.is_empty() {
        swarm::finish(run_id, RunState::Failed, 0);
        Err(MuseError::Other("subagent produced no output".into()))
    } else {
        swarm::finish(run_id, RunState::Done, 0);
        Ok((last_text, TokenUsage::default()))
    }
}

/// Proxy a child approval through the parent event loop, which is the only
/// runner that has a terminal prompt or TUI approval surface.
async fn relay_approval(
    parent_tx: &mpsc::UnboundedSender<AgentEvent>,
    name: String,
    args: String,
    respond: tokio::sync::oneshot::Sender<ApprovalDecision>,
) {
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

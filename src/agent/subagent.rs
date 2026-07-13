//! Nested subagent runner — Claude Code Task-tool style.

use super::mode::{PermissionMode, SharedMode};
use super::session::Session;
use super::{AgentEvent, AgentRunner};
use crate::api::MetaClient;
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
    client: MetaClient,
    config: Config,
    cwd: PathBuf,
    parent_mode: SharedMode,
    prompt: &str,
    subagent_type: &str,
    cancel: &CancellationToken,
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
    // Cap subagent depth/cost
    cfg.max_turns = cfg.max_turns.min(20);
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
        is_subagent: true,
    });

    let session = Session::new(&cfg.model, &cwd.display().to_string());
    // Scoped: don't clobber the global status.json / Orca display.
    let usage = UsageTracker::scoped(session.id.clone(), cfg.model.clone(), cwd);

    let (tx, mut rx) = mpsc::unbounded_channel::<AgentEvent>();
    let cancel = cancel.clone();
    let prompt = format!(
        "[SUBAGENT:{subagent_type}] {prompt}\n\n\
         When finished, respond with a concise report: findings, files touched/read, next steps. \
         Do not ask the user questions."
    );

    let handle = super::spawn_turn(runner, session, usage, prompt, tx, cancel);

    let mut last_text = String::new();
    while let Some(ev) = rx.recv().await {
        match ev {
            AgentEvent::TextDelta(d) => last_text.push_str(&d),
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
                if interrupted {
                    return Err(MuseError::Interrupted);
                }
                return match result {
                    Ok(s) => Ok((
                        if s.trim().is_empty() { last_text } else { s },
                        spent,
                    )),
                    Err(e) => {
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
        Err(MuseError::Other("subagent produced no output".into()))
    } else {
        Ok((last_text, TokenUsage::default()))
    }
}

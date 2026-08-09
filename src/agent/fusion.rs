//! Fusion executor — fans a question out to the panel concurrently and streams
//! the synthesized answer back to the TUI. The (pure, unit-tested) request
//! builders live in [`crate::api::fusion`].

use crate::agent::{AgentEvent, Session};
use crate::api::failover::{plan_targets, resolve_target_key};
use crate::api::fusion::{self, PanelAnswer};
use crate::api::types::{replay_output_items, user_text_item};
use crate::api::{ApiClient, ApiResponse};
use crate::usage::{TokenUsage, UsageTracker};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Spawn a `/fusion` run. Owns `session` + `usage` exactly like
/// [`crate::agent::spawn_turn`], and emits `Done { session, usage, .. }` so the
/// TUI's normal end-of-turn cleanup restores state (busy flag, saved session).
#[allow(clippy::too_many_arguments)]
pub fn spawn_fusion(
    primary: ApiClient,
    primary_provider_id: String,
    primary_model: String,
    panel_ids: Vec<String>,
    question: String,
    mut session: Box<Session>,
    mut usage: Box<UsageTracker>,
    tx: mpsc::UnboundedSender<AgentEvent>,
    cancel: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let result = run(
            &primary,
            &primary_provider_id,
            &primary_model,
            &panel_ids,
            &question,
            &mut session,
            &mut usage,
            &tx,
            &cancel,
        )
        .await;

        usage.set_state("idle");
        let _ = session.save();
        let interrupted = cancel.is_cancelled();
        let _ = tx.send(AgentEvent::Usage {
            session: usage.session_usage().clone(),
            last: usage.last_usage().clone(),
        });
        let _ = tx.send(AgentEvent::Done {
            session,
            usage,
            result,
            interrupted,
        });
    })
}

#[allow(clippy::too_many_arguments)]
async fn run(
    primary: &ApiClient,
    primary_provider_id: &str,
    primary_model: &str,
    panel_ids: &[String],
    question: &str,
    session: &mut Session,
    usage: &mut UsageTracker,
    tx: &mpsc::UnboundedSender<AgentEvent>,
    cancel: &CancellationToken,
) -> Result<String, String> {
    // The panel: the active model first, then every configured provider we
    // actually hold a key for (reusing the failover credential resolver — a
    // panel provider never borrows the primary's auth.json).
    let targets = plan_targets(primary_provider_id, panel_ids, resolve_target_key);
    for requested in panel_ids {
        let id = requested.trim();
        if id.is_empty() || id == primary_provider_id || crate::providers::by_id(id).is_none() {
            continue;
        }
        if targets.iter().any(|target| target.provider_id == id) {
            continue;
        }
        let provider = crate::providers::by_id(id).expect("checked provider");
        let _ = tx.send(AgentEvent::FusionLoginRequired {
            provider_id: provider.id.to_string(),
            provider_name: provider.name.to_string(),
            question: question.to_string(),
            panel_ids: panel_ids.to_vec(),
        });
        return Err(format!(
            "fusion · waiting for {} authentication; request preserved",
            provider.name
        ));
    }

    // A fusion answer is real conversation history, not a disposable side
    // query. Persist the question and final synthesis so /resume, /sessions,
    // forks, and cross-session chat continue from exactly what the user saw.
    session.push_user(question);
    session.input_items.push(user_text_item(question));
    let panel_n = targets.len() + 1;

    let _ = tx.send(AgentEvent::Status(format!(
        "fusion · asking {panel_n} model{}…",
        if panel_n == 1 { "" } else { "s" }
    )));

    // Kick every member off concurrently; each task yields (label, result).
    let primary_effective_model = primary.resolve_local_model(primary_model).await;
    let mut futs = Vec::new();
    {
        let c = primary.clone();
        let model = primary_effective_model.clone();
        let label = fusion::label(primary_provider_id, &model);
        let req = fusion::question_request(&model, question);
        let provider_id = primary_provider_id.to_string();
        futs.push(tokio::spawn(async move {
            let response = c.create_response(&req).await;
            (provider_id, model, label, response)
        }));
    }
    for t in &targets {
        let client = match ApiClient::for_provider(&t.base_url, &t.api_key, &t.provider_id) {
            Ok(c) => c.with_style(t.style),
            Err(_) => continue,
        };
        let model = client.resolve_local_model(&t.model).await;
        let label = fusion::label(&t.provider_id, &model);
        let req = fusion::question_request(&model, question);
        let provider_id = t.provider_id.clone();
        futs.push(tokio::spawn(async move {
            let response = client.create_response(&req).await;
            (provider_id, model, label, response)
        }));
    }

    let mut answers: Vec<PanelAnswer> = Vec::new();
    for f in futs {
        if cancel.is_cancelled() {
            return Err("interrupted".into());
        }
        match f.await {
            Ok((provider_id, model, label, Ok(resp))) => {
                meter(&resp, session, usage, &provider_id, &model);
                answers.push(PanelAnswer {
                    label,
                    text: resp.output_text(),
                    ok: true,
                });
            }
            Ok((_, _, label, Err(e))) => {
                answers.push(PanelAnswer {
                    label,
                    text: e.to_string(),
                    ok: false,
                });
            }
            // Task join failure (panic/cancel) — drop that member.
            Err(_) => {}
        }
    }

    let ok: Vec<PanelAnswer> = answers
        .iter()
        .filter(|a| a.ok && !a.text.trim().is_empty())
        .cloned()
        .collect();
    let ok_n = ok.len();
    if ok_n == 0 {
        return Err(
            "fusion · every panelist failed — check keys with /failover and the panel with /fusion"
                .into(),
        );
    }

    // Single-line roster for the status bar (provider ids + ✓/✗).
    let roster = answers
        .iter()
        .map(|a| {
            let id = a.label.split(" · ").next().unwrap_or(&a.label);
            format!("{id}{}", if a.ok { "✓" } else { "✗" })
        })
        .collect::<Vec<_>>()
        .join(", ");
    let _ = tx.send(AgentEvent::Status(format!(
        "fusion · {ok_n}/{panel_n} answered: {roster} · synthesizing…"
    )));

    if cancel.is_cancelled() {
        return Err("interrupted".into());
    }

    // Judge = the active model (best key + reasoning budget).
    let synth = fusion::synthesis_request(&primary_effective_model, question, &ok);
    match primary.create_response(&synth).await {
        Ok(resp) => {
            meter(
                &resp,
                session,
                usage,
                primary_provider_id,
                &primary_effective_model,
            );
            let fused = resp.output_text();
            session
                .input_items
                .extend(replay_output_items(&resp.output));
            session.push_assistant(&fused);
            let _ = tx.send(AgentEvent::AssistantMessage(fused.clone()));
            Ok(fused)
        }
        Err(e) => Err(format!("fusion · synthesis failed: {e}")),
    }
}

/// Fold one response's token usage into the session total + the tracker.
fn meter(
    resp: &ApiResponse,
    session: &mut Session,
    usage: &mut UsageTracker,
    provider: &str,
    model: &str,
) {
    if let Some(u) = &resp.usage {
        let tu: TokenUsage = u.into();
        usage.record_request_for_route(provider, model, tu, resp.id.clone());
        session.usage.add(usage.last_usage());
    }
}

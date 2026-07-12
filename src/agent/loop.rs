use super::prompt::system_instructions;
use super::session::Session;
use crate::api::types::{
    function_call_output_item, replay_output_items, user_text_item, ReasoningConfig,
    ResponseRequest,
};
use crate::api::{ApiResponse, MetaClient, StreamEvent};
use crate::config::Config;
use crate::error::{MuseError, Result};
use crate::tools::{dispatch, tool_defs, ToolContext};
use crate::usage::{TokenUsage, UsageTracker};
use serde_json::Value;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

/// Events emitted while an agent turn runs. The UI (TUI or headless printer)
/// consumes these live from an unbounded channel.
pub enum AgentEvent {
    /// Short status line ("thinking (turn 2)").
    Status(String),
    /// Reasoning-summary text delta (model thinking).
    ReasoningDelta(String),
    /// Assistant output text delta (streamed).
    TextDelta(String),
    /// Complete assistant message for a request that did not stream.
    AssistantMessage(String),
    /// A tool call is starting.
    ToolStart { id: u64, name: String, args: String },
    /// A tool call finished.
    ToolEnd {
        id: u64,
        name: String,
        result: String,
        ok: bool,
    },
    /// The agent wants to run a mutating tool — reply on `respond`.
    ApprovalRequest {
        name: String,
        args: String,
        respond: oneshot::Sender<ApprovalDecision>,
    },
    /// Cumulative + last-request token usage after each API response.
    Usage { session: TokenUsage, last: TokenUsage },
    /// Turn finished; ownership of session/usage returns to the caller.
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

/// Tools that never mutate the workspace — auto-approved.
pub const READ_ONLY_TOOLS: &[&str] = &["read_file", "grep", "glob"];

pub struct AgentRunner {
    pub client: MetaClient,
    pub config: Config,
    pub cwd: PathBuf,
    pub auto_approve: bool,
    #[allow(dead_code)]
    pub verbose: bool,
    /// Tools the user approved for the whole session ("always allow").
    pub approved_tools: Arc<Mutex<HashSet<String>>>,
}

/// Spawn a full agent turn on the runtime. Events (including the final
/// `Done`, which carries session + usage back) arrive on `tx`.
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
    /// Run one user turn: repeated model requests + tool dispatch until the
    /// model answers without tool calls. Emits events on `tx` throughout.
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

        let instructions = system_instructions(&self.cwd);
        let tools = tool_defs();

        let mut turns = 0u32;
        let mut tool_seq: u64 = 0;

        loop {
            if cancel.is_cancelled() {
                return Err(MuseError::Interrupted);
            }
            turns += 1;
            if turns > self.config.max_turns {
                return Err(MuseError::MaxTurns(self.config.max_turns));
            }

            usage.set_state(format!("thinking (turn {turns})"));
            let _ = tx.send(AgentEvent::Status(format!("thinking · turn {turns}")));

            let req = ResponseRequest {
                model: self.config.model.clone(),
                input: Value::Array(session.input_items.clone()),
                instructions: Some(instructions.clone()),
                tools: Some(tools.clone()),
                tool_choice: Some("auto".into()),
                store: Some(false),
                include: Some(vec!["reasoning.encrypted_content".into()]),
                reasoning: Some(ReasoningConfig {
                    effort: Some(self.config.reasoning_effort.clone()),
                    summary: Some("auto".into()),
                }),
                stream: Some(self.config.stream),
                parallel_tool_calls: Some(true),
            };

            let mut text_deltas = 0usize;
            let resp: ApiResponse = if self.config.stream {
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

            // Replay model output into history for next turn.
            let replayed = replay_output_items(&resp.output);
            session.input_items.extend(replayed);

            let calls = resp.function_calls();
            let text = resp.output_text();

            // If nothing streamed, surface the message text as one block.
            if text_deltas == 0 && !text.is_empty() {
                let _ = tx.send(AgentEvent::AssistantMessage(text.clone()));
            }

            if calls.is_empty() {
                usage.set_state("idle");
                session.push_assistant(&text);
                let _ = session.save();
                return Ok(text);
            }

            for call in calls {
                if cancel.is_cancelled() {
                    session
                        .input_items
                        .push(function_call_output_item(&call.call_id, "[interrupted by user]"));
                    let _ = session.save();
                    return Err(MuseError::Interrupted);
                }

                tool_seq += 1;
                let id = tool_seq;
                let _ = tx.send(AgentEvent::ToolStart {
                    id,
                    name: call.name.clone(),
                    args: call.arguments.clone(),
                });

                // Approval gate for mutating tools.
                let approved = self.check_approval(&call.name, &call.arguments, tx).await;
                if !approved {
                    let msg = "user denied this tool call; ask before retrying or take another approach";
                    let _ = tx.send(AgentEvent::ToolEnd {
                        id,
                        name: call.name.clone(),
                        result: "denied by user".into(),
                        ok: false,
                    });
                    session
                        .input_items
                        .push(function_call_output_item(&call.call_id, msg));
                    continue;
                }

                usage.set_state(format!("tool:{}", call.name));
                let tool_ctx = ToolContext {
                    cwd: self.cwd.clone(),
                    auto_approve: true, // gate handled above
                };
                let name = call.name.clone();
                let args = call.arguments.clone();
                let exec = tokio::task::spawn_blocking(move || dispatch(&name, &args, &tool_ctx));
                let result = tokio::select! {
                    _ = cancel.cancelled() => {
                        session.input_items.push(function_call_output_item(
                            &call.call_id,
                            "[interrupted by user]",
                        ));
                        let _ = session.save();
                        return Err(MuseError::Interrupted);
                    }
                    r = exec => match r {
                        Ok(Ok(s)) => (s, true),
                        Ok(Err(e)) => (format!("error: {e}"), false),
                        Err(e) => (format!("error: tool panicked: {e}"), false),
                    },
                };

                let _ = tx.send(AgentEvent::ToolEnd {
                    id,
                    name: call.name.clone(),
                    result: result.0.clone(),
                    ok: result.1,
                });

                session
                    .input_items
                    .push(function_call_output_item(&call.call_id, &result.0));
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
        if self.auto_approve || READ_ONLY_TOOLS.contains(&name) {
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
            _ => false,
        }
    }
}

/// Compact a session: ask the model to summarize the conversation, then
/// replace the input history with the summary. Returns the summary text.
pub async fn compact_session(
    runner: &AgentRunner,
    session: &mut Session,
    usage: &mut UsageTracker,
) -> Result<String> {
    let mut items = session.input_items.clone();
    items.push(user_text_item(
        "Summarize this conversation so far for a fresh context window. Capture: the user's goals, \
         decisions made, files touched (paths), current state of the work, and any pending next steps. \
         Be dense and factual; use bullet points.",
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

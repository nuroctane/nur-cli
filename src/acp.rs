//! ACP v1 server backed by Nur sessions.
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use agent_client_protocol::v1::{schema, ConnectionTo, Responder};
use agent_client_protocol::{Agent, Client, Stdio};
use tokio_util::sync::CancellationToken;

use crate::agent::{
    self, AgentEvent, AgentRunner, ApprovalDecision, PermissionMode, Session, SharedMode,
};
use crate::api::ApiClient;
use crate::config::Config;
use crate::usage::UsageTracker;

struct Entry {
    runner: Arc<AgentRunner>,
    turn: Arc<tokio::sync::Mutex<Option<(Session, UsageTracker)>>>,
    cancel: Mutex<Option<CancellationToken>>,
}
struct Server {
    client: ApiClient,
    config: Config,
    sessions: Mutex<HashMap<String, Arc<Entry>>>,
    elicitation: AtomicBool,
}

fn invalid(message: &str) -> agent_client_protocol::Error {
    agent_client_protocol::Error::invalid_params().data(message.to_string())
}

impl Server {
    fn new_session(
        &self,
        request: schema::NewSessionRequest,
    ) -> agent_client_protocol::Result<String> {
        if !request.cwd.is_absolute() || !request.cwd.is_dir() {
            return Err(invalid(
                "session cwd must be an existing absolute directory",
            ));
        }
        if !request.additional_directories.is_empty() {
            return Err(invalid("additional directories are not supported"));
        }
        let cwd = request.cwd;
        let mut scoped_servers = std::collections::BTreeMap::new();
        for server in request.mcp_servers {
            let schema::McpServer::Stdio(server) = server else {
                return Err(invalid("only stdio client MCP servers are supported"));
            };
            if server.name.trim().is_empty()
                || server.name.starts_with("__nur_acp_")
                || !server.command.is_absolute()
                || server.command.to_str().is_none()
            {
                return Err(invalid(
                    "client MCP servers need a name and an absolute executable path",
                ));
            }
            let mut env = std::collections::BTreeMap::new();
            for variable in server.env {
                if variable.name.is_empty()
                    || variable.name.contains(['=', '\0'])
                    || variable.value.contains('\0')
                    || env.insert(variable.name, variable.value).is_some()
                {
                    return Err(invalid(
                        "invalid or duplicate client MCP environment variable",
                    ));
                }
            }
            let spec = crate::mcp_stdio::McpServerConfig {
                command: server.command.to_string_lossy().into_owned(),
                args: server.args,
                env,
                session_cwd: Some(cwd.clone()),
                ..Default::default()
            };
            if scoped_servers.insert(server.name, spec).is_some() {
                return Err(invalid("duplicate client MCP server name"));
            }
        }
        let mut config = self.config.clone();
        config.scope_to(&cwd);
        let client =
            if config.provider == self.config.provider && config.base_url == self.config.base_url {
                self.client.clone()
            } else {
                let key =
                    crate::auth::resolve_api_key_for(Some(&config.provider)).map_err(|error| {
                        agent_client_protocol::Error::internal_error()
                            .data(crate::secrets::redact(&error.to_string()).into_owned())
                    })?;
                let style = crate::providers::by_id(&config.provider)
                    .map(|provider| provider.style)
                    .unwrap_or(crate::providers::ApiStyle::Responses);
                ApiClient::for_provider(&config.base_url, &key, &config.provider)
                    .map_err(|error| {
                        agent_client_protocol::Error::internal_error()
                            .data(crate::secrets::redact(&error.to_string()).into_owned())
                    })?
                    .with_style(style)
            };
        let session =
            Session::new_for_provider(&config.model, &config.provider, &cwd.to_string_lossy());
        let id = session.id.clone();
        crate::mcp_stdio::register_scoped(&id, scoped_servers)
            .map_err(|_| agent_client_protocol::Error::internal_error())?;
        let mut usage = UsageTracker::new(id.clone(), config.model.clone(), cwd.clone());
        usage.set_provider(config.provider.clone());
        let runner = Arc::new(AgentRunner {
            client,
            config: config.clone(),
            cwd: cwd.clone(),
            permission_mode: SharedMode::new(PermissionMode::Manual),
            approved_tools: Arc::new(Mutex::new(HashSet::new())),
            tools: crate::tools::ToolHost::default(),
            permissions: agent::SharedPermissions::load(&cwd),
            hooks: agent::hooks::HooksConfig::load(),
            is_subagent: false,
            interactive_children: true,
            prewalk_override: Arc::new(Mutex::new(None)),
            approved_domains: Arc::new(Mutex::new(HashSet::new())),
            subagent_depth: 0,
        });
        agent::prompt::prefetch_slow_sources(&cwd, &config);
        let entry = Arc::new(Entry {
            runner,
            turn: Arc::new(tokio::sync::Mutex::new(Some((session, usage)))),
            cancel: Mutex::new(None),
        });
        self.sessions
            .lock()
            .map_err(|_| agent_client_protocol::Error::internal_error())?
            .insert(id.clone(), entry);
        Ok(id)
    }
    fn get(&self, id: &str) -> Option<Arc<Entry>> {
        self.sessions.lock().ok()?.get(id).cloned()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Ok(sessions) = self.sessions.lock() {
            for (id, entry) in sessions.iter() {
                if let Ok(active) = entry.cancel.lock() {
                    if let Some(cancel) = active.as_ref() {
                        cancel.cancel();
                    }
                }
                crate::mcp_stdio::remove_scoped(id);
                crate::tools::debug_tool::clear_session(id);
            }
        }
    }
}

fn prompt_text(blocks: &[schema::ContentBlock]) -> agent_client_protocol::Result<String> {
    let mut parts = Vec::new();
    for block in blocks {
        match block {
            schema::ContentBlock::Text(text) => parts.push(text.text.clone()),
            schema::ContentBlock::ResourceLink(link) => {
                parts.push(format!("{}: {}", link.name, link.uri))
            }
            _ => return Err(invalid("unsupported prompt content")),
        }
    }
    Ok(parts.join("\n"))
}

fn send_text(
    cx: &ConnectionTo<Client>,
    session_id: &schema::SessionId,
    text: &str,
) -> agent_client_protocol::Result<()> {
    if !text.is_empty() {
        let chunk = schema::ContentChunk::new(schema::ContentBlock::Text(
            schema::TextContent::new(crate::secrets::redact(text)),
        ));
        cx.send_notification(schema::SessionNotification::new(
            session_id.clone(),
            schema::SessionUpdate::AgentMessageChunk(chunk),
        ))?;
    }
    Ok(())
}

struct TurnClient {
    session_id: schema::SessionId,
    cx: ConnectionTo<Client>,
    elicitation: bool,
}

async fn serve_turn(
    entry: Arc<Entry>,
    mut guard: tokio::sync::OwnedMutexGuard<Option<(Session, UsageTracker)>>,
    prompt: String,
    cancel: CancellationToken,
    responder: Responder<schema::PromptResponse>,
    client: TurnClient,
) -> agent_client_protocol::Result<()> {
    let TurnClient {
        session_id,
        cx,
        elicitation,
    } = client;
    let (session, usage) = guard.take().ok_or_else(|| invalid("session unavailable"))?;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = agent::spawn_turn(
        entry.runner.clone(),
        session,
        usage,
        prompt,
        tx,
        cancel.clone(),
    );
    // A connection closing must not leave a detached Nur turn behind.
    struct CancelOnDrop(CancellationToken);
    impl Drop for CancelOnDrop {
        fn drop(&mut self) {
            self.0.cancel();
        }
    }
    let _cancel_on_drop = CancelOnDrop(cancel.clone());
    let turn_id = uuid::Uuid::new_v4();
    let mut calls: Vec<(u64, String, String)> = Vec::new();
    // ACP v1 cannot retract a chunk. Buffer until a model response is known to
    // be complete so a stream retry cannot leave partial text on the client.
    let mut pending_text = String::new();
    let mut done = None;
    while let Some(event) = rx.recv().await {
        match event {
            AgentEvent::TextDelta(text) => pending_text.push_str(&text),
            AgentEvent::StreamRestart { discard } => {
                agent::r#loop::discard_streamed_tail(&mut pending_text, discard)
            }
            AgentEvent::AssistantMessage(text) if pending_text.is_empty() => pending_text = text,
            AgentEvent::ToolStart { id, name, args } => {
                send_text(&cx, &session_id, &pending_text)?;
                pending_text.clear();
                let input = serde_json::from_str(&crate::secrets::redact(&args))
                    .unwrap_or(serde_json::Value::Null);
                let call =
                    schema::ToolCall::new(format!("{turn_id}:{id}"), name.clone()).raw_input(input);
                calls.push((id, name, args));
                cx.send_notification(schema::SessionNotification::new(
                    session_id.clone(),
                    schema::SessionUpdate::ToolCall(call),
                ))?;
            }
            AgentEvent::ToolEnd { id, result, ok, .. } => {
                let status = if ok {
                    schema::ToolCallStatus::Completed
                } else {
                    schema::ToolCallStatus::Failed
                };
                let fields = schema::ToolCallUpdateFields::new()
                    .status(status)
                    .raw_output(serde_json::Value::String(
                        crate::secrets::redact(&result).into_owned(),
                    ));
                cx.send_notification(schema::SessionNotification::new(
                    session_id.clone(),
                    schema::SessionUpdate::ToolCallUpdate(schema::ToolCallUpdate::new(
                        format!("{turn_id}:{id}"),
                        fields,
                    )),
                ))?;
            }
            AgentEvent::ApprovalRequest {
                name,
                args,
                respond,
                ..
            } => {
                let id = if let Some((id, _, _)) = calls
                    .iter()
                    .rev()
                    .find(|(_, n, a)| *n == name && *a == args)
                {
                    format!("{turn_id}:{id}")
                } else {
                    let id = uuid::Uuid::new_v4().to_string();
                    let call = schema::ToolCall::new(id.clone(), name.clone());
                    cx.send_notification(schema::SessionNotification::new(
                        session_id.clone(),
                        schema::SessionUpdate::ToolCall(call),
                    ))?;
                    id
                };
                let fields = schema::ToolCallUpdateFields::new().title(name).raw_input(
                    serde_json::from_str(&crate::secrets::redact(&args))
                        .unwrap_or(serde_json::Value::Null),
                );
                let options = vec![
                    schema::PermissionOption::new(
                        "allow",
                        "Allow once",
                        schema::PermissionOptionKind::AllowOnce,
                    ),
                    schema::PermissionOption::new(
                        "reject",
                        "Reject",
                        schema::PermissionOptionKind::RejectOnce,
                    ),
                ];
                let request = schema::RequestPermissionRequest::new(
                    session_id.clone(),
                    schema::ToolCallUpdate::new(id, fields),
                    options,
                );
                let response = tokio::select! { _ = cancel.cancelled() => None, result = cx.send_request(request).block_task() => result.ok() };
                let decision = match response.map(|r| r.outcome) {
                    Some(schema::RequestPermissionOutcome::Selected(selected))
                        if selected.option_id.0.as_ref() == "allow" =>
                    {
                        ApprovalDecision::Approve
                    }
                    _ => ApprovalDecision::Deny,
                };
                let _ = respond.send(decision);
            }
            AgentEvent::QuestionRequest {
                question,
                header,
                options,
                multi_select,
                respond,
                ..
            } => {
                let answer = if !elicitation {
                    agent::QuestionAnswer::unavailable()
                } else {
                    let labels: Vec<String> =
                        options.iter().map(|(label, _)| label.clone()).collect();
                    let descriptions = options
                        .iter()
                        .map(|(label, description)| format!("{label}: {description}"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    let mut form = schema::ElicitationSchema::new().title(header).property(
                        "answer",
                        schema::StringPropertySchema::new().description(descriptions),
                        false,
                    );
                    if multi_select && !labels.is_empty() {
                        form = form.property(
                            "choices",
                            schema::MultiSelectPropertySchema::new(labels.clone()),
                            false,
                        );
                    }
                    let scope = schema::ElicitationSessionScope::new(session_id.clone());
                    let request = schema::CreateElicitationRequest::new(
                        schema::ElicitationFormMode::new(scope, form),
                        question,
                    );
                    let response = tokio::select! { _ = cancel.cancelled() => None, result = cx.send_request(request).block_task() => result.ok() };
                    match response.map(|r| r.action) {
                        Some(schema::ElicitationAction::Accept(answer)) => {
                            let content = answer.content.unwrap_or_default();
                            match (content.get("choices"), content.get("answer")) {
                                (
                                    Some(schema::ElicitationContentValue::StringArray(selected)),
                                    _,
                                ) if multi_select
                                    && !selected.is_empty()
                                    && selected.iter().all(|s| labels.contains(s)) =>
                                {
                                    agent::QuestionAnswer::picked(selected.clone())
                                }
                                (_, Some(schema::ElicitationContentValue::String(text)))
                                    if labels.contains(text) =>
                                {
                                    agent::QuestionAnswer::picked(vec![text.clone()])
                                }
                                (_, Some(schema::ElicitationContentValue::String(text)))
                                    if !text.trim().is_empty() =>
                                {
                                    agent::QuestionAnswer::typed(text.clone())
                                }
                                _ => agent::QuestionAnswer::dismissed(),
                            }
                        }
                        _ => agent::QuestionAnswer::dismissed(),
                    }
                };
                let _ = respond.send(answer);
            }
            AgentEvent::SecretRequest { respond, .. } => {
                let _ = respond.send(None);
            }
            AgentEvent::Done {
                session,
                usage,
                result,
                interrupted,
                ..
            } => {
                *guard = Some((*session, *usage));
                done = Some((result, interrupted));
                break;
            }
            _ => {}
        }
    }
    if let Ok(mut active) = entry.cancel.lock() {
        *active = None;
    }
    let _ = handle.await;
    let (result, interrupted) = done.ok_or_else(agent_client_protocol::Error::internal_error)?;
    if interrupted || cancel.is_cancelled() {
        return responder.respond(schema::PromptResponse::new(schema::StopReason::Cancelled));
    }
    match result {
        Ok(text) => {
            send_text(&cx, &session_id, &text)?;
            responder.respond(schema::PromptResponse::new(schema::StopReason::EndTurn))
        }
        Err(error) => responder.respond_with_error(
            agent_client_protocol::Error::internal_error().data(crate::secrets::redact(&error)),
        ),
    }
}

pub async fn serve(client: ApiClient, config: Config) -> agent_client_protocol::Result<()> {
    let state = Arc::new(Server {
        client,
        config,
        sessions: Mutex::new(HashMap::new()),
        elicitation: AtomicBool::new(false),
    });
    Agent
        .v1()
        .name("nur")
        .on_receive_request(
            {
                let state = state.clone();
                async move |request: schema::InitializeRequest,
                            responder: Responder<schema::InitializeResponse>,
                            _cx: ConnectionTo<Client>| {
                    state.elicitation.store(
                        request
                            .client_capabilities
                            .elicitation
                            .as_ref()
                            .is_some_and(|caps| caps.supports_form()),
                        Ordering::Relaxed,
                    );
                    responder.respond(
                        schema::InitializeResponse::new(request.protocol_version).agent_info(
                            schema::Implementation::new("nur", env!("CARGO_PKG_VERSION")),
                        ),
                    )
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |request: schema::NewSessionRequest,
                            responder: Responder<schema::NewSessionResponse>,
                            cx: ConnectionTo<Client>| {
                    let server = state.clone();
                    cx.spawn(async move {
                        match tokio::task::spawn_blocking(move || server.new_session(request)).await
                        {
                            Ok(Ok(id)) => responder.respond(schema::NewSessionResponse::new(id)),
                            Ok(Err(error)) => responder.respond_with_error(error),
                            Err(_) => responder
                                .respond_with_error(agent_client_protocol::Error::internal_error()),
                        }
                    })
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |request: schema::PromptRequest,
                            responder: Responder<schema::PromptResponse>,
                            cx: ConnectionTo<Client>| {
                    let Some(entry) = state.get(&request.session_id.to_string()) else {
                        return responder.respond_with_error(invalid("unknown session"));
                    };
                    let prompt = match prompt_text(&request.prompt) {
                        Ok(prompt) => prompt,
                        Err(error) => return responder.respond_with_error(error),
                    };
                    let guard = match entry.turn.clone().try_lock_owned() {
                        Ok(guard) => guard,
                        Err(_) => {
                            return responder
                                .respond_with_error(invalid("session already has a running turn"))
                        }
                    };
                    let cancel = CancellationToken::new();
                    if let Ok(mut active) = entry.cancel.lock() {
                        *active = Some(cancel.clone());
                    }
                    let connection = cx.clone();
                    let elicitation = state.elicitation.load(Ordering::Relaxed);
                    cx.spawn(async move {
                        serve_turn(
                            entry,
                            guard,
                            prompt,
                            cancel,
                            responder,
                            TurnClient {
                                session_id: request.session_id,
                                cx: connection,
                                elicitation,
                            },
                        )
                        .await
                    })
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_notification(
            {
                let state = state.clone();
                async move |request: schema::CancelNotification, _cx: ConnectionTo<Client>| {
                    if let Some(entry) = state.get(&request.session_id.to_string()) {
                        if let Ok(active) = entry.cancel.lock() {
                            if let Some(token) = active.as_ref() {
                                token.cancel();
                            }
                        }
                    }
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_to(Stdio::new())
        .await
}

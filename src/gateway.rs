//! Telegram messaging gateway (ported from wizard's gateway).
//!
//! `nur gateway` long-polls Telegram and runs each inbound message as a headless
//! agent turn in the current project, replying with the answer. Token from
//! `--token` or `$TELEGRAM_BOT_TOKEN`; restrict senders with `--chat` /
//! `$TELEGRAM_CHAT_ID`. One session is carried across messages for continuity;
//! tools auto-approve (nur is sandboxed).

use crate::agent::{self, AgentRunner, Session};
use crate::api::ApiClient;
use crate::config::Config;
use crate::error::{NurError, Result};
use crate::theme;
use crate::usage::UsageTracker;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const TG_API: &str = "https://api.telegram.org";
/// Telegram's hard limit on one message.
const TG_MAX: usize = 4096;

/// A parsed inbound text message.
#[derive(Debug, Clone, PartialEq)]
pub struct Update {
    pub update_id: i64,
    pub chat_id: i64,
    /// The sender (`from.id`); absent for channel posts.
    pub from_id: Option<i64>,
    pub text: String,
}

pub fn get_updates_url(token: &str, offset: i64, timeout_secs: u64) -> String {
    format!("{TG_API}/bot{token}/getUpdates?timeout={timeout_secs}&offset={offset}")
}

pub fn send_message_url(token: &str) -> String {
    format!("{TG_API}/bot{token}/sendMessage")
}

/// Parse a getUpdates body into text messages (skips non-text / non-message updates).
pub fn parse_updates(body: &Value) -> Vec<Update> {
    let mut out = Vec::new();
    let Some(items) = body.get("result").and_then(|v| v.as_array()) else {
        return out;
    };
    for it in items {
        let Some(update_id) = it.get("update_id").and_then(|v| v.as_i64()) else {
            continue;
        };
        let Some(msg) = it.get("message").or_else(|| it.get("edited_message")) else {
            continue;
        };
        let Some(text) = msg.get("text").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(chat_id) = msg
            .get("chat")
            .and_then(|c| c.get("id"))
            .and_then(|v| v.as_i64())
        else {
            continue;
        };
        let from_id = msg
            .get("from")
            .and_then(|f| f.get("id"))
            .and_then(|v| v.as_i64());
        out.push(Update {
            update_id,
            chat_id,
            from_id,
            text: text.to_string(),
        });
    }
    out
}

/// Next long-poll offset = highest update_id + 1 (acks processed updates).
pub fn next_offset(updates: &[Update], current: i64) -> i64 {
    updates
        .iter()
        .map(|u| u.update_id.saturating_add(1))
        .max()
        .unwrap_or(current)
        .max(current)
}

fn next_raw_offset(body: &Value, current: i64) -> i64 {
    body.get("result")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|update| update.get("update_id").and_then(Value::as_i64))
        .map(|id| id.saturating_add(1))
        .max()
        .unwrap_or(current)
        .max(current)
}

/// Who may drive the bot: the one allowed chat, and in a group (Telegram
/// gives groups negative ids) only the one allowed sender, since every
/// member could otherwise run an auto-approved agent with `bash`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Allowed {
    pub chat: Option<i64>,
    pub user: Option<i64>,
}

/// Whether `update` may use the bot. Fail closed: every turn runs with tools
/// auto-approved, `bash` included, so a bot without an allowed chat runs
/// nothing for anyone who finds it, and a group without an allowed sender
/// runs nothing either.
pub fn authorized(update: &Update, allow: Allowed) -> bool {
    if allow.chat != Some(update.chat_id) {
        return false;
    }
    update.chat_id > 0 || (allow.user.is_some() && update.from_id == allow.user)
}

/// What an unauthorized sender is told. Before any chat is allowed it names
/// the sender's own chat id, so pairing is one restart away; an allowed group
/// without an allowed sender names the sender's user id the same way.
pub fn refusal(update: &Update, allow: Allowed) -> String {
    let chat_id = update.chat_id;
    match allow.chat {
        Some(chat) if chat == chat_id && allow.user.is_none() => match update.from_id {
            Some(user) => format!(
                "a group chat also needs the one sender this gateway obeys. Your user id is \
                 {user}: restart it with `--user {user}` (or TELEGRAM_USER_ID={user})."
            ),
            None => "not authorized".to_string(),
        },
        Some(_) => "not authorized".to_string(),
        None => format!(
            "this gateway has no allowed chat yet, so it runs nothing. Your chat id is \
             {chat_id}: restart it with `nur gateway --chat {chat_id}` (or \
             TELEGRAM_CHAT_ID={chat_id})."
        ),
    }
}

/// Split a reply into Telegram-sized chunks (char count), preferring newline
/// boundaries; a single over-long line is hard-split.
pub fn chunk_message(text: &str, max: usize) -> Vec<String> {
    let text = if text.trim().is_empty() {
        "(no output)"
    } else {
        text
    };
    let max = max.max(1);
    let mut chunks = Vec::new();
    let mut cur = String::new();
    let mut cur_len = 0usize;
    for line in text.split_inclusive('\n') {
        let line_len = line.chars().count();
        if line_len > max {
            if cur_len > 0 {
                chunks.push(std::mem::take(&mut cur));
                cur_len = 0;
            }
            let mut buf = String::new();
            let mut n = 0;
            for ch in line.chars() {
                if n + 1 > max {
                    chunks.push(std::mem::take(&mut buf));
                    n = 0;
                }
                buf.push(ch);
                n += 1;
            }
            if n > 0 {
                cur = buf;
                cur_len = n;
            }
        } else if cur_len + line_len > max {
            chunks.push(std::mem::take(&mut cur));
            cur = line.to_string();
            cur_len = line_len;
        } else {
            cur.push_str(line);
            cur_len += line_len;
        }
    }
    if cur_len > 0 {
        chunks.push(cur);
    }
    if chunks.is_empty() {
        chunks.push("(no output)".to_string());
    }
    chunks
}

fn truncate(s: &str, n: usize) -> String {
    let one = s.replace('\n', " ");
    if one.chars().count() <= n {
        one
    } else {
        format!("{}…", one.chars().take(n).collect::<String>())
    }
}

async fn send(http: &reqwest::Client, api: &str, token: &str, chat_id: i64, text: &str) {
    let url = if api == TG_API {
        send_message_url(token)
    } else {
        format!("{api}/bot{token}/sendMessage")
    };
    let _ = http
        .post(url)
        .json(&json!({ "chat_id": chat_id, "text": text }))
        .send()
        .await;
}

/// A loopback override permits a real-binary test without sending a bot token
/// to an arbitrary host. Normal use always goes to Telegram over HTTPS.
fn api_base() -> Result<String> {
    let Ok(raw) = std::env::var("NUR_GATEWAY_API_URL") else {
        return Ok(TG_API.to_string());
    };
    let url = reqwest::Url::parse(&raw)
        .map_err(|_| NurError::Other("invalid gateway API override".into()))?;
    let local = url
        .host_str()
        .and_then(|host| {
            host.trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .ok()
        })
        .is_some_and(|ip| ip.is_loopback());
    if url.scheme() != "http"
        || !local
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(NurError::Other(
            "gateway API override must be a loopback HTTP origin".into(),
        ));
    }
    Ok(raw.trim_end_matches('/').to_string())
}

enum Ticket {
    Approval {
        name: String,
        args: String,
        child: bool,
        respond: tokio::sync::oneshot::Sender<agent::ApprovalDecision>,
    },
    Question {
        header: String,
        question: String,
        options: Vec<(String, String)>,
        multi: bool,
        respond: tokio::sync::oneshot::Sender<agent::QuestionAnswer>,
    },
}

impl Ticket {
    fn is_closed(&self) -> bool {
        match self {
            Ticket::Approval { respond, .. } => respond.is_closed(),
            Ticket::Question { respond, .. } => respond.is_closed(),
        }
    }

    fn render(&self, id: u64) -> String {
        match self {
            Ticket::Approval {
                name, args, child, ..
            } => format!(
                "#{id} {}Approval: {}\n{}\n/answer {id} approve or /answer {id} deny",
                if *child { "Subagent " } else { "" },
                crate::secrets::redact(name),
                crate::secrets::redact(args)
            ),
            Ticket::Question {
                header,
                question,
                options,
                multi,
                ..
            } => {
                let options = options
                    .iter()
                    .enumerate()
                    .map(|(i, (label, description))| {
                        format!(
                            "{}. {}{}",
                            i + 1,
                            label,
                            if description.is_empty() {
                                String::new()
                            } else {
                                format!(" - {description}")
                            }
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                format!(
                    "#{id} {}: {}\n{}\n/answer {id} {}",
                    crate::secrets::redact(header),
                    crate::secrets::redact(question),
                    crate::secrets::redact(&options),
                    if *multi {
                        "numbers separated by commas, or text"
                    } else {
                        "number or text"
                    }
                )
            }
        }
    }

    fn answer(self, answer: &str) -> std::result::Result<(), (Self, String)> {
        match self {
            Ticket::Approval {
                name,
                args,
                child,
                respond,
            } => {
                let decision = match answer.trim().to_ascii_lowercase().as_str() {
                    "approve" | "yes" => agent::ApprovalDecision::Approve,
                    "deny" | "no" => agent::ApprovalDecision::Deny,
                    _ => {
                        return Err((
                            Ticket::Approval {
                                name,
                                args,
                                child,
                                respond,
                            },
                            "Reply with approve or deny.".into(),
                        ))
                    }
                };
                let _ = respond.send(decision);
            }
            Ticket::Question {
                header,
                question,
                options,
                multi,
                respond,
            } => {
                let answer = answer.trim();
                if answer.is_empty() {
                    return Err((
                        Ticket::Question {
                            header,
                            question,
                            options,
                            multi,
                            respond,
                        },
                        "An answer is required.".into(),
                    ));
                }
                let numbers: Option<Vec<usize>> = answer
                    .split(',')
                    .map(|part| part.trim().parse::<usize>().ok())
                    .collect();
                let value = if let Some(numbers) = numbers {
                    if numbers.is_empty()
                        || (!multi && numbers.len() != 1)
                        || numbers.iter().any(|&n| n == 0 || n > options.len())
                    {
                        return Err((
                            Ticket::Question {
                                header,
                                question,
                                options,
                                multi,
                                respond,
                            },
                            "Choose a valid option number.".into(),
                        ));
                    }
                    agent::QuestionAnswer::picked(
                        numbers
                            .into_iter()
                            .map(|n| options[n - 1].0.clone())
                            .collect(),
                    )
                } else {
                    agent::QuestionAnswer::typed(answer.to_string())
                };
                let _ = respond.send(value);
            }
        }
        Ok(())
    }
}

/// Long-poll Telegram and answer messages until Ctrl+C. `session`/`usage` come
/// from startup and carry conversation continuity across messages.
#[allow(clippy::too_many_arguments)]
pub async fn run_gateway(
    client: ApiClient,
    cfg: Config,
    cwd: PathBuf,
    session: Session,
    usage: UsageTracker,
    token: Option<String>,
    allow_chat: Option<i64>,
    allow_user: Option<i64>,
) -> Result<()> {
    let token = token
        .or_else(|| std::env::var("TELEGRAM_BOT_TOKEN").ok())
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .ok_or_else(|| {
            NurError::Other(
                "no Telegram bot token — pass --token or set TELEGRAM_BOT_TOKEN (get one from @BotFather)"
                    .into(),
            )
        })?;
    let env_id = |name: &str| {
        std::env::var(name)
            .ok()
            .and_then(|s| s.trim().parse::<i64>().ok())
    };
    let allow_chat = allow_chat.or_else(|| env_id("TELEGRAM_CHAT_ID"));
    let allow = Allowed {
        chat: allow_chat,
        user: allow_user.or_else(|| env_id("TELEGRAM_USER_ID")),
    };

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| NurError::Other(e.to_string()))?;
    let api = api_base()?;

    // Headless → auto-approve tools (sandboxed).
    let permission_mode = agent::SharedMode::new(agent::PermissionMode::Auto);
    agent::prompt::prefetch_slow_sources(&cwd, &cfg);
    let runner = Arc::new(AgentRunner {
        client,
        config: cfg,
        cwd: cwd.clone(),
        permission_mode,
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

    let cancel = tokio_util::sync::CancellationToken::new();
    {
        let c = cancel.clone();
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            c.cancel();
        });
    }

    theme::print_ok("telegram gateway online — message your bot (Ctrl+C to stop)");
    if let Some(c) = allow_chat {
        theme::print_info(&format!("answering chat id {c} only"));
    } else {
        theme::print_info(
            "pairing only: no chat is allowed yet, so nothing runs. Message the bot; it \
             replies with your chat id. Then restart with --chat <id> (or TELEGRAM_CHAT_ID)",
        );
    }

    let (updates_tx, mut updates_rx) = tokio::sync::mpsc::unbounded_channel();
    let poll_cancel = cancel.child_token();
    let poll_task = {
        let http = http.clone();
        let api = api.clone();
        let token = token.clone();
        let stop = poll_cancel.clone();
        tokio::spawn(async move {
            let mut offset = 0;
            loop {
                let url = if api == TG_API {
                    get_updates_url(&token, offset, 30)
                } else {
                    format!("{api}/bot{token}/getUpdates?timeout=30&offset={offset}")
                };
                let body = tokio::select! {
                    _ = stop.cancelled() => break,
                    response = http.get(url).send() => match response {
                        Ok(response) => response.json::<Value>().await.unwrap_or(Value::Null),
                        Err(_) => Value::Null,
                    },
                };
                if body.get("ok").and_then(Value::as_bool) != Some(true) {
                    let wait = body
                        .pointer("/parameters/retry_after")
                        .and_then(Value::as_u64)
                        .unwrap_or(3)
                        .clamp(1, 300);
                    tokio::select! { _ = stop.cancelled() => break, _ = tokio::time::sleep(std::time::Duration::from_secs(wait)) => {} }
                    continue;
                }
                let updates = parse_updates(&body);
                offset = next_raw_offset(&body, next_offset(&updates, offset));
                for update in updates {
                    if updates_tx.send(update).is_err() {
                        return;
                    }
                }
                if body
                    .get("result")
                    .and_then(Value::as_array)
                    .is_some_and(Vec::is_empty)
                {
                    tokio::select! {
                        _ = stop.cancelled() => break,
                        _ = tokio::time::sleep(std::time::Duration::from_millis(200)) => {}
                    }
                }
            }
        })
    };

    let mut session = Some(session);
    let mut usage = Some(usage);
    let mut pending = VecDeque::<Update>::new();
    // Keep this channel alive between root turns: async children can still
    // need a person's decision after their parent has replied.
    let (events_tx, mut events) = tokio::sync::mpsc::unbounded_channel::<agent::AgentEvent>();
    let (failed_tx, mut failed_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let mut active = false;
    let mut turn_cancel: Option<tokio_util::sync::CancellationToken> = None;
    let mut active_chat = 0;
    let mut tickets = HashMap::<u64, Ticket>::new();
    let mut ticket_id = 0u64;
    let mut streamed = String::new();
    loop {
        if !active {
            if cancel.is_cancelled() {
                break;
            }
            if let Some(update) = pending.pop_front() {
                active_chat = update.chat_id;
                streamed.clear();
                let turn = cancel.child_token();
                let handle = agent::spawn_turn(
                    runner.clone(),
                    session.take().expect("idle session"),
                    usage.take().expect("idle usage"),
                    update.text,
                    events_tx.clone(),
                    turn.clone(),
                );
                let failed = failed_tx.clone();
                tokio::spawn(async move {
                    if let Err(error) = handle.await {
                        let _ = failed.send(error.to_string());
                    }
                });
                turn_cancel = Some(turn);
                active = true;
            }
        }
        tokio::select! {
            _ = cancel.cancelled(), if !active => break,
            failure = failed_rx.recv(), if active => {
                if let Some(failure) = failure {
                    cancel.cancel();
                    poll_cancel.cancel();
                    let _ = poll_task.await;
                    return Err(NurError::Other(format!("gateway turn failed: {failure}")));
                }
            }
            update = updates_rx.recv(), if !cancel.is_cancelled() => {
                let Some(up) = update else { break; };
                if !authorized(&up, allow) {
                    send(&http, &api, &token, up.chat_id, &refusal(&up, allow)).await;
                    continue;
                }
                tickets.retain(|_, ticket| !ticket.is_closed());
                let text = up.text.trim();
                if text == "/inbox" {
                    let mut ids = tickets.keys().copied().collect::<Vec<_>>();
                    ids.sort_unstable();
                    let body = if ids.is_empty() { "No pending items.".into() } else { ids.iter().filter_map(|id| tickets.get(id).map(|t| t.render(*id))).collect::<Vec<_>>().join("\n\n") };
                    for chunk in chunk_message(&body, TG_MAX) { send(&http, &api, &token, up.chat_id, &chunk).await; }
                } else if text == "/answer" {
                    send(&http, &api, &token, up.chat_id, "Use /answer <id> <answer>.").await;
                } else if let Some(rest) = text.strip_prefix("/answer ") {
                    let (id, answer) = rest.trim().split_once(' ').unwrap_or((rest.trim(), ""));
                    let id = id.parse::<u64>().ok();
                    let message = if let Some((id, ticket)) = id.and_then(|id| tickets.remove(&id).map(|ticket| (id, ticket))) {
                        match ticket.answer(answer) {
                            Ok(()) => format!("Answered #{id}."),
                            Err((ticket, message)) => { tickets.insert(id, ticket); message }
                        }
                    } else { "No such pending item.".into() };
                    send(&http, &api, &token, up.chat_id, &message).await;
                } else if text == "/cancel" {
                    if let Some(turn) = &turn_cancel { turn.cancel(); }
                    send(&http, &api, &token, up.chat_id, if turn_cancel.is_some() { "Cancelling the current turn." } else { "No turn is running." }).await;
                } else {
                    theme::print_info(&format!("‹{}› {}", up.chat_id, truncate(&up.text, 80)));
                    pending.push_back(up);
                }
            }
            event = events.recv() => {
                match event {
                    Some(agent::AgentEvent::TextDelta(text)) => streamed.push_str(&text),
                    Some(agent::AgentEvent::StreamRestart { discard }) => agent::r#loop::discard_streamed_tail(&mut streamed, discard),
                    Some(agent::AgentEvent::AssistantMessage(text)) if streamed.trim().is_empty() => streamed = text,
                    Some(agent::AgentEvent::ApprovalRequest { name, args, respond, parent_session }) => {
                        if respond.is_closed() { continue; }
                        ticket_id = ticket_id.saturating_add(1);
                        let ticket = Ticket::Approval { name, args, respond, child: parent_session.is_some() };
                        let body = ticket.render(ticket_id);
                        tickets.insert(ticket_id, ticket);
                        for chunk in chunk_message(&body, TG_MAX) { send(&http, &api, &token, active_chat, &chunk).await; }
                    }
                    Some(agent::AgentEvent::QuestionRequest { turn_cancel, question, header, options, multi_select, respond, .. }) => {
                        if turn_cancel.is_cancelled() || respond.is_closed() {
                            let _ = respond.send(agent::QuestionAnswer::dismissed());
                            continue;
                        }
                        ticket_id = ticket_id.saturating_add(1);
                        let ticket = Ticket::Question { header, question, options, multi: multi_select, respond };
                        let body = ticket.render(ticket_id);
                        tickets.insert(ticket_id, ticket);
                        for chunk in chunk_message(&body, TG_MAX) { send(&http, &api, &token, active_chat, &chunk).await; }
                    }
                    Some(agent::AgentEvent::SecretRequest { name, respond, .. }) => {
                        let _ = respond.send(None);
                        send(&http, &api, &token, active_chat, &format!("A credential is needed. Set it locally with `nur secrets set {name}`, then ask the agent to continue.")).await;
                    }
                    Some(agent::AgentEvent::LoginRequired { provider_id, provider_name, .. }) => {
                        send(&http, &api, &token, active_chat, &format!("Subagent needs {provider_name} sign-in. Run `nur auth login --provider {provider_id}` locally, then retry.")).await;
                    }
                    Some(agent::AgentEvent::Done { session: s, usage: u, result, .. }) => {
                        session = Some(*s);
                        usage = Some(*u);
                        active = false;
                        turn_cancel = None;
                        tickets.retain(|_, ticket| !ticket.is_closed());
                        let reply = result.map(|text| if text.trim().is_empty() { std::mem::take(&mut streamed) } else { text }).unwrap_or_else(|e| format!("error: {e}"));
                        for chunk in chunk_message(&reply, TG_MAX) { send(&http, &api, &token, active_chat, &chunk).await; }
                    }
                    None => { poll_cancel.cancel(); let _ = poll_task.await; return Err(NurError::Other("gateway event channel closed".into())); }
                    _ => {}
                }
            }
        }
    }
    poll_cancel.cancel();
    let _ = poll_task.await;
    if let Some(session) = session {
        let _ = session.save();
    }
    theme::print_info("telegram gateway stopped");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn urls_are_well_formed() {
        assert_eq!(
            get_updates_url("T", 42, 30),
            "https://api.telegram.org/botT/getUpdates?timeout=30&offset=42"
        );
        assert_eq!(
            send_message_url("T"),
            "https://api.telegram.org/botT/sendMessage"
        );
    }

    #[test]
    fn parse_updates_extracts_text_messages_only() {
        let body = json!({
            "result": [
                { "update_id": 1, "message": { "text": "hi", "chat": { "id": 99 } } },
                { "update_id": 2, "message": { "chat": { "id": 99 } } },        // no text → skip
                { "update_id": 3, "my_chat_member": {} },                       // not a message → skip
                { "update_id": 4, "edited_message": { "text": "yo", "chat": { "id": 7 } } },
            ]
        });
        let ups = parse_updates(&body);
        assert_eq!(ups.len(), 2);
        assert_eq!(
            ups[0],
            Update {
                update_id: 1,
                chat_id: 99,
                from_id: None,
                text: "hi".into()
            }
        );
        assert_eq!(
            ups[1],
            Update {
                update_id: 4,
                chat_id: 7,
                from_id: None,
                text: "yo".into()
            }
        );
    }

    #[test]
    fn next_offset_is_max_plus_one() {
        let ups = vec![
            Update {
                update_id: 5,
                chat_id: 1,
                from_id: None,
                text: "a".into(),
            },
            Update {
                update_id: 9,
                chat_id: 1,
                from_id: None,
                text: "b".into(),
            },
        ];
        assert_eq!(next_offset(&ups, 0), 10);
        assert_eq!(next_offset(&[], 3), 3); // no updates → unchanged
    }

    // Failure mode: a bot started without an allow-list runs auto-approved
    // turns (bash included) for anyone on Telegram who finds it.
    #[test]
    fn only_the_allowed_chat_runs_anything() {
        let msg = |chat_id: i64, from_id: Option<i64>| Update {
            update_id: 1,
            chat_id,
            from_id,
            text: "hi".into(),
        };
        let chat = |id: i64| Allowed {
            chat: Some(id),
            user: None,
        };
        assert!(!authorized(&msg(5, Some(5)), Allowed::default()));
        assert!(authorized(&msg(5, Some(5)), chat(5)));
        assert!(!authorized(&msg(5, Some(5)), chat(6)));
        assert!(refusal(&msg(42, Some(42)), Allowed::default()).contains("--chat 42"));
        assert_eq!(refusal(&msg(42, Some(42)), chat(7)), "not authorized");

        // An allowed group still obeys one sender only: every member could
        // otherwise run an auto-approved agent.
        assert!(!authorized(&msg(-100, Some(9)), chat(-100)));
        assert!(refusal(&msg(-100, Some(9)), chat(-100)).contains("--user 9"));
        let group = Allowed {
            chat: Some(-100),
            user: Some(9),
        };
        assert!(authorized(&msg(-100, Some(9)), group));
        assert!(!authorized(&msg(-100, Some(10)), group));
        assert!(!authorized(&msg(-100, None), group));
    }

    #[test]
    fn chunk_message_never_exceeds_max_and_covers_text() {
        let text = "line one\n".repeat(1000); // ~9000 chars
        let chunks = chunk_message(&text, 4096);
        assert!(chunks.len() >= 2);
        assert!(chunks.iter().all(|c| c.chars().count() <= 4096));
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn chunk_message_hard_splits_a_long_line() {
        let text = "x".repeat(5000);
        let chunks = chunk_message(&text, 4096);
        assert_eq!(chunks.len(), 2);
        assert!(chunks.iter().all(|c| c.chars().count() <= 4096));
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn chunk_message_handles_empty() {
        assert_eq!(chunk_message("   ", 4096), vec!["(no output)".to_string()]);
    }
}

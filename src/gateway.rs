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
use crate::error::{MuseError, Result};
use crate::theme;
use crate::usage::UsageTracker;
use serde_json::{json, Value};
use std::collections::HashSet;
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
        out.push(Update {
            update_id,
            chat_id,
            text: text.to_string(),
        });
    }
    out
}

/// Next long-poll offset = highest update_id + 1 (acks processed updates).
pub fn next_offset(updates: &[Update], current: i64) -> i64 {
    updates
        .iter()
        .map(|u| u.update_id + 1)
        .max()
        .unwrap_or(current)
}

/// Whether `chat_id` may use the bot given an optional single-chat allow-list.
pub fn authorized(chat_id: i64, allow: Option<i64>) -> bool {
    match allow {
        Some(a) => a == chat_id,
        None => true,
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

async fn send(http: &reqwest::Client, token: &str, chat_id: i64, text: &str) {
    let _ = http
        .post(send_message_url(token))
        .json(&json!({ "chat_id": chat_id, "text": text }))
        .send()
        .await;
}

/// Long-poll Telegram and answer messages until Ctrl+C. `session`/`usage` come
/// from startup and carry conversation continuity across messages.
#[allow(clippy::too_many_arguments)]
pub async fn run_gateway(
    client: ApiClient,
    cfg: Config,
    cwd: PathBuf,
    mut session: Session,
    mut usage: UsageTracker,
    token: Option<String>,
    allow_chat: Option<i64>,
) -> Result<()> {
    let token = token
        .or_else(|| std::env::var("TELEGRAM_BOT_TOKEN").ok())
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .ok_or_else(|| {
            MuseError::Other(
                "no Telegram bot token — pass --token or set TELEGRAM_BOT_TOKEN (get one from @BotFather)"
                    .into(),
            )
        })?;
    let allow_chat = allow_chat.or_else(|| {
        std::env::var("TELEGRAM_CHAT_ID")
            .ok()
            .and_then(|s| s.trim().parse().ok())
    });

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| MuseError::Other(e.to_string()))?;

    // Headless → auto-approve tools (sandboxed).
    let permission_mode = agent::SharedMode::new(agent::PermissionMode::Auto);
    let runner = Arc::new(AgentRunner {
        client,
        config: cfg,
        cwd: cwd.clone(),
        permission_mode,
        verbose: false,
        approved_tools: Arc::new(Mutex::new(HashSet::new())),
        tools: crate::tools::ToolHost::default(),
        permissions: agent::SharedPermissions::load(&cwd),
        hooks: agent::hooks::HooksConfig::load(),
        is_subagent: false,
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
        theme::print_info(&format!("restricted to chat id {c}"));
    } else {
        theme::print_info("open to any chat — set --chat / TELEGRAM_CHAT_ID to restrict");
    }

    let mut offset: i64 = 0;
    loop {
        if cancel.is_cancelled() {
            break;
        }
        let url = get_updates_url(&token, offset, 30);
        let body: Value = tokio::select! {
            _ = cancel.cancelled() => break,
            r = http.get(&url).send() => match r {
                Ok(resp) => resp.json().await.unwrap_or(Value::Null),
                Err(_) => {
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    continue;
                }
            },
        };
        let updates = parse_updates(&body);
        offset = next_offset(&updates, offset);
        for up in updates {
            if cancel.is_cancelled() {
                break;
            }
            if !authorized(up.chat_id, allow_chat) {
                send(&http, &token, up.chat_id, "not authorized").await;
                continue;
            }
            theme::print_info(&format!("‹{}› {}", up.chat_id, truncate(&up.text, 80)));
            let (s, u, result, _interrupted) = agent::run_collect(
                runner.clone(),
                session,
                usage,
                up.text.clone(),
                cancel.clone(),
            )
            .await;
            session = *s;
            usage = *u;
            let reply = match result {
                Ok(t) => t,
                Err(e) => format!("error: {e}"),
            };
            for chunk in chunk_message(&reply, TG_MAX) {
                send(&http, &token, up.chat_id, &chunk).await;
            }
        }
    }

    let _ = session.save();
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
                text: "hi".into()
            }
        );
        assert_eq!(
            ups[1],
            Update {
                update_id: 4,
                chat_id: 7,
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
                text: "a".into(),
            },
            Update {
                update_id: 9,
                chat_id: 1,
                text: "b".into(),
            },
        ];
        assert_eq!(next_offset(&ups, 0), 10);
        assert_eq!(next_offset(&[], 3), 3); // no updates → unchanged
    }

    #[test]
    fn authorized_respects_allow_list() {
        assert!(authorized(5, None)); // open
        assert!(authorized(5, Some(5)));
        assert!(!authorized(5, Some(6)));
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

//! Agent-to-agent messaging (Prime agent_message / Connectome multi-agent).
//!
//! Sessions on the same machine sharing a `scope` can exchange messages via an
//! append-only mailbox under `~/.nur/messages/<scope>/inbox.jsonl`. This is the
//! nur equivalent of Prime's direct agent messaging + Connectome's siblings:
//! the sender writes, the recipient reads (marking delivered). No daemon
//! required - messages are durable on disk, so a later session on the same
//! project/scope can pick them up (matches "agents keep running" continuity).

use crate::config::{atomic_write, nur_home};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    pub id: String,
    pub from: String,
    pub to: String,
    pub text: String,
    pub ts_unix: u64,
    #[serde(default)]
    pub delivered: bool,
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn scope_dir(scope: &str) -> PathBuf {
    let safe: String = scope
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    nur_home().join("messages").join(safe)
}

fn inbox_path(scope: &str) -> PathBuf {
    scope_dir(scope).join("inbox.jsonl")
}

fn load(scope: &str) -> Vec<AgentMessage> {
    let text = std::fs::read_to_string(inbox_path(scope)).unwrap_or_default();
    text.lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn write_all(scope: &str, msgs: &[AgentMessage]) -> Result<(), String> {
    let p = inbox_path(scope);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut buf = String::new();
    for m in msgs {
        if let Ok(line) = serde_json::to_string(m) {
            buf.push_str(&line);
            buf.push('\n');
        }
    }
    atomic_write(&p, buf.as_bytes()).map_err(|e| e.to_string())
}

/// Send a message to an agent in `scope`.
pub fn send(scope: &str, to: &str, from: &str, text: &str) -> Result<AgentMessage, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("message text required".into());
    }
    if text.chars().count() > 4_000 {
        return Err("message too long (max 4000 chars)".into());
    }
    let m = AgentMessage {
        id: format!("m-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]),
        from: from.chars().take(64).collect(),
        to: to.chars().take(64).collect(),
        text: text.to_string(),
        ts_unix: now_unix(),
        delivered: false,
    };
    let mut all = load(scope);
    all.push(m.clone());
    write_all(scope, &all)?;
    Ok(m)
}

/// List messages addressed to `agent` (undelivered first), optionally mark read.
pub fn receive(scope: &str, agent: &str, mark_delivered: bool) -> Vec<AgentMessage> {
    let mut all = load(scope);
    if mark_delivered {
        for m in &mut all {
            if (m.to == agent || m.to == "all" || m.to == "*") && !m.delivered {
                m.delivered = true;
            }
        }
        let _ = write_all(scope, &all);
    }
    let mut mine: Vec<AgentMessage> = all
        .into_iter()
        .filter(|m| m.to == agent || m.to == "all" || m.to == "*")
        .collect();
    mine.sort_by(|a, b| {
        b.delivered
            .cmp(&a.delivered)
            .then_with(|| b.ts_unix.cmp(&a.ts_unix))
    });
    mine
}

pub fn mailbox_status(scope: &str) -> String {
    let all = load(scope);
    let undelivered = all.iter().filter(|m| !m.delivered).count();
    format!(
        "mailbox scope={scope} total={} undelivered={undelivered} path={}",
        all.len(),
        inbox_path(scope).display()
    )
}

pub fn render(m: &AgentMessage) -> String {
    let flag = if m.delivered { "" } else { " [NEW]" };
    format!(" #{} · from {}{flag}\n  {}", m.id, m.from, m.text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_receive_delivery() {
        let scope = format!("msg-{}", uuid::Uuid::new_v4().simple());
        send(&scope, "recv-agent", "sender-agent", "hello there").unwrap();
        let inbox = receive(&scope, "recv-agent", false);
        assert_eq!(inbox.len(), 1);
        assert!(!inbox[0].delivered);
        let inbox2 = receive(&scope, "recv-agent", true);
        assert!(inbox2[0].delivered);
        let _ = std::fs::remove_dir_all(scope_dir(&scope));
    }

    #[test]
    fn broadcast_to_all() {
        let scope = format!("msg-all-{}", uuid::Uuid::new_v4().simple());
        send(&scope, "all", "a", "broadcast").unwrap();
        let inbox = receive(&scope, "any-agent", false);
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0].text, "broadcast");
        let _ = std::fs::remove_dir_all(scope_dir(&scope));
    }
}

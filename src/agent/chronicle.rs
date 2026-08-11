//! Append-only chronicle — Connectome/Chronicle-inspired event store (lite).
//!
//! Primary sources:
//! - https://animalabs.ai/connectome (history is permanent; context is the focus)
//! - anima-research/chronicle ("git for data": append-only, branchable)
//!
//! Nur's lite port keeps:
//! - append-only JSONL events with sequence numbers
//! - named checkpoints pointing at a sequence
//! - never deletes events (Connectome: loss of resolution ≠ loss of record)
//!
//! Not a full Rust N-API Chronicle fork — that is a separate binary; this is the
//! in-process continuity substrate multi-provider nur needs.

use crate::config::{atomic_write, nur_home};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChronicleEvent {
    pub seq: u64,
    pub ts_unix: u64,
    pub kind: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_seq: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub name: String,
    pub seq: u64,
    pub ts_unix: u64,
    #[serde(default)]
    pub note: String,
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn dir(scope: &str) -> PathBuf {
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
    nur_home().join("chronicle").join(safe)
}

fn events_path(scope: &str) -> PathBuf {
    dir(scope).join("events.jsonl")
}

fn checkpoints_path(scope: &str) -> PathBuf {
    dir(scope).join("checkpoints.json")
}

fn next_seq(scope: &str) -> u64 {
    let text = std::fs::read_to_string(events_path(scope)).unwrap_or_default();
    let mut last = 0u64;
    for line in text.lines() {
        if let Ok(e) = serde_json::from_str::<ChronicleEvent>(line) {
            last = last.max(e.seq);
        }
    }
    last.saturating_add(1)
}

/// Append an event. Never mutates prior lines (KV-stable / append-only).
pub fn append(
    scope: &str,
    kind: &str,
    text: &str,
    causation_seq: Option<u64>,
) -> Result<ChronicleEvent, String> {
    use std::io::Write;
    let text = text.trim();
    if text.is_empty() {
        return Err("chronicle text required".into());
    }
    if text.chars().count() > 8_000 {
        return Err("chronicle event too large (max 8000 chars)".into());
    }
    let ev = ChronicleEvent {
        seq: next_seq(scope),
        ts_unix: now_unix(),
        kind: kind.chars().take(32).collect(),
        text: text.to_string(),
        causation_seq,
    };
    let p = events_path(scope);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
        .map_err(|e| e.to_string())?;
    let line = serde_json::to_string(&ev).map_err(|e| e.to_string())?;
    writeln!(f, "{line}").map_err(|e| e.to_string())?;
    Ok(ev)
}

pub fn tail(scope: &str, n: usize) -> Vec<ChronicleEvent> {
    let n = n.clamp(1, 200);
    let text = std::fs::read_to_string(events_path(scope)).unwrap_or_default();
    let mut all: Vec<ChronicleEvent> = text
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    if all.len() > n {
        all.drain(0..all.len() - n);
    }
    all
}

pub fn checkpoint(scope: &str, name: &str, note: &str) -> Result<Checkpoint, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("checkpoint name required".into());
    }
    let seq = next_seq(scope).saturating_sub(1);
    let cp = Checkpoint {
        name: name.chars().take(64).collect(),
        seq,
        ts_unix: now_unix(),
        note: note.chars().take(200).collect(),
    };
    let mut list = load_checkpoints(scope);
    list.retain(|c| c.name != cp.name);
    list.push(cp.clone());
    let p = checkpoints_path(scope);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_string_pretty(&list).map_err(|e| e.to_string())?;
    atomic_write(&p, body.as_bytes()).map_err(|e| e.to_string())?;
    let _ = append(
        scope,
        "checkpoint",
        &format!("checkpoint `{}` at seq {} — {}", cp.name, cp.seq, cp.note),
        Some(cp.seq),
    );
    Ok(cp)
}

pub fn load_checkpoints(scope: &str) -> Vec<Checkpoint> {
    let text = std::fs::read_to_string(checkpoints_path(scope)).unwrap_or_default();
    serde_json::from_str(&text).unwrap_or_default()
}

/// Describe state as-of a checkpoint without rewriting history (Connectome time-travel lite).
pub fn describe_at(scope: &str, name: &str) -> Result<String, String> {
    let cps = load_checkpoints(scope);
    let cp = cps
        .iter()
        .find(|c| c.name == name)
        .ok_or_else(|| format!("unknown checkpoint `{name}`"))?;
    let text = std::fs::read_to_string(events_path(scope)).unwrap_or_default();
    let mut lines = vec![format!(
        "chronicle as-of checkpoint `{}` (seq={}, note={})",
        cp.name, cp.seq, cp.note
    )];
    for line in text.lines() {
        let Ok(e) = serde_json::from_str::<ChronicleEvent>(line) else {
            continue;
        };
        if e.seq > cp.seq {
            break;
        }
        if e.seq + 12 < cp.seq && e.kind != "checkpoint" {
            continue; // show last ~12 events before checkpoint + the marker
        }
        lines.push(format!(
            "  #{seq} [{kind}] {text}",
            seq = e.seq,
            kind = e.kind,
            text = e.text.chars().take(160).collect::<String>()
        ));
    }
    Ok(lines.join("\n"))
}

pub fn status(scope: &str) -> String {
    let n = tail(scope, 10_000).len();
    let cps = load_checkpoints(scope).len();
    format!(
        "chronicle scope={scope} events≈{n} checkpoints={cps} path={}",
        events_path(scope).display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_checkpoint_describe() {
        let s = format!("chron-{}", uuid::Uuid::new_v4().simple());
        append(&s, "turn", "user asked about memory", None).unwrap();
        append(&s, "turn", "I integrated native memory", Some(1)).unwrap();
        let cp = checkpoint(&s, "before-ship", "stable").unwrap();
        assert!(cp.seq >= 1);
        let d = describe_at(&s, "before-ship").unwrap();
        assert!(d.contains("before-ship"));
        let _ = std::fs::remove_dir_all(dir(&s));
    }
}

//! Continual Harness lite - Prime `/refine` for nur.
//!
//! Supplemental prompts/memories that outlive a chat window, refined with
//! evidence-backed notes. The immutable base system prompt is never rewritten.
//! Snapshots under `~/.nur/harness/<session>/` support rollback.

use crate::config::{atomic_write, nur_home};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessState {
    /// Supplemental instructions (session-local by default).
    pub supplemental: String,
    pub updated_unix: u64,
    pub revision: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMeta {
    pub revision: u32,
    pub ts_unix: u64,
    pub reason: String,
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn dir(session_id: &str) -> PathBuf {
    let safe: String = session_id
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    nur_home().join("harness").join(safe)
}

fn state_path(session_id: &str) -> PathBuf {
    dir(session_id).join("state.json")
}

pub fn load(session_id: &str) -> HarnessState {
    let text = std::fs::read_to_string(state_path(session_id)).unwrap_or_default();
    serde_json::from_str(&text).unwrap_or(HarnessState {
        supplemental: String::new(),
        updated_unix: 0,
        revision: 0,
    })
}

fn save(session_id: &str, state: &HarnessState) -> Result<(), String> {
    let p = state_path(session_id);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    atomic_write(&p, text.as_bytes()).map_err(|e| e.to_string())
}

fn snapshot(session_id: &str, state: &HarnessState, reason: &str) -> Result<(), String> {
    let d = dir(session_id).join("snapshots");
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let meta = SnapshotMeta {
        revision: state.revision,
        ts_unix: now_unix(),
        reason: reason.chars().take(200).collect(),
    };
    let base = format!("r{:04}_{}", state.revision, meta.ts_unix);
    let body = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    atomic_write(&d.join(format!("{base}.json")), body.as_bytes()).map_err(|e| e.to_string())?;
    let m = serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?;
    atomic_write(&d.join(format!("{base}.meta.json")), m.as_bytes()).map_err(|e| e.to_string())?;
    Ok(())
}

/// Append an evidence-backed lesson. Never touches the base system prompt.
pub fn refine(session_id: &str, lesson: &str, evidence: &str) -> Result<HarnessState, String> {
    let lesson = lesson.trim();
    if lesson.is_empty() {
        return Err("lesson required".into());
    }
    if lesson.chars().count() > 2_000 {
        return Err("lesson too long (max 2000 chars)".into());
    }
    let mut state = load(session_id);
    snapshot(session_id, &state, "pre-refine")?;
    let evidence = evidence.trim();
    let block = if evidence.is_empty() {
        format!("- {lesson}\n")
    } else {
        format!(
            "- {lesson}\n  evidence: {}\n",
            evidence.chars().take(500).collect::<String>()
        )
    };
    if state.supplemental.chars().count() + block.chars().count() > 12_000 {
        // Keep the tail of supplemental lessons.
        let keep: String = state
            .supplemental
            .chars()
            .rev()
            .take(8_000)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        state.supplemental = format!("…(truncated older harness notes)…\n{keep}");
    }
    state.supplemental.push_str(&block);
    state.revision = state.revision.saturating_add(1);
    state.updated_unix = now_unix();
    save(session_id, &state)?;
    Ok(state)
}

pub fn rollback(session_id: &str) -> Result<HarnessState, String> {
    let d = dir(session_id).join("snapshots");
    let mut files: Vec<_> = std::fs::read_dir(&d)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
        .filter(|p| {
            p.file_name()
                .and_then(|s| s.to_str())
                .map(|n| !n.ends_with(".meta.json") && n.starts_with('r'))
                .unwrap_or(false)
        })
        .collect();
    files.sort();
    let last = files
        .pop()
        .ok_or_else(|| "no harness snapshots to roll back to".to_string())?;
    let text = std::fs::read_to_string(&last).map_err(|e| e.to_string())?;
    let mut state: HarnessState = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    state.revision = state.revision.saturating_add(1);
    state.updated_unix = now_unix();
    save(session_id, &state)?;
    Ok(state)
}

pub fn prompt_block(session_id: &str) -> String {
    let state = load(session_id);
    if state.supplemental.trim().is_empty() {
        return String::new();
    }
    format!(
        "\n# Continual harness (session supplemental, rev {})\n\
         These are evidence-backed operating notes refined during this session. \
         They do not replace the base system prompt.\n{}\n",
        state.revision, state.supplemental
    )
}

pub fn status(session_id: &str) -> String {
    let s = load(session_id);
    format!(
        "revision={} updated_unix={} supplemental_chars={}",
        s.revision,
        s.updated_unix,
        s.supplemental.chars().count()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refine_and_rollback() {
        let sid = format!("harness-{}", uuid::Uuid::new_v4().simple());
        refine(&sid, "prefer grep over bash find", "loop hang investigation").unwrap();
        let s = load(&sid);
        assert!(s.supplemental.contains("prefer grep"));
        assert_eq!(s.revision, 1);
        refine(&sid, "second lesson", "").unwrap();
        assert_eq!(load(&sid).revision, 2);
        rollback(&sid).unwrap();
        // After rollback we restored a snapshot and bumped revision.
        let s = load(&sid);
        assert!(s.revision >= 1);
        let _ = std::fs::remove_dir_all(dir(&sid));
    }
}

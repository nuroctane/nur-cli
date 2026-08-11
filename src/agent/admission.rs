//! Async subagent admission (Prime `rlm()` / RLM admission-handle model).
//!
//! Prime: `await rlm("subtask")` returns an admission handle *immediately*;
//! it never waits for the child's answer. Results arrive later via
//! `agent_message` replies or files. This is the nur port: spawn a child in
//! the background, return a handle id, and let the parent poll/retrieve the
//! result from `~/.nur/admissions/<session>/<id>.json`.

use crate::config::{atomic_write, nur_home};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionState {
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Admission {
    pub id: u64,
    pub desc: String,
    pub state: AdmissionState,
    #[serde(default)]
    pub result: String,
    #[serde(default)]
    pub error: Option<String>,
    pub created_unix: u64,
    pub finished_unix: Option<u64>,
}

fn next_id() -> u64 {
    static N: AtomicU64 = AtomicU64::new(1);
    N.fetch_add(1, Ordering::SeqCst)
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
    nur_home().join("admissions").join(safe)
}

fn path(session_id: &str, id: u64) -> PathBuf {
    dir(session_id).join(format!("{id}.json"))
}

fn load_file(p: &PathBuf) -> Option<Admission> {
    let text = std::fs::read_to_string(p).ok()?;
    serde_json::from_str(&text).ok()
}

/// Register a new admission, returning its handle id.
pub fn admit(session_id: &str, desc: &str) -> u64 {
    let id = next_id();
    let a = Admission {
        id,
        desc: desc.chars().take(120).collect(),
        state: AdmissionState::Running,
        result: String::new(),
        error: None,
        created_unix: now_unix(),
        finished_unix: None,
    };
    let d = dir(session_id);
    let _ = std::fs::create_dir_all(&d);
    if let Ok(body) = serde_json::to_string_pretty(&a) {
        let _ = atomic_write(&path(session_id, id), body.as_bytes());
    }
    id
}

/// Mark done/failed with result.
pub fn finish(session_id: &str, id: u64, result: &str, ok: bool) {
    let p = path(session_id, id);
    if let Some(mut a) = load_file(&p) {
        a.state = if ok {
            AdmissionState::Done
        } else {
            AdmissionState::Failed
        };
        a.result = result.chars().take(40_000).collect();
        a.error = if ok {
            None
        } else {
            Some("child failed".into())
        };
        a.finished_unix = Some(now_unix());
        if let Ok(body) = serde_json::to_string_pretty(&a) {
            let _ = atomic_write(&p, body.as_bytes());
        }
    }
}

/// Get one admission (for polling).
pub fn get(session_id: &str, id: u64) -> Option<Admission> {
    load_file(&path(session_id, id))
}
/// List all admissions for this session, newest first.
pub fn list(session_id: &str) -> Vec<Admission> {
    let d = dir(session_id);
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&d) {
        for e in rd.flatten() {
            if e.path().extension().and_then(|s| s.to_str()) == Some("json") {
                if let Some(a) = load_file(&e.path()) {
                    out.push(a);
                }
            }
        }
    }
    out.sort_by_key(|a| std::cmp::Reverse(a.id));
    out
}

pub fn render(a: &Admission) -> String {
    let st = match a.state {
        AdmissionState::Running => "running",
        AdmissionState::Done => "done",
        AdmissionState::Failed => "failed",
    };
    let mut s = format!("#{} · {st} · {desc}", a.id, desc = a.desc);
    if let Some(f) = a.finished_unix {
        let _ = f;
    }
    if !a.result.is_empty() {
        s.push_str(&format!("\n---\n{}", a.result));
    }
    if let Some(e) = &a.error {
        s.push_str(&format!("\n[error] {e}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admit_get_finish() {
        let sid = format!("adm-{}", uuid::Uuid::new_v4().simple());
        let id = admit(&sid, "review auth");
        let a = get(&sid, id).unwrap();
        assert_eq!(a.state, AdmissionState::Running);
        finish(&sid, id, "found 2 issues", true);
        let a = get(&sid, id).unwrap();
        assert_eq!(a.state, AdmissionState::Done);
        assert!(a.result.contains("2 issues"));
        let _ = std::fs::remove_dir_all(dir(&sid));
    }

    #[test]
    fn failed_admission_records_error() {
        let sid = format!("adm2-{}", uuid::Uuid::new_v4().simple());
        let id = admit(&sid, "audit");
        finish(&sid, id, "partial", false);
        let a = get(&sid, id).unwrap();
        assert_eq!(a.state, AdmissionState::Failed);
        let _ = std::fs::remove_dir_all(dir(&sid));
    }
}

//! Persistent goals - Prime Agent `/goal` port for nur.
//!
//! A goal is a durable objective the harness keeps presenting across turns until
//! complete, paused, budget-limited, or cleared. Creating a goal is an explicit
//! host/user/tool action (Prime: not inferred from every task).
//!
//! Stored under `~/.nur/goals/<session_id>.json` so it survives detach/restart
//! of the TUI process (daemon workers are a separate future step).

use crate::config::{atomic_write, nur_home};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Active,
    Paused,
    Completed,
    Cleared,
    Exhausted,
}

impl GoalStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Cleared => "cleared",
            Self::Exhausted => "exhausted",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub text: String,
    pub status: GoalStatus,
    pub created_unix: u64,
    pub updated_unix: u64,
    /// Optional token budget for the goal lifetime.
    #[serde(default)]
    pub token_budget: Option<u64>,
    #[serde(default)]
    pub tokens_used: u64,
    #[serde(default)]
    pub continuation_count: u32,
    #[serde(default)]
    pub note: String,
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn path(session_id: &str) -> PathBuf {
    let safe: String = session_id
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    nur_home().join("goals").join(format!("{safe}.json"))
}

pub fn load(session_id: &str) -> Option<Goal> {
    let p = path(session_id);
    let text = std::fs::read_to_string(p).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn save(session_id: &str, goal: &Goal) -> Result<(), String> {
    let p = path(session_id);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(goal).map_err(|e| e.to_string())?;
    atomic_write(&p, text.as_bytes()).map_err(|e| e.to_string())
}

pub fn set(session_id: &str, text: &str, token_budget: Option<u64>) -> Result<Goal, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("goal text required".into());
    }
    if text.chars().count() > 4_000 {
        return Err("goal text too long (max 4000 chars)".into());
    }
    let now = now_unix();
    let goal = Goal {
        text: text.to_string(),
        status: GoalStatus::Active,
        created_unix: now,
        updated_unix: now,
        token_budget,
        tokens_used: 0,
        continuation_count: 0,
        note: String::new(),
    };
    save(session_id, &goal)?;
    Ok(goal)
}

pub fn complete(session_id: &str, note: &str) -> Result<Goal, String> {
    let mut g = load(session_id).ok_or_else(|| "no goal for session".to_string())?;
    g.status = GoalStatus::Completed;
    g.updated_unix = now_unix();
    if !note.trim().is_empty() {
        g.note = note.trim().chars().take(500).collect();
    }
    save(session_id, &g)?;
    Ok(g)
}

pub fn pause(session_id: &str) -> Result<Goal, String> {
    let mut g = load(session_id).ok_or_else(|| "no goal for session".to_string())?;
    g.status = GoalStatus::Paused;
    g.updated_unix = now_unix();
    save(session_id, &g)?;
    Ok(g)
}

pub fn resume(session_id: &str) -> Result<Goal, String> {
    let mut g = load(session_id).ok_or_else(|| "no goal for session".to_string())?;
    if matches!(g.status, GoalStatus::Completed | GoalStatus::Cleared) {
        return Err("cannot resume a completed/cleared goal; set a new one".into());
    }
    g.status = GoalStatus::Active;
    g.updated_unix = now_unix();
    save(session_id, &g)?;
    Ok(g)
}

pub fn clear(session_id: &str) -> Result<(), String> {
    if let Some(mut g) = load(session_id) {
        g.status = GoalStatus::Cleared;
        g.updated_unix = now_unix();
        save(session_id, &g)?;
    }
    let _ = std::fs::remove_file(path(session_id));
    Ok(())
}

/// Record tokens spent toward the goal; may mark Exhausted.
pub fn add_tokens(session_id: &str, tokens: u64) -> Option<Goal> {
    let mut g = load(session_id)?;
    if !matches!(g.status, GoalStatus::Active) {
        return Some(g);
    }
    g.tokens_used = g.tokens_used.saturating_add(tokens);
    g.continuation_count = g.continuation_count.saturating_add(1);
    g.updated_unix = now_unix();
    if let Some(budget) = g.token_budget {
        if g.tokens_used >= budget {
            g.status = GoalStatus::Exhausted;
        }
    }
    let _ = save(session_id, &g);
    Some(g)
}

/// Inject into system prompt when active (Prime keeps objective across turns).
pub fn prompt_block(session_id: &str) -> String {
    let Some(g) = load(session_id) else {
        return String::new();
    };
    if !matches!(g.status, GoalStatus::Active | GoalStatus::Paused) {
        return String::new();
    }
    let budget = g
        .token_budget
        .map(|b| format!(" budget={}/{}", g.tokens_used, b))
        .unwrap_or_default();
    format!(
        "\n# Persistent goal ({status}{budget})\n\
         Objective: {text}\n\
         Progress: {cont} continuations. Call tool `goal` action=complete when fully verified; \
         action=pause to suspend. Do not claim completion without goal.complete.\n",
        status = g.status.as_str(),
        text = g.text,
        cont = g.continuation_count,
    )
}

pub fn format_status(g: &Goal) -> String {
    format!(
        "status={} tokens={}/{:?} continuations={}\ngoal: {}\nnote: {}",
        g.status.as_str(),
        g.tokens_used,
        g.token_budget,
        g.continuation_count,
        g.text,
        g.note
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_complete_clear_cycle() {
        let sid = format!("goal-test-{}", uuid::Uuid::new_v4().simple());
        let g = set(&sid, "Ship the harness amalgamation", Some(1000)).unwrap();
        assert_eq!(g.status, GoalStatus::Active);
        assert!(prompt_block(&sid).contains("Ship the harness"));
        let g = complete(&sid, "done").unwrap();
        assert_eq!(g.status, GoalStatus::Completed);
        clear(&sid).unwrap();
        assert!(load(&sid).is_none() || matches!(load(&sid).unwrap().status, GoalStatus::Cleared));
    }

    #[test]
    fn budget_exhausts() {
        let sid = format!("goal-budget-{}", uuid::Uuid::new_v4().simple());
        set(&sid, "tiny budget", Some(100)).unwrap();
        let g = add_tokens(&sid, 150).unwrap();
        assert_eq!(g.status, GoalStatus::Exhausted);
        clear(&sid).unwrap();
    }
}

//! Persistent goals - Prime Agent `/goal` port for nur.
//!
//! A goal is a durable objective the harness keeps presenting across turns until
//! complete, paused, budget-limited, or cleared. Creating a goal is an explicit
//! host/user/tool action (Prime: not inferred from every task).
//!
//! Stored under `~/.nur/goals/<session_id>.json` so it survives detach/restart
//! of the TUI process (daemon workers are a separate future step).

use crate::config::{atomic_write, nur_home, TypesafeConfig};
use crate::typesafe::{client::TypesafeClient, harness, policy::Judgment};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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

/// Prefix a model turn uses to declare it cannot proceed without the user.
/// Part of the goal stop protocol: a goal-driven turn may only end by
/// completing the goal (tool `goal` action=complete, with evidence), by
/// declaring `BLOCKED: <exactly what is needed>`, or by being auto-continued
/// by the harness. A plain summary while work remains is not an ending.
pub const BLOCKED_PREFIX: &str = "BLOCKED:";

/// Consecutive harness-driven continuations of one goal turn before the model
/// is handed back to the user. Each continuation is itself a full turn (many
/// tool rounds); the cap bounds spend while Esc / `/goal clear` / `/goal pause`
/// stop the chain immediately at any point.
pub const MAX_GOAL_AUTO_CONTINUES: u8 = 5;

/// Find a blocker declaration: the first line starting with `BLOCKED:`
/// (case-insensitive, leading whitespace allowed) with non-empty remainder.
/// Returns the trimmed remainder for display.
pub fn find_blocker(text: &str) -> Option<String> {
    for line in text.lines() {
        let t = line.trim_start();
        // Byte-index the prefix only on a char boundary. A multibyte character
        // straddling byte 9 (emoji, accented text) must not panic the turn.
        let Some(prefix) = t.get(..BLOCKED_PREFIX.len()) else {
            continue;
        };
        if !prefix.eq_ignore_ascii_case(BLOCKED_PREFIX) {
            continue;
        }
        let rest = t[BLOCKED_PREFIX.len()..].trim();
        if !rest.is_empty() {
            return Some(rest.to_string());
        }
    }
    None
}

/// Outcome of the harness check at the end of a goal-driven turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalTurnEnd {
    /// Keep working: start one auto-continuation turn.
    Continue,
    /// Hand back to the user; the chain is over.
    Stop,
}

/// Pure decision for the end of a goal-driven turn: continue while the tracked
/// goal is still active, nothing is queued behind it, no blocker was declared,
/// and auto-continuation budget remains. Progress (or its absence) only changes
/// the wording of the continuation prompt, never the decision - a stall is
/// exactly when the model most needs re-driving.
pub fn goal_turn_end(
    auto_left: u8,
    goal_active: bool,
    has_queued: bool,
    blocker: Option<&str>,
) -> GoalTurnEnd {
    if !goal_active || has_queued || blocker.is_some() || auto_left == 0 {
        GoalTurnEnd::Stop
    } else {
        GoalTurnEnd::Continue
    }
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
    // No length cap: the goal is the user's explicit objective, stored as one
    // small JSON file and injected verbatim into the prompt. Truncating or
    // rejecting it would silently drop part of what the user asked to pursue.
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

/// Re-open a goal that was just marked completed, keeping its text, note and
/// counters. Used when completion verification rejects the claim: the model
/// sees the rejection and keeps working instead of a silently closed goal.
pub fn reopen(session_id: &str) -> Result<Goal, String> {
    let mut g = load(session_id).ok_or_else(|| "no goal for session".to_string())?;
    if g.status != GoalStatus::Completed {
        return Err("goal is not completed".into());
    }
    g.status = GoalStatus::Active;
    g.updated_unix = now_unix();
    save(session_id, &g)?;
    Ok(g)
}

/// The `action` field of a `goal` tool call's arguments, if it parses.
pub fn args_action(args: &str) -> Option<String> {
    serde_json::from_str::<Value>(args)
        .ok()?
        .get("action")?
        .as_str()
        .map(str::to_string)
}

/// Split a goal into verifiable parts, in code - never by the model.
///
/// Bulleted or numbered lines become parts (continuation lines attach to the
/// current part); a goal with fewer than two marked lines is one part. Capped
/// so one pasted checklist cannot fan out into hundreds of questions; the
/// overflow merges into the last part.
pub const MAX_GOAL_PARTS: usize = 24;

fn strip_part_marker(line: &str) -> Option<&str> {
    let t = line.trim();
    for m in ["- ", "* ", "• "] {
        if let Some(rest) = t.strip_prefix(m) {
            return Some(rest);
        }
    }
    // `1. `, `2) `, ...
    let mut digits = 0usize;
    for c in t.chars() {
        if c.is_ascii_digit() {
            digits += 1;
        } else {
            break;
        }
    }
    if digits > 0 {
        let rest = &t[digits..];
        if let Some(stripped) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return Some(stripped);
        }
    }
    None
}

pub fn split_parts(text: &str) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if let Some(marked) = strip_part_marker(t) {
            parts.push(marked.trim().to_string());
        } else if let Some(last) = parts.last_mut() {
            // Continuation of the current marked part.
            last.push(' ');
            last.push_str(t);
        } else {
            // Preamble before any marker: its own part for now.
            parts.push(t.to_string());
        }
    }
    // Fewer than two marked lines means no real checklist - judge whole.
    let marked = text
        .lines()
        .filter(|l| strip_part_marker(l).is_some())
        .count();
    if marked < 2 {
        let whole = text.trim().to_string();
        return vec![whole];
    }
    parts.retain(|p| !p.is_empty());
    if parts.len() > MAX_GOAL_PARTS {
        let tail = parts.split_off(MAX_GOAL_PARTS - 1).join("\n");
        parts.push(tail);
    }
    if parts.is_empty() {
        parts.push(text.trim().to_string());
    }
    parts
}

/// Evidence of work done, from the live transcript items: one short line per
/// tool call and result, newest last. Capped - the judgment layer fits state
/// again, but there is no point sending more than it can use.
pub fn completion_evidence(input_items: &[Value], max_chars: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    for item in input_items {
        let kind = item.get("type").and_then(Value::as_str).unwrap_or("");
        if kind == "function_call" {
            let name = item.get("name").and_then(Value::as_str).unwrap_or("?");
            let args = item.get("arguments").and_then(Value::as_str).unwrap_or("");
            lines.push(format!("CALL {name} {}", harness::judge_preview(args, 300)));
        } else if kind == "function_call_output" {
            let out = item.get("output").and_then(Value::as_str).unwrap_or("");
            let flag = if out.trim_start().to_ascii_lowercase().starts_with("error:") {
                "error"
            } else {
                "ok"
            };
            lines.push(format!(
                "RESULT {flag} {}",
                harness::judge_preview(out, 300)
            ));
        }
    }
    let mut text = lines.join("\n");
    if text.chars().count() > max_chars {
        // Recent evidence matters most: drop from the front.
        let cut: String = text.chars().rev().take(max_chars).collect();
        let mut kept: String = cut.chars().rev().collect();
        if let Some(pos) = kept.find('\n') {
            kept = format!("[... earlier evidence omitted ...]\n{}", &kept[pos + 1..]);
        }
        text = kept;
    }
    text
}

/// Distinct tool names used in this transcript, in first-appearance order.
/// Code-built evidence for lesson drafts: the model never names its own
/// toolbox.
pub fn tools_used(input_items: &[Value]) -> Vec<String> {
    let mut seen = Vec::new();
    for item in input_items {
        if item.get("type").and_then(Value::as_str) != Some("function_call") {
            continue;
        }
        if let Some(name) = item.get("name").and_then(Value::as_str) {
            if !seen.contains(&name.to_string()) {
                seen.push(name.to_string());
            }
            if seen.len() >= 8 {
                break;
            }
        }
    }
    seen
}

/// Draft the learning note for a verified goal completion: strictly factual
/// (goal head, verified count, tools, turns/tokens), 1-2 sentences, so it is
/// safe to file without a human rewrite. Returns (lesson, evidence).
pub fn completion_lesson(
    goal_text: &str,
    summary: &str,
    tools: &[String],
    tokens_used: u64,
    continuations: u32,
) -> (String, String) {
    let head: String = goal_text.chars().take(120).collect();
    let toolbox = if tools.is_empty() {
        "no tools".to_string()
    } else {
        tools.join(", ")
    };
    let lesson = format!(
        "Completed '{head}' ({summary}) with {toolbox} over {continuations} continuation(s)."
    );
    let evidence = format!(
        "goal: {goal_text}\nverification: {summary}\ntools: {toolbox}\ntokens: {tokens_used}, continuations: {continuations}"
    );
    (lesson, evidence)
}

/// One completion claim judged part by part.
pub struct GoalVerification {
    pub parts: Vec<(String, Judgment<bool>)>,
}

impl GoalVerification {
    /// True when no judgment could be produced at all (no key/engine, or the
    /// request failed). Callers fall back to accepting the claim.
    pub fn unavailable(&self) -> bool {
        !self.parts.is_empty() && self.parts.iter().all(|(_, j)| j.confidence.is_none())
    }

    /// Parts Jev confidently judges NOT done: `(part, confidence)`.
    pub fn confident_failures(&self) -> Vec<(&str, f64)> {
        self.parts
            .iter()
            .filter_map(|(part, j)| {
                if j.usable() == Some(&false) {
                    Some((part.as_str(), j.confidence.unwrap_or(0.0)))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Accept unless a part confidently failed. Uncertain parts never block:
    /// a coin flip is not evidence the work is missing.
    pub fn passed(&self) -> bool {
        self.confident_failures().is_empty()
    }

    /// Only positive Act-band judgments for every part constitute verification.
    /// Accepting an uncertain claim must not generate an evidence-backed lesson.
    pub fn verified(&self) -> bool {
        !self.parts.is_empty() && self.parts.iter().all(|(_, j)| j.usable() == Some(&true))
    }

    pub fn summary(&self) -> String {
        let kept = self
            .parts
            .iter()
            .filter(|(_, j)| j.usable() == Some(&true))
            .count();
        format!("{kept}/{} goal part(s) verified", self.parts.len())
    }
}

/// Verify a completion claim part by part. Runs on hosted Jev or a local
/// engine - whichever `ready()` resolves - because it uses the same batched
/// ask path as every other harness question.
pub fn verify_completion(
    cfg: &TypesafeConfig,
    parts: &[String],
    state: &Value,
) -> GoalVerification {
    let judged = harness::judge_goal_parts(cfg, parts, state);
    GoalVerification {
        parts: parts.iter().cloned().zip(judged).collect(),
    }
}

/// [`verify_completion`] with an explicit client (tests, live probes).
#[cfg_attr(not(test), allow(dead_code))]
pub fn verify_completion_with(
    client: &TypesafeClient,
    cfg: &TypesafeConfig,
    parts: &[String],
    state: &Value,
) -> GoalVerification {
    GoalVerification {
        parts: parts
            .iter()
            .zip(harness::judge_goal_parts_with(client, cfg, parts, state))
            .map(|(p, j)| (p.clone(), j))
            .collect(),
    }
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

    #[test]
    fn goal_text_has_no_length_cap() {
        let sid = format!("goal-long-{}", uuid::Uuid::new_v4().simple());
        let long = "objective ".repeat(2_000);
        let g = set(&sid, &long, None).unwrap();
        assert_eq!(g.text, long.trim());
        assert!(prompt_block(&sid).contains(&long.trim()[..64]));
        clear(&sid).unwrap();
    }

    #[test]
    fn blocker_detection_needs_the_prefix_and_a_reason() {
        assert_eq!(
            find_blocker("working...\nBLOCKED: need the prod database password"),
            Some("need the prod database password".into())
        );
        assert_eq!(
            find_blocker("  blocked: waiting on user approval for deploy"),
            Some("waiting on user approval for deploy".into())
        );
        // Bare prefix with no reason is not a declaration.
        assert_eq!(find_blocker("BLOCKED:"), None);
        assert_eq!(find_blocker("BLOCKED:   "), None);
        // Prose about being blocked is not the protocol line.
        assert_eq!(find_blocker("I am blocked on this, will try again"), None);
        assert_eq!(find_blocker("nothing to report"), None);
        // Byte 9 falls inside the emoji. The old slice panicked here.
        let emoji = format!("1234567{} still working on it", "\u{1F389}");
        assert_eq!(find_blocker(&emoji), None);
        assert_eq!(
            find_blocker(&format!("{emoji}\nblocked: need a signing key")),
            Some("need a signing key".into())
        );
    }

    #[test]
    fn turn_end_continues_only_while_everything_holds() {
        use super::GoalTurnEnd::{Continue, Stop};
        assert_eq!(goal_turn_end(5, true, false, None), Continue);
        assert_eq!(goal_turn_end(1, true, false, None), Continue);
        // Budget spent.
        assert_eq!(goal_turn_end(0, true, false, None), Stop);
        // Goal completed / paused / cleared mid-turn.
        assert_eq!(goal_turn_end(5, false, false, None), Stop);
        // User queued something: let it run instead.
        assert_eq!(goal_turn_end(5, true, true, None), Stop);
        // Declared blocker: surface it, do not re-drive.
        assert_eq!(goal_turn_end(5, true, false, Some("need a password")), Stop);
    }

    #[test]
    fn parts_split_on_markers_whole_text_otherwise() {
        assert_eq!(split_parts("ship v2"), vec!["ship v2".to_string()]);
        assert_eq!(
            split_parts("Fix the bug\n- reproduce first\n- add a test\n2) land the fix"),
            vec![
                "Fix the bug".to_string(),
                "reproduce first".to_string(),
                "add a test".to_string(),
                "land the fix".to_string(),
            ]
        );
        // Continuation lines attach to the current part.
        assert_eq!(
            split_parts("- migrate the DB\nthis spans two lines\n- backfill"),
            vec![
                "migrate the DB this spans two lines".to_string(),
                "backfill".to_string(),
            ]
        );
        // Wrapped prose with no markers stays whole.
        let prose = "refactor the auth flow\nso it is easier to test";
        assert_eq!(split_parts(prose), vec![prose.to_string()]);
    }

    #[test]
    fn evidence_lists_calls_and_flagged_results_newest_last() {
        let items = vec![
            serde_json::json!({"type":"function_call","call_id":"c1","name":"read_file","arguments":"{\"path\":\"a.rs\"}"}),
            serde_json::json!({"type":"function_call_output","call_id":"c1","output":"contents here"}),
            serde_json::json!({"type":"function_call_output","call_id":"c2","output":"error: boom"}),
            serde_json::json!({"role":"user","content":[]}),
        ];
        let ev = completion_evidence(&items, 10_000);
        assert!(ev.contains("CALL read_file"), "{ev}");
        assert!(ev.contains("RESULT ok"), "{ev}");
        assert!(ev.contains("RESULT error"), "{ev}");
        assert!(!ev.contains("\"role\""), "user text is not evidence: {ev}");
        // Cap keeps the tail and drops whole head lines.
        let short = completion_evidence(&items, 60);
        assert!(
            short.contains("RESULT error"),
            "tail survives the cap: {short}"
        );
        assert!(!short.contains("CALL read_file"), "head is cut: {short}");
    }

    #[test]
    fn tool_action_parses() {
        assert_eq!(
            args_action(r#"{"action":"complete","note":"done"}"#),
            Some("complete".to_string())
        );
        assert_eq!(args_action("not json"), None);
        assert_eq!(args_action(r#"{"text":"x"}"#), None);
    }

    #[test]
    fn lesson_draft_is_factual_and_short() {
        let items = vec![
            serde_json::json!({"type":"function_call","call_id":"a","name":"shell","arguments":"{}"}),
            serde_json::json!({"type":"function_call","call_id":"b","name":"shell","arguments":"{}"}),
            serde_json::json!({"type":"function_call_output","call_id":"a","output":"ok"}),
        ];
        let used = tools_used(&items);
        assert_eq!(used, vec!["shell".to_string()], "deduped, outputs ignored");
        let (lesson, evidence) =
            completion_lesson("ship v2", "2/2 goal part(s) verified", &used, 900, 3);
        assert!(lesson.contains("ship v2"), "{lesson}");
        assert!(lesson.contains("2/2"), "{lesson}");
        assert!(lesson.contains("shell"), "{lesson}");
        assert!(lesson.chars().count() < 400, "{lesson}");
        assert!(evidence.contains("tokens: 900"), "{evidence}");
    }

    #[test]
    fn reopen_only_from_completed() {
        let sid = format!("goal-reopen-{}", uuid::Uuid::new_v4().simple());
        set(&sid, "do the thing", None).unwrap();
        assert!(reopen(&sid).is_err(), "active is not completed");
        complete(&sid, "done").unwrap();
        let g = reopen(&sid).unwrap();
        assert_eq!(g.status, GoalStatus::Active);
        assert_eq!(g.text, "do the thing");
        clear(&sid).unwrap();
    }

    /// Fake-transport client: every noul question answers `answer(id)`.
    fn fake_client(
        answer: impl Fn(&str) -> f64 + Send + Sync + 'static,
    ) -> crate::typesafe::client::TypesafeClient {
        use crate::typesafe::client::{Transport, TransportFn};
        let t: std::sync::Arc<TransportFn> =
            std::sync::Arc::new(move |body: &serde_json::Value| {
                let qs = body
                    .get("questions")
                    .and_then(serde_json::Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                let mut answers = serde_json::Map::new();
                for (id, q) in qs {
                    let p = match q.get("type").and_then(serde_json::Value::as_str) {
                        Some("noul") => answer(&id),
                        _ => 0.5,
                    };
                    answers.insert(id, serde_json::json!({"type": "noul", "noul": p}));
                }
                Ok(serde_json::json!({
                    "model": "fake",
                    "answers": serde_json::Value::Object(answers),
                    "usage": {"input_tokens": 5, "output_tokens": 1},
                }))
            });
        let cfg = TypesafeConfig {
            enabled: true,
            api_key: "k".into(),
            ..TypesafeConfig::default()
        };
        crate::typesafe::client::TypesafeClient::with_transport(&cfg, Transport::Fake(t))
    }

    fn verify_state() -> serde_json::Value {
        serde_json::json!({"goal": "ship", "evidence": "CALL build ok built"})
    }

    #[test]
    fn confident_failure_blocks_success_and_coin_flip_pass() {
        let cfg = TypesafeConfig {
            enabled: true,
            api_key: "k".into(),
            ..TypesafeConfig::default()
        };
        let parts = vec!["part one".to_string(), "part two".to_string()];
        let v = verify_completion_with(
            &fake_client(|id| if id == "goal_part_0" { 0.02 } else { 0.97 }),
            &cfg,
            &parts,
            &verify_state(),
        );
        assert!(!v.passed(), "a confident no must block");
        assert_eq!(v.confident_failures().len(), 1);
        assert_eq!(v.confident_failures()[0].0, "part one");

        let v = verify_completion_with(&fake_client(|_| 0.97), &cfg, &parts, &verify_state());
        assert!(v.passed());
        assert!(v.verified());
        assert_eq!(v.summary(), "2/2 goal part(s) verified");

        // Coin flip: uncertain, never blocks.
        let v = verify_completion_with(&fake_client(|_| 0.55), &cfg, &parts, &verify_state());
        assert!(v.passed(), "uncertainty must not block completion");
        assert!(
            !v.verified(),
            "acceptance is not verified evidence for a lesson"
        );

        let v = verify_completion_with(
            &fake_client(|id| if id == "goal_part_0" { 0.97 } else { 0.55 }),
            &cfg,
            &parts,
            &verify_state(),
        );
        assert!(v.passed());
        assert!(
            !v.verified(),
            "partial verification must not file a verified win"
        );
        assert!(!GoalVerification { parts: Vec::new() }.verified());
    }

    #[test]
    fn no_judgment_layer_means_unavailable_never_a_block() {
        let cfg = TypesafeConfig {
            enabled: false,
            ..TypesafeConfig::default()
        };
        let v = verify_completion(&cfg, &["only part".to_string()], &verify_state());
        assert!(v.unavailable());
        assert!(v.passed(), "no layer must not block completion");
    }

    /// Live hosted check (needs `TYPESAFE_API_KEY`): an obviously-complete
    /// claim over matching evidence verifies without errors.
    #[test]
    #[ignore = "live: needs TYPESAFE_API_KEY"]
    fn live_hosted_verifies_an_obvious_completion() {
        let key = std::env::var("TYPESAFE_API_KEY").unwrap_or_default();
        if key.trim().is_empty() {
            eprintln!("no TYPESAFE_API_KEY - skipping");
            return;
        }
        let cfg = TypesafeConfig {
            enabled: true,
            api_key: key,
            ..TypesafeConfig::default()
        };
        let parts = vec!["restart the payments service".to_string()];
        let state = serde_json::json!({
            "goal": "restore payments",
            "evidence": "CALL shell systemctl restart payments\nRESULT ok payments active (running)\nCALL shell systemctl is-active payments\nRESULT ok active",
        });
        let v = verify_completion(&cfg, &parts, &state);
        assert!(!v.unavailable(), "hosted layer answered");
        assert!(v.passed(), "obvious completion verifies: {}", v.summary());
    }

    /// Live local-engine check (needs Python + the bridge): the same claim
    /// verifies keyless through a throwaway `mock` bridge.
    #[test]
    #[ignore = "live: spawns the local bridge"]
    fn live_mock_bridge_verifies_keyless() {
        use std::time::{Duration, Instant};
        if crate::jev_local::python().is_none() {
            eprintln!("no usable Python - skipping");
            return;
        }
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
            let port = listener.local_addr().unwrap().port();
            drop(listener);
            port
        };
        let pid = crate::jev_local::spawn_detached("mock", port, &[]).expect("bridge spawns");
        let endpoint = format!("http://127.0.0.1:{port}/v1/systemone");
        let deadline = Instant::now() + Duration::from_secs(20);
        while crate::jev_local::probe_port(port).is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(200));
        }
        assert!(
            crate::jev_local::probe_port(port).is_some(),
            "the bridge did not answer on {port} within 20s"
        );
        let cfg = TypesafeConfig {
            enabled: true,
            api_key: String::new(),
            base_url: endpoint,
            ..TypesafeConfig::default()
        };
        // Mock scores keyword overlap: evidence saturated with the part's
        // words reads as done.
        let parts = vec!["fix the payments outage".to_string()];
        let state = serde_json::json!({
            "goal": "restore payments",
            "evidence": "CALL shell restart payments service\nRESULT ok payments outage fixed and verified",
        });
        let v = verify_completion(&cfg, &parts, &state);
        crate::jev_local::kill_pid(pid);
        assert!(!v.unavailable(), "the bridge answered");
        assert!(
            v.passed(),
            "keyword-saturated evidence verifies: {}",
            v.summary()
        );
    }
}

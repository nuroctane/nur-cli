//! Jev-scored compaction: prune dead tool calls, keep survivors verbatim.
//!
//! Ported from <https://github.com/tamaratran/fast-jev-compaction>. The rule
//! that makes this worth having: **nothing is ever rewritten.** Ordinary
//! compaction asks a model to summarize, and a summary is lossy - a file path,
//! an exact error, or a constraint can vanish even when it still matters. Here
//! Jev is shown the whole conversation and asked two questions per tool call:
//!
//! - should the **call** stay, knowing it was made with these arguments?
//! - should the **result** stay verbatim, or can the tool simply be re-run?
//!
//! Answers are compared against a keep threshold. Keep both, keep the call and
//! truncate the result to a head plus a one-line note, or remove call and result
//! together. Text the model or the user wrote is never touched, no result is
//! ever left without its call, and a failed judgment means the caller falls back
//! to its normal path - a probability is not proof that a result is safe to
//! delete.
//!
//! Ported from
//! [fast-jev-compaction](https://github.com/tamaratran/fast-jev-compaction):
//! the two-question shape, the pinning rule, the staged state fitting, the
//! tokenizer-free estimate, the request-token budget for batching, and the
//! decision rule all follow that reference, so the same transcript produces the
//! same pruning here and there.
//!
//! Differences from upstream, all deliberate:
//!
//! - nur's transcript is a flat item array (calls, results, text), not
//!   role-tagged messages, so pairing is by `call_id` alone and pinning is by
//!   item position.
//! - One batched request serves the whole candidate set (split by the request
//!   budget, run in parallel), instead of one state re-send per handful.
//! - The questions are exactly upstream's two (`keep_call`, `keep_result`) with
//!   no verdict question, because compaction never reads a verdict.
//! - A question that comes back unanswered keeps its content, and a `None`
//!   answer is never read as permission to delete.

use super::harness::{self, JudgeScope, ToolCallItem};
use super::telemetry;
use crate::config::{TypesafeCompactionConfig, TypesafeConfig};
use serde_json::{json, Value};

/// Longest tool argument string kept in the state Jev sees, before staging.
const ARG_KEEP: usize = 1_000;
/// Stage 2/3 argument budgets (upstream's 200 then 60).
const ARG_STAGE_2: usize = 200;
const ARG_STAGE_3: usize = 60;

/// One tool call in the transcript, paired with its result.
#[derive(Debug, Clone, PartialEq)]
pub struct CallPair {
    /// Position of the `function_call` item.
    pub call_index: usize,
    /// Position of its `function_call_output` item, when present.
    pub output_index: Option<usize>,
    pub call_id: String,
    pub tool: String,
    pub arguments: String,
    /// Characters in the result body (`0` when there is no result yet).
    pub result_chars: usize,
    /// Whether the result body is a failure. The reference carries an explicit
    /// `isError`; nur's transcript keeps the reason in the body (the loop writes
    /// `error: …`), so the flag is read from it - Jev should know that an error
    /// is an error, because those are the results worth keeping.
    pub is_error: bool,
}

/// What to do with one call, after Jev's answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Keep the call and its result, byte for byte.
    KeepBoth,
    /// Keep the call, truncate the result to its head plus a note.
    TruncateResult,
    /// Drop the call and its result.
    DropBoth,
}

impl Decision {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::KeepBoth => "keep",
            Self::TruncateResult => "truncate",
            Self::DropBoth => "drop",
        }
    }
}

/// One call's outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct CallOutcome {
    pub call_id: String,
    pub tool: String,
    pub decision: Decision,
    /// `p(keep call)` from Jev, when it answered.
    pub keep_call: Option<f64>,
    /// `p(keep result)` from Jev, when it answered.
    pub keep_result: Option<f64>,
    pub reason: String,
}

/// What compaction did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stats {
    pub items_before: usize,
    pub items_after: usize,
    pub chars_before: usize,
    pub chars_after: usize,
    pub calls_considered: usize,
    pub kept: usize,
    pub truncated: usize,
    pub dropped_calls: usize,
    pub dropped_results: usize,
    /// Fitting stage the state needed (`full`, `args1000`, …).
    pub stage: String,
    pub state_tokens_estimate: u64,
    /// Calls kept because they are inside the pinned window.
    pub pinned: usize,
    /// Requests spent on the judgment (a split batch counts each).
    pub requests: usize,
    /// How long the judgment took, end to end.
    pub ms: u64,
}

impl Stats {
    /// Fraction of characters removed. Upstream treats `< 0.25` as "not worth
    /// it" and lets the caller fall back.
    pub fn reduction_ratio(&self) -> f64 {
        if self.chars_before == 0 {
            return 0.0;
        }
        1.0 - (self.chars_after as f64 / self.chars_before as f64)
    }

    pub fn summary(&self) -> String {
        format!(
            "{}→{} items · {}→{} chars ({:.0}% smaller) · {} call(s) judged: {} kept, {} truncated, \
             {} dropped (stage {}, ~{} state tok, {} request(s))",
            self.items_before,
            self.items_after,
            self.chars_before,
            self.chars_after,
            self.reduction_ratio() * 100.0,
            self.calls_considered,
            self.kept,
            self.truncated,
            self.dropped_calls,
            self.stage,
            self.state_tokens_estimate,
            self.requests,
        )
    }
}

impl Outcome {
    /// One-line breakdown of the decisions, using [`Decision::as_str`].
    pub fn decision_counts(&self) -> String {
        let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        for o in &self.outcomes {
            *counts.entry(o.decision.as_str()).or_default() += 1;
        }
        counts
            .into_iter()
            .map(|(k, v)| format!("{v} {k}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Compaction knobs (`[typesafe.compaction]`).
#[derive(Debug, Clone, PartialEq)]
pub struct CompactOptions {
    /// Newest N items are never touched (plus the first item, always).
    pub preserve_recent: usize,
    /// Minimum keep probability for something to survive.
    pub keep_threshold: f64,
    /// Characters of a dropped result kept before its note.
    pub truncate_head_chars: usize,
    /// Estimated token ceiling for the state Jev sees.
    pub max_state_tokens: u64,
    /// Below this reduction ratio the caller should keep its normal path.
    pub min_reduction: f64,
    /// The ongoing job, so Jev can judge relevance.
    pub goal: String,
}

impl CompactOptions {
    pub fn from_config(cfg: &TypesafeCompactionConfig) -> Self {
        Self {
            preserve_recent: cfg.preserve_recent.max(1),
            keep_threshold: cfg.keep_threshold.clamp(0.0, 1.0),
            truncate_head_chars: cfg.truncate_head_chars,
            max_state_tokens: cfg.max_state_tokens.max(1_000),
            min_reduction: cfg.min_reduction.clamp(0.0, 1.0),
            goal: String::new(),
        }
    }

    /// Attach the current goal (the last user prompts).
    pub fn with_goal(mut self, goal: impl Into<String>) -> Self {
        self.goal = goal.into();
        self
    }
}

/// A successful compaction.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub items: Vec<Value>,
    pub outcomes: Vec<CallOutcome>,
    pub stats: Stats,
}

/// Token estimate without a tokenizer, ported from fast-jev-compaction.
///
/// Pieces are runs of letters, runs of digits, and single non-space symbols: a
/// word costs `1 + (len - 1) / 6`, a digit half a token, any other symbol 0.9.
/// Calibrated to land a little above what Jev reports (2-18% over on real
/// transcripts); a plain characters-per-token ratio undercounts JSON-heavy
/// states by up to 40%, and an undercount builds a request the endpoint then
/// rejects.
pub fn estimate_tokens(text: &str) -> u64 {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = 0.0f64;
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_alphabetic() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                i += 1;
            }
            tokens += 1.0 + ((i - start - 1) / 6) as f64;
        } else if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            tokens += (i - start) as f64 / 2.0;
        } else {
            if !c.is_whitespace() {
                tokens += 0.9;
            }
            i += 1;
        }
    }
    tokens.ceil() as u64
}

fn item_type(v: &Value) -> &str {
    v.get("type").and_then(Value::as_str).unwrap_or("")
}

/// Pair every `function_call` with its `function_call_output`, in order.
pub fn collect_calls(items: &[Value]) -> Vec<CallPair> {
    let mut out: Vec<CallPair> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        if item_type(item) == "function_call" {
            let call_id = item
                .get("call_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            // An empty call_id cannot be paired; keep it visible in the trace by
            // giving it a synthetic id rather than silently merging calls.
            let call_id = if call_id.is_empty() {
                format!("missing-{i}")
            } else {
                call_id
            };
            out.push(CallPair {
                call_index: i,
                output_index: None,
                call_id,
                tool: item
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("?")
                    .to_string(),
                arguments: item
                    .get("arguments")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                result_chars: 0,
                is_error: false,
            });
        }
    }
    // Attach results by id.
    for (i, item) in items.iter().enumerate() {
        if item_type(item) != "function_call_output" {
            continue;
        }
        let call_id = item
            .get("call_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if let Some(pair) = out
            .iter_mut()
            .rev()
            .find(|p| p.call_id == call_id && p.output_index.is_none())
        {
            pair.output_index = Some(i);
            let body = item.get("output").and_then(Value::as_str).unwrap_or("");
            pair.result_chars = body.chars().count();
            pair.is_error = result_is_error(body);
        }
    }
    out
}

/// Whether a tool result body is a failure.
///
/// The tool dispatch writes failures as `error: …` (and two refusals have their
/// own fixed wording), so this reads the harness's own convention rather than
/// inventing a marker in the transcript.
fn result_is_error(body: &str) -> bool {
    let text = body.trim_start();
    if text
        .get(..6)
        .is_some_and(|p| p.eq_ignore_ascii_case("error:"))
    {
        return true;
    }
    text.starts_with("blocked · plan mode") || text.starts_with("user denied this tool call")
}

/// One line describing a call for the state Jev reads.
fn call_line(pair: &CallPair, arg_budget: usize) -> String {
    let args = collapse(&pair.arguments, arg_budget);
    let result = match pair.output_index {
        Some(_) if pair.is_error => format!("error, {} chars (omitted)", pair.result_chars),
        Some(_) => format!("ok, {} chars (omitted)", pair.result_chars),
        None => "no result yet".to_string(),
    };
    format!("{} {} {} → {}", pair.call_id, pair.tool, args, result)
}

fn collapse(text: &str, budget: usize) -> String {
    let one = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= budget {
        return one;
    }
    let head: String = one.chars().take(budget).collect();
    format!("{head}…")
}

fn head_tail(text: &str, budget: usize) -> String {
    let n = text.chars().count();
    if n <= budget {
        return text.to_string();
    }
    let head_budget = budget / 2;
    let tail_budget = budget.saturating_sub(head_budget);
    let head: String = text.chars().take(head_budget).collect();
    let tail: String = text
        .chars()
        .rev()
        .take(tail_budget)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head}…[{} chars omitted]…{tail}", n - budget)
}

/// Build the state string Jev sees: whole conversation, oldest first, tool
/// results replaced by short notes, nothing summarized.
fn build_state(
    items: &[Value],
    calls: &[CallPair],
    pinned: &dyn Fn(usize) -> bool,
    stage: usize,
) -> String {
    let arg_budget = match stage {
        0 => ARG_KEEP,
        1 => ARG_STAGE_2,
        _ => ARG_STAGE_3,
    };
    let text_budget = if stage >= 3 { 400 } else { usize::MAX };
    let mut lines: Vec<String> = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        match item_type(item) {
            "function_call" => {
                if let Some(pair) = calls.iter().find(|p| p.call_index == i) {
                    lines.push(format!("[{i}] {}", call_line(pair, arg_budget)));
                }
            }
            "function_call_output" => {
                // The call line already carries the result note.
                continue;
            }
            _ => {
                let text = item_text(item);
                if text.is_empty() {
                    continue;
                }
                let tag = item.get("role").and_then(Value::as_str).unwrap_or("item");
                // Most aggressive stage first: a `stage >= 3` arm placed first
                // would shadow the `stage >= 4` one, so that stage never ran.
                let body = if pinned(i) {
                    // Pinned text (the newest items, and the first) is what Jev
                    // needs to judge relevance - it stays whole in the state.
                    text
                } else if stage >= 5 {
                    // Last resort before giving up: old text leaves the state
                    // entirely, the way upstream leaves out old messages that
                    // carry no call. The call lines being judged stay.
                    continue;
                } else if stage >= 4 {
                    format!("[… {} chars omitted …]", text.chars().count())
                } else if stage >= 3 {
                    head_tail(&text, text_budget)
                } else {
                    text
                };
                lines.push(format!("[{i}] {tag}: {body}"));
            }
        }
    }
    lines.join("\n")
}

fn item_text(v: &Value) -> String {
    if let Some(s) = v.get("text").and_then(Value::as_str) {
        return s.to_string();
    }
    if let Some(s) = v.get("output").and_then(Value::as_str) {
        return s.to_string();
    }
    if let Some(arr) = v.get("content").and_then(Value::as_array) {
        let joined = arr
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(" ");
        if !joined.is_empty() {
            return joined;
        }
    }
    if let Some(s) = v.get("summary").and_then(Value::as_str) {
        return s.to_string();
    }
    String::new()
}

fn item_chars(v: &Value) -> usize {
    serde_json::to_string(v)
        .map(|s| s.chars().count())
        .unwrap_or(0)
}

fn items_chars(items: &[Value]) -> usize {
    items.iter().map(item_chars).sum()
}

/// Fit the state into the token ceiling, escalating through stages, each
/// applied only when the previous one was not enough: full, tool inputs at 200
/// then 60 characters, long texts abridged head+tail, old texts collapsed to a
/// note, old texts dropped. `None` when even that does not fit - the caller
/// keeps its normal compaction path, exactly as the reference throws.
fn fit_state(
    items: &[Value],
    calls: &[CallPair],
    pinned: &[bool],
    max_tokens: u64,
) -> Option<(String, usize, u64)> {
    let is_pinned = |i: usize| pinned.get(i).copied().unwrap_or(false);
    for stage in 0..=5 {
        let state = build_state(items, calls, &is_pinned, stage);
        let tokens = estimate_tokens(&state);
        if tokens <= max_tokens {
            return Some((state, stage, tokens));
        }
    }
    None
}

/// Whether an item must never be touched: the first item, and the newest
/// `preserve_recent` items.
fn pinned_mask(len: usize, preserve_recent: usize) -> Vec<bool> {
    let mut pinned = vec![false; len];
    if len > 0 {
        pinned[0] = true;
    }
    let start = len.saturating_sub(preserve_recent.max(1));
    for p in pinned.iter_mut().skip(start) {
        *p = true;
    }
    pinned
}

/// Turn one pair + its judgment into a decision, using the rule that the
/// `prune` action and this module share (`harness::prune_decision`).
///
/// `bar` is `keep_threshold` exactly as configured - nothing raises it behind
/// the user's back. 0.5 is the reference's default; 0.925 is the Act band, for
/// anyone who wants pruning to demand the same confidence as acting.
fn decide_pair(pc: Option<f64>, pr: Option<f64>, bar: f64) -> (Decision, String) {
    let (drop_call, drop_result) = harness::prune_decision(pc, pr, bar);
    match (drop_call, drop_result) {
        (false, false) => (
            Decision::KeepBoth,
            match (pc, pr) {
                (Some(c), Some(r)) => format!("kept (p_call={c:.2}, p_result={r:.2})"),
                _ => "kept - no usable answer, and an unjudged call is never deleted".to_string(),
            },
        ),
        (false, true) => (
            Decision::TruncateResult,
            format!(
                "result below {bar:.2} (p={:.2}) and re-runnable; the call itself still matters",
                pr.unwrap_or(0.0)
            ),
        ),
        (true, true) => (
            Decision::DropBoth,
            format!(
                "call and result both below {bar:.2} (p_call={:.2}, p_result={:.2})",
                pc.unwrap_or(0.0),
                pr.unwrap_or(0.0)
            ),
        ),
        // `prune_decision` cannot return this: a dropped call always takes its
        // result with it. Kept rather than guessed at.
        (true, false) => (Decision::KeepBoth, "inconsistent answer - kept".to_string()),
    }
}

fn truncate_note(chars: usize, is_error: bool) -> String {
    let what = if is_error {
        "a result that was an error"
    } else {
        "a tool result"
    };
    format!(
        "\n[… {chars} chars dropped by Jev-scored compaction from {what}; the \
         call kept above is unchanged, and the tool can be re-run …]"
    )
}

/// Rewrite the transcript from the decisions. Text is never touched, survivors
/// stay byte-identical, and no result is left without its call.
///
/// `pinned` (the first item plus the newest `preserve_recent`) is never dropped or
/// truncated, whatever the decisions say: a judged call can sit just outside the
/// window while its *result* sits inside it, and rewriting the call's decision
/// would then have removed a pinned item - the one thing `preserve_recent`
/// promises cannot happen.
fn apply_decisions(
    items: &[Value],
    calls: &[CallPair],
    decisions: &[CallOutcome],
    truncate_head_chars: usize,
    pinned: &[bool],
) -> Vec<Value> {
    use std::collections::HashSet;
    let is_pinned = |i: usize| pinned.get(i).copied().unwrap_or(false);
    let mut drop_idx: HashSet<usize> = HashSet::new();
    // index -> (chars to keep, whether the result was an error)
    let mut truncate_at: std::collections::HashMap<usize, (usize, bool)> =
        std::collections::HashMap::new();
    for (pair, outcome) in calls.iter().zip(decisions.iter()) {
        if is_pinned(pair.call_index) || pair.output_index.is_some_and(is_pinned) {
            continue;
        }
        match outcome.decision {
            Decision::KeepBoth => {}
            Decision::TruncateResult => {
                if let Some(oi) = pair.output_index {
                    // The reference skips a result that is barely longer than the
                    // head it would keep: replacing 320 characters with 300 plus a
                    // note buys nothing and churns the transcript.
                    let body = item_text(&items[oi]);
                    if body.chars().count() > truncate_head_chars + 120 {
                        truncate_at.insert(oi, (truncate_head_chars, pair.is_error));
                    }
                }
            }
            Decision::DropBoth => {
                drop_idx.insert(pair.call_index);
                if let Some(oi) = pair.output_index {
                    drop_idx.insert(oi);
                }
            }
        }
    }
    let mut out = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        if is_pinned(i) {
            // Belt and braces: the decisions already skip pinned pairs, but this
            // guards the invariant itself rather than the caller's care.
            out.push(item.clone());
            continue;
        }
        if drop_idx.contains(&i) {
            continue;
        }
        if let Some((head, is_error)) = truncate_at.get(&i) {
            let body = item_text(item);
            let kept: String = body.chars().take(*head).collect();
            let dropped = body.chars().count().saturating_sub(*head);
            let replacement = format!("{kept}{}", truncate_note(dropped, *is_error));
            // The note itself costs tokens. A character-only threshold can
            // expand short or whitespace-heavy results on every compaction.
            if estimate_tokens(&replacement) >= estimate_tokens(&body) {
                out.push(item.clone());
                continue;
            }
            let mut next = item.clone();
            if let Some(obj) = next.as_object_mut() {
                obj.insert("output".into(), Value::String(replacement));
            }
            out.push(next);
            continue;
        }
        out.push(item.clone());
    }
    out
}

/// Compact `items` with Jev's judgments.
///
/// `None` means "do not prune": no calls to judge, no TypeSafe key, a state that
/// cannot be fitted, or a request that failed. The caller then keeps its normal
/// compaction path, which is the honest fallback - never a silent delete.
pub fn compact_items(
    cfg: &TypesafeConfig,
    items: &[Value],
    opts: &CompactOptions,
) -> Option<Outcome> {
    let client = harness::ready(cfg)?;
    compact_items_with(cfg, items, opts, &client)
}

/// [`compact_items`] against an injected client (tests, replay).
fn compact_items_with(
    cfg: &TypesafeConfig,
    items: &[Value],
    opts: &CompactOptions,
    client: &super::client::TypesafeClient,
) -> Option<Outcome> {
    let calls = collect_calls(items);
    if calls.is_empty() {
        return None;
    }
    let pinned = pinned_mask(items.len(), opts.preserve_recent);
    // The reference's candidate rule: a call with no result has nothing to drop
    // yet, and a pin on *either* side protects the pair - judging a call whose
    // result is inside the window would spend a question on an answer that can
    // never be applied (and used to be filtered out only after the fact).
    let is_pinned = |i: usize| pinned.get(i).copied().unwrap_or(false);
    let judged_pairs: Vec<CallPair> = calls
        .iter()
        .filter(|p| p.output_index.is_some_and(|oi| !is_pinned(oi)) && !is_pinned(p.call_index))
        .cloned()
        .collect();
    if judged_pairs.is_empty() {
        return None;
    }
    let (state, stage, state_tokens) = fit_state(items, &calls, &pinned, opts.max_state_tokens)?;

    let tool_items: Vec<ToolCallItem> = judged_pairs
        .iter()
        .enumerate()
        .map(|(n, p)| ToolCallItem {
            index: n,
            tool: p.tool.clone(),
            args_preview: harness::judge_preview(&p.arguments, harness::JUDGE_PREVIEW_CHARS),
            intent: None,
            result_preview: p.output_index.map(|oi| {
                harness::judge_preview(&item_text(&items[oi]), harness::JUDGE_PREVIEW_CHARS)
            }),
            duplicate_of: None,
            prior_failures: 0,
        })
        .collect();

    let state_value = json!({
        "context": "A coding assistant conversation is being compacted to free context. \
                    `transcript` is the whole conversation so far, oldest first; tool \
                    outputs are replaced by a short note and long texts may be abridged. \
                    Each question asks whether one tool call, or the full output of that \
                    call, still needs to stay in the transcript verbatim. Whatever is not \
                    kept is deleted permanently, but the assistant can always re-run a \
                    tool or re-read a file.",
        "goal": opts.goal,
        "transcript": state,
    });
    let started = std::time::Instant::now();
    // Exactly the reference's two questions per call - no verdict question,
    // which compaction never reads.
    let (judged, meta) = harness::judge_calls_with_meta(
        client,
        cfg,
        &state_value,
        &tool_items,
        JudgeScope::Compaction,
    );
    let elapsed_ms = started.elapsed().as_millis() as u64;
    if judged.is_empty() {
        return None;
    }
    // If literally nothing was answered, do not prune: an unavailable judgment
    // must never look like permission to delete.
    if judged
        .iter()
        .all(|j| j.keep_result_p.is_none() && j.keep_call_p.is_none())
    {
        return None;
    }

    let outcomes: Vec<CallOutcome> = judged_pairs
        .iter()
        .zip(judged.iter())
        .map(|(pair, j)| {
            let (decision, reason) =
                decide_pair(j.keep_call_p, j.keep_result_p, opts.keep_threshold);
            CallOutcome {
                call_id: pair.call_id.clone(),
                tool: pair.tool.clone(),
                decision,
                keep_call: j.keep_call_p,
                keep_result: j.keep_result_p,
                reason,
            }
        })
        .collect();

    // Only the judged pairs are rewritten; pinned calls keep their decisions.
    let pinned_outcomes: Vec<CallOutcome> = calls
        .iter()
        .filter(|p| pinned.get(p.call_index).copied().unwrap_or(false))
        .map(|p| CallOutcome {
            call_id: p.call_id.clone(),
            tool: p.tool.clone(),
            decision: Decision::KeepBoth,
            keep_call: None,
            keep_result: None,
            reason: "pinned (recent or first)".to_string(),
        })
        .collect();

    let after = apply_decisions(
        items,
        &judged_pairs,
        &outcomes,
        opts.truncate_head_chars,
        &pinned,
    );

    let stats = Stats {
        items_before: items.len(),
        items_after: after.len(),
        chars_before: items_chars(items),
        chars_after: items_chars(&after),
        calls_considered: outcomes.len(),
        kept: outcomes
            .iter()
            .filter(|o| o.decision == Decision::KeepBoth)
            .count(),
        truncated: outcomes
            .iter()
            .filter(|o| o.decision == Decision::TruncateResult)
            .count(),
        dropped_calls: outcomes
            .iter()
            .filter(|o| o.decision == Decision::DropBoth)
            .count(),
        dropped_results: outcomes
            .iter()
            .filter(|o| o.decision != Decision::KeepBoth)
            .count(),
        pinned: pinned_outcomes.len(),
        ms: elapsed_ms,
        stage: match stage {
            0 => "full",
            1 => "args200",
            2 => "args60",
            3 => "texts-abridged",
            4 => "old-texts-collapsed",
            5 => "old-texts-dropped",
            _ => "calls-only",
        }
        .to_string(),
        state_tokens_estimate: state_tokens,
        requests: meta.requests.max(1) as usize,
    };

    let chars_saved = stats.chars_before.saturating_sub(stats.chars_after) as u64;
    let frontier_avoided = u64::from(stats.reduction_ratio() >= opts.min_reduction);
    telemetry::record_prune(
        stats.dropped_calls as u64,
        stats.dropped_results as u64,
        chars_saved,
        frontier_avoided,
    );

    let mut all = pinned_outcomes;
    all.extend(outcomes);
    Some(Outcome {
        items: after,
        outcomes: all,
        stats,
    })
}

#[cfg(test)]
mod tests {
    use super::super::client::{Transport, TypesafeClient};
    use super::*;
    use std::sync::Arc;

    fn call_item(id: &str, name: &str, args: &str) -> Value {
        json!({"type":"function_call","call_id":id,"name":name,"arguments":args})
    }

    fn result_item(id: &str, output: &str) -> Value {
        json!({"type":"function_call_output","call_id":id,"output":output})
    }

    fn text_item(text: &str) -> Value {
        json!({"role":"user","content":[{"type":"input_text","text":text}]})
    }

    fn big(s: &str, n: usize) -> String {
        s.repeat(n)
    }

    /// A client that records every question id the harness asked, so a test can
    /// assert that a call was (or was not) judged at all.
    fn recording_client(
        f: impl Fn(&str) -> f64 + Send + Sync + 'static,
    ) -> (TypesafeClient, Arc<std::sync::Mutex<Vec<String>>>) {
        let seen: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorder = seen.clone();
        let t: Arc<super::super::client::TransportFn> = Arc::new(move |body: &Value| {
            let qs = body
                .get("questions")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let mut answers = serde_json::Map::new();
            for (id, q) in qs {
                if let Ok(mut g) = recorder.lock() {
                    g.push(id.clone());
                }
                let a = match q.get("type").and_then(Value::as_str) {
                    Some("noul") => json!({"type":"noul","noul": f(&id)}),
                    _ => json!({"type":"score","score":0.0,"legend":{"0":"x"},
                                "probabilities":{"0":1.0},"confidence":0.9}),
                };
                answers.insert(id, a);
            }
            Ok(
                json!({"model":"jev-latest","answers":Value::Object(answers),
                      "usage":{"input_tokens":5,"output_tokens":1}}),
            )
        });
        let cfg = TypesafeConfig {
            enabled: true,
            api_key: "k".into(),
            ..TypesafeConfig::default()
        };
        (
            TypesafeClient::with_transport(&cfg, Transport::Fake(t)),
            seen,
        )
    }

    fn client(f: impl Fn(&str) -> f64 + Send + Sync + 'static) -> TypesafeClient {
        let t: Arc<super::super::client::TransportFn> = Arc::new(move |body: &Value| {
            let qs = body
                .get("questions")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let mut answers = serde_json::Map::new();
            for (id, q) in qs {
                let a = match q.get("type").and_then(Value::as_str) {
                    Some("noul") => json!({"type":"noul","noul": f(&id)}),
                    Some("choice") => {
                        let first = q
                            .get("criteria")
                            .and_then(Value::as_object)
                            .and_then(|m| m.keys().next().cloned())
                            .unwrap_or_else(|| "succeeded".into());
                        let mut probs = serde_json::Map::new();
                        probs.insert(first.clone(), json!(1.0));
                        json!({"type":"choice","choice":first,
                               "probabilities":Value::Object(probs),"confidence":0.9})
                    }
                    _ => json!({"type":"score","score":0.0,"legend":{"0":"safe"},
                                "probabilities":{"0":1.0},"confidence":0.9}),
                };
                answers.insert(id, a);
            }
            Ok(
                json!({"model":"jev-latest","answers":Value::Object(answers),
                      "usage":{"input_tokens":5,"output_tokens":1}}),
            )
        });
        let cfg = TypesafeConfig {
            enabled: true,
            api_key: "k".into(),
            ..TypesafeConfig::default()
        };
        TypesafeClient::with_transport(&cfg, Transport::Fake(t))
    }

    fn opts() -> CompactOptions {
        CompactOptions {
            preserve_recent: 2,
            keep_threshold: 0.5,
            truncate_head_chars: 8,
            max_state_tokens: 25_000,
            min_reduction: 0.25,
            goal: "fix the failing test".into(),
        }
    }

    /// The fitting stages must run in the order that actually lets the last one
    /// fire: an older `stage >= 3` arm shadowed `stage >= 4`, so the most
    /// aggressive stage never ran, and pinned text was abridged along with the
    /// rest even though it is exactly what Jev needs to judge relevance.
    #[test]
    fn pinned_text_stays_whole_while_old_text_collapses() {
        let mut items = vec![text_item("goal: fix the failing test")];
        for _ in 0..6 {
            items.push(text_item(&"y".repeat(4_000)));
        }
        items.push(call_item(
            "c1",
            "read_file",
            &format!("{{\"path\":\"{}\"}}", "p".repeat(80)),
        ));
        items.push(result_item("c1", &"z".repeat(4_000)));
        items.push(text_item("recent note A"));
        items.push(text_item("recent note B"));
        let calls = collect_calls(&items);
        let pinned = pinned_mask(items.len(), 2);
        assert!(pinned[0], "the first item is always pinned");
        assert!(*pinned.last().unwrap(), "the newest items are pinned");

        let (state, stage, _) =
            fit_state(&items, &calls, &pinned, 400).expect("fits at some stage");
        assert!(
            stage >= 3,
            "a tight budget must reach a deep stage: {stage}"
        );
        // The pinned items keep their text verbatim.
        assert!(state.contains("goal: fix the failing test"), "{state}");
        assert!(state.contains("recent note A"), "{state}");
        // Old bulk text does not: it is abridged or collapsed.
        assert!(
            !state.contains(&"y".repeat(1_000)),
            "old text must be reduced: {state}"
        );
        assert!(state.contains("omitted") || state.contains("abridged") || state.len() < 3_000);
    }

    #[test]
    fn the_mask_marks_the_newest_items_and_the_first() {
        let mask = pinned_mask(5, 2);
        assert_eq!(mask, vec![true, false, false, true, true]);
        let mask = pinned_mask(3, 6);
        assert_eq!(
            mask,
            vec![true, true, true],
            "over-wide preserve keeps everything"
        );
        assert!(pinned_mask(0, 3).is_empty());
    }

    #[test]
    fn pairs_calls_with_results_by_id() {
        let items = vec![
            text_item("hi"),
            call_item("c1", "read_file", "{\"path\":\"a.rs\"}"),
            result_item("c1", "contents"),
            call_item("c2", "grep", "{}"),
        ];
        let calls = collect_calls(&items);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].output_index, Some(2));
        assert_eq!(calls[0].result_chars, 8);
        assert_eq!(calls[1].output_index, None);
        assert_eq!(calls[1].tool, "grep");
    }

    #[test]
    fn token_estimate_matches_the_upstream_calibration() {
        assert_eq!(estimate_tokens("abcdef"), 1);
        assert_eq!(estimate_tokens("12"), 1);
        assert_eq!(estimate_tokens("!!!!"), 4);
        assert!(estimate_tokens(&big("word ", 100)) >= 100);
    }

    #[test]
    fn state_replaces_results_with_notes_and_keeps_text() {
        let items = vec![
            text_item("go"),
            call_item("c1", "read_file", "{\"path\":\"a.rs\"}"),
            result_item("c1", &big("x", 4000)),
        ];
        let calls = collect_calls(&items);
        let state = build_state(&items, &calls, &|_| false, 0);
        assert!(state.contains("user: go"));
        assert!(state.contains("c1 read_file"), "{state}");
        assert!(state.contains("ok, 4000 chars (omitted)"), "{state}");
        // The result body itself is never in the state.
        assert!(!state.contains(&big("x", 20)));
    }

    #[test]
    fn decision_table_is_conservative_around_the_coin_flip() {
        // Confident keep of the result → both survive.
        assert_eq!(decide_pair(Some(0.9), Some(0.9), 0.5).0, Decision::KeepBoth);
        // Result droppable, call still valued → truncate.
        assert_eq!(
            decide_pair(Some(0.9), Some(0.05), 0.5).0,
            Decision::TruncateResult
        );
        // Both stale → drop together.
        assert_eq!(
            decide_pair(Some(0.02), Some(0.05), 0.5).0,
            Decision::DropBoth
        );
        // Ambiguous result probability → keep. Never delete on a coin flip.
        assert_eq!(decide_pair(Some(0.9), Some(0.5), 0.5).0, Decision::KeepBoth);
        assert_eq!(decide_pair(Some(0.9), Some(0.6), 0.5).0, Decision::KeepBoth);
        // Unjudged pair → keep.
        assert_eq!(decide_pair(None, None, 0.5).0, Decision::KeepBoth);
        assert_eq!(decide_pair(Some(0.9), None, 0.5).0, Decision::KeepBoth);
    }

    #[test]
    fn compaction_drops_stale_and_truncates_re_runnable() {
        let items = vec![
            text_item("fix the test"),
            call_item("c1", "read_file", "{\"path\":\"a.rs\"}"),
            result_item("c1", &big("A", 500)),
            call_item("c2", "grep", "{\"pattern\":\"foo\"}"),
            result_item("c2", &big("B", 500)),
            call_item("c3", "read_file", "{\"path\":\"b.rs\"}"),
            result_item("c3", &big("C", 500)),
            text_item("still working"),
            call_item("c4", "recent", "{}"),
            result_item("c4", "recent body"),
        ];
        let cfg = TypesafeConfig {
            enabled: true,
            api_key: "k".into(),
            ..TypesafeConfig::default()
        };
        let client = client(|id| {
            if id.starts_with("keep_result_0") {
                0.95 // c1 result still needed
            } else if id.starts_with("keep_result_1") {
                0.05 // c2 result re-runnable
            } else if id.starts_with("keep_call_1") {
                0.9 // ... but the call still matters
            } else if id.starts_with("keep_call_2") {
                0.02
            } else if id.starts_with("keep_result_2") {
                0.02
            } else {
                0.9
            }
        });
        let outcome = compact_with(&client, &cfg, &items, &opts()).unwrap();
        assert_eq!(outcome.outcomes.len(), 4);
        // c1 kept verbatim, c2 truncated, c3 dropped, c4 pinned.
        let by_id = |id: &str| {
            outcome
                .outcomes
                .iter()
                .find(|o| o.call_id == id)
                .unwrap()
                .decision
                .clone()
        };
        assert_eq!(by_id("c1"), Decision::KeepBoth);
        assert_eq!(by_id("c2"), Decision::TruncateResult);
        assert_eq!(by_id("c3"), Decision::DropBoth);
        assert_eq!(by_id("c4"), Decision::KeepBoth);

        // Every surviving call still has its result; text is untouched.
        let ids: Vec<&str> = outcome
            .items
            .iter()
            .filter_map(|i| i.get("call_id").and_then(Value::as_str))
            .collect();
        assert_eq!(ids, vec!["c1", "c1", "c2", "c2", "c4", "c4"]);
        assert!(outcome
            .items
            .iter()
            .any(|i| i.get("role").and_then(Value::as_str) == Some("user")));
        assert!(outcome.stats.reduction_ratio() > 0.1);
        assert_eq!(outcome.stats.requests, 1);
    }

    #[test]
    fn a_pinned_result_survives_a_decision_on_its_unpinned_call() {
        // The boundary case: `pinned_mask` covers the newest `preserve_recent`
        // items and the first one, and only *calls* outside that window were
        // filtered out of the judging. A call just before the window whose result
        // lands inside it was therefore rewritten from the call's decision, so a
        // "drop both" removed an item `preserve_recent` promises is never touched.
        let items = vec![
            text_item("go"),
            call_item("c1", "read_file", "{\"path\":\"a.rs\"}"),
            result_item("c1", &big("A", 400)),
            call_item("c2", "grep", "{}"), // index 3: judged (window is 4,5)
            result_item("c2", &big("B", 400)), // index 4: pinned
            text_item("latest turn"),      // index 5: pinned
        ];
        let calls = collect_calls(&items);
        let pinned = pinned_mask(items.len(), 2);
        assert!(!pinned[3] && pinned[4] && pinned[5], "test premise");

        let decisions = vec![
            CallOutcome {
                call_id: "c1".into(),
                tool: "read_file".into(),
                decision: Decision::KeepBoth,
                keep_call: None,
                keep_result: None,
                reason: "kept".into(),
            },
            CallOutcome {
                call_id: "c2".into(),
                tool: "grep".into(),
                decision: Decision::DropBoth,
                keep_call: Some(0.01),
                keep_result: Some(0.01),
                reason: "stale".into(),
            },
        ];
        let out = apply_decisions(&items, &calls, &decisions, 100, &pinned);
        // The unpinned call may go, but never the pinned result that follows it.
        let ids: Vec<&str> = out
            .iter()
            .filter_map(|i| i.get("call_id").and_then(Value::as_str))
            .collect();
        assert!(ids.contains(&"c2"), "the pinned result is still there");
        assert_eq!(
            ids.iter().filter(|i| **i == "c1").count(),
            2,
            "c1 untouched"
        );
        // Nothing pinned was dropped or shortened.
        for (i, item) in items.iter().enumerate() {
            if !pinned[i] {
                continue;
            }
            assert!(
                out.contains(item),
                "pinned item {i} came through byte-identical"
            );
        }
    }

    /// The bar is `keep_threshold` as configured. It used to be silently raised
    /// to 0.75, so a user setting the documented default of 0.5 got decisions
    /// made at a stricter bar than the one they could see.
    #[test]
    fn the_bar_is_the_configured_one() {
        // p_result 0.6 sits above 0.5 and below 0.75, so the two bars disagree.
        let at_default = decide_pair(Some(0.8), Some(0.6), 0.5);
        assert_eq!(
            at_default.0,
            Decision::KeepBoth,
            "at the reference bar the result stays: {}",
            at_default.1
        );
        let strict = decide_pair(Some(0.8), Some(0.6), 0.75);
        assert_eq!(
            strict.0,
            Decision::TruncateResult,
            "a raised bar prunes it instead: {}",
            strict.1
        );
        // The shipped default is the reference's 0.5, not an invisible 0.75.
        let cfg = TypesafeCompactionConfig::default();
        assert_eq!(cfg.keep_threshold, 0.5);
        // The Act band is reachable for anyone who wants it, and it cuts the
        // other way: at 0.925 nothing survives on a 0.9 answer, because keeping
        // is what now has to clear the bar.
        assert_eq!(
            decide_pair(Some(0.9), Some(0.3), 0.925).0,
            Decision::DropBoth
        );
        assert_eq!(
            decide_pair(Some(0.95), Some(0.3), 0.925).0,
            Decision::TruncateResult
        );
    }

    /// A dropped call always takes its result with it - the rule cannot produce
    /// "drop the call but keep the result".
    #[test]
    fn a_dropped_call_never_keeps_its_result() {
        for (pc, pr) in [(0.05, 0.05), (0.0, 0.49), (0.1, 0.2)] {
            let (drop_call, drop_result) = harness::prune_decision(Some(pc), Some(pr), 0.5);
            assert!(!(drop_call && !drop_result), "call={pc} result={pr}");
        }
        // A missing answer is never permission to delete.
        assert_eq!(
            harness::prune_decision(None, Some(0.0), 0.5),
            (false, false)
        );
        assert_eq!(
            harness::prune_decision(Some(0.0), None, 0.5),
            (false, false)
        );
    }

    /// The reference's candidate rule: a call with no result has nothing to
    /// drop, and a pin on either side of the pair protects it. Neither is worth
    /// a question.
    #[test]
    fn only_calls_with_unpinned_results_are_judged() {
        let items = vec![
            text_item("go"),
            call_item("pending", "read_file", "{}"), // no result yet
            call_item("c1", "read_file", "{\"path\":\"a.rs\"}"),
            result_item("c1", &big("A", 900)),
            call_item("c2", "grep", "{}"),
            result_item("c2", &big("B", 900)), // pinned: newest item
        ];
        let cfg = TypesafeConfig {
            enabled: true,
            api_key: "k".into(),
            ..TypesafeConfig::default()
        };
        let (client, seen) = recording_client(|_| 0.1); // "drop everything" if asked
        let mut opts = opts();
        opts.preserve_recent = 2; // c2's result is inside the window
        let outcome = compact_with(&client, &cfg, &items, &opts).expect("compaction ran");
        let asked = seen.lock().unwrap().clone();
        assert!(
            asked.iter().all(|q| q.ends_with("_0")),
            "only the first candidate is judged, got {asked:?}"
        );
        assert_eq!(asked.len(), 2, "one call, two questions: {asked:?}");
        assert_eq!(outcome.stats.calls_considered, 1);
        assert_eq!(outcome.stats.pinned, 1, "c2's pair is pinned");
    }

    /// A result barely longer than the head it would keep is left alone:
    /// replacing 350 characters with 300 plus a note buys nothing and churns the
    /// transcript (the reference's own guard).
    #[test]
    fn a_short_result_is_never_rewritten() {
        let items = vec![
            text_item("go"),
            call_item("c1", "read_file", "{}"),
            result_item("c1", &big("A", 350)),
            text_item("ok"),
        ];
        let mut opts = opts();
        opts.truncate_head_chars = 300;
        let calls = collect_calls(&items);
        let decisions = vec![CallOutcome {
            call_id: "c1".into(),
            tool: "read_file".into(),
            decision: Decision::TruncateResult,
            keep_call: Some(0.9),
            keep_result: Some(0.1),
            reason: "re-runnable".into(),
        }];
        let pinned = pinned_mask(items.len(), 1);
        let out = apply_decisions(&items, &calls, &decisions, 300, &pinned);
        assert_eq!(
            out[2].get("output").and_then(Value::as_str),
            items[2].get("output").and_then(Value::as_str),
            "350 chars is not worth truncating to 300"
        );
        // A genuinely long result still gets the head plus a note (the trailing
        // text keeps the newest-window pin off the result).
        let long = vec![
            text_item("go"),
            call_item("c1", "bash", "{}"),
            result_item("c1", &big("L", 40_000)),
            text_item("still working"),
        ];
        let calls = collect_calls(&long);
        let out = apply_decisions(&long, &calls, &decisions, 300, &pinned_mask(long.len(), 1));
        let body = out[2].get("output").and_then(Value::as_str).unwrap();
        assert!(body.len() < 1_000, "truncated: {} chars", body.len());
        assert!(body.contains("dropped by Jev-scored compaction"));

        let sparse = vec![
            text_item("go"),
            call_item("c1", "read_file", "{}"),
            result_item("c1", &format!("ok{}", " ".repeat(500))),
            text_item("still working"),
        ];
        let out = apply_decisions(
            &sparse,
            &collect_calls(&sparse),
            &decisions,
            300,
            &pinned_mask(sparse.len(), 1),
        );
        assert_eq!(
            out, sparse,
            "compaction must not increase provider input tokens"
        );
    }

    /// Errors are the results worth keeping, so the state says which is which.
    #[test]
    fn an_error_result_is_labelled_in_the_state() {
        let items = vec![
            call_item("c1", "bash", "{}"),
            result_item("c1", "error: command not found"),
            call_item("c2", "bash", "{}"),
            result_item("c2", "fine"),
        ];
        let calls = collect_calls(&items);
        assert!(
            calls[0].is_error,
            "harness errors are written as `error: ...`"
        );
        assert!(!calls[1].is_error);
        let state = build_state(&items, &calls, &|_| false, 0);
        assert!(state.contains("error, 24 chars (omitted)"), "{state}");
        assert!(state.contains("ok, 4 chars (omitted)"), "{state}");
    }

    #[test]
    fn failed_judgment_prunes_nothing() {
        let items = vec![
            text_item("go"),
            call_item("c1", "read_file", "{}"),
            result_item("c1", &big("A", 300)),
        ];
        let cfg = TypesafeConfig {
            enabled: true,
            api_key: "k".into(),
            ..TypesafeConfig::default()
        };
        let t: Arc<super::super::client::TransportFn> = Arc::new(|_| Err("down".into()));
        let client = TypesafeClient::with_transport(&cfg, Transport::Fake(t));
        assert!(compact_with(&client, &cfg, &items, &opts()).is_none());
    }

    #[test]
    fn nothing_to_judge_returns_none() {
        let cfg = TypesafeConfig::default();
        let items = vec![text_item("just text")];
        assert!(compact_items(&cfg, &items, &opts()).is_none());
    }

    /// Test seam naming: compaction against an injected client.
    fn compact_with(
        client: &TypesafeClient,
        cfg: &TypesafeConfig,
        items: &[Value],
        opts: &CompactOptions,
    ) -> Option<Outcome> {
        compact_items_with(cfg, items, opts, client)
    }
}

//! Harness policies: the harness's own decisions, asked as typed questions.
//!
//! Every function here replaces one "if statement someone outsourced to a
//! frontier model" with a bounded TypeSafe judgment:
//!
//! | question the harness used to chat about | function | primitive |
//! |------------------------------------------|----------|-----------|
//! | which tool / skill / model next | [`pick`], [`pick_skill`], [`pick_model`] | Choice |
//! | is this chunk relevant | [`relevance`] | Score per candidate |
//! | is this spam / what kind of thing is it | [`classify`] | Choice |
//! | does this need a human | [`needs_human`] | Noul |
//! | is this diff risky | [`risk`] | Score (ordered levels) |
//! | did that tool call work | [`verify_result`] | Choice + Noul |
//!
//! Two invariants hold throughout:
//!
//! 1. **Candidates are built in code.** Options come from the retriever, the
//!    tool trace, the model registry - never from the model. A Choice answer is
//!    mapped back onto the caller's own `String` ([`Answer::resolve_verbatim`]),
//!    and a token that matches nothing yields no judgment instead of an
//!    invented one.
//! 2. **Confidence decides whether the answer may act.** Callers read
//!    [`Judgment::usable`], which is `None` in the `confirm` and `escalate`
//!    bands (`policy::Thresholds`).
//!
//! No key, no network, or a malformed answer all produce
//! [`Judgment::unavailable`] - the caller keeps its pre-Jev behavior.

use super::client::{self, Batch, TypesafeClient};
use super::policy::{self, GateAction, Judgment, Thresholds};
use super::questions::{Answer, ChoiceOption, NoulCriteria, Question, NO_MATCH};
use super::telemetry;
use crate::config::TypesafeConfig;
use serde_json::{json, Value};
use std::sync::Arc;

/// State larger than this is trimmed head+tail before being sent. Jev sees the
/// working set; it never becomes an excuse to ship a megabyte of context.
pub const MAX_STATE_CHARS: usize = 60_000;

/// Documented-scale defaults, used when a caller has no reason to differ.
/// Low to high, five levels: this is the 2..=10 range with concrete meanings.
pub const RISK_LEVELS: &[&str] = &[
    "read-only or a pure question with no side effects",
    "local, reversible change confined to a scratch area or a single file",
    "recoverable change that touches tracked files and may need review",
    "destructive or wide change: deletes, history rewrites, deployments, secrets",
    "irreversible or externally visible: money, production data, public posts",
];

/// Chars of a candidate shown inside its own relevance question. A retrieved
/// chunk is arbitrary in size; the question must stay bounded.
pub const CANDIDATE_PREVIEW_CHARS: usize = 600;

/// Relevance scale for retrieved chunks (low to high).
pub const RELEVANCE_LEVELS: &[&str] = &[
    "irrelevant: nothing in the query depends on it",
    "background: same topic area, no usable answer",
    "supporting: adds context or an example but does not answer the query",
    "directly useful: contains information the query needs",
    "directly answers: it is the evidence the query was looking for",
];

/// Thresholds from `[typesafe]`.
pub fn thresholds(cfg: &TypesafeConfig) -> Thresholds {
    Thresholds::from_config(cfg)
}

/// Trim oversized state to head + tail, in the same spirit as upstream
/// `bounded_remote_compact_transcript`. Strings are the only shape that can
/// explode without bound, so an object state is left alone.
pub fn fit_state(state: &Value, max_chars: usize) -> Value {
    match state {
        Value::String(s) if s.chars().count() > max_chars => {
            const MARKER: &str = "\n...[middle omitted for typesafe state budget]...\n";
            let budget = max_chars.saturating_sub(MARKER.chars().count());
            let head_budget = budget / 4;
            let tail_budget = budget.saturating_sub(head_budget);
            let head: String = s.chars().take(head_budget).collect();
            let tail: String = s
                .chars()
                .rev()
                .take(tail_budget)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            Value::String(format!("{head}{MARKER}{tail}"))
        }
        other => other.clone(),
    }
}

fn unavailable<T>(id: &str, reason: impl Into<String>) -> Judgment<T> {
    telemetry::record_failure();
    Judgment::unavailable(id.to_string(), reason)
}

/// Shared client for this config, or `None` when Jev is not available.
pub fn ready(cfg: &TypesafeConfig) -> Option<Arc<TypesafeClient>> {
    client::shared_client(cfg)
}

/// True when a Jev key is configured and enabled.
pub fn available(cfg: &TypesafeConfig) -> bool {
    ready(cfg).is_some()
}

// ---------------------------------------------------------------------------
// Primitive wrappers. `*_with` variants run against an injected client so the
// policies are testable without a key or the network.
// ---------------------------------------------------------------------------

/// Ask one Noul question. `yes_at` is where the probability counts as yes.
pub fn noul(
    cfg: &TypesafeConfig,
    state: &Value,
    id: &str,
    instructions: impl Into<String>,
    criteria: Option<NoulCriteria>,
    yes_at: f64,
) -> Judgment<bool> {
    match ready(cfg) {
        Some(c) => noul_with(
            &c,
            state,
            id,
            instructions,
            criteria,
            yes_at,
            &thresholds(cfg),
        ),
        None => unavailable(id, "no TypeSafe key"),
    }
}

/// One Choice question over code-supplied candidate labels.
pub fn choose(
    cfg: &TypesafeConfig,
    state: &Value,
    id: &str,
    instructions: impl Into<String>,
    candidates: &[String],
    allow_none: bool,
) -> Judgment<String> {
    match ready(cfg) {
        Some(c) => choose_with(
            &c,
            state,
            id,
            instructions,
            candidates,
            allow_none,
            &thresholds(cfg),
        ),
        None => unavailable(id, "no TypeSafe key"),
    }
}

/// One Score question over ordered levels.
pub fn score(
    cfg: &TypesafeConfig,
    state: &Value,
    id: &str,
    instructions: impl Into<String>,
    levels: &[String],
) -> Judgment<f64> {
    match ready(cfg) {
        Some(c) => score_with(&c, state, id, instructions, levels, &thresholds(cfg)),
        None => unavailable(id, "no TypeSafe key"),
    }
}

/// Score every candidate in **one** request (one question per candidate, run
/// in parallel upstream). Returns judgments in the caller's order.
pub fn rank(
    cfg: &TypesafeConfig,
    query: &str,
    candidates: &[String],
    levels: &[String],
) -> Vec<Judgment<f64>> {
    match ready(cfg) {
        Some(c) => rank_with(&c, query, candidates, levels, &thresholds(cfg)),
        None => candidates
            .iter()
            .map(|_| unavailable("rank", "no TypeSafe key"))
            .collect(),
    }
}

/// Order candidate indices best-first, given their judgments.
///
/// Candidates the judgment acted on come first, highest score first; everything
/// else keeps its input position (a stable, honest fallback rather than a
/// confident-sounding sort of noise). Pure, so the ordering rule is testable
/// without a client - and it is the single implementation of that rule, because
/// a second copy is how the tool once printed scores from one request and an
/// order from another.
pub fn rank_order(judged: &[Judgment<f64>]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..judged.len()).collect();
    let placed = |i: &usize| -> Option<f64> {
        let j = judged.get(*i)?;
        // Only an acted judgment may reorder: a sub-threshold score is not
        // something the policy lets us act on, so it cannot claim a position.
        (j.action == GateAction::Act)
            .then(|| j.raw().copied())
            .flatten()
    };
    order.sort_by(|a, b| match (placed(a), placed(b)) {
        (Some(x), Some(y)) => y.partial_cmp(&x).unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.cmp(b),
    });
    order
}

fn noul_with(
    client: &TypesafeClient,
    state: &Value,
    id: &str,
    instructions: impl Into<String>,
    criteria: Option<NoulCriteria>,
    yes_at: f64,
    t: &Thresholds,
) -> Judgment<bool> {
    let q = match criteria {
        Some(c) => Question::noul_with(instructions, c),
        None => Question::noul(instructions),
    };
    match ask_one(client, state, id, q) {
        Some(answer) => policy::noul_decision(&answer, id, yes_at, t),
        None => unavailable(id, "typesafe request failed"),
    }
}

fn choose_with(
    client: &TypesafeClient,
    state: &Value,
    id: &str,
    instructions: impl Into<String>,
    candidates: &[String],
    allow_none: bool,
    t: &Thresholds,
) -> Judgment<String> {
    if candidates.is_empty() {
        return unavailable(id, "no candidates were supplied by code");
    }
    let mut labels: Vec<String> = candidates.to_vec();
    if allow_none {
        labels.push(NO_MATCH.to_string());
    }
    let q = Question::choice_of(instructions, &labels);
    let Some(answer) = ask_one(client, state, id, q) else {
        return unavailable(id, "typesafe request failed");
    };
    if answer.choice() == Some(NO_MATCH) {
        // A legal "nothing fits" answer is not a low-confidence pick.
        return Judgment::from_answer(id.to_string(), None, answer.confidence(), t);
    }
    let resolved = answer.resolve_verbatim(candidates).cloned();
    Judgment::from_answer(id.to_string(), resolved, answer.confidence(), t)
}

fn score_with(
    client: &TypesafeClient,
    state: &Value,
    id: &str,
    instructions: impl Into<String>,
    levels: &[String],
    t: &Thresholds,
) -> Judgment<f64> {
    let q = Question::score(instructions, levels.to_vec());
    match ask_one(client, state, id, q) {
        Some(answer) => {
            Judgment::from_answer(id.to_string(), answer.score(), answer.confidence(), t)
        }
        None => unavailable(id, "typesafe request failed"),
    }
}

fn rank_with(
    client: &TypesafeClient,
    query: &str,
    candidates: &[String],
    levels: &[String],
    t: &Thresholds,
) -> Vec<Judgment<f64>> {
    if candidates.is_empty() {
        return Vec::new();
    }
    let questions: Vec<(String, Question)> = candidates
        .iter()
        .enumerate()
        .map(|(i, c)| {
            (
                format!("relevance_{i}"),
                Question::score(
                    // The candidate is bounded: a retrieved chunk can be huge, and
                    // a question cannot be longer than the state budget.
                    format!(
                        "How relevant is this candidate to the query? Query: {query}. Candidate: {}",
                        preview(c, CANDIDATE_PREVIEW_CHARS)
                    ),
                    levels.to_vec(),
                ),
            )
        })
        .collect();
    let state = json!({ "query": query });
    let batch = match ask_batched(client, &state, questions) {
        Some(b) => b,
        None => {
            return candidates
                .iter()
                .map(|_| unavailable("relevance", "typesafe request failed"))
                .collect()
        }
    };
    (0..candidates.len())
        .map(|i| {
            let id = format!("relevance_{i}");
            match batch.get(&id) {
                Some(a) => Judgment::from_answer(id, a.score(), a.confidence(), t),
                None => unavailable(&id, "no answer for this candidate"),
            }
        })
        .collect()
}

/// One shared client → one request → one typed answer.
fn ask_one(client: &TypesafeClient, state: &Value, id: &str, question: Question) -> Option<Answer> {
    let state = fit_state(state, MAX_STATE_CHARS);
    let questions = vec![(id.to_string(), question)];
    let batch = client.ask(&state, &questions).ok()?;
    batch.answers.get(id).cloned()
}

/// Batched ask with state fitting. `None` means nothing came back at all.
///
/// Request-level failure accounting belongs to the client, which is the layer
/// that knows a request failed; counting it here too would double every error in
/// the receipts.
pub fn ask_batched(
    client: &TypesafeClient,
    state: &Value,
    questions: Vec<(String, Question)>,
) -> Option<Batch> {
    let state = fit_state(state, MAX_STATE_CHARS);
    client.ask(&state, &questions).ok()
}

// ---------------------------------------------------------------------------
// Concrete harness policies.
// ---------------------------------------------------------------------------

/// Route to one of several named handlers/tools. Candidate list comes from code
/// (the tool trace, the registry, the retriever).
pub fn pick(
    cfg: &TypesafeConfig,
    state: &Value,
    id: &str,
    instructions: impl Into<String>,
    candidates: &[(String, String)],
) -> Judgment<String> {
    let (labels, rubrics): (Vec<String>, Vec<Option<String>>) = candidates
        .iter()
        .map(|(l, r)| (l.clone(), (!r.trim().is_empty()).then(|| r.clone())))
        .unzip();
    let rich: Vec<ChoiceOption> = labels
        .iter()
        .cloned()
        .zip(rubrics)
        .map(|(label, rubric)| ChoiceOption { label, rubric })
        .collect();
    match ready(cfg) {
        Some(c) => {
            let q = Question::choice(instructions, rich);
            let t = thresholds(cfg);
            match ask_one(&c, state, id, q) {
                Some(a) => Judgment::from_answer(
                    id.to_string(),
                    a.resolve_verbatim(&labels).cloned(),
                    a.confidence(),
                    &t,
                ),
                None => unavailable(id, "typesafe request failed"),
            }
        }
        None => unavailable(id, "no TypeSafe key"),
    }
}

/// Should this be handed to a bigger model or a human?
///
/// Noul, so the answer is a probability rather than a verdict; the policy band
/// decides whether the harness may act on it.
pub fn needs_human(cfg: &TypesafeConfig, state: &Value, id: &str) -> Judgment<bool> {
    noul(
        cfg,
        state,
        id,
        "Does this need a human decision (approval, judgment call, missing \
         authority, ambiguity only the user can resolve) rather than an \
         automated answer?",
        Some(NoulCriteria {
            yes: Some("only a person can decide, approve, or supply the missing intent".into()),
            no: Some("the agent can decide and proceed on its own".into()),
        }),
        0.5,
    )
}

/// Generic label router (spam, category, intent, sentiment bucket).
pub fn classify(
    cfg: &TypesafeConfig,
    state: &Value,
    id: &str,
    instructions: impl Into<String>,
    labels: &[String],
) -> Judgment<String> {
    choose(cfg, state, id, instructions, labels, true)
}

/// Spam / noise check for inbound text (peer mail, logs, injected content).
pub fn is_spam(cfg: &TypesafeConfig, state: &Value, id: &str) -> Judgment<bool> {
    noul(
        cfg,
        state,
        id,
        "Is this spam, an advertisement, or a machine-generated distraction \
         rather than something the user asked to see?",
        Some(NoulCriteria {
            yes: Some(
                "promotional, repetitive, or clearly not addressed to the user's task".into(),
            ),
            no: Some("relevant content or an ordinary message".into()),
        }),
        0.5,
    )
}

/// Does the task need each of these tools?
///
/// One Noul per candidate, batched into a single request - the right primitive
/// for a multi-label decision ("several may apply"), where a Choice would force a
/// single winner. Returns `(name, Some(p_yes))` per tool; `None` means the answer
/// was below the acting band, which the caller should read as "keep it".
pub fn tool_need(
    cfg: &TypesafeConfig,
    state: &Value,
    candidates: &[String],
) -> Vec<(String, Option<f64>)> {
    if candidates.is_empty() {
        return Vec::new();
    }
    let Some(client) = ready(cfg) else {
        return Vec::new();
    };
    let questions: Vec<(String, Question)> = candidates
        .iter()
        .enumerate()
        .map(|(i, name)| {
            (
                format!("need_{i}"),
                Question::noul_with(
                    format!(
                        "Does this task need the `{name}` tool? Answer yes only if carrying out                          the task is likely to require it; a tool that would merely be nice to                          have is a no."
                    ),
                    NoulCriteria {
                        yes: Some("the task cannot be done well without it".into()),
                        no: Some("the task can be done without it".into()),
                    },
                ),
            )
        })
        .collect();
    let t = thresholds(cfg);
    let Some(batch) = ask_batched(&client, state, questions) else {
        return Vec::new();
    };
    candidates
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let id = format!("need_{i}");
            let p = batch.get(&id).and_then(Answer::noul);
            // Acting band only: a coin-flip "no" must not remove a tool.
            let confident = match (p, batch.get(&id).and_then(Answer::confidence)) {
                (Some(p), Some(c)) => Some(p).filter(|_| c >= t.act),
                _ => None,
            };
            (name.clone(), confident)
        })
        .collect()
}

/// Spam flags for several pieces of inbound text, in **one** request.
///
/// Used where text arrives from outside the session (peer mail): the harness
/// labels what Jev is confident is promotional or machine-generated and never
/// drops it - inbound content carries no authority either way.
pub fn spam_flags(cfg: &TypesafeConfig, items: &[String]) -> Vec<Judgment<bool>> {
    if items.is_empty() {
        return Vec::new();
    }
    let Some(client) = ready(cfg) else {
        return items
            .iter()
            .map(|_| Judgment::unavailable("spam", "no TypeSafe key"))
            .collect();
    };
    let questions: Vec<(String, Question)> = items
        .iter()
        .enumerate()
        .map(|(i, text)| {
            (
                format!("spam_{i}"),
                Question::noul_with(
                    format!(
                        "Is the following message spam, an advertisement, or a                          machine-generated distraction rather than something relevant to the                          user's work?

{}",
                        judge_preview(text, 1_500)
                    ),
                    NoulCriteria {
                        yes: Some(
                            "promotional, repetitive, or clearly not addressed to the user's task"
                                .into(),
                        ),
                        no: Some("relevant content or an ordinary message".into()),
                    },
                ),
            )
        })
        .collect();
    let t = thresholds(cfg);
    let Some(batch) = ask_batched(
        &client,
        &json!({"inbound": "messages from outside this session"}),
        questions,
    ) else {
        return items
            .iter()
            .map(|_| Judgment::unavailable("spam", "typesafe request failed"))
            .collect();
    };
    (0..items.len())
        .map(|i| {
            let id = format!("spam_{i}");
            match batch.get(&id) {
                Some(a) => policy::noul_decision(a, &id, 0.5, &t),
                None => Judgment::unavailable(id, "no answer for this message"),
            }
        })
        .collect()
}

/// Risk band derived from an ordered Score, so the harness gets a graded
/// answer instead of a binary "risky?" it would have to trust blindly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskBand {
    Safe,
    Low,
    Medium,
    High,
    Critical,
}

impl RiskBand {
    /// Map a weighted score in `0..=(levels-1)` onto a band.
    pub fn from_score(score: f64, levels: usize) -> Self {
        let levels = levels.max(2) as f64;
        let frac = (score / (levels - 1.0)).clamp(0.0, 1.0);
        match frac {
            f if f < 0.125 => Self::Safe,
            f if f < 0.375 => Self::Low,
            f if f < 0.625 => Self::Medium,
            f if f < 0.875 => Self::High,
            _ => Self::Critical,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }

    /// Stakes in `0..=1`, for [`Thresholds::scaled`]: a critical action needs
    /// near-certainty before the harness acts without asking.
    pub fn stakes(&self) -> f64 {
        match self {
            Self::Safe => 0.0,
            Self::Low => 0.25,
            Self::Medium => 0.5,
            Self::High => 0.75,
            Self::Critical => 1.0,
        }
    }

    /// Would a person want to see this before it runs?
    pub fn wants_human(&self) -> bool {
        matches!(self, Self::High | Self::Critical)
    }
}

/// Grade a diff / command / planned action on the risk scale.
pub fn risk(cfg: &TypesafeConfig, state: &Value, id: &str) -> Judgment<RiskBand> {
    let levels: Vec<String> = RISK_LEVELS.iter().map(|s| s.to_string()).collect();
    score(
        cfg,
        state,
        id,
        "How risky is the described action if it runs exactly as written? \
         Consider side effects, reversibility, scope, irreversibility and \
         whether it touches production data, credentials, money or the public.",
        &levels,
    )
    .map(|s| RiskBand::from_score(s, levels.len()))
}

/// Did a tool call actually do what it was called for?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verification {
    Succeeded,
    Partial,
    Failed,
    Unclear,
}

impl Verification {
    pub const LABELS: &'static [&'static str] = &["succeeded", "partial", "failed", "unclear"];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Partial => "partial",
            Self::Failed => "failed",
            Self::Unclear => "unclear",
        }
    }

    pub fn from_label(label: &str) -> Option<Self> {
        match label.trim().to_ascii_lowercase().as_str() {
            "succeeded" => Some(Self::Succeeded),
            "partial" => Some(Self::Partial),
            "failed" => Some(Self::Failed),
            "unclear" => Some(Self::Unclear),
            _ => None,
        }
    }

    /// Worth interrupting the model about?
    pub fn is_problem(&self) -> bool {
        matches!(self, Self::Failed | Self::Partial)
    }
}

/// Judge a tool result against the call's intent.
///
/// The verdict annotates and accounts; it never rewrites the tool body. Kept
/// survivors stay verbatim (upstream fast-jev-compaction's core rule).
pub fn verify_result(
    cfg: &TypesafeConfig,
    call: &str,
    intent: &str,
    output_excerpt: &str,
) -> Judgment<Verification> {
    let labels: Vec<String> = Verification::LABELS.iter().map(|s| s.to_string()).collect();
    let state = json!({
        "tool_call": call,
        "intent": intent,
        "output": output_excerpt,
    });
    choose(
        cfg,
        &state,
        "verify_result",
        "Did the tool output accomplish the stated intent? \
         'failed' = an error or it did not do the thing; 'partial' = it did some \
         of it, or the output contradicts the intent; 'unclear' = not enough \
         information in the output to tell.",
        &labels,
        false,
    )
    .map(|label| Verification::from_label(&label).unwrap_or(Verification::Unclear))
}

/// Judge each part of a session goal against evidence of work done.
///
/// One Noul per part ("is this part achieved, given the evidence?"), batched
/// into a single request - the verify-clause shape: the model supplies the
/// claim (`goal` action=complete / DONE), Jev checks every part against the
/// evidence, and the caller accepts the completion unless a part is
/// *confidently* judged not done. Anything uncertain lets the completion
/// stand: a coin flip is not evidence either way.
///
/// Works on whatever judgment layer is configured - hosted Jev or a local
/// engine through the bridge - because it goes through the same batched
/// `ask` path as every other harness question.
pub fn judge_goal_parts(
    cfg: &TypesafeConfig,
    parts: &[String],
    evidence: &Value,
) -> Vec<Judgment<bool>> {
    let Some(client) = ready(cfg) else {
        return parts
            .iter()
            .map(|_| unavailable("goal_part", "no TypeSafe key"))
            .collect();
    };
    judge_goal_parts_with(&client, cfg, parts, evidence)
}

/// [`judge_goal_parts`] with an explicit client (tests, live probes against a
/// loopback bridge).
pub fn judge_goal_parts_with(
    client: &TypesafeClient,
    cfg: &TypesafeConfig,
    parts: &[String],
    evidence: &Value,
) -> Vec<Judgment<bool>> {
    if parts.is_empty() {
        return Vec::new();
    }
    let t = thresholds(cfg);
    let questions: Vec<(String, Question)> = parts
        .iter()
        .enumerate()
        .map(|(i, part)| {
            (
                format!("goal_part_{i}"),
                Question::noul_with(
                    format!(
                        "Part {i} of the session goal: {part}\n\
                         Given the evidence of work done (tool calls and their \
                         results), is this part achieved? Answer yes only when \
                         the evidence shows it done and verified - a bare claim \
                         with no supporting evidence is a no."
                    ),
                    NoulCriteria {
                        yes: Some("the evidence shows this part done and verified".into()),
                        no: Some("not done, or no supporting evidence".into()),
                    },
                ),
            )
        })
        .collect();
    let Some(batch) = ask_batched(&client, evidence, questions) else {
        return parts
            .iter()
            .map(|_| unavailable("goal_part", "typesafe request failed"))
            .collect();
    };
    (0..parts.len())
        .map(|i| {
            let id = format!("goal_part_{i}");
            match batch.get(&id) {
                Some(a) => policy::noul_decision(a, &id, 0.5, &t),
                None => unavailable(&id, "no answer for this part"),
            }
        })
        .collect()
}

/// Pick the cheap model for this turn from the models actually configured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelOption {
    /// Provider id (code-supplied).
    pub provider: String,
    /// Model id (code-supplied).
    pub model: String,
    /// Short description shown to Jev as the rubric.
    pub note: String,
}

impl ModelOption {
    /// The label Jev sees, and the exact value the harness acts on.
    pub fn label(&self) -> String {
        format!("{}::{}", self.provider, self.model)
    }

    pub fn from_label(label: &str, options: &[ModelOption]) -> Option<ModelOption> {
        options.iter().find(|o| o.label() == label).cloned()
    }
}

/// Router: which configured model should handle this turn.
pub fn pick_model(
    cfg: &TypesafeConfig,
    task: &str,
    options: &[ModelOption],
) -> Judgment<ModelOption> {
    if options.is_empty() {
        return unavailable("route_model", "no model candidates were supplied by code");
    }
    let candidates: Vec<(String, String)> = options
        .iter()
        .map(|o| (o.label(), o.note.clone()))
        .collect();
    let judged = pick(
        cfg,
        &json!({ "task": task }),
        "route_model",
        "Which of these models should handle the task? Prefer the cheapest \
         model that is clearly adequate in capability: routine edits, \
         formatting, lookups and small fixes do not need a frontier model; \
         hard reasoning, architecture, security review and long multi-file \
         work do.",
        &candidates,
    );
    let fallback = options[0].clone();
    judged.map(|label| ModelOption::from_label(&label, options).unwrap_or_else(|| fallback.clone()))
}

/// Rerank skill candidates (name, description) against the request.
pub fn pick_skill(
    cfg: &TypesafeConfig,
    request: &str,
    candidates: &[(String, String)],
) -> Judgment<String> {
    pick(
        cfg,
        &json!({ "request": request }),
        "pick_skill",
        "Which skill, if any, matches what the user is asking for? Choose the \
         one whose described capability the request actually needs.",
        candidates,
    )
}

// ---------------------------------------------------------------------------
// Batched in-loop judging: one request for a whole tool batch.
// ---------------------------------------------------------------------------

/// Chars of a tool argument/result body Jev sees per item. The full body is
/// untouched in the transcript - this only bounds what the judge is shown.
pub const JUDGE_PREVIEW_CHARS: usize = 1_200;

/// Which questions to ask about a batch of tool calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JudgeScope {
    /// Before execution: does this need a human, how risky is it, is it a
    /// redundant repeat of a call whose result is still in context.
    Pre,
    /// After execution: did it succeed, is the call still worth knowing about,
    /// is the result still needed verbatim.
    Post,
    /// Both passes in one request - calls that already have results, judged
    /// before (risk) and after (verification, keep) together.
    #[cfg_attr(not(test), allow(dead_code))]
    Both,
    /// Compaction only: the two questions fast-jev-compaction asks per call
    /// (`keep_call`, `keep_result`) and nothing else. The verdict question is
    /// not needed to decide what to prune, so paying for it would make every
    /// compaction 50% more expensive for an answer nothing reads.
    Compaction,
}

impl JudgeScope {
    fn wants_pre(&self) -> bool {
        matches!(self, Self::Pre | Self::Both)
    }

    fn wants_post(&self) -> bool {
        matches!(self, Self::Post | Self::Both | Self::Compaction)
    }

    /// Whether the "did it succeed" verdict is part of this pass. Compaction
    /// does not read it.
    fn wants_verdict(&self) -> bool {
        matches!(self, Self::Post | Self::Both)
    }
}

/// What one judgment cost, for the caller's stats and telemetry.
///
/// A question set larger than one request is split, so `requests` is the number
/// of HTTP calls actually issued - not a guess.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JudgeMeta {
    pub requests: u64,
    pub parallel: bool,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// One candidate call in a batch, with everything Jev needs - all of it built
/// in code from the tool trace.
#[derive(Debug, Clone)]
pub struct ToolCallItem {
    /// Position in the caller's batch.
    pub index: usize,
    pub tool: String,
    /// Arguments, already truncated by [`preview`].
    pub args_preview: String,
    /// The intent the model stated when it made the call, if any.
    pub intent: Option<String>,
    /// Result body, already truncated. `None` before execution.
    pub result_preview: Option<String>,
    /// Index of an identical earlier call whose result is still in context.
    pub duplicate_of: Option<usize>,
    /// How many times this exact call (tool + arguments) has already failed in
    /// the trace. Non-zero means a third identical attempt is a loop, not a
    /// strategy.
    pub prior_failures: u32,
}

/// Every judgment for one call in the batch. Missing answers come back as
/// unavailable judgments, so partial failures degrade instead of erroring.
#[derive(Debug, Clone)]
pub struct ToolCallJudgment {
    pub index: usize,
    pub needs_human: Judgment<bool>,
    pub redundant: Judgment<bool>,
    pub repeated_failure: Judgment<bool>,
    pub risk: Judgment<RiskBand>,
    pub verification: Judgment<Verification>,
    pub keep_call: Judgment<bool>,
    pub keep_result: Judgment<bool>,
    /// Raw `p(yes)` for the keep questions, when they were asked. Compaction
    /// thresholds on the probability itself (upstream's contract); the
    /// confidence band is applied on top.
    pub keep_call_p: Option<f64>,
    pub keep_result_p: Option<f64>,
}

impl ToolCallJudgment {
    fn blank(index: usize, reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Self {
            index,
            needs_human: Judgment::unavailable("needs_human", reason.clone()),
            redundant: Judgment::unavailable("redundant", reason.clone()),
            repeated_failure: Judgment::unavailable("repeated_failure", reason.clone()),
            risk: Judgment::unavailable("risk", reason.clone()),
            verification: Judgment::unavailable("verification", reason.clone()),
            keep_call: Judgment::unavailable("keep_call", reason.clone()),
            keep_result: Judgment::unavailable("keep_result", reason),
            keep_call_p: None,
            keep_result_p: None,
        }
    }

    /// `true` only when the harness may skip this call without asking anyone:
    /// a duplicate of a call whose result is still verbatim in context, and a
    /// confident answer saying so.
    pub fn gate_skip(&self) -> bool {
        self.redundant.usable() == Some(&true)
    }

    /// Should the harness *say* a person may be needed?
    ///
    /// (Act-band reads of "a person should decide" go through
    /// `needs_human.usable()` - the strict reading - rather than a helper.)
    ///
    /// Surfacing is not acting: a status line the user can ignore does not need
    /// near-certainty, so this uses the confirm band (`p >= 0.75`) instead of the
    /// act band. Anything that changes behavior still goes through `usable()`.
    pub fn wants_human_notice(&self) -> bool {
        self.needs_human.raw() == Some(&true) && self.needs_human.action != GateAction::Escalate
    }

    /// The risk band, when it is worth showing: a high/critical band from an
    /// answer that is at least in the confirm band.
    pub fn risk_notice(&self) -> Option<RiskBand> {
        let band = self.risk.raw().copied()?;
        (self.risk.action != GateAction::Escalate && band.wants_human()).then_some(band)
    }

    /// Is this call the third identical retry of something that keeps failing?
    /// Used to change strategy instead of paying for the loop.
    pub fn stuck(&self) -> bool {
        self.repeated_failure.usable() == Some(&true)
    }

    /// (drop the call, drop its result) at a raw-probability bar, using the
    /// shared [`prune_decision`] rule. `bar` is the pruning threshold, not a
    /// policy band: see [`prune_decision`] for why those differ.
    pub fn prune_at(&self, bar: f64) -> (bool, bool) {
        prune_decision(self.keep_call_p, self.keep_result_p, bar)
    }

    /// A verdict worth a status line: an executed call that failed.
    pub fn failed(&self) -> bool {
        matches!(self.verification.usable(), Some(v) if v.is_problem())
    }
}

/// Whether to drop a call and/or its result, given the two `noul` answers -
/// the rule from
/// [fast-jev-compaction](https://github.com/tamaratran/fast-jev-compaction).
///
/// `bar` is the *pruning* threshold on the raw probability, not a policy band.
/// They answer different questions: `act_confidence` (0.85) decides whether an
/// answer may change what the agent *does*, while this decides whether a tool
/// result may leave the context. The risk here is bounded and recoverable - the
/// result can be re-produced by re-running the tool, user and assistant text is
/// never touched, and the pre-compaction transcript is written to disk - which
/// is why the reference ships 0.5 while the Act band would be 0.925.
/// `[typesafe.compaction] keep_threshold` is the knob; raise it to 0.925 to
/// demand Act-band evidence before anything is pruned.
///
/// A missing answer keeps everything: an unavailable judgment is never treated
/// as permission to delete.
pub fn prune_decision(keep_call: Option<f64>, keep_result: Option<f64>, bar: f64) -> (bool, bool) {
    let (Some(pc), Some(pr)) = (keep_call, keep_result) else {
        return (false, false);
    };
    if pr >= bar {
        (false, false)
    } else if pc >= bar {
        (false, true)
    } else {
        (true, true)
    }
}

/// Truncate a body for judging, marking the cut so Jev knows it is partial.
pub fn preview(body: &str, max: usize) -> String {
    if body.chars().count() <= max {
        return body.to_string();
    }
    let head: String = body.chars().take(max).collect();
    format!(
        "{head}…[truncated by nur: {} of {} chars shown]",
        max,
        body.chars().count()
    )
}

/// The marker that replaces content nur refuses to send to a third-party API.
pub const REDACTED: &str = "[redacted by nur: this looked like a credential and was not sent]";

/// Preview for anything that leaves the machine (Jev state, per-item previews,
/// skill evidence).
///
/// Secret-shaped content is replaced by [`REDACTED`] rather than dropped: the
/// item keeps its position, so index-keyed questions stay aligned, and every
/// other item is still judged - the credential just never leaves the machine.
/// One helper so no call site can forget.
pub fn judge_preview(body: &str, max: usize) -> String {
    if crate::tools::sensitive::body_looks_sensitive(body) {
        return REDACTED.to_string();
    }
    preview(body, max)
}

/// Ask every Judged question for a whole batch in **one** upstream request.
///
/// This is the "put it in the loop" piece: a batch of 8 tool calls costs one
/// Jev round trip, not eight, because the questions are independent and share
/// the same state.
pub fn judge_calls(
    cfg: &TypesafeConfig,
    state: &Value,
    items: &[ToolCallItem],
    scope: JudgeScope,
) -> Vec<ToolCallJudgment> {
    if items.is_empty() {
        return Vec::new();
    }
    let Some(client) = ready(cfg) else {
        return items
            .iter()
            .map(|i| ToolCallJudgment::blank(i.index, "no TypeSafe key"))
            .collect();
    };
    judge_calls_with(&client, cfg, state, items, scope)
}

/// [`judge_calls`] against an injected client (tests, replay).
pub fn judge_calls_with(
    client: &TypesafeClient,
    cfg: &TypesafeConfig,
    state: &Value,
    items: &[ToolCallItem],
    scope: JudgeScope,
) -> Vec<ToolCallJudgment> {
    judge_calls_with_meta(client, cfg, state, items, scope).0
}

/// [`judge_calls_with`] plus what the request cost.
pub fn judge_calls_with_meta(
    client: &TypesafeClient,
    cfg: &TypesafeConfig,
    state: &Value,
    items: &[ToolCallItem],
    scope: JudgeScope,
) -> (Vec<ToolCallJudgment>, JudgeMeta) {
    if items.is_empty() {
        return (Vec::new(), JudgeMeta::default());
    }
    let t = thresholds(cfg);
    let calls: Value = Value::Array(
        items
            .iter()
            .map(|i| {
                json!({
                    "tool": i.tool,
                    "arguments": i.args_preview,
                    "stated_intent": i.intent,
                    "result": i.result_preview,
                    "repeats_an_earlier_call_still_in_context": i.duplicate_of.is_some(),
                })
            })
            .collect(),
    );
    // The questions refer to `calls[<n>]` of the state, so the call table has to
    // be part of that state - otherwise the model is asked about an index it
    // cannot see.
    let state = json!({
        "context": state,
        "calls": calls,
    });
    let state = &state;
    let mut questions: Vec<(String, Question)> = Vec::new();
    let risk_levels: Vec<String> = RISK_LEVELS.iter().map(|s| s.to_string()).collect();
    let verify_labels: Vec<String> = Verification::LABELS.iter().map(|s| s.to_string()).collect();

    for (n, item) in items.iter().enumerate() {
        if scope.wants_pre() {
            questions.push((
                format!("needs_human_{n}"),
                Question::noul_with(
                    format!(
                        "Tool call {0} ({1}) described by `calls[{0}]` of the state: should a \
                         human decide before it runs?",
                        n, item.tool
                    ),
                    NoulCriteria {
                        yes: Some(
                            "it is destructive, irreversible, outward-facing, touches secrets \
                             or money, or exceeds what the user clearly authorized"
                                .into(),
                        ),
                        no: Some("ordinary, in-scope, reversible work".into()),
                    },
                ),
            ));
            if item.duplicate_of.is_some() {
                questions.push((
                    format!("redundant_{n}"),
                    Question::noul_with(
                        format!(
                            "Tool call {0} ({1}) repeats an identical earlier call whose result \
                             is still present in the state. Is running it again unnecessary, \
                             because that earlier result still answers it?",
                            n, item.tool
                        ),
                        NoulCriteria {
                            yes: Some(
                                "the earlier result is still in context and still accurate".into(),
                            ),
                            no: Some(
                                "something changed, or the earlier result is stale/incomplete"
                                    .into(),
                            ),
                        },
                    ),
                ));
            }
            if item.prior_failures > 0 {
                questions.push((
                    format!("repeat_fail_{n}"),
                    Question::noul_with(
                        format!(
                            "Tool call {0} ({1}) is identical to {2} earlier call(s) that already \
                             failed in this trace. Is running it again pointless, because the \
                             same inputs will produce the same failure and something else must \
                             change first?",
                            n, item.tool, item.prior_failures
                        ),
                        NoulCriteria {
                            yes: Some("nothing relevant changed since the earlier failures".into()),
                            no: Some(
                                "the cause was addressed, so the retry can now succeed".into(),
                            ),
                        },
                    ),
                ));
            }
            questions.push((
                format!("risk_{n}"),
                Question::score(
                    format!("How risky is tool call {} ({})?", n, item.tool),
                    risk_levels.clone(),
                ),
            ));
        }
        if scope.wants_post() && item.result_preview.is_some() {
            if scope.wants_verdict() {
                questions.push((
                    format!("verification_{n}"),
                    Question::choice(
                        format!(
                            "Did tool call {0} ({1}) accomplish its intent?",
                            n, item.tool
                        ),
                        verify_labels
                            .iter()
                            .cloned()
                            .map(|l| ChoiceOption::described(l.clone(), verification_rubric(&l)))
                            .collect(),
                    ),
                ));
            }
            questions.push((
                format!("keep_call_{n}"),
                Question::noul_with(
                    format!(
                        "Later in this session, does knowing that tool call {0} ({1}) was made - \
                         with these arguments - still matter?",
                        n, item.tool
                    ),
                    NoulCriteria {
                        yes: Some("the fact of this call constrains or explains later work".into()),
                        no: Some(
                            "it was an intermediate step whose only value was the result".into(),
                        ),
                    },
                ),
            ));
            questions.push((
                format!("keep_result_{n}"),
                Question::noul_with(
                    format!(
                        "Does the result of tool call {0} ({1}) still need to be kept verbatim, \
                         because re-running the tool would not reproduce it or would be costly?",
                        n, item.tool
                    ),
                    NoulCriteria {
                        yes: Some(
                            "its contents will be needed again, or re-running is costly/impossible"
                                .into(),
                        ),
                        no: Some(
                            "the tool can simply be re-run, or the contents are already \
                             superseded"
                                .into(),
                        ),
                    },
                ),
            ));
        }
    }

    let batch = ask_batched(&client, state, questions);
    let batch = match batch {
        Some(b) => b,
        None => {
            return (
                items
                    .iter()
                    .map(|i| ToolCallJudgment::blank(i.index, "typesafe request failed"))
                    .collect(),
                JudgeMeta::default(),
            )
        }
    };
    let meta = JudgeMeta {
        requests: batch.requests.max(1) as u64,
        parallel: batch.parallel,
        input_tokens: batch.input_tokens,
        output_tokens: batch.output_tokens,
    };

    let noul_of = |id: &str| -> Judgment<bool> {
        match batch.get(id) {
            Some(a) => policy::noul_decision(a, id, 0.5, &t),
            None => Judgment::unavailable(id, "no answer for this question"),
        }
    };

    (
        items
            .iter()
            .enumerate()
            .map(|(n, item)| {
                let risk = match batch.get(&format!("risk_{n}")) {
                    Some(a) => Judgment::from_answer(
                        format!("risk_{n}"),
                        a.score()
                            .map(|s| RiskBand::from_score(s, RISK_LEVELS.len())),
                        a.confidence(),
                        &t,
                    ),
                    None => {
                        Judgment::unavailable(format!("risk_{n}"), "no answer for this question")
                    }
                };
                let verification = match batch.get(&format!("verification_{n}")) {
                    Some(a) => {
                        let labels: Vec<String> = batch
                            .get(&format!("verification_{n}"))
                            .and_then(|_| Some(verify_labels.clone()))
                            .unwrap_or_else(|| verify_labels.clone());
                        let value = a
                            .resolve_verbatim(&labels)
                            .and_then(|l| Verification::from_label(l))
                            .or_else(|| a.choice().and_then(Verification::from_label));
                        Judgment::from_answer(
                            format!("verification_{n}"),
                            value,
                            a.confidence(),
                            &t,
                        )
                    }
                    None => Judgment::unavailable(
                        format!("verification_{n}"),
                        "no answer for this question",
                    ),
                };
                let keep_call = noul_of(&format!("keep_call_{n}"));
                let keep_result = noul_of(&format!("keep_result_{n}"));
                ToolCallJudgment {
                    index: item.index,
                    needs_human: noul_of(&format!("needs_human_{n}")),
                    redundant: noul_of(&format!("redundant_{n}")),
                    repeated_failure: noul_of(&format!("repeat_fail_{n}")),
                    risk,
                    verification,
                    keep_call_p: batch.get(&format!("keep_call_{n}")).and_then(Answer::noul),
                    keep_result_p: batch
                        .get(&format!("keep_result_{n}"))
                        .and_then(Answer::noul),
                    keep_call,
                    keep_result,
                }
            })
            .collect(),
        meta,
    )
}

fn verification_rubric(label: &str) -> String {
    match label {
        "succeeded" => "the output shows the intended effect happened".into(),
        "partial" => "some of the intent happened, or the output is mixed".into(),
        "failed" => "an error, or the intended effect clearly did not happen".into(),
        _ => "the output does not contain enough to tell".into(),
    }
}

// ---------------------------------------------------------------------------
// Skill layer judgments.
//
// A triggered skill is not a decoration: its `SKILL.md` states requirements the
// turn is supposed to follow. These two judgments live *in the skill layer* -
// one narrows the skill's own rule list to the rules this request actually
// triggers, the other answers "was it carried out" from the tool trace - so the
// skill machinery can act on it directly. There is no separate reviewer: the
// answer is used where skills are handled.
// ---------------------------------------------------------------------------

/// How an activated skill was carried out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillCompliance {
    UsedProperly,
    Partial,
    Ignored,
    NotApplicable,
}

impl SkillCompliance {
    pub const LABELS: &'static [&'static str] = &[
        "used_properly",
        "partially_used",
        "ignored",
        "not_applicable",
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::UsedProperly => "used_properly",
            Self::Partial => "partially_used",
            Self::Ignored => "ignored",
            Self::NotApplicable => "not_applicable",
        }
    }

    pub fn from_label(label: &str) -> Option<Self> {
        match label.trim().to_ascii_lowercase().as_str() {
            "used_properly" => Some(Self::UsedProperly),
            "partially_used" => Some(Self::Partial),
            "ignored" => Some(Self::Ignored),
            "not_applicable" => Some(Self::NotApplicable),
            _ => None,
        }
    }

    /// Does the skill machinery have something to correct?
    pub fn is_shortfall(&self) -> bool {
        matches!(self, Self::Ignored | Self::Partial)
    }
}

/// Did the work follow this skill as written?
///
/// `requirements` and `evidence` are both built in code: the rules come from
/// the skill body, the evidence from this turn's tool trace.
pub fn judge_skill_use(
    cfg: &TypesafeConfig,
    request: &str,
    skill: &str,
    requirements: &[String],
    evidence: &[String],
) -> Judgment<SkillCompliance> {
    if requirements.is_empty() {
        // Nothing stated to check against: the skill is advisory only.
        return Judgment::unavailable(
            format!("skill_use:{skill}"),
            "this skill states no checkable requirement",
        );
    }
    let labels: Vec<String> = SkillCompliance::LABELS
        .iter()
        .map(|s| s.to_string())
        .collect();
    let state = json!({
        "request": request,
        "skill": skill,
        "skill_requirements": requirements,
        "tool_trace_this_turn": evidence,
    });
    choose(
        cfg,
        &state,
        &format!("skill_use:{skill}"),
        format!(
            "Was **{skill}** carried out as its stated requirements demand? \
             'used_properly' = the trace shows its required actions were taken; \
             'partially_used' = some were, or a stated order/guard was skipped; \
             'ignored' = the trace shows a different approach with none of its \
             required actions; 'not_applicable' = the request turned out not to \
             need this skill."
        ),
        &labels,
        false,
    )
    .map(|l| SkillCompliance::from_label(&l).unwrap_or(SkillCompliance::NotApplicable))
}

/// Which of a skill's stated requirements was not carried out.
///
/// Answers are drawn from the skill's own requirement list plus "no specific
/// rule" - so a steer built from this names a rule the skill really states.
pub fn missed_requirement(
    cfg: &TypesafeConfig,
    request: &str,
    skill: &str,
    requirements: &[String],
    evidence: &[String],
) -> Judgment<String> {
    let state = json!({
        "request": request,
        "skill": skill,
        "skill_requirements": requirements,
        "tool_trace_this_turn": evidence,
    });
    choose(
        cfg,
        &state,
        &format!("missed_requirement:{skill}"),
        format!(
            "Which requirement of **{skill}** was not carried out? Pick the single \
             requirement the trace shows was skipped or done incompletely."
        ),
        requirements,
        true,
    )
}

/// Which of a skill's stated requirements this request actually triggers.
///
/// The candidate list is the skill's own rules (code-extracted), so the answer
/// is always a rule the skill really states - never a rule Jev invented. This
/// is how a triggered skill gets *used properly*: its full body is long, and
/// the applicable rules are injected as a short, specific checklist.
///
/// Returns the selected rules in the skill's own order. An empty result means
/// "no narrowing" - the caller keeps the full body.
pub fn select_requirements(
    cfg: &TypesafeConfig,
    request: &str,
    skill: &str,
    requirements: &[String],
    max_selected: usize,
) -> Vec<String> {
    let client = match ready(cfg) {
        Some(c) => c,
        None => return Vec::new(),
    };
    if requirements.is_empty() || requirements.len() > super::questions::MAX_CHOICE_OPTIONS {
        return Vec::new();
    }
    // One Noul per rule: "does this request trigger this rule?" - asked in one
    // request, so selecting a checklist costs a single round trip.
    let questions: Vec<(String, Question)> = requirements
        .iter()
        .enumerate()
        .map(|(i, r)| {
            (
                format!("applies_{i}"),
                Question::noul_with(
                    format!("Does this request trigger the following rule of **{skill}**?\n\n{r}"),
                    NoulCriteria {
                        yes: Some("the request calls for this rule's action now".into()),
                        no: Some("the rule does not apply to this request".into()),
                    },
                ),
            )
        })
        .collect();
    let state = json!({ "request": request });
    let t = thresholds(cfg);
    let Some(batch) = ask_batched(&client, &state, questions) else {
        return Vec::new();
    };
    let mut selected: Vec<String> = Vec::new();
    for (i, r) in requirements.iter().enumerate() {
        let id = format!("applies_{i}");
        let p = batch.get(&id).and_then(Answer::noul).unwrap_or(0.0);
        let conf = batch.get(&id).and_then(Answer::confidence);
        // Only act-band certainty selects a rule: a checklist nobody is sure
        // about is worse than the full skill body.
        if p >= 0.5 && conf.map(|c| c >= t.act).unwrap_or(false) {
            selected.push(r.clone());
        }
    }
    if selected.len() > max_selected.max(1) {
        selected.truncate(max_selected.max(1));
    }
    if selected.is_empty() {
        return Vec::new();
    }
    telemetry::record_decision(Some(t.act), true, t.escalate);
    selected
}

#[cfg(test)]
mod tests {
    use super::super::client::{Transport, TransportFn, TypesafeClient};
    use super::*;
    use std::sync::{Arc, Mutex};

    fn cfg() -> TypesafeConfig {
        TypesafeConfig {
            enabled: true,
            api_key: "ts-test".into(),
            act_confidence: 0.85,
            escalate_confidence: 0.5,
            ..TypesafeConfig::default()
        }
    }

    /// A fake endpoint: answers come from `answer`, and every request body is
    /// recorded so tests can assert batching.
    fn endpoint(
        answer: impl Fn(&str, &Value) -> Value + Send + Sync + 'static,
    ) -> (TypesafeClient, Arc<Mutex<Vec<Value>>>) {
        let seen: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let recorder = seen.clone();
        let t: Arc<TransportFn> = Arc::new(move |body: &Value| {
            recorder.lock().unwrap().push(body.clone());
            let qs = body
                .get("questions")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let mut answers = serde_json::Map::new();
            for (id, q) in qs {
                answers.insert(id.clone(), answer(&id, &q));
            }
            Ok(json!({
                "model": "jev-latest",
                "answers": Value::Object(answers),
                "usage": {"input_tokens": 7, "output_tokens": 2},
            }))
        });
        (
            TypesafeClient::with_transport(&cfg(), Transport::Fake(t)),
            seen,
        )
    }

    fn noul(p: f64) -> Value {
        json!({"type": "noul", "noul": p})
    }

    fn choice(label: &str, confidence: f64) -> Value {
        let mut probs = serde_json::Map::new();
        probs.insert(label.to_string(), json!(confidence));
        json!({"type": "choice", "choice": label,
               "probabilities": Value::Object(probs), "confidence": confidence})
    }

    /// A sidecar is a credential, not a model: routing a subagent at it must be
    /// refused with a message that says what to do instead.
    #[test]
    fn a_sidecar_is_not_a_chat_provider() {
        assert!(crate::providers::is_sidecar_provider("typesafe"));
        assert!(!crate::providers::is_sidecar_provider("openai"));
        // It still resolves as a credential-bearing entry ...
        assert_eq!(
            crate::providers::by_id("typesafe").map(|p| p.id),
            Some("typesafe")
        );
        // ... while the chat catalog itself is untouched by the pin.
        assert!(
            !crate::providers::PROVIDERS
                .iter()
                .any(|p| p.id == "typesafe"),
            "the sidecar must not join the chat provider catalog"
        );
        assert_eq!(
            crate::providers::sidecar_providers().first().map(|p| p.id),
            Some("typesafe"),
            "and it is pinned first in the picker"
        );
    }

    #[test]
    fn tool_need_asks_one_confident_question_per_tool() {
        // 0.97 -> confidence 0.94 (act band), 0.80 -> 0.60 (confirm band).
        let (client, seen) = endpoint(|id, _| match id {
            "need_0" => noul(0.97),
            "need_1" => noul(0.80),
            _ => noul(0.05),
        });
        let tools = vec![
            "tldraw".to_string(),
            "fractal".to_string(),
            "omp".to_string(),
        ];
        let judged = tool_need_with(
            &client,
            &cfg(),
            &json!({"task": "fix the failing test"}),
            &tools,
        );
        assert_eq!(judged.len(), 3);
        assert_eq!(seen.lock().unwrap().len(), 1, "one request for every tool");
        // Only the acting band produces a probability the caller may act on; a
        // confirm-band answer becomes None, which the caller reads as "keep".
        assert_eq!(judged[0].1, Some(0.97));
        assert_eq!(judged[1].1, None, "confirm band cannot remove a tool");
        assert_eq!(judged[2].1, Some(0.05), "a confident no is actionable");
    }

    #[test]
    fn tool_need_returns_nothing_without_candidates() {
        let (client, seen) = endpoint(|_, _| noul(0.9));
        assert!(tool_need_with(&client, &cfg(), &json!({}), &[]).is_empty());
        assert!(
            seen.lock().unwrap().is_empty(),
            "no request for an empty list"
        );
    }

    /// Test seam for `tool_need` against an injected client.
    fn tool_need_with(
        client: &TypesafeClient,
        cfg: &TypesafeConfig,
        state: &Value,
        candidates: &[String],
    ) -> Vec<(String, Option<f64>)> {
        if candidates.is_empty() {
            return Vec::new();
        }
        let questions: Vec<(String, Question)> = candidates
            .iter()
            .enumerate()
            .map(|(i, name)| {
                (
                    format!("need_{i}"),
                    Question::noul(format!("Does this task need {name}?")),
                )
            })
            .collect();
        let t = thresholds(cfg);
        let Some(batch) = ask_batched(client, state, questions) else {
            return Vec::new();
        };
        candidates
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let id = format!("need_{i}");
                let p = batch.get(&id).and_then(Answer::noul);
                let confident = match (p, batch.get(&id).and_then(Answer::confidence)) {
                    (Some(p), Some(c)) => Some(p).filter(|_| c >= t.act),
                    _ => None,
                };
                (name.clone(), confident)
            })
            .collect()
    }

    #[test]
    fn notices_use_the_confirm_band_while_acting_stays_strict() {
        // p=0.8 -> derived confidence 0.6: worth saying, not worth acting on.
        let (client, _) = endpoint(|id, _| {
            if id.starts_with("needs_human") {
                noul(0.8)
            } else {
                noul(0.2)
            }
        });
        let items = vec![ToolCallItem {
            index: 0,
            tool: "bash".into(),
            args_preview: "{}".into(),
            intent: None,
            result_preview: None,
            duplicate_of: None,
            prior_failures: 0,
        }];
        let judged = judge_calls_with(&client, &cfg(), &json!({}), &items, JudgeScope::Pre);
        assert_eq!(judged[0].needs_human.action, GateAction::Confirm);
        assert!(
            judged[0].wants_human_notice(),
            "a notice may fire in the confirm band"
        );
        assert_eq!(
            judged[0].needs_human.usable(),
            None,
            "acting may not, and there is no act-band helper that says otherwise"
        );
    }

    #[test]
    fn secret_shaped_content_never_reaches_the_request_body() {
        let secret = "export ANTHROPIC_API_KEY=sk-ant-api03-0123456789abcdefghijklmnop";
        let (client, seen) = endpoint(|_, _| noul(0.95));
        let items = vec![ToolCallItem {
            index: 0,
            tool: "bash".into(),
            args_preview: judge_preview(secret, 400),
            intent: None,
            result_preview: Some(judge_preview(secret, 400)),
            duplicate_of: None,
            prior_failures: 0,
        }];
        judge_calls_with(&client, &cfg(), &json!({}), &items, JudgeScope::Both);
        let body = serde_json::to_string(&seen.lock().unwrap()[0]).unwrap();
        assert!(
            !body.contains("sk-ant-api03-0123456789abcdefghijklmnop"),
            "the credential must not be in the request: {body}"
        );
        assert!(!body.contains("ANTHROPIC_API_KEY"), "{body}");
        assert!(body.contains("redacted by nur"), "{body}");
        // The item still counts, so index-keyed questions stay aligned.
        assert_eq!(
            seen.lock().unwrap()[0]["state"]["calls"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn a_plain_body_is_previewed_not_redacted() {
        assert_eq!(
            judge_preview("cargo test output: 12 passed", 100),
            "cargo test output: 12 passed"
        );
        assert!(judge_preview("Authorization: Bearer abcdef", 100).contains("redacted"));
        let long = "x".repeat(500);
        assert!(judge_preview(&long, 50).contains("truncated by nur"));
    }

    #[test]
    fn a_whole_batch_costs_one_request() {
        // 0.97, not 0.9: a Noul's confidence is its distance from the coin flip
        // (0.9 -> 0.80, still the confirm band), so acting needs near-certainty.
        let (client, seen) = endpoint(|_, _| noul(0.97));
        // Both scopes ask five questions per call, so keep the batch under the
        // client's per-request cap to pin the "one request" property itself.
        let items: Vec<ToolCallItem> = (0..3)
            .map(|n| ToolCallItem {
                index: n,
                tool: "read_file".into(),
                args_preview: format!("{{\"path\":\"f{n}.rs\"}}"),
                intent: None,
                result_preview: Some("ok".into()),
                duplicate_of: None,
                prior_failures: 0,
            })
            .collect();
        let judged = judge_calls_with(
            &client,
            &cfg(),
            &json!({"goal": "fix the test"}),
            &items,
            JudgeScope::Both,
        );
        assert_eq!(judged.len(), 3);
        assert_eq!(seen.lock().unwrap().len(), 1, "three calls, one request");
        // The call table the questions refer to is part of the state.
        let body = &seen.lock().unwrap()[0];
        assert_eq!(body["state"]["calls"].as_array().unwrap().len(), 3);
        assert_eq!(body["state"]["context"]["goal"], "fix the test");
        assert_eq!(judged[0].needs_human.usable(), Some(&true));
    }

    #[test]
    fn a_large_batch_splits_without_exceeding_the_cap() {
        let (client, seen) = endpoint(|_, _| noul(0.9));
        let items: Vec<ToolCallItem> = (0..12)
            .map(|n| ToolCallItem {
                index: n,
                tool: "read_file".into(),
                args_preview: "{}".into(),
                intent: None,
                result_preview: Some("ok".into()),
                duplicate_of: None,
                prior_failures: 0,
            })
            .collect();
        let judged = judge_calls_with(&client, &cfg(), &json!({}), &items, JudgeScope::Both);
        assert_eq!(judged.len(), 12);
        let seen = seen.lock().unwrap();
        assert!(seen.len() > 1, "60 questions cannot fit one request");
        for body in seen.iter() {
            let n = body["questions"].as_object().unwrap().len();
            assert!(n <= 24, "request carried {n} questions");
        }
        // Every call is judged exactly once across the requests.
        for i in 0..12 {
            let asked = seen
                .iter()
                .filter(|b| b["questions"].get(format!("risk_{i}")).is_some())
                .count();
            assert_eq!(asked, 1, "call {i} must be judged exactly once");
        }
    }

    #[test]
    fn post_scope_asks_keep_questions_with_raw_probabilities() {
        let (client, seen) = endpoint(|id, _| {
            if id.starts_with("keep_result_") {
                noul(0.1)
            } else if id.starts_with("keep_call_") {
                noul(0.9)
            } else {
                noul(0.5)
            }
        });
        let items = vec![ToolCallItem {
            index: 0,
            tool: "grep".into(),
            args_preview: "{}".into(),
            intent: None,
            result_preview: Some("matches".into()),
            duplicate_of: None,
            prior_failures: 0,
        }];
        let judged = judge_calls_with(&client, &cfg(), &json!({}), &items, JudgeScope::Post);
        assert_eq!(judged[0].keep_result_p, Some(0.1));
        assert_eq!(judged[0].keep_call_p, Some(0.9));
        let ids: Vec<String> = seen.lock().unwrap()[0]["questions"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert!(ids.iter().any(|i| i == "keep_result_0"));
        assert!(ids.iter().any(|i| i == "verification_0"));
        // Pre-scope questions must not be asked here.
        assert!(!ids.iter().any(|i| i == "risk_0"));
    }

    #[test]
    fn pre_scope_only_asks_about_duplicates_that_exist() {
        let (client, seen) = endpoint(|_, _| noul(0.95));
        let items = vec![
            ToolCallItem {
                index: 0,
                tool: "read_file".into(),
                args_preview: "{}".into(),
                intent: None,
                result_preview: None,
                duplicate_of: Some(0),
                prior_failures: 0,
            },
            ToolCallItem {
                index: 1,
                tool: "bash".into(),
                args_preview: "{}".into(),
                intent: None,
                result_preview: None,
                duplicate_of: None,
                prior_failures: 2,
            },
        ];
        let judged = judge_calls_with(&client, &cfg(), &json!({}), &items, JudgeScope::Pre);
        let ids: Vec<String> = seen.lock().unwrap()[0]["questions"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert!(ids.iter().any(|i| i == "redundant_0"));
        assert!(
            !ids.iter().any(|i| i == "redundant_1"),
            "nothing to compare against"
        );
        assert!(
            ids.iter().any(|i| i == "repeat_fail_1"),
            "two prior failures"
        );
        assert!(judged[0].gate_skip());
        assert!(judged[1].stuck());
    }

    #[test]
    fn a_failing_gate_never_prunes_and_never_skips() {
        let t: Arc<TransportFn> = Arc::new(|_| Err("network down".into()));
        let client = TypesafeClient::with_transport(&cfg(), Transport::Fake(t));
        let items = vec![ToolCallItem {
            index: 0,
            tool: "read_file".into(),
            args_preview: "{}".into(),
            intent: None,
            result_preview: None,
            duplicate_of: Some(0),
            prior_failures: 3,
        }];
        let judged = judge_calls_with(&client, &cfg(), &json!({}), &items, JudgeScope::Pre);
        assert!(
            !judged[0].gate_skip(),
            "a failed judgment is not permission to skip"
        );
        assert!(!judged[0].stuck());
        assert!(judged[0].keep_call.usable().is_none());
    }

    #[test]
    fn choose_maps_back_onto_code_options_and_never_invents() {
        let (client, _) = endpoint(|_, _| choice("grep", 0.95));
        let candidates = vec!["read_file".to_string(), "grep".to_string()];
        let j = choose_with(
            &client,
            &json!("s"),
            "pick",
            "which tool",
            &candidates,
            true,
            &Thresholds::default(),
        );
        assert_eq!(j.usable().map(String::as_str), Some("grep"));

        // An option nobody supplied resolves to no judgment at all.
        let (client, _) = endpoint(|_, _| choice("format_every_disk", 0.99));
        let j = choose_with(
            &client,
            &json!("s"),
            "pick",
            "which tool",
            &candidates,
            true,
            &Thresholds::default(),
        );
        assert_eq!(j.raw(), None);
    }

    #[test]
    fn low_confidence_choice_is_an_escalation_not_an_answer() {
        let (client, _) = endpoint(|_, _| choice("grep", 0.62));
        let j = choose_with(
            &client,
            &json!("s"),
            "pick",
            "which tool",
            &["grep".to_string()],
            false,
            &Thresholds::default(),
        );
        assert_eq!(j.action, GateAction::Confirm);
        assert_eq!(j.usable(), None, "confirm band may not act");
        assert_eq!(j.raw().map(String::as_str), Some("grep"));
    }

    #[test]
    fn none_of_these_is_not_a_pick() {
        let (client, _) = endpoint(|_, _| choice(NO_MATCH, 0.97));
        let j = choose_with(
            &client,
            &json!("s"),
            "pick",
            "which tool",
            &["read_file".to_string()],
            true,
            &Thresholds::default(),
        );
        assert_eq!(j.value, None);
        assert_eq!(j.confidence, Some(0.97), "the answer itself was certain");
    }

    #[test]
    fn risk_is_a_graded_band_from_an_ordered_score() {
        assert_eq!(RiskBand::from_score(0.0, 5), RiskBand::Safe);
        assert_eq!(RiskBand::from_score(1.0, 5), RiskBand::Low);
        assert_eq!(RiskBand::from_score(3.0, 5), RiskBand::High);
        assert_eq!(RiskBand::from_score(10.0, 5), RiskBand::Critical);
        assert!(RiskBand::Critical.wants_human());
        assert!(RiskBand::Critical.stakes() > RiskBand::Low.stakes());
        assert!(RiskBand::Safe.stakes() < 0.5);
    }

    #[test]
    fn rank_orders_by_score_in_one_request() {
        let (client, seen) = endpoint(|id, _| {
            let level = match id {
                "relevance_0" => 1.0,
                "relevance_2" => 3.0,
                _ => 0.0,
            };
            json!({"type": "score", "score": level,
                   "legend": {"0": "none", "1": "low", "2": "mid", "3": "high"},
                   "probabilities": {"3": 1.0}, "confidence": 0.9})
        });
        let candidates = vec!["a".into(), "b".into(), "c".into()];
        let levels: Vec<String> = RELEVANCE_LEVELS.iter().map(|s| s.to_string()).collect();
        let judged = rank_with(&client, "q", &candidates, &levels, &Thresholds::default());
        assert_eq!(
            seen.lock().unwrap().len(),
            1,
            "one request for all candidates"
        );
        assert_eq!(judged[2].raw().copied(), Some(3.0));
        assert_eq!(judged[1].raw().copied(), Some(0.0));
        // The order comes from the judgments already fetched - no second request.
        let order = rank_order(&judged);
        assert_eq!(order[0], 2, "highest score first");
        assert_eq!(seen.lock().unwrap().len(), 1, "ordering adds no request");
    }

    #[test]
    fn an_unplaced_candidate_cannot_claim_a_position() {
        // Regression: the old ordering sorted *every* candidate by score, so a
        // low-confidence answer moved candidates around even though the policy
        // thresholds on confidence and the doc promises unplaced ones keep their
        // input position.
        let judged = vec![
            Judgment::<f64>::unavailable("relevance_0", "no key"), // index 0: none at all
            Judgment::<f64>::from_answer(
                "relevance_1",
                Some(2.5),
                Some(0.4),
                &Thresholds::default(),
            ), // index 1: sub-threshold, despite the best raw score
            Judgment::<f64>::from_answer(
                "relevance_2",
                Some(3.0),
                Some(0.9),
                &Thresholds::default(),
            ), // index 2: placed, best
            Judgment::<f64>::from_answer(
                "relevance_3",
                Some(1.0),
                Some(0.9),
                &Thresholds::default(),
            ), // index 3: placed, worst
        ];
        assert_eq!(judged[1].action, GateAction::Escalate, "test premise");
        let order = rank_order(&judged);
        assert_eq!(order[0], 2, "the confident best comes first");
        assert_eq!(order[1], 3, "then the other confident one");
        assert_eq!(&order[2..], &[0, 1], "unplaced candidates keep input order");
    }

    #[test]
    fn a_skill_is_checked_against_its_own_rules() {
        let (client, _) = endpoint(|id, _| {
            if id.starts_with("skill_use") {
                choice("ignored", 0.93)
            } else {
                // A real endpoint can only answer with a candidate it was given.
                choice("You MUST write the failing test first", 0.9)
            }
        });
        let rules = vec![
            "You MUST write the failing test first".to_string(),
            "NEVER refactor before the test passes".to_string(),
        ];
        let evidence = vec!["call read_file src/lib.rs".to_string()];
        let j = judge_skill_use_with(&client, "add tdd", "tdd", &rules, &evidence);
        assert_eq!(j.usable(), Some(&SkillCompliance::Ignored));
        assert!(j.usable().unwrap().is_shortfall());
        let missed = missed_requirement_with(&client, "add tdd", "tdd", &rules, &evidence);
        assert_eq!(
            missed.usable().map(String::as_str),
            Some("You MUST write the failing test first"),
            "the answer is one of the skill's own rules"
        );
    }

    #[test]
    fn a_skill_with_no_stated_rules_is_never_judged() {
        let (client, seen) = endpoint(|_, _| noul(0.9));
        let j = judge_skill_use_with(&client, "do it", "advisory-skill", &[], &[]);
        assert!(j.value.is_none());
        assert_eq!(j.action, GateAction::Escalate);
        assert!(seen.lock().unwrap().is_empty(), "no request for nothing");
    }

    #[test]
    fn requirement_selection_only_takes_confident_rules() {
        let (client, _) = endpoint(|id, _| match id {
            "applies_0" => noul(0.95),
            "applies_1" => noul(0.60), // unsure -> not selected
            _ => noul(0.05),
        });
        let rules = vec![
            "You MUST run the test suite".to_string(),
            "ALWAYS update the changelog".to_string(),
            "NEVER touch generated files".to_string(),
        ];
        let picked = select_requirements_with(&client, &cfg(), "fix it", &rules, 8);
        assert_eq!(picked, vec!["You MUST run the test suite".to_string()]);
    }

    // --- test seams: the policy bodies, against an injected client.

    fn judge_skill_use_with(
        client: &TypesafeClient,
        request: &str,
        skill: &str,
        requirements: &[String],
        evidence: &[String],
    ) -> Judgment<SkillCompliance> {
        if requirements.is_empty() {
            return Judgment::unavailable("skill_use", "no checkable requirement");
        }
        let labels: Vec<String> = SkillCompliance::LABELS
            .iter()
            .map(|s| s.to_string())
            .collect();
        let state = json!({"request": request, "skill": skill,
                           "skill_requirements": requirements,
                           "tool_trace_this_turn": evidence});
        choose_with(
            client,
            &state,
            &format!("skill_use:{skill}"),
            "was the skill carried out",
            &labels,
            false,
            &Thresholds::default(),
        )
        .map(|l| SkillCompliance::from_label(&l).unwrap_or(SkillCompliance::NotApplicable))
    }

    fn missed_requirement_with(
        client: &TypesafeClient,
        request: &str,
        skill: &str,
        requirements: &[String],
        evidence: &[String],
    ) -> Judgment<String> {
        let state = json!({"request": request, "skill": skill,
                           "skill_requirements": requirements,
                           "tool_trace_this_turn": evidence});
        choose_with(
            client,
            &state,
            &format!("missed_requirement:{skill}"),
            "which rule was skipped",
            requirements,
            true,
            &Thresholds::default(),
        )
    }

    fn select_requirements_with(
        client: &TypesafeClient,
        cfg: &TypesafeConfig,
        request: &str,
        requirements: &[String],
        max_selected: usize,
    ) -> Vec<String> {
        let questions: Vec<(String, Question)> = requirements
            .iter()
            .enumerate()
            .map(|(i, r)| {
                (
                    format!("applies_{i}"),
                    Question::noul(format!("does this rule apply: {r}")),
                )
            })
            .collect();
        let t = thresholds(cfg);
        let batch = ask_batched(client, &json!({"request": request}), questions).unwrap();
        let mut out = Vec::new();
        for (i, r) in requirements.iter().enumerate() {
            let id = format!("applies_{i}");
            let p = batch.get(&id).and_then(Answer::noul).unwrap_or(0.0);
            let conf = batch.get(&id).and_then(Answer::confidence);
            if p >= 0.5 && conf.map(|c| c >= t.act).unwrap_or(false) {
                out.push(r.clone());
            }
        }
        out.truncate(max_selected.max(1));
        out
    }
}

/// Live checks against the real endpoint.
///
/// Ignored by default: they cost real requests and need a real key, so the
/// normal suite never touches the network. Run them when a key lands:
///
/// ```text
/// TYPESAFE_API_KEY=... cargo test --bin nur typesafe_live -- --ignored --nocapture
/// ```
#[cfg(test)]
mod live {
    use super::*;
    use crate::config::TypesafeConfig;

    fn live_cfg() -> Option<TypesafeConfig> {
        let cfg = TypesafeConfig::default();
        if crate::typesafe::client::api_key(&cfg).is_none() {
            eprintln!("TYPESAFE_API_KEY not set - skipping the live check");
            return None;
        }
        Some(cfg)
    }

    /// The primitive contract, end to end: a Noul probability, a Choice that
    /// resolves verbatim onto code-supplied options, a Score on ordered levels,
    /// batched into one request with accounting.
    #[test]
    #[ignore = "live TypeSafe request (needs TYPESAFE_API_KEY)"]
    fn typesafe_live_primitives() {
        let Some(cfg) = live_cfg() else { return };
        let client = client::shared_client(&cfg).expect("client with a key");
        let state = json!({
            "support_message": "My payouts have been failing for three days and nobody answered.",
        });
        let questions = vec![
            (
                "urgent".to_string(),
                Question::noul_with(
                    "Does this message convey urgency?",
                    crate::typesafe::questions::NoulCriteria::default(),
                ),
            ),
            (
                "team".to_string(),
                Question::choice(
                    "Which team should handle this?",
                    vec![
                        crate::typesafe::questions::ChoiceOption::described(
                            "billing",
                            "Payments, invoicing, refunds",
                        ),
                        crate::typesafe::questions::ChoiceOption::described(
                            "technical",
                            "Bugs, outages, integrations",
                        ),
                        crate::typesafe::questions::ChoiceOption::described(
                            "sales",
                            "Pricing, upgrades, new accounts",
                        ),
                    ],
                ),
            ),
            (
                "frustration".to_string(),
                Question::score(
                    "How frustrated is the customer?",
                    vec!["Calm".into(), "Frustrated".into(), "Very angry".into()],
                ),
            ),
        ];
        let batch = client
            .ask(&state, &questions)
            .expect("live request succeeds");
        assert!(batch.errors.is_empty(), "{:?}", batch.errors);
        assert_eq!(batch.answers.len(), 3, "one request, three answers");
        // The endpoint resolves the alias: asking for `jev-latest` reports
        // something like `jev-1.13.0`, so never assert the requested string back.
        println!("resolved model: {}", batch.model);
        assert!(batch.model.starts_with("jev"), "{}", batch.model);
        assert!(
            batch.input_tokens > 0 && batch.output_tokens > 0,
            "usage reported"
        );

        let urgent = batch
            .get("urgent")
            .and_then(Answer::noul)
            .expect("noul answer");
        println!("urgent p(yes) = {urgent:.3}");
        assert!((0.0..=1.0).contains(&urgent));
        assert!(urgent > 0.5, "the message reads as urgent: {urgent}");

        let team = batch.get("team").expect("choice answer");
        let options = vec![
            "billing".to_string(),
            "technical".to_string(),
            "sales".to_string(),
        ];
        let picked = team
            .resolve_verbatim(&options)
            .expect("the pick is one of the supplied options");
        println!("team = {picked} (confidence {:?})", team.confidence());
        assert!(
            crate::typesafe::policy::Thresholds::default().band(team.confidence())
                != crate::typesafe::policy::GateAction::Escalate,
            "a clear support message should not come back as a coin flip"
        );

        let frustration = batch
            .get("frustration")
            .and_then(Answer::score)
            .expect("score");
        println!("frustration = {frustration:.2} of 0..=2");
        assert!((0.0..=2.0).contains(&frustration));
        assert!(frustration > 0.5, "three days of silence is not 'calm'");
    }

    /// The compaction path against the real endpoint: a stale tool call may be
    /// dropped, a live one must survive verbatim, and text is never touched.
    #[test]
    #[ignore = "live TypeSafe request (needs TYPESAFE_API_KEY)"]
    fn typesafe_live_compaction_keeps_survivors_verbatim() {
        let Some(cfg) = live_cfg() else { return };
        let items = vec![
            json!({"role":"user","content":[{"type":"input_text",
                "text":"Fix the failing test in src/lib.rs. Never edit generated files."}]}),
            json!({"type":"function_call","call_id":"old1","name":"list_dir",
                "arguments":"{\"path\":\".\"}"}),
            json!({"type":"function_call_output","call_id":"old1",
                "output":"Cargo.toml\nREADME.md\nsrc\n"}),
            json!({"type":"function_call","call_id":"old2","name":"read_file",
                "arguments":"{\"path\":\"README.md\"}"}),
            json!({"type":"function_call_output","call_id":"old2",
                "output":"# nur\n\nThe README, unrelated to the failing test.\n"}),
            json!({"type":"function_call","call_id":"live1","name":"bash",
                "arguments":"{\"command\":\"cargo test --lib\"}"}),
            json!({"type":"function_call_output","call_id":"live1",
                "output":"error[E0425]: cannot find value `TOTAL` in function `sum_all`\n  --> src/lib.rs:42:9\n"}),
            json!({"type":"function_call","call_id":"live2","name":"read_file",
                "arguments":"{\"path\":\"src/lib.rs\"}"}),
            json!({"type":"function_call_output","call_id":"live2",
                "output":"pub fn sum_all(xs: &[u32]) -> u32 { xs.iter().sum() }\n"}),
            json!({"role":"user","content":[{"type":"input_text","text":"still working"}]}),
        ];
        let opts = crate::typesafe::compact::CompactOptions::from_config(&cfg.compaction)
            .with_goal("fix the failing test in src/lib.rs");
        let outcome = crate::typesafe::compact::compact_items(&cfg, &items, &opts)
            .expect("live compaction produces a judgment");
        println!("{}", outcome.stats.summary());
        for o in &outcome.outcomes {
            println!("  {} {} ({})", o.call_id, o.decision.as_str(), o.reason);
        }
        // Text items are never removed.
        let kept_text: Vec<&str> = outcome
            .items
            .iter()
            .filter_map(|i| i.get("content"))
            .filter_map(|c| c.as_array())
            .filter_map(|a| a.first())
            .filter_map(|p| p.get("text"))
            .filter_map(Value::as_str)
            .collect();
        assert!(
            kept_text
                .iter()
                .any(|t| t.contains("Never edit generated files")),
            "{kept_text:?}"
        );
        // Survivors are byte-identical, and no result is orphaned from its call.
        for item in &outcome.items {
            if let Some(id) = item.get("call_id").and_then(Value::as_str) {
                let has_call = outcome.items.iter().any(|o| {
                    o.get("type").and_then(Value::as_str) == Some("function_call")
                        && o.get("call_id").and_then(Value::as_str) == Some(id)
                });
                let has_result = outcome.items.iter().any(|o| {
                    o.get("type").and_then(Value::as_str) == Some("function_call_output")
                        && o.get("call_id").and_then(Value::as_str) == Some(id)
                });
                assert!(has_call == has_result, "pair {id} was split");
            }
        }
        // The error text that names the failure must survive somewhere in the
        // transcript, since that is the task.
        let all = serde_json::to_string(&outcome.items).unwrap();
        assert!(
            outcome.stats.dropped_calls == 0 || !all.contains("TOTAL") || all.contains("E0425"),
            "dropping a result must not lose the error that is the task"
        );
    }

    /// One real judgment through the same policy the loop uses for a pending
    /// action.
    #[test]
    #[ignore = "live TypeSafe request (needs TYPESAFE_API_KEY)"]
    fn typesafe_live_risk_band() {
        let Some(cfg) = live_cfg() else { return };
        let state = json!({
            "planned_action": "force-push the shared main branch after deleting ./target",
            "context": "a shared repository with three contributors",
        });
        let j = risk(&cfg, &state, "risk");
        println!("{}", j.describe());
        let band = j.raw().copied().expect("a live judgment comes back");
        assert!(
            band >= RiskBand::High,
            "force-pushing a shared main is not 'safe': {band:?}"
        );
        // The band maps onto stakes, which is what raises the acting bar.
        assert!(band.stakes() >= 0.5, "{band:?}");
    }

    /// One request, many items: the batch property the loop depends on, against
    /// the real endpoint (a wrong assumption here would show up as latency, not
    /// as an error).
    #[test]
    #[ignore = "live TypeSafe request (needs TYPESAFE_API_KEY)"]
    fn typesafe_live_tool_batch_is_one_request() {
        let Some(cfg) = live_cfg() else { return };
        let items: Vec<ToolCallItem> = vec![
            ToolCallItem {
                index: 0,
                tool: "read_file".into(),
                args_preview: "{\"path\":\"src/lib.rs\"}".into(),
                intent: None,
                result_preview: Some("pub fn sum_all(xs: &[u32]) -> u32".into()),
                duplicate_of: None,
                prior_failures: 0,
            },
            ToolCallItem {
                index: 1,
                tool: "bash".into(),
                args_preview: "{\"command\":\"cargo test --lib\"}".into(),
                intent: None,
                result_preview: Some("error[E0425]: cannot find value `TOTAL`".into()),
                duplicate_of: None,
                prior_failures: 0,
            },
            ToolCallItem {
                index: 2,
                tool: "read_file".into(),
                args_preview: "{\"path\":\"src/lib.rs\"}".into(),
                intent: None,
                result_preview: None,
                duplicate_of: Some(0),
                prior_failures: 0,
            },
        ];
        let started = std::time::Instant::now();
        let judged = judge_calls(
            &cfg,
            &json!({"goal": "fix the failing test"}),
            &items,
            JudgeScope::Both,
        );
        let elapsed = started.elapsed();
        println!("judged {} call(s) in {:?}", judged.len(), elapsed);
        assert_eq!(judged.len(), 3);
        assert!(
            judged.iter().any(|j| j.verification.raw().is_some()),
            "the failing build should get a verdict"
        );
        assert!(
            judged[2].redundant.raw().is_some(),
            "the duplicate has an answer to judge it"
        );
        assert!(
            judged.iter().all(|j| j.risk.raw().is_some()),
            "every call gets a risk band in the same request"
        );
    }
}

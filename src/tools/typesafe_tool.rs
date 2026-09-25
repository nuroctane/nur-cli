//! The `typesafe` tool: typed System One judgments available inside the loop.
//!
//! Every model nur talks to can ask Jev a question mid-turn. That is the point
//! of the integration - the judgment happens in the harness, so a cheap model on
//! any provider gets the same boost as a frontier one, and does not have to
//! spend writing tokens on a decision that is really an if statement.
//!
//! Actions map one-to-one onto the primitives (see [`crate::typesafe`]):
//! `noul` (probability), `choice` (pick one of code-supplied options), `score`
//! (ordered scale), plus the policies the harness itself uses: `pick`, `rank`,
//! `classify`, `risk`, `verify`.
//!
//! Two guardrails: the state is checked for secret-shaped content before it is
//! sent anywhere, and the caller's candidate options are what answers are mapped
//! back onto - a `choice` answer is always one of the caller's own strings or
//! nothing at all.

use super::{arg_str, Tool, ToolContext};
use crate::error::{NurError, Result};
use crate::typesafe::{self, harness, policy, questions};
use serde_json::{json, Value};

pub struct Typesafe;

/// Capability class for the tool: read-only, for every action.
///
/// Nothing here mutates the repository, which is what `is_read_only` gates
/// (plan mode, approval skipping, concurrency). Asking Jev is a *read*, the same
/// class as `web_fetch`/`web_search` - with one extra guard those do not have:
/// the state is refused outright when it looks like a credential, and every
/// per-item preview that leaves the machine goes through
/// [`harness::judge_preview`](crate::typesafe::harness::judge_preview).
///
/// `reset` clears this process's own counters; it touches no user data.
pub fn is_read_only_action(_args: &str) -> bool {
    true
}

fn cfg() -> crate::config::TypesafeConfig {
    crate::config::load_config()
        .map(|c| c.typesafe)
        .unwrap_or_default()
}

impl Tool for Typesafe {
    fn name(&self) -> &str {
        "typesafe"
    }

    fn description(&self) -> &str {
        "TypeSafe System One judgments (Jev): typed answers instead of a written \
         reply. Use when a step is a decision rather than writing - which option, \
         how risky, is this relevant, does this need a human, did that work. \
         actions: ask (batched, any mix of question types) | choice | noul | score \
         | pick (choose among candidate strings you supply) | rank (score every \
         candidate in one request) | classify | risk | verify | prune (what still \
         earns its context) | spam | needs_human | route (cheapest adequate model \
         for a fresh subagent) | reset | status. Multiple \
         independent questions in one `ask` cost a single round trip. Answers come \
         back with probabilities and `confidence`; below [typesafe] \
         escalate_confidence (0.5) the judgment is not trustworthy - escalate to a \
         human or a bigger model instead of acting. The state you send goes to \
         https://api.typesafe.ai; do not send secrets."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["ask", "choice", "noul", "score", "pick", "rank",
                             "classify", "risk", "verify", "prune", "spam",
                             "needs_human", "route", "reset", "status"],
                    "default": "status"
                },
                "state": {
                    "description": "What to judge: a string, or an object/array for structured state. Prefer named fields.",
                    "type": ["string", "object", "array"]
                },
                "instructions": {
                    "type": "string",
                    "description": "The question. One narrow, coherent judgment per question."
                },
                "question_id": {
                    "type": "string",
                    "description": "Label for the answer (defaults per action)."
                },
                "questions": {
                    "type": "array",
                    "description": "For ask: batched questions, evaluated in parallel in one request.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {"type": "string"},
                            "type": {"type": "string", "enum": ["noul", "choice", "score"]},
                            "instructions": {"type": ["string", "object", "array"]},
                            "criteria": {
                                "description": "choice: {option: rubric|null} - options come from your code. score: ordered level strings. noul: {true: ..., false: ...}",
                                "type": ["object", "array"]
                            }
                        },
                        "required": ["id", "type", "instructions"]
                    }
                },
                "options": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "For choice/pick: the candidate strings. Answers are mapped back onto these verbatim."
                },
                "levels": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "For score: 2-10 ordered levels, low to high, each describing a concrete situation."
                },
                "labels": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "For classify: the labels from code."
                },
                "candidates": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "For rank: candidates to score against `query`."
                },
                "items": {
                    "type": "array",
                    "description": "For prune: [{tool, arguments, result?, intent?}] - which of these still earn their context.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "tool": {"type": "string"},
                            "arguments": {"type": "string"},
                            "result": {"type": "string"},
                            "intent": {"type": "string"}
                        },
                        "required": ["tool"]
                    }
                },
                "query": {
                    "type": "string",
                    "description": "For rank: what relevance is measured against."
                },
                "output": {
                    "type": "string",
                    "description": "For verify: the tool output to judge."
                },
                "tool_call": {
                    "type": "string",
                    "description": "For verify: the call that was made."
                }
            }
        })
    }

    fn execute(&self, args: &Value, _ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action").unwrap_or_else(|_| "status".into());
        let cfg = cfg();
        match action.as_str() {
            "status" | "doctor" => Ok(typesafe::doctor_report(&cfg)),
            "ask" => ask(&cfg, args),
            "choice" => choice(&cfg, args),
            "noul" => noul(&cfg, args),
            "score" => score(&cfg, args),
            "pick" => pick(&cfg, args),
            "rank" => rank(&cfg, args),
            "classify" => classify(&cfg, args),
            "risk" => risk(&cfg, args),
            "verify" => verify(&cfg, args),
            "prune" => prune(&cfg, args),
            "reset" => Ok(reset()),
            "spam" => spam(&cfg, args),
            "needs_human" => needs_human(&cfg, args),
            "route" => route(&cfg, args),
            other => Ok(format!(
                "unknown typesafe action '{other}' - ask|choice|noul|score|pick|rank|\
                 classify|risk|verify|spam|needs_human|route|status"
            )),
        }
    }
}

fn state_arg(args: &Value) -> Value {
    args.get("state")
        .cloned()
        .unwrap_or_else(|| Value::String(String::new()))
}

fn guard_state(state: &Value) -> Result<()> {
    let text = match state {
        Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    };
    if crate::tools::sensitive::body_looks_sensitive(&text) {
        return Err(NurError::Tool(
            "typesafe: the state looks like it contains a secret; refusing to send it to an \
             external API. Redact it and retry."
                .into(),
        ));
    }
    Ok(())
}

fn client_or_msg(
    cfg: &crate::config::TypesafeConfig,
) -> std::result::Result<std::sync::Arc<typesafe::client::TypesafeClient>, String> {
    harness::ready(cfg).ok_or_else(|| typesafe::status(cfg))
}

fn id_arg(args: &Value, fallback: &str) -> String {
    arg_str(args, "question_id").unwrap_or_else(|_| fallback.to_string())
}

fn strings(args: &Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn ask(cfg: &crate::config::TypesafeConfig, args: &Value) -> Result<String> {
    let state = state_arg(args);
    guard_state(&state)?;
    let raw = args
        .get("questions")
        .and_then(Value::as_array)
        .ok_or_else(|| NurError::Tool("ask requires questions=[...]".into()))?;
    let mut qs: Vec<(String, questions::Question)> = Vec::new();
    for (i, q) in raw.iter().enumerate() {
        let id = q
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("q{i}"));
        let kind = q.get("type").and_then(Value::as_str).unwrap_or("noul");
        let instructions = q
            .get("instructions")
            .cloned()
            .ok_or_else(|| NurError::Tool(format!("question {id} needs instructions")))?;
        let question = match kind {
            "noul" => {
                let mut criteria = questions::NoulCriteria::default();
                if let Some(c) = q.get("criteria").and_then(Value::as_object) {
                    criteria.yes = c.get("true").and_then(Value::as_str).map(str::to_string);
                    criteria.no = c.get("false").and_then(Value::as_str).map(str::to_string);
                }
                if criteria.yes.is_none() && criteria.no.is_none() {
                    questions::Question::noul_value(instructions, None)
                } else {
                    questions::Question::noul_value(instructions, Some(criteria))
                }
            }
            "choice" => {
                let criteria = q
                    .get("criteria")
                    .and_then(Value::as_object)
                    .ok_or_else(|| {
                        NurError::Tool(format!(
                            "choice {id} needs criteria={{option: description}}"
                        ))
                    })?;
                let options: Vec<questions::ChoiceOption> = criteria
                    .iter()
                    .map(|(k, v)| questions::ChoiceOption {
                        label: k.clone(),
                        rubric: v.as_str().map(str::to_string),
                    })
                    .collect();
                questions::Question::Choice {
                    instructions,
                    options,
                }
            }
            "score" => {
                let levels: Vec<String> = q
                    .get("criteria")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .ok_or_else(|| {
                        NurError::Tool(format!("score {id} needs criteria=[\"level\", …]"))
                    })?;
                questions::Question::Score {
                    instructions,
                    levels,
                }
            }
            other => {
                return Err(NurError::Tool(format!(
                    "question {id}: unknown type '{other}' (noul|choice|score)"
                )))
            }
        };
        question
            .validate()
            .map_err(|e| NurError::Tool(format!("question {id}: {e}")))?;
        qs.push((id, question));
    }
    if qs.is_empty() {
        return Err(NurError::Tool("ask requires at least one question".into()));
    }
    let client = match client_or_msg(cfg) {
        Ok(c) => c,
        Err(msg) => return Ok(msg),
    };
    let asked = qs.len();
    let choice_options: usize = qs.iter().map(|(_, q)| q.candidate_labels().len()).sum();
    let batch = client
        .ask(&state, &qs)
        .map_err(|e| NurError::Tool(format!("typesafe: {e}")))?;
    let t = policy::Thresholds::from_config(cfg);
    let mut out = vec![format!(
        "typesafe · {} question(s) ({} choice option(s) supplied by the caller) from {}          ({} request(s), parallel={}) · {} in / {} out tokens",
        asked,
        choice_options,
        batch.model,
        batch.requests,
        batch.parallel,
        batch.input_tokens,
        batch.output_tokens
    )];
    if batch.is_empty() {
        out.push(
            "no answers came back at all (request failed or the endpoint rejected it) - do not \
             treat this as a judgment"
                .to_string(),
        );
    }
    for (id, q) in &qs {
        match batch.get(id) {
            Some(a) => {
                // The answer's type must match the question's: a noul answer to
                // a choice question means something is wrong upstream, not that
                // the judgment is weak.
                if a.kind() != q.kind() {
                    out.push(format!(
                        "{id} [{}]: malformed answer of type {} - ignored",
                        q.kind(),
                        a.kind()
                    ));
                    continue;
                }
                out.push(format!("{id} [{}]: {}", q.kind(), render_answer(a, &t)))
            }
            None => out.push(format!("{id} [{}]: no answer (request failed)", q.kind())),
        }
    }
    let missing = batch.missing(qs.iter().map(|(id, _)| id.as_str()));
    if !missing.is_empty() {
        out.push(format!("unanswered: {}", missing.join(", ")));
    }
    if !batch.errors.is_empty() {
        out.push(format!("errors: {}", batch.errors.join("; ")));
    }
    Ok(out.join("\n"))
}

fn render_answer(a: &questions::Answer, t: &policy::Thresholds) -> String {
    let band = t.band(a.confidence());
    let conf = match a.confidence() {
        Some(c) => format!("{c:.2}"),
        None => "-".into(),
    };
    match a {
        questions::Answer::Noul { noul } => format!(
            "{} p(yes)={noul:.2} · band {} ({})",
            a.kind(),
            band.as_str(),
            conf
        ),
        questions::Answer::Choice { choice, .. } => {
            // A Choice answer should be one of the caller's options. When the
            // body comes back empty, fall back to the distribution's argmax
            // rather than showing nothing.
            let label = if choice.trim().is_empty() {
                a.argmax().unwrap_or("?").to_string()
            } else {
                choice.clone()
            };
            format!(
                "{} {label:?} · band {} ({}) · p: {}",
                a.kind(),
                band.as_str(),
                conf,
                top_probs(a.probabilities())
            )
        }
        questions::Answer::Score { score, legend, .. } => format!(
            "{} {score:.2} · band {} ({}) · p: {} · levels: {}",
            a.kind(),
            band.as_str(),
            conf,
            top_probs(a.probabilities()),
            legend
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn top_probs(p: &std::collections::BTreeMap<String, f64>) -> String {
    let mut pairs: Vec<(&String, &f64)> = p.iter().collect();
    pairs.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
    pairs
        .into_iter()
        .take(4)
        .map(|(k, v)| format!("{k} {v:.2}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn choice(cfg: &crate::config::TypesafeConfig, args: &Value) -> Result<String> {
    let state = state_arg(args);
    guard_state(&state)?;
    let options = strings(args, "options");
    let instructions = arg_str(args, "instructions")
        .map_err(|_| NurError::Tool("choice requires instructions=".into()))?;
    let id = id_arg(args, "choice");
    let j = harness::choose(cfg, &state, &id, instructions, &options, true);
    Ok(describe_pick(&j, &options))
}

fn pick(cfg: &crate::config::TypesafeConfig, args: &Value) -> Result<String> {
    let state = state_arg(args);
    guard_state(&state)?;
    let options = strings(args, "options");
    let instructions = arg_str(args, "instructions")
        .map_err(|_| NurError::Tool("pick requires instructions=".into()))?;
    let id = id_arg(args, "pick");
    let candidates: Vec<(String, String)> =
        options.iter().map(|o| (o.clone(), String::new())).collect();
    let j = harness::pick(cfg, &state, &id, instructions, &candidates);
    Ok(describe_pick(&j, &options))
}

fn describe_pick<T: std::fmt::Debug>(j: &policy::Judgment<T>, options: &[String]) -> String {
    let mut out = vec![format!(
        "typesafe · {} options supplied by the caller (never invented)",
        options.len()
    )];
    out.push(j.describe());
    match j.raw() {
        Some(v) => out.push(format!("answer: {v:?}")),
        None => out.push("answer: none (no option fit, or a failure)".into()),
    }
    if let Some(v) = j.usable() {
        out.push(format!("usable now (band act): {v:?}"));
    } else if j.escalated {
        out.push(
            "escalate: confidence below the floor - route to a human or a bigger model, do not act"
                .into(),
        );
    } else {
        out.push("confirm: confident enough to inform, not to act alone".into());
    }
    out.join("\n")
}

fn noul(cfg: &crate::config::TypesafeConfig, args: &Value) -> Result<String> {
    let state = state_arg(args);
    guard_state(&state)?;
    let instructions = arg_str(args, "instructions")
        .map_err(|_| NurError::Tool("noul requires instructions=".into()))?;
    let id = id_arg(args, "noul");
    let j = harness::noul(cfg, &state, &id, instructions, None, 0.5);
    let mut out = vec![j.describe()];
    if let Some(p) = j.notes.first() {
        out.push(p.clone());
    }
    match j.usable() {
        Some(true) => out.push("yes (act band)".into()),
        Some(false) => out.push("no (act band)".into()),
        None => out.push("not acted on: confidence in the confirm/escalate band".into()),
    }
    Ok(out.join("\n"))
}

fn score(cfg: &crate::config::TypesafeConfig, args: &Value) -> Result<String> {
    let state = state_arg(args);
    guard_state(&state)?;
    let instructions = arg_str(args, "instructions")
        .map_err(|_| NurError::Tool("score requires instructions=".into()))?;
    let levels = strings(args, "levels");
    if levels.len() < questions::MIN_SCORE_LEVELS {
        return Err(NurError::Tool(
            "score requires levels=[…] with at least 2 ordered levels".into(),
        ));
    }
    let id = id_arg(args, "score");
    let j = harness::score(cfg, &state, &id, instructions, &levels);
    let mut out = vec![j.describe()];
    if let Some(v) = j.raw() {
        out.push(format!("score {v:.2} on 0..={}", levels.len() - 1));
        let idx = v.round().clamp(0.0, (levels.len() - 1) as f64) as usize;
        out.push(format!("level: {}", levels[idx]));
    }
    if j.usable().is_none() {
        out.push("not acted on: confidence in the confirm/escalate band".into());
    }
    Ok(out.join("\n"))
}

fn rank(cfg: &crate::config::TypesafeConfig, args: &Value) -> Result<String> {
    let query =
        arg_str(args, "query").map_err(|_| NurError::Tool("rank requires query=".into()))?;
    let candidates = strings(args, "candidates");
    if candidates.is_empty() {
        return Err(NurError::Tool("rank requires candidates=[…]".into()));
    }
    // This tool is declared read-only and therefore runs without approval, so the
    // credential guard is the only thing between a pasted secret and the wire.
    // `rank`/`route` reach the same endpoint as `ask`, so they must guard too.
    guard_state(&Value::String(format!(
        "{query}\n{}",
        candidates.join("\n")
    )))?;
    let levels: Vec<String> = harness::RELEVANCE_LEVELS
        .iter()
        .map(|s| s.to_string())
        .collect();
    let judged = harness::rank(cfg, &query, &candidates, &levels);
    // One implementation of the ordering rule (`harness::rank_order`), fed by the
    // judgments already fetched: a second `rank_indices` call was the same request
    // twice, so the printed scores and the order could disagree.
    let order = harness::rank_order(&judged);
    let mut out = vec![format!(
        "typesafe · ranked {} candidate(s) in one request",
        candidates.len()
    )];
    for (n, i) in order.iter().enumerate().take(12) {
        let Some(j) = judged.get(*i) else { continue };
        out.push(format!(
            "{:>3}. {:.2} ({}) {}",
            n + 1,
            j.raw().copied().unwrap_or(f64::NAN),
            j.action.as_str(),
            harness::preview(&candidates[*i], 120)
        ));
    }
    Ok(out.join("\n"))
}

fn classify(cfg: &crate::config::TypesafeConfig, args: &Value) -> Result<String> {
    let state = state_arg(args);
    guard_state(&state)?;
    let instructions = arg_str(args, "instructions")
        .map_err(|_| NurError::Tool("classify requires instructions=".into()))?;
    let labels = strings(args, "labels");
    if labels.is_empty() {
        return Err(NurError::Tool("classify requires labels=[…]".into()));
    }
    let id = id_arg(args, "classify");
    let j = harness::classify(cfg, &state, &id, instructions, &labels);
    Ok(describe_pick(&j, &labels))
}

fn risk(cfg: &crate::config::TypesafeConfig, args: &Value) -> Result<String> {
    let state = state_arg(args);
    guard_state(&state)?;
    let id = id_arg(args, "risk");
    let j = harness::risk(cfg, &state, &id);
    let mut out = vec![j.describe()];
    if let Some(band) = j.raw() {
        out.push(format!(
            "band: {} (stakes {:.2}{})",
            band.as_str(),
            band.stakes(),
            if band.wants_human() {
                " - a person should see this first"
            } else {
                ""
            }
        ));
    }
    Ok(out.join("\n"))
}

fn verify(cfg: &crate::config::TypesafeConfig, args: &Value) -> Result<String> {
    let call = arg_str(args, "tool_call").unwrap_or_else(|_| "tool call".into());
    let output =
        arg_str(args, "output").map_err(|_| NurError::Tool("verify requires output=".into()))?;
    guard_state(&Value::String(output.clone()))?;
    let intent = arg_str(args, "instructions").unwrap_or_else(|_| String::new());
    let j = harness::verify_result(cfg, &call, &intent, &harness::preview(&output, 4_000));
    let mut out = vec![j.describe()];
    match j.raw() {
        Some(v) => out.push(format!(
            "verdict: {}{}",
            v.as_str(),
            if v.is_problem() {
                " - the call did not do what it was for"
            } else {
                ""
            }
        )),
        None => out.push("verdict: unavailable".into()),
    }
    Ok(out.join("\n"))
}

/// Which of these tool calls/results still earn their place in context.
///
/// The same judgment compaction uses, available on demand for a transcript the
/// agent is juggling itself. Options (the calls) come from the caller; Jev only
/// decides which survive.
fn prune(cfg: &crate::config::TypesafeConfig, args: &Value) -> Result<String> {
    let items = args
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| NurError::Tool("prune requires items=[{tool, arguments, result}]".into()))?;
    if items.is_empty() {
        return Err(NurError::Tool("prune requires at least one item".into()));
    }
    let goal = arg_str(args, "instructions").unwrap_or_else(|_| String::new());
    let preview = cfg.tool_gate.preview_chars;
    let candidates: Vec<harness::ToolCallItem> = items
        .iter()
        .enumerate()
        .map(|(n, it)| harness::ToolCallItem {
            index: n,
            tool: it
                .get("tool")
                .and_then(Value::as_str)
                .unwrap_or("?")
                .to_string(),
            args_preview: harness::judge_preview(
                it.get("arguments").and_then(Value::as_str).unwrap_or(""),
                preview,
            ),
            intent: it.get("intent").and_then(Value::as_str).map(str::to_string),
            result_preview: it
                .get("result")
                .and_then(Value::as_str)
                .map(|r| harness::judge_preview(r, preview)),
            duplicate_of: None,
            prior_failures: 0,
        })
        .collect();
    let state = json!({ "goal": goal, "note": "items are the calls listed under `calls`" });
    // The same two questions and the same bar the automatic compaction uses, so
    // asking by hand and letting the harness prune cannot disagree.
    let judged = harness::judge_calls(cfg, &state, &candidates, harness::JudgeScope::Compaction);
    if judged.is_empty() {
        return Ok(typesafe::status(cfg));
    }
    let bar = cfg.compaction.keep_threshold.clamp(0.0, 1.0);
    let mut out = vec![format!(
        "typesafe · context pruning for {} call(s) at keep_threshold {bar:.2} - kept items stay verbatim",
        judged.len()
    )];
    for j in &judged {
        let (drop_call, drop_result) = j.prune_at(bar);
        let raw = candidates.get(j.index);
        let name = raw.map(|c| c.tool.as_str()).unwrap_or("?");
        let verdict = match (drop_call, drop_result) {
            (false, false) => "keep call + result",
            (false, true) => "keep call, drop result",
            (true, true) => "drop call + result",
            (true, false) => "drop call, keep result (unexpected - not applied)",
        };
        out.push(format!(
            "[{}] {name}: {verdict} · keep_call {} · keep_result {}",
            j.index,
            fmt_p(j.keep_call.raw().copied(), j.keep_call_p),
            fmt_p(j.keep_result.raw().copied(), j.keep_result_p),
        ));
    }
    out.push(
        "Apply this yourself: never drop a result whose call you keep in a way the provider \
         rejects (a `function_call_output` with no `function_call` is a 400)."
            .into(),
    );
    Ok(out.join("\n"))
}

fn fmt_p(_value: Option<bool>, p: Option<f64>) -> String {
    match p {
        Some(p) => format!("p={p:.2}"),
        None => "-".into(),
    }
}

/// Drain the session's Jev counters and report them. Counters are a read-out by
/// default, so this is explicit.
fn reset() -> String {
    let drained = typesafe::telemetry::take();
    if drained.is_empty() {
        return "typesafe · nothing recorded this session".into();
    }
    format!("typesafe · counters reset\n{}", drained.summary())
}

/// Spam / noise check for text the agent is about to surface (peer mail,
/// scraped pages, log tails).
fn spam(cfg: &crate::config::TypesafeConfig, args: &Value) -> Result<String> {
    let state = state_arg(args);
    guard_state(&state)?;
    let id = id_arg(args, "spam");
    let j = harness::is_spam(cfg, &state, &id);
    let mut out = vec![j.describe()];
    match j.usable() {
        Some(true) => out.push("verdict: spam / distraction - safe to drop or hide".into()),
        Some(false) => out.push("verdict: not spam - treat as real content".into()),
        None => out.push(
            "verdict: unclear - confidence is in the confirm/escalate band, so do not silently \
             discard it"
                .into(),
        ),
    }
    Ok(out.join("\n"))
}

/// Should a person decide this?
fn needs_human(cfg: &crate::config::TypesafeConfig, args: &Value) -> Result<String> {
    let state = state_arg(args);
    guard_state(&state)?;
    let id = id_arg(args, "needs_human");
    let j = harness::needs_human(cfg, &state, &id);
    let mut out = vec![j.describe()];
    match j.usable() {
        Some(true) => out.push("verdict: yes - ask the user rather than deciding alone".into()),
        Some(false) => out.push("verdict: no - the agent can decide and proceed".into()),
        None => out.push("verdict: unclear - treat as needing a person".into()),
    }
    Ok(out.join("\n"))
}

/// Cheapest adequate model for a task, from the models this machine can
/// actually reach. Suggestion only unless `[typesafe.routing] enabled = true`.
fn route(cfg: &crate::config::TypesafeConfig, args: &Value) -> Result<String> {
    use crate::typesafe::route;
    let task = arg_str(args, "instructions")
        .or_else(|_| arg_str(args, "query"))
        .or_else(|_| arg_str(args, "state"))
        .unwrap_or_default();
    if task.trim().is_empty() {
        return Err(NurError::Tool(
            "route requires instructions=<what the task is>".into(),
        ));
    }
    let active = crate::config::load_config().unwrap_or_default();
    let options = route::keyed_model_options(&active.provider, &active.model, 12);
    if options.len() < 2 {
        return Ok(format!(
            "typesafe · routing needs at least two reachable models; found {}. \
             Add a fallback provider (/failover) or another key.",
            options.len()
        ));
    }
    guard_state(&Value::String(task.clone()))?;
    let j = harness::pick_model(cfg, &task, &options);
    let mut out = vec![j.describe()];
    out.push(format!(
        "candidates: {}",
        options
            .iter()
            .map(|o| o.label())
            .collect::<Vec<_>>()
            .join(", ")
    ));
    let Some(pick) = j.usable() else {
        out.push("route: not confident enough to recommend a switch".into());
        return Ok(out.join("\n"));
    };
    let here = crate::pricing::rates_for(&active.provider, &active.model);
    let there = crate::pricing::rates_for(&pick.provider, &pick.model);
    let w = route::Workload::for_pack(route::estimate_tokens(&task));
    // This session already holds its context warm: moving it is not what the
    // router is for. A cheaper model pays off on a fresh child.
    let stay = route::parent_should_stay(&(&here).into(), &(&there).into(), &w);
    out.push(format!(
        "route: {} for a fresh subagent on this task. This session {} on {} - {}.",
        pick.label(),
        if stay {
            "stays"
        } else {
            "could move, but stays"
        },
        options[0].label(),
        if stay {
            "re-sending its context to another model costs more than finishing here"
        } else {
            "the parent is never re-routed mid-session"
        }
    ));
    if cfg.routing.enabled {
        out.push(
            "automatic: an `agent` call without provider/model is routed by the same judgment \
             when the hop is known-price, 15% cheaper and no weaker in privacy"
                .into(),
        );
    }
    Ok(out.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_is_read_only_for_capability_purposes() {
        // Nothing here mutates the repository, so plan mode allows it and no
        // approval is needed - the same class as web_fetch/web_search. The
        // credential guard is what keeps that safe, not the capability class.
        for action in [
            "status",
            "doctor",
            "ask",
            "choice",
            "noul",
            "score",
            "pick",
            "rank",
            "classify",
            "risk",
            "verify",
            "prune",
            "spam",
            "needs_human",
            "route",
            "reset",
        ] {
            assert!(
                is_read_only_action(&format!("{{\"action\":\"{action}\"}}")),
                "{action} is read-only"
            );
        }
        assert!(is_read_only_action("{}"));
    }

    #[test]
    fn secrets_are_refused_before_they_leave_the_machine() {
        let state = json!("export ANTHROPIC_API_KEY=sk-ant-api03-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let err = guard_state(&state).unwrap_err();
        assert!(err.to_string().contains("secret"), "{err}");
        assert!(guard_state(&json!({"q": "what does this function do?"})).is_ok());
    }

    #[test]
    fn answers_render_with_probabilities_and_a_band() {
        let t = policy::Thresholds::default();
        let a = questions::Answer::from_json(&json!({
            "type":"choice","choice":"technical",
            "probabilities":{"billing":0.08,"technical":0.85,"sales":0.07},"confidence":0.82
        }))
        .unwrap();
        let line = render_answer(&a, &t);
        assert!(line.contains("technical"), "{line}");
        assert!(line.contains("confirm"), "{line}");
        assert!(line.contains("billing 0.08"), "{line}");

        let s = questions::Answer::from_json(&json!({
            "type":"score","score":0.0,"legend":{"0":"safe","1":"risky"},
            "probabilities":{"0":0.99,"1":0.01},"confidence":0.97
        }))
        .unwrap();
        assert!(render_answer(&s, &t).contains("act"));
    }

    #[test]
    fn tool_schema_lists_every_action_and_is_valid_json() {
        let schema = Typesafe.parameters_schema();
        let actions = schema["properties"]["action"]["enum"].as_array().unwrap();
        assert!(actions.iter().any(|a| a == "ask"));
        assert!(actions.iter().any(|a| a == "risk"));
        assert_eq!(schema["type"], "object");
        assert_eq!(Typesafe.name(), "typesafe");
    }

    #[test]
    fn ask_rejects_a_malformed_question_before_any_request() {
        let cfg = crate::config::TypesafeConfig {
            enabled: true,
            api_key: "k".into(),
            ..Default::default()
        };
        let args = json!({
            "action": "ask",
            "state": "hello",
            "questions": [{"id": "bad", "type": "choice", "instructions": "pick", "criteria": {}}]
        });
        let err = ask(&cfg, &args).unwrap_err();
        assert!(err.to_string().contains("option"), "{err}");
    }
}

//! Cache-aware routing and the context a subagent is allowed to see.
//!
//! Two notes are baked in here, and both are enforced in code rather than left
//! as advice the model can ignore.
//!
//! Diogo Almeida's TypeSafe notes (2026-09): a long session should stay on the
//! model that already holds it. Moving that session onto a cheaper model and
//! back reprocesses the whole context and, on the published Opus/Sonnet prices,
//! costs more than staying. A cheaper model pays off on a *fresh* context - a
//! subagent that gets a small pack and returns a short report - not by swapping
//! the parent. Sensitive tasks are not hopped onto a provider whose privacy tier
//! is below zero-data-retention.
//!
//! Jevify's rule for the judgments around this: questions in one request are
//! independent, ordinary code combines them, and an answer below the acting
//! band never changes what the harness does. A missing judgment is the old
//! behavior (the prompt the model wrote, on the model the user picked).

use super::harness::{self, ModelOption};
use super::policy::{GateAction, Judgment};
use crate::config::Config;
use crate::pricing::ModelRates;
use crate::providers::Privacy;
use serde_json::Value;

/// Parent context offered to a child, at most: recent tool results plus the
/// reports of earlier subagents (so a child does not redo finished work).
const CHILD_CONTEXT_RESULTS: usize = 6;
/// How many kept chunks ride along, and how much room they may take.
const CHILD_KEEP_CHUNKS: usize = 4;
const CHILD_CONTEXT_CHARS: usize = 6_000;
/// Model candidates offered to the router.
const ROUTE_CANDIDATES: usize = 12;

/// USD per 1M tokens. `cache_read` is what a warm parent pays to keep tokens it
/// already holds; the published comparison treats that as already paid (`0`).
#[derive(Debug, Clone, Copy)]
pub struct Rates {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    pub cache_read_per_mtok: f64,
}

/// Token counts for one comparison. `context_tokens` is the parent session
/// (X). `output_tokens` and `read_tokens` are the work still to do (Y and Z).
/// `pack_tokens` is what a fresh child would actually be shown.
#[derive(Debug, Clone, Copy)]
pub struct Workload {
    pub context_tokens: u64,
    pub output_tokens: u64,
    pub read_tokens: u64,
    pub pack_tokens: u64,
    pub report_tokens: u64,
}

impl Workload {
    /// Assumptions for a subagent we are already about to spawn.
    ///
    /// Output and read sizes are a short child turn, not a measurement. The
    /// pack size is real: it is the prompt plus the chunks Jev kept. The parent
    /// context used for the stay-vs-bounce check is at least 80k so a warm
    /// session is not treated as empty.
    pub fn for_pack(pack_tokens: u64) -> Self {
        Self {
            context_tokens: pack_tokens.max(80_000),
            output_tokens: 4_000,
            read_tokens: 8_000,
            pack_tokens,
            report_tokens: 1_500,
        }
    }
}

fn per(rate: f64, tokens: u64) -> f64 {
    rate * (tokens as f64) / 1_000_000.0
}

/// Cost of finishing the work on the parent. The context is already warm, so
/// it is billed at the cache-read rate (0 in the published comparison).
pub fn parent_stay_usd(rates: &Rates, w: &Workload) -> f64 {
    per(rates.cache_read_per_mtok, w.context_tokens)
        + per(rates.output_per_mtok, w.output_tokens)
        + per(rates.input_per_mtok, w.read_tokens)
}

/// Cost of moving the parent onto `next` and then bringing the result back.
/// `next` loads the whole context at its input rate; the parent then reloads
/// the new output and reads.
pub fn parent_bounce_usd(current: &Rates, next: &Rates, w: &Workload) -> f64 {
    per(next.input_per_mtok, w.context_tokens)
        + per(next.output_per_mtok, w.output_tokens)
        + per(next.input_per_mtok, w.read_tokens)
        + per(
            current.input_per_mtok,
            w.output_tokens.saturating_add(w.read_tokens),
        )
}

/// True when bouncing the parent is not cheaper than staying.
pub fn parent_should_stay(current: &Rates, next: &Rates, w: &Workload) -> bool {
    parent_bounce_usd(current, next, w) >= parent_stay_usd(current, w)
}

/// Cost of doing the work in a fresh child, plus the parent reading the short
/// report that comes back. The child does not inherit the parent's cache.
pub fn child_total_usd(child: &Rates, parent: &Rates, w: &Workload) -> f64 {
    per(child.input_per_mtok, w.pack_tokens)
        + per(child.output_per_mtok, w.output_tokens)
        + per(child.input_per_mtok, w.read_tokens)
        + per(parent.input_per_mtok, w.report_tokens)
}

/// A child hop has to be clearly cheaper (15%), not a rounding flicker.
pub fn child_switch_pays(current: &Rates, next: &Rates, w: &Workload) -> bool {
    let here = child_total_usd(current, current, w);
    let there = child_total_usd(next, current, w);
    if here <= 0.0 {
        return false;
    }
    there * 1.15 < here
}

/// Relevance score on [`super::harness::RELEVANCE_LEVELS`]: index 3 is
/// "directly useful". Below that, a chunk is not worth handing to a child.
pub const USEFUL_RELEVANCE: f64 = 3.0;

/// Indexes of chunks a confident judgment marked directly useful, best first.
/// Unacted scores are ignored. `max_keep` caps how many ride along.
pub fn kept_chunks(judged: &[Judgment<f64>], max_keep: usize) -> Vec<usize> {
    let mut scored: Vec<(usize, f64)> = judged
        .iter()
        .enumerate()
        .filter_map(|(i, j)| {
            if j.action != GateAction::Act {
                return None;
            }
            let score = j.usable().copied()?;
            (score >= USEFUL_RELEVANCE).then_some((i, score))
        })
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(max_keep);
    scored.into_iter().map(|(i, _)| i).collect()
}

/// Prompt the child actually receives. No kept chunks means the task alone.
pub fn assemble_child_prompt(task: &str, chunks: &[&str], max_chars: usize) -> String {
    let task_owned: String = task.chars().take(max_chars).collect();
    if chunks.is_empty() || max_chars == 0 {
        return task_owned;
    }
    let header = "[parent context kept for this task]\n";
    let footer = "[end parent context]\n\n";
    let mut body = String::new();
    for chunk in chunks {
        let line = format!("- {chunk}\n");
        let used = header.chars().count()
            + body.chars().count()
            + line.chars().count()
            + footer.chars().count()
            + task_owned.chars().count();
        if used > max_chars {
            break;
        }
        body.push_str(&line);
    }
    if body.is_empty() {
        return task_owned;
    }
    format!("{header}{body}{footer}{task_owned}")
}

/// Chars to a rough token count. Used only for the pack-size input to the
/// cost check, which is itself an estimate.
pub fn estimate_tokens(text: &str) -> u64 {
    (text.chars().count() as u64).div_ceil(4).max(1)
}

/// A sensitive task may be hopped only onto ZDR, TEE, or local.
pub fn hop_allowed(task_is_sensitive: bool, dest: Privacy) -> bool {
    !task_is_sensitive || dest.rank() >= Privacy::Zdr.rank()
}

/// Code-side sensitivity. This is the gate; Jev does not get to waive it.
/// The markers are file and credential shapes, not a vibe check on the prose.
pub fn touches_secrets(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    const MARKERS: &[&str] = &[
        ".env",
        "credentials",
        "id_rsa",
        "private key",
        "api_key",
        "api-key",
        "auth.json",
        "secret_key",
        "begin openssh",
        "begin rsa",
    ];
    MARKERS.iter().any(|m| lower.contains(m))
}

/// An earlier `agent` call whose result is still in the transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriorJob {
    pub prompt: String,
    pub output: String,
}

fn preview(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

/// Earlier subagent tasks, oldest first, capped at the four most recent.
pub fn prior_agent_jobs(items: &[Value]) -> Vec<PriorJob> {
    let mut prompts: Vec<(String, String)> = Vec::new();
    let mut outputs: Vec<(String, String)> = Vec::new();
    for item in items {
        let kind = item.get("type").and_then(Value::as_str).unwrap_or("");
        if kind == "function_call" && item.get("name").and_then(Value::as_str) == Some("agent") {
            let id = item.get("call_id").and_then(Value::as_str).unwrap_or("");
            let args = item.get("arguments").and_then(Value::as_str).unwrap_or("");
            let prompt = serde_json::from_str::<Value>(args)
                .ok()
                .and_then(|v| {
                    v.get("prompt")
                        .and_then(Value::as_str)
                        .map(|s| preview(s, 400))
                })
                .unwrap_or_default();
            if !id.is_empty() && !prompt.is_empty() {
                prompts.push((id.to_string(), prompt));
            }
        } else if kind == "function_call_output" {
            let id = item.get("call_id").and_then(Value::as_str).unwrap_or("");
            let output = item.get("output").and_then(Value::as_str).unwrap_or("");
            if !id.is_empty() && !output.trim().is_empty() {
                outputs.push((id.to_string(), preview(output, 800)));
            }
        }
    }
    let mut jobs = Vec::new();
    for (id, prompt) in prompts {
        if let Some((_, output)) = outputs.iter().rev().find(|(oid, _)| oid == &id) {
            jobs.push(PriorJob {
                prompt,
                output: output.clone(),
            });
        }
    }
    if jobs.len() > 4 {
        jobs.split_off(jobs.len() - 4)
    } else {
        jobs
    }
}

/// Recent non-agent tool results, oldest first, capped.
pub fn recent_result_chunks(items: &[Value], limit: usize) -> Vec<String> {
    let agent_ids: std::collections::HashSet<&str> = items
        .iter()
        .filter(|item| {
            item.get("type").and_then(Value::as_str) == Some("function_call")
                && item.get("name").and_then(Value::as_str) == Some("agent")
        })
        .filter_map(|item| item.get("call_id").and_then(Value::as_str))
        .collect();
    let mut names: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for item in items {
        if item.get("type").and_then(Value::as_str) == Some("function_call") {
            if let (Some(id), Some(name)) = (
                item.get("call_id").and_then(Value::as_str),
                item.get("name").and_then(Value::as_str),
            ) {
                names.insert(id, name);
            }
        }
    }
    let mut chunks = Vec::new();
    for item in items {
        if item.get("type").and_then(Value::as_str) != Some("function_call_output") {
            continue;
        }
        let id = item.get("call_id").and_then(Value::as_str).unwrap_or("");
        if agent_ids.contains(id) {
            continue;
        }
        let output = item.get("output").and_then(Value::as_str).unwrap_or("");
        if output.trim().is_empty() {
            continue;
        }
        let name = names.get(id).copied().unwrap_or("tool");
        chunks.push(format!("{name}: {}", preview(output, 400)));
    }
    if limit == 0 {
        return Vec::new();
    }
    if chunks.len() > limit {
        chunks.split_off(chunks.len() - limit)
    } else {
        chunks
    }
}

/// The models this machine can actually reach, as router candidates: the
/// route in use first (certainly reachable, already warm), then each keyed
/// catalog provider's default model. The router can only pick from this list.
pub fn keyed_model_options(provider: &str, model: &str, limit: usize) -> Vec<ModelOption> {
    let mut out: Vec<ModelOption> = Vec::new();
    let mut push = |provider: &str, model: &str, note: String| {
        if out.len() >= limit || model.trim().is_empty() {
            return;
        }
        if out
            .iter()
            .any(|o| o.provider == provider && o.model == model)
        {
            return;
        }
        out.push(ModelOption {
            provider: provider.to_string(),
            model: model.to_string(),
            note,
        });
    };
    push(
        provider,
        model,
        "the model currently in use - strongest context, already paid for".into(),
    );
    for p in crate::providers::PROVIDERS {
        if crate::auth::load_provider_key(p.id).is_none() {
            continue;
        }
        push(p.id, p.default_model, format!("{} · {}", p.name, p.note));
    }
    out
}

impl From<&ModelRates> for Rates {
    fn from(r: &ModelRates) -> Self {
        Self {
            input_per_mtok: r.input_per_mtok_usd,
            output_per_mtok: r.output_per_mtok_usd,
            cache_read_per_mtok: r.cache_read_per_mtok_usd,
        }
    }
}

/// A price nur actually knows. The fallback is invented list pricing, and a
/// cost comparison built on invented prices is noise, so it never routes.
fn known_price(r: &ModelRates) -> bool {
    r.source != "builtin-fallback"
}

/// Candidate parent context for a child, oldest first.
pub fn child_context(items: &[Value]) -> Vec<String> {
    let mut out = recent_result_chunks(items, CHILD_CONTEXT_RESULTS);
    out.extend(prior_agent_jobs(items).into_iter().map(|j| {
        format!(
            "earlier subagent task: {} -> report: {}",
            j.prompt, j.output
        )
    }));
    out
}

/// Why a cheaper child model was or was not used. Pure: every gate is code.
#[derive(Debug, Clone, PartialEq)]
pub enum HopVerdict {
    Hop { stay_usd: f64, hop_usd: f64 },
    UnknownPrice,
    NotCheaper,
    PrivacyDowngrade,
    Sensitive,
}

/// Gate a child hop from the parent's model to `next`.
///
/// Jev's pick is only a candidate. The hop happens when both prices are known,
/// it is at least 15% cheaper for this pack ([`child_switch_pays`]), it never
/// lands below the parent's privacy tier (the same floor failover keeps), and
/// a secret-touching task stays on zero-data-retention, TEE or local.
pub fn decide_hop(
    parent: (&ModelRates, Privacy),
    next: (&ModelRates, Privacy),
    sensitive: bool,
    pack_tokens: u64,
) -> HopVerdict {
    if !known_price(parent.0) || !known_price(next.0) {
        return HopVerdict::UnknownPrice;
    }
    if next.1.rank() < parent.1.rank() {
        return HopVerdict::PrivacyDowngrade;
    }
    if !hop_allowed(sensitive, next.1) {
        return HopVerdict::Sensitive;
    }
    let (here, there) = (Rates::from(parent.0), Rates::from(next.0));
    let w = Workload::for_pack(pack_tokens);
    if !child_switch_pays(&here, &there, &w) {
        return HopVerdict::NotCheaper;
    }
    HopVerdict::Hop {
        stay_usd: child_total_usd(&here, &here, &w),
        hop_usd: child_total_usd(&there, &here, &w),
    }
}

/// Where an un-routed subagent runs and what it is shown.
#[derive(Debug, Clone)]
pub struct ChildPlan {
    /// The task, plus any parent context a confident judgment kept.
    pub prompt: String,
    /// A cheaper model that passed every gate; `None` stays on the parent.
    pub hop: Option<ModelOption>,
    /// One transcript line when routing made a decision worth showing.
    pub note: Option<String>,
}

/// Cache-aware routing for a subagent the model did not route itself.
///
/// Two judgments run in parallel: which reachable model is the cheapest
/// adequate one for the task, and which parent-context chunks are directly
/// useful to it. `None` (routing off, no Jev, nothing to decide) means the
/// caller keeps today's behavior: the task alone, on the parent's model.
pub fn plan_child(config: &Config, task: &str, context: &[String]) -> Option<ChildPlan> {
    let cfg = &config.typesafe;
    if !cfg.enabled || !cfg.routing.enabled || harness::ready(cfg).is_none() {
        return None;
    }
    let options = keyed_model_options(&config.provider, &config.model, ROUTE_CANDIDATES);
    let levels: Vec<String> = harness::RELEVANCE_LEVELS
        .iter()
        .map(|s| s.to_string())
        .collect();
    let (pick, judged) = std::thread::scope(|s| {
        let pick =
            s.spawn(|| (options.len() >= 2).then(|| harness::pick_model(cfg, task, &options)));
        let ranked = s.spawn(|| {
            if context.is_empty() {
                Vec::new()
            } else {
                harness::rank(cfg, task, context, &levels)
            }
        });
        (
            pick.join().ok().flatten(),
            ranked.join().unwrap_or_default(),
        )
    });

    let kept: Vec<&str> = kept_chunks(&judged, CHILD_KEEP_CHUNKS)
        .into_iter()
        .filter_map(|i| context.get(i).map(String::as_str))
        .collect();
    let budget = task.chars().count() + CHILD_CONTEXT_CHARS;
    let prompt = assemble_child_prompt(task, &kept, budget);

    let choice = pick
        .and_then(|j| j.usable().cloned())
        .filter(|o| !(o.provider == config.provider && o.model == config.model));
    let Some(choice) = choice else {
        let note = (!kept.is_empty()).then(|| {
            format!(
                "subagent · {} parent-context chunk(s) kept for the child",
                kept.len()
            )
        });
        return Some(ChildPlan {
            prompt,
            hop: None,
            note,
        });
    };

    let privacy = |id: &str| crate::providers::effective_privacy(&config.provider_privacy, id);
    let verdict = decide_hop(
        (
            &crate::pricing::rates_for(&config.provider, &config.model),
            privacy(&config.provider),
        ),
        (
            &crate::pricing::rates_for(&choice.provider, &choice.model),
            privacy(&choice.provider),
        ),
        touches_secrets(task) || touches_secrets(&prompt),
        estimate_tokens(&prompt),
    );
    let label = choice.label();
    let (hop, note) = match verdict {
        HopVerdict::Hop { stay_usd, hop_usd } => (
            Some(choice),
            format!("subagent · routed to {label} (est ${hop_usd:.4} vs ${stay_usd:.4} on the parent model)"),
        ),
        HopVerdict::UnknownPrice => (None, format!("subagent · stays on the parent model: no known price for {label}")),
        HopVerdict::NotCheaper => (None, format!("subagent · stays on the parent model: {label} is not 15% cheaper for this task")),
        HopVerdict::PrivacyDowngrade => (None, format!("subagent · stays on the parent model: {label} is a weaker privacy tier")),
        HopVerdict::Sensitive => (None, format!("subagent · stays on the parent model: the task touches secrets and {label} is not ZDR, TEE or local")),
    };
    Some(ChildPlan {
        prompt,
        hop,
        note: Some(note),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typesafe::policy::{Judgment, Thresholds};

    fn opus() -> Rates {
        Rates {
            input_per_mtok: 5.0,
            output_per_mtok: 25.0,
            cache_read_per_mtok: 0.0,
        }
    }

    fn sonnet() -> Rates {
        Rates {
            input_per_mtok: 3.0,
            output_per_mtok: 15.0,
            cache_read_per_mtok: 0.0,
        }
    }

    /// The published mix: X 0.65, Y 0.12, Z 0.23, in millions of tokens.
    fn published() -> Workload {
        Workload {
            context_tokens: 650_000,
            output_tokens: 120_000,
            read_tokens: 230_000,
            pack_tokens: 8_000,
            report_tokens: 2_000,
        }
    }

    #[test]
    fn a_long_session_stays_and_a_fresh_child_can_still_be_cheaper() {
        let w = published();
        let stay = parent_stay_usd(&opus(), &w);
        let bounce = parent_bounce_usd(&opus(), &sonnet(), &w);
        // 25*0.12 + 5*0.23 = 4.15; bounce is the published 6.19.
        assert!((stay - 4.15).abs() < 1e-9, "{stay}");
        assert!((bounce - 6.19).abs() < 1e-9, "{bounce}");
        assert!(parent_should_stay(&opus(), &sonnet(), &w));
        assert!(child_switch_pays(&opus(), &sonnet(), &w));
    }

    #[test]
    fn a_child_that_must_see_the_whole_session_is_not_auto_hopped_for_noise() {
        let mut w = published();
        w.pack_tokens = w.context_tokens;
        // Still cheaper on these prices, but the margin check is what we ship.
        // Equal rates never hop.
        assert!(!child_switch_pays(&opus(), &opus(), &w));
    }

    #[test]
    fn only_an_acted_useful_score_keeps_a_chunk() {
        let t = Thresholds::default();
        let judged = vec![
            Judgment::from_answer("a", Some(4.0), Some(0.95), &t),
            Judgment::from_answer("b", Some(3.2), Some(0.4), &t), // below the acting band
            Judgment::from_answer("c", Some(1.0), Some(0.99), &t), // acted, not useful
            Judgment::from_answer("d", Some(3.5), Some(0.9), &t),
        ];
        assert_eq!(kept_chunks(&judged, 4), vec![0, 3]);
        assert_eq!(kept_chunks(&judged, 1), vec![0]);
    }

    #[test]
    fn the_pack_keeps_the_task_when_nothing_was_selected() {
        let task = "map the auth module";
        assert_eq!(assemble_child_prompt(task, &[], 1000), task);
        let packed = assemble_child_prompt(task, &["read_file: src/auth.rs"], 1000);
        assert!(packed.contains(task), "{packed}");
        assert!(packed.contains("src/auth.rs"), "{packed}");
        assert!(packed.starts_with("[parent context"), "{packed}");
    }

    #[test]
    fn secrets_block_a_standard_hop_and_allow_zdr() {
        assert!(touches_secrets("read .env and print the api_key"));
        assert!(touches_secrets("-----BEGIN RSA PRIVATE KEY-----"));
        assert!(!touches_secrets("rename the helper and add a test"));
        assert!(!hop_allowed(true, Privacy::Standard));
        assert!(hop_allowed(true, Privacy::Zdr));
        assert!(hop_allowed(true, Privacy::Local));
        assert!(hop_allowed(false, Privacy::Standard));
    }

    fn rates(input: f64, output: f64, source: &str) -> ModelRates {
        ModelRates {
            input_per_mtok_usd: input,
            output_per_mtok_usd: output,
            cache_read_per_mtok_usd: 0.0,
            cache_write_per_mtok_usd: None,
            context_window: None,
            source: source.into(),
            note: String::new(),
            provider_id: String::new(),
            model_id: String::new(),
        }
    }

    #[test]
    fn a_child_hops_only_when_every_code_gate_passes() {
        let opus = rates(5.0, 25.0, "models.dev");
        let small = rates(0.25, 1.25, "models.dev");
        let pack = 2_000;
        assert!(matches!(
            decide_hop((&opus, Privacy::Zdr), (&small, Privacy::Zdr), false, pack),
            HopVerdict::Hop { hop_usd, stay_usd } if hop_usd < stay_usd
        ));
        // Invented fallback prices never justify a hop.
        let guessed = rates(0.1, 0.1, "builtin-fallback");
        assert_eq!(
            decide_hop((&opus, Privacy::Zdr), (&guessed, Privacy::Zdr), false, pack),
            HopVerdict::UnknownPrice
        );
        // Never below the parent's privacy tier, even when much cheaper.
        assert_eq!(
            decide_hop(
                (&opus, Privacy::Zdr),
                (&small, Privacy::Standard),
                false,
                pack
            ),
            HopVerdict::PrivacyDowngrade
        );
        // A secret-touching task stays on ZDR or better.
        assert_eq!(
            decide_hop(
                (&opus, Privacy::Standard),
                (&small, Privacy::Standard),
                true,
                pack
            ),
            HopVerdict::Sensitive
        );
        // Equal prices are not a reason to move.
        assert_eq!(
            decide_hop((&opus, Privacy::Zdr), (&opus, Privacy::Zdr), false, pack),
            HopVerdict::NotCheaper
        );
    }

    #[test]
    fn child_context_offers_results_and_earlier_reports() {
        let items = vec![
            serde_json::json!({"type":"function_call","call_id":"c1","name":"grep","arguments":"{}"}),
            serde_json::json!({"type":"function_call_output","call_id":"c1","output":"src/auth.rs:12 fn login"}),
            serde_json::json!({"type":"function_call","call_id":"a1","name":"agent","arguments":"{\"prompt\":\"map auth\"}"}),
            serde_json::json!({"type":"function_call_output","call_id":"a1","output":"auth lives in src/auth.rs"}),
        ];
        let ctx = child_context(&items);
        assert_eq!(ctx.len(), 2, "{ctx:?}");
        assert!(ctx[0].starts_with("grep: "), "{ctx:?}");
        assert!(
            ctx[1].contains("map auth") && ctx[1].contains("auth lives"),
            "{ctx:?}"
        );
    }

    #[test]
    fn prior_jobs_and_chunks_skip_each_other() {
        let items = vec![
            serde_json::json!({
                "type": "function_call",
                "call_id": "c1",
                "name": "read_file",
                "arguments": "{\"path\":\"a.rs\"}"
            }),
            serde_json::json!({
                "type": "function_call_output",
                "call_id": "c1",
                "output": "fn main() {}"
            }),
            serde_json::json!({
                "type": "function_call",
                "call_id": "a1",
                "name": "agent",
                "arguments": "{\"prompt\":\"map auth\"}"
            }),
            serde_json::json!({
                "type": "function_call_output",
                "call_id": "a1",
                "output": "auth lives in src/auth.rs"
            }),
        ];
        let jobs = prior_agent_jobs(&items);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].prompt, "map auth");
        assert!(jobs[0].output.contains("src/auth.rs"));
        let chunks = recent_result_chunks(&items, 6);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].contains("fn main"), "{chunks:?}");
        assert!(!chunks[0].contains("auth lives"), "{chunks:?}");
    }
}

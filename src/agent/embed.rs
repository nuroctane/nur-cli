//! Real embeddings for nur memory.
//!
//! Embedding is pluggable:
//! - **API path**: POST `<active base_url>/embeddings` with the provider's key,
//!   using a configurable embedding model (`nur.memory_embed_model`, defaults to
//!   a provider-sensible OpenAI-compatible id). Produces genuine semantic vectors.
//! - **Honest local fallback**: `embed_local()` returns a fixed-dimension feature
//!   vector from character n-grams + hashing. This is a **bag-of-n-grams
//!   embedding, NOT a semantic/neural one** — but it is dimensionally valid for
//!   cosine similarity and works fully offline. It is labeled honestly.
//!
//! Every embedding is L2-normalized so cosine similarity == dot product, and the
//! dimension is fixed (EMBED_DIM) so vectors from either path are comparable
//! within a scope (we only ever mix in the fallback when API is unavailable).

use crate::config::{atomic_write, nur_home};
use crate::error::{NurError, Result};
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// Fixed vector dimension for local embeddings. API embeddings are resized
/// (truncate/pad) to this when stored so cosine is always comparable.
pub const EMBED_DIM: usize = 256;

/// Default OpenAI-compatible embedding model id.
pub const DEFAULT_EMBED_MODEL: &str = "text-embedding-3-small";

/// A receipt for an embedding request. It records no text, only the route,
/// model and estimable accounting inputs needed by the session ledger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingTelemetry {
    pub route: String,
    pub model: String,
    pub input_chars: usize,
    pub input_tokens_estimate: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens_reported: Option<u64>,
    pub latency_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd_estimate: Option<f64>,
    pub cost_provenance: String,
    /// local | remote | unknown. This is a processing/privacy label, not a
    /// provider marketing claim.
    pub processing: String,
    pub outcome: String,
}

fn telemetry_queue() -> &'static Mutex<Vec<EmbeddingTelemetry>> {
    static QUEUE: OnceLock<Mutex<Vec<EmbeddingTelemetry>>> = OnceLock::new();
    QUEUE.get_or_init(|| Mutex::new(Vec::new()))
}

fn telemetry_path() -> std::path::PathBuf {
    nur_home().join("telemetry").join("embeddings.ndjson")
}

fn estimate_tokens(text: &str) -> u64 {
    // Conservative, model-agnostic approximation for a receipt when the API
    // omits usage. It intentionally does not inspect or persist content.
    (text.chars().count() as u64).div_ceil(4)
}

fn embedding_cost(model: &str, input_tokens: u64) -> Option<f64> {
    // Public list-price fallback. Unknown models remain explicitly unknown,
    // rather than inheriting an unrelated provider/chat rate.
    let per_million = match model {
        "text-embedding-3-small" => 0.02,
        "text-embedding-3-large" => 0.13,
        "text-embedding-ada-002" => 0.10,
        _ => return None,
    };
    Some(input_tokens as f64 * per_million / 1_000_000.0)
}

fn record_telemetry(event: EmbeddingTelemetry) {
    // The append is best effort and deliberately does not turn a successful
    // local/vector write into a failure. Keep a bounded on-disk queue so a
    // later session-ledger integration can import background events.
    let Ok(mut queue) = telemetry_queue().lock() else {
        return;
    };
    queue.push(event.clone());
    let path = telemetry_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut existing = std::fs::read_to_string(&path).unwrap_or_default();
    if let Ok(line) = serde_json::to_string(&event) {
        existing.push_str(&line);
        existing.push('\n');
        let lines: Vec<&str> = existing.lines().rev().take(1_000).collect();
        let bounded = lines.into_iter().rev().collect::<Vec<_>>().join("\n");
        let _ = atomic_write(&path, format!("{bounded}\n").as_bytes());
    }
}

/// Drain in-process telemetry for a ledger owner. Persistent history is kept
/// separately for process exits/background upgrades.
pub fn take_embedding_telemetry() -> Vec<EmbeddingTelemetry> {
    telemetry_queue()
        .lock()
        .map(|mut q| std::mem::take(&mut *q))
        .unwrap_or_default()
}

/// Read the bounded persistent queue without consuming it, for status/receipt
/// views or a startup importer.
#[allow(dead_code)]
pub fn persisted_embedding_telemetry() -> Vec<EmbeddingTelemetry> {
    std::fs::read_to_string(telemetry_path())
        .ok()
        .into_iter()
        .flat_map(|body| body.lines().map(str::to_owned).collect::<Vec<_>>())
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect()
}

/// A session with explicit cost or token ceilings is budget-sensitive. The env
/// override also lets callers select local-first before config schema changes.
pub fn budget_sensitive() -> bool {
    if matches!(
        std::env::var("NUR_MEMORY_EMBED_LOCAL_FIRST").as_deref(),
        Ok("1" | "true" | "yes")
    ) {
        return true;
    }
    crate::config::load_config()
        .map(|c| c.max_session_cost_usd.is_some() || c.max_session_tokens.is_some())
        .unwrap_or(false)
}

fn configured_mode() -> String {
    crate::config::load_config()
        .map(|c| c.memory_embed_mode.trim().to_ascii_lowercase())
        .unwrap_or_else(|_| "auto".to_string())
}

/// Whether auto-mode indexing may be upgraded remotely in a background task.
pub fn remote_upgrade_enabled() -> bool {
    configured_mode() == "auto" && !budget_sensitive() && embedding_api_details().is_ok()
}

/// Embed `text` to an L2-normalized vector of length EMBED_DIM.
///
/// Mode from `nur.memory_embed_mode`:
/// - `api`: require the API; on error, still return a local vector (we never
///   store a silent zero) but expose the source so the caller can report it.
/// - `local`: always the honest offline n-gram hash embedding.
/// - `auto` (default): API, then local fallback.
pub fn embed(text: &str) -> Vec<f32> {
    let mode = configured_mode();
    match mode.as_str() {
        "local" => embed_local_observed(text, "configured-local"),
        _ if budget_sensitive() && mode != "api" => {
            embed_local_observed(text, "budget-local-first")
        }
        _ => embed_api(text).unwrap_or_else(|_| embed_local_observed(text, "remote-fallback")),
    }
}

/// Embed and report which source produced the vector (for honesty/telemetry).
pub fn embed_with_source(text: &str) -> (Vec<f32>, &'static str) {
    let mode = configured_mode();
    if mode == "local" {
        return (embed_local_observed(text, "configured-local"), "local");
    }
    if budget_sensitive() && mode != "api" {
        return (
            embed_local_observed(text, "budget-local-first"),
            "local-budget",
        );
    }
    match embed_api(text) {
        Ok(v) => (v, "api"),
        Err(_) => (embed_local_observed(text, "remote-fallback"), "local"),
    }
}

fn embed_local_observed(text: &str, outcome: &str) -> Vec<f32> {
    let started = Instant::now();
    let out = embed_local(text);
    record_telemetry(EmbeddingTelemetry {
        route: "local:ngram-hash".into(),
        model: "nur-local-ngram-256".into(),
        input_chars: text.chars().count(),
        input_tokens_estimate: estimate_tokens(text),
        input_tokens_reported: None,
        latency_ms: started.elapsed().as_millis() as u64,
        cost_usd_estimate: Some(0.0),
        cost_provenance: "local-zero-cost".into(),
        processing: "local".into(),
        outcome: outcome.into(),
    });
    out
}

/// Local embedding with a receipt for local-first indexing paths.
pub fn embed_local_with_telemetry(text: &str, outcome: &str) -> Vec<f32> {
    embed_local_observed(text, outcome)
}

#[derive(Debug, Clone)]
struct ApiDetails {
    key: String,
    model: String,
    url: String,
    route: String,
}

fn embedding_api_details() -> Result<ApiDetails> {
    let cfg = crate::config::load_config().unwrap_or_default();
    let key = crate::auth::resolve_api_key_for(Some(cfg.provider.as_str())).unwrap_or_default();
    if key.trim().is_empty() {
        return Err(NurError::Other("no provider key for embeddings".into()));
    }
    let model = if cfg.memory_embed_model.trim().is_empty() {
        DEFAULT_EMBED_MODEL.to_string()
    } else {
        cfg.memory_embed_model
    };
    Ok(ApiDetails {
        key,
        model,
        url: format!("{}/embeddings", cfg.base_url.trim_end_matches('/')),
        route: format!("remote:{}", cfg.provider),
    })
}

fn record_remote_failure(
    details: &ApiDetails,
    text: &str,
    latency_ms: u64,
    outcome: &str,
    estimated_cost: bool,
) {
    let tokens = estimate_tokens(text);
    let cost = estimated_cost
        .then(|| embedding_cost(&details.model, tokens))
        .flatten();
    record_telemetry(EmbeddingTelemetry {
        route: details.route.clone(),
        model: details.model.clone(),
        input_chars: text.chars().count(),
        input_tokens_estimate: tokens,
        input_tokens_reported: None,
        latency_ms,
        cost_usd_estimate: cost,
        cost_provenance: if cost.is_some() {
            "input-estimate+model-rate"
        } else {
            "unknown"
        }
        .into(),
        processing: "remote".into(),
        outcome: outcome.into(),
    });
}

/// API embeddings call (OpenAI-compatible `<base>/embeddings`).
/// Best-effort; errors mean callers fall back to local.
pub fn embed_api(text: &str) -> Result<Vec<f32>> {
    let details = embedding_api_details()?;
    let started = Instant::now();
    let body = serde_json::json!({ "model": details.model, "input": [text] });
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| NurError::Other(format!("embed client: {e}")))?;
    let resp = client
        .post(&details.url)
        .bearer_auth(&details.key)
        .json(&body)
        .send()
        .map_err(|e| {
            record_remote_failure(
                &details,
                text,
                started.elapsed().as_millis() as u64,
                "transport-error",
                true,
            );
            NurError::Other(format!("embed send: {e}"))
        })?;
    if !resp.status().is_success() {
        record_remote_failure(
            &details,
            text,
            started.elapsed().as_millis() as u64,
            &format!("http-{}", resp.status().as_u16()),
            false,
        );
        return Err(NurError::Other(format!("embed status {}", resp.status())));
    }
    let json: serde_json::Value = resp
        .json()
        .map_err(|e| NurError::Other(format!("embed parse: {e}")))?;
    let reported_tokens = json["usage"]["prompt_tokens"].as_u64();
    let vec = json["data"][0]["embedding"]
        .as_array()
        .ok_or_else(|| NurError::Other("embedding missing".into()))?
        .iter()
        .filter_map(|v| v.as_f64())
        .map(|x| x as f32)
        .collect::<Vec<f32>>();
    if vec.is_empty() {
        return Err(NurError::Other("empty embedding".into()));
    }
    let input_tokens = reported_tokens.unwrap_or_else(|| estimate_tokens(text));
    let cost = embedding_cost(&details.model, input_tokens);
    record_telemetry(EmbeddingTelemetry {
        route: details.route,
        model: details.model,
        input_chars: text.chars().count(),
        input_tokens_estimate: estimate_tokens(text),
        input_tokens_reported: reported_tokens,
        latency_ms: started.elapsed().as_millis() as u64,
        cost_usd_estimate: cost,
        cost_provenance: if cost.is_some() {
            if reported_tokens.is_some() {
                "provider-usage+model-rate"
            } else {
                "input-estimate+model-rate"
            }
            .into()
        } else {
            "unknown".into()
        },
        processing: "remote".into(),
        outcome: "ok".into(),
    });
    // Normalize to EMBED_DIM (truncate or zero-pad) and L2-normalize.
    Ok(normalize_dim(&vec))
}

/// Honest local embedding: bag of character n-grams hashed into EMBED_DIM bins,
/// L2-normalized. NOT semantic — no neural/transformer model. Valid for cosine on
/// surface (form) similarity and fully offline.
pub fn embed_local(text: &str) -> Vec<f32> {
    let mut v = vec![0.0f32; EMBED_DIM];
    let t = text.to_ascii_lowercase();
    let chars: Vec<char> = t.chars().collect();
    let n = t.chars().count();
    // Contributing grams: characters and bigrams (unigrams + position-ish).
    for (i, c) in chars.iter().enumerate() {
        hash_into(c, i, n, &mut v);
        if i + 1 < chars.len() {
            let mut gram = [0u16; 2];
            gram[0] = *c as u16;
            gram[1] = chars[i + 1] as u16;
            hash_pair(gram[0], gram[1], &mut v);
        }
    }
    normalize(&mut v);
    v
}

fn hash_into(c: &char, idx: usize, _n: usize, v: &mut Vec<f32>) {
    let mut seed = (*c as u64).wrapping_add((idx as u64).wrapping_mul(2654435761));
    seed ^= seed >> 21;
    let bin = (seed % EMBED_DIM as u64) as usize;
    v[bin] += 1.0;
}

fn hash_pair(a: u16, b: u16, v: &mut Vec<f32>) {
    let key = (a as u64) << 16 | b as u64;
    let mut h = key.wrapping_mul(0x9E3779B97F4A7C15);
    h ^= h >> 29;
    h ^= (key >> 16).wrapping_mul(0xC2B2AE3D27D4EB4F);
    let bin = (h % EMBED_DIM as u64) as usize;
    v[bin] += 1.0;
}

/// Truncate or zero-pad a vector to EMBED_DIM, then L2-normalize.
pub fn normalize_dim(v: &[f32]) -> Vec<f32> {
    let mut out = vec![0.0f32; EMBED_DIM];
    for (i, x) in v.iter().take(EMBED_DIM).enumerate() {
        out[i] = *x;
    }
    normalize(&mut out);
    out
}

fn normalize(v: &mut [f32]) {
    let mag = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if mag > 1e-8 {
        for x in v.iter_mut() {
            *x /= mag;
        }
    }
}

/// Cosine similarity between two L2-normalized vectors (== dot product).
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    let mut dot = 0.0f32;
    for i in 0..n {
        dot += a[i] * b[i];
    }
    dot.max(0.0).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_embedding_is_fixed_dim_and_normalized() {
        let v = embed_local("grep over bash find for search");
        assert_eq!(v.len(), EMBED_DIM);
        let mag: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((mag - 1.0).abs() < 1e-3, "mag={mag}");
    }

    #[test]
    fn similar_text_scores_higher_than_unrelated() {
        let a = embed_local("I prefer grep over bash find");
        let b = embed_local("use grep not bash for searching files");
        let c = embed_local("the weather in tokyo is rainy today");
        let sim_ab = cosine(&a, &b);
        let sim_ac = cosine(&a, &c);
        assert!(
            sim_ab > sim_ac,
            "similar text should score higher: ab={sim_ab:.3} ac={sim_ac:.3}"
        );
    }

    #[test]
    fn normalize_dim_pads_or_truncates() {
        let short = vec![1.0f32, 2.0, 3.0];
        let v = normalize_dim(&short);
        assert_eq!(v.len(), EMBED_DIM);
    }

    #[test]
    fn local_embedding_records_cost_and_processing_provenance() {
        let _ = take_embedding_telemetry();
        let _ = embed_local_with_telemetry("telemetry is metadata only", "unit-test");
        let events = take_embedding_telemetry();
        let event = events.last().expect("local event");
        assert_eq!(event.processing, "local");
        assert_eq!(event.cost_usd_estimate, Some(0.0));
        assert!(event.input_tokens_estimate > 0);
        assert_eq!(event.outcome, "unit-test");
    }
}

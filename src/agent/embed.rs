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

use crate::error::{NurError, Result};

/// Fixed vector dimension for local embeddings. API embeddings are resized
/// (truncate/pad) to this when stored so cosine is always comparable.
pub const EMBED_DIM: usize = 256;

/// Default OpenAI-compatible embedding model id.
pub const DEFAULT_EMBED_MODEL: &str = "text-embedding-3-small";

/// Embed `text` to an L2-normalized vector of length EMBED_DIM using the active
/// provider's embeddings endpoint. Falls back to `embed_local` on any error so
/// memory indexing never hard-fails a turn.
pub fn embed(text: &str) -> Vec<f32> {
    embed_api(text).unwrap_or_else(|_| embed_local(text))
}

/// API embeddings call (OpenAI-compatible `<base>/embeddings`).
/// Best-effort; errors mean callers fall back to local.
pub fn embed_api(text: &str) -> Result<Vec<f32>> {
    let cfg = crate::config::load_config().unwrap_or_default();
    let key = crate::auth::resolve_api_key_for(Some(cfg.provider.as_str()))
        .unwrap_or_default();
    if key.trim().is_empty() {
        return Err(NurError::Other("no provider key for embeddings".into()));
    }
    let model = if cfg.memory_embed_model.trim().is_empty() {
        DEFAULT_EMBED_MODEL
    } else {
        &cfg.memory_embed_model
    };
    let base = cfg.base_url.trim_end_matches('/').to_string();
    let url = format!("{base}/embeddings");
    let body = serde_json::json!({ "model": model, "input": [text] });
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| NurError::Other(format!("embed client: {e}")))?;
    let resp = client
        .post(&url)
        .bearer_auth(&key)
        .json(&body)
        .send()
        .map_err(|e| NurError::Other(format!("embed send: {e}")))?;
    if !resp.status().is_success() {
        return Err(NurError::Other(format!("embed status {}", resp.status())));
    }
    let json: serde_json::Value = resp
        .json()
        .map_err(|e| NurError::Other(format!("embed parse: {e}")))?;
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
}

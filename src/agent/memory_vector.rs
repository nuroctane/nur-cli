//! Persistent vector store for native memory (real embeddings, m2).
//!
//! Each native_memory entry that is indexed gets a normalized vector persisted
//! under `~/.nur/native-memory/<scope>/vectors.json` keyed by entry id. Search is
//! cosine top-k over the L2-normalized vectors (from `embed::embed`).
//!
//! The vector is a *denormalized index complement*: the text lives in the
//! memory entry; the vector makes semantic top-k retrieval fast/precise. When a
//! memory is retired or deleted we drop its vector (keep the archive row).

use crate::agent::embed;
use crate::config::{atomic_write, nur_home};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorDoc {
    pub id: String,
    pub vec: Vec<f32>,
    /// Which embedding source produced this (api|local) — for honesty/telemetry.
    pub source: String,
}

pub struct VectorStore {
    scope: String,
    docs: BTreeMap<String, VectorDoc>,
}

fn store_path(scope: &str) -> PathBuf {
    let safe: String = scope
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    nur_home().join("native-memory").join(safe).join("vectors.json")
}

#[allow(dead_code)] // public API (len/remove/above) used by tests/tools; kept for vector hygiene
impl VectorStore {
    /// Open the store for a scope, loading persisted vectors.
    pub fn open(scope: &str) -> Self {
        let docs = std::fs::read_to_string(store_path(scope))
            .ok()
            .and_then(|t| serde_json::from_str::<BTreeMap<String, VectorDoc>>(&t).ok())
            .unwrap_or_default();
        Self {
            scope: scope.to_string(),
            docs,
        }
    }

    pub fn len(&self) -> usize {
        self.docs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    /// Index (embed + persist) a memory entry's text under its id.
    pub fn index(&mut self, id: &str, text: &str) -> String {
        // Real embedding; source is honest (api vs local) and reported.
        let (vec, source) = crate::agent::embed::embed_with_source(text);
        let source = source.to_string();
        let source_for_return = source.clone();
        self.docs.insert(
            id.to_string(),
            VectorDoc {
                id: id.to_string(),
                vec,
                source,
            },
        );
        let _ = self.save();
        source_for_return
    }

    /// Remove a vector by entry id (memory retired/deleted).
    pub fn remove(&mut self, id: &str) -> bool {
        let removed = self.docs.remove(id).is_some();
        if removed {
            let _ = self.save();
        }
        removed
    }

    /// Top-k semantic neighbors of `query` via cosine. Returns (id, score).
    pub fn search(&self, query: &str, k: usize) -> Vec<(String, f32)> {
        let q = embed::embed(query);
        let mut scored: Vec<(String, f32)> = self
            .docs
            .values()
            .map(|d| (d.id.clone(), embed::cosine(&q, &d.vec)))
            .collect();
        scored.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(k.max(1));
        scored
    }

    /// The ids whose similarity to `query` exceeds `threshold` (for routing).
    pub fn above(&self, query: &str, threshold: f32, k: usize) -> Vec<(String, f32)> {
        self.search(query, k)
            .into_iter()
            .filter(|(_, s)| *s >= threshold)
            .collect()
    }

    pub fn source_distribution(&self) -> BTreeMap<String, usize> {
        let mut m: BTreeMap<String, usize> = Default::default();
        for d in self.docs.values() {
            *m.entry(d.source.clone()).or_default() += 1;
        }
        m
    }

    fn save(&self) -> Result<(), String> {
        let p = store_path(&self.scope);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let body = serde_json::to_string_pretty(&self.docs).map_err(|e| e.to_string())?;
        atomic_write(&p, body.as_bytes()).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> String {
        format!("vec-test-{}", uuid::Uuid::new_v4().simple())
    }

    #[test]
    fn index_search_and_remove() {
        let s = scope();
        let mut vs = VectorStore::open(&s);
        let src_a = vs.index("mem-a", "I prefer grep over bash find for search");
        let _ = vs.index("mem-b", "deploy pipeline runs on kubernetes");
        assert_eq!(vs.len(), 2);
        // Semantic-ish search should rank mem-a near "which search tool do I prefer".
        let hits = vs.search("how should I search files grep or bash", 2);
        assert_eq!(hits.len(), 2);
        assert_eq!(
            hits[0].0, "mem-a",
            "expected mem-a most similar: {hits:?}"
        );
        assert!(
            hits[0].1 > hits[1].1,
            "mem-a should outrank mem-b: {hits:?}"
        );
        // Remove
        assert!(vs.remove("mem-b"));
        assert_eq!(vs.len(), 1);
        let _ = std::fs::remove_dir_all(
            crate::config::nur_home()
                .join("native-memory")
                .join(&s),
        );
        let _ = src_a;
    }

    #[test]
    fn open_reloads_persisted() {
        let s = scope();
        {
            let mut vs = VectorStore::open(&s);
            vs.index("x", "remember this fact about tokyo weather");
        }
        let vs = VectorStore::open(&s);
        assert_eq!(vs.len(), 1);
        let hits = vs.search("what is the weather in tokyo", 1);
        assert_eq!(hits[0].0, "x");
        let _ = std::fs::remove_dir_all(
            crate::config::nur_home().join("native-memory").join(&s),
        );
    }
}

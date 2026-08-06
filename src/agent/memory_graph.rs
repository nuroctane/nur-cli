//! Real knowledge-graph store for native memory (m3).
//!
//! Replaces the earlier string-match "triple" heuristic with a persistent,
//! queryable graph: nodes (entities) and typed edges (relations) persisted under
//! `~/.nur/native-memory/<scope>/graph.json`. Supports neighbors() and BFS
//! shortest-path traversal so the model can answer "what connects X to Y" and
//! walk relations beyond exact keyword match.
//!
//! Node/edge extraction is still heuristic (no NLP dep) but the STORAGE + TRAVERSAL
//! is a real graph — that is the honest upgrade we asked for.

use crate::agent::embed;
use crate::config::{atomic_write, nur_home};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Entry id(s) that mention this entity (provenance).
    #[serde(default)]
    pub memory_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphEdge {
    pub source: String,
    pub relation: String,
    pub target: String,
    #[serde(default)]
    pub memory_id: Option<String>,
    #[serde(default)]
    pub weight: f32,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct KnowledgeGraph {
    nodes: BTreeMap<String, GraphNode>,
    edges: Vec<GraphEdge>,
}

pub struct GraphStore {
    scope: String,
    graph: KnowledgeGraph,
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
    nur_home().join("native-memory").join(safe).join("graph.json")
}

const RELATIONS: &[&str] = &[
    "prefers",
    "uses",
    "depends_on",
    "requires",
    "is_part_of",
    "belongs_to",
    "is_a",
    "kind_of",
    "causes",
    "blocks",
    "conflicts_with",
    "runs_on",
    "lives_in",
    "wants",
    "avoids",
    "disables",
    "enables",
    "builds",
    "installs",
    "owned_by",
    "managed_by",
    "replaces",
    "supersedes",
];

/// Token split for entity canonicalization (lowercase alnum, min len 2).
fn tokens(s: &str) -> Vec<String> {
    s.to_ascii_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 2)
        .map(str::to_string)
        .collect()
}

impl GraphStore {
    pub fn open(scope: &str) -> Self {
        let graph = std::fs::read_to_string(store_path(scope))
            .ok()
            .and_then(|t| serde_json::from_str::<KnowledgeGraph>(&t).ok())
            .unwrap_or_default();
        Self {
            scope: scope.to_string(),
            graph,
        }
    }

    pub fn node_count(&self) -> usize {
        self.graph.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.graph.edges.len()
    }

    /// Upsert an entity node, merging aliases/memory provenance.
    fn upsert_node(&mut self, entity: &str, memory_id: &str) -> String {
        let canon = tokens(entity).join("_");
        if canon.is_empty() {
            return String::new();
        }
        let id = format!("n:{canon}");
        let node = self
            .graph
            .nodes
            .entry(id.clone())
            .or_insert_with(|| GraphNode {
                id: id.clone(),
                aliases: vec![entity.to_string()],
                memory_ids: Vec::new(),
            });
        if !node.aliases.iter().any(|a| *a == entity) {
            node.aliases.push(entity.to_string());
        }
        if !node.memory_ids.iter().any(|m| m == memory_id) {
            node.memory_ids.push(memory_id.to_string());
        }
        id
    }

    /// Add an edge (dedupe same source/relation/target with the new memory id).
    fn upsert_edge(&mut self, source: &str, relation: &str, target: &str, memory_id: &str) {
        self.graph.edges.retain(|e| {
            !(e.source == source && e.relation == relation && e.target == target)
                && !(e.source == target && e.relation == relation && e.target == source)
        });
        self.graph.edges.push(GraphEdge {
            source: source.to_string(),
            relation: relation.to_string(),
            target: target.to_string(),
            memory_id: Some(memory_id.to_string()),
            weight: 1.0,
        });
    }

    /// Heuristically extract + insert edges from a memory text.
    pub fn absorb(&mut self, memory_id: &str, text: &str) -> usize {
        let mut added = 0usize;
        let lower = text.to_ascii_lowercase();
        // "X <relation> Y" — find relation, split subject/object, strip noise words.
        for relation in RELATIONS {
            // Accept both spaced ("depends on") and underscored ("depends_on") spellings.
            let needle = relation.replace('_', " ");
            let alt = relation.to_string();
            for n in [&needle, &alt] {
                if n.len() < 2 {
                    continue;
                }
                let Some(idx) = lower.find(n) else { continue };
                let subject_raw = &text[..idx];
                let after = &text[idx + n.len()..];
                let object_pre = after.split([',', '.', '!', '?', ';', '\n']).next().unwrap_or("");
                let subject = clean_entity(subject_raw.split([',', ':', '-', ';', '.']).next().unwrap_or(subject_raw));
                let object = clean_entity(truncate_noise(object_pre));
                if names_ok(&subject) && names_ok(&object) && subject != object {
                    let src = self.upsert_node(&subject, memory_id);
                    let dst = self.upsert_node(&object, memory_id);
                    if !src.is_empty() && !dst.is_empty() {
                        self.upsert_edge(&src, relation, &dst, memory_id);
                        added += 1;
                    }
                }
                break; // one match per relation per text
            }
        }
        if added > 0 {
            let _ = self.save();
        }
        added
    }

    /// Direct neighbors of an entity (by canonical or alias match).
    pub fn neighbors(&self, entity: &str) -> Vec<(String, String, String)> {
        let id = self.resolve(entity);
        let id = id.as_deref().unwrap_or("");
        if id.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for e in &self.graph.edges {
            if e.source == id {
                out.push((e.source.clone(), e.relation.clone(), e.target.clone()));
            } else if e.target == id {
                out.push((e.target.clone(), e.relation.clone(), e.source.clone()));
            }
        }
        out
    }

    /// BFS shortest undirected path between two entities. Returns relation-labeled hops.
    pub fn path(&self, from: &str, to: &str) -> Option<Vec<(String, String, String)>> {
        let src = self.resolve(from)?;
        let dst = self.resolve(to)?;
        let mut prev: HashMap<String, (String, String, String)> = HashMap::new(); // cur -> (prev, rel, cur-or-other)
        let mut visited: BTreeSet<String> = BTreeSet::new();
        visited.insert(src.clone());
        let mut q = VecDeque::new();
        q.push_back(src.clone());
        while let Some(cur) = q.pop_front() {
            if cur == dst {
                break;
            }
            for (s, rel, t) in self.neighbors(&cur) {
                let next = if s == cur { t } else { s };
                if !visited.contains(&next) {
                    visited.insert(next.clone());
                    prev.insert(next.clone(), (cur.clone(), rel.clone(), next.clone()));
                    q.push_back(next);
                }
            }
        }
        if !prev.contains_key(&dst) {
            return None;
        }
        // Reconstruct path dst -> src.
        let mut hops = Vec::new();
        let mut cur = dst.clone();
        let mut guard = 0;
        while cur != src && guard < 128 {
            if let Some((p, rel, _)) = prev.get(&cur) {
                hops.push(((*p).clone(), rel.clone(), cur.clone()));
                cur = (*p).clone();
                guard += 1;
            } else {
                return None;
            }
        }
        hops.reverse();
        Some(hops)
    }

    pub fn resolve(&self, entity: &str) -> Option<String> {
        let canon = tokens(entity).join("_");
        let id = format!("n:{canon}");
        if self.graph.nodes.contains_key(&id) {
            return Some(id);
        }
        // alias lookup
        for (nid, node) in &self.graph.nodes {
            if node.aliases.iter().any(|a| a.to_ascii_lowercase() == entity.to_ascii_lowercase()) {
                return Some(nid.clone());
            }
        }
        None
    }

    pub fn all_entities(&self) -> Vec<String> {
        self.graph
            .nodes
            .values()
            .map(|n| {
                n.aliases
                    .first()
                    .cloned()
                    .unwrap_or_else(|| n.id.clone())
            })
            .collect()
    }

    /// Semantic rerank of entities against a query (embedding boost) for routing.
    pub fn closest_entities(&self, query: &str, k: usize) -> Vec<(String, f32)> {
        let q = embed::embed(query);
        let mut scored: Vec<(String, f32)> = self
            .graph
            .nodes
            .values()
            .map(|n| {
                let label = n.aliases.first().cloned().unwrap_or_default();
                let nv = embed::embed_local(&label);
                (label, embed::cosine(&q, &nv))
            })
            .collect();
        scored.sort_by(|a, b| {
            b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(k.max(1));
        scored
    }

    fn save(&self) -> Result<(), String> {
        let p = store_path(&self.scope);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let body = serde_json::to_string_pretty(&self.graph).map_err(|e| e.to_string())?;
        atomic_write(&p, body.as_bytes()).map_err(|e| e.to_string())
    }
}

fn clean_entity(s: &str) -> String {
    s.trim()
        .trim_start_matches(|c: char| !c.is_ascii_alphanumeric())
        .trim_end_matches(|c: char| !c.is_ascii_alphanumeric())
        .trim()
        .to_string()
}

/// Truncate an object phrase at stop-words so "tokio for async" → "tokio".
fn truncate_noise(s: &str) -> &str {
    const STOPS: &[&str] = &[
        " for ", " with ", " and ", " using ", " to ", " in ", " of ", " via ", " at ", " on ",
    ];
    let lower = s.to_ascii_lowercase();
    let mut best = s;
    let mut best_idx = s.len();
    for stop in STOPS {
        if let Some(idx) = lower.find(stop) {
            if idx < best_idx {
                best_idx = idx;
                best = &s[..idx];
            }
        }
    }
    best
}

fn names_ok(s: &str) -> bool {
    let toks = tokens(s);
    !toks.is_empty() && s.len() >= 3
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> String {
        format!("graph-test-{}", uuid::Uuid::new_v4().simple())
    }

    #[test]
    fn absorb_neighbors_and_path() {
        let s = scope();
        let mut g = GraphStore::open(&s);
        g.absorb("m1", "build agent uses cargo");
        g.absorb("m2", "cargo depends_on tokio for async");
        g.absorb("m3", "deploy job runs_on kubernetes");
        assert!(g.node_count() >= 4, "nodes: {}", g.node_count());
        assert!(g.edge_count() >= 3, "edges: {}", g.edge_count());
        // neighbors of cargo: build agent (uses) + tokio (depends_on)
        let nbrs = g.neighbors("cargo");
        assert!(
            nbrs.iter().any(|(_, r, t)| r == "uses" && t.contains("build_agent")),
            "cargo neighbors: {nbrs:?}"
        );
        // path build agent -> tokio should exist via cargo
        let path = g.path("build agent", "tokio");
        assert!(path.is_some(), "expected path: {path:?}");
        let hops = path.unwrap();
        assert_eq!(hops[0].2, "n:cargo", "first hop should hit cargo: {hops:?}");
        let _ = std::fs::remove_dir_all(
            crate::config::nur_home()
                .join("native-memory")
                .join(&s),
        );
    }

    #[test]
    fn path_returns_none_when_disconnected() {
        let s = scope();
        let mut g = GraphStore::open(&s);
        g.absorb("m1", "alpha uses beta");
        g.absorb("m2", "xenon uses yttrium");
        assert!(g.path("alpha", "yttrium").is_none());
        let _ = std::fs::remove_dir_all(
            crate::config::nur_home().join("native-memory").join(&s),
        );
    }
}

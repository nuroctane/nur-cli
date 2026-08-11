//! Central memory router (m4): the model stops guessing which memory resident to
//! use. Given an intent class, this decides the correct resident(s) and provides
//! a harmonized read that fans out to the real vector store + knowledge graph +
//! hierarchical memory and merges results.
//!
//! Optimization axes (see docs/research/memory-routing.md):
//! - **Efficiency**: don't slam every resident into the prompt; query on demand.
//! - **Contextual fullness**: pick the resident whose structure answers the
//!   question (vector for semantic similarity, graph for relationships/paths,
//!   keyword for exact facts, RLM store for docs).

use crate::agent::{embed, helix_memory, memory_graph, memory_vector, native_memory};
use std::collections::HashSet;

/// Retention / retrieval intent classes. Used to route, not to gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// "what did I decide / prefer / believe" → hierarchical memory + vector.
    Preference,
    /// project/workspace fact → hierarchical memory + graph.
    Fact,
    /// "find things semantically similar to X" → vector store.
    Semantic,
    /// "how is X connected to Y / what does X relate to" → knowledge graph.
    Relationship,
    /// exact doc / tool output I saw → RLM context store (peek/slice).
    ExactDoc,
    /// "what did another agent say" → mailbox.
    Message,
    /// generic / unknown → prefer the recency+confidence hierarchical path.
    General,
}

pub fn intent_class(text: &str) -> Intent {
    let t = text.to_ascii_lowercase();
    if contains_any(
        &t,
        &[
            "neighbor",
            "path",
            "connect",
            "relat",
            "depends",
            "depends_on",
            "graph",
            "link",
            "between",
        ],
    ) {
        return Intent::Relationship;
    }
    if contains_any(
        &t,
        &[
            "similar",
            "semantic",
            "embed",
            "closest",
            "like",
            "recall by meaning",
            "surface",
        ],
    ) {
        return Intent::Semantic;
    }
    if contains_any(
        &t,
        &[
            "exact",
            "verbatim",
            "tool result",
            "doc",
            "file i saw",
            "peek",
            "slice",
            "quoted",
            "snippet",
        ],
    ) {
        return Intent::ExactDoc;
    }
    if contains_any(
        &t,
        &[
            "agent said",
            "message",
            "mailbox",
            "other agent",
            "sibling",
            " said",
            " says ",
            "agent say",
            "another agent",
            "reviewer agent",
        ],
    ) {
        return Intent::Message;
    }
    if contains_any(
        &t,
        &[
            "decide", "prefer", "decision", "believe", "identity", "i am", "choice", "value",
        ],
    ) {
        return Intent::Preference;
    }
    if contains_any(
        &t,
        &["fact", "memory", "remember", "about", "know", "recall"],
    ) {
        return Intent::Fact;
    }
    Intent::General
}

fn contains_any(lower: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| lower.contains(n))
}

/// Human instruction for the model: which resident to use per intent. Injected
/// so the model routes by intent instead of guessing (kept compact to limit
/// per-turn prompt tokens).
pub fn routing_guidance() -> &'static str {
    r#"# Memory routing (use instead of guessing)
preference/decision → connectome remember (I) · fact → connectome recall
semantic "like X/by meaning" → mem vector · relationship/neighbors → mem graph
exact doc/tool output → context · "other agent said" → message recv
general → connectome recall. One-stop fan-out read: mem action=read.
"#
}

/// Unified write: route by intent, write to the right resident(s), dedup (e5).
pub fn write(scope: &str, text: &str, source: &str) -> String {
    let intent = intent_class(text);

    // Semantic dedup: if the vector store already has a near-identical memory,
    // don't create a duplicate — report the existing one instead.
    let vs = memory_vector::VectorStore::open(scope);
    if !vs.is_empty() {
        let near = vs.above(text, 0.92, 1);
        if let Some((existing_id, sim)) = near.into_iter().next() {
            if let Some(existing) = native_memory::get_by_id(scope, &existing_id) {
                return format!(
                    "dedup: already have a nearly-identical memory ({}, sim={sim:.2}) — not \
                     storing a duplicate. Existing: {}",
                    existing.id,
                    existing.text.chars().take(160).collect::<String>()
                );
            }
        }
    }

    let instance = native_memory::remember(
        scope,
        text,
        native_memory::Tier::L1,
        match intent {
            Intent::Preference => native_memory::Voice::FirstPerson,
            _ => native_memory::Voice::Observed,
        },
        &[intent_tag(intent).to_string(), "routed".into()],
        0.8,
        source,
    );
    match instance {
        Ok(e) => format!(
            "stored to hierarchical memory ({}, tier l1) + vector + graph\nid={} routed={:?}",
            e.id, e.id, intent
        ),
        Err(err) => format!("memory write failed: {err}"),
    }
}

fn intent_tag(i: Intent) -> &'static str {
    match i {
        Intent::Preference => "preference",
        Intent::Fact => "fact",
        Intent::Semantic => "semantic",
        Intent::Relationship => "relationship",
        Intent::ExactDoc => "exactdoc",
        Intent::Message => "message",
        Intent::General => "general",
    }
}

/// Unified read: fans out across the real vector store, knowledge graph, and
/// hierarchical memory, merging into a ranked context string.
pub fn read(scope: &str, query: &str, vector_k: usize, graph_k: usize) -> String {
    let intent = intent_class(query);
    let mut out = Vec::new();
    let header = format!("memory read · query={query:?} · routed={intent:?} ·  scope={scope}");
    out.push(header);

    let query_has_text = !query.trim().is_empty();
    let vs = memory_vector::VectorStore::open(scope);
    let g = memory_graph::GraphStore::open(scope);
    let graph_needs_embedding = query_has_text
        && matches!(intent, Intent::Relationship | Intent::Semantic)
        && g.node_count() > 0;
    let helix_enabled = query_has_text && helix_memory::is_configured();

    // Compute exactly one semantic query vector and share it across local
    // vector search, Helix, and graph reranking. Empty prompt snapshots skip
    // semantic residents entirely and use hierarchical recency recall below.
    let query_embedding = (query_has_text
        && (!vs.is_empty() || helix_enabled || graph_needs_embedding))
        .then(|| embed::embed(query));
    let mut seen_memory_ids = HashSet::new();

    // 1) Local vector store (semantic top-k).
    if !vs.is_empty() {
        if let Some(query_embedding) = query_embedding.as_deref() {
            let hits = vs.search_with_embedding(query_embedding, vector_k.max(1));
            let mut lines = Vec::new();
            for (id, score) in hits {
                let Some(mem) = native_memory::get_by_id(scope, &id) else {
                    continue;
                };
                if !seen_memory_ids.insert(id) {
                    continue;
                }
                let snip: String = mem.text.chars().take(160).collect();
                lines.push(format!("  · sim={score:.2} · {snip}"));
            }
            if !lines.is_empty() {
                out.push(format!("[vector] top-{}:", lines.len()));
                out.extend(lines);
            }
        }
    }

    // Optional HelixDB graph-vector resident. Never run for the empty prompt
    // snapshot, so an unavailable service cannot affect startup or per-turn
    // prompt construction. Local memory remains the source of truth.
    if helix_enabled {
        match helix_memory::search_with_embedding(
            scope,
            query,
            query_embedding.as_deref().unwrap_or_default(),
            vector_k.max(1),
        ) {
            Ok(hits) if !hits.is_empty() => {
                let mut lines = Vec::new();
                for hit in hits {
                    if !seen_memory_ids.insert(hit.id.clone()) {
                        continue;
                    }
                    let snip: String = hit.text.chars().take(160).collect();
                    lines.push(format!("  - score={:.2} - {} - {snip}", hit.score, hit.id));
                }
                if !lines.is_empty() {
                    out.push(format!("[helix] top-{}:", lines.len()));
                    out.extend(lines);
                }
            }
            Ok(_) => {}
            Err(err) if !err.contains("not configured") => {
                out.push(format!("[helix] unavailable: {err}"));
            }
            Err(_) => {}
        }
    }

    // 2) Knowledge graph (neighbors + path) only for Relationship/Semantic intents.
    if graph_needs_embedding {
        let entities = g.closest_entities_with_embedding(
            query_embedding.as_deref().unwrap_or_default(),
            graph_k.max(1),
        );
        for (label, sim) in entities {
            let nbrs = g.neighbors(&label);
            if nbrs.is_empty() {
                continue;
            }
            out.push(format!("[graph] {label} (sim={sim:.2}) neighbors:"));
            for (s, r, t) in nbrs {
                out.push(format!("    {s} -[{r}]-> {t}"));
            }
        }
    }

    // 2b) Exact-doc intent → RLM context store (peek/search for the exact text).
    if intent == Intent::ExactDoc {
        let sid = std::env::var("NUR_SESSION_ID").unwrap_or_default();
        if !sid.is_empty() {
            let inv = crate::agent::context_store::prompt_inventory(&sid);
            if !inv.is_empty() {
                out.push("[context_store] exact-doc intent — variables available:".to_string());
                out.push(inv);
            } else {
                out.push(
                    "[context_store] no RLM context variables (exact-doc not available)"
                        .to_string(),
                );
            }
        }
    }

    // 2c) Message intent → mailbox (what another agent said).
    if intent == Intent::Message {
        let mailbox = crate::agent::mailbox::receive(scope, "agent", false);
        if !mailbox.is_empty() {
            out.push(format!("[mailbox] {} message(s):", mailbox.len()));
            for m in mailbox.iter().take(5) {
                let t: String = m.text.chars().take(140).collect();
                out.push(format!("   · {t}"));
            }
        } else {
            out.push("[mailbox] no messages in this scope".to_string());
        }
    }

    // 3) Hierarchical memory: intent-weighted keyword+recency recall.
    let recall_k = vector_k.max(1);
    let hits = native_memory::recall(scope, query, recall_k.saturating_mul(2));
    let mut lines = Vec::new();
    for (entry, score) in hits {
        if lines.len() >= recall_k {
            break;
        }
        if !seen_memory_ids.insert(entry.id.clone()) {
            continue;
        }
        let snip: String = entry.text.chars().take(140).collect();
        lines.push(format!(
            "  · s={score:.2} · [{}] · {snip}",
            entry.tier.as_str()
        ));
    }
    if !lines.is_empty() {
        out.push(format!("[hierarchical] top-{}:", lines.len()));
        out.extend(lines);
    }

    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intent_classification() {
        assert_eq!(
            intent_class("how is cargo connected to tokio"),
            Intent::Relationship
        );
        assert_eq!(
            intent_class("find memories similar to this idea"),
            Intent::Semantic
        );
        assert_eq!(
            intent_class("what did I decide about the deploy"),
            Intent::Preference
        );
        assert_eq!(
            intent_class("show me the exact tool result for the audit"),
            Intent::ExactDoc
        );
        assert_eq!(
            intent_class("what did the reviewer agent say"),
            Intent::Message
        );
        assert_eq!(
            intent_class("greetings, tell me broadly something"),
            Intent::General
        );
    }

    #[test]
    fn write_dedups_near_identical() {
        let scope = format!("router-dedup-{}", uuid::Uuid::new_v4().simple());
        let m1 = write(&scope, "I prefer grep over bash find for searching", "t");
        assert!(m1.contains("stored"), "first write: {m1}");
        // Near-identical write should be deduped, not stored twice.
        let m2 = write(&scope, "I prefer grep over bash find for searching", "t");
        assert!(m2.contains("dedup"), "expected dedup, got: {m2}");
        let _ =
            std::fs::remove_dir_all(crate::config::nur_home().join("native-memory").join(&scope));
    }

    #[test]
    fn write_and_read_fanout() {
        let scope = format!("router-{}", uuid::Uuid::new_v4().simple());
        // Write a preference (routes to hierarchical + vector + graph).
        let msg = write(&scope, "I prefer grep over bash find for searching", "test");
        assert!(msg.contains("stored"), "write failed: {msg}");
        // Read should surface the vector hit for a semantic-ish query.
        let res = read(&scope, "which search approach do I prefer", 3, 2);
        assert!(res.contains("[vector]"), "vector missing: {res}");
        assert!(
            res.to_ascii_lowercase().contains("grep"),
            "expected grep in result: {res}"
        );
        assert_eq!(
            res.matches("I prefer grep over bash find for searching")
                .count(),
            1,
            "the same memory id must only be rendered once: {res}"
        );
        let _ =
            std::fs::remove_dir_all(crate::config::nur_home().join("native-memory").join(&scope));
    }

    #[test]
    fn empty_snapshot_skips_semantic_residents() {
        let scope = format!("router-empty-{}", uuid::Uuid::new_v4().simple());
        let msg = write(&scope, "Remember the cobalt deployment checklist", "test");
        assert!(msg.contains("stored"), "write failed: {msg}");

        let res = read(&scope, "", 3, 2);
        assert!(!res.contains("[vector]"), "empty query embedded: {res}");
        assert!(!res.contains("[helix]"), "empty query reached Helix: {res}");
        assert!(
            res.contains("[hierarchical]"),
            "recency fallback missing: {res}"
        );

        let _ =
            std::fs::remove_dir_all(crate::config::nur_home().join("native-memory").join(&scope));
    }
}

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

use crate::agent::{memory_graph, memory_vector, native_memory};

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
    if contains_any(&t, &["neighbor", "path", "connect", "relat", "depends", "depends_on", "graph", "link", "between"]) {
        return Intent::Relationship;
    }
    if contains_any(&t, &["similar", "semantic", "embed", "closest", "like", "recall by meaning", "surface"]) {
        return Intent::Semantic;
    }
    if contains_any(&t, &["exact", "verbatim", "tool result", "doc", "file i saw", "peek", "slice", "quoted", "snippet"]) {
        return Intent::ExactDoc;
    }
    if contains_any(&t, &["agent said", "message", "mailbox", "other agent", "sibling", " said", " says ", "agent say", "another agent", "reviewer agent"]) {
        return Intent::Message;
    }
    if contains_any(&t, &["decide", "prefer", "decision", "believe", "identity", "i am", "choice", "value"]) {
        return Intent::Preference;
    }
    if contains_any(&t, &["fact", "memory", "remember", "about", "know", "recall"]) {
        return Intent::Fact;
    }
    Intent::General
}

fn contains_any(lower: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| lower.contains(n))
}

/// Human instruction for the model: which resident to use per intent. Injected
/// so the model routes by intent instead of guessing.
pub fn routing_guidance() -> &'static str {
    r#"# Memory routing (use this instead of guessing)
When recalling or storing, pick the resident by intent:
- preference / decision / identity  → `connectome remember` (first-person) + vector recall
- project fact / "what do I know about X"  → `connectome recall` (hierarchical) 
- SEMANTIC similarity ("find things like X / by meaning")  → `mem` action=vector
- relationship / "how is X connected to Y / neighbors"  → `mem` action=graph (neighbors|path)
- exact verbatim doc / tool output I saw  → `context` (peek/slice/search)
- "what did another agent say"  → `message` recv
- general memory  → `connectome recall`

Harmonized one-stop: `mem` action=read fans out across vector + graph + hierarchical.
Never store a memory in more than one resident for the same fact.
"#
}

/// Unified write: route by intent, write to the right resident(s), dedup.
pub fn write(scope: &str, text: &str, source: &str) -> String {
    let intent = intent_class(text);
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
pub fn read(
    scope: &str,
    query: &str,
    vector_k: usize,
    graph_k: usize,
) -> String {
    let intent = intent_class(query);
    let mut out = Vec::new();
    let header = format!(
        "memory read · query={query:?} · routed={intent:?} ·  scope={scope}"
    );
    out.push(header);

    // 1) Vector store (semantic top-k), for any query.
    let vs = memory_vector::VectorStore::open(scope);
    if !vs.is_empty() {
        let hits = vs.search(query, vector_k.max(1));
        if !hits.is_empty() {
            out.push(format!("[vector] top-{}:", hits.len()));
            for (id, s) in hits {
                if let Some(mem) = native_memory::get_by_id(scope, &id) {
                    let snip: String = mem.text.chars().take(160).collect();
                    out.push(format!("  · sim={s:.2} · {snip}"));
                } else {
                    out.push(format!("  · sim={s:.2} · (mem {id} archived)"));
                }
            }
        } else {
            out.push("[vector] no entries".to_string());
        }
    }

    // 2) Knowledge graph (neighbors + path) only for Relationship/Semantic intents.
    let g = memory_graph::GraphStore::open(scope);
    if matches!(intent, Intent::Relationship | Intent::Semantic) && g.node_count() > 0 {
        let entities = g.closest_entities(query, graph_k.max(1));
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
    } else if g.node_count() > 0 {
        out.push(format!(
            "[graph] {} nodes/{} edges (query not relationship-shaped; ask `mem graph` for links)",
            g.node_count(),
            g.edge_count()
        ));
    }

    // 3) Hierarchical memory: intent-weighted keyword+recency recall.
    let hits = native_memory::recall(scope, query, vector_k.max(1));
    if !hits.is_empty() {
        out.push(format!("[hierarchical] top-{}:", hits.len()));
        for (e, s) in hits.iter().take(vector_k.max(1)) {
            let snip: String = e.text.chars().take(140).collect();
            out.push(format!("  · s={s:.2} · [{}] · {snip}", e.tier.as_str()));
        }
    }

    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intent_classification() {
        assert_eq!(intent_class("how is cargo connected to tokio"), Intent::Relationship);
        assert_eq!(intent_class("find memories similar to this idea"), Intent::Semantic);
        assert_eq!(intent_class("what did I decide about the deploy"), Intent::Preference);
        assert_eq!(intent_class("show me the exact tool result for the audit"), Intent::ExactDoc);
        assert_eq!(intent_class("what did the reviewer agent say"), Intent::Message);
        assert_eq!(intent_class("greetings, tell me broadly something"), Intent::General);
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
        let _ = std::fs::remove_dir_all(
            crate::config::nur_home().join("native-memory").join(&scope),
        );
    }
}

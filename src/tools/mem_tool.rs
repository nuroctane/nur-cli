//! `mem` tool — harmonized memory access via the central router (m4).
//!
//! One entry point that replaces "which memory system do I use?" with an
//! intent-routed fan-out: vector (semantic) + knowledge graph (relations/paths)
//! + hierarchical memory + RLM context-store pointers. Writing via `mem write`
//!   stores once (dedup) across the unified hierarchy and auto-indexes vector+graph.

use super::{arg_str, arg_u64, Tool, ToolContext};
use crate::agent::memory_router;
use crate::error::{NurError, Result};
use serde_json::Value;

pub struct MemTool;

fn scope_from(args: &Value, ctx: &ToolContext) -> String {
    if let Some(s) = args
        .get("scope")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return s.to_string();
    }
    let proj = ctx
        .cwd
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace");
    let sid = std::env::var("NUR_SESSION_ID").unwrap_or_default();
    if sid.is_empty() {
        format!("{proj}:global")
    } else {
        format!("{proj}:{sid}")
    }
}

impl Tool for MemTool {
    fn name(&self) -> &str {
        "mem"
    }

    fn description(&self) -> &str {
        "Harmonized memory access (central router). One-stop read/write across \
         vector (semantic), knowledge graph (relations/paths), and hierarchical \
         memory — no guessing which system. Actions: read (fan-out, intent-routed; \
         prefer for any recall) | write (store once, auto-indexes vector+graph) | \
         vector (semantic top-k) | graph (neighbors|path) | helix_status | \
         helix_sync | guidance. Never store \
         secrets. Complements connectome/context/memory/optmem/plur/ruflo."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["read", "write", "vector", "graph", "helix_status", "helix_sync", "guidance"],
                    "description": "read = intent-routed fan-out (preferred); write = store once + auto-index; \
                    vector = semantic top-k only; graph neighbors|path; guidance = routing table"
                },
                "query": {"type": "string", "description": "For read/vector: the question or text to match"},
                "text": {"type": "string", "description": "For write: the memory to store"},
                "mode": {
                    "type": "string",
                    "enum": ["neighbors", "path"],
                    "description": "For graph: neighbors of entry, or path between entry and target"
                },
                "entry": {"type": "string", "description": "For graph: start entity"},
                "target": {"type": "string", "description": "For graph path: target entity"},
                "k": {"type": "integer", "description": "Result count (default 8)"},
                "scope": {"type": "string"}
            },
            "required": ["action"]
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action")?;
        let scope = scope_from(args, ctx);
        let k = arg_u64(args, "k").unwrap_or(8) as usize;

        match action.as_str() {
            "guidance" => Ok(memory_router::routing_guidance().to_string()),
            "helix_status" => Ok(crate::agent::helix_memory::status(&scope)),
            "helix_sync" => crate::agent::helix_memory::sync(&scope).map_err(NurError::Tool),
            "write" => {
                let text = arg_str(args, "text")?;
                Ok(memory_router::write(&scope, &text, "mem_tool"))
            }
            "read" => {
                let query = arg_str(args, "query")?;
                Ok(memory_router::read(&scope, &query, k, k))
            }
            "vector" => {
                let query = arg_str(args, "query")?;
                let vs = crate::agent::memory_vector::VectorStore::open(&scope);
                if vs.is_empty() {
                    return Ok("vector store empty for this scope".into());
                }
                let hits = vs.search(&query, k);
                let mut lines = vec![format!(
                    "vector top-{} (dims=256, sources={:?}):",
                    hits.len(),
                    vs.source_distribution()
                )];
                for (id, s) in hits {
                    if let Some(m) = crate::agent::native_memory::get_by_id(&scope, &id) {
                        let snip: String = m.text.chars().take(150).collect();
                        lines.push(format!("  sim={s:.2} · {snip}"));
                    }
                }
                Ok(lines.join("\n"))
            }
            "graph" => {
                let mode = args
                    .get("mode")
                    .and_then(|v| v.as_str())
                    .unwrap_or("neighbors");
                let g = crate::agent::memory_graph::GraphStore::open(&scope);
                if g.node_count() == 0 {
                    return Ok("knowledge graph empty for this scope (write memories to build it)".into());
                }
                match mode {
                    "path" => {
                        let entry = arg_str(args, "entry")?;
                        let target = arg_str(args, "target")?;
                        match g.path(&entry, &target) {
                            Some(hops) => {
                                let mut lines = vec![format!(
                                    "path {entry} → {target} ({} nodes/{} edges):",
                                    g.node_count(),
                                    g.edge_count()
                                )];
                                // hops are (source_label, relation, target_label)
                                for (s, r, t) in &hops {
                                    lines.push(format!("  {s} -[{r}]-> {t}"));
                                }
                                Ok(lines.join("\n"))
                            }
                            None => Err(NurError::Tool(format!(
                                "no path between `{entry}` and `{target}` in the graph"
                            ))),
                        }
                    }
                    _ => {
                        let entry = arg_str(args, "entry")?;
                        let nbrs = g.neighbors(&entry);
                        if nbrs.is_empty() {
                            return Ok(format!("no neighbors for `{entry}` in scope"));
                        }
                        let mut lines = vec![format!(
                            "neighbors of {entry} ({} total):",
                            nbrs.len()
                        )];
                        for (_, r, t) in nbrs {
                            lines.push(format!("  -[{r}]-> {t}"));
                        }
                        Ok(lines.join("\n"))
                    }
                }
            }
            other => Err(NurError::Tool(format!(
                "unknown mem action `{other}`; use read|write|vector|graph|helix_status|helix_sync|guidance"
            ))),
        }
    }
}

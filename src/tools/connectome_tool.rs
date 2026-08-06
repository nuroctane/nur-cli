//! Connectome + agent-native memory tool surface.

use super::{arg_str, arg_u64, Tool, ToolContext};
use crate::agent::chronicle;
use crate::agent::native_memory::{self, Tier, Voice};
use crate::error::{NurError, Result};
use serde_json::Value;

pub struct ConnectomeTool;

fn scope_from(args: &Value, ctx: &ToolContext) -> String {
    if let Some(s) = args
        .get("scope")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return s.to_string();
    }
    // Prefer project-scoped continuity (Connectome: same agent across months on a machine).
    let proj = ctx
        .cwd
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("workspace");
    std::env::var("NUR_SESSION_ID")
        .ok()
        .filter(|s| !s.is_empty())
        .map(|sid| format!("{proj}:{sid}"))
        .unwrap_or_else(|| format!("{proj}:global"))
}

impl Tool for ConnectomeTool {
    fn name(&self) -> &str {
        "connectome"
    }

    fn description(&self) -> &str {
        "Agent-native memory + continuity (arXiv:2606.24775 four modules + Anima Connectome). \
         Hierarchical self-authored memory (recent/l1/l2/l3), append-only chronicle, checkpoints. \
         Actions: remember | recall | list | consolidate | extract | chronicle | chronicle_tail | \
         checkpoint | restore | status. Prefer first-person remember for on-policy continuity. \
         Never store secrets. Complements memory/optmem/plur — does not replace them."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": [
                        "remember", "recall", "list", "consolidate", "extract", "supersede",
                        "chronicle", "chronicle_tail", "checkpoint", "restore", "status",
                        "graph"
                    ]
                },
                "text": {"type": "string", "description": "Memory text or chronicle body"},
                "query": {"type": "string", "description": "For recall"},
                "tier": {
                    "type": "string",
                    "enum": ["recent", "l1", "l2", "l3"],
                    "description": "Connectome resolution tier (default l1 for remember)"
                },
                "voice": {
                    "type": "string",
                    "enum": ["first_person", "observed"],
                    "description": "first_person = on-policy self-authored (preferred)"
                },
                "tags": {
                    "type": "array",
                    "items": {"type": "string"}
                },
                "confidence": {"type": "number"},
                "name": {"type": "string", "description": "Checkpoint name"},
                "note": {"type": "string"},
                "k": {"type": "integer", "description": "Recall top-k (default 8)"},
                "n": {"type": "integer", "description": "Chronicle tail length"},
                "scope": {"type": "string", "description": "Override memory scope id"}
            },
            "required": ["action"]
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action")?;
        let scope = scope_from(args, ctx);

        match action.as_str() {
            "status" => Ok(format!(
                "{}\n{}\n{}",
                native_memory::status(&scope),
                native_memory::ops_report(&scope),
                chronicle::status(&scope)
            )),
            "list" => {
                let entries = native_memory::load_entries(&scope);
                if entries.is_empty() {
                    return Ok("no native memories in this scope".into());
                }
                let mut lines = vec![format!("{} memories:", entries.len())];
                for e in entries.iter().rev().take(40) {
                    lines.push(format!(
                        "- {} [{}/{:?}] {}",
                        e.id,
                        e.tier.as_str(),
                        e.voice,
                        e.text.chars().take(120).collect::<String>()
                    ));
                }
                Ok(lines.join("\n"))
            }
            "remember" => {
                let text = arg_str(args, "text")?;
                let tier = match args
                    .get("tier")
                    .and_then(|v| v.as_str())
                    .unwrap_or("l1")
                    .to_ascii_lowercase()
                    .as_str()
                {
                    "recent" => Tier::Recent,
                    "l2" => Tier::L2,
                    "l3" => Tier::L3,
                    _ => Tier::L1,
                };
                let voice = match args
                    .get("voice")
                    .and_then(|v| v.as_str())
                    .unwrap_or("first_person")
                {
                    "observed" => Voice::Observed,
                    _ => Voice::FirstPerson,
                };
                let tags: Vec<String> = args
                    .get("tags")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                let conf = args
                    .get("confidence")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.75) as f32;
                let e = native_memory::remember(
                    &scope, &text, tier, voice, &tags, conf, "connectome_tool",
                )
                .map_err(NurError::Tool)?;
                let _ = chronicle::append(
                    &scope,
                    "remember",
                    &format!("stored {} {} — {}", e.id, e.tier.as_str(), e.text.chars().take(80).collect::<String>()),
                    None,
                );
                Ok(format!(
                    "remembered {} tier={} voice={:?} conf={:.2}",
                    e.id,
                    e.tier.as_str(),
                    e.voice,
                    e.confidence
                ))
            }
            "recall" => {
                let query = args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let k = arg_u64(args, "k").unwrap_or(8) as usize;
                let hits = native_memory::recall(&scope, &query, k);
                if hits.is_empty() {
                    return Ok("no matching memories".into());
                }
                let mut lines = vec![format!("recall q={query:?} hits={}", hits.len())];
                for (e, s) in hits {
                    lines.push(format!(
                        "- [{tier} s={s:.2} c={c:.2}] {text}",
                        tier = e.tier.as_str(),
                        c = e.confidence,
                        text = e.text
                    ));
                }
                Ok(lines.join("\n"))
            }
            "consolidate" => {
                let max_l1 = arg_u64(args, "k").unwrap_or(24) as usize;
                let msg =
                    native_memory::consolidate_localized(&scope, max_l1).map_err(NurError::Tool)?;
                let _ = chronicle::append(&scope, "maintain", &msg, None);
                Ok(msg)
            }
            "supersede" => {
                let text = arg_str(args, "text")?;
                // Subject = a short noun token the contradiction pivots on (e.g. "grep").
                let subject: String = args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        text.split_whitespace()
                            .nth(1)
                            .unwrap_or(text.as_str())
                            .to_string()
                    });
                let n =
                    native_memory::supersede_contradictions(&scope, &subject, &text)
                        .map_err(NurError::Tool)?;
                Ok(format!(
                    "superseded {n} older contradicting memory/memories (archived, lowered confidence)\\n  recommend: also call remember to store the corrected fact"
                ))
            }
            "extract" => {
                let text = arg_str(args, "text")?;
                let cands = native_memory::extract_candidates(&text);
                if cands.is_empty() {
                    return Ok("no extractable durable lines (use remember for explicit notes)".into());
                }
                let mut stored = Vec::new();
                for c in cands {
                    if let Ok(e) = native_memory::remember(
                        &scope,
                        &c,
                        Tier::Recent,
                        Voice::Observed,
                        &["extracted".into()],
                        0.55,
                        "extract",
                    ) {
                        stored.push(e.id);
                    }
                }
                Ok(format!("extracted {} candidate(s): {}", stored.len(), stored.join(", ")))
            }
            "chronicle" => {
                let text = arg_str(args, "text")?;
                let kind = args
                    .get("tier")
                    .and_then(|v| v.as_str())
                    .unwrap_or("note");
                let ev = chronicle::append(&scope, kind, &text, None).map_err(NurError::Tool)?;
                Ok(format!("chronicle #{} [{}] recorded", ev.seq, ev.kind))
            }
            "chronicle_tail" => {
                let n = arg_u64(args, "n").unwrap_or(20) as usize;
                let events = chronicle::tail(&scope, n);
                if events.is_empty() {
                    return Ok("chronicle empty".into());
                }
                let mut lines = Vec::new();
                for e in events {
                    lines.push(format!(
                        "#{} [{}] {}",
                        e.seq,
                        e.kind,
                        e.text.chars().take(200).collect::<String>()
                    ));
                }
                Ok(lines.join("\n"))
            }
            "checkpoint" => {
                let name = arg_str(args, "name")?;
                let note = args.get("note").and_then(|v| v.as_str()).unwrap_or("");
                let cp = chronicle::checkpoint(&scope, &name, note).map_err(NurError::Tool)?;
                Ok(format!(
                    "checkpoint `{}` at seq {} ({})",
                    cp.name, cp.seq, cp.note
                ))
            }
            "restore" => {
                // Soft restore: describe as-of, never rewrite live history (Connectome).
                let name = arg_str(args, "name")?;
                chronicle::describe_at(&scope, &name).map_err(NurError::Tool)
            }
            "graph" => {
                // Real knowledge-graph store (m3): neighbors + entity list.
                let g = crate::agent::memory_graph::GraphStore::open(&scope);
                if g.node_count() == 0 {
                    return Ok("knowledge graph empty for this scope (use `mem` or `connectome remember` to build it)".into());
                }
                let mut out = vec![format!(
                    "knowledge graph scope={scope} · {} nodes / {} edges",
                    g.node_count(),
                    g.edge_count()
                )];
                for label in g.all_entities().into_iter().take(40) {
                    let nbrs = g.neighbors(&label);
                    if nbrs.is_empty() {
                        continue;
                    }
                    let rels: Vec<String> = nbrs
                        .iter()
                        .map(|(_, r, t)| format!("{r}→{t}"))
                        .take(6)
                        .collect();
                    out.push(format!("  {label}: {}", rels.join(", ")));
                }
                Ok(out.join("\n"))
            }
            other => Err(NurError::Tool(format!("unknown connectome action `{other}`"))),
        }
    }
}

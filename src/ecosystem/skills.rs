//! Install Meta-bundled SKILL.md packs for plur / ruflo / graphify into
//! `~/.meta/skills/` so the agent discovers them on first launch.

use crate::config::muse_home;
use crate::error::Result;
use std::fs;
use std::path::PathBuf;

pub fn install_bundled_skills() -> Result<Vec<String>> {
    let root = muse_home().join("skills");
    fs::create_dir_all(&root)?;
    let mut installed = Vec::new();

    for (name, body) in BUNDLED {
        let dir = root.join(name);
        fs::create_dir_all(&dir)?;
        let path = dir.join("SKILL.md");
        // Always refresh so skill docs stay in sync with this meta version.
        fs::write(&path, body)?;
        installed.push(name.to_string());
    }

    // Also mirror into ~/.agents/skills for cross-framework discovery.
    if let Some(home) = dirs::home_dir() {
        let agents = home.join(".agents").join("skills");
        let _ = fs::create_dir_all(&agents);
        for (name, body) in BUNDLED {
            let dir = agents.join(name);
            let _ = fs::create_dir_all(&dir);
            let _ = fs::write(dir.join("SKILL.md"), body);
        }
    }

    Ok(installed)
}

#[allow(dead_code)]
pub fn skill_paths() -> Vec<PathBuf> {
    let mut out = vec![muse_home().join("skills")];
    if let Some(home) = dirs::home_dir() {
        out.push(home.join(".agents").join("skills"));
    }
    out
}

const BUNDLED: &[(&str, &str)] = &[
    ("plur", PLUR_SKILL),
    ("ruflo", RUFLO_SKILL),
    ("graphify", GRAPHIFY_SKILL),
];

const PLUR_SKILL: &str = r#"---
name: plur
description: "Local-first shared memory for AI agents (engrams + episodes). Use for preferences, corrections, conventions, and session learnings that must persist across tools and sessions."
---

# PLUR — shared agent memory

Meta CLI auto-installs `@plur-ai/cli` and provisions `~/.plur/`. Use the **`plur`** tool
(or `/plur` slash command) — do not ask the user to run npm.

## When to use

- User corrects style, architecture, or conventions → `plur(action=learn, …)`
- Start of a task that may reuse past knowledge → `plur(action=inject)` or `recall`
- After fixing an incident → `plur(action=capture)` episode
- Cross-session preferences → always prefer PLUR over ephemeral chat memory

## Actions (via `plur` tool)

| action | purpose |
|--------|---------|
| status | store health + engram counts |
| learn | store a correction / preference / convention |
| recall | hybrid search over engrams |
| inject | select engrams for the current task (token-budgeted) |
| list | list engrams |
| capture | record an episode (what happened when) |
| timeline | query episodes |
| feedback | rate an engram positive/negative |
| forget | retire an engram |
| ingest | extract engrams from free text |

## Rules

- Never store secrets, API keys, tokens, or passwords in engrams.
- Scope project knowledge with `scope` (e.g. `project:meta-cli`); personal prefs → `global`.
- After a user correction, learn it immediately so the next turn benefits.
- PLUR is memory of *assertions*, not a code index — use graphify for code structure.

Upstream: https://github.com/plur-ai/plur · https://plur.ai
"#;

const RUFLO_SKILL: &str = r#"---
name: ruflo
description: "Agent meta-harness: vector memory, swarm coordination, hive-mind, hooks. Use for multi-agent orchestration patterns, semantic memory search, and self-learning trajectories."
---

# Ruflo — agent orchestration harness

Meta CLI auto-installs `ruflo` and provisions global vector memory at
`~/.meta/ruflo/memory.db`. Use the **`ruflo`** tool (or `/ruflo`) — no separate init.

## When to use

- Need semantic pattern memory across sessions → `ruflo(action=memory_search|memory_store)`
- Multi-step parallel research that benefits from swarm topology → `swarm_init` / `hive_status`
- Check harness health → `status`
- List agent types → `agent_list`

## Actions (via `ruflo` tool)

| action | purpose |
|--------|---------|
| status | ruflo + memory status |
| memory_store | store key/value (+ optional vector) in global AgentDB |
| memory_search | semantic/hybrid search |
| memory_stats | entry counts / backend |
| memory_list | list entries |
| agent_list | available agent types |
| swarm_init | init hierarchical swarm (coordination state) |
| swarm_status | swarm health |
| hive_status | hive-mind status |
| doctor | diagnostics |

## Rules

- Default memory lives under Meta's home (`~/.meta/ruflo/`) so project trees stay clean.
- Prefer PLUR for *preferences and corrections*; Ruflo memory for *patterns, trajectories, embeddings*.
- Prefer graphify for *code structure graphs*.
- Do not require Claude Code — Meta is the host agent.
- Swarm spawn of external Claude/Codex workers is optional; Meta's own `agent` tool covers nested research.

Upstream: https://github.com/ruvnet/ruflo
"#;

const GRAPHIFY_SKILL: &str = r#"---
name: graphify
description: "Code knowledge graph. Prefer graphify query/path/explain over broad grep when graphify-out/ exists. Build with extract (local AST, no API key)."
---

# Graphify — knowledge graph over the workspace

Meta auto-installs `graphifyy` (CLI: `graphify`) and registers the agents skill.
Use the **`graphify`** tool or `/graphify`.

## Fast path

1. If `graphify-out/graph.json` exists and the question is architectural →
   `graphify(action=query|path|explain)` immediately.
2. If missing → `graphify(action=extract)` (defaults to `--code-only`, free, local).
3. Full docs/PDF semantic pipeline: read upstream skill references or run with `code_only=false`
   (needs an LLM backend).

## Actions

status · query · path · explain · affected · report · extract · update

Upstream: https://github.com/Graphify-Labs/graphify
"#;

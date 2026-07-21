//! Install Meta-bundled SKILL packs into `~/.nur/skills/` so the agent
//! discovers them on first launch (also mirrors to `~/.agents/skills`).

use crate::config::muse_home;
use crate::error::Result;
use std::fs;
use std::path::PathBuf;

/// Skills we used to ship and now retire on upgrade. Without this sweep an
/// existing install keeps discovering (and NL-activating) them forever, since
/// the installer only ever writes files. Superseded by `/takeover`.
const RETIRED_SKILLS: &[&str] = &[
    "resume-claude",
    "resume-codex",
    "resume-cursor",
    "resume-grok",
    "resume-nur",
    "resume-meta", // legacy name for resume-nur
];

/// Remove retired skill dirs, plus the SKILL.md that used to make the
/// `resume-session` reader a user-facing skill (the reader itself stays —
/// `chagent` shells out to it).
fn retire_stale_skills(root: &std::path::Path) {
    for name in RETIRED_SKILLS {
        let _ = fs::remove_dir_all(root.join(name));
    }
    let _ = fs::remove_file(root.join("resume-session").join("SKILL.md"));
}

pub fn install_bundled_skills() -> Result<Vec<String>> {
    let root = muse_home().join("skills");
    fs::create_dir_all(&root)?;
    retire_stale_skills(&root);
    let mut installed = Vec::new();

    for (name, body) in BUNDLED {
        let dir = root.join(name);
        fs::create_dir_all(&dir)?;
        let path = dir.join("SKILL.md");
        // Always refresh so skill docs stay in sync with this meta version.
        fs::write(&path, body)?;
        installed.push(name.to_string());
    }

    // Multi-file packs (the resume-session reader, dmmulroy skills, …).
    for (name, files) in MULTI_FILE_PACKS {
        let dir = root.join(name);
        fs::create_dir_all(&dir)?;
        for (filename, body) in *files {
            fs::write(dir.join(filename), body)?;
        }
        if !installed.iter().any(|n| n == name) {
            installed.push((*name).to_string());
        }
    }

    // Also mirror into ~/.agents/skills for cross-framework discovery.
    if let Some(home) = dirs::home_dir() {
        let agents = home.join(".agents").join("skills");
        let _ = fs::create_dir_all(&agents);
        retire_stale_skills(&agents);
        for (name, body) in BUNDLED {
            let dir = agents.join(name);
            let _ = fs::create_dir_all(&dir);
            let _ = fs::write(dir.join("SKILL.md"), body);
        }
        for (name, files) in MULTI_FILE_PACKS {
            let dir = agents.join(name);
            let _ = fs::create_dir_all(&dir);
            for (filename, body) in *files {
                let _ = fs::write(dir.join(filename), body);
            }
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
    ("excalidraw", EXCALIDRAW_SKILL),
];

/// Packs with extra files (Python reader, CORE.md). Paths relative to crate root.
const MULTI_FILE_PACKS: &[(&str, &[(&str, &str)])] = &[
    // Engine for `/takeover`, not a user-facing skill: no SKILL.md, so it is
    // never indexed or activated. `chagent` shells out to `session_reader.py`.
    (
        "resume-session",
        &[
            (
                "CORE.md",
                include_str!("../../skills/resume-session/CORE.md"),
            ),
            (
                "session_reader.py",
                include_str!("../../skills/resume-session/session_reader.py"),
            ),
        ],
    ),
    // dmmulroy/skills (MIT) — baked in so a fresh install ships them.
    (
        "bro",
        &[("SKILL.md", include_str!("../../skills/bro/SKILL.md"))],
    ),
    (
        "scan",
        &[("SKILL.md", include_str!("../../skills/scan/SKILL.md"))],
    ),
    (
        "coding-standards",
        &[(
            "SKILL.md",
            include_str!("../../skills/coding-standards/SKILL.md"),
        )],
    ),
    (
        "tech-spec",
        &[("SKILL.md", include_str!("../../skills/tech-spec/SKILL.md"))],
    ),
    (
        "herdr",
        &[("SKILL.md", include_str!("../../skills/herdr/SKILL.md"))],
    ),
    (
        "prelude",
        &[
            ("SKILL.md", include_str!("../../skills/prelude/SKILL.md")),
            (
                "prelude.ts",
                include_str!("../../skills/prelude/prelude.ts"),
            ),
        ],
    ),
    (
        "cloudflare-composition-root",
        &[
            (
                "SKILL.md",
                include_str!("../../skills/cloudflare-composition-root/SKILL.md"),
            ),
            (
                "EXAMPLES.md",
                include_str!("../../skills/cloudflare-composition-root/EXAMPLES.md"),
            ),
        ],
    ),
    (
        "toolcraft",
        &[("SKILL.md", include_str!("../../skills/toolcraft/SKILL.md"))],
    ),
    // Saurabh-2607/Skills — dark skeuomorphic UI component skill.
    (
        "skeuomorphic-ui",
        &[(
            "SKILL.md",
            include_str!("../../skills/skeuomorphic-ui/SKILL.md"),
        )],
    ),
    // Gateway/proxy prompt-cache hit-rate awareness (inference-point insight).
    (
        "gateway-cache-awareness",
        &[(
            "SKILL.md",
            include_str!("../../skills/gateway-cache-awareness/SKILL.md"),
        )],
    ),
    // Akarso — social posting CLI/MCP (paired with the native `akarso` tool).
    (
        "akarso",
        &[("SKILL.md", include_str!("../../skills/akarso/SKILL.md"))],
    ),
    // OpenSEO — SEO research/audits via MCP (open-source Semrush/Ahrefs alt).
    (
        "openseo",
        &[("SKILL.md", include_str!("../../skills/openseo/SKILL.md"))],
    ),
    // Dialkit — live tuning of interface parameters (multi-framework UI lib).
    (
        "dialkit",
        &[("SKILL.md", include_str!("../../skills/dialkit/SKILL.md"))],
    ),
];

const PLUR_SKILL: &str = r#"---
name: plur
description: "Local-first shared memory for AI agents (engrams + episodes). Use for preferences, corrections, conventions, and session learnings that must persist across tools and sessions."
---

# PLUR — shared agent memory

NurCLI auto-installs `@plur-ai/cli` and provisions `~/.plur/`. Use the **`plur`** tool
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
- Scope project knowledge with `scope` (e.g. `project:nur-cli`); personal prefs → `global`.
- After a user correction, learn it immediately so the next turn benefits.
- PLUR is memory of *assertions*, not a code index — use graphify for code structure.

Upstream: https://github.com/plur-ai/plur · https://plur.ai
"#;

const RUFLO_SKILL: &str = r#"---
name: ruflo
description: "Agent meta-harness: vector memory, swarm coordination, hive-mind, hooks. Use for multi-agent orchestration patterns, semantic memory search, and self-learning trajectories."
---

# Ruflo — agent orchestration harness

NurCLI auto-installs `ruflo` and provisions global vector memory at
`~/.nur/ruflo/memory.db`. Use the **`ruflo`** tool (or `/ruflo`) — no separate init.

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

- Default memory lives under Meta's home (`~/.nur/ruflo/`) so project trees stay clean.
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

const EXCALIDRAW_SKILL: &str = r##"---
name: excalidraw
description: "Hand-drawn Excalidraw diagrams (.excalidraw). Use for architecture, flowcharts, decision trees. Prefer the excalidraw tool over bash."
---

# Excalidraw — diagrams from element JSON

Nur auto-installs `excalidraw-cli` (npm) when Node is available.
Prefer the **`excalidraw`** tool — do not shell out to bash for diagrams.

## When to use

- Architecture diagrams, request/data flows, decision flowcharts
- Not for pixel-perfect UI mockups (use design skills / HTML instead)

## Fast path

1. Build element JSON (camera → dark bg optional → shapes → arrows with bindings)
2. **`excalidraw(action=create, elements=[…], output="docs/foo.excalidraw")`**
   - Writes the file, uploads to excalidraw.com, and **opens the share URL in the user's browser** (default)
   - Do **not** stop at "here's a link" — create already opens it. Tell the user the browser should have opened.
3. Format help: `excalidraw(action=reference)` if stuck on schema
4. `open=false` only when the user asked not to open anything

## CLI defaults (omit these in JSON)

roughness=2 · roundness rounded · fontFamily=1 (handwritten) · strokeWidth=2

## Minimal shapes

```json
{ "type": "rectangle", "id": "r1", "x": 100, "y": 100, "width": 200, "height": 80,
  "backgroundColor": "#1e3a5f", "fillStyle": "solid", "strokeColor": "#4a9eed",
  "label": { "text": "Label", "strokeColor": "#e5e5e5" } }
```

Types: `rectangle` · `ellipse` · `diamond` · `text` · `arrow`

## Labels

Use `label: { "text": "…", "fontSize": 20, "strokeColor": "#e5e5e5" }` on shapes and arrows.
CLI expands labels into bound text elements.

## Arrows + bindings

```json
{ "type": "arrow", "id": "a1", "x": 300, "y": 140, "width": 150, "height": 0,
  "points": [[0,0],[150,0]], "endArrowhead": "arrow", "strokeColor": "#4a9eed",
  "startBinding": { "elementId": "b1", "fixedPoint": [1, 0.5] },
  "endBinding": { "elementId": "b2", "fixedPoint": [0, 0.5] },
  "label": { "text": "edge", "strokeColor": "#a0a0a0" } }
```

fixedPoint: right `[1,0.5]` · left `[0,0.5]` · top `[0.5,0]` · bottom `[0.5,1]`

Also add the arrow id to each shape's `boundElements`.

## Camera (4:3 required)

```json
{ "type": "cameraUpdate", "width": 1200, "height": 900, "x": 0, "y": 0 }
```

Sizes: 400×300 S · 600×450 M · **800×600 L** · **1200×900 XL** · 1600×1200 XXL

## Dark mode background (first element after camera)

```json
{ "type": "rectangle", "id": "darkbg", "x": -4000, "y": -3000, "width": 10000, "height": 7500,
  "backgroundColor": "#1e1e2e", "fillStyle": "solid", "strokeColor": "transparent", "strokeWidth": 0 }
```

Dark fills: `#1e3a5f` blue · `#1a4d2e` green · `#2d1b69` purple · `#5c3d1a` amber · `#5c1a1a` red · `#1a4d4d` teal  
Text: `#e5e5e5` primary · `#a0a0a0` muted

## Drawing order

camera → background zones → shape → its text/label → its arrows → next shape…  
**Bad:** all rects then all arrows. **Good:** progressive per node.

## Sizing rules

- Min labeled shape ~120×60 · gaps 20–30px · fontSize 28 title · 20 labels · 14 min
- No emoji (Excalifont does not render them)

## 3-box flow template

```json
[
  { "type": "cameraUpdate", "width": 1200, "height": 900, "x": 0, "y": 100 },
  { "type": "rectangle", "id": "darkbg", "x": -4000, "y": -3000, "width": 10000, "height": 7500,
    "backgroundColor": "#1e1e2e", "fillStyle": "solid", "strokeColor": "transparent", "strokeWidth": 0 },
  { "type": "rectangle", "id": "b1", "x": 60, "y": 350, "width": 220, "height": 90,
    "backgroundColor": "#1e3a5f", "fillStyle": "solid", "strokeColor": "#4a9eed",
    "label": { "text": "Request", "strokeColor": "#e5e5e5" },
    "boundElements": [{ "id": "a1", "type": "arrow" }] },
  { "type": "arrow", "id": "a1", "x": 280, "y": 395, "width": 200, "height": 0,
    "points": [[0,0],[200,0]], "endArrowhead": "arrow", "strokeColor": "#4a9eed",
    "startBinding": { "elementId": "b1", "fixedPoint": [1, 0.5] },
    "endBinding": { "elementId": "b2", "fixedPoint": [0, 0.5] },
    "label": { "text": "process", "strokeColor": "#a0a0a0" } },
  { "type": "rectangle", "id": "b2", "x": 500, "y": 350, "width": 220, "height": 90,
    "backgroundColor": "#5c3d1a", "fillStyle": "solid", "strokeColor": "#f59e0b",
    "label": { "text": "Server", "strokeColor": "#e5e5e5" },
    "boundElements": [{ "id": "a1", "type": "arrow" }, { "id": "a2", "type": "arrow" }] },
  { "type": "arrow", "id": "a2", "x": 720, "y": 395, "width": 200, "height": 0,
    "points": [[0,0],[200,0]], "endArrowhead": "arrow", "strokeColor": "#22c55e",
    "startBinding": { "elementId": "b2", "fixedPoint": [1, 0.5] },
    "endBinding": { "elementId": "b3", "fixedPoint": [0, 0.5] },
    "label": { "text": "respond", "strokeColor": "#a0a0a0" } },
  { "type": "rectangle", "id": "b3", "x": 940, "y": 350, "width": 220, "height": 90,
    "backgroundColor": "#1a4d2e", "fillStyle": "solid", "strokeColor": "#22c55e",
    "label": { "text": "Response", "strokeColor": "#e5e5e5" },
    "boundElements": [{ "id": "a2", "type": "arrow" }] }
]
```

Upstream: https://github.com/ahmadawais/excalidraw-cli
"##;

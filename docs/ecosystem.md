# Ecosystem

Meta CLI ships with an auto-provisioned knowledge stack.

## Components

| Component | Role |
|-----------|------|
| **Graphify** | Code knowledge graph (`graphify-out/`) — query / path / explain |
| **PLUR** | Shared engram memory across tools and sessions |
| **Ruflo** | Vector memory + swarm / hive-mind patterns |
| **Executor** | MCP / OpenAPI gateway catalog |
| **omp** | [Oh My Pi](https://omp.sh) coding-agent backend — headless `omp -p` runs via the `omp` tool (needs Bun) |
| **Skills** | Progressive packs (design-eng, clone-website, cybersecurity, …) via `skill` |
| **AKM** | Agent knowledge package manager (requires Node.js) |

---

## Auto-provisioning

When you open the TUI, Meta CLI:

1. Snapshots whatever is already provisioned (instant)
2. Spawns a **background thread** that runs `meta ecosystem ensure`
3. The TUI never blocks on npm / uv installs

You can also run provisioning manually:

```bash
meta ecosystem ensure          # install / repair
meta ecosystem ensure --force  # force re-install
meta ecosystem status          # check readiness
```

---

## Graphify

Code knowledge graph stored in `graphify-out/` at your project root.

**What it does:**

- Indexes your codebase into a queryable graph
- Supports path queries (how does A reach B)
- Supports explain queries (what does this function do)
- Integrates with the `graphify` tool in the TUI

**Requires:** uv (or Python 3.10+)

---

## PLUR

Shared engram memory for AI agents.

**What it does:**

- Persists preferences, corrections, conventions across sessions
- Shared across tools (Meta CLI, Claude Code, Cursor, etc.)
- Searchable from the TUI via `/plur`

**Requires:** Node.js 20+

---

## Ruflo

Vector memory + swarm coordination patterns.

**What it does:**

- Semantic memory search across sessions
- Swarm / hive-mind agent coordination
- Hooks for routing tasks between agents
- Searchable from the TUI via `/ruflo`

**Requires:** Node.js 20+

---

## Executor

MCP / OpenAPI gateway catalog.

**What it does:**

- Provides a unified catalog for API integrations
- Routes tool calls to the right MCP server or OpenAPI endpoint
- Policy enforcement for multi-agent tool routing

**Requires:** Node.js 20+

---

## Skills

Progressive skill packs loaded on demand.

**Built-in skills include:**

- `design-eng` — UI polish and animation
- `clone-website` — Website reverse-engineering
- `cybersecurity` — Security investigations and DFIR
- `apple-design` — Apple-style interface design
- And 800+ more...

**Browse skills:**

```text
/skills
```

---

## AKM

Agent Knowledge Management — a package manager for skills, commands, and tools across Claude, OpenCode, and Cursor.

**Requires:** Node.js

---

## Manual management

```bash
meta ecosystem ensure          # install / repair all components
meta ecosystem ensure --force  # force re-install (useful after updates)
meta ecosystem status          # show readiness per component
```

**Environment variables:**

| Variable | Purpose |
|----------|---------|
| `CLAUDE_FLOW_DB_PATH` | Ruflo database path |
| `CLAUDE_FLOW_MEMORY_PATH` | Ruflo home path |

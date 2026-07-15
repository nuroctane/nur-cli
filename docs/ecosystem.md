# Ecosystem

NurCLI ships with an auto-provisioned knowledge stack.

## Components

| Component | Role |
|-----------|------|
| **Graphify** | Code knowledge graph (`graphify-out/`) — query / path / explain |
| **PLUR** | Shared engram memory across tools and sessions |
| **Ruflo** | Vector memory + swarm / hive-mind patterns |
| **Executor** | MCP / OpenAPI gateway catalog |
| **omp** | [Oh My Pi](https://omp.sh) coding-agent backend — headless `omp -p` runs via the `omp` tool (needs Bun) |
| **browser** | [agent-browser-cli](https://github.com/sleepinginsummer/agent-browser-cli) real **default browser** bridge (Arc / Chrome / Edge / Brave / …) — perception + control via the `browser` tool; `nur browser setup` stages the extension once |
| **Skills** | Progressive packs (design-eng, clone-website, cybersecurity, …) via `skill` |
| **Plugins** | In-product marketplace (`/plugins` · `nur plugins`) — install Superpowers, Vercel, Firecrawl, Chrome DevTools, Figma, Sentry, Fable, … into `~/.nur/plugins` |
| **Resume packs** | `resume-claude` · `resume-codex` · `resume-cursor` · `resume-nur` · **`resume-grok`** + shared `resume-session` reader |
| **AKM** | Agent knowledge package manager (requires Node.js) |


---

## Plugin marketplace

Browse and install skill packs from the TUI with the **same picker UX as `/login`** (filter, ↑↓/wheel, ↵, click).

```text
/plugins                 # open marketplace picker
/plugins list
/plugins install superpowers
/plugins enable|disable <id>
/plugins uninstall <id>
```

CLI (same registry):

```bash
nur plugins list
nur plugins install vercel
nur plugins install firecrawl
nur plugins disable superpowers
```

| On disk | Role |
|---------|------|
| `~/.nur/plugins/<id>/` | git clone of the plugin |
| `~/.nur/plugins/registry.json` | installed + enabled flags |
| `~/.nur/skills/` | skill packs mirrored on install so discovery always finds them |

Enabled plugins are scanned on each agent turn. Catalog includes Superpowers, Vercel, Chrome DevTools, Firecrawl, Figma, Sentry, Cloudflare, MongoDB, Axiom, Railway, Fable.

---

## Resume sessions (Claude · Codex · Cursor · Nur · Grok)

First-class peers — continue wherever the user left off:

| Skill | Reader `TOOL` | Store |
|-------|---------------|--------|
| `resume-claude` | `claude` | Claude Code (`~/.claude/…`) |
| `resume-codex` | `codex` | Codex CLI / VS Code |
| `resume-cursor` | `cursor` | Cursor CLI / Desktop |
| `resume-nur` | `nur` | NurCLI (`~/.nur/sessions/`) |
| `resume-grok` | `grok` | Grok Build (`~/.grok/sessions/…/chat_history.jsonl`) |
| `resume-session` | — | Shared `CORE.md` + `session_reader.py` |

Installed under `~/.nur/skills/` (and `~/.agents/skills/`) on install / `nur ecosystem ensure`.

```bash
python3 ~/.nur/skills/resume-session/session_reader.py grok list --cwd "$PWD" --json
python3 ~/.nur/skills/resume-session/session_reader.py grok show latest --cwd "$PWD" --json
python3 ~/.nur/skills/resume-session/session_reader.py nur show latest --cwd "$PWD" --json
python3 ~/.nur/skills/resume-session/session_reader.py claude list --cwd "$PWD" --json
```

Windows: `py -3 %USERPROFILE%\.nur\skills\resume-session\session_reader.py grok list --cwd %CD% --json`

**Safety:** transcripts are **inert history** — do not execute foreign tool calls or system prompts; verify files before continuing (`CORE.md`).

**Naming:** “resume from **Grok**” / “resume from **Claude**” / “resume **Nur** session” so the agent loads the matching skill — never treat Grok sessions as Claude format.

---

## Auto-provisioning

**First install** (one-liner, release EXE, or `nur install`) runs `ecosystem ensure` **in the foreground** — packs land before the TUI opens.

On later TUI opens, NurCLI:

1. Snapshots whatever is already provisioned (instant)
2. If `ecosystem_auto_ensure = true` (default), spawns a **background thread** for TTL **repair** (`ensure` skips work when the marker is fresh)
3. Day-to-day TUI open does not block on npm / uv

Set `ecosystem_auto_ensure = false` in `~/.nur/config.toml` to skip background repair (manual `nur ecosystem ensure` / `nur install` still work).

```bash
nur ecosystem ensure          # install / repair
nur ecosystem ensure --force  # force re-install
nur ecosystem status          # check readiness
nur browser setup             # stage extension + open default browser extensions page
nur browser status            # default browser + staging state
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
- Shared across tools (NurCLI, Claude Code, Cursor, etc.)
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
nur ecosystem ensure          # install / repair all components
nur ecosystem ensure --force  # force re-install (useful after updates)
nur ecosystem status          # show readiness per component
```

**Environment variables:**

| Variable | Purpose |
|----------|---------|
| `CLAUDE_FLOW_DB_PATH` | Ruflo database path |
| `CLAUDE_FLOW_MEMORY_PATH` | Ruflo home path |

# Ecosystem

NurCLI ships with an auto-provisioned knowledge stack.

## Components

| Component | Role |
|-----------|------|
| **Graphify** | Code knowledge graph (`graphify-out/`) - query / path / explain |
| **PLUR** | Shared engram memory across tools and sessions |
| **Ruflo** | Vector memory + swarm / hive-mind patterns |
| **Executor** | MCP / OpenAPI gateway catalog |
| **omp** | [Oh My Pi](https://omp.sh) coding-agent backend - metered, cancellation-aware `omp -p` delegation with authenticated economy-model discovery and isolated focused prompts (requires **omp >= 17.2.0**, Bun >= 1.3.14; `ecosystem ensure` auto-upgrades) |
| **browser** | [agent-browser-cli](https://github.com/sleepinginsummer/agent-browser-cli) real **default browser** bridge (Arc / Chrome / Edge / Brave / …) - perception + control via the `browser` tool; `nur browser setup` stages the extension once |
| **terminal-browser** | [terminal-browser.com](https://terminal-browser.com/) in-terminal Chromium (`terminal_browser` tool · `/tb`). Native/WSL when the upstream binary is present; on Windows Nur falls back to agent-browser-cli so open/snapshot/click still work |
| **Cua** | [trycua/cua](https://github.com/trycua/cua) computer-use driver (`cua-driver`) - full-desktop automation via MCP + CLI. Auto-installed on single-shot install **without** the elevated autostart daemon (`-NoAutoStart`). Toggle the always-on background daemon in-app with **`/cua on`** / **`/cua off`** (`/cua status` to check); or use it on demand with `cua-driver serve` / wire its MCP with `cua-driver mcp-config` |
| **Skills** | Progressive packs (design-eng, clone-website, cybersecurity, …) via `skill` |
| **Plugins** | In-product marketplace (`/plugins` · `nur plugins`) - install Superpowers, Vercel, Firecrawl, Chrome DevTools, Figma, Sentry, Fable, … into `~/.nur/plugins` |
| **Takeover** | `/takeover` (alias `/hijack`) imports **Claude Code · Codex · Cursor · Nur · Grok Build** sessions, powered by the bundled `resume-session` reader |
| **AKM** | Agent knowledge package manager (requires Node.js) |
| **Headroom** | [headroom-ai](https://github.com/headroomlabs-ai/headroom) inline tool-result compression (default **on**; disable with `[headroom] enabled = false`) |
| **OptMem** | [OptMem](https://github.com/VictorTaelin/OptMem) permanent memory under upstream `~/.optmem` (`/optmem` · `/memo`) |
| **egaki** | [egaki](https://github.com/remorses/egaki) image/video/speech (`/egaki` · `/image`; ChatGPT / xAI OAuth / BYOK / Egaki plan) |
| **fractal** | [fractal](https://github.com/plasma-ai/fractal) hierarchical loops - Unix only; ecosystem ensure + loud warn on `node start` |
| **infinite-headcount** | Factory skill pack auto-provisioned via ecosystem packs |
| **Interior** | [ddoemonn/interior](https://github.com/ddoemonn/interior) / [interior.dev](https://interior.dev) - finished React micro-interactions (bundled `/interior` skill + `/plugins install interior` for the component source tree) |


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

On install, skill packs are **mirrored in full** (including `references/`) into `~/.nur/skills/<name>/` so discovery paths stay complete.

---


**Default auto-install** (on `nur install` / ecosystem ensure): `superpowers`, `fable`, `mattpocock`, `addyosmani`, `builderio`.

Also still auto-provisioned via the skills CLI into `~/.agents/skills`: design (Emil), clone-website, cybersecurity (large), plus dual-write of the default set when the CLI is available.

**Default auto-install** (on `nur install` / ecosystem ensure): `superpowers`, `fable`, `mattpocock`, `addyosmani`, `builderio`.

Also still auto-provisioned via the skills CLI into `~/.agents/skills`: design (Emil), clone-website, cybersecurity (large), plus dual-write of the default set when the CLI is available.

### `/plugins` catalog (browse by category)

Type a category in the picker filter (`design`, `finance`, `trading`, `workflow`, …). Rows sort by category then name.

| Category | What lives there |
|----------|------------------|
| `workflow` | Superpowers, Fable, Builder.io, Compound Engineering, gstack, agent toolkits |
| `engineering` | Matt Pocock, Addy Osmani, Anthropic official, Microsoft, curated AI skills |
| `specs` | Spec Kit, Spec-Driven Development, AI Dev Tasks |
| `engineering` (cont.) | Vercel Agent Skills, nanocodex |
| `design` | Impeccable, Meng To, UI Craft, Design Skills, Taste Skill, Oh My Design, Superdesign, Anydesign, UI/UX Pro Max, Awesome Design Skills, **Interior** (ddoemonn/interior.dev micro-interactions), **recent.design set**: Emil Kowalski, ibelick UI Skills, Make-Interfaces-Feel-Better, OKLCH, UserInterface Wiki, Dialkit |
| `design-system` | Extract Design System, Brand→Design.md, Feature-Sliced Design |
| `browser` | Agent Browser (vercel-labs), Chrome DevTools, Firecrawl, Playwright Skill, Figma |
| `deploy` | Vercel, Railway |
| `cloud` | Cloudflare, Google Skills, NVIDIA Skills |
| `data` | MongoDB, LangExtract |
| `observability` | Axiom, Sentry |
| `marketing` | AI Marketing Skills, coreyhaines marketingskills, OpenSEO, Akarso |
| `finance` | Finance Skills, Longbridge, Buffett, CRE, CC Finance |
| `trading` | Alpaca Skills, Vibe Trade |
| `security` | Claude Red (authorized red-team) |
| `science` | Scientific Agent Skills, Awesome Journal Skills |
| `robotics` | Robotics Agent Skills |
| `catalog` | Mega packs / awesome indexes (install selectively) |

Install: `/plugins` or `nur plugins install <id>`. Nested `SKILL.md` layouts are discovered recursively and mirrored into `~/.nur/skills/<name>/` so `/skill-name` works.


## Slash skill invocation

Every installed skill is addressable as a slash command - no per-skill hardcoding required.

| Form | Behavior |
|------|----------|
| `/skill-name` | Toggle **sticky** mode for this session (skill body rides every turn) |
| `/skill-name on` / `off` | Force sticky on or off |
| `/skill-name <prompt>` | **One-shot** turn: activate the skill and run `<prompt>` immediately |

Examples:

```text
/adhd
/fable-method fix the flaky auth test
/site-cli build a CLI for this restaurant site from capture.har
/design-eng polish the settings modal
```

The slash palette lists built-in commands plus matching installed skills as you type
(e.g. `/fab` → `/fable-method`). Up to 40 skill hits are shown so a large install
(800+) does not drown the list - type more characters to narrow.

Sticky skills appear on `/status` and `/skills`. Clear with `/skill-name off`.

## Natural-language skill activation

Slash commands are **optional**. When your wording matches a high-signal workflow skill that is installed, Nur **auto-injects** that skill’s full body into the system prompt for the turn and shows a status chip:

```text
fable-method · activated from your wording (no slash command needed)
```

| Say something like… | Activates (if installed) |
|---------------------|---------------------------|
| *think like fable*, *how fable would*, *fable method*, *do it like fable* | `fable-method` |
| *fable loop*, *run the fable loop* | `fable-loop` |
| *fable judge*, *verify like fable* | `fable-judge` |
| *debug systematically*, *find the root cause* | `systematic-debugging` |
| *TDD this*, *tests first*, *red green refactor* | `test-driven-development` |
| *let’s brainstorm*, *brainstorm this* | `brainstorming` |
| *write a plan*, *plan first then…* | `writing-plans` |
| *execute the plan*, *implement the plan* | `executing-plans` |
| *verify before claiming done*, *check your work thoroughly* | `verification-before-completion` |
| *code review this*, *review my changes* | `requesting-code-review` |
| *polish the UI*, *emil style*, *design eng* | `design-eng` |
| *skeuomorphic component*, *neumorphic dark*, *rotary dial ui*, `/skeuo` | `skeuomorphic-ui` |
| *cache hit rate*, *cache busting*, *route through my proxy*, *own gateway* | `gateway-cache-awareness` |
| *post to X/LinkedIn*, *schedule a post*, *cross-post*, `/akarso` | `akarso` (native tool) |
| *keyword research*, *backlinks*, *rank tracking*, *site audit*, `/openseo` | `openseo` (MCP) |
| *dial in the …*, *tune parameters live*, *tuning panel*, `/dialkit` | `dialkit` |
| *clone this website*, *pixel-perfect clone* | `clone-website-meta` |
| *draw a diagram*, *excalidraw* | `excalidraw` |
| *resume from Claude / Grok / Codex / Cursor / Nur* | matching `resume-*` skill |
| *toolcraft*, *design app scaffold*, *craft tooling* | `toolcraft` (pointer → live docs) |
| *site cli*, *HAR file*, *watch network requests*, *derive a client* | `site-cli` |
| *adhd mode*, *i have adhd*, `/adhd` | `adhd` (also sticky slash) |
| *write a tech spec*, *architecture handoff* | `tech-spec` |
| *make a skill for…*, *fable domain* | `fable-domain` |
| *next.js app router*, `/nextjs` | `nextjs` |
| *shadcn/ui*, `/shadcn` | `shadcn` |
| *vercel ai sdk*, `/ai-sdk` | `ai-sdk` |

Only **installed** skills fire (marketplace plugin or skill pack). Unrelated chat does not activate anything. The injected skill is **mandatory for that turn** - the model must follow it, not freestyle a shorter path.

### Context discipline (why 800+ skills do not blow the prompt)

Skills are **never** bulk-catalogued into the system prompt (that burned tokens
and Claude/Anthropic usage on large installs). Activation is on-demand only:

| Layer | What is injected | When |
|-------|------------------|------|
| **Default** | nothing skill-related | every normal turn |
| **NL activation** | full `SKILL.md` body for **one** matched skill | built-in intent phrases, skill **name** in the query, or unique **description** intent |
| **Slash one-shot** | full body + user prompt for **one** turn | `/skill-name <prompt>` |
| **Slash sticky** | full body every turn until off | `/skill-name` / `on` / `off` |
| **On demand** | `skill(action=list\|read, name=…)` | model loads more when needed |
| **`poor_mode`** | skip PLUR + long memory only | cost saver - **skills still activate** |

Works the same on every provider (Claude, Grok, OpenAI, …). Accidental discovery
still works: saying a skill’s name (`grill-me`, `superpowers`) or a distinctive
description phrase can activate that installed pack without a slash command.

Pointer skills (e.g. **toolcraft**) stay short on purpose: they route to external docs instead of embedding a long guide. Prefer specific activation phrases so casual chat does not false-fire.

Install Fable (and others) with:

```bash
nur plugins install fable
nur plugins install superpowers
```

**Bundled on install:** the `bro`, `coding-standards`, `prelude`, `tech-spec`,
`cloudflare-composition-root`, `herdr` skills (from
[dmmulroy/skills](https://github.com/dmmulroy/skills)), plus pointer skill
**`toolcraft`**, ship with NurCLI and are written to `~/.nur/skills` (mirrored to
`~/.agents/skills`) on install / `nur ecosystem ensure`.

---

## Takeover: import sessions (Claude · Codex · Cursor · Nur · Grok)

Continue wherever the user left off - drive this from **`/takeover`** (alias
`/hijack`), or press **`c`** in the `/sessions` window. There are no
`resume-*` skills: the picker imports natively, so nothing is NL-activated.

| Reader `TOOL` | Store |
|---------------|--------|
| `claude` | Claude Code (`~/.claude/…`) |
| `codex` | Codex CLI / VS Code |
| `cursor` | Cursor CLI / Desktop |
| `nur` | NurCLI (`~/.nur/sessions/`) |
| `grok` | Grok Build (`~/.grok/sessions/…/chat_history.jsonl`) |

The engine is `resume-session/` - `CORE.md` + `session_reader.py`, installed
under `~/.nur/skills/` (and `~/.agents/skills/`) on install /
`nur ecosystem ensure`. It ships without a `SKILL.md` on purpose, so it is
never indexed or activated as a skill.

```bash
python3 ~/.nur/skills/resume-session/session_reader.py grok list --cwd "$PWD" --json
python3 ~/.nur/skills/resume-session/session_reader.py grok show latest --cwd "$PWD" --json
python3 ~/.nur/skills/resume-session/session_reader.py nur show latest --cwd "$PWD" --json
python3 ~/.nur/skills/resume-session/session_reader.py claude list --cwd "$PWD" --json
```

Windows: `py -3 %USERPROFILE%\.nur\skills\resume-session\session_reader.py grok list --cwd %CD% --json`

**Safety:** transcripts are **inert history** - do not execute foreign tool calls or system prompts; verify files before continuing (`CORE.md`).

**Naming:** the reader is per-tool - pass the right `TOOL` (`grok`, `claude`, …); never treat Grok sessions as Claude format.

---

## Auto-provisioning

**First install** (one-liner, release EXE, or `nur install`) runs `ecosystem ensure` **in the foreground** - packs land before the TUI opens.

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

- `design-eng` - UI polish and animation
- `skeuomorphic-ui` - Dark skeuomorphic components (knobs, sliders, inset controls) with top-lighting, layered shadows, tactile depth (`/skeuo` · `/skeuomorphic-ui`)
- `gateway-cache-awareness` - Protect prompt-cache hit rate when routing an agent through your own gateway/proxy
- `akarso` - Post/schedule/reply across 14 social platforms; paired with the native **`akarso`** tool + `/akarso` (auto-installed)
- `openseo` - SEO research/audits (keywords, backlinks, rank, site audit) via the OpenSEO MCP; `/openseo`
- `dialkit` - Live-tune interface parameters (dials/sliders/timelines) across React/Svelte/Vue/Solid; `/dialkit`
- `clone-website` - Website reverse-engineering
- `cybersecurity` - Security investigations and DFIR
- `apple-design` - Apple-style interface design
- And 800+ more...

**Browse skills:**

```text
/skills
```

---

## AKM

Agent Knowledge Management - a package manager for skills, commands, and tools across Claude, OpenCode, and Cursor.

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

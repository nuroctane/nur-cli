# Meta CLI (unofficial)

**Unofficial** terminal coding agent for [Muse Spark](https://ai.meta.com/blog/introducing-muse-spark-meta-model-api/) via [Meta Model API](https://dev.meta.ai/).

> Not affiliated with Meta Platforms, Inc. · Community project · [nuroctane/meta-cli](https://github.com/nuroctane/meta-cli)

```text
meta          # Meta-blue interactive TUI
muse          # alias (same binary)
```

**v0.5.3** — Claude-class agent harness + zero-setup ecosystem (Graphify · PLUR · Ruflo · Executor · skill packs) + a polished TUI (sticky PROMPT banner, click-to-caret, clipboard, draggable scrollbar).

---

## Why Meta CLI

| | |
|--|--|
| **Real Muse Spark agent** | Full custom Rust harness — not a thin wrapper. Modes, tools, sandbox, streaming, cancel. |
| **One-shot install** | One command builds, installs, and provisions the ecosystem. No multi-step “quick starts.” |
| **Opens instantly** | Ecosystem repair runs in the **background** so the TUI never hangs on npm/uv. |
| **Knowledge stack** | Code graph + shared engrams + vector memory + MCP gateway, all auto-wired. |
| **Tasteful TUI** | Colour = meaning, sticky prompt banner, scrollbar you can drag, normal editor keys. |
| **Secrets stay local** | API key only in `~/.muse/auth.json` — never in the repo. |

---

## Install (one shot)

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/nuroctane/meta-cli/main/install.ps1 | iex
```

### macOS / Linux

```bash
curl -fsSL https://raw.githubusercontent.com/nuroctane/meta-cli/main/install.sh | bash
```

That single command will:

1. Install Rust if needed  
2. Clone or update this repo  
3. `cargo build --release`  
4. Put **`meta`** (+ `muse` alias) on your PATH (`~/.local/bin`)  
5. Provision the agent ecosystem (`meta ecosystem ensure`) when Node/uv are available  
6. Install the Orca ADE hook when possible  
7. If `MODEL_API_KEY` is set, save auth under `~/.muse/` **on your machine only**

Then:

```powershell
meta auth login    # paste Meta Model API key → ~/.muse/auth.json only
meta               # open the TUI
```

Key: [dev.meta.ai](https://dev.meta.ai/) → API keys.

### Already cloned (Laboratory / local)

```powershell
cd meta-cli
.\install.ps1          # Windows
# ./install.sh         # macOS / Linux
```

Windows Laboratory clone script (if you use it):

```text
C:\Users\david\Scripts\clone meta-cli main to Laboratory local.cmd
```

Then `cd` into the folder and run `.\install.ps1`.

### Prerequisites (optional but recommended)

| Need | For |
|------|-----|
| **Node.js 20+** | PLUR, Ruflo, Executor, skills CLI, AKM |
| **uv** (or Python 3.10+) | Graphify (`uv tool install graphifyy`) |
| **ripgrep** | Fast `grep` / `glob` (falls back if missing) |

Missing pieces are retried in the background on open, or via `meta ecosystem ensure --force`.

---

## Secrets (important)

| On GitHub | On your PC only |
|-----------|-----------------|
| Source, README, install scripts | `~/.muse/auth.json` (API key) |
| No keys, no `.env`, no sessions | `~/.muse/sessions/`, usage logs, ecosystem marker |

See [SECURITY.md](./SECURITY.md). **Never commit your Meta API key.**

---

## Quick use

```text
meta                         # interactive Meta-blue TUI
meta "fix the bug"          # start with a prompt
meta -c                      # continue last session in this directory
meta -r <session-id>         # resume a session
meta --mode plan "…"         # plan mode (read-only tools)
meta run "…" -y              # headless + auto-approve
meta sessions
meta usage                   # token / cost for ADEs
meta auth status
meta ecosystem status        # graphify · plur · ruflo · executor · packs
meta ecosystem ensure --force
```

Launching from a drive root (`C:\`) auto-picks a safe workspace (git / last session / Laboratory) so tools never run on the entire disk.

---

## Permission modes (live — Shift+Tab)

| Mode | Behavior |
|------|----------|
| **manual** | Reads free; writes / shell need approval (`y` / `a` / `n`) |
| **plan** | Research only — read tools + graphify query/path + plur recall + ruflo search |
| **auto** | Auto-approve tools (`-y` / `--mode auto`) |

Mode lives in a shared atomic: **Shift+Tab applies immediately**, including mid-turn (next tool gate). Statusline shows the live mode.

---

## TUI

### Highlights

- **Streaming** assistant text + violet-italic model thinking (never reads as the answer)
- **Tool cards** colour-coded by family (read / edit / shell / web / git / agent / knowledge)
- **Approvals** — `y` once · `a` always this session · `n` deny  
- **Esc cancel** — freezes stream/thinking; status shows *cancelling…* until work stops  
- **Markdown** rendering, multi-line input, usage + cost + **ctx%** on the statusline  
- **Project instructions** from `MUSE.md`, `AGENTS.md`, or `CLAUDE.md`  
- **Sticky PROMPT banner** — full-width Meta-blue 3-row bar while you scroll older turns  
- **Draggable scrollbar** — right edge of the transcript; click or drag to scrub history  
- **Session picker** — `/resume` or `Ctrl+R` (preview first prompt; `Tab` = this workspace / all)  
- **Slash palette** — type `/` for commands with live filter  
- **Auto-compact** when context pressure is high; `/compact` anytime  

### Keys

| Key | Action |
|-----|--------|
| `↑` `↓` | Scroll the chat (caret only inside a multi-line draft) |
| `PgUp` `PgDn` · `Home` `End` | Page · top · latest |
| Wheel · drag scrollbar | Scroll transcript |
| Click in input | **Place caret** where you click |
| `Ctrl+A` / `Ctrl+C` / `Ctrl+V` / `Ctrl+X` | Select all · copy · paste · cut (system clipboard) |
| `Ctrl+P` `Ctrl+N` (or `Alt+↑/↓`) | Prompt history |
| `Enter` · `\+Enter` / `Ctrl+J` | Send · newline |
| `Shift+Tab` | Cycle permission mode |
| `Ctrl+R` | Resume a session |
| `Esc` | Cancel turn |
| `Ctrl+C` (no selection) ×2 | Quit |
| `Ctrl+L` | Clear transcript view |
| `y` / `a` / `n` | Approve once / always / deny |

### Slash commands

| Command | Purpose |
|---------|---------|
| `/help` | Keys + commands |
| `/mode` `manual\|plan\|auto` | Permission mode (or Shift+Tab) |
| `/plan` `/manual` `/auto` | Shortcuts |
| `/todos` `/memory` `/skills` | Session todos · local memory · skill list |
| `/graphify` … | Knowledge graph status / query / extract |
| `/plur` … | Engram memory learn / recall / inject |
| `/ruflo` … | Vector memory search / store / status |
| `/ecosystem` | Full stack readiness |
| `/compact` | Summarize conversation, free context |
| `/usage` `/cost` | Tokens + est. USD |
| `/model` `/effort` | Model / reasoning effort |
| `/sessions` `/resume` | List / pick sessions |
| `/init` | Generate a `MUSE.md` project guide |
| `/config` | Paths + config dump |
| `/mouse` | Mouse notes (capture always on for caret + scrollbar) |
| `/clear` `/new` `/exit` | Clear view · new session · quit |

### Colour system

Colour is information — a blue spine with hues at matched lightness:

| Family | Hue | Tools |
|--------|-----|-------|
| read | sky blue | `read_file` `list_dir` `grep` `glob` |
| edit | violet | `write_file` `edit_file` `multi_edit` `apply_patch` |
| shell | amber | `bash` |
| web | teal | `web_fetch` `web_search` |
| git | cyan | `git_status` `git_diff` |
| delegate | pink | `agent` |
| knowledge | indigo / orange | `skill` `todo_write` `graphify` `plur` `ruflo` `executor` · `memory` |

System notices use their own glyph + tone: `◈` mode · `✦` plan · `☰` todos · `∑` usage · `⟲` session · `❖` memory.

Statusline segments are individually coloured: **tokens · cost · ctx% · model · mode · state**.

---

## Agent ecosystem (zero extra setup)

One-shot install + background ensure on every open. You do **not** need each project’s own quick-start.

### Runtime systems

| System | What it is | Store / endpoint | In Meta |
|--------|------------|------------------|---------|
| **[Graphify](https://github.com/Graphify-Labs/graphify)** | Code knowledge graph (tree-sitter AST) | `graphify-out/` | tool `graphify` · `/graphify` |
| **[PLUR](https://github.com/plur-ai/plur)** | Shared engram memory (preferences, corrections) | `~/.plur/` | tool `plur` · `/plur` · **auto-inject** each turn |
| **[Ruflo](https://github.com/ruvnet/ruflo)** | Vector memory + swarm harness | `~/.muse/ruflo/` (global, no project pollution) | tool `ruflo` · `/ruflo` |
| **[Executor](https://executor.sh/docs)** | MCP gateway for OpenAPI / GraphQL / MCP | local `:4788/mcp` | tool `executor` |
| **[skills](https://www.npmjs.com/package/skills)** CLI | Open agent skills installer | `~/.agents/skills/` | used by `ecosystem ensure` |
| **[akm-cli](https://www.npmjs.com/package/akm-cli)** | Agent knowledge package manager | multi-agent | skill `akm-manager` |

### Skill packs (catalog routers + full packs on disk)

| Pack | Source | What you get |
|------|--------|--------------|
| Design engineering | [emilkowalski/skills](https://github.com/emilkowalski/skills) | Motion/UI taste — easings, review tables, improve-animations |
| Clone website | [JCodesMore/ai-website-cloner-template](https://github.com/JCodesMore/ai-website-cloner-template) | Pixel-perfect reverse-engineering pipeline |
| Cybersecurity | [mukul975/Anthropic-Cybersecurity-Skills](https://github.com/mukul975/Anthropic-Cybersecurity-Skills) | 817 MITRE/NIST-mapped playbooks — load **one** by name |
| OpenCode catalog | [awesome-opencode](https://github.com/awesome-opencode/awesome-opencode) | Curated plugin *patterns* (Meta is not OpenCode) |
| Context pruning | [Opencode-DCP](https://github.com/Opencode-DCP/opencode-dynamic-context-pruning) | DCP ideas + Meta native `/compact` auto-compact |

```powershell
meta ecosystem status
meta ecosystem ensure --force   # repair / first-time full provision
```

In the TUI: `/ecosystem` · `/plur` · `/ruflo` · `/graphify` · `/skills`

### What to use when

| Need | Use |
|------|-----|
| “What calls X?” / architecture | **graphify** (`query` / `path` / `explain`) |
| Remember preferences & corrections | **plur** (auto-injected) |
| Semantic pattern memory / swarm status | **ruflo** |
| External SaaS / APIs over MCP | **executor** |
| UI / motion polish | skill **design-eng** / emil packs |
| Clone a live site into Next.js | skill **clone-website-meta** |
| Security investigation | skill **cybersecurity** → specific playbook |
| Long-session context pressure | `/compact` + **context-pruning** |
| Local markdown scratchpad | built-in `memory` (`~/.muse/memory.md`) |

---

## Safety & tools

- **Workspace sandbox** — paths cannot escape session cwd (case + symlink/junction aware); refuse filesystem-root workspaces  
- **Shell** — Git Bash → pwsh → PowerShell → cmd (labeled; `MUSE_SHELL` override); Esc/timeout kills the process tree  
- **grep / glob** — ripgrep-first; hard-excludes `node_modules` / `target` / … + time budget  
- **apply_patch** — unified-diff multi-hunk edits; ambiguous context refused  
- **web_fetch** — public HTTP(S) only; every redirect hop DNS-validated + IP-pinned; size-capped  
- **web_search** — DuckDuckGo, no API key  
- **git_status / git_diff** — approval-free repo inspection  
- **skills** — `~/.muse/skills/`, `~/.agents/skills/`, project skills; agent loads via `skill` tool  
- **subagents** — `agent` explore/general; usage rolled into the parent session  
- **Windows ecosystem spawn** — npm `.cmd` shims resolved correctly so ensure actually installs Executor / skills / etc.

### Built-in tools

```text
read_file · list_dir · write_file · edit_file · multi_edit · apply_patch · bash
grep · glob · web_fetch · web_search · git_status · git_diff
graphify · plur · ruflo · executor · skill · memory · todo_write · submit_plan · agent
```

---

## ADE / Orca

Usage for host tools (**never** includes your API key):

| Path | Purpose |
|------|---------|
| `~/.muse/status.json` | Live tokens / est. USD / model / state |
| `~/.muse/usage.jsonl` | Per-request log |
| `~/.muse/ade.json` | Discovery manifest |
| `~/.muse/ecosystem.json` | Ecosystem ensure marker (CLIs + packs) |

```powershell
meta install-hook
orca terminal create --worktree active --command "meta" --title "Meta CLI" --json
```

---

## Config

`~/.muse/config.toml` (created on first run):

```toml
model = "muse-spark-1.1"
base_url = "https://api.meta.ai/v1"
reasoning_effort = "high"
max_turns = 40
stream = true
context_window = 1000000
# mouse = false   # preference flag; capture is always on for caret + scrollbar
```

Env overrides (user-level is fine): `MODEL_API_KEY` / `MUSE_API_KEY` / `META_API_KEY`, `META_MODEL`, `META_CWD`, `MUSE_SHELL`.

---

## Model API

Responses API (`POST /v1/responses`), default **`muse-spark-1.1`**, streaming + reasoning continuity.  
Docs: https://dev.meta.ai/docs/getting-started/overview

---

## Development

```powershell
cd meta-cli
cargo test
cargo build --release
# install to ~/.local/bin
.\install.ps1
```

---

## License

MIT — unofficial community software; not a Meta product.

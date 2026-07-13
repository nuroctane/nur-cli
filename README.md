# Meta CLI (unofficial)

**Fully loaded terminal coding agent** for [Meta Model API](https://dev.meta.ai/) — not a thin wrapper. Custom Rust harness, dense Meta-blue TUI, native tools, knowledge stack, hardened sandbox. Default model [Muse Spark](https://ai.meta.com/blog/introducing-muse-spark-meta-model-api/); any model id via `--model` / `/model`.

> Not affiliated with Meta Platforms, Inc. · Community · [nuroctane/meta-cli](https://github.com/nuroctane/meta-cli)

```text
meta          # primary — Meta-blue interactive TUI
muse          # legacy alias (same binary)
```

**v0.5.13** — Production-minded agent harness, end to end:

| Surface | What ships |
|---------|------------|
| **TUI** | Streaming · duration chips · expandable thought/tool cards · click-to-peek · drag-select · always-on scrollbar · ↓ End chip · sticky prompt · sessions browser · approval mini-diff |
| **Agent** | Manual / plan / auto modes · tool loop · subagents · todos · auto-compact · Esc cancel · Shift+Tab mid-turn · prompt-cache keys |
| **Tools** | read · edit · multi_edit · apply_patch · bash · web · git · graphify · plur · ruflo · executor · skill · memory · agent |
| **Ecosystem** | Graphify · PLUR · Ruflo · Executor · AKM · **800+ skills** — auto-provisioned in the background |
| **Hardening** | Sandbox · bash denylist · SSRF blocks · atomic session/auth IO · API retries · install SHA-256 · `meta doctor` |
| **ADE** | Live `status.json` / `usage.jsonl` · window title `🔵 meta · prompt…` · Orca hook |

---

## Why Meta CLI

| | |
|--|--|
| **Real agent, not a wrapper** | Custom Rust harness: modes, tools, sandbox, streaming, cancel, subagents, auto-compact |
| **One-shot install** | Build · PATH · ecosystem provision · Orca hook · optional auth — one script |
| **Opens instantly** | Ecosystem repair runs in the **background**; TUI never blocks on npm/uv |
| **Knowledge stack, auto-wired** | Code graph · shared engrams · vector memory · MCP gateway · skill packs |
| **Tasteful, dense TUI** | Duration chips, expandable thoughts/tools, click-to-peek, drag-select, sticky prompt, sessions browser |
| **Hardened by default** | Sandbox, bash denylist, SSRF blocks, atomic writes, API retries, SHA-256 install verify |
| **ADE-ready** | Live `status.json` / usage log for Orca-style panels; window title `🔵 meta · prompt…` |
| **Secrets stay local** | API key only in `~/.muse/auth.json` — never in the repo |

---

## Feature map

### Agent harness
- Meta Model API **Responses** streaming + reasoning effort (`minimal` → `xhigh`)
- **Manual / plan / auto** permission modes — **Shift+Tab** applies mid-turn
- Tool loop with parallel-safe tools, approval gates, Esc cancel
- **Subagents**, todos, plan mode (`submit_plan`), auto-compact under context pressure
- Project instructions: `MUSE.md` · `AGENTS.md` · `CLAUDE.md`
- Session resume (`-c`, `-r`, `/sessions`) with prompt-first picker
- **Prompt cache key** per session (helps surface `cached_tokens` / cheaper multi-turn)

### Tools (native)
| Family | Tools |
|--------|--------|
| read | `read_file` `list_dir` `grep` `glob` |
| edit | `write_file` `edit_file` `multi_edit` `apply_patch` |
| shell | `bash` (hardened denylist + timeout) |
| web | `web_search` `web_fetch` (SSRF / obfuscated-IP blocks) |
| git | `git_status` `git_diff` |
| knowledge | `graphify` `plur` `ruflo` `executor` `skill` `memory` |
| agent | `todo_write` `submit_plan` `agent` |

### Ecosystem (auto-provisioned)
| Piece | Role |
|-------|------|
| **Graphify** | Code knowledge graph (`graphify-out/`) — query / path / explain |
| **PLUR** | Shared engram memory across tools/sessions |
| **Ruflo** | Vector memory + swarm/hive patterns |
| **Executor** | MCP / OpenAPI gateway catalog |
| **Skills** | Progressive packs (incl. large cyber skill set) via `skill` tool |
| **AKM** | Agent knowledge package manager (when Node available) |

### TUI (Meta-blue)
- Streaming assistant · violet **thought** cards · colour-coded **tool** cards  
- **Duration chips** on thoughts, tools, and end-of-turn (`took …` · `thought …`)  
- Cards **collapsed by default** · click `▸` to expand · **click-to-peek** dialogue  
- **Drag text to select** (highlight + auto-copy) · **drag scrollbar** always on  
- Click **↓ N · End** to jump to latest  
- Sticky PROMPT banner · click-to-caret · clipboard · sessions modal  
- Approval modal with **mini unified-diff** for edits  
- Per-cell **wrap cache** so long sessions stay snappy while animating  
- Splash subtitle spells the stack: *fully loaded · TUI · tools · Graphify/PLUR/Ruflo · 800+ skills*

### Reliability & safety
- Atomic writes for sessions, status, auth, config, history  
- API client **retries** with backoff (429 / 5xx / flaky streams)  
- Real **process timeouts** for graphify / ecosystem CLIs (kill on hang)  
- Config validation · auth key hygiene · `meta doctor`  
- Install scripts verify **SHA-256** of the installed binary  

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

That command will:

1. Install Rust if needed  
2. Clone or update this repo  
3. `cargo build --release`  
4. Install **`meta`** (+ `muse` alias) to `~/.local/bin` and **verify SHA-256**  
5. `meta ecosystem ensure` when Node/uv are available  
6. Orca ADE hook when possible  
7. Save auth if `MODEL_API_KEY` is set (**machine-local only**)  

```powershell
meta auth login    # paste Meta Model API key → ~/.muse/auth.json only
meta               # open the TUI
meta doctor        # health check
```

Key: [dev.meta.ai](https://dev.meta.ai/) → API keys.

### Already cloned (Laboratory / local)

```powershell
cd meta-cli
.\install.ps1          # Windows
# ./install.sh         # macOS / Linux
```

### Prerequisites (optional but recommended)

| Need | For |
|------|-----|
| **Node.js 20+** | PLUR, Ruflo, Executor, skills CLI, AKM |
| **uv** (or Python 3.10+) | Graphify |
| **ripgrep** | Fast `grep` / `glob` (falls back if missing) |

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
meta doctor                  # auth · config · ecosystem · PATH · sha256
```

Launching from a drive root (`C:\`) auto-picks a safe workspace (git / last session / Laboratory).

---

## Permission modes (live — Shift+Tab)

| Mode | Behavior |
|------|----------|
| **manual** | Reads free; writes / shell need approval (`y` / `a` / `n`) |
| **plan** | Research only — read tools + graphify query + plur/ruflo search |
| **auto** | Auto-approve tools (`-y` / `--mode auto`) |

---

## TUI

### Highlights

- **Duration chips** — `took 1.2s` on thoughts, tools, and finished turns (`thought …` always posted)  
- **Collapsed cards** — click `▸` or press `e` to expand; **click-to-peek** for full content  
- **Drag-select** chat text (auto-copy) · **drag scrollbar** · click **↓ End**  
- **Streaming** + violet thinking · colour-coded tools · sticky PROMPT banner  
- **Approvals** with mini **diff preview** for edits · Esc cancel  
- **Sessions** browser: `/sessions` = `/resume` = `Ctrl+R`  
- Markdown input, usage + cost + **ctx%**, model-agnostic banner  

### Keys

| Key | Action |
|-----|--------|
| `↑` `↓` · wheel · drag scrollbar | Scroll transcript |
| **Drag on chat text** | Select + auto-copy |
| **Click `↓ N · End`** | Jump to latest |
| Click card / `▸` | Peek / expand |
| `p` / `e` (empty input) | Peek latest / expand |
| `Shift+Tab` | Cycle permission mode |
| `Ctrl+R` | Sessions browser |
| `y` / `a` / `n` | Approve once / always / deny |
| `Esc` | Close peek, then cancel turn |
| `Ctrl+C` | Copy selection, or double-tap quit |

### Slash commands

| Command | Purpose |
|---------|---------|
| `/help` | Keys + commands |
| `/mode` `/plan` `/manual` `/auto` | Permission |
| `/todos` `/memory` `/skills` | Session state |
| `/graphify` `/plur` `/ruflo` `/ecosystem` | Knowledge stack |
| `/compact` `/usage` `/model` `/effort` | Context & model |
| `/sessions` `/resume` | Same sessions browser |
| `/init` `/config` `/mouse` `/clear` `/new` `/exit` | Project & shell |

### Colour system

| Family | Hue | Tools |
|--------|-----|-------|
| read | sky | `read_file` `list_dir` `grep` `glob` |
| edit | violet | `write_file` `edit_file` `multi_edit` `apply_patch` |
| shell | amber | `bash` |
| web | teal | `web_fetch` `web_search` |
| git | cyan | `git_status` `git_diff` |
| knowledge | indigo / orange | `graphify` `plur` `ruflo` `skill` `memory` … |

---

## ADE / Orca

| Path | Role |
|------|------|
| `~/.muse/status.json` | Live tokens · cost · model · state |
| `~/.muse/usage.jsonl` | Per-request log |
| `~/.muse/ade.json` | Discovery manifest |
| Window title | `🔵 meta · <first prompt…>` |

```text
meta install-hook
orca terminal create --command meta
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
mouse = true
```

---

## License

MIT — see [LICENSE](./LICENSE).

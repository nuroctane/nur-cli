# Meta CLI (unofficial)

**Unofficial** terminal coding agent for [Muse Spark](https://ai.meta.com/blog/introducing-muse-spark-meta-model-api/) via [Meta Model API](https://dev.meta.ai/).

> Not affiliated with Meta Platforms, Inc. · Community project · [nuroctane/meta-cli](https://github.com/nuroctane/meta-cli)

The command you run is **`meta`** (alias: `muse` for compatibility).

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
2. Clone this repo (or update it)  
3. `cargo build --release`  
4. Put **`meta`** (and `muse` alias) on your PATH (`~/.local/bin`)  
5. Install the Orca ADE hook when possible  
6. If `MODEL_API_KEY` is already set, save auth under `~/.muse/` **on your machine only**

Then:

```powershell
meta auth login    # paste your Meta Model API key (stored only in ~/.muse)
meta               # open the TUI
```

Get a key: [dev.meta.ai](https://dev.meta.ai/) → API keys.

### Already cloned?

```powershell
cd meta-cli
.\install.ps1          # Windows
# ./install.sh         # macOS / Linux
```

### Windows: Laboratory clone script

```text
C:\Users\david\Scripts\clone meta-cli main to Laboratory local.cmd
```

Then `cd` into the folder and run `.\install.ps1`.

---

## Secrets (important)

| On GitHub | On your PC only |
|-----------|-----------------|
| Source code, README, install scripts | `~/.muse/auth.json` (API key) |
| No API keys, no `.env`, no sessions | `~/.muse/sessions/`, usage logs |

`.gitignore` excludes `.env*`, `auth.json`, `.muse/`, and common key material.  
See [SECURITY.md](./SECURITY.md).

**Never commit your Meta API key. Never paste it into issues or PRs.**

---

## Quick use

```
meta                      # interactive Meta-blue TUI
meta "fix the bug"       # start with a prompt
meta -c                   # continue last session in this directory
meta -r <session-id>      # resume a session
meta --mode plan "…"      # plan mode (read-only tools)
meta run "…" -y           # headless + auto-approve
meta sessions
meta usage                # token / cost for ADEs
meta auth status
```

### Permission modes (live — Shift+Tab)

| Mode | Behavior |
|------|----------|
| **manual** | Reads free; writes/shell need approval (`y` / `a` / `n`) |
| **plan** | Read-only (`read_file` / `grep` / `glob` / `web_fetch`) |
| **auto** | Auto-approve tools (`-y` / `--mode auto`) |

Mode is stored in a shared atomic: toggling applies **immediately**, including mid-turn (next tool gate).

### TUI highlights

- Live streaming · tool approvals · slash commands (`/help` `/mode` `/plan` `/auto`)  
- Esc **cancels** the turn: stream/thinking freeze; status shows *cancelling…* until work stops  
- Markdown · multi-line input · usage + **mode** on the statusline  
- Project instructions from `MUSE.md`, `AGENTS.md`, or `CLAUDE.md`  

### Safety & tools (v0.4.1)

- **Workspace sandbox** — paths cannot escape session cwd (junction/symlink-aware); refuse filesystem-root workspaces  
- **Shell** — prefers Git Bash → pwsh → PowerShell → cmd (labeled in tool output; set `MUSE_SHELL`); Esc/timeout kills the whole process tree  
- **grep/glob** — ripgrep when installed; hard-excludes `node_modules`/`target`/… + time budget  
- **apply_patch** — unified-diff multi-hunk edits; ambiguous context refused  
- **web_fetch** — public HTTP(S) only: every redirect hop DNS-validated + IP-pinned, size-capped  
- **web_search** — DuckDuckGo, no API key  
- **git_status / git_diff** — approval-free repo inspection (diff|staged|log|show)  
- **skills** — SKILL.md packs in `~/.muse/skills/` or `<repo>/.muse/skills/`; agent loads them via the `skill` tool  
- **subagents** — scoped usage tracking, tokens rolled up into the parent session

---

## ADE / Orca

Usage is written for host tools (never includes your API key):

| Path | Purpose |
|------|---------|
| `~/.muse/status.json` | Live tokens / est. USD / model / state |
| `~/.muse/usage.jsonl` | Per-request log |
| `~/.muse/ade.json` | Discovery manifest |

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
```

## Tools

`read_file` · `list_dir` · `write_file` · `edit_file` · `multi_edit` · `apply_patch` · `bash` ·
`grep` · `glob` · `web_fetch` · `web_search` · `git_status` · `git_diff` · `skill` · `memory` ·
`todo_write` · `submit_plan` · `agent`

## Model API

Responses API (`POST /v1/responses`), `muse-spark-1.1`, streaming + reasoning continuity.  
Docs: https://dev.meta.ai/docs/getting-started/overview

## License

MIT — unofficial community software; not a Meta product.

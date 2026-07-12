# Meta CLI (unofficial)

**Unofficial** terminal coding agent for [Muse Spark](https://ai.meta.com/blog/introducing-muse-spark-meta-model-api/) via [Meta Model API](https://dev.meta.ai/).

> Not affiliated with Meta Platforms, Inc. Community project under `nuroctane/meta-cli`.

The installed command is **`muse`** (Muse Spark agent).

```
muse                      # interactive TUI (Meta blue theme)
muse "fix the bug"        # start with a prompt
muse -c                   # continue last session in this directory
muse -r <session-id>      # resume a session
muse run "…" -y           # headless agent turn (streams output)
muse sessions             # list sessions
muse usage                # token / cost snapshot for ADEs
muse auth login
muse install-hook         # install Orca ADE hook
```

## TUI

- **Live streaming** — assistant text and reasoning summaries render token-by-token
- **Tool approvals** — mutating tools (`bash`, `write_file`, `edit_file`) pop a modal:
  `y` allow once · `a` always allow this session · `n` deny. Read-only tools run freely.
  `-y` / `--yes` auto-approves everything (headless loops).
- **Slash commands** with autocomplete palette (type `/`):
  `/help` `/clear` `/new` `/compact` `/usage` `/cost` `/model` `/effort`
  `/sessions` `/resume` `/init` `/config` `/exit`
- **Statusline** — live usage bottom-left (`tokens · $cost · ctx %`), model/session/state bottom-right
- **Markdown rendering** — headings, bullets, fenced code, inline bold/italic/code
- **Input** — multi-line (`\`+Enter or Ctrl+J), ↑/↓ history (persisted), readline keys
  (Ctrl+A/E/U/W, Ctrl+←/→), bracketed paste
- **Esc interrupts** a running turn; prompts typed while busy are queued
- Ctrl+C twice quits · PgUp/PgDn scroll · Ctrl+L clear

`/init` asks the agent to generate a `MUSE.md`. Project instructions are loaded automatically
from `MUSE.md`, `AGENTS.md`, or `CLAUDE.md` (first found) in the workspace root.

## Install

### Windows

```powershell
cd path\to\meta-cli
.\install.ps1
```

### macOS / Linux

```bash
./install.sh
# or
cargo install --path .
```

Requires [Rust](https://rustup.rs) / cargo.

## Auth

```bash
export MODEL_API_KEY="your-key"   # or MUSE_API_KEY
muse auth login                   # stores in ~/.muse/auth.json
muse auth status
```

Env overrides the file. Get a key at [dev.meta.ai](https://dev.meta.ai/).

## ADE / Orca usage (your Meta API key)

Meta CLI writes **machine-readable usage** so host tools can show tokens/cost for Meta/Muse traffic:

| Path | Purpose |
|------|---------|
| `~/.muse/status.json` | Live snapshot: model, session, tokens, est. USD, state |
| `~/.muse/usage.jsonl` | Append-only per-request log |
| `~/.muse/ade.json` | Discovery manifest for ADEs |
| `~/.muse/latest_session.json` | Last active session pointer |
| `~/.muse/sessions/<id>.json` | Full session + cumulative usage |

Process env (for hooks / child tools):

- `MUSE_STATUS_PATH`, `MUSE_USAGE_LOG_PATH`, `MUSE_HOME`
- `MUSE_SESSION_ID`, `MUSE_MODEL`, `MUSE_PROVIDER=meta`
- `MUSE_USAGE_INPUT_TOKENS` / `OUTPUT` / `TOTAL` / `MUSE_USAGE_COST_USD`

```bash
muse usage
muse install-hook    # ~/.orca/agent-hooks/muse-hook.cmd
```

### Orca

```powershell
muse install-hook
orca terminal create --worktree active --command "muse" --title "Meta CLI" --json
```

Poll `%USERPROFILE%\.muse\status.json` for live Meta token usage tied to the user's key.
The same numbers are shown live in the TUI statusline (bottom-left).

## Config

`~/.muse/config.toml`:

```toml
model = "muse-spark-1.1"
base_url = "https://api.meta.ai/v1"
reasoning_effort = "high"
max_turns = 40
stream = true
context_window = 1000000   # for the ctx% meter
```

## Tools

`read_file` · `write_file` · `edit_file` · `bash` · `grep` · `glob`

Read-only tools are auto-approved; mutating tools require approval in the TUI
(or `-y` / `--yes` for unattended runs).

## Model API

**Responses API** (`POST /v1/responses`) with:

- `store: false`
- `include: ["reasoning.encrypted_content"]`
- SSE streaming (`stream: true`) with graceful fallback to plain JSON
- default model `muse-spark-1.1`

Docs: https://dev.meta.ai/docs/getting-started/overview

## License

MIT — unofficial community software; not a Meta product.

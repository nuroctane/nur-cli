# Configuration

Meta CLI is configured via a TOML file and optional rule/hook files, plus environment variables.

## Config file

The config file lives at `~/.meta/config.toml` and is created on first run.

```toml
# Active provider id from the catalog (set by TUI /login)
provider = "meta"
model = "muse-spark-1.1"
base_url = "https://api.meta.ai/v1"
reasoning_effort = "high"
max_turns = 40
stream = true
context_window = 1000000

# Tool results larger than this spill to ~/.meta/tool-results/ (0 = unlimited)
tool_result_max_chars = 12000

# Optional hard stops (omit or leave unset for unlimited)
# max_session_cost_usd = 5.0
# max_session_tokens = 500000

# Compaction (auto under context pressure, or /compact)
compact_keep_user_turns = 4
compact_tool_body_max_chars = 800

# Cost-saver prompt: skip PLUR inject, skills catalog, long memory (tools stay full)
poor_mode = false

# Background TTL pack repair on later TUI opens (first install is foreground)
ecosystem_auto_ensure = true
```

### Settings reference

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `provider` | string | `meta` | Catalog id (`meta`, `openai`, `openrouter`, `ollama`, …). Set by TUI **`/login`** with matching `base_url` + `model` |
| `model` | string | `muse-spark-1.1` | Model id for the active provider |
| `base_url` | string | `https://api.meta.ai/v1` | API base (no trailing path); providers use Responses or Chat Completions under this base |
| `reasoning_effort` | string | `high` | Reasoning depth: `minimal`, `low`, `medium`, `high`, `xhigh` |
| `max_turns` | integer | `40` | Max agent turns per prompt (range: 1–200) |
| `stream` | bool | `true` | Stream API responses |
| `context_window` | integer | `1000000` | Model context window in tokens (range: 1000–2000000) |
| `tool_result_max_chars` | integer | `12000` | Max inline tool output chars; larger results spill to disk (`0` = unlimited) |
| `max_session_cost_usd` | float? | unset | Hard stop when session estimated cost reaches this USD amount |
| `max_session_tokens` | integer? | unset | Hard stop when session `total_tokens` reaches this value |
| `compact_keep_user_turns` | integer | `4` | Recent user turns kept after compaction |
| `compact_tool_body_max_chars` | integer | `800` | When compacting, truncate older tool bodies to this many chars (`0` = leave intact) |
| `poor_mode` | bool | `false` | Skip PLUR auto-inject, skills catalog, and long memory in the system prompt |
| `ecosystem_auto_ensure` | bool | `true` | Background TTL **repair** of packs on later TUI opens (first install is foreground via one-liner / EXE / `meta install`); set `false` to skip repair |

### Reasoning effort levels

| Level | Behaviour |
|-------|-----------|
| `minimal` | Fastest, shallowest reasoning |
| `low` | Light reasoning |
| `medium` | Balanced |
| `high` | Deep reasoning (default) |
| `xhigh` | Maximum reasoning depth |

### Session budgets (interactive)

In the TUI you can set ceilings without editing the file:

```text
/budget                 # show ceilings + spend so far
/budget cost 2.5        # hard stop at ~$2.50 this process
/budget tokens 500000
/budget clear           # unlimited this process
/budget save            # write current ceilings into config.toml
```

When a ceiling is hit, the agent **refuses new API turns** with a clear status message.

---

## Permission rules

Optional file: **`~/.meta/permissions.toml`** (and/or project **`.meta/permissions.toml`** — both are merged).

```toml
# Patterns: "tool" or "tool:glob"  (* = any sequence)
# Order: deny > ask > allow > mode default
# Plan mode still blocks code authoring / VCS mutation even if allow matches.

deny  = ["bash:rm -rf *", "bash:git push --force*"]
ask   = ["bash:npm publish*"]
allow = ["bash:git status*", "bash:cargo test*"]
```

| Decision | Effect |
|----------|--------|
| **deny** | Always block (including auto mode) |
| **ask** | Force an approval prompt (even in auto) |
| **allow** | Skip approval in manual (plan structural blocks still win) |

Reload without restart: `/permissions reload`.

---

## Tool hooks

Optional file: **`~/.meta/hooks.toml`**.

```toml
pre_tool = "echo pre $META_TOOL"
post_tool = ""
timeout_ms = 5000
```

Environment for hook commands:

| Env | Meaning |
|-----|---------|
| `META_TOOL` | Tool name |
| `META_ARGS_JSON` | Raw JSON args |
| `META_CWD` | Workspace cwd |
| `META_SESSION` | Session id |

Non-zero **pre_tool** exit blocks the tool. Missing file = no hooks. Check status with `/hooks`.

---

## Environment variables

### API and model

| Variable | Purpose |
|----------|---------|
| `META_API_KEY` | API key (preferred) |
| `MODEL_API_KEY` | API key (alternative) |
| `MUSE_API_KEY` | API key (legacy) |
| `META_BASE_URL` | Override API base URL (self-hosted Ollama/vLLM/LiteLLM/gateways) |
| `META_MODEL` | Override model id |
| `MUSE_MODEL` | Override model id (legacy) |

### Paths

| Variable | Purpose |
|----------|---------|
| `META_HOME` | Override data home (default `~/.meta`) |
| `MUSE_HOME` | Override data home (legacy) |
| `META_CWD` | Default working directory |

### Status and usage

| Variable | Purpose |
|----------|---------|
| `META_STATUS_PATH` | Path to live status file |
| `META_USAGE_LOG_PATH` | Path to usage log |
| `META_SESSION_ID` | Current session id |
| `META_PROVIDER` | Provider identifier (set to `meta`) |

### Update control

| Variable | Purpose |
|----------|---------|
| `DISABLE_AUTOUPDATER` | Set to `1` to disable background auto-updates |
| `DISABLE_UPDATES` | Set to `1` to block all update paths |

### Ecosystem

| Variable | Purpose |
|----------|---------|
| `CLAUDE_FLOW_DB_PATH` | Ruflo database path |
| `CLAUDE_FLOW_MEMORY_PATH` | Ruflo home path |
| `USE_BUILTIN_RIPGREP` | Set to `0` to use system ripgrep |

---

## Data home

All Meta CLI state lives under `~/.meta/` by default:

```
~/.meta/
├── auth.json           # API key
├── config.toml         # Configuration
├── permissions.toml    # Optional allow/deny/ask rules
├── hooks.toml          # Optional pre/post tool hooks
├── meta.log            # Tracing (not painted into the TUI)
├── status.json         # Live token/cost status
├── usage.jsonl         # Per-request usage log
├── ade.json            # ADE discovery manifest
├── memory.md           # Cross-session memory journal (quick-memory #notes)
├── history.jsonl       # Prompt history
├── sessions/           # Session files (UUID.json + .json.bak / .precompact.bak)
├── tool-results/       # Spilled oversized tool outputs
├── browser-extension/  # Staged tmwd_cdp_bridge for browser tool
├── skills/             # Installed skill packs
├── ruflo/              # Vector memory database
└── skill-packs/        # Skill pack metadata
```

Override with `META_HOME` (or legacy `MUSE_HOME`).

---

## Legacy migration

If you upgraded from a pre-0.5.14 build, Meta CLI automatically gap-fills missing files from `~/.muse/` into `~/.meta/`. Existing files are never overwritten. When the same session id exists in both homes, the **richer** copy (more tokens / newer) wins.

`meta auth logout` clears auth from both `~/.meta/` and legacy `~/.muse/`.

---

## Project instructions

Meta CLI reads project-level instruction files from your working directory:

| File | Purpose |
|------|---------|
| `META.md` | Primary project instructions |
| `AGENTS.md` | Agent conventions (shared with other tools) |
| `CLAUDE.md` | Legacy instructions (still loaded) |
| `MUSE.md` | Legacy instructions (still loaded) |

These are loaded at session start and prepended to the system prompt.

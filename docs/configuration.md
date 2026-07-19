# Configuration

NurCLI is configured via a TOML file and optional rule/hook files, plus environment variables.

## Config file

The config file lives at `~/.nur/config.toml` and is created on first run.

```toml
# Active provider id from the catalog (set by TUI /login)
provider = "meta"
model = "muse-spark-1.1"
base_url = "https://api.meta.ai/v1"
reasoning_effort = "high"
# 0 = unlimited agent rounds per prompt (default). Set a number to cap.
max_turns = 0
stream = true
context_window = 1000000

# Tool results larger than this spill to ~/.nur/tool-results/ (0 = unlimited)
tool_result_max_chars = 12000

# Optional hard stops (omit or leave unset for unlimited)
# max_session_cost_usd = 5.0
# max_session_tokens = 500000

# Compaction (auto under context pressure, or /compact)
compact_keep_user_turns = 4
compact_tool_body_max_chars = 800

# Cost-saver prompt: skip PLUR inject + long memory (tools + skill NL/slash stay full)
poor_mode = false

# Background TTL pack repair on later TUI opens (first install is foreground)
ecosystem_auto_ensure = true

# Self-update from GitHub Releases on interactive launch (TTL-throttled)
auto_update = true
```

### Settings reference

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `provider` | string | `nur` | Catalog id (`nur`, `openai`, `openrouter`, `ollama`, …). Set by TUI **`/login`** with matching `base_url` + `model` |
| `model` | string | `muse-spark-1.1` | Model id for the active provider |
| `base_url` | string | `https://api.meta.ai/v1` | API base (no trailing path); providers use Responses or Chat Completions under this base |
| `reasoning_effort` | string | `high` | Reasoning depth: `minimal`, `low`, `medium`, `high`, `xhigh` |
| `max_turns` | integer | `0` | Max agent tool/model rounds per user prompt. **`0` = unlimited** (default). Set via config or `/budget turns` / `/turns` |
| `max_session_cost_usd` | float? | unset (∞) | Optional session $ hard-stop. `/budget cost <usd>` · `/budget clear` |
| `max_session_tokens` | integer? | unset (∞) | Optional session token hard-stop. `/budget tokens <n>` · `/budget clear` |
| `stream` | bool | `true` | Stream API responses |
| `context_window` | integer | `1000000` | Model context window in tokens (range: 1000–2000000) |
| `tool_result_max_chars` | integer | `12000` | Max inline tool output chars; larger results spill to disk (`0` = unlimited) |
| `compact_keep_user_turns` | integer | `4` | Recent user turns kept after compaction |
| `compact_tool_body_max_chars` | integer | `800` | When compacting, truncate older tool bodies to this many chars (`0` = leave intact) |
| `poor_mode` | bool | `false` | Skip PLUR auto-inject and long memory (skill NL/slash activation still works) |
| `ecosystem_auto_ensure` | bool | `true` | Background TTL **repair** of packs on later TUI opens (first install is foreground via one-liner / EXE / `nur install`); set `false` to skip repair |
| `auto_update` | bool | `true` | On interactive launch, check [GitHub Releases](https://github.com/nuroctane/nur-cli/releases/latest) and install a newer binary when available (6h TTL when already current). Opt out with `false` or env `NUR_SKIP_AUTO_UPDATE=1`. `nur update` always runs the full update path |

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

Optional file: **`~/.nur/permissions.toml`** (and/or project **`.meta/permissions.toml`** — both are merged).

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

Optional file: **`~/.nur/hooks.toml`**.

```toml
pre_tool = "echo pre $NUR_TOOL"
post_tool = ""
timeout_ms = 5000
```

Environment for hook commands (legacy `META_*` aliases are also set):

| Env | Meaning |
|-----|---------|
| `NUR_TOOL` | Tool name |
| `NUR_ARGS_JSON` | Raw JSON args |
| `NUR_CWD` | Workspace cwd |
| `NUR_SESSION` | Session id |

Non-zero **pre_tool** exit blocks the tool. Missing file = no hooks. Check status with `/hooks`.

---

## Environment variables

### API and model

| Variable | Purpose |
|----------|---------|
| `NUR_API_KEY` | API key (preferred) |
| `META_API_KEY` | Optional key for Meta Model API provider / legacy installs |
| `MODEL_API_KEY` | API key (alternative) |
| `MUSE_API_KEY` | API key (legacy) |
| `NUR_BASE_URL` | Override API base URL (self-hosted Ollama/vLLM/LiteLLM/gateways); legacy `META_BASE_URL` |
| `NUR_MODEL` | Override model id; legacy `META_MODEL` / `MUSE_MODEL` |

### Paths

| Variable | Purpose |
|----------|---------|
| `NUR_HOME` | Override data home (default `~/.nur`); legacy `META_HOME` / `MUSE_HOME` |
| `NUR_CWD` | Default working directory; legacy `META_CWD` |

### Status and usage

Set by NurCLI for host integrations (legacy `META_*` aliases are also exported):

| Variable | Purpose |
|----------|---------|
| `NUR_STATUS_PATH` | Path to live status file |
| `NUR_USAGE_LOG_PATH` | Path to usage log |
| `NUR_SESSION_ID` | Current session id |
| `NUR_PROVIDER` | Provider identifier (set to `nur`) |

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

All NurCLI state lives under `~/.nur/` by default:

```
~/.nur/
├── auth.json           # API key
├── config.toml         # Configuration
├── permissions.toml    # Optional allow/deny/ask rules
├── hooks.toml          # Optional pre/post tool hooks
├── nur.log            # Tracing (not painted into the TUI)
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

If you upgraded from a pre-0.5.14 build, NurCLI automatically gap-fills missing files from `~/.muse/` into `~/.nur/`. Existing files are never overwritten. When the same session id exists in both homes, the **richer** copy (more tokens / newer) wins.

`nur auth logout` clears auth from both `~/.nur/` and legacy `~/.muse/`.

---

## Project instructions

NurCLI reads project-level instruction files from your working directory:

| File | Purpose |
|------|---------|
| `NUR.md` | Primary project instructions |
| `AGENTS.md` | Agent conventions (shared with other tools) |
| `CLAUDE.md` | Legacy instructions (still loaded) |
| `MUSE.md` | Legacy instructions (still loaded) |

These are loaded at session start and prepended to the system prompt.

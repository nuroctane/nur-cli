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

# Optional OMP-compatible remote summarizer (POST {systemPrompt,prompt} → {summary}).
# Off by default — local model summarization is unchanged. On failure, nur falls back locally.
# [compaction]
# remote_enabled = true
# remote_endpoint = "https://example.com/compact"

# OMP-style prewalk: strong model plans + todos, then switch to a cheap model at
# the first write/edit. Off by default. Toggle with /prewalk in the TUI.
# [prewalk]
# enabled = true
# into = "openai-codex/gpt-5.6-luna"

# Cost-saver prompt: skip PLUR inject + long memory (tools + skill NL/slash stay full)
poor_mode = false

# Background TTL pack repair on later TUI opens (first install is foreground)
ecosystem_auto_ensure = true

# Self-update from GitHub Releases — checked on EVERY launch (60s network floor)
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
| `context_window` | integer | `1000000` | Fallback context window in tokens (range: 1000–2000000). The models.dev catalog is authoritative when it knows the active provider/model; this value is used only when it does not. Auto-compaction triggers at 55% of the effective window. |
| `tool_result_max_chars` | integer | `12000` | Max inline tool output chars; larger results spill to disk (`0` = unlimited) |
| `compact_keep_user_turns` | integer | `4` | Recent user turns kept after compaction |
| `compact_tool_body_max_chars` | integer | `800` | When compacting, truncate older tool bodies to this many chars (`0` = leave intact) |
| `compaction.remote_enabled` | bool | `false` | Prefer remote summarizer when an endpoint is set |
| `compaction.remote_endpoint` | string | unset | OMP-compatible compact URL; setting it opts in. Env: `NUR_COMPACT_REMOTE_ENDPOINT` (+ `NUR_COMPACT_REMOTE=1` if only env) |
| `prewalk.enabled` | bool | `false` | After todos exist, first write/edit switches to `prewalk.into` / smol |
| `prewalk.into` | string | unset | Cheap model for prewalk handoff (`/prewalk into …`, or `OMP_SMOL_MODEL` / OMP `modelRoles.smol`) |
| `poor_mode` | bool | `false` | Skip PLUR auto-inject and long memory (skill NL/slash activation still works) |
| `ecosystem_auto_ensure` | bool | `true` | Background TTL **repair** of packs on later TUI opens (first install is foreground via one-liner / EXE / `nur install`); set `false` to skip repair |
| `auto_update` | bool | `true` | On **every** launch (bare TUI, `nur "prompt"`, `nur run …`, gateway), check [GitHub Releases](https://github.com/nuroctane/nur-cli/releases/latest) and install a newer binary when available; it runs on a background thread so it never delays or breaks a run, and the new binary is picked up on the next launch. A 60s floor between network checks stops a script that loops `nur` from hammering the API — tune it with `NUR_AUTO_UPDATE_TTL_SECS` (`0` = check every run). Opt out with `false` or env `NUR_SKIP_AUTO_UPDATE=1`. Verify with `nur update --check`; `nur update` always runs the full update path |

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

### Provider reliability

| Variable | Purpose |
|----------|---------|
| `NUR_PROVIDER_TURN_TIMEOUT_SECS` | Maximum time for one provider request before Nur cancels it (default `300`). Applies to Responses, Chat Completions, Anthropic Messages, Gemini Cloud Code, and Cursor Agent CLI transports |

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

The release check runs on **every** launch (see `auto_update` above).

| Variable | Purpose |
|----------|---------|
| `NUR_SKIP_AUTO_UPDATE` | Set to `1` to skip the launch-time release check for this shell (legacy `META_SKIP_AUTO_UPDATE`) |
| `NUR_DISABLE_UPDATES` / `DISABLE_UPDATES` | Set to `1` to disable the launch-time release check |
| `DISABLE_AUTOUPDATER` | Claude Code's kill switch — also honored. It is injected only inside AI-agent sessions, so nur stays on your last version there instead of swapping its own binary mid-session. Absent from an ordinary terminal, where nur still updates every run |
| `NUR_AUTO_UPDATE_TTL_SECS` | Minimum seconds between network checks (default `60`; `0` = check every single run). Guards against a script looping `nur` |
| `NUR_AUTO_UPDATE_BLOCKING` | Set to `1` to wait for the check to finish before continuing, instead of backgrounding it (scripts / CI that want to be on the newest build now) |

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

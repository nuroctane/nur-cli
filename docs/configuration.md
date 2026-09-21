# Configuration

NurCLI is configured via a TOML file and optional rule/hook files, plus environment variables.

## Config file

The config file lives at `~/.nur/config.toml` and is created on first run.

```toml
# Active provider id from the catalog (set by TUI /login)
provider = "meta"
model = "muse-spark-1.2"
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

# Per-request completion ceiling and reservation used before every dispatch.
# 0 restores the provider default and removes this safety reservation.
request_output_reserve_tokens = 8192

# Compaction (auto under context pressure, or /compact)
compact_keep_user_turns = 4
compact_tool_body_max_chars = 800

# Optional OMP-compatible remote summarizer (POST {systemPrompt,prompt} -> {summary}).
# Setting remote_endpoint opts in. OpenAI /chat/completions endpoints are also supported.
# On failure, nur falls back to local model summarization.
# [compaction]
# remote_enabled = true
# remote_endpoint = "https://example.com/compact"

# OMP-style prewalk: strong model plans + todos, then switch to a cheap model at
# the first write/edit. Off by default. Toggle with /prewalk in the TUI.
# [prewalk]
# enabled = true
# into = "openai-codex/gpt-5.6-luna"

# Optional HelixDB graph-vector memory resident. `auto` activates when a URL is
# configured here or through HELIX_URL/NUR_HELIX_URL, so an untouched default
# has no network/startup cost.
# Local native memory remains authoritative and failed mirrors retry from a
# durable outbox.
# [helix_memory]
# mode = "on"                       # auto | on | off
# url = "http://127.0.0.1:6969"
# api_key_env = "HELIX_API_KEY"     # env var name, never the key itself
# timeout_ms = 1500

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
| `model` | string | `muse-spark-1.2` | Model id for the active provider |
| `base_url` | string | `https://api.meta.ai/v1` | API base (no trailing path); providers use Responses or Chat Completions under this base |
| `reasoning_effort` | string | `high` | Reasoning depth: `minimal`, `low`, `medium`, `high`, `xhigh` |
| `max_turns` | integer | `0` | Max agent tool/model rounds per user prompt. **`0` = unlimited** (default). Set via config or `/budget turns` / `/turns` |
| `max_session_cost_usd` | float? | unset (∞) | Optional session $ hard-stop. `/budget cost <usd>` · `/budget clear` |
| `max_session_tokens` | integer? | unset (∞) | Optional session token hard-stop. `/budget tokens <n>` · `/budget clear` |
| `request_output_reserve_tokens` | integer | `8192` | Conservative per-request completion ceiling/reservation. Nur preflights estimated input + this reserve against context and session budgets, then sends the matching provider output-limit field where supported. `0` uses the provider default and is intentionally less safe. |
| `stream` | bool | `true` | Stream API responses |
| `context_window` | integer | `1000000` | Fallback context window in tokens (range: 1000–2000000). The models.dev catalog is authoritative when it knows the active provider/model; this value is used only when it does not. Auto-compaction keeps an OMP-style response/tool reserve: the larger of 15% or 16,384 tokens on large windows, with a proportional safety clamp on small windows. A local stored-context estimate is used when provider usage is absent or under-reported. |
| `tool_result_max_chars` | integer | `12000` | Max inline tool output chars; larger results spill to disk (`0` = unlimited) |
| `compact_keep_user_turns` | integer | `4` | Recent user turns kept after compaction |
| `compact_tool_body_max_chars` | integer | `800` | When compacting, truncate older tool bodies to this many chars (`0` = leave intact) |
| `kv_stable_compact` | bool | `true` | `true` preserves the recent working edge verbatim after a stable summary for provider prompt-cache reuse. `false` uses a classic reconstructed working-tail order. Compaction status reports the active strategy. |
| `compaction.remote_enabled` | bool | `false` | Prefer remote summarizer when an endpoint is set |
| `compaction.remote_endpoint` | string | unset | OMP-compatible compact URL; setting it opts in. Supports `{systemPrompt,prompt}` endpoints and OpenAI `/chat/completions`; failures fall back locally. The bounded transcript crosses this configured remote data boundary. Nur records endpoint origin, bounded input estimate, output estimate, and provider usage when returned in the session receipt. Env: `NUR_COMPACT_REMOTE_ENDPOINT` (+ `NUR_COMPACT_REMOTE=1` if only env) |
| `typesafe.enabled` | bool | `true` | [TypeSafe](https://docs.typesafe.ai) (Jev) typed judgments for the harness: tool gate, result judge, Jev-scored compaction, skill checks, model routing. Needs a key (`TYPESAFE_API_KEY` or `/auth`); with none, every policy returns "no judgment" and behavior is unchanged. Full reference: [typesafe.md](./typesafe.md) |
| `typesafe.model` | string | `jev-latest` | System One model that answers the questions |
| `typesafe.base_url` | string | `https://api.typesafe.ai/v1/systemone` | System One endpoint. A **loopback** URL (127.0.0.1 / localhost / `[::1]`) needs no key and runs the whole judgment layer against a local engine - see [jev-local.md](./jev-local.md) |
| `typesafe.act_confidence` | float | `0.85` | Confidence at or above which a judgment may change behavior |
| `typesafe.escalate_confidence` | float | `0.5` | Below this the judgment is handed to a bigger model or a human instead of being acted on |
| `typesafe.max_questions_per_request` | integer | `24` | Questions merged into one request before they are split and sent in parallel |
| `typesafe.max_parallel` | integer | `4` | Concurrent requests when a question set is split |
| `typesafe.compaction.replace_summary` | bool | `true` | When a Jev prune frees enough context, skip the summarizing model call entirely (survivors stay verbatim) |
| `typesafe.compaction.min_reduction` | float | `0.25` | Reduction ratio required to take that path instead of summarizing |
| `typesafe.tool_gate.skip_redundant` | bool | `true` | Answer a confident duplicate of a call whose result is still in context without re-running the tool |
| `typesafe.tool_gate.skip_repeated_failures` | bool | `false` | Also skip identical repeats of a call that already failed (surfaced in the transcript, not enforced, by default) |
| `typesafe.skills.narrow_requirements` | bool | `true` | Inject only the triggered skill's applicable rules as a checklist |
| `typesafe.routing.enabled` | bool | `false` | Let the harness switch to the suggested model; `false` keeps the suggestion advisory |
| `typesafe.tools.subset` | bool | `false` | Narrow the tool surface on the first round of a turn (one Noul per specialist; later rounds get the full surface). Off because tool schemas ride the prompt cache |
| `typesafe.tools.keep_probability` | float | `0.75` | How confident the answer must be that a tool is *not* needed before it is dropped (keeping is the safe direction) |
| `prewalk.enabled` | bool | `false` | After todos exist, first write/edit switches to `prewalk.into` / smol |
| `prewalk.into` | string | unset | Cheap model for prewalk handoff (`/prewalk into …`, or `OMP_SMOL_MODEL` / OMP `modelRoles.smol`) |
| `helix_memory.mode` | string | `auto` | `auto` enables only when `HELIX_URL`/`NUR_HELIX_URL` or a configured URL exists; `on` uses the configured/local endpoint; `off` disables the resident |
| `helix_memory.url` | string | local endpoint when on | HelixDB gateway base URL. Nur uses `/healthz` and typed `/v2/query` requests |
| `helix_memory.api_key_env` | string | `HELIX_API_KEY` | Name of the env var containing the Helix bearer token. The token is never written to config or memory |
| `helix_memory.timeout_ms` | integer | `1500` | Per-request deadline. Writes queue locally first and retry from the outbox on later writes or `helix_sync` |
| `poor_mode` | bool | `false` | Skip PLUR auto-inject and long memory (skill NL/slash activation still works) |
| `theme` | string | unset | Picked theme id (`/theme`). Unset = Nur Gold until onboarding choice |
| `theme_setup.accent` | hex string | unset | Personal accent color (`#rrggbb`) layered over **every** theme pick - recolors the accent ramp, highlights, and banner gradient. `theme_setup.accent_deep` / `theme_setup.accent_sky` optionally pin the ramp ends (derived automatically when absent) |
| `theme_setup.protocol` | string | `auto` | Force the terminal graphics protocol for inline images: `kitty`, `sixel`, `iterm2`, `halfblocks`, or `auto` |
| `theme_setup.inline_images` | bool | `true` | Master switch for inline pixel rendering in transcript + peeks |
| `ecosystem_auto_ensure` | bool | `true` | Background TTL **repair** of packs on later TUI opens (first install is foreground via npx / one-liner / EXE / `nur install\); set `false` to skip repair |
| `auto_update` | bool | `true` | On **every** launch (bare TUI, `nur "prompt"`, `nur run …`, gateway), check [GitHub Releases](https://github.com/nuroctane/nur-cli/releases/latest) and install a newer binary when available; it runs on a background thread so it never delays or breaks a run, and the new binary is picked up on the next launch. A 60s floor between network checks stops a script that loops `nur` from hammering the API — tune it with `NUR_AUTO_UPDATE_TTL_SECS` (`0` = check every run). Opt out with `false` or env `NUR_SKIP_AUTO_UPDATE=1`. Verify with `nur update --check`; `nur update` always runs the full update path |

### Personal accent (theme setup)

Layer a personal accent color on top of any `/theme` pick:

```toml
[theme]
accent = "#e8b923"        # your accent (any #rrggbb)
# accent_deep = "#8a5a00" # optional; derived when absent
# accent_sky  = "#ffe08c" # optional; derived when absent
```

The accent recolors the theme's ramp (sky → accent → deep), selection highlights,
and the banner gradient - so `/theme midnight` + `accent = "#ff5470"` gives you
midnight with a pink spine. `/theme` switching keeps the override.

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

Budgets are the only thing that stops a turn. With no ceilings set, nothing ends
a turn on its own: context-window overflow (a preflight estimate or a provider
rejection) recovers through staged compaction - promote to a larger sibling
model, model-assisted compact, then a local minimal trim - and oversized pasted
input is auto-registered with the `context` tool (pointer + preview) instead of
being refused. Every other turn stop is an explicit budget you set.

`max_turns = 0` means unlimited task completion rounds - it does **not** mean
that spending is safe or capped. On the first unrestricted turn Nur warns about
this distinction. Provider usage can be absent or subscription-only, so token
and dollar enforcement remains estimate-based on those routes.

### Conservative budget preset

For unattended or weekly-allowance-sensitive work, start with this explicit
preset and adjust upward only after inspecting `/usage` and `/receipt`:

```toml
# Conservative: 40 model/tool rounds, 250k total tokens, $2.00 estimated
# list-price spend, and an 8k reserve before every request/failover target.
max_turns = 40
max_session_tokens = 250000
max_session_cost_usd = 2.0
request_output_reserve_tokens = 8192

# Keep fusion/fan-out opt-in. Every configured panel is an additional reserved
# request; set fusion_panel = [] for the conservative preset.
fusion_panel = []
```

The preflight also evaluates each failover destination for context capacity,
tool capability, parallel-tool safety, output-limit support, and incremental
catalog-estimated cost. Local OpenAI-compatible servers use conservative serial
tool behavior because tool support depends on the selected model template.

---

## Permission rules

Optional file: **`~/.nur/permissions.toml`** (and/or project **`.nur/permissions.toml`** - both are merged).

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

Hook **output is discarded** - exit status is the only channel (`0` allows, non-zero
blocks the call in Manual/Auto and is reported to the model). stdout/stderr are not
captured, so a hook that writes a lot cannot stall the call.


Optional file: **`~/.nur/hooks.toml`**.

```toml
pre_tool = "echo pre $NUR_TOOL"
post_tool = ""
timeout_ms = 5000
```

Environment for hook commands:

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
| `META_API_KEY` | Optional key for Meta Model API provider |
| `NUR_BASE_URL` | Override API base URL (self-hosted Ollama/vLLM/LiteLLM/gateways) |
| `NUR_MODEL` | Override model id |
| `NUR_JEV_LOCAL_URL` | Point the TypeSafe layer at a local System One engine (e.g. `http://127.0.0.1:8788`). Loopback needs no key, so `TYPESAFE_API_KEY` becomes optional. See [jev-local.md](./jev-local.md) |
| `TYPESAFE_API_KEY` | TypeSafe (Jev) key for the harness boost layer. Aliases: `TYPESAFE_KEY`, `JEV_API_KEY`; also `/auth` → `TypeSafe · Jev`, or `~/.nur/typesafe.key`. It is a judgment credential, never the active chat provider |

### Provider reliability

| Variable | Purpose |
|----------|---------|
| `NUR_PROVIDER_TURN_TIMEOUT_SECS` | Maximum time for one provider request before Nur cancels it (default `300`). Applies to Responses, Chat Completions, Anthropic Messages, Gemini Cloud Code, and Cursor Agent CLI transports |

### Paths

| Variable | Purpose |
|----------|---------|
| `NUR_HOME` | Override data home (default `~/.nur`) |
| `NUR_CWD` | Default working directory |

### Status and usage

Set by NurCLI for host integrations:

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
| `NUR_SKIP_AUTO_UPDATE` | Set to `1` to skip the launch-time release check for this shell |
| `NUR_DISABLE_UPDATES` / `DISABLE_UPDATES` | Set to `1` to disable the launch-time release check |
| `DISABLE_AUTOUPDATER` | Claude Code's kill switch — also honored. It is injected only inside AI-agent sessions, so nur stays on your last version there instead of swapping its own binary mid-session. Absent from an ordinary terminal, where nur still updates every run |
| `NUR_AUTO_UPDATE_TTL_SECS` | Minimum seconds between network checks (default `60`; `0` = check every single run). Guards against a script looping `nur` |
| `NUR_AUTO_UPDATE_BLOCKING` | Set to `1` to wait for the check to finish before continuing, instead of backgrounding it (scripts / CI that want to be on the newest build now) |

### Ecosystem

| Variable | Purpose |
|----------|---------|
| `CLAUDE_FLOW_DB_PATH` | Ruflo database path |
| `CLAUDE_FLOW_MEMORY_PATH` | Ruflo home path |
| `HELIX_URL` / `NUR_HELIX_URL` | Activate the HelixDB memory resident in `auto` mode and select its gateway |
| `HELIX_API_KEY` | Optional HelixDB bearer token (or use the env-var name configured by `helix_memory.api_key_env`) |
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

Override with `NUR_HOME`.

---

## Project instructions

NurCLI reads project-level instruction files from your working directory:

| File | Purpose |
|------|---------|
| `NUR.md` | Primary project instructions |
| `AGENTS.md` | Agent conventions (shared with other tools) |
| `CLAUDE.md` | Legacy instructions (still loaded) |

These are loaded at session start and prepended to the system prompt.

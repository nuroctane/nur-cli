# Commands

Full CLI reference for NurCLI.

## Usage

```bash
nur [OPTIONS] [PROMPT]
nur <COMMAND> [ARGS]
```

## Global options

| Flag | Short | Description |
|------|-------|-------------|
| `--model <MODEL>` | `-m` | Model id for the active provider (default from config; browse with `/model` in the TUI) |
| `--cwd <DIR>` | | Working directory |
| `--yes` | `-y` | Auto-approve tools (sets permission mode to auto) |
| `--mode <MODE>` | | Permission mode: `manual`, `plan`, or `auto` |
| `--effort <LEVEL>` | | Reasoning effort: `minimal`, `low`, `medium`, `high`, `xhigh` |
| `--max-turns <N>` | | Max agent turns per prompt |
| `--continuous` | | Sovereign mode: loop headless turns toward the prompt (as a goal) until the model replies `DONE`, Ctrl+C, or `--max-iters`. Auto-approves tools (sandboxed). |
| `--max-iters <N>` | | Continuous mode: stop after N iterations (`0` = unlimited) |
| `--verbose` | `-v` | Verbose tool logging (headless mode) |
| `--continue` | `-c` | Continue the most recent session for this cwd |
| `--resume <ID>` | `-r` | Resume a specific session id (full UUID or unique prefix) |
| `--version` | | Print version |
| `--help` | `-h` | Print help |

## Examples

```bash
nur                                     # open interactive TUI
nur install                            # one-stop stack install (same as release EXE)
nur "fix the bug"                       # start with a prompt
nur "design from ref.mp4"              # vision: auto-attach media
nur -c                                  # continue last session
nur -r abc123                           # resume session abc123
nur --mode plan "explain this"         # plan: explore + shell, no edits/commits
nur --effort xhigh "deep analysis"     # maximum reasoning
nur --model muse-spark-1.1 "hello"     # explicit model
nur run "add tests" -y                 # headless + auto-approve
```

---

## Subcommands

### `nur run`

Run a single agent turn headlessly. Prints the final answer to stdout.

```bash
nur run <PROMPT...> [OPTIONS]
```

| Arg / Flag | Description |
|------------|-------------|
| `PROMPT` | Prompt text (required, multiple words joined) |
| `-y`, `--yes` | Auto-approve all tools |

**Example:**

```bash
nur run "write a hello world in Rust" -y
nur run "explain what this repo does" -v
```

---

### `nur auth`

Manage the stored API key (`~/.nur/auth.json`).

For **multi-provider** sign-in (pick OpenAI, OpenRouter, Ollama, xAI, … + endpoint
and default model), use the TUI slash command **`/login`**. See
[Authentication](authentication.md). CLI `nur auth login` stores a key for the
active provider without opening the full catalog picker.

```bash
nur auth <SUBCOMMAND>
```

#### `nur auth login`

Save API key to `~/.nur/auth.json`.

```bash
nur auth login [--key <KEY>]
```

| Flag | Description |
|------|-------------|
| `--key <KEY>` | API key (optional; prompts if omitted) |

#### `nur auth status`

Show auth status. Never prints the full key.

```bash
nur auth status
```

#### `nur auth logout`

Remove saved key from `~/.nur/auth.json` and legacy `~/.muse/`.

```bash
nur auth logout
nur auth logout --revoke   # local delete + best-effort revoke notes for az/aws/gcloud
```

---

### `nur sessions`

List recent sessions (prompt-first summaries from `~/.nur/sessions` and legacy `~/.muse/sessions`).

```bash
nur sessions [--limit <N>]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--limit` | `20` | Max rows to display (`0` = all) |

Columns: **ID · UPDATED · MSGS · TOKENS · COST · CWD**.

---

### `nur usage`

Show last known token usage and **estimated** cost. Displays paths to status and usage log files.

Dollar values on the TUI status line and here are **list-price estimates** (per-model rates from [models.dev](https://models.dev) when available, else Meta fallback constants, else `$0` for local providers). They are **not** provider invoices. Cached prompt tokens are priced at the catalog’s cache-read rate when known.

Opt out of the catalog fetch with `NUR_PRICING_OFF=1`. Cache lives at `~/.nur/cache/models-dev.json` (24h TTL).

```bash
nur usage
```

---

### `nur install`

One-stop install — **same job as the release EXE and the shell one-liners** (minus compiling from source): copy binary → PATH → prereqs (best-effort) → ecosystem ensure → browser stage → Orca hook → optional auth from env. **No TUI** until this finishes (or until you open `nur` afterward).

```bash
nur install
# alias:
nur self-install
```

Double-clicking `nur-windows-x86_64.exe` from [Releases](https://github.com/nuroctane/nur-cli/releases/latest) runs this path automatically, then opens NurCLI.

### `nur update`

**How you upgrade NurCLI.** Pull latest source, rebuild release, reinstall binary + full stack.

```bash
nur update
```

| Step | Action |
|------|--------|
| Source | Uses `~/laboratory/nur-cli` or `~/Laboratory/nur-cli` if present |
| Git | `git pull --ff-only origin main` |
| Build | `cargo build --release` |
| Binary | Installs to `~/.local/bin/nur` |
| Stack | `ecosystem ensure --force`, `browser setup`, Orca hook |
| No checkout | Falls back to `nur install` (repair from the running binary) |

Afterward: `nur --version` · `nur doctor`.

Full paths and alternatives (one-liner / EXE / `nur install`): **[Setup → Update](setup.md#update-keep-nurcli-current)**.

---

### `nur plugins`

Marketplace plugins (install into `~/.nur/plugins`, skills mirrored to `~/.nur/skills`).

```bash
nur plugins                 # list catalog + install state
nur plugins list
nur plugins install <id>    # e.g. superpowers, vercel, firecrawl, fable
nur plugins enable <id>
nur plugins disable <id>
nur plugins uninstall <id>
```

**Skills as slash commands:** every installed skill is also `/skill-name` (sticky session mode) or `/skill-name <prompt>` (one-shot turn). Natural-language activation still works with no slash: *think like fable*, *TDD this*, *debug systematically*, *site cli*, *HAR file*, *resume from Claude* — see [Ecosystem](ecosystem.md#natural-language-skill-activation).


In the TUI, bare **`/plugins`** opens the full marketplace picker (provider-picker UX).

### `nur doctor`

Diagnose install, auth, config, ecosystem, and plugin marketplace readiness.

```bash
nur doctor
```

Checks:

- Binary path and version
- Config file (model, effort, max_turns, **budget caps**)
- Auth status (key present, last 4 chars)
- Data home, status, usage, sessions paths
- Ecosystem readiness (Graphify, PLUR, Ruflo, browser, omp when present)
- Shell backend (Bash / PowerShell)
- Optional tools on PATH (rg, git, node, npm, uv, ffmpeg)
- Vision support (look, extract_frames)
- Binary SHA-256 integrity

See [Troubleshooting](troubleshooting.md) for interpreting results.

---

### `nur ecosystem`

Manage the Graphify / PLUR / Ruflo / browser / omp ecosystem.

```bash
nur ecosystem <SUBCOMMAND>
```

#### `nur ecosystem ensure`

Install or repair Graphify, PLUR, Ruflo, skills, and related packs. The one-liner, release EXE, and `nur install` already run this **in the foreground**. On later TUI opens it also runs as **background TTL repair** when `ecosystem_auto_ensure = true` (default).

```bash
nur ecosystem ensure [--force]
```

| Flag | Description |
|------|-------------|
| `--force`, `-f` | Force re-install even if marker is fresh |

#### `nur ecosystem status`

Show ecosystem readiness.

```bash
nur ecosystem status
```

---

### `nur browser`

Set up the real-browser `browser` tool for your **default Chromium browser**
(Arc, Chrome, Edge, Brave, …). Stages the `tmwd_cdp_bridge` extension and
walks you through the one-time Load unpacked click.

```bash
nur browser <SUBCOMMAND>
```

#### `nur browser setup`

Stage the extension (no download), detect the default browser, copy the staged
path to the clipboard, and open `chrome://extensions`.

```bash
nur browser setup
```

#### `nur browser status`

Show detected default browser + extension staging state.

```bash
nur browser status
```

Also runs automatically from the installer after `ecosystem ensure`.

---

### `nur install-hook`

Install the Orca agent hook for usage/status reporting.

```bash
nur install-hook
```

---

### `nur gateway`

Run headless as a **Telegram bot** — each inbound message is an agent turn in the
current project, with the answer sent back (one session for continuity, tools
auto-approved). Get a token from [@BotFather](https://t.me/BotFather).

```bash
nur gateway [--token <TOKEN>] [--chat <CHAT_ID>]
# token also from $TELEGRAM_BOT_TOKEN · chat from $TELEGRAM_CHAT_ID
```

| Flag | Description |
|------|-------------|
| `--token` | Bot token (else `$TELEGRAM_BOT_TOKEN`) |
| `--chat` | Restrict to a single chat id (else `$TELEGRAM_CHAT_ID`; unset = anyone) |

---

### `nur local`

Managed **local models** — bundles llama.cpp (fetches a prebuilt `llama-server`
for your platform on demand), downloads a GGUF sized to your RAM, and runs it on
`127.0.0.1:8080` (the `llama.cpp (local)` catalog provider). No API key.

```bash
nur local up [<tier|url>]   # size to RAM (or a tier: small|medium|large, or a direct .gguf URL)
nur local status            # server · downloaded models · running state
nur local down              # stop the managed server
nur local models            # list the built-in tiers
```

Also available in the TUI as **`/local`**.

---

### `nur bench`

Benchmark models on **your own tasks**: record a task once, replay it across
models in isolated **git worktrees**, and score them (pass/fail via a check
command, wall time, tokens).

```bash
nur bench add <name> "<prompt>" [--check "<shell cmd>"]   # exit 0 of the check = pass
nur bench list
nur bench remove <name>
nur bench run <name|all> [--models <m1,m2>]               # default: the active model
```

Also available in the TUI as **`/bench`**.

---

## TUI slash commands

Type these inside the `nur` TUI. Aliases are shown in the same row.

| Command | Purpose |
|---------|---------|
| `/help` · `/commands` | Keys + command list |
| `/exit` · `/quit` | Quit |
| `/clear` | Clear the transcript display |
| `/new` | Start a fresh session |
| `/compact` | Summarize the conversation to free context |
| `/sessions` · `/resume` | Browse & open past sessions (`/resume <id>` also works) |
| `/login` | Provider + API key or browser sign-in |
| `/logout` | Clear the stored API key |
| `/model` · `/models` | Show and switch models for the active provider |
| `/plugins` · `/plugin` | Browse / install / enable marketplace plugins |
| `/effort` | Reasoning effort: `minimal` → `xhigh` |
| `/mode` | Permission mode: `manual` \| `plan` \| `auto` (or Shift+Tab) |
| `/manual` | Switch to manual mode (approve writes/shell) |
| `/plan` | Switch to plan mode (read-only explore) |
| `/auto` | Switch to auto-approve mode |
| `/cd` | Change working directory (tool sandbox root) |
| `/pwd` | Print the working directory |
| `/budget` | Session spend ceiling |
| `/turns` | Per-session agent-turn ceiling (`0` = unlimited) |
| `/poor` | Cost-saver lean prompt |
| `/usage` · `/cost` | Token usage + estimated cost this session |
| `/context` | Context-window utilization |
| `/status` | Session snapshot: model · mode · cwd · tokens |
| `/doctor` | Health check: version · auth · ecosystem · shell |
| `/fusion` | Multi-model debate → one synthesized answer |
| `/local` | Run a model locally via bundled llama.cpp |
| `/bench` | Benchmark models on your tasks |
| `/failover` | Cross-provider failover + privacy tiers |
| `/undo` | Revert the last file edit this session |
| `/receipt` | Session receipt — hash-chained verification |
| `/cua` | Computer-use desktop driver: `on` \| `off` \| `status` |
| `/graph` | Inline live execution-graph card for the current turn |
| `/draw` | Open / build **tldraw offline** boards (`/draw <file.tldraw>`, `/draw install`, `/draw <idea>`). New static boards save to the **Desktop**. Opening a board auto-enables document scripts (canvas API `script-workspace` → applied) for interactive agent-shape files. |
| `/steer` | Inject a message into the running turn without cancelling it |
| `/scan` | Map the codebase → shareable foglamp scan |
| `/goal` | Set a standing session goal |
| `/btw` | One-off note attached to the next message |
| `/bro` | Chill mode: plain words, straight answers (toggle) |
| `/adhd` | Sticky ADHD-friendly output for this session (toggle) |
| `/site-cli` | Skill: HAR → derived site API client/CLI |
| `/fable-method` | Skill: Fable think-act-prove loop |
| `/fable-loop` | Skill: orchestrated Fable multi-step loop |
| `/fable-judge` | Skill: adversarial verification of finished work |
| `/tech-spec` | Skill: typed call-stack architecture handoff |
| `/design-eng` | Skill: Emil design-eng UI/motion craft |
| `/test-driven-development` | Skill: TDD red-green-refactor |
| `/systematic-debugging` | Skill: root-cause-first debugging |
| `/<skill>` | **Any installed skill** — sticky toggle, or `/<skill> <prompt>` one-shot |
| `/codesearch` · `/cs` | Fast ripgrep over the workspace |
| `/mc` · `/mcp` | Manage MCP servers via Executor |
| `/skills` | List installed skills |
| `/memory` | Show the `~/.nur` memory excerpt |
| `/graphify` | Knowledge graph: status / query / extract |
| `/plur` | Shared engram memory |
| `/ruflo` | Vector memory / swarm |
| `/ecosystem` | Check / provision the ecosystem |
| `/todos` | Show the session task list |
| `/init` | Generate a `NUR.md` project guide |
| `/config` | Show config + data paths |
| `/permissions` | Show or reload allow/deny/ask rules |
| `/hooks` | Show local tool hook status |
| `/feedback` | File a GitHub issue from here |
| `/bug` | Report an issue (GitHub link) |
| `/tips` | Mouse + keyboard interaction tips |

---

## Project instruction files

NurCLI loads project-level instructions from your working directory at session start:

| File | Purpose |
|------|---------|
| `NUR.md` | Primary project instructions |
| `AGENTS.md` | Agent conventions |
| `CLAUDE.md` | Also loaded |
| `META.md` | Legacy (still loaded) |
| `MUSE.md` | Legacy (still loaded) |

---

## Safe workspace

When launched from a drive root (`C:\` or `/`), NurCLI auto-selects a safe workspace by checking (in order):

1. Git repository root
2. Last session's working directory
3. `~/Laboratory` (or fallback)

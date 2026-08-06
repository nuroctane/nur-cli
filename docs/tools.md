# Tools

All native tools available to the NurCLI agent.

## Tool families

| Family | Tools | Colour |
|--------|-------|--------|
| **read** | `read_file` `list_dir` `grep` `glob` | sky |
| **edit** | `write_file` `edit_file` `multi_edit` `apply_patch` | violet |
| **shell** | `bash` | amber |
| **vision** | `look` `extract_frames` | pink |
| **web** | `web_search` `web_fetch` | teal |
| **git** | `git_status` `git_diff` | cyan |
| **browser** | `browser` `terminal_browser` | teal |
| **knowledge** | `graphify` `graphjin` `plur` `ruflo` `akarso` `executor` `skill` `memory` | indigo / orange |
| **delegate** | `omp` | - |
| **agent** | `todo_write` `submit_plan` `agent` | - |

All of the above are **first-class** in the tool schema every turn (nothing is hidden behind a “search tools” gate). Capability flags (read-only / concurrency-safe / destructive) drive parallel batching and approvals.

### Tool results and context

- Results larger than `tool_result_max_chars` (default **12000**) are written under `~/.nur/tool-results/` and the model gets a short preview + path (use `read_file` for more).
- Set `tool_result_max_chars = 0` for unlimited inline results (legacy behaviour).

---

## Read tools

### `read_file`

Read the contents of a file.

### `list_dir`

List directory contents.

### `grep`

Search file contents using regular expressions. Uses ripgrep when available, falls back to native implementation.

### `glob`

Find files matching a pattern (e.g. `**/*.rs`). Uses ripgrep when available.

---

## Edit tools

### `write_file`

Write content to a file. Creates the file if it doesn't exist, overwrites if it does.

### `edit_file`

Apply targeted string replacements to a file. Requires exact string matching.

### `multi_edit`

Apply multiple edits to a file in a single operation.

### `apply_patch`

Apply a unified diff patch to a file.

---

## Shell

### `bash`

Execute shell commands in the workspace cwd (cwd-scoped, not a full OS sandbox). Hardened with:

- **Denylist** — blocks dangerous commands (e.g. `rm -rf /`, fork bombs) and hang-prone patterns (dev servers, `watch`, interactive prompts)
- **Timeout** — default **60s**, hard max **180s**; idle (no output ~90s after grace) kills the process tree
- **Failure lockout** — identical timed-out/hung commands in the same cwd are refused for 15 minutes
- **Non-interactive env** — `CI=1`, `GIT_TERMINAL_PROMPT=0`, stdin null

!!! note "Shell backend"
    NurCLI uses Git Bash on Windows when available, otherwise falls back to PowerShell. On macOS/Linux it uses Bash. Check with `nur doctor`. Prefer `list_dir` / `read_file` / `grep` / `glob` over shell for file IO and search.

---

## Vision tools

### `look`

Attach workspace images or video so the model sees them. Accepts png, jpg, webp, gif (direct) and mp4 (direct up to ~20 MB). See [Vision](vision.md) for details.

### `extract_frames`

Extract keyframes from video via ffmpeg. Output goes to `.nur/frames/<name>/`.

---

## Web tools

### `web_search`

Search the web for information.

### `web_fetch`

Fetch content from a URL.

---

## Git tools

### `git_status`

Show working tree status.

### `git_diff`

Show file changes.

---

## Knowledge tools

### `graphify`

Query the code knowledge graph — offline, no API key, graph traversal
(`query` / `path` / `explain` / `affected`). See [Ecosystem](ecosystem.md).

### `graphjin`

Governed access to **live data**: one GraphQL→SQL surface over 12+ database
engines, plus the `gj_*` system roots (`gj_catalog` discovery, `gj_code` — the
repo as queryable tables joinable with live data — `gj_security`, `gj_config`,
`gj_runtime`).

`action=status|catalog|schema|help|explain|query|security|ask|mutate`. Discovery
comes first and the server enforces it: a result whose `status` is `blocked`
means required discovery was skipped, and nur relabels it as a failure rather
than letting a plausible-looking answer through. `mutate` is the only
write-class action, so plan mode blocks it and manual mode gates it.

Needs `graphjin` on PATH (`npm i -g graphjin`) and a one-time
`graphjin cli setup <server-url>`. Also `/graphjin`.

**graphify or graphjin?** graphify traverses a local code graph with no
prerequisites — reach for it for architecture and impact questions. graphjin
*joins*: use it when a question spans code and live data, config, or security
posture. See the [Ax & GraphJin evaluation](integrations-ax-graphjin.md).

### `plur`

Search shared engram memory.

### `ruflo`

Search vector memory.

### `headroom`

Doctor / optional one-shot compress for [Headroom](https://github.com/headroomlabs-ai/headroom).
Inline compression of large tool results is **on by default** (`[headroom] enabled`);
missing install no-ops. Also `/headroom`.

### `optmem`

Permanent memory via [OptMem](https://github.com/VictorTaelin/OptMem) at `~/.optmem`
(upstream path, not under `~/.nur`). Actions: `doctor` · `wake` · `note` · `nap` ·
`recall` · `zoom` · `forget` · `config`. Also `/optmem` · `/memo`.

### `egaki`

Image/video generation via [egaki](https://github.com/remorses/egaki). Prefer
`egaki login --provider chatgpt` when using a ChatGPT subscription. Also `/egaki`
· `/image`.

### `akarso`

Post, schedule, and reply across 14 social platforms (X, LinkedIn, Instagram, Facebook, TikTok, YouTube, Threads, Reddit, Bluesky, Mastodon, Discord, Slack, Pinterest, Google Business). `action=auth_check|accounts_list|accounts_health|accounts_get|accounts_connect|posts_list|posts_get|posts_create|posts_delete|profiles_list`. Read actions are free; `posts_create` (publish/schedule), `posts_delete`, and `accounts_connect` are outward-facing and approval-gated. Requires `akarso auth login` once. Also `/akarso`.

### `executor`

Dispatch to external APIs.

### `browser`

Perceive and control the user's **real, default browser** — Arc, Chrome, Edge,
Brave, or any Chromium browser — with login state preserved, via
[agent-browser-cli](https://github.com/sleepinginsummer/agent-browser-cli).
Perception is free: `tabs`, `scan`, `snapshot` (page → `@e` element refs),
`tabtree`, `console`, `network`, `status`. Control needs approval in manual
mode and is blocked in plan mode: `open`, `click`, `fill`, `send_keys`, `exec`,
`close`. `screenshot` is plan-safe perception — pair it with `look` for vision.
Cookie reading is deliberately not exposed.

**Setup is automatic.** The one-liner, release EXE, and `nur install` provision
the CLI, stage the `tmwd_cdp_bridge` extension (no download), detect your
**default browser**, and run browser setup — which opens that browser's
`chrome://extensions` page. The only manual step is a one-time **Load unpacked**
click (a Chromium security boundary that can't be scripted); the staged folder
path is copied to your clipboard. Re-run any time with:

```bash
nur browser setup     # stage + open the default browser's extensions page
nur browser status    # detected default browser + extension state
```

The `browser` tool's own `status` action folds in this local state so the agent
can self-diagnose a disconnected bridge.

### `terminal_browser`

[terminal-browser.com](https://terminal-browser.com/) - show a site or local HTML
beside the agent and drive the open tab (`snapshot` / `click` / `fill`).

Actions: `status` · `doctor` · `help` · `ls` · `setup` · `open` · `action`.
Default `open` uses `split=right`. `action` takes `command` (or `args[]`) after
`--` in agent-browser form.

**Windows:** when the upstream binary is not installed, Nur uses a **windows-host**
runtime that maps the same tool API onto `agent-browser-cli` (real Chrome) and
optionally opens a Windows Terminal split pane. Prefer WSL if
`terminal-browser` is installed there. Native in-terminal kitty-graphics Chromium
is still macOS (Apple Silicon) upstream today; Linux is WIP.

Slash: `/tb` · `/terminal-browser`. Skill: `terminal-browser`.

### `omp`

Delegate a focused coding task to the [Oh My Pi](https://omp.sh) agent backend
(headless one-shot `omp -p` run in the workspace; the IDE/ACP surface is not
used). Strong at LSP-backed refactors, debugger-driven diagnosis (DAP), and AST
rewrites.

Focused delegation defaults to `cost_mode=economy`. Nur first honors
`OMP_SMOL_MODEL` / `PI_SMOL_MODEL` or OMP's configured `modelRoles.smol`; when
none exists, it inspects OMP's authenticated accounts and live model catalog and
selects a small, low-cost text model. The route is cached briefly, but every OMP
run still reports the provider and model it actually used. No authenticated
route means a clear login error, never a silent fallback to an unrelated model.

Economy runs use low thinking and a reduced coding tool surface. All delegated
runs disable ambient OMP extensions, skills, and rules because Nur already supplies the
bounded handoff and owns skill activation. This keeps unrelated local plugin
state out of the run and avoids duplicating large prompt catalogs. Use
`cost_mode=balanced` or an explicit `model` only when the task needs more
capability. Runs use bounded time, ephemeral sessions, and compact result
contracts. OMP's JSON events supply the concrete provider, model, token counts,
and cost, which Nur folds into `/usage`, session status, and `/budget` totals.
Invalid cost, thinking, tool-profile, and timeout options fail closed rather than
silently widening the delegated tool surface.

`run` is write-class: it needs approval in manual mode and is blocked in plan
mode. Once Nur approves the delegation, the headless OMP child receives an
explicit approval policy. Esc kills the whole OMP process tree so it cannot keep
editing or spending after cancellation. OMP JSON error events remain failures
even when the OMP process exits with code 0. `status` reports version,
authenticated providers, model roles, the resolved economy route, and warnings;
`version` is the lightweight version-only check. Both remain free.
Provisioning requires **omp >= 17.2.0** (feature floor; `nur ecosystem ensure`
auto-upgrades). Bun installs require version 1.3.14 or newer.

### `skill`

List or load a skill pack (`SKILL.md`) into context.

| Action | Behaviour |
|--------|-----------|
| `list` | Installed skills under `~/.nur/skills`, enabled plugins, project skill dirs |
| `read` | Full skill body (large packs are not truncated the way the system catalog is) |

**Natural-language activation:** many workflow skills also auto-activate from plain wording (no slash, no first `skill` call). Examples: *think like fable*, *TDD this*, *debug systematically*, *polish the UI*, *resume from Claude*. When that fires, the harness injects the skill body for the whole turn and shows a status chip. Details: **[Ecosystem → Natural-language skill activation](ecosystem.md#natural-language-skill-activation)**.

### `memory`

Read or append to the cross-session memory journal (`~/.nur/memory.md`).

---

## Agent tools

### `todo_write`

Create and manage task lists.

### `submit_plan`

Submit a plan for approval.

### `agent`

Spawn a subagent for complex tasks. `explore` runs read-only research; `general`
inherits the parent permission mode. In the CLI, child approval requests are
proxied to the parent prompt, and subagent transcripts stay out of the native
session list while their usage is folded into the parent turn. Child prompts
receive a focused core tool schema rather than the parent's full ecosystem
catalog. Nested delegation and OMP are intentionally absent, so a failed child
cannot recurse or silently substitute OMP. Partial output from a failed child is
preserved in the error but never marked successful.

Native fan-out runs at most four children concurrently. Cancellation interrupts
children waiting for approval, the shared approval prompt, and concurrency
permits, then aborts every queued or running child so no detached task can begin
credential resolution, edits, or token spend after the parent turn ends.

**Cross-provider subagents.** An `agent` call may set `provider` (and optionally
`model`) to run a subagent on a *different* provider than the parent — in natural
language. `provider:"grok"` → xAI, `"gemini"`/`"google"` → Google, `"claude"` →
Anthropic, `"chatgpt"`/`"gpt"` → OpenAI, `"antigravity"`/`"agy"` → Antigravity
(own OAuth — not the same as Gemini), plus `deepseek`, `mistral`, `kimi`, and
direct catalog ids. Omit both fields to inherit the parent's provider/model.
This lets one turn fan out across providers — e.g. a Claude reviewer alongside a
Grok auditor — from a single prompt.

**No silent parent fallback.** If you are not signed in to the requested
provider, nur **blocks** the spawn (tool result is an error), opens the `/login`
modal pre-selected to that provider, and does **not** quietly re-run the
subagent on the parent backend. After you finish `/login`, nur injects a
**mandatory re-deploy** steer with the exact structured `agent({ "provider":
"…", … })` recipe so the model cannot omit `provider` on retry. Mid-turn steers
that name a provider get a short cross-provider deploy nudge for the same
reason.

**Auto-retry recipe (after `/login`):**

```text
agent({
  "provider": "<catalog-id>",
  "subagent_type": "general|explore",
  "description": "<original label>",
  "prompt": "<original task>",
  "model": "<optional exact model id>"
})
```

### `fractal`

Bridge to **[fractal](https://github.com/plasma-ai/fractal)** (Apache-2.0) —
hierarchical recursive agent loops in git worktrees. Each node is an isolated
worktree running its own autonomous loop; a parent spawns children for
separable subtasks to get multiplicative parallelism.

| Action | Behaviour |
|--------|-----------|
| `probe` · `doctor` · `status` | Binary, git repo, `.fractal` folder, worktrees |
| `init` | Repo-level root init (`fractal init <path> --agent=…`) |
| `node list` · `node status <name>` | Inspect the tree |
| `node start <name>` | Launch a node's loop. Run caps come from `config.json` (set at `fractal node init`), **not** from flags on `start` |
| `open` | fractal's own full-screen dashboard — nur suspends its TUI, hands over the terminal, and restores it on exit |

!!! warning "Unix only"
    fractal 1.0.0 imports the Unix-only `fcntl` module, so **every** invocation
    fails on Windows — including `--version` and `--help`. Use WSL or a
    Linux/macOS host. nur detects this and reports one clear line rather than a
    Python traceback. Requires **Python 3.12–3.14**:
    `pipx install plasma-fractal` (or `uv tool install plasma-fractal`).

Paths are sandboxed: `workdir`, `--path=…`, and any bare absolute path in the
free-text `args` must resolve inside the workspace.

### `penecho`

Bridge to **[penecho](https://github.com/penecho/penecho)** (AGPL-3.0) — an
infinite canvas for thinking beyond the chat box (20k × 20k, ink, MathJax,
plots, animation scenes). Run as a **sidecar**: nur launches it and maps auth
into its `config.env`; there is no linking.

| Action | Behaviour |
|--------|-----------|
| `probe` · `doctor` | Binary, `~/.penecho/config.env`, codex/claude CLI presence |
| `export` | Render a `config.env` mapping nur's provider settings to `AI_PROVIDER=api` |
| `atlas` | Describe a canvas image |
| `launch` | Sidecar launch instructions |

**The API key is never rendered.** `export` emits a redacted placeholder —
tool results are sent to the model provider and persisted to
`~/.nur/sessions/*.json`, so echoing a live key would write it to disk in
cleartext. Fill `AI_API_KEY` in `~/.penecho/config.env` yourself.

### `t3code`

Compatibility layer mirroring **[t3code](https://github.com/pingdotgg/t3code)**
(MIT) — delegate auth to vendor CLIs instead of storing secrets.

| Action | Behaviour |
|--------|-----------|
| `probe` · `probe_all` · `status` | Driver detection with env isolation (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`, …) so probing never disturbs a vendor's keychain |
| `delegate` | Report which vendor CLI can serve a provider, without copying its secret |
| `env` | The env map a driver instance needs |
| `pairing_create` | Issue a TTL-bounded pairing token (CSPRNG-backed) |

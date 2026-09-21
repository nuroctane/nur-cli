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
| **agent** | `todo_write` `submit_plan` `agent` `harness` | - |
| **memory** | `optmem` `connectome` `mem` `memory` `plur` `ruflo` | indigo |
| **judgments** | `typesafe` | violet |
| **context** | `context` `anydoc` | sky |
| **async** | `bg` `admission` `goal` `proposal` `message` | amber |
| **diagrams** | `excalidraw` `tldraw` `penecho` | pink |
| **policy** | `dogwood` | cyan |
| **python** | `repl` | teal |

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


### `repl`

Persistent Python kernel (Prime Intellect RLM "ipython" pattern): a long-lived
interpreter whose variables, imports, and functions **survive across turns and
compaction** - the subprocess outlives the chat window. Actions: `exec` (run
Python, keep state) · `expr` (eval, returns `repr`) · `bash` (a temporary
subshell; Python state persists) · `cd` (change the kernel cwd) · `status` ·
`list` · `kill`. `code` carries the Python or shell text; `cwd` sets this cell's
directory (defaults to the session cwd, and `cd` persists via `os.chdir`). Cells
run on `python`/`python3`/`py` (override with `NUR_REPL_PYTHON`) and time out
after 120s of no result (a blocking cell errors). Kernel state is stored under
`~/.nur/repl/`. Only `status` and `list` are read-only and free. It is **not**
for secrets - never paste keys or tokens into a cell.

<!-- src/tools/ipython_tool.rs:79 actions: exec|expr|bash|cd|status|list|kill -->

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

### `mem`

One intent-routed entry point over native hierarchical memory, the local vector
index, the knowledge graph, and optional HelixDB acceleration. Use `read` and
`write` for normal recall/storage, `vector` or `graph` for direct inspection,
and `helix_status` / `helix_sync` to diagnose or reconcile a configured Helix
resident. Helix mirroring is local-first and never replaces the native archive.
Explicit reads share one query embedding across these residents and deduplicate
the merged context by memory ID automatically.


### `connectome`

Agent-native memory + continuity (arXiv:2606.24775 four modules + Anima
Connectome): hierarchical self-authored memory (`recent`/`l1`/`l2`/`l3`), an
append-only chronicle, and checkpoints. Actions: `remember` (`text`, `tier`
default `l1`, `voice` `first_person`|`observed`, `tags`, `confidence`) · `recall`
(`query`, `k` default 8) · `list` · `consolidate` · `promote` · `extract` ·
`supersede` · `chronicle` (`text`, kind via `tier`) · `chronicle_tail` (`n`
default 20) · `checkpoint` (`name`, `note`) · `restore` (`name` - a soft restore
that describes the state as-of and never rewrites live history) · `status` ·
`graph` (knowledge-graph entity + neighbor listing). Read-only and free: `recall`
| `list` | `status` | `chronicle_tail` | `restore`; the rest write. Prefer
first-person `remember` for on-policy continuity, and never store secrets. Scope
defaults to `<project>:<NUR_SESSION_ID>` (or `<project>:global`); data is written
under `~/.nur/native-memory/` and `~/.nur/chronicle/`. Complements `memory`,
`optmem`, and `plur` - it does not replace them.

<!-- src/tools/connectome_tool.rs:89 actions: status|list|remember|recall|consolidate|promote|supersede|extract|chronicle|chronicle_tail|checkpoint|restore|graph -->

### `headroom`

Doctor / optional one-shot compress for [Headroom](https://github.com/headroomlabs-ai/headroom).
Inline compression of large tool results is **on by default** (`[headroom] enabled`);
missing install no-ops. Also `/headroom`.

### `context`

RLM prompt-as-variable store. Perception actions (`list` · `peek` · `slice` ·
`search` · `inventory`) are read-only; **`register` and `delete` write and remove
files under `~/.nur/context-store`, so they need approval** (Manual mode asks,
Plan mode blocks) rather than riding the read-only shortcut. `anydoc` splits the
same way: `convert`/`read`/`status` are perception, anything that registers a
converted document is not.


### `anydoc`

Convert PDF/DOCX/PPTX/XLSX/ODT/RTF/EPUB/CSV (and more) to clean GitHub-Flavored
Markdown via [Firecrawl anydoc](https://github.com/firecrawl/anydoc) - the local
`anydoc` Rust crate when built with `--features anydoc`, else an `anydoc` /
`firecrawl-anydoc` CLI on PATH. Actions: `convert` - the only action, and the
single dispatch path (`path` is required; the tooled `action` field is read but
never matched, so any value still converts). `register` (default true) stores the
markdown in the RLM context store as a `document` var (default name
`doc_<stem>`); `max_chars` caps the inline return only (default 12000) - the full
text stays in the store. Scanned-image PDFs are not supported (local text
extraction only; `pdf-inspector` errors as unsupported) - they need Firecrawl
hosted `/parse` OCR. Read-only and free in `capabilities` when `action` is
`convert` (the allowlist also names `read`/`status`, neither of which the tool
implements).

<!-- src/tools/anydoc_tool.rs:47 actions: convert (action field ignored; only conversion implemented) -->

### `typesafe`

System One judgments ([TypeSafe](https://docs.typesafe.ai), flagship model **Jev**)
for the decisions an agent loop is already making: which tool or model, how risky,
is this relevant, did that work, does this need a human. Actions: `ask` (batched -
any mix of `choice` / `noul` / `score` in one request) · `choice` · `noul` · `score`
· `pick` · `rank` · `classify` · `risk` · `verify` · `prune` · `spam` ·
`needs_human` · `route` · `reset` · `status`. Also `/typesafe` · `/jev`.

Options come from code (the caller's candidate list), answers come back with
probabilities, and a `confidence` below `[typesafe] escalate_confidence` is an
escalation rather than a judgment. Provider-agnostic: this boosts whichever model
you are using. Key from `TYPESAFE_API_KEY`, `/auth` → `TypeSafe · Jev`, or
`~/.nur/typesafe.key`. Details: [typesafe.md](./typesafe.md).


### `dogwood`

Runtime verification for AI agents ([dogwood-policy/dogwood](https://github.com/dogwood-policy/dogwood)):
a Cedar-extended policy language with temporal logic (`since`/`formerly`/`once`/
`count_within`) over an agent's event stream, used to govern tool calls. Actions:
`status` / `which` (report the `dogwood` CLI path and version, or the install
hint) · `check-parse` (`policy=`) · `validate` (`policy=`, optional `schema=`,
`event_schema=`, `providers=`, `macros=`) · `replay` (`policy=` + `trace=`) ·
`lower` (`policy=` + `schema=`, `emit=cedar-policies|cedar-schema|cedar-json|both`)
· `schema` (`schema_kind=action|event|providers` with `input=`, or `mcp` with
`manifest=`). Every evaluation action is read-only and free. Requires the
`dogwood` CLI on PATH: `cargo install --git https://github.com/dogwood-policy/dogwood amzn-dogwood-cli`.
Each CLI call is capped at 120s. Caveat stated in the code: the reference
interpreter is explicitly **not** production-grade enforcement - treat it as an
on-demand evaluation/guardrail layer, not a trust anchor and not a runtime gate on
every tool call.

<!-- src/tools/dogwood_tool.rs:139 actions: status|which|check-parse|validate|replay|lower|schema -->

### `optmem`

Permanent memory via [OptMem](https://github.com/VictorTaelin/OptMem) at `~/.optmem`
(upstream path, not under `~/.nur`). Actions: `status` · `doctor` · `wake` · `note` ·
`nap` · `recall` · `zoom` · `forget` · `config`. Also `/optmem` · `/memo`.

**`nap` is housekeeping, not a task queue.** Upstream renders one pending
compression block at a time and prints its instructions again after every applied
block, so applying them one at a time reads as an endless instruction chain
(observed live: six applied blocks before the agent stopped the chain by hand).
nur therefore never forwards upstream's prompt as an imperative:

- `nap` shows the next block, the count, and says plainly that nothing depends on it;
- `nap` with `range` + `text` applies that one block;
- `nap` with `lines=[…]` (one line per block, in order, max 24) **drains several in
  one call** - the normal way to finish the queue;
- after `[typesafe]`-style housekeeping limits (two single-block applies in a row),
  nur stops showing the next block and points at `lines=[…]` instead.

Nothing about the queue blocks a turn, and a non-empty queue is not an error.

### `egaki`

Image/video generation via [egaki](https://github.com/remorses/egaki). Prefer
`egaki login --provider chatgpt` when using a ChatGPT subscription. Slash:
`/egaki` (bare `/image` attaches a file for vision, it does not generate).

### `akarso`

Post, schedule, and reply across 14 social platforms (X, LinkedIn, Instagram, Facebook, TikTok, YouTube, Threads, Reddit, Bluesky, Mastodon, Discord, Slack, Pinterest, Google Business). `action=auth_check|accounts_list|accounts_health|accounts_get|accounts_connect|posts_list|posts_get|posts_create|posts_delete|profiles_list`. Read actions are free; `posts_create` (publish/schedule), `posts_delete`, and `accounts_connect` are outward-facing and approval-gated. Requires `akarso auth login` once. Also `/akarso`.

### `executor`

Dispatch to external APIs.

### `browser`

Perceive and control the user's **real, default browser** — Arc, Chrome, Edge,
Brave, or any Chromium browser — with login state preserved, via
[agent-browser-cli](https://github.com/sleepinginsummer/agent-browser-cli).
Perception is free: `tabs`, `scan`, `snapshot` (page → `@e` element refs),
`pick` (the snapshot's indexed table → Jev chooses the operation and the target
in one request, then you execute with `click`/`fill`; it refuses to pick when
fewer than two elements parse),
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
`terminal-browser` is installed there. Upstream ships native macOS and Linux
builds (0.7.x); Windows still relies on the host fallback. Upstream's agent
surface is the agent-browser-compatible `terminal-browser action` CLI.

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
Provisioning requires **omp >= 18.0.9** (feature floor; `nur ecosystem ensure`
auto-upgrades). Bun installs require version 1.3.14 or newer.

### `skill`

List or load a skill pack (`SKILL.md`) into context.

| Action | Behaviour |
|--------|-----------|
| `list` | Installed skills under `~/.nur/skills`, enabled plugins, project skill dirs |
| `read` | Full skill body (large packs are not truncated the way the system catalog is) |

**Natural-language activation:** many workflow skills also auto-activate from plain wording (no slash, no first `skill` call). Examples: *think like fable*, *TDD this*, *debug systematically*, *polish the UI*, *resume from Claude*. When that fires, the harness injects the skill body for the whole turn and shows a status chip. Details: **[Ecosystem → Natural-language skill activation](ecosystem.md#natural-language-skill-activation)**.


### `harness`

Continual harness lite (Prime Agent `/refine`): append evidence-backed operating
lessons to per-session supplemental state that outlives a chat window. Actions:
`status` · `refine` (`lesson` required, max 2000 chars; optional `evidence`,
truncated to 500 chars) · `rollback`. `refine` snapshots the current state first,
appends the lesson as a bullet, and bumps `revision`; supplemental is capped at
12000 chars (older notes trimmed, newest 8000 kept). `rollback` restores the most
recent snapshot and bumps `revision` again. It **never** rewrites the immutable
base system prompt - supplemental notes are injected separately (see
`harness::prompt_block`). State and snapshots live under
`~/.nur/harness/<session>/` (`state.json`, `snapshots/`); the session key is
`session_id` or `NUR_SESSION_ID` (default `default`). Only `status` is read-only
and free; `refine` and `rollback` are write-class.

<!-- src/tools/harness_tool.rs:48 actions: status|refine|rollback -->

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


### `goal`

Persistent goal (Prime Agent pattern): a durable objective that survives across
turns. Actions: `get` · `set` (`text` required, optional `token_budget`) ·
`complete` (optional `note`) · `pause` · `resume` · `clear`. Only `complete` marks
successful completion. `get` is read-only and free; the rest write. State is
stored per session under `~/.nur/goals/<session>.json`, keyed by `session_id` or
`NUR_SESSION_ID` (default `default`). Also `/goal`.

<!-- src/tools/goal_tool.rs:49 actions: get|set|complete|pause|resume|clear -->

### `proposal`

Retained-output proposals (Shepherd pattern): when the config key `proposal_mode`
is on, write tools stage changes under `~/.nur/proposals/<session>/` instead of
the workspace until you review them. Actions: `list` (free) · `apply` (copies the
staged files into the current working directory) · `discard`. Review staged files
before apply. The session key is `session_id` or `NUR_SESSION_ID` (default
`default`).

<!-- src/tools/proposal_tool.rs:46 actions: list|apply|discard -->

### `admission`

Retrieve results of asynchronously admitted subagents (Prime `rlm()` admission-handle
model, RLM paper). A child spawned with `agent async=true` returns a handle id
immediately and runs in the background; poll it here. Actions: `list` (handles in
this session - id, state `running`/`done`/`failed`, description) and `get` with
`id` (render one handle). `list` is read-only and free; `get` is not, so it takes
the approval path. Handles live under `~/.nur/admissions/<session>/`, scoped by
`NUR_SESSION_ID` (default `default`). `all` renders every handle in the session in one call (added because the
description and schema had promised it while only `list`/`get` existed).

<!-- src/tools/admission_tool.rs:45 actions: list|get|all -->

### `message`

Agent-to-agent messaging (a pi-peer port): per-session inboxes under
`~/.nur/peers/`, real heartbeat presence, and true file receipts (consumed vs
queued). Actions: `send` (`to=<peer name|id|all>`, `text=`) · `recv` (drain this
session's inbox) · `peers` (a.k.a. `list` - presence, cwd, status) · `inbound`
(show, or set with `policy=accept|ask|refuse`) · `status`. Read-only and free:
`recv` | `status` | `peers` | `list`; `send` and `inbound` policy changes are
write-class. Scope defaults to the project directory name (override with `scope=`).
Boundary stated in the code: inbound peer mail carries **no authority** - it cannot
approve actions or change config, and slash commands inside it are inert.

<!-- src/tools/message_tool.rs:67 actions: send|recv|peers|list|inbound|status -->

### `bg`

Push long-running work off the agent turn so the CLI stays interactive. `run`
spawns a shell command (Windows `cmd /C <command>`, elsewhere `sh -c <command>`)
or a `program` + `args` pair, and returns a job id immediately. `list` / `chip`
show running jobs, and `status` / `result` / `cancel` each take an `id`. Read-only
(free): `list` | `status` | `result` | `chip`; `run` and `cancel` are write-class.
Diagram tools (`tldraw install`, `penecho install`, long exports) accept
`background=true` and route through this module. TUI: `/bg`, and the status chip
shows running jobs. A `spawn_label` action used to be advertised in the description while nothing implemented it (and nothing called it): the description now lists exactly the implemented set.

<!-- src/tools/bg_tool.rs:69 actions: list|chip|status|result|cancel|run -->

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


### `tldraw`

tldraw offline desktop app ([tldraw-offline](https://github.com/tldraw/tldraw-offline))
for interactive `.tldraw` boards. `status` / `detect` report the app, the local
API (port and token), and open docs; `install` downloads and runs the official
platform installer (network access; `background=true` returns a bg job id);
`create` writes a valid static Desktop `.tldraw` from a `shapes` list
(contrast-safe, dark theme) and opens it; `open` / `run` launches a path and
auto-enables document scripts; `enable_scripts` re-enables scripts on an open
board; `api` runs JS (`code=`) against the live canvas (needs the app running with
its local API); `screenshot` / `export_png` write a PNG (default
`.nur/media/tldraw-*.png`); plus `list_docs` and `fit_camera`. Read-only (free):
`status` | `detect`; everything else mutates the desktop or canvas and needs
approval in manual mode. Boards go to the Desktop (reserved for tldraw; excalidraw
refuses that path). Never invent `.tldraw` JSON with `write_file` - it is not a
valid document and will not open usefully.

<!-- src/tools/tldraw.rs:93 actions: status|detect|install|open|run|create|enable_scripts|api|screenshot|export_png|list_docs|fit_camera -->

### `excalidraw`

Create hand-drawn diagrams via [excalidraw-cli](https://github.com/ahmadawais/excalidraw-cli)
and open them for the user. `create` writes a `.excalidraw` file from `elements`
(a JSON array or string), `elements_path`, or `from_mermaid` (flowchart lines like
`A[Start] --> B[End]`), then uploads it to excalidraw.com and opens the share URL
in the default browser (`open=true` by default). `export` uploads an existing
`path` and opens its share URL; `reference` prints the element-format reference;
`status` reports the CLI path and version. `checkpoint` takes sub-actions via
`checkpoint_action`: `list` (free) | `save` (`name` + `path`) | `load` | `remove`.
Read-only (free): `status`, `reference`, and `checkpoint` `list`; `create`,
`export`, and the other checkpoint mutators need approval in manual mode. Requires
`excalidraw-cli` on PATH - `npm i -g excalidraw-cli`, or run `nur ecosystem` to
auto-provision it when Node is available. Output defaults to
`.nur/diagrams/<slug>.excalidraw`; Desktop is refused (that path is reserved for
tldraw). It never OS-opens the local `.excalidraw` file - browser share URL only
(avoids the Windows "Open with" dialog).

<!-- src/tools/excalidraw.rs:101 actions: status|reference|ref|create|export|checkpoint -->

### `t3code`

Compatibility layer mirroring **[t3code](https://github.com/pingdotgg/t3code)**
(MIT) — delegate auth to vendor CLIs instead of storing secrets.

| Action | Behaviour |
|--------|-----------|
| `probe` · `probe_all` · `status` | Driver detection with env isolation (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`, …) so probing never disturbs a vendor's keychain |
| `delegate` | Report which vendor CLI can serve a provider, without copying its secret |
| `env` | The env map a driver instance needs |
| `pairing_create` | Issue a TTL-bounded pairing token (CSPRNG-backed) |

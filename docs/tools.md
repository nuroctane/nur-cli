# Tools

All native tools available to the NurCLI agent.

Large tool results are stored on disk with a bounded preview. The preview's
size budget includes the complete spill path; long paths and tiny limits stay
within that budget. If the path cannot fit, the result is truncated without a spill.

## Tool families

| Family | Tools | Colour |
|--------|-------|--------|
| **read** | `read_file` `list_dir` `sgrep` `grep` `glob` | sky |
| **edit** | `write_file` `edit_file` `multi_edit` `apply_patch` `hashline_edit` | violet |
| **shell** | `bash` | amber |
| **vision** | `look` `extract_frames` | pink |
| **web** | `web_search` `web_fetch` | teal |
| **git** | `git_status` `git_diff` | cyan |
| **browser** | `browser` `terminal_browser` | teal |
| **knowledge** | `graphify` `graphjin` `plur` `ruflo` `akarso` `executor` `skill` `memory` | indigo / orange |
| **delegate** | `omp` | - |
| **agent** | `todo_write` `submit_plan` `agent` `harness` `codemode` `tool_search` | - |
| **memory** | `optmem` `connectome` `mem` `memory` `plur` `ruflo` | indigo |
| **judgments** | `typesafe` | violet |
| **security** | `enclave` | teal |
| **context** | `context` `anydoc` | sky |
| **async** | `bg` `admission` `goal` `proposal` `message` | amber |
| **diagrams** | `excalidraw` `tldraw` `penecho` | pink |
| **policy** | `dogwood` | cyan |
| **python** | `repl` | teal |

Which schemas ride on every request depends on `[tools] profile`:

- **`lean` (default):** the editing shell and session tools are always offered (`read_file`, `list_dir`, the edit tools, `bash`, `sgrep`/`grep`/`glob`, `web_search`/`web_fetch`, `look`, `git_status`/`git_diff`, `context`, `goal`, `message`, `bg`, `skill`, `memory`, `question`, `todo_write`, `submit_plan`, `agent`, `codemode`, `tool_search`). Every other tool is listed by name and one line in the system prompt and loads through `tool_search`; from the next request on the model calls it by its real name, so permissions, approvals and plan mode see the real tool. The loaded set is read back from the conversation, so `/resume` keeps it. About 6k tool-schema tokens per request instead of 15k.
- **`full`:** every schema on every request.
- **`core`:** 16 local tools, no ecosystem tools.

Capability flags (read-only / concurrency-safe / destructive) drive parallel batching and approvals.

### Tool results and context

- Results larger than `tool_result_max_chars` (default **12000**) are written under `~/.nur/tool-results/` and the model gets head + tail around a `...N bytes truncated; complete output in <path>...` marker (use `read_file` for the middle). `bash` bounds its own output once (see below) and is not re-spilled, so a result is never previewed twice.
- Set `tool_result_max_chars = 0` for unlimited inline results (legacy behaviour).

---

## Read tools

### `read_file`

Read the contents of a file.

### `list_dir`

List directory contents.

### `sgrep`

Semantic code search, the first tool the agent reaches for when it knows what the code *does* but not what it is called. Backed by [sgrep](https://github.com/context-dot-dev/sgrep) from Context.dev (MIT): ripgrep lists the files, static embeddings (`potion-code-16M-v2`, 32 MB, CPU only) plus BM25 rank every chunk, exact identifier hits sort first, and the index is cached under `~/.cache/sgrep`. Nothing leaves the machine; the only network traffic is the one-time model download.

Two modes:

- **discover** - `query` alone: `sgrep "where are retries with backoff"` over `path` (default cwd), the top `count` chunks (default 5, max 25) as `path:start-end` followed by the chunk text.
- **rerank** - `query` + `pattern`: ripgrep finds every line matching the regex (three lines of context, optional `glob`), sgrep ranks only those passages by the query. A broad grep comes back as the few blocks that matter.

Arguments: `query` (required), `path`, `pattern`, `glob`, `count`, `model` (`auto` | `code` | `text`; `text` for prose corpora). Chunks longer than 4000 bytes are cut with a `[... N more bytes - read_file for the rest]` marker. One run is bounded at 120 s and killed on cancel; the first run on a repository embeds it, later runs hit the index.

When the binary is missing the tool answers through `grep` (the regex, or the query's identifiers as alternatives) and leads with `[sgrep not installed - searched with grep instead. Install: …]`, so a model that leads with `sgrep` never wastes the call. The system prompt tells every model to lead with `sgrep` for intent queries and unfamiliar code, `grep` for an exact identifier or regex, `glob` for file names.

Models trained on grep-first habits are held to this rather than reminded of it (`[tools] sgrep_first = true`, the default, and only while sgrep is installed):

- A `grep` whose pattern reads like a description (three or more words, no regex syntax - the kind grep matches literally and finds nothing for) is answered by sgrep in discover mode; the result opens with `[grep: "…" reads as a description … answered by sgrep query="…"]`.
- A `grep` that overflows the **default** `max_matches` comes back as sgrep's top eight passages for that pattern (rerank mode, same glob/type/case/literal flags) instead of the first page, with a header saying how to get the exhaustive list: set `max_matches` yourself, which is always honored as written.
- A `bash` command that is a workspace search - `grep`, `rg`, `ag`, `ack`, `findstr`, `Select-String` as the command, including behind `cd …` or a bare `cat file |` - is refused before it runs, naming the tool calls that do the job. Filtering another command's output (`git log | grep fix`, `cargo test | grep FAILED`) is not a search and runs.

When sgrep fails mid-call (timeout on a first index, a crash), grep's own result is returned, so routing never costs an answer. `sgrep_first = false` turns all three off.

Install: `nur ecosystem ensure` builds it with `cargo install --git https://github.com/context-dot-dev/sgrep --locked` and warms the model; the installer scripts do the same. `nur doctor` shows the resolved path. `NUR_SGREP_BIN=<path>` pins the binary; `NUR_SGREP_BIN=` (empty) disables it.

### `grep`

Search file contents with a regex. Uses ripgrep when available (`NUR_RG_BIN`, PATH, then the usual cargo/scoop/winget/Homebrew locations, probed once per process), otherwise a native walk with the same semantics. Both paths respect `.gitignore`, include hidden files, and skip `node_modules`, `target`, `.git`, `dist`, … at any depth.

Arguments: `pattern`, `path`, `case_insensitive`, `fixed_strings` (literal text, nothing to escape), `glob` (`*.rs`, `src/**/*.ts`, `!*.test.js`), `type` (ripgrep file type), `context` (lines around each match, max 10), `multiline` (ripgrep only), `max_matches` (default 50, max 500). Output is `path:line:text` sorted by path with forward slashes; a `[truncated at N matches …]` trailer means more exist. ripgrep's own errors (bad regex, unknown type) come back as the tool error instead of an empty result.

### `glob`

Find files by name or glob. Uses `rg --files` when available, with the same excludes and hidden-file policy as `grep`. Loose patterns are made precise: `src/` lists everything under a `src` directory, `mod.rs` finds that file anywhere, `.env` matches `*.env`, `*.rs` / `**/*.test.ts` are used as given. Paths are sorted, forward-slashed, capped by `max_results` (default 200, max 1000) with a `[truncated …]` trailer.

---

## Edit tools

All four share one commit path: the new contents are written to a temp file and renamed into place (never a half-written file), the file's line endings are kept (a CRLF file stays CRLF), and the bytes written are remembered so the next edit chains without a re-read. If a file the model has read was changed since (formatter, shell command, user, subagent), the edit is refused with `changed since you last read it` until the model reads it again; files the session never read are not checked, so creating files and editing on instruction keep working. `/undo` restores the pre-edit snapshot only while the file still holds that edit; once something else changed it, undo refuses rather than discard the later work.

### `write_file`

Write content to a file. Creates the file if it doesn't exist, overwrites if it does.

### `edit_file`

Apply targeted string replacements to a file. Requires exact string matching, with two fallbacks: a line-ending-insensitive match, then a match after trimming the whitespace margins of `old_string` (the same margins are dropped from `new_string`, nothing more). Both fallbacks say so in the result.

### `multi_edit`

Apply multiple edits to a file in a single operation. Every edit is validated before anything is written: an empty or unmatched `old_string` fails the whole call and leaves the file untouched.

### `hashline_edit`

Edits an existing file by line number instead of by retyping old text (oh-my-pi's "hashline" format). `read_file` heads every read with `[path#TAG]`, a 4-hex snapshot of the whole file; pass that `tag` with the edits, and a file that changed since the read is refused before anything is written. Numbers always refer to that snapshot, never to lines shifted by an earlier operation in the same call.

```text
PUT 4.=6:        replace lines 4-6 with the + rows below (PUT 4.=4: for one line)
+    new text
PUT <1:          insert at the file head (PUT <N: before line N)
PUT >$:          append at the end (PUT >N: after line N)
CUT 8.=9 @fn     delete lines 8-9, keeping them in register @fn
PUT >12 @fn      paste the register after line 12 (no body)
PUT 20*:         replace the block line 20 opens
CUT 20*          delete it;  PUT >20*:  insert after it
```

Body rows are `+` and the exact final text (`+` alone is a blank line). The file's line endings, BOM and final newline are kept, and the result returns the new tag with the changed lines, so the next edit needs no re-read.

A block anchor `N*` goes anywhere a range or `>N` does and means line N through the end of the block it opens: its brackets until they close (a `{` alone on the next line counts, and `} else {` starts a new block), the indented body under a line ending in `:`, or a Markdown heading's section up to the next heading of the same or higher level. Brackets in strings and `//` comments are ignored. A line that opens none of these is refused with a request for the explicit range, never guessed. File moves and deletes are left to `bash`.

### `apply_patch`

Apply a unified diff patch to a file. `@@ -N,0 +M,k @@` inserts after line N; hunk positions follow the original numbering, with earlier hunks' growth accounted for; context that does not match is an error, never a guess, and ambiguous context (several identical sites) is refused.

---

## Shell

### `bash`

Execute shell commands in the workspace cwd (cwd-scoped, not a full OS sandbox). Hardened with:

- **Denylist** — blocks dangerous commands (e.g. `rm -rf /`, fork bombs) and hang-prone patterns (dev servers, `watch`, interactive prompts)
- **Timeout with handoff** — default **60s**, hard max **600s**. A command that has produced output in the last 15 s at the deadline is **not killed**: it moves to a `bg` job and the model gets a "still running" placeholder with the job id and the output so far, so the turn carries on and the result arrives as a completion note in a later turn (or on demand via `bg(action=result, id=N)`). A command that has been silent is killed and reported as timed out, and the error says how to retry (`timeout_ms` up to the cap, or `background=true`). `background=true` hands off almost at once (long builds, test suites). Handed-off jobs still end after 30 minutes. `shell_timeout_handoff = false` in `config.toml` restores kill-on-timeout.
- **Idle kill** — no output for ~90s after a grace period kills the process tree (a prompt waiting on stdin never finishes on its own)
- **Whole-tree kill** — the command is spawned inside a Windows Job Object (kill-on-close) or as its own Unix process group, so a timeout, a cancel, or nur exiting takes the shell *and* everything it started (build servers, test runners) with it. No grandchild can escape by forking before the job exists: the child starts suspended and is resumed after assignment.
- **Lean result** — the model receives the stdout alone; `Stderr:` and `Exit code: N` sections are added only when non-empty / non-zero, `(no output)` when nothing was printed. No per-call header, so a typical result costs a few tokens.
- **Bounded output** — stdout keeps `max_output_length` bytes (default `min(80000, tool_result_max_chars)`, max 400000) as **head + tail** around a `...N bytes truncated; complete output in <path>...` marker, with the full text under `~/.nur/tool-results/`. Output past a 2 MB capture cap is discarded and the result says so.
- **Failure lockout** — a command that hit its deadline may be retried in the same cwd only with a **larger** `timeout_ms` (or `background=true`); the identical budget is refused for 15 minutes with the budget that failed. Commands killed for idling or cancelled by the user are refused outright for 15 minutes, since more time would not help. A bg handoff does not count as a failure.
- **Non-interactive env** — `CI=1`, `GIT_TERMINAL_PROMPT=0`, stdin null. Bash runs as a **non-login** shell (`bash -c`), so `.bash_profile` / `.profile` are not sourced on every call; set per-project environment in the command itself or through hooks.

!!! note "Shell backend"
    NurCLI uses Git Bash on Windows when available, otherwise falls back to PowerShell. On macOS/Linux it uses Bash. Check with `nur doctor`. Prefer `list_dir` / `read_file` / `sgrep` / `grep` / `glob` over shell for file IO and search. Pipelines, `&&` chains and redirections are fine; what is refused is the denylist, hang-prone interactive commands, and repeats of a command that already hung.

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

Search the web (DuckDuckGo). With `TREG_TOKEN` set, [treg](https://treg.to/web-access)'s free routed search runs first and DuckDuckGo is the fallback.

### `web_fetch`

Fetch a public URL (SSRF-checked on every redirect hop). With `TREG_TOKEN` set, a page that refuses the request (401, 403, 429, 5xx) or only renders with JavaScript (x.com, most single-page apps) is read through `treg.web.extract` and returned as markdown. treg calls prefer the free vendor and carry a 2-cent ceiling each; without a token nothing goes to treg.

---

### `tool_search`

Loads deferred tools in the `lean` profile. Pass `names` (exact tool names from the system prompt's "Loaded on demand" list) or a `query` describing the job; the result carries each tool's full schema, and the tool joins the request from the next step on. Read-only and free in plan mode.

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

`remember` also accepts optional `triggers`: up to eight short future-query
cues (160 characters each). For example, a factual food restriction can carry
`["choosing a restaurant", "planning dinner"]`. Recall searches these cues,
but only the stored fact becomes memory evidence in the prompt. This adopts
the separation of retrieval cues and evidence from
[T-Mem](https://github.com/Sherlockwz/T-Mem); it is a bounded native adaptation,
not the upstream extraction, embedding, reranking, or evaluation pipeline.
No additional model call or service is required. Existing memories remain compatible.

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

The store is bound to the calling session (the one that registered the
variables, also after `/new`, `/resume`, a fork or inside a subagent); there is
no `session_id` argument. A parent hands a subagent variables by naming them in
the `agent` call's `context_vars`: the child reads exactly those, read-only,
while it runs, and nothing else of the parent's store (spilled tool output,
oversized pastes) is visible to it. Its own registrations stay in its store. Tool results are auto-registered only when they are
over `tool_result_max_chars` and will therefore reach the model as a preview;
smaller results arrive whole and are not registered, and the store's own reads
are never registered. `peek`/`slice` honour `max_chars` up to the inline limit
(`tool_result_max_chars` minus a small header), so a window of that size arrives
intact; the header ends with `next_offset=N` or `(end)`.


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

### `enclave`

[Enclave](https://enclave.ai) security agents over native MCP (Streamable HTTP to
`https://mcp.enclave.ai/`): pentests, code security review, findings, CVE
monitoring. Actions: `status` · `tools` (catalog with parameters and the server's
instructions; `tool=<name>` for one full schema) · `call` (`tool` + `arguments`, an
object matching that tool's schema). `status` and `tools` are read-only; `call`
is approval-gated and high impact, and an "always" rule is scoped to one Enclave
tool. Key from `ENCLAVE_MCP_API_KEY` or `nur auth login --provider enclave`.
Also `/enclave`. Details: [enclave.md](./enclave.md).


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
used). nur does not install omp unless `ecosystem_omp = true`; an omp already
on PATH is used. The `agent` tool with `cost_mode: economy` does the same kind
of delegation natively. Strong at LSP-backed refactors, debugger-driven diagnosis (DAP), and AST
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
Provisioning requires **omp >= 18.4.4** (feature floor; `nur ecosystem ensure`
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

### `codemode`

Run a short JavaScript script that calls other tools, so several steps cost
one model round instead of one each. An agent loop re-sends the whole
conversation on every request, so five reads cost five re-sends of the
history; a script that does the same five reads, filters them and prints one
line costs one. The system prompt tells the model to reach for it when the
next step is three or more read-only calls whose outputs feed each other and
only part of the result matters.

```js
// code: the body of an async function; top-level await and return work
const files = tools.glob({ pattern: "src/**/*.rs" }).split("\n");
let todo = 0;
for (const f of files) if (tools.read_file({ path: f }).includes("TODO")) todo++;
text(`${todo} of ${files.length} files carry a TODO`);
return todo;
```

Only what the script prints (`text(...)` / `console.log(...)`) or returns
reaches the model, followed by a one-line account of the calls it made. Globals:
`tools.<name>(args)` (returns the tool's text result, throws on error),
`ALL_TOOLS`, `searchTools(q)`, `describeTool(name)`, `CWD`, `store(k, v)` /
`load(k)` (values survive into the next script of the session), `exit(value)`.
Parameters: `code` (required) and `timeout_ms` (default 120 s, 1 s to 10 min).

**Policy.** The script runs in [Boa](https://boajs.dev), a pure-Rust engine with
no filesystem, network, timer or process bindings, so the tool bridge is the
only way out. The bridge admits a nested call only when the permission layer
classifies it as read-only (the same `is_read_only` rule the approval gate and
the parallel batch use, not a second allowlist), and never `codemode`, `agent`,
`question`, `submit_plan`, `message`, `admission`, `bg`, `fractal` or `omp`. A
script therefore cannot write a file, run a shell command or spawn a subagent;
`codemode` itself is classified read-only, so it is free in plan mode and needs
no approval in manual mode. Limits: 64 nested calls per script, 1 MB of printed
output, 256 KB per stored value (1 MB per session), ten million loop iterations
and a 400-deep call stack. Nested calls run one after another; `await` and
`Promise.all` work but do not overlap. Subagents are not offered the tool.
Cargo feature `codemode` (default on). E2E scenario 32.

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

**Economy children.** `cost_mode: "economy"` runs the child on a cheap model
with low reasoning effort: `economy_model` from config when the child stays on
the parent's provider, otherwise the cheapest model the account lists that
models.dev prices and does not mark as unable to call tools. Nothing external
is launched; when no priced model is found the child stays on its model and the
status line says so. An explicit `model` always wins.

**Cross-provider subagents.** An `agent` call may set `provider` (and optionally
`model`) to run a subagent on a *different* provider than the parent — in natural
language. `provider:"grok"` → xAI, `"gemini"`/`"google"` → Google, `"claude"` →
Anthropic, `"chatgpt"`/`"gpt"` → OpenAI, `"antigravity"`/`"agy"` → Antigravity
(own OAuth — not the same as Gemini), plus `deepseek`, `mistral`, `kimi`, and
direct catalog ids. Omit both fields to inherit the parent's provider/model.
This lets one turn fan out across providers — e.g. a Claude reviewer alongside a
Grok auditor — from a single prompt. A child on any provider can read the
`context` variables the call names in `context_vars` (see `context` above);
when that child runs on another provider, the status line says how many were
shared with it. The conversation history each provider receives is rebuilt for
its wire protocol:
fields another vendor keeps (`reasoning_content`, `thought_signature`, encrypted
reasoning made on another account) are left out, and tool-call ids are mapped
to the form the vendor accepts (Anthropic's charset, Mistral's nine characters,
OpenAI's 40-character limit).

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
before apply. Proposals belong to the session that staged them; there is no
`session_id` argument.

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
Inbound mail defaults to `refuse`: nothing arrives until you opt in with
`message_inbound = "ask"` (you approve each letter before the model sees it;
held when nobody can answer) or `"accept"`. Every registered session carries a
mailbox secret that outgoing letters present, so a local process cannot forge
mail from a live session. Delivery is wrapped in an explicit
`<peer-mail authority="none">` block: inbound peer mail carries **no
authority** - it cannot approve actions or change config, and slash commands
inside it are inert.

<!-- src/tools/message_tool.rs:67 actions: send|recv|peers|list|inbound|status -->

### `question`

Close-ended user questions (OpenCode-desktop-style clarification): `question`
(required), `header` (short modal title), `options` (`label` + optional
`description`, 2-8 items, required), `multiSelect` (default false). The TUI
shows a modal - `1-8` pick, arrows + Enter, Space toggles multi-select, Esc
dismisses; headless `nur run` answers over stdin the same way. Fenced on
purpose: goal-driven turns, subagents, and headless integrations never
interrupt the user (they get a redirect to `BLOCKED:` instead), and a Jev gate
resolves the question without a modal when context, tools, or a safe default
suffice. A dismissed question returns guidance, never an invented answer - do
not re-ask immediately.

<!-- src/tools/question_tool.rs:30 no action enum; executed by the loop, not dispatch -->

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

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
| **browser** | `browser` | teal |
| **knowledge** | `graphify` `plur` `ruflo` `executor` `skill` `memory` | indigo / orange |
| **delegate** | `omp` | — |
| **agent** | `todo_write` `submit_plan` `agent` | — |

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

Execute shell commands. Hardened with:

- **Denylist** — blocks dangerous commands (e.g. `rm -rf /`, fork bombs)
- **Timeout** — commands are killed after a configurable timeout
- **Sandbox** — when available, runs in an isolated environment

!!! note "Shell backend"
    NurCLI uses Git Bash on Windows when available, otherwise falls back to PowerShell. On macOS/Linux it uses Bash. Check with `nur doctor`.

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

Query the code knowledge graph. See [Ecosystem](ecosystem.md).

### `plur`

Search shared engram memory.

### `ruflo`

Search vector memory.

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

### `omp`

Delegate a focused coding task to the [Oh My Pi](https://omp.sh) agent backend
(headless one-shot `omp -p` run in the workspace — the IDE/ACP surface is not
used). Strong at LSP-backed refactors, debugger-driven diagnosis (DAP), AST
rewrites, and web research. `run` is write-class: it needs approval in manual
mode and is blocked in plan mode; `status`/`version` are free. Provisioned by
`nur ecosystem ensure` when Bun is installed.

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

Spawn a subagent for complex tasks.

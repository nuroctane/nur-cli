# Tools

All native tools available to the Meta CLI agent.

## Tool families

| Family | Tools | Colour |
|--------|-------|--------|
| **read** | `read_file` `list_dir` `grep` `glob` | sky |
| **edit** | `write_file` `edit_file` `multi_edit` `apply_patch` | violet |
| **shell** | `bash` | amber |
| **vision** | `look` `extract_frames` | pink |
| **web** | `web_search` `web_fetch` | teal |
| **git** | `git_status` `git_diff` | cyan |
| **knowledge** | `graphify` `plur` `ruflo` `executor` `skill` `memory` | indigo / orange |
| **agent** | `todo_write` `submit_plan` `agent` | — |

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
    Meta CLI uses Git Bash on Windows when available, otherwise falls back to PowerShell. On macOS/Linux it uses Bash. Check with `meta doctor`.

---

## Vision tools

### `look`

Attach workspace images or video so the model sees them. Accepts png, jpg, webp, gif (direct) and mp4 (direct up to ~20 MB). See [Vision](vision.md) for details.

### `extract_frames`

Extract keyframes from video via ffmpeg. Output goes to `.meta/frames/<name>/`.

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

### `omp`

Delegate a focused coding task to the [Oh My Pi](https://omp.sh) agent backend
(headless one-shot `omp -p` run in the workspace — the IDE/ACP surface is not
used). Strong at LSP-backed refactors, debugger-driven diagnosis (DAP), AST
rewrites, and web research. `run` is write-class: it needs approval in manual
mode and is blocked in plan mode; `status`/`version` are free. Provisioned by
`meta ecosystem ensure` when Bun is installed.

### `skill`

Load a skill into context.

### `memory`

Read or append to the cross-session memory journal (`~/.meta/memory.md`).

---

## Agent tools

### `todo_write`

Create and manage task lists.

### `submit_plan`

Submit a plan for approval.

### `agent`

Spawn a subagent for complex tasks.

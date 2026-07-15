# TUI

The Nur-gold terminal UI for interactive sessions.

!!! quote "Built with Ratatui"
    The entire interface — cards, borders, animations, drag-select, and the
    scrollbar — is built on **[Ratatui](https://ratatui.rs/)**
    ([github.com/ratatui/ratatui](https://github.com/ratatui/ratatui)) with
    **[crossterm](https://github.com/crossterm-rs/crossterm)** for input/rendering.
    Enormous thanks to the Ratatui folks. 💙

## Opening the TUI

```bash
nur                     # fresh session
nur "fix the bug"       # start with a prompt
nur -c                  # continue last session
nur -r <session-id>     # resume specific session
```

### Opening banner

The splash is intentionally lean:

1. MUSE art  
2. **`<active provider> loaded · v<cli>`**  
3. **`model · cwd · session`**  
4. Purple ecosystem line (**sandbox · subagents · tools** + pack status)  
5. Permission **mode**

Mouse/keyboard interaction tips that used to live under the art are behind
**`/tips`** so the banner stays short. The active provider name comes from
`config.provider` (see [Authentication](authentication.md)).

---

## Keyboard shortcuts

### Navigation

| Key | Action |
|-----|--------|
| `↑` `↓` · mouse wheel · drag scrollbar | Scroll transcript |
| **Drag on chat text** | Select + auto-copy (keeps range while you scroll; works on expanded thought/tool bodies) |
| **Click `↓ N · End`** | Jump to latest message |
| Click the exact **click to peek** text | Opens stable dialogue (frozen position; Esc · outside · ✕ only) |
| Click `▸` | Expand / collapse in place |
| **Ctrl+C** (peek open, no selection) | Copy full thought / tool body |
| **Right-click or double-click a prompt** | Prompt menu: **fork · revert · copy** (works on the sticky header too) |
| `Ctrl+P` / `Ctrl+N` · `Alt+↑`/`↓` | Previous / next prompt from history |

!!! tip "Peek dialogue"
    Click a card (or a finished turn's timing strip) to pin a peek box with its
    full content. Close it with the **✕**, `Esc`, or a click **anywhere outside**
    the box.

### Input

| Key | Action |
|-----|--------|
| **Ctrl+A** | Select-all input (or whole transcript if input empty) |
| **Ctrl+C** | Copy selection (transcript or input); else interrupt / double-tap quit |
| **Ctrl+V** | Paste into input |
| **Ctrl+X** | Cut input selection (or whole input) |
| `Enter` | Send message |
| `Shift+Enter` | Newline in input |

### Control

| Key | Action |
|-----|--------|
| `Shift+Tab` | Cycle permission mode (manual → plan → auto → manual) |
| `Ctrl+R` | Open sessions browser |
| `Esc` | Close peek, then cancel current turn |

### Approval

When the agent requests permission to run a write/shell tool:

| Key | Action |
|-----|--------|
| `y` | Approve this one time |
| `a` | Always approve this tool (for this session) |
| `n` | Deny |

---

## Slash commands

Type `/` in the input to see available commands.

### Permission and mode

| Command | Purpose |
|---------|---------|
| `/mode` | Show current permission mode |
| `/plan` | Switch to plan mode (explore + shell freely; no code edits or repo commits) |
| `/manual` | Switch to manual mode (approval required for writes) |
| `/auto` | Switch to auto mode (auto-approve all) |

See [Permission modes](#permission-modes) below for exactly what each mode allows.

### Session and state

| Command | Purpose |
|---------|---------|
| `/sessions` | Open sessions browser (same as Ctrl+R) |
| `/resume` | Resume a session |
| `/todos` | Show current todos |
| `/clear` | Clear current screen |
| `/new` | Start a new session |

### Knowledge stack

| Command | Purpose |
|---------|---------|
| `/graphify` | Query the code knowledge graph |
| `/plur` | Search shared engram memory |
| `/ruflo` | Search vector memory |
| `/skills` | List available skills |
| `/ecosystem` | Show ecosystem status |
| `/memory` | Show session memory |

### Quick memory

Type `#` followed by a note to save it directly to `~/.nur/memory.md` without starting a turn:

```text
# use cargo-nextest for test runner, not cargo test
```

The note is appended to your persistent memory file and recalled automatically in future sessions.

### Model and context

| Command | Purpose |
|---------|---------|
| `/model` | Show and switch models. Run bare to open a picker that fetches your provider's live model list (`/models`) — filter, arrow, and ↵ to switch, or type any id. `/model <id>` switches directly (e.g. `/model muse-spark-1.1`) |
| `/plugins` | Marketplace picker (same UX as provider/`/login` picker): filter, ↑↓/wheel, ↵ to install or enable/disable. Skills land in `~/.nur/plugins/<id>` and mirror **in full** (incl. `references/`) to `~/.nur/skills`. CLI: `nur plugins list\|install\|enable\|disable\|uninstall`. Natural-language phrases (e.g. *think like fable*) auto-activate skills for the turn — status chip `skill · activated from your wording…` |
| `/effort` | Change reasoning effort |
| `/compact` | Manually compact context (thins old tool bodies; keeps recent turns; writes `.precompact.bak`) |
| `/usage` | Show token usage and cost (`/cost`) — includes budget caps when set |
| `/budget` | Session spend ceiling: `/budget [cost <usd>\|tokens <n>\|clear\|save]` |
| `/poor` | Toggle cost-saver prompt (skip PLUR inject / skills catalog / long memory; **tools stay full**) |
| `/context` | Context-window utilization (bar + tokens) |
| `/status` | Session snapshot: model · mode · cwd · tokens · cost |

### Project and shell

| Command | Purpose |
|---------|---------|
| `/cd <path>` | Change the working directory tools are sandboxed to (`~` and relative paths OK) |
| `/pwd` | Print the current working directory |
| `/init` | Initialise project instructions (`NUR.md`) |
| `/config` | Show config + data paths |
| `/permissions` | Show or reload allow/deny/ask rules (`permissions.toml`) |
| `/hooks` | Local tool hook status (`hooks.toml`) |
| `/doctor` | Inline health check: version · auth · ecosystem · shell · budgets |
| `/help` | Show keys + commands reference |
| `/login` | Multi-provider sign-in (see below) |
| `/logout` | Clear stored API key |
| `/goal` | Standing session goal (see below) |
| `/btw` | One-off note for the next message only |
| `/codesearch` `/cs` | Fast workspace ripgrep |
| `/mc` `/mcp` | MCP servers via the Executor gateway |
| `/feedback` | File a GitHub issue (`gh` or browser) |
| `/tips` | Mouse + keyboard interaction tips (lean banner counterpart) |
| `/bug` | Open GitHub issues page (report a bug) |
| `/exit` | Quit NurCLI |

### Multi-provider `/login`

```text
/login
```

Scrollable, **type-to-filter** catalog of 60+ providers → masked key → writes
`provider` / `base_url` / `model` to config and hot-swaps the HTTP client.
Full detail: [Authentication](authentication.md).

### Session goal & side notes

| Command | Behaviour |
|---------|-----------|
| `/goal <text>` | Standing goal for this session. Prepended as context on **every** turn (not shown as a user bubble). |
| `/goal` | Show the current goal |
| `/goal clear` | Drop the goal (`none` / `off` also work) |
| `/btw <note>` | Queues a one-off note that rides along with your **next** message only (stackable) |

### Code search

```text
/codesearch <regex or text>
/cs foo::bar
```

Runs the workspace `grep` tool immediately and prints matches in the
transcript (no full agent turn).

### MCP (`/mc`)

```text
/mc                 # list sources (default)
/mc sources         # same
/mc status
/mc search <query>
```

Uses the **Executor** gateway (`executor` tool). If MCP is missing:

```bash
nur ecosystem ensure
```

### Feedback

```text
/feedback <what happened / what you'd like>
```

Creates a GitHub issue on `nuroctane/nur-cli` via `gh` when available;
otherwise opens a prefilled new-issue page in the browser. Includes CLI
version, OS, and model in the body footer.

### Cost control (quick)

```text
/budget cost 5          # hard-stop this session around $5
/budget save            # persist ceilings to config.toml
/poor                   # leaner system prompt (tools unchanged)
```

Oversized tool results automatically spill under `~/.nur/tool-results/` with a short preview for the model — see [Configuration](configuration.md).

---

## Permission modes

Cycle with **Shift+Tab** (`manual → plan → auto`); switch directly with `/manual`, `/plan`, `/auto`. The change applies immediately, even mid-turn.

| Mode | What runs freely | What needs approval / is blocked |
|------|------------------|----------------------------------|
| **manual** | Reads (`read_file`, `grep`, `look`, `git_status`, …) | Writes, `bash`, `extract_frames` → prompt `y` / `a` / `n` |
| **plan** | Reads **and shell** for reading, parsing, tests, and scratch/media compute (`ffmpeg`, `extract_frames`, copying a clip, analysis scripts) | **Blocked:** code authoring (`write_file` / `edit_file` / `multi_edit` / `apply_patch`) and repo/VCS mutation — `git commit`/`push`/`add`/`reset`/… , `gh pr create`, dependency installs |
| **auto** | Everything (no prompts) | — |

Plan mode is for exploring and understanding a codebase without changing it: run whatever analysis you like, but no edits land and nothing is committed until you switch to manual or auto.

---

## Visual design

### Colour system

Tool cards are colour-coded by family:

| Family | Hue | Tools |
|--------|-----|-------|
| read | sky | `read_file` `list_dir` `grep` `glob` |
| edit | violet | `write_file` `edit_file` `multi_edit` `apply_patch` |
| shell | amber | `bash` |
| vision | pink | `look` `extract_frames` |
| web | teal | `web_fetch` `web_search` |
| git | cyan | `git_status` `git_diff` |
| knowledge | indigo / orange | `graphify` `plur` `ruflo` `skill` `memory` |

### Thought cards

The model's reasoning is displayed in **violet thought cards** that are collapsed by default. Click to expand.

### Duration chips

Each tool call shows a duration chip (e.g. `1.2s`) so you can see where time is spent.

### Approval mini-diff

When a write tool requests approval, the TUI shows a compact diff preview of what will change. The diff is line-numbered with `+`/`-` indicators so you can see exactly which lines will be added or removed before approving.

### Transcript diffs

Edit tools (`edit_file`, `write_file`, `multi_edit`, `apply_patch`) render a **green/red unified diff inline** in the transcript — added lines in green bands, removed in red, with a `+adds -dels` chip on the card header. The full diff is shown when the card is expanded and inside its peek box.

### Prompt menu — fork · revert · copy

**Right-click** or **double-click** any of your prompts (in the transcript or on the sticky header) to open a small menu — styled like every other dialogue:

| Action | What it does |
|--------|--------------|
| **Fork** | Branch into a **new session** seeded with the conversation up to that prompt. The original session is kept intact on disk; the prompt lands in your input to continue the fork. |
| **Revert** | **Rewind** the session to just before that prompt (transcript, messages, and the model's context are all truncated). The prompt returns to the input to edit and resend. |
| **Copy** | Copy the prompt text to the clipboard. |

No keyboard shortcuts — move the highlight with the wheel or `↑`/`↓`, choose with `Enter` or a click, dismiss with `Esc`, the ✕, or a click outside.

### Sessions browser

Open with `Ctrl+R` or `/sessions`. Browse recent sessions with a prompt-first picker — see the first user message of each session to find the one you want.

- Defaults to **all** workspaces (not only the current cwd). Toggle **here** / **all** with Tab or the scope chip.
- Scans both `~/.nur/sessions` and legacy `~/.muse/sessions`; when the same id exists twice, the **richer** copy wins.
- Lists show message counts, tokens, and **estimated cost** so high-spend sessions are easy to spot.
- Session saves write a sidecar **`.json.bak`** before overwrite.

### Sticky prompt

The PROMPT banner stays visible at the top as you scroll, so you always see the context of the conversation.

### Splash screen

The splash shows the active model title. The rest of the chrome is model-agnostic.

---

## ADE / Orca integration

NurCLI writes live status files for host panels:

| Path | Contents |
|------|----------|
| `~/.nur/status.json` | Live tokens, cost, model, state |
| `~/.nur/usage.jsonl` | Per-request log |
| `~/.nur/ade.json` | Discovery manifest |

```bash
nur install-hook           # install Orca agent hook
orca terminal create --command nur    # launch in Orca
```

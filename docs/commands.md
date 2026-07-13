# Commands

Full CLI reference for Meta CLI.

## Usage

```bash
meta [OPTIONS] [PROMPT]
meta <COMMAND> [ARGS]
```

## Global options

| Flag | Short | Description |
|------|-------|-------------|
| `--model <MODEL>` | `-m` | Meta Model API model id (default from config) |
| `--cwd <DIR>` | | Working directory |
| `--yes` | `-y` | Auto-approve tools (sets permission mode to auto) |
| `--mode <MODE>` | | Permission mode: `manual`, `plan`, or `auto` |
| `--effort <LEVEL>` | | Reasoning effort: `minimal`, `low`, `medium`, `high`, `xhigh` |
| `--max-turns <N>` | | Max agent turns per prompt |
| `--verbose` | `-v` | Verbose tool logging (headless mode) |
| `--continue` | `-c` | Continue the most recent session for this cwd |
| `--resume <ID>` | `-r` | Resume a specific session id (full UUID or unique prefix) |
| `--version` | | Print version |
| `--help` | `-h` | Print help |

## Examples

```bash
meta                                    # open interactive TUI
meta "fix the bug"                      # start with a prompt
meta "design from ref.mp4"             # vision: auto-attach media
meta -c                                 # continue last session
meta -r abc123                          # resume session abc123
meta --mode plan "explain this"         # plan: explore + shell, no edits/commits
meta --effort xhigh "deep analysis"     # maximum reasoning
meta --model muse-spark-1.1 "hello"     # explicit model
meta run "add tests" -y                 # headless + auto-approve
```

---

## Subcommands

### `meta run`

Run a single agent turn headlessly. Prints the final answer to stdout.

```bash
meta run <PROMPT...> [OPTIONS]
```

| Arg / Flag | Description |
|------------|-------------|
| `PROMPT` | Prompt text (required, multiple words joined) |
| `-y`, `--yes` | Auto-approve all tools |

**Example:**

```bash
meta run "write a hello world in Rust" -y
meta run "explain what this repo does" -v
```

---

### `meta auth`

Manage authentication against the Meta Model API.

```bash
meta auth <SUBCOMMAND>
```

#### `meta auth login`

Save API key to `~/.meta/auth.json`.

```bash
meta auth login [--key <KEY>]
```

| Flag | Description |
|------|-------------|
| `--key <KEY>` | API key (optional; prompts if omitted) |

#### `meta auth status`

Show auth status. Never prints the full key.

```bash
meta auth status
```

#### `meta auth logout`

Remove saved key from `~/.meta/auth.json` and legacy `~/.muse/`.

```bash
meta auth logout
```

---

### `meta sessions`

List recent sessions.

```bash
meta sessions [--limit <N>]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--limit` | `20` | Max rows to display |

---

### `meta usage`

Show last known token usage and cost. Displays paths to status and usage log files.

```bash
meta usage
```

---

### `meta doctor`

Diagnose install, auth, config, and ecosystem readiness.

```bash
meta doctor
```

Checks:

- Binary path and version
- Config file (model, effort, max_turns)
- Auth status (key present, last 4 chars)
- Data home, status, usage, sessions paths
- Ecosystem readiness (Graphify, PLUR, Ruflo)
- Shell backend (Bash / PowerShell)
- Optional tools on PATH (rg, git, node, npm, uv, ffmpeg)
- Vision support (look, extract_frames)
- Binary SHA-256 integrity

See [Troubleshooting](troubleshooting.md) for interpreting results.

---

### `meta ecosystem`

Manage the Graphify / PLUR / Ruflo ecosystem (auto-provisioned on TUI open).

```bash
meta ecosystem <SUBCOMMAND>
```

#### `meta ecosystem ensure`

Install or repair Graphify, PLUR, Ruflo, and skills. Also runs automatically on TUI open.

```bash
meta ecosystem ensure [--force]
```

| Flag | Description |
|------|-------------|
| `--force`, `-f` | Force re-install even if marker is fresh |

#### `meta ecosystem status`

Show ecosystem readiness.

```bash
meta ecosystem status
```

---

### `meta install-hook`

Install the Orca agent hook for usage/status reporting.

```bash
meta install-hook
```

---

## Project instruction files

Meta CLI loads project-level instructions from your working directory at session start:

| File | Purpose |
|------|---------|
| `META.md` | Primary project instructions |
| `AGENTS.md` | Agent conventions |
| `CLAUDE.md` | Legacy (still loaded) |
| `MUSE.md` | Legacy (still loaded) |

---

## Safe workspace

When launched from a drive root (`C:\` or `/`), Meta CLI auto-selects a safe workspace by checking (in order):

1. Git repository root
2. Last session's working directory
3. `~/Laboratory` (or fallback)

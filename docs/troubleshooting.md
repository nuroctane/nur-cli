# Troubleshooting

## `nur doctor`

The built-in health check for install, auth, config, and ecosystem:

```bash
nur doctor
```

### What it checks

| Check | Shows |
|-------|-------|
| `binary` | Path to the `nur` binary |
| `config` | Model, effort, max_turns, **budget** (`$/tok` caps), config path |
| `auth` | Whether a key is set (last 4 chars only) |
| `home` | Data home directory |
| `status` | Path to `status.json` |
| `usage` | Path to `usage.jsonl` |
| `sessions` | Path to sessions directory |
| `ecosystem` | Graphify, PLUR, Ruflo (and related packs) readiness |
| `shell` | Bash / PowerShell backend |
| `rg`, `git`, `node`, `npm`, `uv`, `ffmpeg` | Whether on PATH |
| `vision` | look + extract_frames support |
| `sha256` | Binary integrity check |

### Common doctor output

**All green:**

```text
nur doctor · v0.13.2

binary  C:\Users\you\.local\bin\nur.exe
config  model=muse-spark-1.1 effort=high max_turns=40 budget=∞$/∞tok  (C:\Users\you\.nur\config.toml)
auth    key set (…abcd)
home    C:\Users\you\.nur
status  C:\Users\you\.nur\status.json
usage   C:\Users\you\.nur\usage.jsonl
sessions C:\Users\you\.nur\sessions

ecosystem
  graphify  ✓
  plur      ✓
  ruflo     ✓

shell   Git Bash
rg      C:\Program Files\Git\usr\bin\rg.exe
git     C:\Program Files\Git\bin\git.exe
node    C:\Program Files\nodejs\node.exe
npm     C:\Program Files\nodejs\npm.cmd
uv      C:\Users\you\.local\bin\uv.exe
ffmpeg  C:\Program Files\ffmpeg\bin\ffmpeg.exe
vision  look · extract_frames (input_image / input_video)

sha256  abc123...  (matches install record)

doctor complete
```

---

## How do I update NurCLI?

```bash
nur update
```

That pulls your Laboratory checkout (`~/laboratory/nur-cli` or `~/Laboratory/nur-cli`), runs `cargo build --release`, reinstalls `~/.local/bin/nur`, and re-runs ecosystem / browser setup.

| If… | Then… |
|-----|--------|
| No source checkout | `nur update` falls back to `nur install` (repair from the running binary) |
| You use the Windows EXE only | Re-download `nur-windows-x86_64.exe` from [Releases](https://github.com/nuroctane/nur-cli/releases/latest) and double‑click, **or** `nur update` if you later have a clone |
| You want a full network reinstall | Re-run the [one-liner](setup.md) |
| You only want stack packs refreshed | `nur ecosystem ensure --force` (does not rebuild the CLI) |

Confirm:

```bash
nur --version
nur doctor
```

More: **[Setup → Update](setup.md#update-keep-nurcli-current)** · **[Commands → nur update](commands.md#nur-update)**.

---

## Common issues

### `command not found: nur`

The `nur` binary is not on your PATH.

**Fix:**

1. Check where it was installed: `ls ~/.local/bin/nur`
2. Add `~/.local/bin` to your PATH:
    ```bash
    # Bash / Zsh
    export PATH="$HOME/.local/bin:$PATH"

    # PowerShell
    $env:Path += ";$env:USERPROFILE\.local\bin"
    ```
3. Restart your terminal

### `auth    not set`

No API key found.

**Fix:**

```bash
nur auth login
# or
export NUR_API_KEY="your-key-here"
# Meta Model API / older installs also accept META_API_KEY / MODEL_API_KEY
```

### Missing session in `/sessions`

Sessions are never auto-deleted. If a chat “vanished”:

1. Toggle the sessions picker scope to **all** (not only this cwd) — Tab or the scope chip.
2. CLI: `nur sessions --limit 50` and look at the **COST** column for high-spend chats.
3. Resume by id: `nur -r <prefix>` (first 8 chars of the UUID are enough when unique).
4. Check both `~/.nur/sessions/` and legacy `~/.muse/sessions/`. Sidecar `*.json.bak` may hold the previous save.

### Session budget stopped the agent

```text
session cost $X ≥ budget $Y
```

**Fix:**

```text
/budget cost 10
/budget clear
/budget save
```

Or edit `max_session_cost_usd` / `max_session_tokens` in `~/.nur/config.toml`.

### Garbled text in the TUI on launch

Logs go to `~/.nur/nur.log` (not stderr). If you still see noise, check that you're on **v0.13.2+** and no wrapper is redirecting `RUST_LOG` to the console at `warn` for syntect.

### Ecosystem components missing

```text
ecosystem
  graphify  ✗
  plur      ✗
  ruflo     ✗
```

**Fix:**

1. Install Node.js 20+ and uv:
    ```bash
    # Windows
    winget install OpenJS.NodeJS.LTS
    winget install astral-sh.uv

    # macOS
    brew install node uv

    # Linux
    sudo apt install nodejs npm
    pip install uv
    ```
2. Re-run any one-shot installer:
    ```bash
    nur install
    # or: nur ecosystem ensure --force
    # or re-run the one-liner / double-click a fresh release EXE
    ```

### `ffmpeg not on PATH`

`extract_frames` requires ffmpeg.

**Fix:** Install ffmpeg (see [Vision](vision.md#requirements)).

### `sha256 mismatch`

Binary may be corrupted or from a different source.

**Fix:** Re-run a one-shot install:

```bash
# already on PATH
nur install

# Windows one-liner
irm https://raw.githubusercontent.com/nuroctane/nur-cli/main/install.ps1 | iex

# macOS / Linux one-liner
curl -fsSL https://raw.githubusercontent.com/nuroctane/nur-cli/main/install.sh | bash

# or download nur-windows-x86_64.exe from Releases and double-click
```

### API errors / rate limits

If you see API errors:

1. Check your key: `nur auth status`
2. Check the model: `cat ~/.nur/config.toml`
3. Verify the API is up: [dev.meta.ai](https://dev.meta.ai/)

### Session not resuming

```bash
nur sessions              # list sessions
nur -r <session-id>        # resume by id
nur -c                     # continue most recent for this cwd
```

### `config` validation errors

```text
config  invalid reasoning_effort 'super' — use minimal|low|medium|high|xhigh
```

**Fix:** Edit `~/.nur/config.toml` and set a valid effort level.

---

## Legacy migration

If you upgraded from a pre-0.5.14 build (using `~/.muse/`), NurCLI automatically gap-fills missing files into `~/.nur/`. Existing files are never overwritten.

To manually migrate:

```bash
# Files are copied automatically on first run.
# To force a clean start:
nur auth logout     # clears both ~/.nur and ~/.muse
nur auth login      # re-authenticate
```

---

## Getting more help

- Run `nur doctor` for a full diagnostic
- Check the [GitHub issues](https://github.com/nuroctane/nur-cli/issues)
- Open a new issue with your `nur doctor` output

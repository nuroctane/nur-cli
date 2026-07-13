# Setup

System requirements, **every install path**, what lands on your PC, updates, and uninstallation.

!!! tip "It's one line"
    **<span class="install-hot">Windows:</span>** `irm https://raw.githubusercontent.com/nuroctane/meta-cli/main/install.ps1 | iex`  
    **<span class="install-hot">macOS / Linux:</span>** `curl -fsSL https://raw.githubusercontent.com/nuroctane/meta-cli/main/install.sh | bash`  
    Then: `meta auth login` → `meta`. Full detail below.

---

## System requirements

| Requirement | Details |
|-------------|---------|
| **Operating system** | Windows 10+ · macOS 13+ · Ubuntu 20.04+ · Debian 10+ · Alpine 3.19+ |
| **Hardware** | 4 GB+ RAM, x64 or ARM64 processor |
| **Network** | Internet (Meta Model API + first install downloads) |
| **Shell** | PowerShell, CMD, Bash, or Zsh |
| **Git** | Required for the one-liner / clone paths |

---

## Install methods

### 1. One-liner (recommended)

Does **everything**: Rust if needed, prereqs, build, PATH, ecosystem packs, browser stage, optional Orca hook + auth.

=== "<span class='install-hot'>Windows (PowerShell)</span>"

    ```powershell
    irm https://raw.githubusercontent.com/nuroctane/meta-cli/main/install.ps1 | iex
    ```

=== "<span class='install-hot'>macOS / Linux</span>"

    ```bash
    curl -fsSL https://raw.githubusercontent.com/nuroctane/meta-cli/main/install.sh | bash
    ```

### 2. <span class="install-hot">Prebuilt Windows EXE</span> (no local compile)

**Same job as the one-liner** — download, run, full stack. The EXE *is* the installer.

1. Open [**Releases → latest**](https://github.com/nuroctane/meta-cli/releases/latest)
2. Download **`meta-windows-x86_64.exe`**
3. **Double‑click it** (or `.\meta-windows-x86_64.exe`)

What it does **before** any TUI (console progress):

| Step | Action |
|------|--------|
| Binary | Copies itself → `%USERPROFILE%\.local\bin\meta.exe` (+ `muse.exe` alias) |
| PATH | Adds `~\.local\bin` to User PATH |
| Prereqs | Best-effort: node · bun · uv · rg · ffmpeg |
| Ecosystem | `ecosystem ensure --force` (graphify · plur · ruflo · omp · browser · skills) |
| Browser | Stages Chromium extension for your default browser |
| Hook | Orca hook if present |
| Auth | Saves `META_API_KEY` / `MODEL_API_KEY` if set in the environment |
| Launch | Opens the installed `meta` TUI |

Re-download + re-run the release EXE to upgrade. Force again anytime: `meta install`.

!!! tip "Dev / skip"
    `META_SKIP_BOOTSTRAP=1` skips auto-install (e.g. local cargo builds).

### 3. From a local clone

```bash
cd meta-cli
./install.ps1    # Windows PowerShell: .\install.ps1
# ./install.sh   # macOS / Linux
```

Same steps as the remote one-liner, using the checkout you already have.

### 4. Manual `cargo` build

```bash
git clone https://github.com/nuroctane/meta-cli.git && cd meta-cli
cargo build --release
./target/release/meta install   # Windows: .\target\release\meta.exe install
meta auth login
```

`meta install` is the same one-stop path the release EXE runs (binary → PATH → ecosystem → browser).

### Verify

```bash
meta --version
meta doctor
```

---

## What gets installed (A → Z)

Everything is **on your machine only**. Secrets never go into the git checkout.

### A–G · Runtimes & build tools (one-liner installs if missing)

| Piece | Typical location | Used for |
|-------|------------------|----------|
| **Rust / cargo** (rustup) | `~/.cargo/` | Compiling Meta CLI |
| **Git** | system | Clone / update source |
| **Node.js 20+** | system / winget / brew / apt | PLUR, Ruflo, Executor, skills, browser CLI, AKM |
| **Bun** | `~/.bun/` | **omp** (Oh My Pi) |
| **uv** | `~/.local/bin` | **Graphify** |
| **ripgrep** | system | Fast `grep` / `glob` |
| **ffmpeg** | system | `extract_frames` / design-from-video |

### Meta CLI binary

| Piece | Path |
|-------|------|
| **`meta`** | `~/.local/bin/meta` · Windows `meta.exe` |
| **`muse`** | Same binary, legacy alias |
| **Integrity** | `~/.local/bin/meta.sha256` |
| **Source tree** (one-liner) | `~/laboratory/meta-cli` (Windows: `%USERPROFILE%\laboratory\meta-cli`) |
| **PATH** | `~/.local/bin` added to User PATH (Windows) or a shell rc (Unix) |

### Data home — `~/.meta/`

Created on first auth / first run:

| Path | Purpose |
|------|---------|
| `auth.json` | API key |
| `config.toml` | Model, effort, budgets, compact, `poor_mode`, `ecosystem_auto_ensure`, … |
| `permissions.toml` | Optional allow/deny/ask rules |
| `hooks.toml` | Optional pre/post tool hooks |
| `meta.log` | Tracing (not drawn into the TUI) |
| `status.json` / `usage.jsonl` / `ade.json` | Live usage + host panels |
| `memory.md` / `history.jsonl` | Notes + prompt history |
| `sessions/` | Sessions + `.json.bak` / `.precompact.bak` |
| `tool-results/` | Spilled large tool outputs |
| `browser-extension/` | Staged Chromium extension for `browser` |
| `skills/` · `skill-packs/` · `ruflo/` | Skills + vector memory |

### Ecosystem (after `ecosystem ensure`)

External CLIs / packs (not inside the `meta` binary):

| Component | Role |
|-----------|------|
| Graphify | Code knowledge graph |
| PLUR | Shared engram memory |
| Ruflo | Vector memory / swarm helpers |
| Executor | MCP / OpenAPI catalog |
| omp | Headless coding-agent backend |
| agent-browser-cli | Real-browser bridge |
| Skills + AKM | Progressive skill packs |
| Browser setup | Stages extension; one manual “Load unpacked” in Chromium |

### Optional

| Piece | Notes |
|-------|--------|
| Orca hook | `meta install-hook` when Orca is present |
| Env-based auth | `META_API_KEY` / `MODEL_API_KEY` → saved under `~/.meta/auth.json` only |

---

## Authenticate

Get a key from [dev.meta.ai](https://dev.meta.ai/) → API keys.

```bash
meta auth login
```

Or inside the TUI: `/login` (masked) · `/logout`. No key on launch → login modal opens automatically.

See [Authentication](authentication.md).

---

## Update

Re-run the install one-liner, or:

```bash
# from a clone
git pull && ./install.ps1   # or install.sh
```

Prebuilt users: download a newer `meta-windows-x86_64.exe` from Releases and replace `~/.local/bin/meta.exe`.

```bash
meta doctor   # confirm version + sha256
```

---

## Uninstall

### Binary

=== "Windows"

    ```powershell
    Remove-Item -Force "$env:USERPROFILE\.local\bin\meta.exe","$env:USERPROFILE\.local\bin\muse.exe","$env:USERPROFILE\.local\bin\meta.sha256" -ErrorAction SilentlyContinue
    ```

=== "macOS / Linux"

    ```bash
    rm -f ~/.local/bin/meta ~/.local/bin/muse ~/.local/bin/meta.sha256
    ```

### Config, sessions, usage (destructive)

=== "Windows"

    ```powershell
    Remove-Item -Recurse -Force "$env:USERPROFILE\.meta"
    ```

=== "macOS / Linux"

    ```bash
    rm -rf ~/.meta
    ```

### Legacy home

Older builds used `~/.muse/` — remove the same way if you no longer need it.

### Build cache / source (optional)

- Source checkout: `~/laboratory/meta-cli` (or your clone path)
- Rust target artifacts: inside that repo’s `target/`
- rustup / node / bun / uv / ffmpeg: uninstall with your OS package manager if you installed them only for Meta and don’t need them elsewhere

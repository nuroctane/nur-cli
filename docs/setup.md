# Setup

System requirements, **every install path**, what lands on your PC, updates, and uninstallation.

!!! tip "It's one line"
    **<span class="install-hot">Windows:</span>** `irm https://raw.githubusercontent.com/nuroctane/nur-cli/main/install.ps1 | iex`  
    **<span class="install-hot">macOS / Linux:</span>** `curl -fsSL https://raw.githubusercontent.com/nuroctane/nur-cli/main/install.sh | bash`  
    Then: `nur auth login` → `nur`. Full detail below.

---

## System requirements

| Requirement | Details |
|-------------|---------|
| **Operating system** | Windows 10+ · macOS 13+ · Ubuntu 20.04+ · Debian 10+ · Alpine 3.19+ |
| **Hardware** | 4 GB+ RAM, x64 or ARM64 processor |
| **Network** | Internet (provider APIs + first install downloads) |
| **Shell** | PowerShell, CMD, Bash, or Zsh |
| **Git** | Required for the one-liner / clone paths |

---

## Install methods

### 1. One-liner (recommended)

Does **everything**: Rust if needed, prereqs, build, PATH, ecosystem packs, browser stage, optional Orca hook + auth.

=== "<span class='install-hot'>Windows (PowerShell)</span>"

    ```powershell
    irm https://raw.githubusercontent.com/nuroctane/nur-cli/main/install.ps1 | iex
    ```

=== "<span class='install-hot'>macOS / Linux</span>"

    ```bash
    curl -fsSL https://raw.githubusercontent.com/nuroctane/nur-cli/main/install.sh | bash
    ```

### 2. <span class="install-hot">Prebuilt Windows EXE</span> (no local compile)

**Same job as the one-liner:** download, run, full stack. The EXE *is* the installer.

1. Open [**Releases → latest**](https://github.com/nuroctane/nur-cli/releases/latest)
2. Download **`nur-windows-x86_64.exe`**
3. **Double-click it** (or `.\nur-windows-x86_64.exe`)

What it does **before** any TUI (console progress):

| Step | Action |
|------|--------|
| Binary | Copies itself → `%USERPROFILE%\.local\bin\nur.exe` |
| PATH | Adds `~\.local\bin` to User PATH |
| Prereqs | Best-effort: node · bun · uv · rg · ffmpeg |
| Ecosystem | `ecosystem ensure --force` (graphify · plur · ruflo · omp · browser · skills) |
| Browser | Stages Chromium extension for your default browser |
| Hook | Orca hook if present |
| Auth | Saves `NUR_API_KEY` (or legacy `META_API_KEY` / `MODEL_API_KEY`) if set in the environment |
| Launch | Opens the installed `nur` TUI |

Re-download + re-run the release EXE to upgrade. Force again anytime: `nur install`.

!!! tip "When auto-install runs"
    **Release EXE** (`nur-windows-*.exe`) and **first** run with no `~\.local\bin\nur` → full one-stop install.  
    Already-installed `nur` on PATH opens the TUI immediately (no reinstall).  
    Force again: `nur install`. Dev skip: `NUR_SKIP_BOOTSTRAP=1`.

### 3. From a local clone

```bash
cd nur-cli
./install.ps1    # Windows PowerShell: .\install.ps1
# ./install.sh   # macOS / Linux
```

Same steps as the remote one-liner, using the checkout you already have.

### 4. Manual `cargo` build

```bash
git clone https://github.com/nuroctane/nur-cli.git && cd nur-cli
cargo build --release
./target/release/nur install   # Windows: .\target\release\nur.exe install
nur auth login
```

`nur install` is the same one-stop path the release EXE runs (binary → PATH → ecosystem → browser).

### Update (keep NurCLI current)

**Default — one command:**

```bash
nur update
```

That is the supported upgrade path after any install. Same spirit as the one-liner: refresh source if present, rebuild, reinstall binary + ecosystem + browser stage.

| What `nur update` does | Detail |
|-------------------------|--------|
| 1. Find source | `~/laboratory/nur-cli` or `~/Laboratory/nur-cli` (Windows `%USERPROFILE%\…`) |
| 2. Pull | `git pull --ff-only origin main` when a checkout exists |
| 3. Build | `cargo build --release` in that tree |
| 4. Install binary | Copy → `~/.local/bin/nur` |
| 5. Stack | `ecosystem ensure --force` · `browser setup` · Orca hook |
| No source tree? | Falls back to **`nur install`** (self-repair from the running binary) |

Then verify:

```bash
nur --version
nur doctor
```

!!! tip "Remember this"
    After install, the CLI prints: **`Update: nur update`**.  
    Bookmark it. Re-run anytime you want the latest main + stack repair.

#### Other upgrade options

| Method | When to use |
|--------|-------------|
| **`nur update`** | **Preferred** — always try this first |
| **Re-run the one-liner** | Same as first install; rebuilds from GitHub main |
| **Re-download + double‑click** `nur-windows-x86_64.exe` | Windows prebuilt path (no local compile) |
| **`nur install`** | Re-copy *this* binary + full stack (no `git pull` / rebuild) |
| **Manual** | `cd` checkout → `git pull` → `cargo build --release` → `.\target\release\nur.exe install` |

=== "Windows (PowerShell)"

    ```powershell
    nur update
    # or reinstall from network:
    irm https://raw.githubusercontent.com/nuroctane/nur-cli/main/install.ps1 | iex
    # or re-download EXE from Releases and double-click
    ```

=== "macOS / Linux"

    ```bash
    nur update
    # or:
    curl -fsSL https://raw.githubusercontent.com/nuroctane/nur-cli/main/install.sh | bash
    ```

Disable automatic background repair (not the same as `nur update`): set `DISABLE_AUTOUPDATER=1` / `DISABLE_UPDATES=1` or `ecosystem_auto_ensure = false` in config — see [Configuration](configuration.md).

### Verify

```bash
nur --version
nur doctor
```

---

## What the one-liner and EXE install on your PC (A → Z)

Everything is **on your machine only**. Secrets never go into the git checkout. Same inventory for the PowerShell/bash one-liner **and** the prebuilt Windows EXE.

### A–G · Runtimes & build tools (installed if missing)

| Piece | Typical location | Used for |
|-------|------------------|----------|
| **Rust / cargo** (rustup) | `~/.cargo/` | Compiling NurCLI (**one-liner / cargo only** — not the release EXE) |
| **Git** | system | Clone / update source (**one-liner / clone only**) |
| **Node.js 20+** | system / winget / brew / apt | PLUR, Ruflo, Executor, skills, browser CLI, AKM |
| **Bun** | `~/.bun/` | **omp** (Oh My Pi) |
| **uv** | `~/.local/bin` | **Graphify** |
| **ripgrep** | system | Fast `grep` / `glob` |
| **ffmpeg** | system | `extract_frames` / design-from-video |

### NurCLI binary

| Piece | Path |
|-------|------|
| **`nur`** | `~/.local/bin/nur` · Windows `nur.exe` |
| **`muse`** | Same binary, legacy alias |
| **Integrity** | `~/.local/bin/nur.sha256` |
| **Source tree** (one-liner) | `~/laboratory/nur-cli` (Windows: `%USERPROFILE%\laboratory\nur-cli`) |
| **PATH** | `~/.local/bin` added to User PATH (Windows) or a shell rc (Unix) |

### Data home: `~/.nur/`

Created on first auth / first run:

| Path | Purpose |
|------|---------|
| `auth.json` | API key |
| `config.toml` | Model, effort, budgets, compact, `poor_mode`, `ecosystem_auto_ensure`, … |
| `bootstrap.json` | One-stop install marker (`nur install` / release EXE) |
| `ecosystem.json` | Ecosystem ensure marker / component snapshot |
| `permissions.toml` | Optional allow/deny/ask rules |
| `hooks.toml` | Optional pre/post tool hooks |
| `nur.log` | Tracing (not drawn into the TUI) |
| `status.json` / `usage.jsonl` / `ade.json` | Live usage + host panels |
| `memory.md` / `history.jsonl` | Notes + prompt history |
| `plugins/` | Marketplace clones + `registry.json` (`/plugins`) |
| `sessions/` | Sessions + `.json.bak` / `.precompact.bak` |
| `tool-results/` | Spilled large tool outputs |
| `browser-extension/` | Staged Chromium extension for `browser` |
| `skills/` · `skill-packs/` · `ruflo/` | Skills + vector memory |

### Ecosystem (installed during one-liner / EXE / `nur install`)

External CLIs / packs (not inside the `nur` binary):

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
| Orca hook | `nur install-hook` when Orca is present |
| Env-based auth | `NUR_API_KEY` (legacy `META_API_KEY` / `MODEL_API_KEY`) → saved under `~/.nur/auth.json` only |

---

## Authenticate

Get a key from [dev.meta.ai](https://dev.meta.ai/) → API keys.

```bash
nur auth login
```

Or inside the TUI: **`/login`** (provider picker + masked key) · **`/logout`**.
No key on launch → login modal opens automatically. Non-Meta providers (OpenRouter,
Ollama, …) are selected through TUI `/login` so endpoint and model switch with the key.

See [Authentication](authentication.md).

---

## Update

Any of:

```bash
# one-liner again
irm https://raw.githubusercontent.com/nuroctane/nur-cli/main/install.ps1 | iex   # Windows
# curl -fsSL …/install.sh | bash                                                 # macOS / Linux

# from a clone
git pull && ./install.ps1   # or install.sh

# already have nur on PATH
nur install

# prebuilt: download newer nur-windows-x86_64.exe from Releases and double-click
# (self-installs over ~/.local/bin/nur.exe — no hand PATH surgery)
```

```bash
nur doctor   # confirm version + sha256
```

---

## Uninstall

### Binary

=== "Windows"

    ```powershell
    Remove-Item -Force "$env:USERPROFILE\.local\bin\nur.exe","$env:USERPROFILE\.local\bin\muse.exe","$env:USERPROFILE\.local\bin\nur.sha256" -ErrorAction SilentlyContinue
    ```

=== "macOS / Linux"

    ```bash
    rm -f ~/.local/bin/nur ~/.local/bin/muse ~/.local/bin/meta ~/.local/bin/nur.sha256
    ```

### Config, sessions, usage (destructive)

=== "Windows"

    ```powershell
    Remove-Item -Recurse -Force "$env:USERPROFILE\.nur"
    ```

=== "macOS / Linux"

    ```bash
    rm -rf ~/.nur
    ```

### Legacy home

Older builds used `~/.muse/` — remove the same way if you no longer need it.

### Build cache / source (optional)

- Source checkout: `~/laboratory/nur-cli` (or your clone path)
- Rust target artifacts: inside that repo’s `target/`
- rustup / node / bun / uv / ffmpeg: uninstall with your OS package manager if you installed them only for NurCLI and don’t need them elsewhere

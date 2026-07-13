# Quickstart

Your first Meta CLI session in 60 seconds.

## 1. Install

=== "<span class='install-hot'>Windows (PowerShell)</span>"

    ```powershell
    irm https://raw.githubusercontent.com/nuroctane/meta-cli/main/install.ps1 | iex
    ```

=== "<span class='install-hot'>macOS / Linux</span>"

    ```bash
    curl -fsSL https://raw.githubusercontent.com/nuroctane/meta-cli/main/install.sh | bash
    ```

That’s the full stack (binary + PATH + prereqs + ecosystem).

**<span class="install-hot">Windows without building:</span>** download `meta-windows-x86_64.exe` from [Releases](https://github.com/nuroctane/meta-cli/releases/latest) and double‑click — it runs the **same full install**, then opens Meta. Other paths: **[Setup](setup.md)**.

## 2. Authenticate

```bash
meta auth login
```

Paste your [Meta Model API key](https://dev.meta.ai/) when prompted. The key is stored locally in `~/.meta/auth.json` — never printed or echoed.

!!! tip "TUI shortcuts"
    Once in the TUI, you can also use `/login` to re-authenticate and `/logout` to clear the stored key.

## 3. Open the TUI

```bash
meta
```

This opens the interactive Meta-blue TUI in your current directory.

## 4. Start working

Type your request and press Enter:

```text
fix the bug in src/main.rs where the parser hangs on empty input
```

The agent will read files, run tools, and stream its response in real time.

---

## Common first commands

```bash
meta                              # interactive TUI
meta "fix the bug"                # start with a prompt
meta -c                           # continue last session in this directory
meta --mode plan "explain this"   # plan mode (read-only, no writes)
meta run "add tests" -y           # headless + auto-approve
```

---

## Permission modes

Meta CLI has three permission modes. **Shift+Tab** cycles between them in the TUI.

| Mode | Behavior |
|------|----------|
| **manual** (default) | Reads are free; writes, shell, and `extract_frames` require approval (`y` / `a` / `n`) |
| **plan** | Explore freely — reads, `look`, knowledge queries, and shell for read/parse/tests/scratch; blocks code writes + repo/VCS mutation |
| **auto** | Auto-approve all tools (`-y` or `--mode auto`) |

---

## What just happened?

When you installed and ran `meta`, it:

1. **Installed the full stack** (one-liner or EXE): binary · PATH · prereqs · ecosystem · browser stage — **before** the TUI
2. Loaded your config from `~/.meta/config.toml`
3. Created (or resumed) a session under `~/.meta/sessions/`
4. Opened the streaming TUI with the Meta-blue theme
5. Connected to the Meta Model API with your key (or prompted `/login`)

Later opens only run light **background repair** if `ecosystem_auto_ensure` is on. All state lives under `~/.meta/`. No keys, sessions, or usage data are written to your project or git repo.

---

## Update later

Keep Meta current with one command:

```bash
meta update
```

Pulls latest main (when a Laboratory checkout exists), rebuilds, reinstalls the binary, and re-provisions the ecosystem. Alternatives (re-run one-liner, re-download Windows EXE): **[Setup → Update](setup.md#update-keep-meta-current)**.

---

## Next steps

- **[Setup](setup.md)** — Install paths, **how to update**, uninstall
- **[Commands](commands.md)** — Full CLI reference (`meta update`, `meta doctor`, …)
- **[TUI](tui.md)** — Keyboard shortcuts, slash commands
- **[Tools](tools.md)** — What the agent can do
- **[Vision](vision.md)** — Send images and video to the model
- **[Configuration](configuration.md)** — Customise model, effort, context window

# Quickstart

Your first NurCLI session in 60 seconds.

## 1. Install

=== "<span class='install-hot'>Windows (PowerShell)</span>"

    ```powershell
    irm https://raw.githubusercontent.com/nuroctane/nur-cli/main/install.ps1 | iex
    ```

=== "<span class='install-hot'>macOS / Linux</span>"

    ```bash
    curl -fsSL https://raw.githubusercontent.com/nuroctane/nur-cli/main/install.sh | bash
    ```

That’s the full stack (binary + PATH + prereqs + ecosystem).

**<span class="install-hot">Windows without building:</span>** download `nur-windows-x86_64.exe` from [Releases](https://github.com/nuroctane/nur-cli/releases/latest) and double-click. It runs the **same full install**, then opens NurCLI. Other paths: **[Setup](setup.md)**.

## 2. Authenticate

**Recommended:** open the TUI and run **`/login`**. Pick any of **45+ providers**
(OpenAI, Anthropic, Gemini, xAI, Groq, OpenRouter, local Ollama/LM Studio, Meta Model API, and more), then paste a key or use browser sign-in where available. That path also sets endpoint + default model.

CLI path (stores a key in `~/.nur/auth.json`; never printed):

```bash
nur auth login
```

!!! tip "Multi-provider is the point"
    `/login` is the real multi-provider path. Prefer it over assuming any single vendor.
    **`/logout`** clears the stored key. Details: [Authentication](authentication.md).

## 3. Open the TUI

```bash
nur
```

This opens the interactive Nur-gold TUI in your current directory.

## 4. Start working

Type your request and press Enter:

```text
fix the bug in src/main.rs where the parser hangs on empty input
```

The agent will read files, run tools, and stream its response in real time.

---

## Common first commands

```bash
nur                               # interactive TUI
nur "fix the bug"                 # start with a prompt
nur -c                            # continue last session in this directory
nur --mode plan "explain this"   # plan mode (read-only, no writes)
nur run "add tests" -y           # headless + auto-approve
```

---

## Permission modes

NurCLI has three permission modes. **Shift+Tab** cycles between them in the TUI.

| Mode | Behavior |
|------|----------|
| **manual** (default) | Reads are free; writes, shell, and `extract_frames` require approval (`y` / `a` / `n`) |
| **plan** | Explore freely: reads, `look`, knowledge queries, and shell for read/parse/tests/scratch; blocks code writes + repo/VCS mutation |
| **auto** | Auto-approve all tools (`-y` or `--mode auto`) |

---

## What just happened?

When you installed and ran `nur`, it:

1. **Installed the full stack** (one-liner or EXE): binary · PATH · prereqs · ecosystem · browser stage **before** the TUI
2. Loaded your config from `~/.nur/config.toml`
3. Created (or resumed) a session under `~/.nur/sessions/`
4. Opened the streaming gold TUI
5. Used your stored key / env key, or opened **`/login`** so you can pick a provider

Later opens only run light **background repair** if `ecosystem_auto_ensure` is on. All state lives under `~/.nur/`. No keys, sessions, or usage data are written to your project or git repo.

---

## Update later

Keep NurCLI current with one command:

```bash
nur update
```

Pulls latest main (when a Laboratory checkout exists), rebuilds, reinstalls the binary, and re-provisions the ecosystem. Alternatives (re-run one-liner, re-download Windows EXE): **[Setup → Update](setup.md#update-keep-nurcli-current)**.

---

## Next steps

- **[Setup](setup.md)** - Install paths, **how to update**, uninstall
- **[Commands](commands.md)** - Full CLI reference (`nur update`, `nur doctor`, …)
- **[TUI](tui.md)** - Keyboard shortcuts, slash commands
- **[Tools](tools.md)** - What the agent can do
- **[Vision](vision.md)** - Send images and video to the model
- **[Configuration](configuration.md)** - Customise model, effort, context window

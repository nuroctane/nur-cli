# NurCLI

**Fully loaded multi-provider terminal coding agent.** Not a thin wrapper.

Custom Rust harness, dense gold TUI, **native vision**, tools, knowledge stack, hardened sandbox. Pick any of **61 providers** with `/login`. Any model via `--model` / `/model`. Install marketplace plugins with `/plugins` (same picker UX as providers).

```text
nur           # gold interactive TUI
```

Repo: [nuroctane/nur-cli](https://github.com/nuroctane/nur-cli)

---

## Install: one line

=== "<span class='install-hot'>Windows (PowerShell)</span>"

    ```powershell
    irm https://raw.githubusercontent.com/nuroctane/nur-cli/main/install.ps1 | iex
    ```

=== "<span class='install-hot'>macOS / Linux</span>"

    ```bash
    curl -fsSL https://raw.githubusercontent.com/nuroctane/nur-cli/main/install.sh | bash
    ```

Then: `nur auth login` or open `nur` and use **`/login`** → pick a provider.

**<span class="install-hot">Windows EXE:</span>** download `nur-windows-x86_64.exe` from [Releases](https://github.com/nuroctane/nur-cli/releases/latest) → double-click → **full install** (PATH + ecosystem + browser) then TUI. Same stack as the one-liner, no compile. Details: **[Setup](setup.md)**.

### Update

```bash
nur update
```

That's the normal upgrade. Pulls / rebuilds when you have a Laboratory checkout, reinstalls `nur` + ecosystem. Or re-run the one-liner / re-download the EXE. Full detail: **[Setup → Update](setup.md#update-keep-nurcli-current)**.

---

## What you get

| Surface | Details |
|---------|---------|
| **TUI** | Streaming · duration chips · expandable thought/tool cards · click-to-peek (full write/edit content) · **queued follow-ups with send now** · **green/red transcript diffs** · **prompt menu (fork · edit · revert · copy)** · drag-select · always-on scrollbar · ↓ End · sticky prompt · sessions browser · approval mini-diff · lean banner · **`/login` (61 providers)** · **`/model` (live model list)** · **`/plugins` marketplace** · **`/goal` `/bro` `/adhd` `/scan` `/btw` `/codesearch` `/mc` `/feedback` `/tips` · **`/<skill>`**** · **`/budget` `/poor` `/permissions` `/hooks` `/cd` `/doctor`** |
| **Agent** | Manual / plan / auto · tool loop · subagents · todos · **smarter auto-compact** · **session $ / token budgets** · **tool-result spill** · Esc cancel · Shift+Tab mid-turn · prompt-cache keys · **Chat Completions adapter** for non-Responses providers |
| **Vision** | `look` (images / short video) · `extract_frames` (ffmpeg keyframes) · prompt auto-attach of media paths |
| **Tools** | read · edit · bash · web · **browser** · git · knowledge stack · agent (all first-class) |
| **Ecosystem** | Graphify · PLUR · Ruflo · Executor · **omp** · **browser** · AKM · **800+ skills** · **plugin marketplace** (`~/.nur/plugins`, incl. **Fable**) · **natural-language + slash skill activation** (*think like fable*, *site cli*, *TDD this*, `/fable-method`, `/adhd`, `/<skill>`, …). Full install at setup; later open = TTL repair (`ecosystem_auto_ensure`) |
| **Hardening** | Sandbox · bash denylist · SSRF blocks · atomic `~/.nur` IO · session **`.json.bak`** · **permissions.toml** · optional **hooks.toml** · API retries · install SHA-256 · `nur doctor` |
| **Host panels** | Live `status.json` / `usage.jsonl` · dual **`NUR_*` + `META_*`** env exports · Orca hook (`nur-hook.cmd` / `meta-hook.cmd`) |

**Current version: v0.13.4**

---

## Quick links

- **[Setup](setup.md)** - System requirements, install, **how to update**, uninstall
- **[Quickstart](quickstart.md)** - Your first session in 60 seconds
- **[Commands](commands.md)** - Full CLI reference
- **[TUI](tui.md)** - Keyboard shortcuts, slash commands, colour system
- **[Tools](tools.md)** - All native tools (read, edit, shell, web, git, knowledge, agent)
- **[Vision](vision.md)** - Images, video, `look`, `extract_frames`
- **[Ecosystem](ecosystem.md)** - Graphify, PLUR, Ruflo, skills, AKM, **plugin marketplace**
- **[Configuration](configuration.md)** - `config.toml`, environment variables, settings
- **[Security](security.md)** - Where secrets live, sandbox, reporting
- **[Troubleshooting](troubleshooting.md)** - `nur doctor`, common issues

---

## Built with

The terminal UI is powered by **[Ratatui](https://ratatui.rs/)** ([github](https://github.com/ratatui/ratatui)) and **[crossterm](https://github.com/crossterm-rs/crossterm)**. Huge thanks to the Ratatui project. Also built on [tokio](https://tokio.rs), [reqwest](https://github.com/seanmonstar/reqwest), [serde](https://serde.rs), and [clap](https://github.com/clap-rs/clap).

# NurCLI

**v0.38.3:** [Session inspector and compact activity](tui.md#session-inspector-and-compact-activity),
viewport-only transcript row preparation, and quieter idle rendering.
[Compact image attachments](vision.md) with on-demand previews and
per-message queue ownership, [memory retrieval cues](tools.md#connectome), and
four [startup workflow skills](ecosystem.md). Includes 41 themes, editable
question answers, and [Jev-first compaction](jev-performance-evaluation.md).

**Fully loaded multi-provider terminal coding agent.** Not a thin wrapper.

Custom Rust harness, dense gold TUI, **native vision**, 51 tools, a knowledge stack, and a hardened sandbox. Pick any of **65 providers** with `/login` (`/provider`), any model via `--model` / `/model`. **TypeSafe - Jev** typed judgments run *inside* the loop: which tool calls still earn their tokens, whether a result actually worked, what a fresh context can drop, which skill rules matter, which model is adequate - and they are **keyless on this machine** through three bundled local engines. Install marketplace plugins with `/plugins` (same picker UX as providers).

```text
nur           # gold interactive TUI
```

Repo: [nuroctane/nur-cli](https://github.com/nuroctane/nur-cli)

---

## Install: one line

=== "<span class='install-hot'>Every OS - npx</span>"

    ```bash
    npx nur-cli
    ```

Prebuilt binary, no Rust toolchain, no build. Keep it on PATH with `npm i -g nur-cli`.

=== "<span class='install-hot'>Windows (PowerShell)</span>"

    ```powershell
    irm https://raw.githubusercontent.com/nuroctane/nur-cli/main/install.ps1 | iex
    ```

=== "<span class='install-hot'>macOS / Linux</span>"

    ```bash
    curl -fsSL https://raw.githubusercontent.com/nuroctane/nur-cli/main/install.sh | bash
    ```

Then: `nur auth login` or open `nur` and use **`/login`** → pick a provider.

**<span class="install-hot">Windows EXE:</span>** download `nur-windows-x86_64.exe` from [Releases](https://github.com/nuroctane/nur-cli/releases/latest) → double-click → **full install** (PATH + ecosystem + browser) then TUI. Same stack as the npx path, no compile. Details: **[Setup](setup.md)**.

### Update

```bash
nur update
```

That's the normal upgrade. Pulls / rebuilds when you have a Laboratory checkout, reinstalls `nur` + ecosystem. Or re-run `npx nur-cli` / re-download the EXE. Full detail: **[Setup → Update](setup.md#update-keep-nurcli-current)**.

---

## What you get

| Surface | Details |
|---------|---------|
| **TUI** | Streaming · duration chips · expandable thought/tool cards · click-to-peek (full write/edit content) · **queued follow-ups with send now** · **green/red transcript diffs** · **prompt menu (fork · edit · revert · copy)** · drag-select · always-on scrollbar · ↓ End · sticky prompt · sessions browser · approval mini-diff · lean banner · **`/login` (65 providers)** · **`/model` (live model list)** · **`/plugins` marketplace** · **`/goal` `/bro` `/adhd` `/scan` `/btw` `/codesearch` `/mc` `/feedback` `/tips` · **`/<skill>`**** · **`/budget` `/poor` `/permissions` `/hooks` `/cd` `/doctor`** · **every path in the transcript is a link that opens** · **block-structured markdown** (headings, lists, quotes and rules as structure, not punctuation) · **LaTeX** when built with `image-peek` · **`/receipt` verifies the session hash chain** |
| **Agent** | Manual / plan / auto · tool loop · subagents · todos · **Jev-scored compaction** (survivors verbatim, no summary) · **pre-exec tool gate + post-exec result judge** · **session $ / token budgets** · **tool-result spill** · Esc cancel · Shift+Tab mid-turn · prompt-cache keys · **Chat Completions adapter** for non-Responses providers |
| **Vision** | `look` (images / short video) · `extract_frames` (ffmpeg keyframes) · prompt auto-attach of media paths |
| **Tools** | read · edit · bash · web · **browser** (incl. **element picking**) · **terminal-browser** (`/tb`) · git · judgments (`typesafe`) · memory (`optmem` `connectome` `mem`) · diagrams (`excalidraw` `tldraw` `penecho`) · policy (`dogwood`) · background (`bg`) and async (`admission` `goal` `proposal` `message` `question`) · docs (`anydoc`) · a persistent **Python REPL** · knowledge stack · agent (all first-class) |
| **Ecosystem** | Graphify · GraphJin · PLUR · Ruflo · Executor · **omp** · **browser** · **TypeSafe - Jev** (+ **three keyless local engines**) · **OptMem** · **Headroom** · **Connectome** · **dogwood** · **tldraw** · **egaki** · AKM · **1,000+ installed skills** (1,587 shipped here) · **plugin marketplace** (`~/.nur/plugins`, incl. **Fable**) · **natural-language + slash skill activation** (*think like fable*, *site cli*, *TDD this*, `/fable-method`, `/adhd`, `/<skill>`, …). Full install at setup; later open = TTL repair (`ecosystem_auto_ensure`) |
| **Hardening** | Sandbox · bash denylist · SSRF blocks · atomic `~/.nur` IO · session **`.json.bak`** · **permissions.toml** · optional **hooks.toml** · API retries · install SHA-256 · `nur doctor` |
| **Host panels** | Live `status.json` / `usage.jsonl` · **`NUR_*`** env exports · Orca hook (`nur-hook.cmd`) |

**Current version: v0.38.3**

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
- **[TypeSafe - Jev](typesafe.md)** - Typed judgments in the loop (tool gate, result judge, compaction, skills, routing)
- **[Local Jev engines](jev-local.md)** - The same contract with no key: openJev-verdict-2.0, Bespoke-Nimble-9B, Laya Core ML
- **[Troubleshooting](troubleshooting.md)** - `nur doctor`, common issues

---

## Built with

The terminal UI is powered by **[Ratatui](https://ratatui.rs/)** ([github](https://github.com/ratatui/ratatui)) and **[crossterm](https://github.com/crossterm-rs/crossterm)**. Huge thanks to the Ratatui project. Also built on [tokio](https://tokio.rs), [reqwest](https://github.com/seanmonstar/reqwest), [serde](https://serde.rs), and [clap](https://github.com/clap-rs/clap).

Judgments run through [TypeSafe System One](https://docs.typesafe.ai), extended on this machine by [openJev-verdict-2.0](https://github.com/Heman10x-NGU/openJev-verdict-2.0), [Bespoke-Nimble-9B](https://huggingface.co/bespokelabs/Bespoke-Nimble-9B), and [Laya Core ML](https://github.com/mizorewww/laya-coreml). Memory and context: [OptMem](https://github.com/VictorTaelin/OptMem), [Headroom](https://github.com/headroomlabs-ai/headroom), [Connectome](https://arxiv.org/abs/2606.24775). Code intelligence: [Graphify](https://graphify.dev). The full lineage ledger lives on the [CLI site](https://www.nuroctane.xyz/cli).

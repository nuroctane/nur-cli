# Meta CLI

**Fully loaded terminal coding agent** for [Meta Model API](https://dev.meta.ai/) — not a thin wrapper.

Custom Rust harness, dense Meta-blue TUI, **native vision**, tools, knowledge stack, hardened sandbox. Any model id via `--model` / `/model` / config.

!!! info "Unofficial"
    Not affiliated with Meta Platforms, Inc. · Community · [nuroctane/meta-cli](https://github.com/nuroctane/meta-cli)

```text
meta          # primary — Meta-blue interactive TUI
muse          # legacy alias (same binary)
```

---

## What you get

| Surface | Details |
|---------|---------|
| **TUI** | Streaming · duration chips · expandable thought/tool cards · click-to-peek · **green/red transcript diffs** · **prompt menu (fork · revert · copy)** · drag-select · always-on scrollbar · ↓ End · sticky prompt · sessions browser · approval mini-diff |
| **Agent** | Manual / plan / auto · tool loop · subagents · todos · auto-compact · Esc cancel · Shift+Tab mid-turn · prompt-cache keys |
| **Vision** | `look` (images / short video) · `extract_frames` (ffmpeg keyframes) · prompt auto-attach of media paths |
| **Tools** | read · edit · bash · web · git · knowledge stack · agent |
| **Ecosystem** | Graphify · PLUR · Ruflo · Executor · AKM · **800+ skills** — background provision |
| **Hardening** | Sandbox · bash denylist · SSRF blocks · atomic `~/.meta` IO · API retries · install SHA-256 · `meta doctor` |
| **Host panels** | Live `status.json` / `usage.jsonl` · Orca hook when present |

---

## Quick links

- **[Setup](setup.md)** — System requirements, install, update, uninstall
- **[Quickstart](quickstart.md)** — Your first session in 60 seconds
- **[Commands](commands.md)** — Full CLI reference
- **[TUI](tui.md)** — Keyboard shortcuts, slash commands, colour system
- **[Tools](tools.md)** — All native tools (read, edit, shell, web, git, knowledge, agent)
- **[Vision](vision.md)** — Images, video, `look`, `extract_frames`
- **[Ecosystem](ecosystem.md)** — Graphify, PLUR, Ruflo, skills, AKM
- **[Configuration](configuration.md)** — `config.toml`, environment variables, settings
- **[Security](security.md)** — Where secrets live, sandbox, reporting
- **[Troubleshooting](troubleshooting.md)** — `meta doctor`, common issues

---

## Built with

The terminal UI is powered by **[Ratatui](https://ratatui.rs/)** ([github](https://github.com/ratatui/ratatui)) and **[crossterm](https://github.com/crossterm-rs/crossterm)** — huge thanks to the Ratatui project. Also built on [tokio](https://tokio.rs), [reqwest](https://github.com/seanmonstar/reqwest), [serde](https://serde.rs), and [clap](https://github.com/clap-rs/clap).

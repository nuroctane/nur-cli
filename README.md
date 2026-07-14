# Meta CLI (unofficial)

<p align="center">
  <img src="docs/assets/muse-demo-obfuscated.gif" alt="Meta CLI demo" width="600">
</p>

**FULLY LOADED coding agent** for [Meta Model API](https://dev.meta.ai/) — not a thin wrapper. Custom Rust harness, dense Meta-blue TUI, **native vision**, tools, knowledge stack, hardened sandbox. Any model id via `--model` / `/model` / config.

> Not affiliated with Meta Platforms, Inc. · Community · [nuroctane/meta-cli](https://github.com/nuroctane/meta-cli)

```text
meta          # primary — Meta-blue interactive TUI
muse          # legacy alias (same binary)
```

---

## Install — dead simple

One shot. That’s it. The **one-liner** (builds from source) or the **Windows EXE** (prebuilt) each drop `meta` on your PATH, pull in every runtime dependency they can, and wire the full agent stack **before** the TUI opens.

### <img alt="Windows (PowerShell) — recommended" src="https://img.shields.io/badge/Windows_(PowerShell)_—_recommended-a855f7?style=for-the-badge">

```powershell
irm https://raw.githubusercontent.com/nuroctane/meta-cli/main/install.ps1 | iex
```

### <img alt="macOS / Linux" src="https://img.shields.io/badge/macOS_/_Linux-a855f7?style=for-the-badge">

```bash
curl -fsSL https://raw.githubusercontent.com/nuroctane/meta-cli/main/install.sh | bash
```

### After install (30 seconds)

```text
meta auth login      # paste key from https://dev.meta.ai/  →  ~/.meta/auth.json only
meta                 # open the TUI
meta doctor          # health check
```

Or skip the CLI login: run `meta` and use **`/login`** in the TUI (masked entry). No key? login opens automatically.

### Update (do this later)

```bash
meta update
```

**That’s how you upgrade.** Pulls latest `main` when a Laboratory checkout exists (`~/laboratory/meta-cli` or `~/Laboratory/meta-cli`), runs `cargo build --release`, reinstalls `meta` on PATH, and re-provisions the ecosystem stack. No checkout? It falls back to `meta install` (self-repair).

| Also fine | |
|-----------|--|
| Re-run the **one-liner** above | Full rebuild from GitHub |
| Re-download + double‑click **Windows EXE** | Prebuilt path |
| `meta install` | Reinstall *this* binary + stack (no git pull) |

Verify: `meta --version` · `meta doctor`. Full write-up: [docs/setup.md → Update](./docs/setup.md#update-keep-meta-current).

### Other ways to install

<table>
<tr>
<td width="33%"><strong>① One-liner (above)</strong><br/>Easiest. Builds from source + full stack.</td>
<td width="33%"><img alt="② Prebuilt EXE (Windows)" src="https://img.shields.io/badge/②_Prebuilt_EXE_(Windows)-a855f7?style=for-the-badge"><br/>Download → double‑click → done.</td>
<td width="33%"><strong>③ From a clone</strong><br/>You already have the repo.</td>
</tr>
</table>

### <img alt="② Prebuilt Windows binary" src="https://img.shields.io/badge/②_Prebuilt_Windows_binary-a855f7?style=for-the-badge">

Same full stack as the one-liner (no compile).

1. Open **[Releases → latest](https://github.com/nuroctane/meta-cli/releases/latest)**
2. Download **`meta-windows-x86_64.exe`**
3. **Double‑click it** (or `.\meta-windows-x86_64.exe` in a terminal)

The EXE is a **one-stop installer**: copies itself to `~\.local\bin\meta.exe`, adds PATH, pulls prereqs it can (node · bun · uv · rg · ffmpeg), runs **ecosystem ensure** + **browser setup**, then opens Meta. No hand-rolled PATH. No “open TUI while packs install later.”

Sign in when prompted (`/login`, or `meta auth login`). **To upgrade day-to-day: `meta update`.** Or re-download + re-run the release EXE anytime.

**③ Already cloned**

```powershell
cd meta-cli
.\install.ps1          # Windows
# ./install.sh         # macOS / Linux
```

**④ Manual cargo build** (power users)

```bash
git clone https://github.com/nuroctane/meta-cli.git
cd meta-cli
cargo build --release
# one-stop from the binary you just built:
./target/release/meta install   # Windows: .\target\release\meta.exe install
meta auth login
```

---

### What the one-liner and EXE install on your PC

Everything below is **local to your machine**. Nothing secrets-related is written into the git repo.

#### A. Tooling the one-liner / EXE may install (if missing)

| Piece | Where it usually lands | Why Meta needs it |
|-------|------------------------|-------------------|
| **Rust / cargo** (rustup stable) | `~/.cargo/` | Builds the CLI (**one-liner / cargo only** — not needed for the release EXE) |
| **Git** | system | One-liner / clone paths only |
| **Node.js 20+ LTS** | system / winget / package manager | PLUR · Ruflo · Executor · skills · browser CLI · AKM |
| **Bun** | `~/.bun/` | **omp** (Oh My Pi) backend |
| **uv** | `~/.local/bin` | **Graphify** |
| **ripgrep (`rg`)** | system | Fast `grep` / `glob` (native fallback if absent) |
| **ffmpeg** | system | `extract_frames` / design-from-video |

#### B. Meta CLI itself

| Piece | Path |
|-------|------|
| **`meta` binary** | `~/.local/bin/meta` (Windows: `meta.exe`) |
| **`muse` alias** | same dir — identical binary, legacy name |
| **SHA-256 record** | `~/.local/bin/meta.sha256` |
| **Source checkout** (one-liner) | `~/laboratory/meta-cli` (override with `RepoDir` / `META_CLI_DIR`) |
| **User PATH** | `~/.local/bin` appended (Windows User PATH · shell rc on Unix) |

#### C. Agent data home — `~/.meta/` (created on first use / auth)

| Path | Purpose |
|------|---------|
| `auth.json` | API key after login (**only place keys live**) |
| `config.toml` | Model, effort, budgets, compact, poor mode, … |
| `bootstrap.json` | One-stop install marker (EXE / `meta install`) |
| `ecosystem.json` | Ecosystem ensure marker |
| `permissions.toml` | Optional allow/deny/ask rules |
| `hooks.toml` | Optional pre/post tool hooks |
| `meta.log` | Tracing (not painted into the TUI) |
| `status.json` · `usage.jsonl` · `ade.json` | Live usage + ADE / Orca panels |
| `memory.md` · `history.jsonl` | Cross-session notes + prompt history |
| `sessions/` | Chat sessions (`*.json`, `*.json.bak`, `*.precompact.bak`) |
| `tool-results/` | Spilled oversized tool outputs |
| `browser-extension/` | Staged `tmwd_cdp_bridge` for the browser tool |
| `skills/` · `skill-packs/` · `ruflo/` | Skills + vector memory store |

#### D. Ecosystem packs (one-liner · EXE · `meta install` / `ecosystem ensure`)

Installed as **external CLIs / skill trees** when Node/uv/Bun are available — not baked into the binary:

| Component | What you get |
|-----------|----------------|
| **Graphify** | Code knowledge graph CLI (`uv` / Python) |
| **PLUR** | Shared engram memory |
| **Ruflo** | Vector memory / swarm helpers under `~/.meta/ruflo/` |
| **Executor** | MCP / OpenAPI gateway tooling |
| **omp** | Oh My Pi headless coding backend (needs Bun) |
| **agent-browser-cli** | Real-browser bridge (npm) |
| **Skills + AKM** | Progressive skill packs under `~/.meta/skills` / agents skills dirs |
| **Browser setup** | Stages extension; opens `chrome://extensions` once for Load unpacked |

#### E. Optional host integration

| Piece | Notes |
|-------|--------|
| **Orca hook** | Best-effort `meta install-hook` if Orca is present |
| **Auth from env** | If `META_API_KEY` / `MODEL_API_KEY` is set, saved to `~/.meta/auth.json` only |

**That’s the full stack** — binary + PATH + runtimes + knowledge/browser packs + local data home. One-liner first run may spend a few minutes on `cargo build`; the EXE skips compile but still runs ecosystem install **up front**. Later sessions open fast; `ecosystem_auto_ensure` only does light **background repair** when packs drift.

Docs: **[nuroctane.github.io/meta-cli](https://nuroctane.github.io/meta-cli/)** · Setup detail: [docs/setup.md](./docs/setup.md)

---

**v0.10.0** — Production-minded agent harness, end to end: **[Docs](https://nuroctane.github.io/meta-cli/)**

| Surface | What ships |
|---------|------------|
| **TUI** | Streaming · duration chips · expandable thought/tool cards · click-to-peek · **drag-select** · always-on scrollbar · ↓ End · sticky prompt · sessions browser · approval mini-diff · **`/cd` `/pwd` `/context` `/status` `/doctor` `/budget` `/poor` `/permissions` `/hooks`** |
| **Agent** | Manual / plan / auto · tool loop · subagents · todos · **smarter auto-compact** · **session $ / token budgets** · tool-result spill · Esc cancel · Shift+Tab mid-turn · prompt-cache keys |
| **Vision** | **`look`** (images / short video) · **`extract_frames`** (ffmpeg keyframes) · prompt auto-attach of media paths |
| **Tools** | read · edit · bash · web · **browser** (real default browser: Arc/Chrome/Edge/…) · git · knowledge stack · agent — **all first-class** (no deferred demotion) |
| **Ecosystem** | Graphify · PLUR · Ruflo · Executor · **omp** · **browser** · AKM · **800+ skills** — installed at setup; later open = TTL **repair** (`ecosystem_auto_ensure`) |
| **Hardening** | Sandbox · bash denylist · SSRF blocks · atomic `~/.meta` IO · **session `.json.bak`** · **permissions.toml** · optional **hooks.toml** · API retries · install SHA-256 · `meta doctor` |
| **Host panels** | Live `status.json` / `usage.jsonl` · Orca hook when present |

---

## Why Meta CLI

| | |
|--|--|
| **Real agent, not a wrapper** | Custom Rust harness: modes, tools, sandbox, streaming, cancel, subagents, auto-compact |
| **Sees media** | Muse multimodal via Responses `input_image` / `input_video` — sparse frames, not frame-by-frame spam |
| **One-shot install** | One-liner **or** Windows EXE · PATH · ecosystem · browser · Orca hook · optional auth |
| **Easy updates** | **`meta update`** — pull · rebuild · reinstall stack (or re-run one-liner / EXE) |
| **Install first, then TUI** | Full stack runs **before** the UI; later sessions only do light background repair |
| **Knowledge stack** | Code graph · shared engrams · vector memory · MCP gateway · skill packs |
| **Resume other agents** | Skills: `resume-claude` · `resume-codex` · `resume-cursor` · `resume-meta` · **`resume-grok`** (shared reader: `~/.meta/skills/resume-session/`) |
| **Simple input** | Drag-select · scrollbar · **Ctrl+A / C / V / X** — no mouse “mode” toggle |
| **Secrets stay local** | API key only in `~/.meta/auth.json` |

---

## Feature map

### Agent harness
- Meta Model API **Responses** streaming + reasoning effort (`minimal` → `xhigh`)
- **Manual / plan / auto** permission modes — **Shift+Tab** applies mid-turn
- Tool loop with fail-closed capability flags (read-only / parallel / destructive), approval gates, Esc cancel
- **Subagents**, todos, plan mode (`submit_plan`)
- **Session budgets** — hard stop on `$` and/or tokens (`/budget`, `max_session_cost_usd` / `max_session_tokens`)
- **Tool-result spill** — oversized tool output → `~/.meta/tool-results/` + short model preview
- **Smarter auto-compact** — thins old tool bodies, keeps recent turns, writes `.precompact.bak`
- Optional **`permissions.toml`** allow/deny/ask patterns; optional **`hooks.toml`** pre/post tool
- **`/poor`** — cost-saver prompt (skip PLUR inject / skills catalog / long memory; tools stay full)
- Project instructions: `META.md` · `AGENTS.md` · `CLAUDE.md` (legacy `MUSE.md` still loaded)
- Session resume (`-c`, `-r`, `/sessions`) — defaults to **all** workspaces; dual `~/.meta` / `~/.muse` prefers richer copy
- **Prompt cache key** per session (helps surface `cached_tokens`)

### Tools (native)

| Family | Tools |
|--------|--------|
| read | `read_file` `list_dir` `grep` `glob` |
| edit | `write_file` `edit_file` `multi_edit` `apply_patch` |
| shell | `bash` (hardened denylist + timeout) |
| **vision** | **`look`** · **`extract_frames`** |
| web | `web_search` `web_fetch` (text only; SSRF / private-IP blocks) |
| browser | `browser` — the user's **real default browser** (Arc/Chrome/Edge/Brave/…) via agent-browser-cli: tabs · snapshot (@e refs) · click/fill/keys · JS · screenshots (pair with `look`); setup via `meta browser setup` |
| git | `git_status` `git_diff` |
| knowledge | `graphify` `plur` `ruflo` `executor` `skill` `memory` |
| delegate | `agent` `omp` — omp.sh coding-agent backend (LSP renames, DAP debugging, AST rewrites) |
| agent | `todo_write` `submit_plan` `agent` |

### Vision (design / multimodal)

Muse Spark accepts multimodal input on the Responses API. Meta CLI wires that in:

| Tool | What it does |
|------|----------------|
| **`look`** | Attach workspace **image(s)** (png/jpg/webp/gif) or a **short video** (mp4/webm/mov, ~20MB cap) so the model *sees* them on the next turn |
| **`extract_frames`** | Sparse **keyframes** via **ffmpeg** (default ~1 fps, max ~8) → `.meta/frames/<name>/` and auto-queues `look` |

**Efficient design-from-video (e.g. 10s reference clip):**

```text
meta "steal UI design tokens from demo.mp4 and scaffold a matching component"
```

Or: `extract_frames` → model inspects stills → implement with **design-eng** skills.

- Paths like `demo.mp4` / `shot.png` in the user prompt **auto-attach** when the file exists in the workspace  
- Prefer sparse frames over frame-by-frame every pixel  
- Longer / huge videos: extract frames first; don’t `look` a giant file  

### Ecosystem (auto-provisioned)

| Piece | Role |
|-------|------|
| **Graphify** | Code knowledge graph (`graphify-out/`) — query / path / explain |
| **PLUR** | Shared engram memory across tools/sessions |
| **Ruflo** | Vector memory + swarm/hive patterns |
| **Executor** | MCP / OpenAPI gateway catalog |
| **Skills** | Progressive packs (design-eng, clone-website, cybersecurity, …) via `skill` |
| **AKM** | Agent knowledge package manager (when Node available) |

### TUI (Meta-blue)
- Streaming assistant · violet **thought** cards · colour-coded **tool** cards  
- **Duration chips** · cards collapsed by default · click-to-peek · **↓ End**  
- **Drag text to select** (auto-copy) · **drag scrollbar** always on  
- **Ctrl+A** select-all · **Ctrl+C** copy · **Ctrl+V** paste · **Ctrl+X** cut  
- Sticky PROMPT banner · sessions modal · approval **mini-diff**  
- Splash shows the **active model title** only there; rest of chrome is model-agnostic  

### Reliability & safety
- Atomic writes under **`~/.meta/`** (auth, sessions, status, history)  
- Session saves write **`*.json.bak`** first; compaction writes **`*.precompact.bak`**  
- API **retries** · process timeouts · config validation · `meta doctor`  
- Install scripts verify **SHA-256** of the binary  
- Gap-fill migrate from legacy `~/.muse/` (never overwrites existing `.meta` files)  
- Logs to **`~/.meta/meta.log`** (never paints stderr over the TUI)

---

## Secrets (important)

| On GitHub | On your PC only |
|-----------|-----------------|
| Source, README, install scripts | `~/.meta/auth.json` (API key) |
| No keys, no `.env`, no sessions | `~/.meta/sessions/`, usage logs, frames under workspace `.meta/frames/` |

See [SECURITY.md](./SECURITY.md). **Never commit your Meta API key.**

Upgrading from older builds: gap-fill copy from `~/.muse/` → `~/.meta/` for any missing files (auth, sessions, ruflo, skills, …). `meta auth logout` clears **both** homes.

---

## Quick use

```text
meta                         # interactive Meta-blue TUI
meta "fix the bug"          # start with a prompt
meta "design from ref.mp4"   # vision: auto-attach media if path exists
meta -c                      # continue last session in this directory
meta -r <session-id>         # resume a session
meta --mode plan "…"         # plan mode (explore + shell freely; no edits/commits)
meta run "…" -y              # headless + auto-approve
meta sessions
meta usage
meta auth status
meta ecosystem status
meta ecosystem ensure --force
meta doctor                  # auth · config · ecosystem · PATH · ffmpeg · sha256
```

Launching from a drive root (`C:\`) auto-picks a safe workspace (git / last session / Laboratory).

---

## Permission modes (live — Shift+Tab)

| Mode | Behavior |
|------|----------|
| **manual** | Reads free (`look`, reads, …); writes / shell / `extract_frames` need approval (`y` / `a` / `n`) |
| **plan** | Explore + analyze freely — reads, `look`, graphify/plur/ruflo queries, **and shell** for reading/parsing/tests/scratch + media compute (`ffmpeg`, `extract_frames`, copy a clip). Blocks only **code authoring** (`write_file`/`edit_file`/`multi_edit`/`apply_patch`) and **repo/VCS mutations** (git commit/push/add/reset/…, `gh pr create`, dependency installs) |
| **auto** | Auto-approve tools (`-y` / `--mode auto`) |

---

## TUI

### Keys

| Key | Action |
|-----|--------|
| `↑` `↓` · wheel · drag scrollbar | Scroll transcript |
| **Drag on chat text** | Select + auto-copy (survives scroll; expanded thought/tool text included) |
| **Click `↓ N · End`** | Jump to latest |
| Click exact **click to peek** text | Stable peek (frozen; Esc · outside · ✕) · `▸` expands |
| **Ctrl+A** | Select-all input (or whole transcript if input empty) |
| **Ctrl+C** | Copy selection · open peek body · else interrupt / double-tap quit |
| **Ctrl+V** | Paste into input |
| **Ctrl+X** | Cut input selection (or whole input) |
| `Shift+Tab` | Cycle permission mode |
| `Ctrl+R` | Sessions browser |
| `y` / `a` / `n` | Approve once / always / deny |
| `Esc` | Close peek, then cancel turn |

### Slash commands

| Command | Purpose |
|---------|---------|
| `/help` | Keys + commands |
| `/mode` `/plan` `/manual` `/auto` | Permission |
| `/cd` `/pwd` | Change / print workspace cwd |
| `/todos` `/memory` `/skills` | Session state |
| `/graphify` `/plur` `/ruflo` `/ecosystem` | Knowledge stack |
| `/compact` `/usage` `/cost` `/context` `/status` | Context, tokens, cost |
| `/budget` | Session spend ceiling (`cost` / `tokens` / `clear` / `save`) |
| `/poor` | Toggle cost-saver prompt (tools stay full) |
| `/permissions` | Show / reload `permissions.toml` rules |
| `/hooks` | Local `hooks.toml` status |
| `/doctor` | Inline health check |
| `/model` `/effort` | Model & reasoning |
| `/sessions` `/resume` | Sessions browser (all workspaces by default) |
| `/init` `/config` `/clear` `/new` `/exit` | Project & shell |
| `/login` `/logout` | Authenticate / clear stored key |
| `/bug` | Open GitHub issues page |

### Quick memory

Type `#` followed by a note to save it directly to `~/.meta/memory.md` without starting a turn — persisted and recalled across sessions.

### Colour system

| Family | Hue | Tools |
|--------|-----|-------|
| read | sky | `read_file` `list_dir` `grep` `glob` |
| edit | violet | `write_file` `edit_file` `multi_edit` `apply_patch` |
| shell | amber | `bash` |
| vision | pink | `look` `extract_frames` |
| web | teal | `web_fetch` `web_search` |
| git | cyan | `git_status` `git_diff` |
| knowledge | indigo / orange | `graphify` `plur` `ruflo` `skill` `memory` … |

---

## ADE / Orca

| Path | Role |
|------|------|
| `~/.meta/status.json` | Live tokens · cost · model · state |
| `~/.meta/usage.jsonl` | Per-request log |
| `~/.meta/ade.json` | Discovery manifest |

```text
meta install-hook
orca terminal create --command meta
```

---

## Config

`~/.meta/config.toml` (created on first run):

```toml
model = "muse-spark-1.1"   # any Meta Model API model id
base_url = "https://api.meta.ai/v1"
reasoning_effort = "high"
max_turns = 40
stream = true
context_window = 1000000
tool_result_max_chars = 12000   # 0 = unlimited; spill oversized tool output
# max_session_cost_usd = 5.0    # optional hard stop
# max_session_tokens = 500000   # optional hard stop
compact_keep_user_turns = 4
compact_tool_body_max_chars = 800
poor_mode = false
ecosystem_auto_ensure = true    # background TTL pack repair on later TUI opens
```

Optional files:

| Path | Purpose |
|------|---------|
| `~/.meta/permissions.toml` | `allow` / `deny` / `ask` patterns (`bash:git *`, …) |
| `~/.meta/hooks.toml` | `pre_tool` / `post_tool` shell hooks |
| `~/.meta/tool-results/` | Spilled large tool outputs |
| `~/.meta/meta.log` | Tracing (not the terminal) |

Override home with `META_HOME` (legacy `MUSE_HOME` still honored). Env: `META_API_KEY` / `MODEL_API_KEY` / `META_MODEL`.

---

## Acknowledgements

The whole terminal UI — every card, border, animation, and the drag-select /
scrollbar plumbing — is built on **[Ratatui](https://ratatui.rs/)**
([github.com/ratatui/ratatui](https://github.com/ratatui/ratatui)), the
Rust TUI library, with **[crossterm](https://github.com/crossterm-rs/crossterm)**
underneath for input and rendering. Meta CLI's dense Meta-blue interface simply
wouldn't exist without the Ratatui folks — huge thanks to them. 💙

Assistant markdown in the transcript is parsed by joshka's
**[tui-markdown](https://github.com/joshka/tui-markdown)** — we re-tint its
output to the Meta-blue palette on top. Long peek dialogues scroll via
**[tui-scrollview](https://crates.io/crates/tui-scrollview)**, inline image
peeks render through **[ratatui-image](https://crates.io/crates/ratatui-image)**
(sixel / kitty / iTerm2, halfblocks fallback), and the smooth fractional
scrollbar is modelled on **[tui-scrollbar](https://crates.io/crates/tui-scrollbar)**'s
subcell math.

The `omp` tool delegates to **[Oh My Pi](https://omp.sh)**
([can1357/oh-my-pi](https://github.com/can1357/oh-my-pi)) — headless backend
runs only, provisioned automatically when Bun is available.

The `browser` tool drives the user's real, **default browser** — Arc, Chrome,
Edge, Brave, or any Chromium browser — through
**[agent-browser-cli](https://github.com/sleepinginsummer/agent-browser-cli)**
(browser bridge lineage from
[GenericAgent](https://github.com/lsdefine/GenericAgent)) — login state stays
in the browser, cookies are never exposed to the model. Install auto-detects
your default browser and stages the extension; `meta browser setup` finishes
the one-time load.

Also built on: [tokio](https://tokio.rs), [reqwest](https://github.com/seanmonstar/reqwest),
[serde](https://serde.rs), and [clap](https://github.com/clap-rs/clap).

---

## License

**GNU General Public License v3.0 (or later)** — see [LICENSE](./LICENSE).

Meta CLI is free software: you may redistribute it and/or modify it under the
terms of the GPL as published by the Free Software Foundation, either version 3
of the License, or (at your option) any later version. It is distributed in the
hope that it will be useful, but **without any warranty**; without even the
implied warranty of merchantability or fitness for a particular purpose.

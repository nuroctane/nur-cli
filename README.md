# [NurCLI](https://www.nuroctane.xyz/cli)

<div align="center">

### Extremely efficient token spend

**Cut costs for every provider by up to ~85%.**  
The harness is built to burn fewer tokens. Less waste, more work per dollar, across OpenAI, Anthropic, Grok, Gemini, Meta Model API, and the rest of the catalog.

</div>

<p align="center">
  <img src="docs/assets/nur-cli-logo.png" alt="NurCLI logo" width="132">
</p>

<p align="center">
  <img src="docs/assets/nur-demo.gif" alt="NurCLI demo" width="600">
</p>

**Your personal coding agent.** Custom Rust harness, dense gold TUI, **native vision**, tools, knowledge stack, hardened sandbox. Multi-provider `/login`. Any model via `--model` / `/model` / config.

```text
nur          # interactive gold TUI
```

[NurCLI]([https://www.nuroctane.xyz/cli])

---

## Install: dead simple

One shot. The **one-liner** (builds from source) or the **Windows EXE** (prebuilt) each drop `nur` on your PATH, pull in every runtime dependency they can, and wire the full agent stack **before** the TUI opens.

### <img alt="Windows (PowerShell) - recommended" src="https://img.shields.io/badge/Windows_(PowerShell)_-_recommended-a855f7?style=for-the-badge">

```powershell
irm https://raw.githubusercontent.com/nuroctane/nur-cli/main/install.ps1 | iex
```

### <img alt="macOS / Linux" src="https://img.shields.io/badge/macOS_/_Linux-a855f7?style=for-the-badge">

```bash
curl -fsSL https://raw.githubusercontent.com/nuroctane/nur-cli/main/install.sh | bash
```

### After install

```text
nur auth login      # key -> ~/.nur/auth.json  (or set NUR_API_KEY)
nur                 # open the TUI
nur doctor          # health check
```

Or run `nur` and use **`/login`** in the TUI: pick any of **60+ providers**
(OpenAI, Anthropic, Gemini, xAI, Groq, OpenRouter, OmniRoute, local Ollama/LM Studio, Meta Model API, and so on).
For **Antigravity, Hugging Face, Azure, AWS Bedrock, and GitHub Models** you can **sign in with the browser** or paste an API key. OAuth sessions refresh before use, retry once after an authentication rejection, and `/model` detects the models available to the active credential. No credential on launch and login opens automatically.

OpenAI, Anthropic, xAI, and Kimi are **API key only** — those vendors issue subscription OAuth tokens exclusively to their own CLIs (Codex, Claude Code, Grok CLI, Kimi Code), so `/login` goes straight to the key prompt. See [docs/authentication.md](docs/authentication.md#why-openai-anthropic-xai-and-kimi-are-api-key-only).

### Update

```bash
nur update
```

Pulls latest `main` when a Laboratory checkout exists (`~/laboratory/nur-cli` or `~/Laboratory/nur-cli`), rebuilds, reinstalls `nur` on PATH, re-provisions the ecosystem. No checkout? Falls back to `nur install`.

| Also fine | |
|-----------|--|
| Re-run the **one-liner** | Full rebuild from GitHub |
| Re-download + double-click **Windows EXE** | Prebuilt path |
| `nur install` | Reinstall *this* binary + stack (no git pull) |

Verify: `nur --version` · `nur doctor`. Detail: [docs/setup.md → Update](./docs/setup.md#update-keep-nurcli-current).

### Other ways to install

<table>
<tr>
<td width="33%"><strong>1. One-liner (above)</strong><br/>Easiest. Builds from source + full stack.</td>
<td width="33%"><img alt="2. Prebuilt EXE (Windows)" src="https://img.shields.io/badge/2_Prebuilt_EXE_(Windows)-a855f7?style=for-the-badge"><br/>Download → double-click → done.</td>
<td width="33%"><strong>3. From a clone</strong><br/>You already have the repo.</td>
</tr>
</table>

### <img alt="2. Prebuilt Windows binary" src="https://img.shields.io/badge/2_Prebuilt_Windows_binary-a855f7?style=for-the-badge">

1. Open **[Releases → latest](https://github.com/nuroctane/nur-cli/releases/latest)**
2. Download **`nur-windows-x86_64.exe`**
3. **Double-click it** (or `.\nur-windows-x86_64.exe` in a terminal)

Copies itself to `~\.local\bin\nur.exe`, PATH, prereqs, ecosystem + browser setup, then opens NurCLI.

**3. Already cloned**

```powershell
cd nur-cli
.\install.ps1          # Windows
# ./install.sh         # macOS / Linux
```

**4. Manual cargo build**

```bash
git clone https://github.com/nuroctane/nur-cli.git
cd nur-cli
cargo build --release
./target/release/nur install   # Windows: .\target\release\nur.exe install
nur auth login
```

---

### What install puts on your PC

Everything is **local**. Secrets never go into the git repo.

#### A. Tooling install may add (if missing)

| Piece | Why |
|-------|-----|
| **Rust / cargo** | Builds the CLI (one-liner / cargo only) |
| **Git** | Clone / update |
| **Node.js 20+** | PLUR · Ruflo · Executor · skills · browser · AKM |
| **Bun** | **omp** backend |
| **uv** | **Graphify** |
| **ripgrep (`rg`)** | Fast `grep` / `glob` |
| **ffmpeg** | `extract_frames` |

#### B. NurCLI itself

| Piece | Path |
|-------|------|
| **`nur` binary** | `~/.local/bin/nur` (Windows: `nur.exe`) |
| **SHA-256 record** | `~/.local/bin/nur.sha256` |
| **Source checkout** (one-liner) | `~/laboratory/nur-cli` (override `NUR_CLI_DIR`) |
| **User PATH** | `~/.local/bin` |

#### C. Data home: `~/.nur/`

| Path | Purpose |
|------|---------|
| `auth.json` | API key after login |
| `config.toml` | Model, effort, budgets, etc. |
| `sessions/` | Chat sessions |
| `plugins/` · `plugins/registry.json` | Marketplace installs (`/plugins`, `nur plugins`) |
| `skills/` · `ruflo/` · `tool-results/` | Skills, vector memory, spilled tool output |
| `status.json` · `usage.jsonl` · `ade.json` | Live usage + host panels |
| `nur.log` | Tracing (not painted into the TUI) |

Older builds may still have data under `~/.meta` or `~/.muse`. On first run NurCLI **gap-fills** missing files into `~/.nur/` (never overwrites). Delete the old homes once you're happy everything works.

#### D. Ecosystem packs

Graphify · PLUR · Ruflo · Executor · omp · agent-browser-cli · skill packs. Installed when Node/uv/Bun are available.

Docs: **[nuroctane.xyz/cli](https://www.nuroctane.xyz/cli)** · [docs/setup.md](./docs/setup.md)

---

**v0.13.4**: Natural-language skill activation · Fable + Superpowers phrases · full skill mirror · NUR_* host-panel envs · plugins. **[Docs](https://www.nuroctane.xyz/cli)**

| Surface | What ships |
|---------|------------|
| **TUI** | Streaming · duration chips · thought/tool cards · peek · drag-select · scrollbar · sessions · multi-provider `/login` · **`/model` picker** · **`/plugins` marketplace** · `/goal` `/bro` `/scan` `/btw` `/codesearch` `/mc` `/feedback` `/tips` · budgets · doctor |
| **Agent** | Manual / plan / auto · tools · subagents · todos · auto-compact · session $ / token budgets · Esc cancel · Shift+Tab mid-turn · **NL skill auto-activation** |
| **Vision** | `look` · `extract_frames` · prompt auto-attach of media paths |
| **Tools** | read · edit · bash · web · **browser** · git · knowledge · agent · **excalidraw** |
| **Ecosystem** | Graphify · PLUR · Ruflo · Executor · omp · browser · AKM · 800+ skills · **plugin marketplace** (Fable, Superpowers, Vercel, …) |
| **Hardening** | Sandbox · denylist · SSRF blocks · atomic `~/.nur` IO · permissions/hooks · SHA-256 install · `nur doctor` |

---

## Why NurCLI

| | |
|--|--|
| **Real agent, not a wrapper** | Modes, tools, sandbox, streaming, cancel, subagents, auto-compact |
| **Sees media** | Multimodal images/short video. Sparse frames, not spam. |
| **One-shot install** | One-liner or Windows EXE · PATH · ecosystem · browser |
| **Easy updates** | `nur update` |
| **Knowledge stack** | Graph · engrams · vector memory · MCP · skills |
| **Plugin marketplace** | `/plugins` picker (same UX as `/login`) · install Superpowers, Vercel, Firecrawl, Fable, … into `~/.nur/plugins` |
| **Natural-language skills** | *think like fable* · *TDD this* · *debug systematically* · *polish the UI* · *resume from Claude* — no slash required |
| **Resume other agents** | `resume-claude` · `resume-codex` · `resume-cursor` · `resume-nur` · `resume-grok` |
| **Secrets stay local** | Keys only in `~/.nur/auth.json` (or env) · prefer `NUR_API_KEY` |

---

## Feature map

### Agent harness
- **Multi-provider** via `/login` (60+); Responses or Chat Completions adapter
- Manual / plan / auto · Shift+Tab mid-turn
- Tool loop, approvals, Esc cancel, subagents, todos, plan mode
- Session budgets (`/budget`), tool-result spill, smarter auto-compact
- Optional `permissions.toml` / `hooks.toml`
- `/poor` cost-saver prompt
- Project instructions: `NUR.md` · `AGENTS.md` · `CLAUDE.md` (also loads legacy `META.md` / `MUSE.md` if present)
- Session resume: `-c`, `-r`, `/sessions`
- `/model` opens a live model list for the active provider (or `/model <id>` to set one directly)
- `/plugins` marketplace picker (same UX as `/login`): install Superpowers, Vercel, Firecrawl, Chrome DevTools, **Fable**, and more into `~/.nur/plugins`
- **Natural-language skill activation**: plain phrases inject the skill body for the turn (Fable, TDD, systematic debugging, design-eng, resume-*, Excalidraw, …). Status chip confirms activation. [Docs](https://www.nuroctane.xyz/cli/ecosystem/#natural-language-skill-activation)
- **`/fusion`** — multi-model debate → one synthesized answer (panel of providers, active model judges)
- **`--continuous`** — sovereign/autonomous mode: loop headless turns toward a goal until `DONE` or Ctrl+C
- **`/local`** — run a model locally with **bundled llama.cpp** (auto-fetch `llama-server` + a GGUF sized to your RAM); no API key
- **`/bench`** — benchmark models on your own tasks, replayed in isolated git worktrees and scored
- **`nur gateway`** — run headless as a Telegram bot; each message is an agent turn in your project

### Tools (native)

| Family | Tools |
|--------|--------|
| read | `read_file` `list_dir` `grep` `glob` |
| edit | `write_file` `edit_file` `multi_edit` `apply_patch` |
| shell | `bash` |
| vision | `look` · `extract_frames` |
| web | `web_search` `web_fetch` |
| browser | real default browser via agent-browser-cli |
| git | `git_status` `git_diff` |
| knowledge | `graphify` `plur` `ruflo` `executor` `skill` `memory` |
| diagrams | `excalidraw` (hand-drawn `.excalidraw` via excalidraw-cli) |
| agent | `todo_write` `submit_plan` `agent` `omp` |

### Vision

| Tool | What it does |
|------|----------------|
| **`look`** | Attach workspace images or short video so the model sees them |
| **`extract_frames`** | Sparse keyframes via ffmpeg → `.nur/frames/<name>/` |

```text
nur "steal UI design tokens from demo.mp4 and scaffold a matching component"
```

### Ecosystem

| Piece | Role |
|-------|------|
| **Graphify** | Code knowledge graph (`graphify-out/` in the workspace; gitignored, regenerable) |
| **Excalidraw** | Architecture / flow diagrams → `.excalidraw` (`npm i -g excalidraw-cli`; auto-provisioned) |
| **PLUR** | Shared engram memory |
| **Ruflo** | Vector memory / swarm helpers under `~/.nur/ruflo/` |
| **Executor** | MCP / OpenAPI gateway |
| **Skills** | Progressive packs via `skill` |
| **AKM** | Skill package manager |

### TUI (gold)
- Streaming · thought/tool cards · duration chips · peek · ↓ End  
- Drag-select · scrollbar · Ctrl+A / C / V / X  
- Sticky prompt · sessions · approval mini-diff  
- Splash: **NUR** logotype + active provider  

### Reliability
- Atomic writes under `~/.nur/`  
- Session `*.json.bak` · compaction `.precompact.bak`  
- API retries · `nur doctor` · install SHA-256  
- Logs: `~/.nur/nur.log`  

---

## Secrets

| On GitHub | On your PC only |
|-----------|-----------------|
| Source, install scripts | `~/.nur/auth.json` |
| No keys in the repo | sessions, usage, frames under `~/.nur` / workspace |

Env: **`NUR_API_KEY`** preferred. Vendor keys (`OPENAI_API_KEY`, `META_API_KEY` for Meta Model API, etc.) still work. Home override: **`NUR_HOME`**.

---

## Quick use

```text
nur                         # interactive TUI
nur "fix the bug"          # start with a prompt
nur "design from ref.mp4"   # vision: auto-attach media if path exists
nur -c                      # continue last session in this directory
nur -r <session-id>         # resume a session
nur --mode plan "..."       # plan mode
nur run "..." -y            # headless + auto-approve
nur sessions
nur usage
nur auth status
nur ecosystem status
nur ecosystem ensure --force
nur doctor
```

Launching from a drive root (`C:\`) auto-picks a safe workspace (git / last session / Laboratory).

---

## Permission modes (Shift+Tab)

| Mode | Behavior |
|------|----------|
| **manual** | Reads free; writes / shell need approval |
| **plan** | Explore + shell freely; blocks code authoring and VCS mutation |
| **auto** | Auto-approve tools (`-y`) |

---

## TUI keys

| Key | Action |
|-----|--------|
| ↑ ↓ · wheel · scrollbar | Scroll transcript |
| Drag chat text | Select + auto-copy |
| Click `↓ N · End` | Jump to latest |
| Ctrl+A / C / V / X | Select-all · copy · paste · cut |
| Enter | Send |
| Shift+Enter | Newline |
| Shift+Tab | Cycle permission mode |
| Ctrl+R | Reverse-search prompt history (Ctrl+R steps older, Esc cancels) |
| y / a / n | Approve once / always / deny |
| Esc | Close peek → cancel turn |

### Slash commands (highlights)

| Command | Purpose |
|---------|---------|
| `/help` | Keys + commands |
| `/login` `/logout` | Provider + key / clear |
| `/model` | Show and switch models for the active provider |
| `/plugins` | Browse and install marketplace plugins (provider-picker UX) |
| `/goal` `/btw` | Session goal / one-off note |
| `/bro` | Chill mode: plain words, straight answers (toggle) |
| `/scan` | Map the codebase → shareable foglamp architecture scan |
| `/codesearch` `/cs` | Workspace ripgrep |
| `/mc` `/mcp` | MCP via Executor |
| `/feedback` `/tips` | Issue / interaction tips |
| `/budget` `/poor` | Spend ceiling / lean prompt |
| `/cd` `/pwd` `/doctor` `/status` | Workspace + health |
| `/sessions` `/resume` | Session browser |
| `/failover` | Cross-provider failover + privacy tiers (provider-picker; Space adds a fallback, Alt+P sets its privacy tier) |
| `/fusion` | Multi-model debate → one synthesized answer (`/fusion panel <ids>`, then `/fusion <question>`) |
| `/local` | Run a model locally via bundled llama.cpp: `/local up [tier\|url]` · `status` · `models` · `down` |
| `/bench` | Benchmark models on your tasks: `/bench add\|list\|run <name> [models]\|remove` |
| `/undo` | Revert the last file edit (write / edit / multi_edit) this session |
| `/receipt` | Session receipt — verify what actually ran (models, tools, privacy tiers), hash-chained |
| `/cua` | Computer-use desktop driver: `/cua on` = always-on background desktop control (elevated), `off` = on-demand only, `status` |

---

## ADE / Orca

| Path | Role |
|------|------|
| `~/.nur/status.json` | Live tokens · cost · model · state |
| `~/.nur/usage.jsonl` | Per-request log |
| `~/.nur/ade.json` | Discovery manifest |

---

## Config sketch

`~/.nur/config.toml` (created on first run):

```toml
provider = "meta"              # or openai, openrouter, ollama, ...
model = "muse-spark-1.1"       # whatever the active provider expects
base_url = "https://api.meta.ai/v1"
reasoning_effort = "high"
max_turns = 40
```

See [docs/configuration.md](./docs/configuration.md) and [docs/authentication.md](./docs/authentication.md).

---

## License

**GNU General Public License v3.0 (or later)** — see [LICENSE](./LICENSE).

Meta CLI is free software: you may redistribute it and/or modify it under the
terms of the GPL as published by the Free Software Foundation, either version 3
of the License, or (at your option) any later version. It is distributed in the
hope that it will be useful, but **without any warranty**; without even the
implied warranty of merchantability or fitness for a particular purpose.

# penecho Integration

## Overview
`penecho` (https://github.com/penecho/penecho) — "Think with AI beyond the chat box": 20k x 20k canvas, pressure-sensitive ink, sparse 512 tiles, draft layer (unconfirmed outputs draggable/resizable), MathJax (LaTeX->SVG), plots, mixed drawings, declarative animation scenes (32 objects+32 motions max 20 concurrent), fixed-screen markdown/LaTeX editor, PNG export cropped to ink + 1 tile margin, Manual/Auto AI mode 0-10s delay, local snapshots.

Runtime: Node >=18.17, 2 deps only (@inquirer/prompts + sharp), no bundler, vanilla JS client, http core only. Bin `penecho` -> cli.js.

Provider abstraction (api-config.js 49 LOC): `AI_PROVIDER=api|codex-cli|claude-cli`
- API mode: `AI_API_URL`/`OPENAI_API_URL`, `AI_API_KEY`/`OPENAI_API_KEY`, `AI_API_MODEL`/`OPENAI_MODEL`, `AI_API_FORMAT`/`OPENAI_API_FORMAT=openai|anthropic` auto-detect from URL suffix `/chat/completions` vs `/v1/messages` vs `/v1` or `*/openai`. Normalizes endpoint, validates https no user/pass, placeholder detection `your[_ -]|replace|changeme|api[_ -]?key|sk-\...`. Effort mapping `config|none|low|medium|high|max|xhigh` → anthropic `thinking adaptive/disabled` + tokens 8192/16384, openai `reasoning_effort`. Headers: Anthropic `x-api-key` + `anthropic-version`, OpenAI Bearer.
- Codex CLI: `CODEX_CLI_PATH` default `codex`, `findOnPath` with .exe/.cmd/.bat + .js wrapper handling, `resolveCodexLaunch()` spawns child 1MB cap, doctor checks `codex --version`, `codex login status`, `codex debug models --bundled`.
- Claude CLI: `CLAUDE_CLI_PATH` default `claude`, handles .js/.cjs/.mjs => process.execPath + prefixArgs, .ps1 on win, systemPrompt + userPrompt split, atlas image binary arg.
- State: `PENECHO_STATE_DIR` or `~/.penecho/config.env` with export optional + quote handling via JSON.parse, fileEnv+process.env merging, session cookie `penecho_ai_session` + 32-byte base64url token (no JWT).

Architecture: Browser canvas (sparse tiles + anim scenes) -> cropped visual request atlas + 1 fixed animation frame -> Node validation + model executor -> structured AI commands (text/formula/plot/mixed/anim/erase) -> draft layer -> confirmed tiles or declarative anim objects.

## What nur-cli can learn / improve
- Auto-detect openai vs anthropic from URL suffix (cleaner than per-provider flags) — borrow for `nur auth`.
- Effort unified knob (nur has ad-hoc flags) — unify.
- Robust `findOnPath` with Windows extension + .js wrapper detection — more thorough than nur's.
- Placeholder detection for API keys useful for `nur doctor`.
- Prompt headroom reservation 4096 final JSON + 7000 thinking cap reduces truncation.
- Image format toggle webp|png with sharp optional — good degradation.
- Nur is stronger: OAuth browser flow, OS keychain, multi-provider rotation, pricing catalog, skill registry, MCP, background update.

## Full Integration
- `nur penecho` command / skill: wrapper spawning penecho via binary management, auto-detects `AI_PROVIDER` from `nur auth list`, writes `config.env` in `~/.penecho`.
- Provider bridge: Map nur's unified auth (openai/anthropic/github copilot) to PenEcho's `AI_API_*` env via `nur auth export --format penecho` — implemented as `export` action in `penecho` tool.
- Canvas skill: `/penecho` opens PenEcho + injects current conversation context as initial ink.
- CLI transport: Reuse `codex-cli.js`/`claude-cli.js` spawn logic as Rust for `nur codex`/`nur claude` transparent fallback when API key absent but CLI logged in.
- Model atlas: cropped canvas tiles + focus insets could inspire `nur draw` / `nur canvas` where handwriting screenshot sent to model — use image crate.
- Declarative animation output: Support PenEcho's animation JSON (32 objects/motions) as valid `nur` output type alongside tldraw.
- AGPL compliance: sidecar spawn / optional sidecar, not linking code.

## Usage
- `penecho` tool: `action=status|probe|doctor|export|atlas|launch`
- `export` generates `~/.penecho/config.env` from nur auth.
- `doctor` mirrors `cli.js doctor`.
- Skill activation: `/penecho` or "use penecho"

## References
- https://github.com/penecho/penecho
- `api-config.js`, `codex-cli.js`, `claude-cli.js`, `cli.js`, `server.js`, `docs/architecture.md`

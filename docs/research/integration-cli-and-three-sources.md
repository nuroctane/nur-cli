# Integration: CLI Enter/palette fix + pi-subagents + MSW Kernel + Takumi

Covers (1) the slash-command Enter / modal-loading UX bugs you reported, and
(2) the three new sources, studied from primary material and integrated with the
same care as the earlier RLM/Prime/Shepherd/Connectome/AnyDoc/OpenAI work.

---

## 1. CLI fixes (b1 / b2)

### Bug 1 — the double/triple-Enter on slash commands

**Root cause:** the palette's Enter handler compared the typed text against only
the *highlighted* row (`matches[idx].0`). After a first Enter filled the command,
a second Enter would re-fill with whatever row was highlighted (which could be a
different command) instead of submitting — punting to a third Enter. The behavior
was flaky depending on hover/selection.

**Fix:** extracted a pure, tested decision `decide_palette_enter` (`src/tui/app.rs`):
if the typed text is already an exact full command appearing anywhere in the
matches, **submit** it; otherwise **fill** with the highlighted row. Now:
- Enter 1: partial input → fills the full command.
- Enter 2: input is now an exact command → submits.
Two enters, every time, regardless of which row is highlighted.
(`PaletteEnter::{Submit,Fill}` + unit test `palette_enter_fills_then_submits...`.)

### Bug 2 — /sessions & /takeover modal-loading queued-Enter auto-selects

**Root cause:** `open_session_picker` / `open_chagent_picker` load **synchronously**
(disk scan + subprocess session readers), blocking the TUI event loop. The user
sees nothing, presses Enter again, and when the modal finally appears those
queued Enters auto-confirm the first row.

**Fix:** a `picker_opened_at` debounce on the picker — keys (Enter/nav) within the
first ~300ms after the modal opens are treated as stray presses from the loading
window and ignored, so an impatient Enter no longer auto-selects before the user
could read the modal. Set on open (`open_session_picker`, `open_chagent_picker`,
and window-switch rerenders) and reset on close.

---

## 2. New integrations

### pi-subagents (nicopreme) → `subagent-orchestration` skill
Source is the tweet + `nicobailon/pi-subagents` README/docs/workflows.md. Ported
as a skill: the **persona table** (scout/researcher/worker/reviewer/oracle/
delegate), **recommended loops** (clarify→scout→worker→fresh reviewers→worker),
**parallel fan-out** (multiple `agent` calls in one batch), **sequential
phases** (branch on real child output), **async/background** (nur `admission`),
**worktree isolation** for parallel writers (fractal), and **child-boundary
rules** (no re-spawn, focused surface, escalate-don't-guess). Keeps nur's native
`agent`/`admission`/`fractal` as the runtime — no JS DSL bolted on, which is the
careful choice (nur already has the mechanics; the skill supplies the vocabulary).

### MSW Kernel (aienginerd) → `msw-contract` skill
Source is the tweet's embedded kernel. Ported faithfully as an on-demand skill:
the **necessity test** (act only if it closes a claim↔contract gap), **do ; prove;
halt** loop, **fuses** (round limits, reject cross-round claims), and **no
unauthoritative limits** (every cap must cite authority/derivation). An on-demand
skill activation so rigor applies when it matters, not every turn.

### Takumi (kane50613) → `takumi-usage` skill + honest integration note
Deep-studied the Rust engine (JSX/HTML→PNG/SVG/PDF, no headless browser; taffy/
parley/resvg pipeline; `next/og`-compatible). **Honesty decision:** I attempted to
bundle the `takumi` Rust crate as a first-class `render_card` tool, but
`takumi-raster 0.4.11` does **not compile** on our toolchain (upstream bug:
`RenderedImage configured out` in `node_paint.rs`/`background_drawing.rs`; already
at latest version, so not bump-fixable). Rather than ship a broken dependency, I
reverted the Cargo wiring and delivered the integration as a **skill** + a clear
design note. The skill gives the model the full takumi CLI/JS/`next/og` usage
(`takumi-js`, `ImageResponse`, `renderSvg`, animations, Tailwind v4, fonts) plus
the honest "crate path blocked upstream; re-enable when takumi-raster is fixed"
flag. This keeps the build green and doesn't oversell.

---

## Verification

- **666 tests pass / 0 failed** (added the palette-Enter unit test).
- Clean `cargo build --bin nur` (default features, no warnings).
- Cargo.lock clean — no broken takumi remnants.
- All three skills registered in `src/agent/skill_intents.json` (1000 total) with
  proper NL + slash triggers; verified present + activatable.
- New skills follow AGENTS.md: added under `skills/`, regenerated the index with
  `scripts/generate_skill_intents.py`, tested via the skill unit suite.

## Honest note on Takumi

Not wired into the binary because its raster backend fails to compile. That is a
deliberate "no mistakes over breadth" call — a tool that can't build is worse than
a well-documented skill. When `takumi-raster` is patched upstream, the `render_card`
design (in the skill) drops straight in: `from_html` → `render`/`render_svg` →
write to `.nur/media/`, gated behind an optional `takumi` feature.

## Ship status

All in-tree, not yet shipped/released. Rebuilt install is the running binary's
responsibility (rename-swap on ship). Run-level test count + this report are the
deliverable; say the word to ship.

---
name: factory-overnight
description: Fractal-first overnight software factory from HANDOFF.md (Unix preferred).
triggers:
  - factory overnight
  - overnight factory
  - fractal factory
  - run handoff overnight
  - /factory-overnight
---

# Factory overnight (fractal-first)

Locked nur decision: **overnight factory runs prefer fractal** on Unix.

## When fractal is usable (Unix)

1. Ensure factory skills are installed (`factory`, `factory-plan`, `factory-tests`,
   `factory-handoff`, `factory-review`, `factory-explain`) via ecosystem ensure /
   infinite-headcount pack.
2. Produce or locate `HANDOFF.md` (from `/factory-handoff`).
3. Use the **fractal** skill / tool:
   - Distill HANDOFF into NODE.md `## Instructions` + `## Completion Requirements`
   - `fractal node init <name>` with a clear `max-cost` / `max-iters`
   - Confirm with the user, then `fractal(action="node start", node=...)`
4. Warn: fractal disables approval gates - only start trusted tasks.
5. Operator: watch with `fractal(action="node activity")` / `fractal open`.

## When fractal is unusable (Windows host)

Say clearly: fractal is Unix-only (fcntl). For overnight factory on this machine
use WSL/Linux, or as a weaker fallback `nur run` / continuous mode with the
handoff prompt - not the preferred path.

## Do not

- Auto-start fractal without user confirmation of cost/scope
- Use auto-loom unless ffmpeg + Playwright + ElevenLabs are ready

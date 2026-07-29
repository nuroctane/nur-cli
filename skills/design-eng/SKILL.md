---
name: design-eng
description: "Emil Kowalski design-engineering & animation skills. Use for UI polish, motion review, easing/duration decisions, and avoiding animation slop."
---

# Design engineering (Emil Kowalski)

Installed from https://github.com/emilkowalski/skills via Meta ecosystem ensure.

## Skills (load with skill tool when needed)

- **emil-design-eng** — core philosophy, easing tables, review format (Before/After/Why table)
- **review-animations** — strict animation review
- **improve-animations** — codebase audit → prioritized plans in `plans/`
- **animation-vocabulary** — precise motion language for prompts
- **apple-design** — Apple WWDC motion principles for the web

## When to activate

UI work, component polish, motion bugs, "make it feel premium", shadcn/radix animations.

## Quick rules (always-on taste)

- Never animate keyboard-triggered actions used 100×/day
- Prefer `ease-out` custom curves; never `ease-in` for UI entry
- UI animations < 300ms; buttons get `:active { scale(0.97) }`
- Never `scale(0)` — start at ≥0.95 + opacity
- `transition: transform/opacity` only — not `all`, not layout props

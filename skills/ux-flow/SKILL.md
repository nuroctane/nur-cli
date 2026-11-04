---
name: ux-flow
description: Audit a product's user journeys and interface clarity: build a story catalog from the codebase, then critique each journey for simplicity, redundancy, and friction. Use when asked to review UX, map user flows, find friction, or improve a product's interface.
---

# UX flow

Critique from the journeys users actually take, not from a screenshot
of one screen.

1. **Build the story catalog.** From the codebase, list every user
   journey: routes or screens, the steps each journey takes, and the
   states it passes through. A journey that exists in the UI but not in
   the catalog is a journey that will never be audited.
2. **Walk each journey with the real browser.** Drive the journey with
   the `browser` tool (real Chrome session) and `look` for vision, or
   with `terminal_browser` for terminal flows. Note where the journey
   breaks, where it asks twice, and where it hides the next step.
3. **Score friction, per journey.** For each journey, record: simplicity
   (steps to the goal), clarity (does each screen say what it does and
   what happens next), and redundancy (the same information asked twice,
   the same action in two places). Score with evidence: a screenshot,
   a step count, or a file:line, not an impression.
4. **Report ranked friction.** List the journeys by friction, worst
   first, each with the evidence and the smallest change that would
   remove it. Small fixes (copy, ordering, a missing affordance) go in
   the report as directly actionable items; large redesigns go in as
   issues with the journey and the friction score attached.

## Rules

- Do not critique a journey you did not walk. A route list is not a
  walkthrough.
- Do not use a macOS accessibility driver or another harness's browser
  tool. The driver here is nur-cli's `browser` tool plus `look`, and
  `extract_frames` for recorded walks.
- Change nothing until the user picks items. A UX audit reports ranked
  friction; a redesign is a separate request.

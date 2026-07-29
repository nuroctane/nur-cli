---
name: plur
description: "Local-first shared memory for AI agents (engrams + episodes). Use for preferences, corrections, conventions, and session learnings that must persist across tools and sessions."
---

# PLUR — shared agent memory

NurCLI auto-installs `@plur-ai/cli` and provisions `~/.plur/`. Use the **`plur`** tool
(or `/plur` slash command) — do not ask the user to run npm.

## When to use

- User corrects style, architecture, or conventions → `plur(action=learn, …)`
- Start of a task that may reuse past knowledge → `plur(action=inject)` or `recall`
- After fixing an incident → `plur(action=capture)` episode
- Cross-session preferences → always prefer PLUR over ephemeral chat memory

## Actions (via `plur` tool)

| action | purpose |
|--------|---------|
| status | store health + engram counts |
| learn | store a correction / preference / convention |
| recall | hybrid search over engrams |
| inject | select engrams for the current task (token-budgeted) |
| list | list engrams |
| capture | record an episode (what happened when) |
| timeline | query episodes |
| feedback | rate an engram positive/negative |
| forget | retire an engram |
| ingest | extract engrams from free text |

## Rules

- Never store secrets, API keys, tokens, or passwords in engrams.
- Scope project knowledge with `scope` (e.g. `project:nur-cli`); personal prefs → `global`.
- After a user correction, learn it immediately so the next turn benefits.
- PLUR is memory of *assertions*, not a code index — use graphify for code structure.

Upstream: https://github.com/plur-ai/plur · https://plur.ai

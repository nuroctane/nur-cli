# /plan-arbiter

Compare competing agent plans and choose one executable direction.

`/plan-arbiter` is for workflows where two or more agents, often Codex and
Claude Code, each produce a plan and then need a judge to cross-review,
reconcile, and pick what should actually be implemented.

## What It Does

- Collects plans from pasted text, files, sessions, transcripts, PRs, comments,
  or plan links.
- Normalizes each plan into assumptions, scope, files, sequence, and
  verification.
- Reviews plans against each other and against the real task context.
- Chooses a winner, creates a hybrid, or sends the plans back for revision.
- Produces one implementation handoff with rejected alternatives and validation
  gates.

## When To Use It

Use it when a user says things like:

- "Have Codex and Claude each make a plan, then choose one."
- "Review these two plans against each other."
- "Merge the best parts and tell me which agent should execute."
- "Pick the cheaper execution path if quality is comparable."

Skip it when there is only one straightforward plan and no meaningful
alternative to evaluate.

## Output

The skill returns a short decision memo: decision, why, execution plan, borrowed
pieces, rejected ideas, verification, and executor recommendation.

## Install

```sh
npx @agent-native/skills@latest add --skill plan-arbiter
```

---

## License

**GNU General Public License v3.0 (or later)** — see [LICENSE](./LICENSE).

Meta CLI is free software: you may redistribute it and/or modify it under the
terms of the GPL as published by the Free Software Foundation, either version 3
of the License, or (at your option) any later version. It is distributed in the
hope that it will be useful, but **without any warranty**; without even the
implied warranty of merchantability or fitness for a particular purpose.

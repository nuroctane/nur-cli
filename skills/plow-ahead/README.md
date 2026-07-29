# /plow-ahead

Keep working through ordinary ambiguity and finish with a clear decision recap.

`/plow-ahead` is for explicit autonomy: the user does not want the agent to stop
for routine clarification questions. The agent should make reasonable
assumptions, proceed, validate, and explain the decisions it made.

## What It Does

- Converts ordinary questions into stated assumptions.
- Keeps momentum through implementation choices, missing context, and normal
  test failures.
- Stops only for true blockers like credentials, destructive actions,
  irreversible production changes, explicit branch operations, or high-risk
  safety/security uncertainty.
- Uses conservative, reversible, repo-consistent choices.
- Ends with a recap of decisions, changes, validation, and residual risk.

## When To Use It

Use it when a user says things like:

- "Use your best judgment and finish."
- "Do not stop for questions unless truly blocked."
- "Keep going until done."
- "I want this done when I come back."
- "Plow ahead."

Skip it when the user is explicitly asking to brainstorm, compare options, or
wait for approval before implementation.

## Final Recap

The final response should make the autonomous work easy to audit: goal, key
decisions, assumptions, files changed, validation run, and remaining risk.

## Install

```sh
npx @agent-native/skills@latest add --skill plow-ahead
```

---

## License

**GNU General Public License v3.0 (or later)** — see [LICENSE](./LICENSE).

Meta CLI is free software: you may redistribute it and/or modify it under the
terms of the GPL as published by the Free Software Foundation, either version 3
of the License, or (at your option) any later version. It is distributed in the
hope that it will be useful, but **without any warranty**; without even the
implied warranty of merchantability or fitness for a particular purpose.

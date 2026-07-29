# /quick-recap

Make the agent's work state obvious at the end of every response.

`/quick-recap` adds a small red/yellow/green status footer convention. It answers
the thing users otherwise have to infer from a long response: done, pending, or
blocked.

## What It Does

- Adds a single final status line.
- Defines when to use green, yellow, or red.
- Keeps the status user-facing and under 100 characters.
- Installs cleanly into `AGENTS.md` or `CLAUDE.md` through a managed block.

## When To Use It

Use it when you want every completed unit of work to end with a reliable state
signal. It is especially useful in repos where multiple agents, long-running
work, or partial handoffs can make status hard to scan.

## Status Meanings

- Green: the requested coding or work unit is finished.
- Yellow: specific non-routine follow-up remains.
- Red: the agent cannot continue without user input.

## Output Examples

Green means the work unit is done:

```md
🟢 Updated quick recap docs with output examples
```

Yellow means a specific non-routine item remains:

```md
🟡 Code updated, set PROVIDER_WEBHOOK_SECRET before testing webhooks
```

Red means the agent is blocked on user input:

```md
🔴 Need the production API key to continue
```

## Install

```sh
npx @agent-native/skills@latest add --skill quick-recap --update-instructions
```

Use `--update-instructions`; copying the skill alone does not make the footer
automatic.

---

## License

**GNU General Public License v3.0 (or later)** — see [LICENSE](./LICENSE).

Meta CLI is free software: you may redistribute it and/or modify it under the
terms of the GPL as published by the Free Software Foundation, either version 3
of the License, or (at your option) any later version. It is distributed in the
hope that it will be useful, but **without any warranty**; without even the
implied warranty of merchantability or fitness for a particular purpose.

# `/factory-babysit-pr`

Monitor one explicitly authorized pull or merge request. Each pass checks its
live head, review threads, required checks, and mergeability. Fixes, publishing,
replies, approvals, merges, and soak requirements each have separate gates.

Use `/factory-review-prs` for a PR queue and `/factory-watchdog` for stopped
delivery work that needs a verified reminder.

Configure the exact host, repository, PR, authorization, cadence, worktree, and
action policies with `/factory` in `.agent-factory/config.yaml`. See the
[Factory configuration reference](../../docs/factory/configuration.md#workflowspr-babysitting).

## Install

```sh
npx @agent-native/skills@latest add
```

Select **Factory** to preselect its modules, remove any modules you do not
want, then choose the supported client and install scope.

---

## License

**GNU General Public License v3.0 (or later)** — see [LICENSE](./LICENSE).

Meta CLI is free software: you may redistribute it and/or modify it under the
terms of the GPL as published by the Free Software Foundation, either version 3
of the License, or (at your option) any later version. It is distributed in the
hope that it will be useful, but **without any warranty**; without even the
implied warranty of merchantability or fitness for a particular purpose.

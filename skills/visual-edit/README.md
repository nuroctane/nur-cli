# /visual-edit

Open a running local app in the Agent-Native Design surface as URL-backed iframe
screens for visual inspection, route-state exploration, and source-backed edits.

Use `/visual-edit` when a UI needs to be reviewed or changed in context: compare
real routes, inspect responsive states, walk a multi-screen flow, duplicate a
screen for a new URL state, or apply visual changes back through the coding
agent. The canvas uses the app's live routes and local bridge rather than a
copied static HTML snapshot.

For the full workflow, install the skill with the Agent-Native CLI:

```sh
npx @agent-native/core@latest skills add visual-edit
```

The hosted Design MCP connector handles the account-backed open, screen
placement, and source-edit workflow. Public/read-only designs may be viewed
without signing in; creating, saving, or sharing a design still requires an
authenticated account.

---

## License

**GNU General Public License v3.0 (or later)** — see [LICENSE](./LICENSE).

Meta CLI is free software: you may redistribute it and/or modify it under the
terms of the GPL as published by the Free Software Foundation, either version 3
of the License, or (at your option) any later version. It is distributed in the
hope that it will be useful, but **without any warranty**; without even the
implied warranty of merchantability or fitness for a particular purpose.

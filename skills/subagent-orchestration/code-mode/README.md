# Code mode: scripted subagent orchestration

Reusable template for orchestrating nur subagents from plain JavaScript
(pi-subagents "code mode" pattern). One script that can **loop, fan out,
await, branch on real child output, mix parallel and sequential phases, and
isolate every child in its own git worktree**.

## Backends

The template ships with two `dispatch` implementations - pick per task:

- **native** - `nur run -y <prompt>`: one headless native agent turn per child
  (inherits your configured provider/model).
- **omp** - `omp --mode json ... -p <prompt>`: Oh My Pi, LSP-wired edits,
  explicit model. Same CLI shape `src/tools/omp.rs` builds.

Switch by setting `backend: "native" | "omp"` on each task, or set a default at
the top.

## Usage

```bash
cp orchestrate.mjs ./my-orchestration.mjs   # or edit in place
bun orchestrate.mjs                          # node orchestrate.mjs works too
```

Edit the `tasks` array to describe each child (`name`, `prompt`, optional
`cwd`, `backend`, `isolate`). The script:

1. Spins up a git worktree per task marked `isolate: true` (concurrent writers
   never clobber one branch).
2. Runs the **parallel** group with `Promise.all` (fan-out).
3. Branches on the real child results, then runs a **sequential** integration
   phase with every successful branch/worktree and report.
4. Prints a combined report (name, ok, exit status, text) and preserves created
   worktrees for verification. Remove them only after confirming integration.

## Files

- `orchestrate.mjs` - the runnable template (works with bun or node).
- This README.

## Notes

- Verify with `nur run -y "hello"` and `omp --version` before orchestrating.
- OMP writes to the workspace; keep `--approval-mode yolo` to disposable
  worktrees only. Prefer the nur `omp` tool when already inside a session.
- `maxBuffer` is already capped at 10 MB; raise it for very chatty children.

---

## License

**GNU General Public License v3.0 (or later)** — see [LICENSE](./LICENSE).

Meta CLI is free software: you may redistribute it and/or modify it under the
terms of the GPL as published by the Free Software Foundation, either version 3
of the License, or (at your option) any later version. It is distributed in the
hope that it will be useful, but **without any warranty**; without even the
implied warranty of merchantability or fitness for a particular purpose.

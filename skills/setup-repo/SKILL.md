---
name: setup-repo
description: Scaffold a repository's working conventions: issue and pull-request templates, process docs, and CI. Use when setting up a new repo, or when asked to add templates, contribution docs, or CI to an existing one.
---

# Setup repo

Scaffold only what the repo will actually use. A template nobody reads
is clutter, and a CI job nobody maintains is a red badge.

1. **Issue and PR templates.** Short templates that ask for the facts
   the maintainer needs: for issues, reproduction steps, expected vs
   actual, and environment; for PRs, what changed, why, how it was
   tested, and what it does not change. Keep each template under a
   screenful.
2. **Process docs.** `AGENTS.md` or the repo's equivalent: how to build,
   test, and verify, in the commands that actually run here. Do not copy
   another project's process doc: run the commands, then write what
   worked.
3. **CI.** One workflow that runs the repo's real checks (build, lint,
   tests) on push and pull request. Add jobs only when the repo has a
   check to run; a workflow that runs nothing is worse than none, because
   it teaches contributors to ignore CI.
4. **Verify.** Run the CI steps locally where possible, open a template
   issue and PR to check they render, and confirm the docs match the
   commands. Report what was added and what was deliberately left out.

## Rules

- Do not add Claude Code hooks, slash commands, or another harness's
  conventions. Nur-cli hooks live in `hooks.toml`; nur-cli skills live
  in `skills/`. Adapt the template to this harness, not the other way.
- Do not scaffold tests, docs, or CI for a language or stack the repo
  does not use.

---
name: desloppify
description: Multi-language codebase health scanner with a persistent, prioritized cleanup queue. Use when the user explicitly asks to run desloppify, scan for technical debt, get a health score, or create a cleanup plan. Do NOT trigger for general code review, renaming, or fixing individual bugs.
---

# Desloppify

Desloppify is an external Python CLI, detected by `nur ecosystem
ensure` but never installed or managed by nur-cli. Use it for its queue,
not for its detectors alone: nur-cli already has the mechanical half
natively in `slop_scan` (god files, duplication, coupling, layers,
pass-through wrappers) behind the `codebase-slop` skill, and running
both scanners for the same signals is duplicate work.

## When to use which scanner

- General structural audit, or a repo where a native, bounded Rust tool
  is preferred: use `slop_scan` + `codebase-slop` first.
- A persistent cleanup queue across sessions, a health score, or the
  signals `slop_scan` lacks (orphaned/dead code, complexity, naming,
  test-coverage gaps, single-use helpers, signature analysis, and the
  Rust-specific smells: undocumented `unsafe`, `#[allow]` attributes,
  stringly error results, async-locking and drop-safety, feature and
  doctest hygiene, import and public-API hygiene): use desloppify.

## Workflow: scan, plan, execute, rescan

1. **Scan.** `desloppify scan --path .` For CI, use
   `desloppify scan --path . --profile ci --no-badge` (the CI profile
   skips slow and subjective phases), then `desloppify status --json`.
2. **Plan.** Work the living plan, not the whole backlog:
   `desloppify plan triage --stage observe` (then `reflect`, then
   `organize`), `desloppify plan triage --complete`, then
   `desloppify plan queue`, `reorder`, `cluster`, `focus`, or `skip`.
3. **Execute.** `desloppify next` gives the execution queue: the next
   item to fix, not the whole list. Fix that item, with its own test,
   then rescan. The score is designed to resist gaming: widening the
   gap with wontfix entries does not raise it.
4. **Rescan.** After each fix, rescan and let the queue move. State
   persists in `.desloppify/` across sessions. Note the convention
   difference: nur-cli's own state lives in `.nur/`, desloppify's lives
   in `.desloppify/`. Do not move it, and do not treat it as nur-cli
   state.

## Rules

- Do not replace `slop_scan` with desloppify, and do not run both for
  the same signals in one pass. Pick the scanner that fits the job and
  say which one you used.
- Do not vendor desloppify's code into nur-cli. It is an external tool
  with its own license; nur-cli uses it as a separate process via
  `bash`, the same way `secscan` uses external scanners.
- Credentials go through the `secret` tool and `$NAME` only, never
  pasted into a scan, a plan, or a report.
- A desloppify score is not a verdict on a codebase. Report the score
  with the profile used and the signals behind it, or do not report it.

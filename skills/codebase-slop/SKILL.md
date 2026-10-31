---
name: codebase-slop
description: Audit a codebase for structural slop (implicit coupling, layer-first folders, duplication, illegal states, god files, shotgun surgery, layering violations, pass-through wrappers, unvalidated boundaries) and report ranked findings with file:line evidence. Use when asked to find slop, check code quality or structure, or review what recent work left behind. Changes nothing until the user picks items.
---

# Codebase slop audit

Find the structural problems that make a codebase hard to change, prove each with
evidence, and rank them. **Change nothing** until the user picks what to fix.

## Scope

- Default: the working directory's repository.
- "Recent work" / `--since 14d`: limit to files touched in that window
  (`git log --since=14.days --name-only --pretty=format: | sort -u`), and across several
  repositories when the user names them.
- Skip generated, vendored and build output (`target/`, `node_modules/`, `dist/`, `vendor/`,
  lockfiles, snapshots).

## The nine indicators

For each: what it looks like, how to find it, common false positives, the fix pattern.

1. **Implicit coupling**: a module reaches into another's internals, shares mutable
   globals, or depends on call order.
   - Find: files that always change together across module boundaries
     (`git log --no-merges --name-only -n 500`, count co-changing pairs, flag pairs that
     co-change in at least 60% of their commits and at least 5 times); `static mut`,
     process-wide singletons, `pub` fields read across modules.
   - False positives: a type and its tests, a schema and its migration.
   - Fix: name the interface; move the shared state behind it.
2. **Layer-first structure**: three or more sibling folders named after layers
   (`controllers`, `services`, `models`, `repositories`, `handlers`, `utils`, `helpers`, `dto`)
   so one feature is spread over all of them.
   - Find: list directories; check where one feature's files live.
   - False positives: frameworks that require the layout (Rails, Django apps).
   - Fix: group new code by feature; move a feature's slices together when touched.
3. **Duplication**: the same logic written twice instead of shared.
   - Find: `sgrep` for the behavior ("parse the config file", "retry with backoff"), then
     compare hits; normalized line windows of 8+ lines that repeat across files.
   - False positives: deliberate copies across a stability boundary, test fixtures.
   - Fix: extend the existing helper; delete the copy.
4. **Illegal states representable**: flag pairs, stringly-typed enums, optional fields
   that must be set together.
   - Find: structs with two or more `bool`/`Option` fields read together; string
     comparisons against fixed sets (`== "pending"`).
   - False positives: wire formats that must mirror an external schema.
   - Fix: an enum per state; newtypes for validated strings.
5. **God files**: one file owns several responsibilities and keeps growing.
   - Find: largest files by lines and top-level definitions (compare against the repo's
     own distribution, not an absolute number); `git log --follow` growth.
   - False positives: generated code, data tables, exhaustive match dispatch.
   - Fix: split along the seam the definitions already show.
6. **Shotgun surgery**: one conceptual change needs edits in many unrelated places.
   - Find: commits touching 6+ files in 4+ directories with small hunks each.
   - False positives: renames, dependency bumps, formatting passes.
   - Fix: name the missing abstraction those places share.
7. **Layering violations**: dependencies point the wrong way (core imports UI,
   domain imports transport).
   - Find: the project's documented layering (AGENTS.md, architecture docs), then grep
     imports that cross it upward.
   - Fix: invert through an interface owned by the lower layer.
8. **Pass-through wrappers**: functions whose whole body forwards the same arguments to
   another function.
   - Find: single-expression bodies calling another function with the same parameters in
     the same order. Confirm by reading; a wrapper that narrows types or adds a seam for
     tests is not slop.
   - Fix: call the target directly; delete the wrapper.
9. **Unvalidated boundaries**: input from CLI args, HTTP, files, env or model output
   flows inward unparsed, and interior code re-checks it everywhere.
   - Find: `unwrap()`/`as`-casts/`JSON.parse` deep inside on values that came from outside;
     the same validation repeated in several places.
   - Fix: parse once where the data enters, into a type that cannot hold invalid values.

## Method

1. Orient: read AGENTS.md / architecture docs for the documented layout and layering.
   Run the `slop_scan` tool first: it counts what can be counted (god files, duplicated
   blocks, change coupling, shotgun commits, layer-first folders, pass-through wrappers)
   and gives `file:line` candidates to read.
2. Gather evidence per indicator with the commands above (prefer `sgrep` for intent,
   `grep` for exact names, `git log` for history). Read the code before claiming anything.
3. Discard what fails the false-positive checks.
4. Rank by cost of leaving it: how often the area changes (`git log` count) times how much
   each change pays for the slop.

## Report

A table, most costly first:

| # | Indicator | Where (`file:line`) | Evidence | Fix | Effort |
|---|---|---|---|---|---|

Then a short ranked plan. Ask which items to do; do not edit files in this skill.

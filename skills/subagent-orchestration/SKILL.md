---
name: subagent-orchestration
description: Subagent personas + orchestration patterns (port of pi-subagents by @nicopreme). Delegate with the right persona, fan out/sequence with the agent tool, isolate writers with fractal worktrees.
disable-model-invocation: false
---

# Subagent delivery & orchestration (pi-subagents port)

Use the right persona, compose work with the `agent` tool, and isolate writers so
children can't clobber each other. This is the nur port of `pi-subagents`
(orchestration vocabulary + worktree isolation), keeping nur's native `agent`
tool and async admissions instead of a JS runtime.

## Personas (route by need, not by habit)

| Persona | Use it when you want... | Knowing on the child |
|---------|-------------------------|----------------------|
| `scout` | Fast recon: relevant files, entry points, data flow, risks | read-only explore before you understand the code |
| `researcher` | Web/docs research, sources, a concise brief | web_fetch/search, leave it to facts |
| `worker` | Implementation that edits files, validates, escalates instead of guessing | the file-authoring tools; verify before claiming |
| `reviewer` | Code review + small fixes against plan/tests/edge cases/simplicity | a fresh read-only pass |
| `oracle` | A second opinion before acting; challenge assumptions without editing | read-only, no edits |
| `delegate` | A lightweight general child close to the parent session | same surface as parent |

## Recommended loops

- Implementation: **clarify -> scout -> worker -> fresh reviewers -> worker**
- Second opinion: **ask oracle to challenge assumptions before we edit**
- Hard bug: **oracle to investigate before we change anything**
- Review a diff: **reviewer**
- Parallel review: **fan out reviewer for correctness, tests, cleanup**

## Fan out in one batch (parallel)

Issue several `agent` calls in one response — they run concurrently (nur fans
them out). Each is an independent child:

```
agent(description="review correctness", subagent_type="explore", prompt="...")
agent(description="review tests", subagent_type="explore", prompt="...")
```

## Sequence phases (await before branching)

When one child's output feeds the next, run them sequentially — wait for the
result, then act on it:

1. `agent scout` -> read results
2. `agent worker` with the scout findings
3. `agent reviewer` fresh, with the diff

Branch on real child output — pass `scan.output` into the next task string.

## Background / async (nur admissions)

For work that should keep running while you continue, use `agent async=true`
(returns a handle immediately; collect later via `admission get`). Prefer this for
long reviewers or implementations you can proceed in parallel with.

## Worktree isolation for parallel writers (fractal)

When two children both **write**, give each its own worktree so they don't
clobber each other. Nur uses fractal worktrees. Pass cwd/fractal on the child, or
run writers in separate fractal nodes, then merge. Keep ONE writer per location
unless writers are deliberately isolated.

## Child safety (boundaries)

- Children don't re-spawn agents (depth limit). If a child must delegate, it can
  only do so when explicitly allowed; otherwise it completes directly.
- Children get a focused tool surface — they don't inherit the parent's full
  orchestrator role.
- Never let a child guess an unapproved decision; it escalates instead.

## Source

Ported from `pi-subagents` (nicobailon) — the persona table, recommended loops,
parallel/sequence pattern, worktree isolation, and the child-boundary rules.
Keep nur's native `agent`/`admission`/`fractal` as the runtime.

---
name: subagent-orchestration
description: Subagent personas + orchestration patterns (port of pi-subagents by @nicopreme). Delegate with the right persona, fan out/sequence with the agent tool, isolate writers with fractal worktrees. Includes code mode: scripted orchestration in plain JS that loops, fans out, awaits, branches on real child output, mixes parallel/sequential phases, and isolates each child in its own git worktree - for native agents (nur run) and OMP.
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

## Code mode: scripted orchestration (plain JS)

pi-subagents' "code mode" lets you orchestrate subagents from a plain
JavaScript script: loop, fan out, await, branch on real child output, mix
parallel and sequential phases, and isolate each child in its own git worktree
- all in one file. In nur this runs *outside* the model loop (a bun/node
script), but it drives the SAME two delegation backends the model would use:

- **NATIVE** - every delegation is a headless `nur run` (one agent turn,
  prints the final answer). Parallel = `Promise.all` over child processes.
- **OMP** - every delegation is an `omp` tool run (Oh My Pi, LSP-wired).
  Call the `omp` CLI directly the way `src/tools/omp.rs` does, or shell out to
  the nur `omp` tool via `nur` itself. Results come back as JSON events you can
  branch on.

Why script instead of a batched tool block? The script gives real control flow:
`for`/`while` loops over children, `if` on their actual text output, mixing
`await` (sequential phases) with `Promise.all` (parallel fan-out), and a
`worktree` per child so concurrent writers never clobber each other - none of
which is expressible as a single static tool call.

### The reusable template

A working, copy-and-fill-in template ships with this skill at:

```
skills/subagent-orchestration/code-mode/orchestrate.mjs
```

Copy it next to your task, edit the `tasks` array and the `dispatch` function,
then `bun orchestrate.mjs` (or `node orchestrate.mjs`). It already implements:
parallel fan-out, sequential phases, branching on real child text output,
per-child git worktrees, and a combined report. See its header comment and the
README in the same folder.

### Core pattern (native - nur run)

```js
import { execFile } from "node:child_process";
import { promisify } from "node:util";
const run = promisify(execFile);

// dispatch ONE child -> one headless native agent turn
async function dispatch({ name, prompt, cwd }) {
  const { stdout } = await run(
    "nur", ["run", "-y", prompt],
    { cwd, maxBuffer: 10 * 1024 * 1024 },        // big enough for long reports
  );
  return { name, ok: true, text: stdout };
}

// PHASE 1: parallel fan-out (independent writers, each in its own worktree)
const tasks = [ /* fill in: {name, prompt, cwd} */ ];
const parallel = await Promise.all(tasks.map(dispatch));

// PHASE 2: branch on REAL child output, then a sequential dependent phase
const winner = parallel.find((r) => /SUCCESS|DONE/i.test(r.text));
const dependent = winner
  ? await dispatch({ name: "integrate", prompt: `Use ${winner.name}'s result: ${winner.text.slice(0, 2000)}`, cwd: "./" })
  : await dispatch({ name: "fallback", prompt: "Nothing succeeded; reassess and produce a plan." });
```

### Same pattern for OMP (parallel + collect + branch)

OMP returns JSON events (see `src/tools/omp.rs`). The `omp` CLI flags mirror
what the tool builds:

```js
// dispatch ONE child -> one OMP one-shot run (JSON events on stdout)
async function dispatchOmp({ name, prompt }) {
  const args = [
    "--mode", "json", "--no-session", "--no-title",
    "--no-extensions", "--no-skills", "--no-rules",
    "--max-time", "300", "--approval-mode", "yolo",
    "--thinking", "low", "-p", prompt,
  ];
  const { stdout } = await run("omp", args, { maxBuffer: 10 * 1024 * 1024 });
  // pick the final assistant text out of the newline-delimited JSON events
  let output = "";
  for (const line of stdout.split("\n")) {
    try {
      const ev = JSON.parse(line);
      if (ev?.type === "message_end") {
        output = ev.message?.content?.map?.((c) => c.text ?? "").join("") ?? output;
      }
    } catch { /* not JSON - skip */ }
  }
  return { name, ok: !!output, text: output };
}

const results = await Promise.all(tasks.map(dispatchOmp));  // parallel fan-out
const failed = results.filter((r) => !r.ok);
// ... branch on failed.length, re-route, sequence dependent phases
```

> Prefer the nur `omp` **tool** (approval-gated, budget-tracked, cancellation
> aware) when you are already inside a nur session. Call the `omp` CLI directly
> only when the script is fully standalone. OMP writes to the workspace, so use
> a worktree per writer and `--approval-mode yolo` only in a disposable
> environment.

### Worktree isolation per child

Give each *writing* child its own git worktree so parallel writers never
clobber one branch:

```js
import { execFileSync } from "node:child_process";
function worktree(name) {
  const slug = name.toLowerCase().replace(/[^a-z0-9._-]+/g, "-");
  if (!slug) throw new Error("safe worktree name required");
  const dir = `.wt-${slug}`;
  const branch = `wip/${slug}`;
  execFileSync("git", ["worktree", "add", "-b", branch, dir], { stdio: "inherit" });
  return { dir, branch }; // pass dir as cwd; hand branch to the integrator
}
// Preserve writer worktrees until their commits/changes are verified integrated.
// Never auto-remove --force: that can discard uncommitted child work.
```

This mirrors nur/fractal's worktree isolation (`.worktrees`) without needing a
fractal coordinator: keep ONE writer per worktree, then merge or cherry-pick
the branches you accept. The template wires this in with an `isolate: true`
per task.

### Checklist when a model writes code-mode

1. Choose backend per task: native `nur run` (fast, inherits your provider) vs
   `omp` (LSP-wired edits, explicit model) vs mixed.
2. Fan out independent tasks with `Promise.all`; keep dependent phases in
   `await` sequence.
3. Branch on the *actual child text* (`r.text`), never on a guess.
4. Give every concurrent writer its own worktree, pass its branch/path to the
   integration phase, and preserve it until integration is verified.
5. Cap `maxBuffer` and `--max-time`; collect all results, don't fail fast on
   the first error unless you mean to.
6. Report per child: name, ok, files changed, checks run - then a combined
   summary.

## Source

Ported from `pi-subagents` (nicobailon) — the persona table, recommended loops,
parallel/sequence pattern, worktree isolation, and the child-boundary rules.
Keep nur's native `agent`/`admission`/`fractal` as the runtime.

#!/usr/bin/env node
// Code mode: scripted subagent orchestration for nur (pi-subagents pattern).
//
// Orchestrates subagents from plain JS: fan out, await, branch on real child
// output, mix parallel and sequential phases, and isolate each writer in its
// own git worktree - all in one script.
//
// Two backends:
//   native -> `nur run -y <prompt>`  (one headless native agent turn)
//   omp    -> `omp --mode json ... -p <prompt>`  (Oh My Pi, LSP-wired edits)
//
// Run:  bun orchestrate.mjs   (or: node orchestrate.mjs)
// Edit the TASKS array and the phase wiring below for your job.

import { execFile, execFileSync } from "node:child_process";
import { promisify } from "node:util";
const run = promisify(execFile);

const MAX_BUFFER = 10 * 1024 * 1024; // 10 MB per child report

// ---------------------------------------------------------------------------
// 1. Describe each child here.
//    - backend: "native" | "omp"
//    - isolate: true -> spin up a git worktree for this child (concurrent writers)
//    - cwd: override where the child runs (else repo root / its worktree)
// ---------------------------------------------------------------------------
const TASKS = [
  {
    name: "scout",
    backend: "native",
    prompt:
      "Explore this repo as a read-only scout. Report the key files, entry points, " +
      "and any obvious risks in a concise brief. Do not edit anything.",
  },
  {
    name: "worker-a",
    backend: "native",
    isolate: true,
    prompt:
      "Implement feature A described in the task. Make the change, validate it, " +
      "and report files changed + checks run.",
  },
  {
    name: "worker-b",
    backend: "omp",
    isolate: true,
    prompt:
      "Implement feature B described in the task. Make the change, validate it, " +
      "and report files changed + checks run.",
  },
];

// ---------------------------------------------------------------------------
// 2. Dispatch: one child = one headless delegation on the chosen backend.
// ---------------------------------------------------------------------------
async function dispatchNative({ name, prompt, cwd }) {
  const { stdout } = await run(
    "nur",
    ["run", "-y", prompt],
    { cwd: cwd ?? ".", maxBuffer: MAX_BUFFER },
  );
  return { name, backend: "native", ok: true, text: stdout.trim() };
}

async function dispatchOmp({ name, prompt, cwd }) {
  const args = [
    "--mode", "json", "--no-session", "--no-title",
    "--no-extensions", "--no-skills", "--no-rules",
    "--max-time", "300", "--approval-mode", "yolo",
    "--thinking", "low", "-p", prompt,
  ];
  const { stdout } = await run("omp", args, { cwd: cwd ?? ".", maxBuffer: MAX_BUFFER });
  // Pull the final assistant text out of the newline-delimited JSON events.
  let output = "";
  for (const line of stdout.split("\n")) {
    try {
      const ev = JSON.parse(line);
      if (ev?.type === "message_end") {
        const text = ev.message?.content?.map?.((c) => c.text ?? "").join("") ?? "";
        if (text) output = text;
      }
    } catch {
      /* not JSON - skip */
    }
  }
  return { name, backend: "omp", ok: !!output, text: output.trim() };
}

function dispatch(task) {
  return task.backend === "omp" ? dispatchOmp(task) : dispatchNative(task);
}

// ---------------------------------------------------------------------------
// 3. Worktree isolation for concurrent writers.
// ---------------------------------------------------------------------------
function worktree(name) {
  const slug = name.toLowerCase().replace(/[^a-z0-9._-]+/g, "-").replace(/^-+|-+$/g, "");
  if (!slug) throw new Error(`Cannot derive a safe worktree name from ${JSON.stringify(name)}`);
  const dir = `.wt-${slug}`;
  const branch = `wip/${slug}`;
  // Argument-array invocation avoids shell interpolation from task names.
  execFileSync("git", ["worktree", "add", "-b", branch, dir], { stdio: "inherit" });
  return { dir, branch };
}

function plan(name) {
  const base = { cwd: ".", worktreeDir: null, branch: null };
  const t = TASKS.find((x) => x.name === name) ?? {};
  if (t.isolate) {
    const isolated = worktree(name);
    base.worktreeDir = isolated.dir;
    base.branch = isolated.branch;
    base.cwd = isolated.dir;
  } else if (t.cwd) {
    base.cwd = t.cwd;
  }
  return base;
}

// ---------------------------------------------------------------------------
// 4. Orchestrate.
// ---------------------------------------------------------------------------
async function main() {
  const created = [];
  const wrap = (t) => {
    const p = plan(t.name);
    if (p.worktreeDir) created.push(p.worktreeDir);
    return { ...t, cwd: p.cwd, worktreeDir: p.worktreeDir, branch: p.branch };
  };
  const planned = TASKS.map(wrap);

  console.log(`\n=== Phase 1: parallel fan-out (${planned.length} children) ===`);
  const parallel = await Promise.all(planned.map((t) =>
    dispatch(t).then((r) => ({
      ...r,
      cwd: t.cwd,
      worktreeDir: t.worktreeDir,
      branch: t.branch,
    })).catch((e) => ({
      name: t.name, backend: t.backend, ok: false, text: String(e.message ?? e),
    })),
  ));

  // 4a. Branch on REAL child output. All successful writers remain candidates;
  // do not throw away an independent feature merely because another report used
  // the word "DONE" first.
  const good = parallel.filter((r) => r.ok);

  // 4b. Sequential dependent phase receives concrete branches + reports.
  console.log("\n=== Phase 2: sequential dependent phase ===");
  const handoff = good.map((r) =>
    `CHILD ${r.name}\nbranch: ${r.branch ?? "(root/read-only)"}\nworktree: ${r.worktreeDir ?? r.cwd ?? "."}\nreport:\n${r.text.slice(0, 3000)}`,
  ).join("\n\n");
  const dependent = good.length
    ? await dispatch({
        name: "integrate",
        backend: "native",
        cwd: ".",
        prompt:
          "Review every successful child result below. Inspect each branch/worktree; " +
          "integrate only accepted commits or changes, resolve conflicts, run the full test suite, " +
          "and report exactly what was integrated. Do not claim integration from report text alone.\n\n" +
          handoff,
      }).catch((e) => ({ name: "integrate", backend: "native", ok: false, text: String(e.message ?? e) }))
    : { name: "integrate", backend: "native", ok: false, text: "No child succeeded." };

  // 4c. Combined report.
  console.log("\n=== Combined report ===");
  for (const r of [...parallel, dependent]) {
    console.log(`\n[${r.ok ? "OK" : "FAIL"}] ${r.name} (${r.backend})`);
    console.log(r.text.slice(0, 1200));
  }
  const okCount = [...parallel, dependent].filter((r) => r.ok).length;
  console.log(`\nSucceeded: ${okCount}/${parallel.length + 1}`);

  // Preserve isolated worktrees. Force-removing them here would silently discard
  // uncommitted child work before a human can verify integration.
  if (created.length) {
    console.log("\nPreserved worktrees (remove only after verifying integration):");
    for (const dir of created) console.log(`  ${dir}`);
  }
}

main().catch((e) => {
  console.error("Orchestration failed:", e);
  process.exit(1);
});

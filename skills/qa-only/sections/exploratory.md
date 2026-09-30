<!-- AUTO-GENERATED from exploratory.md.tmpl — do not edit directly -->
<!-- Regenerate: bun run gen:skill-docs -->
# Shared exploratory QA

The **caller** (/qa, /qa-only, /review or /ship) owns decisions, tests, fixes and publication. Discovery writes only reports/evidence
and owned fixture state; no workflows, framework installs or publication.

## 0. Preparation gate

Complete these Reads in order before writing charters or probing:
1. Read `sections/scope.md` relative to the installed `qa`/`gstack-qa` SKILL.md directory in full and select the surfaces.
2. Read the selected surface methods below in full.

Use this host's installed `qa`/`gstack-qa` SKILL.md directory for these reads:

**Functional surfaces:**
Read `sections/system-functional.md` in full.

**Browser surfaces only:**
Read `sections/qa-patterns.md` in full.

Await each successful Read result before continuing. A supplied target, isolation
description, section index or remembered method is not a completed instruction Read.
Do not repeat a Read already completed in this invocation; reuse only its acknowledged
full contents. If either required Read is missing, complete it now before Charter and preflight.
Missing or unreadable assets, prerequisites or permission block affected probes, not independent safe checks. Report QA setup blockers.

## 1. Charter and preflight

Reuse resolved REPORT_DIR; otherwise own a fresh `.gstack/qa-reports` subdirectory.
Write a **charter** per behavior: contract, risk, entrypoint, isolation, exit condition, source, commands and inputs. Save charters as Markdown in the report.


For /qa and /qa-only:
- Browser Quick: SECONDS=30. Browser Full/Regression: SECONDS=900.
- Functional Full, Quick and Regression have no default total timer.
Set SECONDS to the shorter mode/caller limit; an unlimited mode uses the caller's bound.
Without a total time limit, do not start D; announce finite command timeouts.
Stop when scoped contracts are tested or blocked.
Clocks/checkpoints use REPORT_DIR; mixed standalone runs use REPORT_DIR/browser and REPORT_DIR/functional, with one final report at REPORT_DIR. Caller paths win.
R = owned probe directory; D = R/deadline.json. Quote paths.
G = `$HOME/.claude/skills/gstack/bin/gstack-qa-deadline`; Q = `$HOME/.claude/skills/gstack/bin/gstack-qa-evidence`.
Start once before baseline: `bun G start D SECONDS [EARLIER_UTC]` if bounded.
EARLIER_UTC = caller's absolute deadline, if set.
Functional: `bun Q capture R NNN [--public] --deadline D -- COMMAND ARGS`.
Unbounded: use `--timeout-ms MS` instead. Use fresh three-digit IDs.
--public requires approved public/synthetic output; Q screens credentials. For complete private captures, await a safe Read of `R/.qa-evidence/NNN/observation.json`. Sensitive/incomplete captures cannot anchor checkpoints.
Bounded browsers: `bun G run D -- COMMAND ARGS`. No detached probes.
Never reset D/bypass G. Expiry or invalid/missing D stops probes; report unfinished coverage. QA_DEADLINE receipts are not observations.

## 2. Probe loop

Each probe is one native command/interaction plus checks, excluding bookkeeping.
Never batch probes.

1. First demonstrate success: output AND durable effects. Guard if bounded; await completion.
2. **Decide whether another probe is needed.** If bounded, run `bun G status D`.
   If expired or no safe next probe remains, STOP exploration; write the report, not a checkpoint.
   **Classify the last result before copying it.** For public or synthetic observations,
   retain the entire result unchanged, including owned fixture paths, IDs, hashes and
   existing credential placeholders. An absolute state path is not itself a secret.
   For actual secrets/private payloads, withhold those values and disclose the redaction
   and replay limits in the report. If no safe exact observation can be retained,
   stop the affected probe chain; never invent a substitute path, identity or state.
   **Publish before probing.** Create `exploration-NNN.json` in the probe directory, beside its deadline if bounded, with exactly four top-level fields:
   observationCommand: last completed probe's full outer command, including guard.
   observed: its exact decoded child JSON (no wrapper/extra keys), or its full non-JSON text.
   For guarded text, copy the complete span between the guard's started and finished receipt lines.
   Keep its whitespace and content fences verbatim. Do not summarize, relabel or add timing text.
   The guard adds one newline before its finished receipt; that separator is not child text.
   For unguarded text, copy the complete result instead.
   If capture is incomplete, report that limit instead of reconstructing it.
   hypothesis: why nextCommand. nextCommand: exact command/request, guarded if bounded.
   Preserve every safe program-JSON key/value and identity hash unchanged.
   Withhold unsafe values, disclose limits and stop that chain.
   Check fields before publication. No drafts/placeholders or invented safe-path redactions; corrections cannot repair published notes.
   Functional: `bun Q checkpoint R NNN CAPTURE_ID 'observationCommand' 'hypothesis' 'nextCommand'` with literal arguments. Q supplies observed; never transcribe it.
   Browser checkpoints use Write.
   Wait for successful checkpoint publication before dispatch.
   Never backfill or overwrite notes.
3. Run that exact probe; G enforces the deadline when bounded.
   Report refusals as not-run; retain initial state/inputs/results. Repeat from step 2.
4. Replay the exact failing command/request from the same initial fixture state via steps 2–3 (same native command, fresh capture ID)
   to confirm it, then minimize via those gates. Expiry leaves confirmation/minimization incomplete.
   Another input or a regression test is not that replay.
5. If the user or another process changes source, commands or fixtures, review the affected
   contracts and return to step 2 for each affected revalidation. Do not make product changes yourself.
   Keep the original limits/notes; update outcomes only from fresh evidence.

## 3. Parent handoff

Never change product code, tests, configuration, dependencies or Git through any tool,
including shell, rename, deletion, commit, stash or edit-then-restore. Return test_stub proposals
with their failing contract and expected assertion; never create tests or freeze buggy output.

## 4. Final report

Use the surface report template; link each checkpoint. Separate browser scores, functional outcomes and proposed/executed tests.
For evidence.json, Write R/annotations.json: {revision, runtime, cwd, evidence: [{capture, command, contract, expected, classification}], learning: [checkpoint IDs], limits}.
Run `bun Q materialize R annotations.json` before Markdown; Q fills observed/learning, not classifications. Retain all safe probes, including failures/replays; disclose withheld/incomplete evidence.
Evidence is invocation-local.
Missing prerequisites/expectations/observations, timeouts and refusal never pass.
Pass requires all required current-input contracts to pass with no required remainder.
Report blocked, inconclusive and not-run coverage without claiming success.

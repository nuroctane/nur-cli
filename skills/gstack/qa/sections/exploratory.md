<!-- AUTO-GENERATED from exploratory.md.tmpl — do not edit directly -->
<!-- Regenerate: bun run gen:skill-docs -->
# Shared exploratory QA

The **caller** (/qa, /qa-only, /review or /ship) owns decisions, tests, fixes and publication. Discovery writes only reports/evidence
and owned fixture state; no workflows, framework installs or publication.

Complete these Reads in order before writing charters or probing. Do not repeat a Read already completed in this invocation.
1. Read `sections/scope.md` relative to the installed `qa`/`gstack-qa` SKILL.md directory in full and select the surfaces.
2. Read the selected surface methods below in full.

**Functional surfaces:**
Read `sections/system-functional.md` in full.

**Browser surfaces only:**
Read `sections/qa-patterns.md` in full.

Missing or unreadable assets, prerequisites or permission block affected probes, not independent safe checks. Report QA setup blockers.

## 1. Charter and preflight

Reuse resolved REPORT_DIR; otherwise own a fresh `.gstack/qa-reports` subdirectory.
Write a **charter** per behavior: contract, risk, entrypoint, isolation, exit condition, source, commands and inputs. Save charters as Markdown in the report.

For /review and /ship, no plan/server is required.
Stop after 5 minutes or 12 probes, whichever comes first (SECONDS=300 across surfaces).
Explicit plan checks remain required beyond this smoke budget.
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
   **Publish before probing.** Create `exploration-NNN.json` in the probe directory, beside its deadline if bounded, with exactly four top-level fields:
   observationCommand: last completed probe's full outer command, including guard.
   observed: its exact decoded child JSON (no wrapper/extra keys), or its full non-JSON text.
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
   before repair, then minimize via those gates. Expiry leaves confirmation/minimization incomplete.
   Another input or a regression test is not that replay.
5. After source/commands/fixtures change, repeat affected review and return to step 2 for each affected revalidation. Keep limits/notes; status requires fresh evidence.

## 3. Parent handoff

- **/qa:** parent owns severity, root-cause and Phase 8 regression gates before verified repair.
- **/review:** return before Fix-First; test_stub proposals require ASK approval.
- **Planning:** propose charters only; no execution.

Choose the smallest native test: unit for logic, integration for state/requests; E2E only if smaller tests miss the journey, not automatically both.
Mock only unrelated services.
Never freeze buggy output, weaken tests or delete valid red tests.

## 4. Final report

Use the surface report template; link each checkpoint. Separate browser scores, functional outcomes and proposed/executed tests.
For evidence.json, Write R/annotations.json: {revision, runtime, cwd, evidence: [{capture, command, contract, expected, classification}], learning: [checkpoint IDs], limits}.
Run `bun Q materialize R annotations.json` before Markdown; Q fills observed/learning, not classifications. Retain all safe probes, including failures/replays; disclose withheld/incomplete evidence.
Evidence is invocation-local; /ship reruns once per invocation.
Missing prerequisites/expectations/observations, timeouts and refusal never pass.
Pass requires all required current-input contracts to pass with no required remainder.
Required failure leaves /review incomplete and /ship blocked unless the user explicitly accepts that named risk; noninteractive runs return blocked. Only nonbehavioral diffs may be not applicable (give a reason); prompts/templates are behavioral.

<!-- AUTO-GENERATED from reporting.md.tmpl — do not edit directly -->
<!-- Regenerate: bun run gen:skill-docs -->
# Finalize a report from retained evidence

Complete these steps before the final report Write. They use retained results, not
new probes. Missing evidence stays unknown; an expired clock stays expired.
The caller's write boundary includes reports, learning notes and automatic memory.
Keep all of them in authorized destinations; a memory feature grants no extra path.

## 1. Establish each finding once

Ground the report and learnings in retained observations. For every finding, distinguish
the observed result, the expected contract and any untested causal hypothesis. Link the
supporting command/result or screenshot; unknown impact remains unknown. A console error
message does not establish an uncaught exception, failed payload or missing UI. Missing
text in a page-text extract does not establish an absent attribute or inaccessible element.
Verify those claims with an appropriate probe, or leave them unconfirmed when time expires.

Give each finding one ID and write its Observed, Expected, Evidence and Confirmation
fields first. A logged exception-shaped string proves a logged message, not that the
named operation executed. Keep possible causes in a separate Hypothesis field; omit
speculation that does not help the next investigation. Observed-once is not replay-confirmed.

## 2. Fill timing fields from their actual boundaries

Report **Probe budget** (configured limit) and **Guarded command time** (sum of measured
child spans). Measure guard start to child launch as pre-launch elapsed time, and child
start to finish as command duration, not a component's latency without its own measurement.
A deadline window is not total run time. Gaps between receipts do not measure
status/Write overhead or prove how many probes fit; if late, say only that this run
dispatched its follow-up after the deadline.

Use **Total session elapsed**: `unmeasured` for the invocation whose report is being written.
Its final report Write, acknowledgement and cleanup are not finished yet. Do not fetch
a clock merely to fill that field. An optional **Measured interval** must cite its actual
start/end receipts and name the work outside those boundaries, including later report
Writes and cleanup; it is never a completed-session measurement.

## 3. Assemble and check every repetition before writing

Use the caller's selected surface templates and assembly rules. Build headlines,
Top 3, summaries and completion text from each finding's Observed and Confirmation
fields, not its Hypothesis. Choose one conservative factual sentence per finding and
reuse it verbatim in those locations; do not introduce a new causal paraphrase.
A disclaimer in the detail does not qualify a stronger claim elsewhere.

Proposed regression assertions must detect the original observation on its actual
channel. For a logged console error, capture console errors; exception-only hooks
do not detect a console-only message. Additional causal tests remain separate proposals.
Apply these evidence limits to proposed tests and learnings too.

Before the final Write, check every mention of each finding against its evidence
fields, every proposed test against the observed channel, and each timing claim against
its named boundaries. Remove unsupported claims from all sections, not only the detail.
Keep refused/unstarted probes and untested categories explicit. Write the report only
after this consistency check; do not repair an evidence gap with invented facts.
Check claims about frequency and executed checks against the actual commands/results:
one observation proves neither recurrence nor an unexecuted check. Apply the same
evidence limits to the final response and any caller-authorized learning note.

## 4. Capture permitted notes, then write the report

Run the learning step below only if its destination is caller-authorized:
the user or invoking workflow explicitly permitted that learning-store path.
Invoking /qa-only alone does not grant this permission. Otherwise
keep notes in `REPORT_FILE`; do not write learning stores or automatic memory.

**No explicit permission:** skip learning-store writes and continue to the report.

**Explicit permission:** Read the named store first. Preserve its existing contents
and use the permitted write tool to append a verified note in that store's format.
If the format or write interface is unavailable, keep the note in the report instead.
Do not run logging helpers: they may write caches or enqueue synchronization outside
the permitted path. This branch never changes configuration or enables synchronization.

Write the checked report to the entrypoint's permitted destinations.
After the final Write, respond briefly with its path and verified coverage/limits;
do not append new findings or timing explanations.

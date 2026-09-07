# Dedup and completeness gates

The unglamorous half of a defensible report: nothing real gets dropped, nothing gets double-counted.

## group_key dedup

- Every finding gets `group_key = Contract | function | bug_class`.
- Merge synonymous bug-class names before grouping.
- Surviving merged findings annotate how many passes flagged them (`[passes: N]`).
- **Function isolation (hard rule)**: never merge findings across different functions, even when the root cause text looks identical - the same defect in `deposit` and `mint` is two findings.

## Wide descriptions (multiple bugs per group key)

One function can host several coexisting bugs under the same group key. All distinct mechanisms survive; none is absorbed. Test: if two findings need two DIFFERENT fixes, they are two findings.

## Second pass over hot functions

Any function with 2+ findings gets one extra full-body scan so every distinct mechanism present appears in at least one final finding. This is the cheapest completeness insurance there is - one targeted re-read, not a re-audit.

## Fix-preservation gate

Collect raw fixes from all findings; group by added lines. If 2+ fixes for the same root cause are structurally distinct (different expression, check direction, or parameter), present them verbatim as labeled Options - typical families: validate / restrict / allow-and-handle / ban-path. Do not editorialize fixes into one "best" version; the protocol team owns the tradeoff.

## Completeness gate (silent-drop check)

Every unique `(Contract, function)` pair that appeared in ANY working note must appear in the final report - as a finding, a lead, or an explicit reviewed-clean entry. A missing pair is a silent drop and counts as an audit failure. Print the coverage line (pairs covered / pairs discovered) directly above the report.

## Working discipline that feeds these gates

- Notes are structured from the start: `contract | function | bug_class | root cause | proof | fix` - retrofitting structure at report time is where drops happen.
- Weaponize every confirmed bug: grep the name and code pattern across ALL contracts; a missed repeat of a confirmed bug is the classic audit failure.
- Escalate every finding to its worst exploitable variant (a DoS that conceals fund theft is reported as the fund theft), then attack sibling branches of the same flaw.

## Relation to detector triage

These gates govern human/lens findings. Static-analyzer noise follows its own pipeline: `triaging-and-deduping-findings` (and its corpus-calibration reference). Do not mix the two streams in one report section.

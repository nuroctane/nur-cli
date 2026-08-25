---
name: triaging-and-deduping-findings
description: "Dedupe and rank Slither/Aderyn/Mythril/fuzz hits: (file, line), SWC, exploitability x funds, false positives, known issues. Use before writing a contest report."
---

# Triaging and deduping findings

A 200-line Slither dump is not research. This is the filter.

## Procedure

1. Combine Slither + Aderyn + Mythril + invariant failures + manual notes.
2. Key: `(file, function, root cause)` not every detector name.
3. Drop: informational, style, known issues from the contest README, issues in `lib/`.
4. Rank: exploitability x funds at risk. Critical/High block a deploy recommendation.
5. Map SWC + Solodit analog + bug class (`bug-classes.md`).
6. Require a local Foundry test for High/Critical. No test = not ready to file.
7. One report per root cause (`contest-and-bounty-reporting`).

PASS/FAIL for pre-deploy still lives on `auditing-foundry-smart-contract-security`.

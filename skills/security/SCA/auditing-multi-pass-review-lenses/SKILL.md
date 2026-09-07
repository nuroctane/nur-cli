---
name: auditing-multi-pass-review-lenses
description: "Multi-pass Solidity audit methodology distilled from senior-auditor practice: nine review lenses plus three gap-hunter seam lenses, the Feynman/Socratic/Inversion mental tools, the finding-vs-lead standard, four-gate severity judging with confidence scoring, dedup and completeness gates, and safe-pattern/do-not-report lists. Runs single-session and sequential by design - no subagent swarms, no parallel fan-out. Use for a structured full-scope review of in-scope Solidity. Load one reference slice."
---

# Auditing with multi-pass review lenses

A complete review methodology for in-scope Solidity: pass over the codebase through named lenses, promote leads through hard judging gates, dedup with coverage guarantees, and emit a defensible report. Distilled from published senior-auditor skill work (pashov/skills solidity-auditor; PlamenTSV/plamen methodology) with the orchestration machinery deliberately removed.

## Single-session discipline (non-negotiable)

The source material orchestrates 12-100 parallel subagents. This skill does NOT. Every lens runs **sequentially in this one session, in one context**:

- Never spawn a subagent per lens, per file, or per phase. No fan-out, no worker pools, no parallel background audits.
- One full read of each file; annotate rather than re-read. Cross-file checks use targeted greps, not fresh full reads.
- If scope is huge, cut lens count before cutting session discipline: prefer lenses 1-4 + 10-12 on a full pass over all 12 lenses run through a swarm.
- Token spend stays proportional to code size: budget roughly one focused re-read per gap-hunter hit, nothing else.

## The passes

| Pass | Lens | Looks for |
|------|------|-----------|
| 0 | Discover + scope | Enumerate in-scope files; exclude tests/mocks/interfaces/libs; resolve shared refs; note entry points |
| 1 | Math precision | Rounding direction, precision loss, mulDiv edges, scaling mismatches, share/rate math |
| 2 | Access control | Every state changer's caller assumption; missing/tautological guards; init paths; role graphs |
| 3 | Economic security | Value flows vs incentives; extractable combinations; fee/reward accounting; oracle-priced actions |
| 4 | Execution trace | Real call sequences end-to-end; state deltas per step; check-then-effect ordering |
| 5 | Invariant | What must always be true; every write site that could break it (guard-lifting) |
| 6 | Periphery | Integration edges: tokens, oracles, routers, receivers, callbacks - the code you did not write |
| 7 | First principles | "What is this actually for?" - design-level flaws invisible line-by-line |
| 8 | Asymmetry | Who can cause harm vs who bears it; privilege/impact imbalance; grief asymmetries |
| 9 | Boundary | Zero/max/empty/full states; first depositor; empty markets; overflows at limits; paused/frozen paths |
| 10 | NUMERICAL GAP (seam) | Lens 1 x lens 9: precision bugs that only fire at boundaries |
| 11 | TRUST GAP (seam) | Lens 2 x lens 6: trust assumptions between your code and periphery that neither side enforces |
| 12 | FLOW GAP (seam) | Lenses 3 x 4: value-flow combinations across functions that no single function shows |

Three mental tools run continuously during ALL reading (annotate with `[F]`, `[S]`, `[I]` markers as you go):

- **Feynman**: explain each new function in plain English; the spot where your explanation goes fuzzy is a bug location.
- **Socratic**: ask "why?" of unclear lines until you reach the implicit assumption the code relies on - that assumption is the bug candidate.
- **Inversion**: for any path that looks clean, write three concrete attacker moves (specific values, specific states) that try to break it.

## What each reference holds

| File | Contents |
|------|----------|
| `references/finding-standard-and-judging.md` | Finding vs LEAD, the four judging gates, confidence scoring, safe patterns, do-not-report list |
| `references/dedup-and-completeness.md` | group_key dedup, function isolation, fix preservation, coverage gate, finding chains |
| `references/senior-sop.md` | Working SOP: read protocol, weaponize patterns across contracts, escalate to worst variant |

Next after findings: `writing-foundry-invariant-handlers` for regression tests, `contest-and-bounty-reporting` for writeups, `triaging-and-deduping-findings` for detector-noise merges.

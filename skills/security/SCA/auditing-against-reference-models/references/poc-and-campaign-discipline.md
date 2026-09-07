# PoC gate, failure triage, and campaign discipline

## The forced-PoC gate (material findings must run)

A finding claiming concrete material harm MUST attempt an executable proof-of-concept before it ships - unless a closed set of code-grounded blockers applies:

1. State unreachable without live dependencies that cannot be mocked faithfully (say which and why).
2. PoC would require broadcasting txs (forbidden - local only; a fork test is the substitute).
3. The finding is informational/design-level with no executable behavior to demonstrate.

Blockers are stated IN the report next to the finding - an unstated blocker is a demotion. Everything else: write the Foundry test, pin the fork block, run it, attach the failing output. "Trust me, it reverts" is a LEAD.

Devil's-advocate pass before shipping: attack your own PoC - would a skeptical reviewer find a hidden guard you missed, a mocked dependency that lies, a tolerance that hides the theft? One self-review iteration, then ship.

## Failure triage (when tests/fuzzing fail)

| Failure class | First interpretation | Action |
|---------------|---------------------|--------|
| Property fails on tiny amounts/zero states | Boundary modeling gap in the harness | Fix harness; re-run before celebrating |
| Property fails only under specific sequence | Real candidate; minimize the sequence (shrinking) | Minimize -> judge via four gates |
| Invariant fails after N calls, un-minimizable | State leakage across handlers - ghost/snapshot bug OR real cross-function bug (T33 class) | Bisect handlers; check ghost update placement |
| Model/target divergence where BOTH look wrong | Spec ambiguity | Document as design finding + spec-gap note |
| Everything passes but coverage is thin | The suite is quiet, not safe | Report coverage honestly; add targeted boundary properties |

Campaign sizing: fixed seed set for reproducibility + time-boxed exploration; record seeds, corpus dir, and fuzzer version with results. Long-running campaigns belong to `fuzzing-with-echidna-and-medusa`; this skill's campaigns are audit-scoped and bounded.

## Test-campaign discipline (report-facing)

1. Every claim of "tested" cites: file, test name, seed, command, result.
2. Unfuzzed areas are listed as untested - not omitted (silent coverage gaps are the audit's own silent drops).
3. Findings verified by PoC get the PoC path in the report; findings blocked from PoC get their blocker text.
4. Post-audit: hand the harness to the protocol team - a fuzz suite that dies with the audit report wasted half its value.

## Anti-swarm reminder

All of the above is one engineer, one session, sequential. If you catch yourself planning "one worker per function" or "parallel model branches", stop: that is the orchestration pattern this skill family deliberately excludes (token-spend discipline; see AGENTS.md working style on swarm approval).

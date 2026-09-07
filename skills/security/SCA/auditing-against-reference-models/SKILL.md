---
name: auditing-against-reference-models
description: "Model-based audit technique: build an independent reference model of the protocol and diff the real code against it (differential testing), derive spec properties, mirror fuzz scenarios across implementations, triage failures, and run forced PoC verification for material findings. Distilled from foundry test-suite skills and audit-agent methodology; single-session by design - no worker pools, no agent swarms. Use when a spec or reference implementation exists, or when findings need executable proof. Load one reference slice."
---

# Auditing against reference models

Instead of reading code and hoping, build a second, simple implementation of what the protocol SHOULD do, then diff reality against the model. Divergence = finding candidate; convergence = coverage. Distilled from published foundry skill suites (aviggiano/security) and audit-agent PoC-gate methodology (PlamenTSV/plamen), orchestration removed.

## Choose your file

| You want | Load |
|----------|------|
| Reference model construction + differential testing | `references/reference-model-and-differential.md` |
| PoC gate, failure triage, campaign discipline | `references/poc-and-campaign-discipline.md` |

## Single-session discipline

One model, one session, sequential diffs. No per-function subagent tests, no worker pools, no model-per-parallel-branch. A reference model that does not fit in one head does not fit in one session - shrink the model to the value-critical core first.

## When this is the right tool

- A spec, reference implementation, or prior version exists (fork deltas, "inspired by Compound/Aave/Uniswap").
- The protocol's math is simple but its enforcement is spread out (fees, caps, ratios).
- Findings need executable proof to survive judging (the PoC gate below).

## Pipeline

1. Pick the value-critical core (deposit/withdraw/mint/burn/liquidate/swap paths + accounting).
2. Write the reference model: simplest correct implementation, no gas tricks, no upgradeability, no periphery - spec-faithful.
3. Run `references/reference-model-and-differential.md`: property mirrors, differential diffs, fork-mirrors if a prior version exists.
4. Every divergence -> is the model wrong (spec ambiguity) or the code wrong (finding)? Resolve via `finding-standard-and-judging.md` gates in `auditing-multi-pass-review-lenses`.
5. Material findings get the forced-PoC treatment (`references/poc-and-campaign-discipline.md`) - a finding without an executable proof demotes to LEAD.

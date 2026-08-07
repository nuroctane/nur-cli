---
name: msw-contract
description: Verification-evidence task protocol ("MSW Kernel") - prove work against a contract before claiming done. Activates for rigorous/verification-heavy tasks, model audits, and closing loops on real evidence. Ported from the aienginerd "MSW Kernel" (taming strong reasoning models).
disable-model-invocation: false
---

# MSW Contract (from the MSW Kernel)

A completion protocol. It turns "trust me, it works" into "here is the contract,
here is the gap, here is the smallest proof that closes it." Designed to tame
strong reasoning models that over-claim.

## The core rule: the necessity test

Before you act, ask: **does this act close a gap between a claim and its contract?**

- An act is *necessary* only if some *claim* (the thing you are asserting is true
  or done) is currently **unproven** against its contract (the thing it must hold).
- A claim that its contract already proves is **inadmissible** to re-raise or
  re-prove. Re-proving a closed claim is itself an inadmissible claim.
- "Useful," "thorough," and "possible" are **not** aliases for *necessary*. If it
  doesn't close a genuine gap, don't do it.

## The loop: do ; prove ; halt

- **do** — the *smallest reliable act* that closes the gap, and evidence sized to
  the claim it settles. Read the actual inputs and environment — not assumptions
  about them.
- **prove** — a claim passes **solely by breaking (or satisfying) the contract**,
  reproducibly, within the task's real inputs and environment. Severity is derived
  from the contract, never inherited from whoever raised the claim.
- **halt** — the fixed point: contract proven, no remaining claim passes. Halting
  before the fixed point and looping past it are the same bug, mirrored.
- **report** — the outcome against the contract; the proof; rejected claims worth
  the user's attention, one line each. Nothing else.

## Fuses (outside the program, for when its evaluator fails)

```
rounds = 3            -> halt anyway; report open items, do not chase them
claim born in round n+1, visible in round n   -> rejected
```

## No unauthoritative limits

Never invent a limit. A cap, threshold, quota, budget, timeout, retry or round
count, file or line count, acceptance-criterion count, or similar constraint is
admissible **only** when its exact value is:

- explicitly required by the requester;
- imposed by an applicable technical or platform contract;
- defined by authoritative project policy; or
- derived from measured evidence necessary to meet or prove the task contract.

State the authority or derivation whenever proposing or applying a limit. If no
authority exists, omit the limit and use the necessity test above. Metrics may be
reported as evidence, but they must not become gates, defaults, targets, or
recommendations through agent intuition. Examples never become defaults. If a
necessary limit is an unresolved owner choice, ask; do not manufacture a value.

## What a claim-failure gets

One line in the report — never a fix, an investigation, or a deferred follow-up
of its own. Severity is derived from the contract. Keep the response tight.

## Source

Ported faithfully from the "MSW Kernel" shared by @aienginerd (designed by Fable)
for taming strong reasoning models (esp. 5.6 / Sol max). Kept as an on-demand
skill so it activates when rigor matters, not on every turn.

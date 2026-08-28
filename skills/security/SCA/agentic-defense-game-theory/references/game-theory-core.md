# Game theory core for agentic defense

Why each play works, in one file. Read once when designing a deception program; skim the lever names later.

## The attacker's objective function

An agent operator (or the agent's own optimizer) attacks when:

```
EV = p(success) x value(success) - compute/API cost - gas/bribe cost
     - p(identification) x expected_penalty
     - p(fund-loss) x at-risk-capital
```

Every defender lever moves exactly one term. Plays are tagged with their term:

- **C** (cost): raise compute/gas/time per attempt. Tarpits, decoy depth, verification hoops.
- **I** (identification): raise `p(identification)`. Watermarked lures, tripwire telemetry.
- **D** (deterrence): raise `expected_penalty` *perception*. Visible tripwire capability + published enforcement.
- **P** (poisoning): corrupt the attacker's estimate of `p(success)` and `value`. False surfaces that simulate-profit but fail live.
- **S** (separation): split the population so decoys only bite attackers, never users (screening).

## Core results to design against

1. **Cheap talk collapses.** A warning nobody can verify is free to ignore. Deterrence claims need *costly signals*: a demonstrable tripwire that fired once publicly ("a probe against our decoy admin path was recorded at tx 0x... - reported to X") converts talk into commitment.
2. **Commitment devices beat discretion.** Automated responses (guardian pauses, sweep-to-guardian, alert fan-out) committed in advance are credible because no human hesitation is exploitable mid-attack. Manual response = exploitable latency (the Term Finance lesson: minutes mattered).
3. **Ambiguity is an asset.** If the attacker cannot tell which of twenty oddity-checks are instrumented, they must handle all twenty - each cheap for you, expensive for them. Publish nothing about which decoys exist. (Asymmetric information: you know your decoy set; they must plan for the power set.)
4. **Screening separates types.** A lure reachable ONLY by behavior no legitimate user exhibits (calling an undocumented admin-shaped function, retrieving a "leaked key" file, following a robots.txt disallow) is a perfect classifier: bite = attacker with probability approaching 1. That is what makes tripwire telemetry high-confidence.
5. **Repeated games enable punishment - but you are limited.** Identify-first, then lawfully act. Your "punishment" levers are protocol-internal (pause, blacklist within your own protocol, blocklist within your own frontend) and lawful external handoff. Anything beyond that breaks the lawful gate.
6. **POMDP poisoning.** Agents are planners over observations. Poisoned observation (a profitable-looking decoy, a misleading storage layout, a fake internal doc) does not just waste one attack: if it persists, it corrupts the operator's priors for every future run - "is this protocol's surface real?" becomes unanswerable cheaply. That is the deepest cost imposition available, and the benchmark-solutions experiment is its cleanest public demonstration.
7. **Honeypot reflexive risk.** Smart-contract honeypots have a literature problem: researchers ship detectors (HoneyBadger-class tools flag lure-shaped bytecode patterns `[verify tool name currency]`). Counter: don't run textbook honeypot shapes on production-adjacent code; use novel, protocol-plausible shapes (decoy *features*, not decoy *traps*). A decoy "rewards-boost module" beats a decoy "sendMeEth()".

## Program design order

1. Inventory landing surfaces (where recon actually hits your protocol).
2. Choose 5-15 plays max - a sparse, plausible set. Overseeding reads as honeypot and collapses the ambiguity asset.
3. Instrument BEFORE publishing lures (telemetry first, bait second).
4. Rehearse the response ladder; assign a human owner.
5. Review quarterly against `defi-security-trends-standards` (attacker tooling shifts the landing surfaces).

## Effect-code legend (used across all play tables)

| Code | Lever | Attacker term moved |
|------|-------|---------------------|
| C | impose cost | compute/gas/time cost |
| I | identify | p(identification) |
| D | deter | expected_penalty perception |
| P | poison | p(success)/value estimates |
| S | separate | precision of attacker detection |

---
name: reviewing-cross-function-and-composer
description: "Review compositions of otherwise-safe functions in one transaction: deposit+borrow, harvest+redeem, vote+queue. Use when unit tests pass but invariants fail on sequences. The handler skill is the test harness."
---

# Reviewing cross-function composition

Most paid logic bugs are not a single detector hit. Two functions, one tx.

In-scope. This playbook is the **question list**; `writing-foundry-invariant-handlers` is the engine.

## Method

1. List value-moving entrypoints.
2. For each pair (including the same function twice), ask: after A then B, does conservation still hold?
3. Include donate, hook, flash loan, and oracle update as A.
4. Put A and B on the handler so the fuzzer finds the pair.

## Classic pairs

- deposit -> borrow (undercollateral if price is spot)
- harvest -> redeem (share price pumped for the harvester)
- transfer -> withdraw (read-only reentrancy)
- vote -> transfer (flashloan governance)
- liquidate -> oracle update

If the invariant fails, file **one** finding on the composition, not two half-bugs.

## Paid bounty analogs

This is where most "logic" Immunefi rows land. Full `$` table: `hunting-x-linked-bounties` `references/paid-payouts.md`.

| $ | Protocol | Pack | Whitehat |
|---|----------|------|----------|
| $1.1M | Beanstalk insufficient validation | T03 | nicole |
| ~$182k | Beanstalk logic | T03 | Immunefi |
| $150k | Synthetix | T01 | thunderdeep14 |
| $60k | Mushrooms | T01 | CKK Sec |
| $40k of ~$70k | Hats Velvet calldata | T34 | 16 wardens |
| $25k | Zapper arbitrary calldata | T34 | Lucash-dev |
| $25k | Tidal; Ondo | T01 | csanuragjain; Ashiq/iosiro |
| $20k x2 | Thena (C4 miss) | T01 | zzykxx |
| $5k | xDai Stake; Charged Particles | T34 / T03 | |
| High | Lido Dual Governance (funds not at risk) | T21 | 0xriptide |
| High | dHEDGE 2026 X | T01 | s4muraii77 (confirm) |
| 1 Crit + 1 High | Uniswap v4 periphery + Universal Router (OZ) | T34 | OpenZeppelin |

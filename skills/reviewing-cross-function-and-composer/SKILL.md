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

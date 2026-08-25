---
name: reviewing-with-solodit-and-swc
description: "Manual smart-contract review against Solodit historical findings and SWC weakness IDs by protocol type (vault, AMM, bridge, governance). Use after static analysis, before writing a contest report."
---

# Reviewing with Solodit and SWC

Tools catch known detectors. This playbook catches **logic bugs that already landed on similar protocols**. Whitehat only: in-scope source, local tests.

Load `sc-research` if you do not yet have scope. Companion checklist: `skills/security/auditing-foundry-smart-contract-security/references/vulnerability-checklist.md`.

## Steps

### 1. Label the protocol

Pick one or more labels before searching. Wrong label = wrong bugs.

| Label | Typical conservation laws |
|-------|---------------------------|
| ERC-4626 / vault / yield | `totalAssets` vs shares, first-depositor inflation, donation attack |
| AMM / CL / spot pool | reserve accounting, fee-on-transfer, empty-pool, tick math |
| Lending / CDP | oracle, LTV, liquidation bonus, interest index, bad debt |
| Staking / rewards | reward-index checkpoint, leftover dust, double-claim |
| Bridge / messenger | message replay, finality, address spoof, amount decode |
| Governance / timelock | proposal hijack, delay bypass, queue/execute mismatch |
| Proxy / upgrade | storage clash, missing initializer lock, impl selfdestruct |
| NFT / marketplace | callback reentrancy, royalty, signature replay |

### 2. Pull historical findings

Open [Solodit](https://solodit.xyz/) and search:

- protocol type + function name (`deposit`, `harvest`, `liquidate`, `borrow`)
- the exact dependency (OpenZeppelin version, Chainlink feed, Uniswap callback)
- known-issue titles from the contest README so you do **not** re-report them

For each hit: extract the **pattern** (what was trusted that shouldn't be), not the copy-paste report text.

Also map to [SWC](https://swcregistry.io/) ids. Core set: SWC-107 reentrancy, 105/106/115 access control, 101 arithmetic, 104 unchecked call, 112 delegatecall, 114 timing, 120 randomness, 113/128 DoS.

### 3. Walk the in-scope contracts

For every value-moving function:

1. Who can call it? (`msg.sender`, roles, callbacks, `tx.origin`)
2. What price / share / rate does it trust?
3. What external call happens, and can that callee re-enter or return less than assumed?
4. What invariant would break if this function is composed with one other function in the same tx?

Write the question down. Then try to break it with a Foundry test - do not "reason it is fine".

### 4. Dedup against tools

If Slither/Aderyn already flagged the line, keep **one** finding. Prefer the tool hit when it is a true positive; prefer the Solodit-shaped writeup when the bug is economic / compositional.

## Expected output

A short table: `(contract, function, pattern, SWC, Solodit analog, local test idea, severity)`. No exploit scripts. Next skill is usually `writing-foundry-invariant-handlers` or `contest-and-bounty-reporting`.

---
name: reviewing-cairo-and-starknet
description: "Review Cairo/Starknet contracts: u256 rounding, felt vs u256, account/session keys, L1-L2 messages. Use for Starknet DeFi (e.g. Vesu-class rounding). Lawful/in-scope only."
---

# Reviewing Cairo and Starknet

In-scope Cairo only. Snforge / Cairo tests. L1 messengers still need the EVM bridge playbook.

Historical: Vesu rounding disclosures in `move-cosmos-cairo.md`.

## Arithmetic

- Documented rounding direction on `u256` div (`K01`). 1-wei tests.
- `felt252` is not `uint256` (`K02`). Overflow aborts.

## Account / session

Native AA. Session keys must bind calldata and expiry (T39-shaped).

## Messages

L1->L2 and L2->L1: consume once, bind amount and recipient (`T17-T19` on both sides).

## Output

`(fn, rounding or felt mix, K0x, snforge idea)`.

## Paid bounty analogs

| $ | Protocol | Pack | Whitehat |
|---|----------|------|----------|
| High | Vesu rounding convention | K01 | kankodu, __alexxander_ |

zkLend ~$9.57M is a **hack** (`move-cosmos-cairo.md`). L1 handler `from_address` is still a review seed.

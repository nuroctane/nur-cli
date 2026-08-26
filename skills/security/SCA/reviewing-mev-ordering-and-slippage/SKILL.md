---
name: reviewing-mev-ordering-and-slippage
description: "Review MEV, ordering, and slippage: missing minOut/deadline, approve races, auction sniping, commit-reveal skips. Use for AMMs, launches, Dutch auctions, and any tx-order-sensitive value path. Analysis only - no sandwiching."
---

# Reviewing MEV, ordering, and slippage

SWC-114. Whitehat analysis. Do not sandwich users or send attacking bundles.

## Checklist

- Swaps/mints: `amountOutMin` / `minShares` and `deadline` (or block-based analogue).
- ERC-20 `approve` race - `forceApprove` / `permit`.
- Dutch auction / IDO: last-block sniping, missing commit-reveal.
- Oracle updates in the same block as a liquidation (`reviewing-oracles-and-pricing`).
- `block.timestamp` used as randomness (SWC-116/120) for a payout.
- Relayer gas griefing (SWC-126) on meta-txs.

## Report shape

Impact is usually user-level sandwich (Medium) unless the protocol itself can be drained by ordering (High). Say who loses: LP, trader, or protocol.

Tests: `vm.prank` two actors in chosen order; assert the victim still gets `minOut` or the tx reverts.

## Paid bounty analogs

| Note | Protocol | Pack |
|------|----------|------|
| Deposit front-run class (also-paid) | Rocket Pool / Lido | T25 |
| Tick compression / unsubscribe Mediums | Uniswap v4 periphery (Spearbit 2024) | T15 / T25 |

Sandwich of a user swap is usually Medium unless the **protocol** can be drained by ordering.

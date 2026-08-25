---
name: reviewing-amm-and-cl-pools
description: "Review AMM and concentrated-liquidity pools: k-invariant, callbacks, fee-on-transfer, empty pool, tick math, and spoofed swap callbacks. Use for Uniswap-style, stableswap, and custom AMMs."
---

# Reviewing AMM and CL pools

In-scope pool/router/callback code only. Prove with Foundry, not sandwiches on mainnet.

## CPMM / stableswap

- Invariant (`x*y=k`, stableswap `sum`/`prod`) holds after fees.
- Fee-on-transfer / rebasing tokens: reserves vs `balanceOf` desync.
- Empty pool / first liquidity: price set by attacker.
- Router: `minOut`, `deadline`, path length, recipient.

## Concentrated liquidity (v3-style)

- `uniswapV3SwapCallback` / mint callback: `msg.sender` must be a real pool (factory authenticates).
- Tick net liquidity matches positions.
- Fee growth overflow / owed tokens.
- Price limit and remaining amount on multi-pool hops.

## Callback spoof (common paid class)

If a callback trusts `msg.sender` without checking it is a factory-deployed pool, anyone can fake a swap/mint callback and drain tokens the pair thought it was owed. Write `test_RevertWhen_CallbackFromUnknown`.

## Flash / donate

- Flash swap not repaid.
- `donate` that changes price without fees (if the pool allows it, document).

Grep: `sc-research/references/grep-patterns.md` (callbacks, reserves, slot0).

Next: `reviewing-oracles-and-pricing` if this pool is used as a price, `writing-foundry-invariant-handlers` for `k`.

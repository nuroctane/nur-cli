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

## Paid bounty analogs

Full `$` table: `hunting-x-linked-bounties` `references/paid-payouts.md`. Raydium also lives on `reviewing-solana-programs`.

| $ | Protocol | Pack | Whitehat |
|---|----------|------|----------|
| **$1M** | Balancer rounding + flashSwap | T04 / T15 | GothicShanon89238 |
| $250k | Balancer V2 2025 | T15 | kankodu |
| **$505k** | Raydium ticks | S09 | riproprip |
| $100k | DFX EURS 2-decimal rounding | T04 | perseverance |
| ~$290k | The Graph rounding | T04 | GregadETH |
| 50 ETH | Balancer (riptide) | T15 | riptide |
| 4 Medium / 1 Crit | Uniswap v4 periphery (Spearbit / OZ 2024) | T15 | firm audits |

KyberSwap CL and Balancer V2 2025 composable stables are **hack** analogs (`case-cards.md`).

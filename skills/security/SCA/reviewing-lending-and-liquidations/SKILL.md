---
name: reviewing-lending-and-liquidations
description: "Review lending markets, CDPs, and liquidations: LTV, oracle, interest index, liquidation bonus, bad debt, and self-liquidation. Use for money markets and isolated pools."
---

# Reviewing lending and liquidations

In-scope. Fork tests of **public** markets are allowed as `eth_call` / Foundry fork without `--broadcast`.

## Market

- Interest index monotonic; jump/kink params cannot brick borrow.
- Collateral factor * price uses the **checked** oracle (staleness, sequencer).
- Isolated vs cross-margin: can one market's insolvency leak?

## Liquidation

- Seized collateral / repaid debt = documented bonus, never more than user collateral.
- Close factor, min repay, dust.
- Self-liquidation + flash loan: can a user print bad debt and socialize it?
- Liquidation on stale oracle or during sequencer downtime.

## Bad debt

- Who eats it (protocol reserves, remaining users, admin)?
- Can an attacker force bad debt cheaper than the profit from it?

## Same-tx

Borrow against spot-priced collateral in the same tx as an AMM push. If yes, it is an oracle finding too.

Invariants: see `protocol-invariants.md` lending section.

Next: `reviewing-oracles-and-pricing`, `contest-and-bounty-reporting`.

## Paid bounty analogs

Full `$` table: `hunting-x-linked-bounties` `references/paid-payouts.md`. Port Finance / marginfi also on `reviewing-solana-programs`. Vesu on `reviewing-cairo-and-starknet`.

| $ | Protocol | Pack | Whitehat |
|---|----------|------|----------|
| **$1M+** | Notional free collateral | T14 | 0x60511e57 |
| ~$630k | Port Finance | S02 / T14 | nojob |
| $100k | Silo | T14 | kankodu |
| ~$95k | Yield Protocol | T14 | Paludo0x |
| $50k | Sense `onSwap` | T02 | alephv.eth |
| $30k | Perpetual Protocol bad debt | T14 | banditx0x |
| $2k | Fringe.fi insolvency | T14 | Trust |
| High | Fraxlend | T14 | 0xjuaan, 0xSpearmint |
| Crit | marginfi flash + health | S10 | Felix Wilhelm |
| Crit | Alchemix V3 liq-fee overpay | T14 | Immunefi boost |
| High | Vesu rounding | K01 | kankodu / alexxander |

Euler donate, Radiant precision, Compound-fork empty markets are **hack** analogs.

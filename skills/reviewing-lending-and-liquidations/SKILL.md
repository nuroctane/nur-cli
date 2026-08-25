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

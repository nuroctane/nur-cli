---
name: reviewing-oracles-and-pricing
description: "Review DeFi price oracles and share/rate math for flash-loan, staleness, sequencer, and first-depositor manipulation. Use for vaults, lending, liquidations, and any contract that reads a spot price."
---

# Reviewing oracles and pricing

DeFi's #1 loss class. Spot AMM prices and naive ERC-4626 share math are not oracles.

In-scope review only. Prove issues with **fork or local Foundry tests**, not mainnet txs.

## Threat model (walk every price read)

1. **Spot pool / `getReserves` / `slot0`** - one-tx flash-loan manipulable. If this number sizes a mint, borrow, liquidation, or LP value, it is a finding until proven otherwise.
2. **Missing TWAP window / too-short window** - still manipulable around low-liquidity ticks.
3. **Chainlink without checks** - require `updatedAt` freshness, `answeredInRound`, min/max bounds, and on L2 a sequencer-uptime feed. Stale answers during downtime are a known paid class.
4. **Wrong feed / ETH vs USD vs token decimals** - silent scale bugs. Confirm `decimals()` of the aggregator vs the token.
5. **Share inflation / donation** - first depositor donates, then a victim mints at a broken rate. Virtual offset / dead shares are the usual fix; absence is a High on a public vault.
6. **Read-only reentrancy** - a view used as a price (`totalAssets`, `getRate`) during an external call.

## Steps

### 1. Grep the price surface

Search in-scope source for: `getReserves`, `slot0`, `latestAnswer`, `latestRoundData`, `consult`, `peek`, `convertToShares`, `convertToAssets`, `getPrice`, `twap`, `observe(`.

Every hit: who consumes it, and can the consumer move value in the same tx?

### 2. Check Chainlink hygiene

For each aggregator:

- `updatedAt + heartbeat` vs protocol's max staleness
- `answeredInRound >= roundId` (or current API equivalent)
- min/max deviation vs a secondary source if the protocol claims one
- L2: sequencer uptime grace period after restart

### 3. Prove with a test, not a story

Fork test shape (local, no broadcast):

- deal attacker funds on the pool
- swap to push the spot price
- call the victim function in the **same tx** (or after one block if they used a 1-block TWAP)
- assert the protocol stayed solvent / the attacker did not mint extra shares

If the assertion holds, document why (TWAP window, Chainlink, internal TWAP). If it fails, that is the finding.

### 4. ERC-4626 extras

- convertToShares(1) on an empty vault
- donate, then tiny deposit
- fee-on-transfer / rebasing underlying (balance delta vs `amount` param)

## Expected output

Table: `(price read, source, manipulable in one tx?, check missing, Foundry test, SWC/analog, severity)`. Critical/High if value moves from a spot price. Next: `writing-foundry-invariant-handlers` or `contest-and-bounty-reporting`.

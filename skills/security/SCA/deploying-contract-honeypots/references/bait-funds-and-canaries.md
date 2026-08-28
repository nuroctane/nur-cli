# Bait funds, bait wallets, and canary transfers

Money-shaped lures. Everything here is pre-funded by you, so nothing is "taken" from anyone: an attacker draining bait loses only their gas, and their wallet self-identifies on first touch.

## 1. Bait wallets (I, C)

Wallets with:
- A believable history (aged, small in/out flows, maybe a name like `ops-treasury-2` in an explorer label you control).
- Small real balances (enough to be worth a probe, not enough to matter; think hundreds of USD, not thousands).
- Watchers on every explorer/indexer: first inbound-or-outbound movement after you publish their "keys" = attacker wallet surfaced.

Publishing channel is the point: the fake `.env` / runbook / backup-key file leaks these keys through a watermarked doc (`poisoning-agent-recon-surfaces`). Whichever wallet moves first tells you which channel leaked AND gives you the attacker's funding wallet for the evidence pack. Effect: **I** at its purest, plus **C** (their funded attack wallet just burned time on nothing).

## 2. Canary transfers (I)

From a bait wallet, send tiny amounts (dust) with unique amounts per distribution (0.0042069 ETH, 0.0031337 ETH...) to fresh addresses. If dust from channel X moves or consolidates in attacker infrastructure, channel X's artifact leaked and you have an address chain. Standard canary-token mechanics, onchain flavor. Zero legal exposure; pure marking.

## 3. Decoy profit pools (P, C, S)

Contracts that look extractable and never pay anyone:

- A "yield sink" whose `claim()` reverts politely while its view math says the caller is owed a fortune.
- A poorly-collateralized-looking CDP whose "liquidation" path pays only on oddity-checks (see `anti-simulation-fork-traps.md` divergence pattern 1).
- An "unguarded reward distributor" whose numbers are bait-value (not backed by real tokens).

Fork-sims show profit; live attempts burn gas. Legit searchers lose nothing (pool never pays anyone). Keep these on decoy deployments; never inside prod.

## 4. Approved-spender traps (I) - OPS14-adjacent

Decoy tokens/approvals: a decoy token grants the caller an allowance (recorded) that can only pull from the decoy itself. Token-drainer bots that mass-scan for approvals touch the tripwire and identify themselves. Deploy on decoy addresses only.

## 5. Watermarked canary NFTs / soul-tokens (I)

Hold canary NFTs in bait wallets; each has a unique token id per leak channel. If a drainer sweeps the wallet, the swept ids tell you which "collection" (i.e., which leaked artifact) drove it. Also works with ordinary tokens using unique decimal dust (method 2).

## 6. What bait is FOR (keep the objective honest)

| Objective | Bait type |
|-----------|-----------|
| Prove unauthorized access to a specific artifact | watermarked keys/dust (channel attribution) |
| Identify attacker funding wallet | bait wallet first-touch |
| Waste attacker capital | decoy pools that never pay |
| Feed the evidence pack | all of the above, logged |

Bait is not revenge and not a business line. Total value at risk in your entire bait layer should be a rounding error - the asymmetric play is your $50 of bait against their compute budget and their exposure to identification.

## Deployment checklist

1. Fund wallets from an ops-safe path (never your treasury hot path).
2. Register every bait in the internal manifest (address, channel, canary amount, watcher).
3. Wire watchers to the response ladder BEFORE publishing the leak artifacts.
4. Review quarterly: refresh baits, retire burned ones, re-score channels.
5. When a bait fires: evidence pack first (`identifying-agentic-attackers`), public deterrence signal second, never "counter-drain".

---
name: deploying-contract-honeypots
description: "Onchain deception primitives and per-attack-vector deception plays for defending DeFi protocols against AI-agent attackers: decoy functions and tripwire storage, anti-simulation fork traps, bait funds and canaries, plus concrete play tables for every vulnerability class (T01-T40 EVM, S/M/C/K non-EVM, OPS ops-layer) and per-chain adaptations. Use to design tripwires for a protocol under active agentic recon. Lawful/defender-owned only. Load one reference slice."
---

# Deploying contract honeypots (active onchain defense)

Tripwires, decoys, and bait for the execution and simulation stages of the agentic attack loop. Every primitive here is **inert for legitimate actors**: decoys record and revert; they never take anyone's funds. Lawful gate + screening invariants: `agentic-defense-game-theory` `references/lawful-deception-bounds.md`.

Class ids reference `historical-smart-contract-vulns` `references/taxonomy.md`. Effect codes: C cost, I identification, D deterrence, P planner-poisoning, S separation.

## Choose your file

| You want | Load |
|----------|------|
| Build decoy functions, tripwire slots, guardian automation | `references/decoy-primitives.md` |
| Make agent fork-simulations see a world that fails live | `references/anti-simulation-fork-traps.md` |
| Bait wallets, watermarked canary transfers, decoy pools | `references/bait-funds-and-canaries.md` |
| Plays per EVM vulnerability class T01-T40 | `references/plays-evm.md` |
| Plays per Solana/Move/Cosmos/Cairo/Bitcoin/other classes | `references/plays-non-evm.md` |
| Plays per ops/phishing/key-theft class OPS01-20 | `references/plays-ops-surfaces.md` |
| Chain-specific tripwire mechanics (Base/Robinhood/Arbitrum/OP/Hyperliquid/Solana/...) | `references/plays-per-chain.md` |

## Design order (repeat: telemetry first, bait second)

1. Pick the 3-6 vulnerability classes your protocol is most exposed to (audit + `defi-security-trends-standards`).
2. For each, take the matching play row(s) from the play tables.
3. Define the tripwire event schema and wire the response ladder (`identifying-agentic-attackers`) BEFORE any decoy is deployed.
4. Deploy decoys under the same review process as prod code - a broken decoy is worse than none (it trains attackers on your sloppiness).
5. Never seed more than ~15 decoys; sparse beats pattern-detectable.

## Invariants (every decoy must pass)

- Inert: no path takes funds from any address except pre-funded decoy balances.
- Separable: only reachable via attacker-shaped behavior (undocumented function, leaked-reference shape, decoy-contract address, sim-detectable sequence).
- Instrumented: every touch emits a structured event the response ladder consumes.
- Disclosed internally: auditors + security council hold the manifest; attackers do not.

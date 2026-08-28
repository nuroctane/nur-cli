---
name: reviewing-major-chain-surfaces
description: "Chain-specific audit surfaces for every major chain by current volume and TVL: Ethereum mainnet, Base, Arbitrum One + Orbit (incl. Robinhood Chain), OP Mainnet + Superchain, Hyperliquid HyperCore/HyperEVM, Solana, BNB Chain, Linea, Scroll, zkSync Era, Sonic, Berachain, Monad, Plasma, Sui, Aptos, TON, TRON, and more. Use when reviewing or deploying code on a named chain, checking precompile / fee / oracle / bridge deltas, or asking what is different about auditing there. Load one reference slice."
---

# Reviewing major chain surfaces

A deployed contract inherits whatever its host chain lies about: precompiles that behave differently, sequencers that reorder, oracles with different freshness guarantees, canonical bridges with their own trust assumptions, hardfork deltas that change gas math under you.

This skill is the **per-chain dossier**. Pick **one** reference file from `references/index.md`. Deep protocol logic still belongs to the class playbook (`reviewing-amm-and-cl-pools`, `reviewing-lending-and-liquidations`, ...). VM-native languages route to their own playbooks (`reviewing-solana-programs`, `reviewing-move-modules`).

## Lawful gate (unchanged)

In-scope source only. Local tests on your own deployment. Chain facts here are review aids, not targets.

## How to use

1. Name the chain -> open that one reference file.
2. Work its checklist into the review: execution quirks, canonical bridge, oracle availability, fork/current hardfork deltas, known past incidents on that host.
3. Cross-reference `references/evm-fork-deltas.md` once per engagement (Pectra/Fusaka/Glamsterdam era, EIP-7702 account-abstraction patterns).
4. Any discrepancy found -> prove it with a local fork test pinned to that chain's RPC, then file via `contest-and-bounty-reporting`.

## What each file contains

| Section | Why an auditor cares |
|---------|---------------------|
| Stack & proofs | Who can censor/reorder/freeze; stage-0 vs stage-2 is the real threat model |
| Execution quirks | Precompile divergence, fee model, address aliasing, block-time effects |
| Canonical bridge | The wrongly-trusted thing most likely to be exploited next |
| Oracles | Which feeds exist there; sequencer-uptime-feed obligations |
| Host incidents | Same-class regressions to grep for on this exact infrastructure |

## Deltas are dated

Files were refreshed 2026-08 from public sources (chain docs, L2BEAT-class pages, postmortems). Facts move weekly on active rollups; anything time-sensitive carries `[verify]`. Re-check the linked primary source before citing in a report.

---
name: auditing-blockchain-clients
description: "Audit blockchain node clients (Go, Rust, C/C++) for P2P, consensus, RPC, and memory-safety issues. Use for client contest repos (Firedancer-class) and node source. Prefer DarkNavy client-auditor when installed."
---

# Auditing blockchain clients

Not Solidity. In-scope client source only (the contest repo or a published program).

If DarkNavy `client-auditor` is installed (`npx skills add DarkNavySecurity/web3-skills` / ecosystem ensure), use that three-phase flow: `start` then `verify` then `report`. Do not silently chain.

## Nur fallback (when the kit is missing)

1. Map: networking, consensus, execution, RPC, storage, mempool.
2. Families: panic/DoS on malformed p2p, integer overflow on heights/fees, RPC auth, resource exhaustion, unsafe FFI, replay of consensus messages.
3. Proof: unit/fuzz in the client's own test harness. No mainnet packet spam.

Track record context: DarkNavy posts client findings on Immunefi (Firedancer comps, rippled). Same lawful bar as the rest of this pack.

## Paid bounty analogs (chain / client, not Solidity)

Full `$` table: `hunting-x-linked-bounties` `references/paid-payouts.md` (clients section).

| $ | Protocol | Pack | Whitehat |
|---|----------|------|----------|
| **$2M+$75k** | Sei | O04 | usmannk, Catchme |
| $1M+ | Moonbeam / Frontier / Polkadot EVM | O02 | pwning.eth |
| $150k | Evmos docs vs consensus | O04 | jayjonah.eth |
| $75k | Polygon consensus bypass | O04 | Niv Yehezkel |
| $70k | Acala block production | O04 | Lastc0de |
| $50k | Astar; Q Blockchain; Injective X | O04 / O02 | Zellic; Blockian; f4lc0n |
| $20k | Oasis shutdown | O04 | Trust |
| $200k | Oasys | O04 | merkle_bonsai |
| $6.71k | Movement chain split | M06 | Yunus Emre |

Optimism $2M OVM is **also** `reviewing-l2-sequencer-and-finality`. Sui $50k halt is **also** `reviewing-move-modules`. Stacks DoS is `reviewing-bitcoin-adjacent`.

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

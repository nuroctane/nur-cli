---
name: reviewing-cosmos-and-ibc
description: "Review Cosmos SDK modules, IBC (ICS-20/23), ibc-hooks, CosmWasm submessages, Authz, Ante/fees. Use for appchains and IBC-connected DeFi. Lawful/in-scope only."
---

# Reviewing Cosmos and IBC

In-scope Go modules / CosmWasm contracts only. Use simapp or cw-multi-test. Never forge live light-client proofs.

Historical: `historical-smart-contract-vulns` `references/move-cosmos-cairo.md` (Dragonberry, ASA-2024-007, Cronos fees, Evmos, Sei, Axelar halt).

## ICS-20

Minted vouchers cannot exceed escrow (`C01`). Denom trace cannot be spoofed into a native.

## ICS-23 / light client

`ibc-go` / `ics23` versions include the Dragonberry-era fixes (`C02`). If a fork is behind, that is the finding. Do not ship a proof-forger.

## ibc-hooks and CosmWasm

Timeout / ack / `reply` cannot re-enter mint (`C03`). Submessages: same conservation as Solidity CEI.

## Authz and Ante

Grants are least privilege (`C04`). Fee/Ante cannot steal the current block's fees (`C05`, Cronos analog).

## Output

`(msg_type, invariant, C0x, simapp test idea)`. Next: `reviewing-bridges-and-messaging` if there is also an EVM gateway.

## Paid bounty analogs

| $ | Protocol | Pack | Whitehat |
|---|----------|------|----------|
| **$2M+$75k** | Sei chain/module | O04 | usmannk, Catchme |
| $150k | Evmos docs vs impl | O04 | jayjonah.eth |
| $50k | Axelar halt | C01 | Marco Nunes |
| $50k | Injective 2026 X | O04 | f4lc0n (confirm Immunefi) |
| $40k | Cronos tx-fee theft | C05 | zb3 |

BNB IAVL $586M is a **hack** (C02). Dragonberry is a library patch, not a payout row.

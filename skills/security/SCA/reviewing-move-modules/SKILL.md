---
name: reviewing-move-modules
description: "Review Aptos/Sui/Movement Move modules: borrow_global address, shared-object ACL, abilities, capabilities, PTB composition, halt/DoS. Use for Move programs. Lawful/in-scope only."
---

# Reviewing Move modules

In-scope modules only. Aptos tests or Sui Move tests locally. Shared objects on Sui have **no runtime ACL** - the module must check every `public` / `entry`.

Historical: `historical-smart-contract-vulns` `references/move-cosmos-cairo.md` (Cetus ~$223M overflow, Sui shutdown $50k, Movement chain-split).

## Aptos

- `borrow_global(_mut)` address is the intended account (`M01`).
- Capabilities (`TreasuryCap`, admin) cannot leak (`M04`).
- Abilities `key`/`store`/`drop`/`copy` match the asset (`M03`).

## Sui

- Shared object: every entry checks sender or a cap (`M02`).
- PTB: compose deposit/borrow/withdraw in one tx (`M05`).
- Frozen / owned / shared transitions are documented.

## Halt

Pathological payloads must abort, not crash validators (`M06`). Client crashes -> `auditing-blockchain-clients`.

## Output

`(module, entry, missing check, M0x, unit test idea)`. No mainnet objects as victims.

## Paid bounty analogs

| $ | Protocol | Pack | Whitehat |
|---|----------|------|----------|
| $50k | Sui network shutdown | M06 | F4lt |
| $6.71k | Movement Labs chain split | M06 | Yunus Emre |

Cetus integer-mate overflow ~$223M is a **hack** (M07). Still fuzz `<<` then `*`. Validator crash proofs may belong in `auditing-blockchain-clients`.

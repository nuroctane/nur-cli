---
name: reviewing-upgradeable-proxies
description: "Review upgradeable Solidity proxies (UUPS, transparent, beacon) for storage-layout clashes, unprotected initialize, missing initializer lock, and implementation suicide. Use before shipping or auditing proxies."
---

# Reviewing upgradeable proxies

A proxy bug is usually instant total loss. Review this class even when Slither is quiet.

In-scope source only. Confirm with storage-layout diffs and `test_RevertWhen_*`, not live upgrades.

## Pattern checklist

### Storage layout

- Implementation storage starts after the proxy reserved slots (ERC-1967).
- Upgrades **never reorder or remove** existing state variables. Append only, or use a storage-gap / ERC-7201 namespaced struct.
- Diff layouts between versions: `forge inspect Impl storage --pretty` (or solc storage layout JSON) and compare.
- Packed structs: changing a type width shifts neighbors.

### Initialize / constructors

- Implementation constructor must `_disableInitializers()` (OpenZeppelin) so the impl cannot be initialized and `selfdestruct`ed under old EVM rules / taken over.
- `initialize` is `initializer` (or `reinitializer(n)` on purpose) and **not** callable twice.
- Front-run: if `initialize` sets owner, the deploy script must initialize in the same tx as proxy creation (`forge script` + factory, or OZ upgrades plugin). A public uninitialized proxy is Critical.
- `reinitializer` version must increase; a lower/equal version is a bug.

### UUPS vs transparent vs beacon

- UUPS: `_authorizeUpgrade` must be access-controlled. Missing modifier = anyone upgrades. Slither `unprotected-upgrade`.
- Transparent: admin must not be a hot wallet that also calls logic (selector clash). Admin is for upgrade only.
- Beacon: swapping the beacon implementation upgrades **every** child. Beacon owner is a crown jewel.

### delegatecall surface

- No `delegatecall` to a user-supplied address.
- `upgradeToAndCall` data is trusted-admin only.
- Avoid `delegatecall` into contracts that use `selfdestruct` or that write `address(this)` assuming they are not a proxy.

## Steps

1. Identify proxy kind from `ERC1967Upgrade` / `UUPSUpgradeable` / `TransparentUpgradeableProxy` / `UpgradeableBeacon`.
2. List every `initialize` / `reinitializer` / `_authorizeUpgrade`.
3. Diff storage layout vs the previous tagged implementation (or vs OZ parent).
4. Write tests: attacker cannot `initialize`, cannot `upgradeTo`, cannot call admin functions on the proxy.
5. Confirm deploy script initializes atomically and keys live in an encrypted `cast` keystore (Foundry skill `secure-deployment-and-keys.md`).

## Expected output

`(issue, contract, slot/function, test, severity)`. Unprotected upgrade or uninitialized proxy = Critical, blocks deploy. Next: `contest-and-bounty-reporting` or the Foundry PASS/FAIL gate.

## Paid bounty analogs

Full `$` table: `hunting-x-linked-bounties` `references/paid-payouts.md`. Local tests only.

| $ | Protocol | Pack | Whitehat |
|---|----------|------|----------|
| **$10M** | Wormhole uninitialized UUPS impl | T12 | satya0x |
| $42k | 88mph unprotected `init` | T12 | Ashiq Amien |
| also-paid | Harvest uninit Uniswap V3 vaults; Teller beacon | T12 | Immunefi library |

Invariant: attacker cannot `initialize` the implementation. `test_RevertWhen_AttackerInitializesImplementation`.

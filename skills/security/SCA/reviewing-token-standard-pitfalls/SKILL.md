---
name: reviewing-token-standard-pitfalls
description: "Review ERC-20/721/1155 integration footguns: fee-on-transfer, rebasing, missing return, approve race, weird decimals, ERC-777/721 hooks. Use whenever a protocol talks to arbitrary tokens."
---

# Reviewing token standard pitfalls

ToB token integration checklist lives in building-secure-contracts. This is the Nur walk. In-scope.

## ERC-20

- Missing `bool` return (USDT-class): use OZ `SafeERC20`.
- Fee-on-transfer: `balanceOf` delta vs `amount`.
- Rebasing (stETH-class): shares vs balances; don't cache `balanceOf`.
- Decimals != 18 assumed.
- `approve` from non-zero to non-zero (USDT race) - `forceApprove` / `permit`.
- Permit (2612) - see `reviewing-signatures-permit-and-eip712`.
- Unlimited allowance to a spoofed router.

## ERC-721 / 1155

- `safeMint` / `safeTransfer` callbacks = reentrancy (`reviewing-reentrancy-and-callbacks`).
- Approval for all scope.
- 1155 batch array length mismatch.

## ERC-777 / tokensReceived

Default-on callbacks. If the vault does CEI wrong, this is classic reentrancy.

## Multiple tokens in one pool

One weird token should not brick or drain the others (empty-pool, donation, return-false).

Tests: mock tokens (fee-on-transfer, rebase, no-return) in Foundry. Do not need mainnet.

Next: `reviewing-erc4626-and-vaults` if the token is an asset of a vault.

## Paid bounty analogs

| $ | Protocol | Pack | Whitehat |
|---|----------|------|----------|
| $560k | Redacted Cartel **custom** approval (not vanilla ERC-20) | T23 | Tommaso Pifferi |

Fee-on-transfer / missing-return are mostly contest + 2026 DHL rows (`incident-corpus.md`), not mega Immunefi payouts. Still write mock-token tests.

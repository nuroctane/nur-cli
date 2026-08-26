---
name: reviewing-access-control-and-auth
description: "Review Solidity access control: missing modifiers, tx.origin, initialize front-run, role rugging, default-public, and two-step ownership. Use on every admin, mint, upgrade, and withdraw path."
---

# Reviewing access control and auth

SWC-105/106/115. In-scope. Every state changer gets a revert test.

## Walk

1. List every `external`/`public` that writes storage or moves value.
2. Who is allowed? Owner, role, callback, anyone.
3. `tx.origin` is a finding (phishable).
4. `initialize` / constructor: proxy vs impl (`reviewing-upgradeable-proxies`).
5. Two-step ownership (OZ `Ownable2Step`) if the owner can rug TVL.
6. Renounce/transfer owner while a critical role is the same EOA.
7. `onlyRole` with `DEFAULT_ADMIN_ROLE` held by a hot wallet.

## Tests

```solidity
function test_RevertWhen_AttackerWithdraws() public {
    vm.prank(attacker);
    vm.expectRevert();
    vault.withdraw(1 ether);
}
```

One per guard. Name them `test_RevertWhen_*`.

If a function is intentionally open (deposits), document why it cannot break conservation laws (`writing-foundry-invariant-handlers`).

## Paid bounty analogs

Full `$` table: `hunting-x-linked-bounties` `references/paid-payouts.md`. Uninitialized proxy is `reviewing-upgradeable-proxies`, not this file.

| $ | Protocol | Pack | Whitehat |
|---|----------|------|----------|
| $400k | Enzyme missing privilege | T02 | rootrescue |
| $100k | APWine delegations | T02 / T10 | setuid0 |
| $50k | Sense | T02 | alephv.eth |
| $50k | VeChainThor VTHO accrual | T01 | nnez |
| $40k | Cronos tx fees | C05 | zb3 |
| $28k | Alchemist admin brick | T02 | Dacian |
| $7.5k | Alchemix access | T02 | Ashiq |
| $4.5k | Bitswift unlimited mint | T02 | Immunefi |

---
name: writing-foundry-invariant-handlers
description: "Write Foundry invariant tests with handler contracts, ghost variables, and actor rotation. Use when a vault, AMM, or any value-moving Solidity contract only has unit tests."
---

# Writing Foundry invariant handlers

Unit tests show one path. Handler-based `invariant_*` tests show whether conservation laws survive **random sequences**. This is the researcher skill that actually finds economic bugs.

Authorized code only. Fail the audit if value-moving contracts have no invariants.

Full snippet lives in `skills/security/SCA/auditing-foundry-smart-contract-security/references/api-reference.md`.

## When this is the playbook

- User says invariant, handler, ghost variable, stateful fuzz, "forge invariant".
- Coverage of deposit/withdraw/borrow/liquidate is only happy-path unit tests.
- Static analysis is clean but accounting could still desync.

## Steps

### 1. Write the laws first (English, then `assert`)

Examples:

- Conservation: `address(vault).balance == ghostDeposits - ghostWithdraws` (adjust for fees).
- Solvency: `totalAssets() >= totalShares implied assets`.
- Access: only role X can `setFeed` / `upgradeTo`.
- No unexpected mint: `totalSupply` only changes in `deposit`/`withdraw`.

If you cannot state the law, you cannot test it. Stop and read the protocol.

### 2. Handler, not raw `targetContract(vault)`

The fuzzer calling `vault` directly will spam reverts and miss realistic sequences. Wrap it:

- `bound` every amount into a live range.
- Rotate actors with `useActor(seed)` + `makeAddr`.
- Track **ghosts** for sums the contract does not expose.
- Early-return (don't revert) on empty-balance withdraws so `fail_on_revert` can later be `true`.
- `targetContract(address(handler))` in `setUp`.

See the VaultHandler pattern in the Foundry skill api-reference.

### 3. Cover the adversarial extras

Add handler actions for the ways real attackers compose calls:

- donate tokens / ETH directly to the vault (ERC-4626 inflation).
- `deal` weird balances, then deposit 1 wei.
- set a stale oracle if the protocol allows it in-test.
- call view functions that other protocols might use mid-reentrancy (read-only reentrancy).

Do **not** add a "drain" action that broadcasts. Stay in `forge test`.

### 4. Tune and read failures

```toml
[invariant]
runs = 256
depth = 128
fail_on_revert = false   # true once the handler constrains inputs
[fuzz]
runs = 10000
```

When an invariant breaks: shrink the call sequence (`-vvvv`), turn it into a **unit test** that replays those calls, then fix or file.

### 5. Pair with revert tests

Every `onlyOwner` / `require` gets `test_RevertWhen_*` + `vm.prank(attacker)` + `vm.expectRevert`. Invariants do not replace access-control unit tests.

## Expected output

- `test/handlers/*Handler.sol` and `test/*Invariant.t.sol`
- `forge test --match-test invariant_ -vvv` passing (or a minimized failing sequence for the report)
- A one-line statement of each conservation law

Rounding / first-depositor / Balancer-class paid bugs are handler work. After a hit, map `$` via `hunting-x-linked-bounties` so the report cites a real analog, not a vibe.

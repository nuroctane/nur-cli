---
name: reviewing-erc4626-and-vaults
description: "Review ERC-4626 and vault share math for inflation, donation, first-depositor, rounding, and fee-on-transfer. Use for yield vaults, tokenized strategies, and convertToShares/convertToAssets paths."
---

# Reviewing ERC-4626 and vaults

Empty-vault and donation bugs have paid Immunefi multiple times. Treat every vault as hostile on deposit 1.

In-scope only. Local Foundry tests. See `sc-research/references/protocol-invariants.md`.

## Checklist

1. **Virtual offset / dead shares** - OpenZeppelin ERC-4626 offset (or equivalent) present? If not, first depositor can donate, then a victim mints at a broken rate. High/Critical on a public vault.
2. **`decimals` / asset vs share** - `convertToShares` uses asset decimals correctly. Fee-on-transfer: measure **balance delta**, not `amount`.
3. **Inflation via `totalAssets`** - if `totalAssets` reads a manipulable strategy/price, this is also an oracle bug (`reviewing-oracles-and-pricing`).
4. **Donation** - `asset.transfer(vault, X)` then tiny deposit. Invariant: donor does not steal victim shares.
5. **Rounding** - `mulDiv` direction: mint rounds in favor of the vault, redeem likewise. Fuzz 1 wei through max.
6. **Hooks** - `deposit` -> strategy `invest` that calls back (`tokensReceived`, ERC-777). CEI + `nonReentrant`.
7. **Lossy strategies** - slippage on harvest, sandwich of `totalAssets` during deposit.

## Tests to write

```solidity
function test_FirstDepositInflation() public {
    // attacker deposits 1, donates, victim deposits. assert victim shares ~= expected
}
function invariant_AssetsCoverShares() public view {
    assertGe(vault.totalAssets(), vault.convertToAssets(vault.totalSupply()));
}
```

Handler actions: deposit, mint, withdraw, redeem, donate, harvest. `targetContract(handler)`.

Next: `writing-foundry-invariant-handlers` or `contest-and-bounty-reporting`.

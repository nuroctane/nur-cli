---
name: reviewing-reentrancy-and-callbacks
description: "Review reentrancy and token/AMM callbacks: classic, cross-function, read-only, ERC-777/721/1155 hooks, Uniswap callbacks, failed-call DoS. Use on any value-moving Solidity that makes external calls."
---

# Reviewing reentrancy and callbacks

SWC-107 plus the economic variants static tools miss. In-scope. Attacker contracts live in `test/` only.

## Variants

1. **Classic** - external call then state update. CEI or `nonReentrant`.
2. **Cross-function** - `withdraw` re-enters `transfer` that still sees old balances.
3. **Read-only** - a view (`totalAssets`, `getRate`, `balanceOf`) used by another protocol mid-call. The view is "safe" and still exploitable.
4. **ERC-777 / 721 / 1155 hooks** - `tokensReceived`, `onERC721Received`.
5. **AMM callbacks** - `uniswapV2Call`, v3 swap/mint callbacks (`reviewing-amm-and-cl-pools`).
6. **Failed-call DoS** - push ETH to a contract that `revert`s in `receive` (SWC-113). Pull payments.

## Confirm

- Slither `reentrancy-eth` / `reentrancy-no-eth` / `reentrancy-benign`.
- Write a test attacker with `receive()` / `onERC721Received` that re-enters. Assert the invariant still holds (or that the call reverts).

## Fix language (for reports)

Checks-Effects-Interactions, OZ `ReentrancyGuard`, pull-over-push, view isolation (snapshot before external call).

Do not ship a mainnet-ready attacker. The test file is the proof.

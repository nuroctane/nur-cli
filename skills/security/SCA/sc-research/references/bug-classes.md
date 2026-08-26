# DeFi bug-class checklist (whitehat)

Walk this on every in-scope protocol. Each class: **question**, **grep seeds**, **test idea**. Tests stay local. Do not write drain scripts.

Derived from contest corpora (Solodit, Immunefi paid writeups, Cyfrin/Quill/DarkNavy skill kits) plus Nur playbooks.

## 1. Accounting desync

Shares, assets, indexes, or reserves that can diverge from actual balances.

- Grep: `totalAssets`, `totalSupply`, `convertToShares`, `index`, `accumulate`, `rewardPer`
- Test: donate tokens, fee-on-transfer, first depositor 1 wei, harvest mid-stream
- Playbook: `reviewing-erc4626-and-vaults`

## 2. Access control

Missing, bypassable, or `tx.origin` auth. `initialize` front-run. Role that can rug.

- Grep: `onlyOwner`, `onlyRole`, `msg.sender`, `tx.origin`, `initialize`, `upgradeTo`
- Test: `test_RevertWhen_*` with `vm.prank(attacker)` on every state changer
- Playbook: `reviewing-access-control-and-auth`

## 3. Incomplete path / missing check

A function handles the happy path and skips pause, cap, deadline, or insolvency.

- Grep: early `return`, commented `require`, TODOs, `// unchecked`
- Test: fuzz the skipped branch
- Playbook: `triaging-and-deduping-findings`

## 4. Off-by-one / rounding

`>` vs `>=`, tick boundaries, `mulDiv` rounding against the protocol.

- Grep: `mulDiv`, `/ `, `% `, `+ 1`, `- 1`
- Test: amounts 1, 2, max-1; rounding direction invariant
- Playbook: `writing-foundry-invariant-handlers`

## 5. Oracle / price

Spot AMM, stale Chainlink, wrong decimals, sequencer down.

- Grep: `getReserves`, `slot0`, `latestRoundData`, `consult`, `twap`
- Playbook: `reviewing-oracles-and-pricing`

## 6. ERC-4626 / vault inflation

Donation, first depositor, virtual offset missing, inflation attack.

- Playbook: `reviewing-erc4626-and-vaults`

## 7. Reentrancy

Classic, cross-function, read-only, ERC-777/721/1155 callbacks, `safeTransfer` hooks.

- Playbook: `reviewing-reentrancy-and-callbacks`

## 8. Flash loan / same-tx composition

Anything that reads a price or share rate and moves value in one tx.

- Playbook: `reviewing-oracles-and-pricing`, `reviewing-lending-and-liquidations`

## 9. Signature replay

Permit, EIP-712, missing `chainId`/`nonce`/`deadline`, `ecrecover == 0`.

- Playbook: `reviewing-signatures-permit-and-eip712`

## 10. Proxy / upgrade

Unprotected `upgradeTo`, uninitialized impl, storage clash, beacon owner.

- Playbook: `reviewing-upgradeable-proxies`

## 11. Liquidation / bad debt

Bonus too high, oracle delay, self-liquidation, seized amount vs debt.

- Playbook: `reviewing-lending-and-liquidations`

## 12. AMM / concentrated liquidity

Tick math, empty pool, fee-on-transfer, callback spoof (`uniswapV3SwapCallback`).

- Playbook: `reviewing-amm-and-cl-pools`

## 13. Bridge / message

Replay, out-of-order, wrong decoder, address aliasing, finality assumed too early.

- Playbook: `reviewing-bridges-and-messaging`

## 14. Governance / timelock

Delay bypass, proposal hijack, `queue`/`execute` mismatch, short delay vs flashloan votes.

- Playbook: `reviewing-governance-and-timelocks`

## 15. Token weirdness

Fee-on-transfer, rebasing, missing return, ERC-20 approve race, ERC-721 safeMint hooks.

- Playbook: `reviewing-token-standard-pitfalls`

## 16. MEV / ordering / slippage

Missing `minOut`/`deadline`, AMM sandwich, Dutch auction, commit-reveal skipped.

- Playbook: `reviewing-mev-ordering-and-slippage`

## 17. L2 / sequencer

Sequencer downtime, L1-L2 alias, delayed inbox, custom gas token.

- Playbook: `reviewing-l2-sequencer-and-finality`

## 18. NFT / marketplace

Callback reentrancy, royalty bypass, signature listing replay.

- Playbook: `reviewing-nft-and-marketplace`

## 19. Cross-function composer

Two "safe" functions composed in one tx break an invariant.

- Playbook: `reviewing-cross-function-and-composer`

## 20. Client / node (non-Solidity)

P2P, consensus, RPC, memory safety on Go/Rust/C++ clients.

- Playbook: `auditing-blockchain-clients` (or DarkNavy `/client-auditor` if installed)

---

## 21+. Historical + non-EVM (do not skip)

The 20 rows above are the **EVM DeFi** walk. They will not catch Wormhole's Solana sysvar spoof, Dragonberry IAVL, Sui shared-object ACL, or Vesu Cairo rounding.

Load **one** file from `historical-smart-contract-vulns`:

| Need | File |
|------|------|
| Paid Immunefi / X bounty -> playbook | `hunting-x-linked-bounties` (one slice) |
| Class id T01-O04 | `references/taxonomy.md` |
| Ingest T-id vs pack id | `references/ingest-class-map.md` |
| Paid Immunefi / contest $ (deep cards) | `references/notable-bounties.md` |
| Loss-side timeline | `references/incident-timeline.md` |
| Rekt-scale named losses | `references/named-losses.md` |
| 858 public incident names | `references/incident-corpus.md` |
| zachxbt / tayvano_ / Bybit / drainers | `references/investigator-ops.md` then `reviewing-frontend-and-ops-surfaces` |
| Which chain / VM | `references/chain-catalog.md` |
| Solana | `references/solana.md` then `reviewing-solana-programs` |
| Move / IBC / Cairo | `references/move-cosmos-cairo.md` |
| Bitcoin-adjacent / other VMs | `references/bitcoin-and-other-vms.md` |

Minimum extra walk when the repo is not vanilla Ethereum:

| # | Class | Playbook |
|---|-------|----------|
| 21 | L2 sequencer / alias / retryable / zk verifier | `reviewing-l2-sequencer-and-finality` |
| 22 | Bridge single-verifier / DVN / message spoof | `reviewing-bridges-and-messaging` |
| 23 | ERC-4337 paymaster / EIP-7702 | `reviewing-signatures-permit-and-eip712` |
| 24 | Uniswap v4 hooks | `reviewing-amm-and-cl-pools` |
| 25 | Solana signer / CPI / PDA / sysvar | `reviewing-solana-programs` |
| 26 | Move shared object / capability | `reviewing-move-modules` |
| 27 | IBC ICS-20/23 / ibc-hooks | `reviewing-cosmos-and-ibc` |
| 28 | Cairo `u256` rounding | `reviewing-cairo-and-starknet` |
| 29 | Bitcoin Script / Lightning / Stacks | `reviewing-bitcoin-adjacent` |
| 30 | Ops vs code (Bybit Safe UI, drainers, Permit2 phishing, keys) | `reviewing-frontend-and-ops-surfaces` + `investigator-ops.md` |

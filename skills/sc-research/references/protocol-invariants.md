# Conservation laws by protocol type

Write these in English first, then as `invariant_*` / Echidna properties. Handler-based. Local only.

## Vault / ERC-4626

- `totalAssets() >= convertToAssets(totalSupply())` (or documented rounding)
- empty-vault `convertToShares(assets)` is linear after virtual offset
- only `deposit`/`mint` increase shares; only `withdraw`/`redeem` decrease
- donation of underlying does not mint extra shares to the donor beyond the documented fee

## AMM / CPMM

- `x * y >= k` (or the pool's actual invariant, including fees)
- reserves match token balances minus unclaimed fees (if the design says so)
- no negative liquidity; empty-pool path reverts or is defined

## CL / Uniswap v3-style

- liquidity net at a tick matches positions that cross it
- fee growth does not overflow position owed tokens in a way that steals others
- callback caller is the pool (`msg.sender == pool`)

## Lending

- `sum(collateral * price * ltv) >= sum(debt)` for solvent accounts
- seized collateral vs repaid debt matches the bonus, never more than collateral
- interest index is monotonic
- market cannot be insolvent solely from a one-block oracle spike if the design forbids it

## Staking / rewards

- reward index only increases
- `claimed <= earned` per user
- leftover dust cannot be stolen via `notifyRewardAmount(0)` games

## Bridge

- minted on destination <= locked/burned on source (plus documented fees)
- each message id processed at most once
- decoder cannot inflate amount or redirect recipient

## Governor / timelock

- execute only after delay
- eta/queue hash matches the payload
- cancelled ops cannot execute
- flashloan-vote cannot pass if votes are snapshotted

## Proxy

- implementation slot is ERC-1967
- only authorized can change it
- initialize runs exactly once per version

Playbook: `writing-foundry-invariant-handlers`

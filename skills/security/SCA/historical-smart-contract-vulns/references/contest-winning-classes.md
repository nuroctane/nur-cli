# Contest-winning classes (C4 / Sherlock / Cantina / Hats / CodeHawks)

What **wardens get paid for in contests** is not identical to what **hackers steal**. Contests overweight logic, rounding, and "one function away" bugs. 2022 hacks overweight bridges and keys.

Use this list as a **review order** on in-scope Solidity. Pair with Solodit (`how-to-query.md`). Paid Immunefi analogs for the same classes: `hunting-x-linked-bounties`.

## Still winning (walk these every contest)

| Rank | Class | Why it still lands | Taxonomy | Playbook |
|------|-------|--------------------|----------|----------|
| 1 | Rounding / precision / `mulDiv` direction | Fee, interest, shares; 1 wei loops | T04 | vaults + invariants |
| 2 | ERC-4626 inflation / donation / first depositor | Empty vault, virtual offset missing | T06/T36 | `reviewing-erc4626-and-vaults` |
| 3 | Access control / `initialize` / role mix-up | One missing modifier | T02/T12 | access + proxies |
| 4 | Oracle: spot, stale, sequencer, wrong decimals | Copy-paste Chainlink without L2 grace | T05/T26 | oracles |
| 5 | Signature: nonce, deadline, chainId, `ecrecover(0)` | Permit + intent + 712 | T10/T11 | signatures |
| 6 | Fee-on-transfer / rebasing / missing return | Hardcoded `amount` vs `balanceOf` delta | T23/T24 | tokens |
| 7 | Read-only reentrancy / Curve LP as oracle | `get_virtual_price` mid-callback | T08 | reentrancy |
| 8 | Callback spoof (Uni v2/v3/v4 hooks) | `msg.sender` is "a pool" | T16 | AMM |
| 9 | Liquidation bonus / bad debt / empty market | Sonne-class donation into empty market | T14 | lending |
| 10 | L2 sequencer / alias / retryable | Forgotten on OP/Arb copies | T26-T28 | L2 |
| 11 | Bridge: replay, trusted remote, amount decode | Every LZ/OP/CCIP OApp | T17-T20 | bridges |
| 12 | Cross-function composer | Two audited fns, one tx | T33 | composer |
| 13 | Proxy storage / ERC-7201 / missing `_authorizeUpgrade` | | T12/T13 | proxies |
| 14 | Unchecked / silent `call` | | T37 | access |
| 15 | `abi.encodePacked` hash collision | | T38 | signatures |
| 16 | NFT callback / royalty bypass | | T32 | NFT |
| 17 | Governance delay / flash votes | | T21/T22 | gov |
| 18 | MEV: missing `minOut` / deadline | Often Medium, still valid | T25 | MEV |
| 19 | Uniswap v4 hook trust | Hook can steal if host trusts return data | T16 | AMM |
| 20 | ERC-4337 validation / paymaster | 2025-2026 contests | T39 | signatures |

## Compiler / language landmines (contest + historical)

| Landmine | When to load |
|----------|----------------|
| Vyper 0.2.15-0.3.0 reentrancy | Any Vyper pool (T35). Curve 2023 analog |
| Solidity `<0.8` overflow | Legacy; still in forks |
| `tx.origin` auth | T02 |
| `transfer()` 2300 gas | griefing, not always Critical |
| `extcodesize == 0` constructor bypass | T02 |
| `delegatecall` to user-controlled target | T34 |

## What contests under-weight (do not skip)

- **Config**: LayerZero DVN set of size 1 (KelpDAO 2026 analog). Not a missing `nonReentrant`.
- **Ops**: admin keys, Safe modules, DNS (CoW 2026 $1.2M domain hijack - internal funds).
- **Clients**: Firedancer / geth / reth comps pay; `auditing-blockchain-clients`.
- **Non-EVM**: Solana account checks, IBC proofs, Cairo rounding. Separate playbooks.

## Severity heuristic (Immunefi-shaped)

| Impact | Typical class |
|--------|----------------|
| Direct theft / unbacked mint | T12, T17-T20, T02 on `mint`, S01-S05 |
| Protocol insolvency | T04, T14, T01 |
| Theft of yield / MEV forced | T25, T01 rewards |
| Grief / temporary halt | M06, B03, T21 DoS |
| Gas grief | rarely Critical |

Do not inflate Medium slippage into Critical. Judges will.

## Solodit search seeds (copy)

```
"first depositor" OR inflation ERC4626
rounding OR "precision loss" vault
"read-only reentrancy"
sequencerUptimeFeed
"trusted remote" OR lzReceive
uniswapV3SwapCallback
"empty market" liquidation
"canonical bump"  (Solana reports on Solodit if any)
```

Then grep the **in-scope** tree. Known issues in the README are not findings.

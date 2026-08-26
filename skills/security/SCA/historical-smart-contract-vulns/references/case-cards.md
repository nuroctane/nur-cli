# Case cards (public incidents and paid bounties)

Eighty already-public cases extracted from the Grok 4.6 ingest. Each card has: year, chain, type, class, what was wrongly trusted, grep seeds, **local test idea** (not an exploit), source URL.

**Class ids in these cards are ingest labels.** Map through `ingest-class-map.md` (or the short table below) before filing:

| Ingest label (this file) | Pack id | Meaning |
|--------------------------|---------|---------|
| T01 access | T02 | missing auth |
| T02 uninit proxy | T12 | uninitialized impl |
| T04 signature | T10/T11 | replay / ecrecover(0) |
| T06 oracle spot | T05 | AMM/spot as price |
| T13/T14 inflation | T06/T36 | 4626 / empty market |
| T16/T18 reentrancy | T07/T08 | value / read-only |
| T20 callback spoof | T16 | Uni callback |
| T21 tick | T15 / S09 | CL math |
| T25 donate/liq | T14 | Euler-class |
| T26/T27 message/proof | T17-T20 / C02 | bridge / IAVL |
| T28 keys | ops / T20 | validator/guardian |
| T29 flash gov | T22 | Beanstalk-class |
| T31 arbitrary call | T34 | router calldata |
| T33 rounding | T04 | mulDiv |
| T37 Vyper | T35 | compiler lock |
| T38 client/OVM | O04 / T27 | l2geth / Aurora |
| T39 zk proof | T31 | verifier |
| T40-T42 Solana | S01-S05 | account / sysvar |
| T43-T44 Move | M01-M07 | shared object / overflow |
| T45 IBC | C01-C05 | ICS-20/23 |
| T46 Bitcoin-adj | B01-B03 | Clarity / Script |
| T47 Cairo | K01-K02 | felt / L1 handler |

Do not load this file together with `incident-corpus.md`. Pick cards that match the in-scope protocol type.

Each card: name, year, chain/VM, protocol type, bug class, what was wrongly trusted, grep seeds, local test idea, sources.

### 7.1 Wormhole uninitialized proxy (bounty)

- **Year / chain / type:** 2022; Ethereum; bridge / UUPS
- **Class:** T02
- **Trusted:** Uninitialized implementation; `initialize` sets owner
- **Grep:** `initialize(`, `upgradeTo`, `UUPSUpgradeable`, `_authorizeUpgrade`
- **Test idea:** Deploy impl; as attacker call `initialize` then assert owner != attacker (should already be initialized or initializer disabled)
- **Sources:** https://immunefi.com/blog/bug-fix-reviews/wormhole-uninitialized-proxy-bugfix-review/

### 7.2 Wormhole Solana sysvar (hack)

- **Year / chain / type:** 2022; Solana program; bridge signatures
- **Class:** T42
- **Trusted:** Instructions sysvar account identity without pubkey check (`load_instruction_at`)
- **Grep:** `load_instruction_at`, `Sysvar1nstructions`, `verify_signatures`
- **Test idea:** Anchor test: pass a dummy account as sysvar; instruction must return error after `load_instruction_at_checked`
- **Sources:** https://rekt.news/wormhole-rekt ; https://neodyme.io/en/blog/solana_common_pitfalls

### 7.3 Aurora infinite spend (bounty)

- **Year / chain / type:** 2022; Aurora engine / NEAR
- **Class:** T38 / T26
- **Trusted:** ExitToNear callee identity under DELEGATECALL; ETH value from caller
- **Grep:** `ExitToNear`, `DELEGATECALL`, `STATICCALL`, exit events
- **Test idea:** Engine test: DELEGATECALL exit must not credit unmatched address
- **Sources:** https://immunefi.com/blog/bug-fix-reviews/aurora-inflation-spend-bugfix-review-6m-payout/ ; https://blog.offside.io/p/how-did-i-save-70000-eth-and-win-6-million-bug-b

### 7.4 Optimism SELFDESTRUCT inflation (bounty)

- **Year / chain / type:** 2022; OP Stack l2geth
- **Class:** T38
- **Trusted:** Zeroing `stateObject` balance instead of `OVM_ETH`
- **Grep:** `SELFDESTRUCT`, `UsingOVM`, `OVM_ETH`
- **Test idea:** Client regression: selfdestruct must zero OVM_ETH mapping
- **Sources:** https://github.com/ethereum-optimism/optimism/blob/124b3c42378cc2dd01333f3084c057c9691360ec/technical-documents/postmortems/2022-02-02-inflation-vuln.md

### 7.5 Polygon Plasma branchMask (bounty)

- **Year / chain / type:** 2021; Polygon Plasma
- **Class:** T27 / T04
- **Trusted:** Encoded branch mask uniqueness; first byte dropped
- **Grep:** `branchMask`, `exitId`, `WithdrawManager`
- **Test idea:** Two masks that collide on exit id must not both succeed
- **Sources:** https://immunefi.com/blog/bug-fix-reviews/polygon-double-spend-bugfix-review-2m-bounty/

### 7.6 Euler donateToReserves (hack)

- **Year / chain / type:** 2023; Ethereum; lending
- **Class:** T25
- **Trusted:** Donate path without remaining liquidity; self-liquidation discount
- **Grep:** `donateToReserves`, `liquidate`, `violator`
- **Test idea:** After donate brings deposits near 0, liquidate must not extract more than remaining assets
- **Sources:** https://rekt.news/euler-rekt

### 7.7 Nomad trusted root (hack)

- **Year / chain / type:** 2022; Ethereum; messaging bridge
- **Class:** T26 / T27
- **Trusted:** Uninitialized / zero Merkle root as valid; replica `acceptableRoot`
- **Grep:** `acceptableRoot`, `process`, `confirmAt`, `0x00`
- **Test idea:** Message with unproven root must revert; zero root not accepted
- **Sources:** https://rekt.news/nomad-rekt

### 7.8 BNB IAVL (hack)

- **Year / chain / type:** 2022; BNB Beacon / BSC
- **Class:** T27
- **Trusted:** IAVL RangeProof verification
- **Grep:** `IAVL`, `RangeProof`, `SimpleValueOp`
- **Test idea:** Client unit test: mutated proof fails (use upstream Cosmos tests)
- **Sources:** https://rekt.news/bnbbridge-rekt ; https://blog.verichains.io/p/vsa-2022-101-iavl-spoofing-attack

### 7.9 Ronin validator keys (hack)

- **Year / chain / type:** 2022; Ronin
- **Class:** T28
- **Trusted:** 5-of-9 validator signatures (centralized operators)
- **Grep:** `validator`, `threshold`, guardian set
- **Test idea:** N-1 signatures cannot mint; document operator diversity (process test)
- **Sources:** https://rekt.news/ronin-rekt

### 7.10 Poly Network (hack)

- **Year / chain / type:** 2021; multi-chain
- **Class:** T01 / T26
- **Trusted:** Keepers / `putCurEpochConPubKeyBytes`-style privileged path (public analyses)
- **Grep:** keeper, epoch pubkey, `verifyHeader`
- **Test idea:** Unauthorized keeper method reverts
- **Sources:** https://rekt.news/poly-network-rekt

### 7.11 Beanstalk flash governance (hack)

- **Year / chain / type:** 2022; Ethereum; stablecoin DAO
- **Class:** T29
- **Trusted:** Emergency commit + current-balance votes
- **Grep:** `emergencyCommit`, `getVotes`, `BIP`
- **Test idea:** Flash-loaned tokens cannot pass if snapshot is at proposal start
- **Sources:** https://rekt.news/beanstalk-rekt ; Immunefi Top 10 V10

### 7.12 Harvest Finance (hack)

- **Year / chain / type:** 2020; Ethereum; yield
- **Class:** T06
- **Trusted:** Curve virtual price / spot in same tx as deposit
- **Grep:** `getPricePerFullShare`, Curve `get_virtual_price`
- **Test idea:** Sandwich deposit vs manipulated virtual price; share mint bounded
- **Sources:** https://rekt.news/harvest-finance-rekt

### 7.13 Cream AMP reentrancy (hack)

- **Year / chain / type:** 2021; Ethereum; lending
- **Class:** T16
- **Trusted:** ERC-777 `tokensReceived` during borrow/liquidate
- **Grep:** `tokensReceived`, `seize`, AMP
- **Test idea:** Hook cannot reenter before exchange rate updates
- **Sources:** https://rekt.news/cream-rekt

### 7.14 Cream yUSD oracle (hack)

- **Year / chain / type:** 2021; Ethereum
- **Class:** T06 / T23
- **Trusted:** yUSD vault as collateral price
- **Grep:** `getUnderlyingPrice`, yVault
- **Test idea:** Donate/inflate vault share price; borrow cap vs true value
- **Sources:** https://rekt.news/cream-rekt-2

### 7.15 Qubit QBridge (hack)

- **Year / chain / type:** 2022; BSC
- **Class:** T26
- **Trusted:** Deposit event / proof of ETH deposit on BSC
- **Grep:** `deposit`, `ethDeposit`, `xqETH`
- **Test idea:** Fake deposit proof cannot mint
- **Sources:** https://rekt.news/qubit-rekt

### 7.16 Harmony Horizon (hack)

- **Year / chain / type:** 2022; Harmony
- **Class:** T28
- **Trusted:** 2-of-5 multisig
- **Grep:** n/a (keys)
- **Test idea:** Threshold invariant in tests; ops review
- **Sources:** https://rekt.news/harmony-rekt

### 7.17 Mango Markets (hack)

- **Year / chain / type:** 2022; Solana; perps
- **Class:** T06
- **Trusted:** Illiquid MNGO spot as mark
- **Grep:** oracle, `MNGO`, perp health
- **Test idea:** Isolated oracle test: mark move without inventory does not increase withdrawable
- **Sources:** https://rekt.news/mango-rekt

### 7.18 Cashio (hack)

- **Year / chain / type:** 2022; Solana; stablecoin
- **Class:** T40
- **Trusted:** Collateral token mint / Arrow account chain
- **Grep:** `crate_collateral`, bank, mint field
- **Test idea:** Unrelated mint as collateral must fail
- **Sources:** https://www.certik.com/blog/cashio-app-incident-analysis

### 7.19 Curve Vyper (hack)

- **Year / chain / type:** 2023; Ethereum; AMM
- **Class:** T37
- **Trusted:** Vyper `@nonreentrant` lock across compiler versions
- **Grep:** `@nonreentrant`, vyper version pin
- **Test idea:** Reenter `add_liquidity`/`remove_liquidity` in one tx; must revert on patched compiler
- **Sources:** https://rekt.news/vyper-rekt

### 7.20 KyberSwap CL (hack)

- **Year / chain / type:** 2023; multi; CL AMM
- **Class:** T21 / T33
- **Trusted:** Tick computation / double-count liquidity
- **Grep:** `tick`, `computeSwap`, `liquidity`
- **Test idea:** Extreme after-swap tick; liquidity net must match
- **Sources:** https://rekt.news/kyberswap-rekt

### 7.21 BonqDAO (hack)

- **Year / chain / type:** 2023; Polygon; CDP
- **Class:** T06
- **Trusted:** Tellor last reported as spot; WALBT
- **Grep:** `tellor`, `getCurrentValue`, `lastPrice`
- **Test idea:** Single update cannot jump collateral value unbounded
- **Sources:** https://rekt.news/bonq-rekt ; Immunefi Top 10 V03

### 7.22 Platypus USP (hack)

- **Year / chain / type:** 2023; Avalanche
- **Class:** T24
- **Trusted:** USP solvency / emergency withdraw
- **Grep:** `USP`, `emergency`, `solvency`
- **Test idea:** Underwater USP cannot withdraw full LP
- **Sources:** https://rekt.news/platypus-rekt

### 7.23 Conic read-only reentrancy (hack)

- **Year / chain / type:** 2023; Ethereum
- **Class:** T18
- **Trusted:** `get_virtual_price` during Curve remove
- **Grep:** `get_virtual_price`, `remove_liquidity`
- **Test idea:** View price during callback not used as oracle
- **Sources:** https://rekt.news/conic-rekt

### 7.24 Sturdy read-only reentrancy (hack)

- **Year / chain / type:** 2023; Ethereum; lending
- **Class:** T18
- **Trusted:** Balancer / Curve view during callback
- **Grep:** `manageUserBalance`, `getRate`
- **Test idea:** Same as Conic
- **Sources:** https://rekt.news/sturdy-rekt

### 7.25 Hundred ERC-4626 inflation (hack)

- **Year / chain / type:** 2023; Ethereum
- **Class:** T13 / T14
- **Trusted:** Empty market exchangeRate
- **Grep:** `exchangeRateStored`, `mint`, `donate`
- **Test idea:** 1 wei mint + donate; next mint shares != 0
- **Sources:** https://rekt.news/hundred-rekt

### 7.26 Sonne empty market (hack)

- **Year / chain / type:** 2024; Optimism; Compound fork
- **Class:** T14
- **Trusted:** Empty cToken
- **Grep:** same as Hundred
- **Test idea:** same
- **Sources:** https://rekt.news/sonne-finance-rekt

### 7.27 Raft donate inflation (hack)

- **Year / chain / type:** 2023; Ethereum
- **Class:** T14
- **Trusted:** Position NFT / index after donate
- **Grep:** `deposit`, `index`, `R`
- **Test idea:** Donate then open position; collateral value bounded
- **Sources:** https://rekt.news/raft-rekt

### 7.28 Exactly permit (hack)

- **Year / chain / type:** 2023; Optimism
- **Class:** T04
- **Trusted:** Permit on debt/credit
- **Grep:** `permit`, `borrow`, `leverage`
- **Test idea:** Replay / wrong nonce fails
- **Sources:** https://rekt.news/exactly-protocol-rekt

### 7.29 Jimbo's Protocol (hack)

- **Year / chain / type:** 2023; Arbitrum; floor AMM
- **Class:** T06 / T21
- **Trusted:** Floor price interaction with Uniswap
- **Grep:** `floor`, `shift`, `bin`
- **Test idea:** Flash shift cannot drain treasury
- **Sources:** https://rekt.news/jimbos-protocol-rekt

### 7.30 Sushi router 2023 (hack)

- **Year / chain / type:** 2023; Ethereum; router
- **Class:** T31
- **Trusted:** User-supplied `to` / route
- **Grep:** `route`, `swap`, `to`
- **Test idea:** Attacker `to` cannot take user funds beyond msg.sender
- **Sources:** https://rekt.news/leaderboard ; DHL 20230409 SushiSwap

### 7.31 Socket gateway (hack)

- **Year / chain / type:** 2024; multi; aggregator
- **Class:** T31
- **Trusted:** Calldata + approvals
- **Grep:** `execute`, `userData`, `call(`
- **Test idea:** Arbitrary call cannot `transferFrom` leftover allowance
- **Sources:** https://rekt.news/leaderboard ; DHL 20240112 SocketGateway

### 7.32 LiFi / Jumper (hack)

- **Year / chain / type:** 2024; multi
- **Class:** T31
- **Trusted:** Same as Socket
- **Grep:** `swapAndStartBridgeTokensVia*`, packed calls
- **Test idea:** Same
- **Sources:** https://rekt.news/leaderboard

### 7.33 Radiant precision (hack)

- **Year / chain / type:** 2024; Arbitrum; Aave fork
- **Class:** T33
- **Trusted:** Ray math / totalSupply edge
- **Grep:** `rayMul`, `liquidityIndex`
- **Test idea:** Tiny amounts; index cannot teleport
- **Sources:** https://rekt.news/leaderboard ; DHL 20240102

### 7.34 Radiant II keys (hack)

- **Year / chain / type:** 2024; multi
- **Class:** T28 ops
- **Trusted:** Compromised signer machines
- **Grep:** n/a
- **Test idea:** Process: hardware isolation (not Foundry)
- **Sources:** https://rekt.news/leaderboard

### 7.35 Uwulend oracle (hack)

- **Year / chain / type:** 2024; Ethereum; Aave fork
- **Class:** T06
- **Trusted:** Manipulable pool as oracle
- **Grep:** `getAssetPrice`, `source`
- **Test idea:** Flash-swap source pool; LTV check fails
- **Sources:** https://rekt.news/uwulend-rekt

### 7.36 WooFi sPMM (hack)

- **Year / chain / type:** 2024; multi
- **Class:** T06
- **Trusted:** Woo oracle / sPMM
- **Grep:** `Wooracle`, `priceNow`
- **Test idea:** Stale or single-source price rejected
- **Sources:** https://rekt.news/woofi-rekt

### 7.37 Penpie reentrancy (hack)

- **Year / chain / type:** 2024; Ethereum; Pendle
- **Class:** T16
- **Trusted:** Callback during batch harvest / mint
- **Grep:** `harvest`, `pendle`, `nonReentrant`
- **Test idea:** Reenter deposit during harvest
- **Sources:** https://rekt.news/leaderboard ; DHL 20240903

### 7.38 Cetus integer-mate (hack)

- **Year / chain / type:** 2025; Sui; CL AMM
- **Class:** T44
- **Trusted:** Overflow check in shared math lib
- **Grep:** `checked_shlw`, `integer-mate`, overflow
- **Test idea:** Move unit test: overflowing shift-mul aborts
- **Sources:** SlowMist 2025 report; rekt leaderboard Cetus

### 7.39 Balancer V2 2025 precision (hack)

- **Year / chain / type:** 2025; Ethereum / L2s
- **Class:** T33
- **Trusted:** Composable stable / rounding
- **Grep:** `mulDown`, `divDown`, `amp`
- **Test idea:** Tiny join/exit; pool invariant
- **Sources:** DHL 20251103 ; rekt leaderboard

### 7.40 GMX 2025 share price (hack)

- **Year / chain / type:** 2025; Arbitrum
- **Class:** T15
- **Trusted:** GLP/GM share valuation
- **Grep:** `getAum`, `price`
- **Test idea:** Donate / rebalance cannot mint free GM
- **Sources:** DHL 20250709 ; rekt

### 7.41 Multichain (hack)

- **Year / chain / type:** 2023; multi
- **Class:** T28
- **Trusted:** MPC operators / CEO key narrative
- **Grep:** n/a
- **Test idea:** Ops, not unit test
- **Sources:** https://rekt.news/multichain-rekt-2

### 7.42 Wintermute Gnosis Safe L2 (hack)

- **Year / chain / type:** 2022; Optimism
- **Class:** T02 / T09
- **Trusted:** Safe singleton initialized on L1 only
- **Grep:** `setup(`, `GnosisSafe`, `masterCopy`
- **Test idea:** Fresh L2 Safe must not be takeable via `setup`
- **Sources:** https://rekt.news/leaderboard

### 7.43 Audius storage collision (hack)

- **Year / chain / type:** 2022; Ethereum; governance
- **Class:** T03 / T29
- **Trusted:** Proxy storage vs initializer
- **Grep:** `initialize`, governance proxy
- **Test idea:** Storage dump: voting power slot not aliasable
- **Sources:** https://rekt.news/audius-rekt

### 7.44 TempleDAO (hack)

- **Year / chain / type:** 2022; Ethereum
- **Class:** T01
- **Trusted:** Stale contract auth / migrate
- **Grep:** `stale`, `migrate`, `onlyOwner`
- **Test idea:** Old vault cannot mint
- **Sources:** https://rekt.news/templedao-rekt

### 7.45 Deus (multiple)

- **Year / chain / type:** 2021-2023; Ethereum / Fantom
- **Class:** T06 then T35 DEI
- **Trusted:** DEI/DEUS oracles then mint logic
- **Grep:** `DEI`, `oracle`
- **Test idea:** Spot cannot mint unbounded DEI
- **Sources:** rekt Deus series

### 7.46 Inverse Finance (hack)

- **Year / chain / type:** 2022; Ethereum
- **Class:** T06
- **Trusted:** Low-liq INV TWAP/spot
- **Grep:** `INV`, `anchor`, `oracle`
- **Test idea:** 50x spot move cannot 10x borrow
- **Sources:** https://rekt.news/inverse-finance-rekt

### 7.47 Lodestar plvGLP (hack)

- **Year / chain / type:** 2022; Arbitrum
- **Class:** T06 / T15
- **Trusted:** `donate()` on GlpDepositor in oracle
- **Grep:** `donate`, `GLPOracle`, `plvGLP`
- **Test idea:** Donate does not change oracle price
- **Sources:** https://rekt.news/lodestar-rekt

### 7.48 Fei / Rari reentrancy (hack)

- **Year / chain / type:** 2022; Ethereum
- **Class:** T16
- **Trusted:** ETH send in fuse pool
- **Grep:** `exitPool`, `receive(`
- **Test idea:** Reenter before borrow settled
- **Sources:** https://rekt.news/fei-rari-rekt

### 7.49 Redacted custom approval (bounty)

- **Year / chain / type:** 2022; Ethereum
- **Class:** T05
- **Trusted:** Custom approve semantics
- **Grep:** `approve`, `allowance`, `transferFrom`
- **Test idea:** Allowance decrease matches ERC-20
- **Sources:** https://medium.com/immunefi/redacted-cartel-custom-approval-logic-bugfix-review-9b2d039ca2c5

### 7.50 Notional free collateral (bounty)

- **Year / chain / type:** 2022; Ethereum; fixed rate
- **Class:** T35
- **Trusted:** Double-counted fCash / collateral
- **Grep:** `freeCollateral`, `fCash`
- **Test idea:** Account FC cannot exceed true assets
- **Sources:** https://medium.com/immunefi/notional-double-counting-free-collateral-bugfix-review-28b634903934

### 7.51 Balancer rounding bounty (2023)

- **Year / chain / type:** 2023; Ethereum
- **Class:** T33
- **Trusted:** Rounding direction in swaps
- **Grep:** `mulDown`, `divUp`
- **Test idea:** 1 wei swap does not extract value
- **Sources:** https://medium.com/immunefi/balancer-rounding-error-bugfix-review-cbf69482ee3d

### 7.52 DFX EURS rounding (bounty)

- **Year / chain / type:** ~2023; Ethereum
- **Class:** T33
- **Trusted:** 2-decimal EURS in assimilator
- **Grep:** `Assimilator`, `numeraire`, `decimals`
- **Test idea:** Deposit of dust mints 0 curve tokens (must revert)
- **Sources:** https://medium.com/immunefi/dfx-finance-rounding-error-bugfix-review-17ba5ffb4114

### 7.53 Raydium ticks (bounty)

- **Year / chain / type:** 2024; Solana
- **Class:** T21
- **Trusted:** Tick array / price
- **Grep:** `tick`, `tick_array`
- **Test idea:** Out-of-range tick cannot mint free tokens
- **Sources:** https://medium.com/immunefi/raydium-tick-manipulation-bugfix-review-c6aae4527ed6

### 7.54 Scroll message spoof (bounty)

- **Year / chain / type:** 2025; Scroll
- **Class:** T26
- **Trusted:** Bridge message identity
- **Grep:** `onDropMessage`, `relayMessage`, `xDomain`
- **Test idea:** Spoofed messenger sender reverts
- **Sources:** https://forum.scroll.io/t/report-scroll-mainnet-emergency-upgrade-on-2025-04-25/666

### 7.55 zkSync Lite proof (bounty)

- **Year / chain / type:** 2023-24; zkSync Lite
- **Class:** T39
- **Trusted:** Proof verification completeness
- **Grep:** `verifyProof`, public inputs
- **Test idea:** Random proof rejected
- **Sources:** https://medium.com/immunefi/zksync-insufficient-proof-verification-bugfix-review-dcd57944d0e2

### 7.56 zkSync Era soundness (bounty)

- **Year / chain / type:** 2023; zkEVM
- **Class:** T39
- **Trusted:** Circuit / compiler
- **Grep:** circuit constraints (not Solidity)
- **Test idea:** ChainLight writeup reproduction in **local** zkVM tests
- **Sources:** https://medium.com/chainlight/uncovering-a-zk-evm-soundness-bug-in-zksync-era-f3bc1b2a66d8

### 7.57 saurik / riptide / pwning.eth cluster

See bounty table. Additional: Moonbeam truncation, Frontier EVM, Interlay - all **client/VM** not app Solidity. Playbook: `auditing-blockchain-clients`.

### 7.58 Across V3 (bounty, Obront / deadroses)

- **Year / chain / type:** 2025; Ethereum + L2s; canonical bridge
- **Class:** T26
- **Trusted:** Fill / relay identity
- **Grep:** `fillV3Relay`, `depositId`, `originChainId`
- **Test idea:** Replay fill on wrong chain fails
- **Sources:** https://mirror.xyz/0x9D6b7f5e8d1b9dFea8dDD29c0DbD81687e721601/mrt70ckjaZymv9keUy_TzHVIzjBOQr-Hx_KI1ydFeoQ

### 7.59 Axelar halt (bounty)

- **Year / chain / type:** 2025; Axelar
- **Class:** T26 / T52
- **Trusted:** Gateway / validator set handling
- **Grep:** chain-specific
- **Test idea:** Malformed packet cannot halt (or is explicitly DoS-scoped)
- **Sources:** https://marcotnunes.com/axelar-network-cross-chain-halt-vulnerability/

### 7.60 Sui shutdown (bounty)

- **Year / chain / type:** 2025; Sui
- **Class:** T52
- **Trusted:** Runtime / tx limits
- **Grep:** Move runtime
- **Test idea:** Pathological tx rejected without halt
- **Sources:** https://immunefi.com/blog/bug-fix-reviews/sui-network-shutdown/

### 7.61 Story Network (bounty)

- **Year / chain / type:** 2025; Story
- **Class:** T26 / client
- **Trusted:** (see official postmortem)
- **Grep:** per postmortem
- **Test idea:** Follow official regression tests
- **Sources:** https://www.story.foundation/blog/story-network-postmortem

### 7.62 Alchemix V3 liquidation fee (contest)

- **Year / chain / type:** 2025; Ethereum; Immunefi audit comp
- **Class:** T24 / T35
- **Trusted:** `_resolveRepaymentFee` return vs collateral debit
- **Grep:** `_resolveRepaymentFee`, `_liquidate`, `repaymentFee`
- **Test idea:** Underwater position: fee transferred == fee debited
- **Sources:** https://reports.immunefi.com/alchemix-v3/58772-sc-critical-resolverepaymentfee-overpays-liquidators-when-collateral-is-gone-letting-attackers

### 7.63 Sherlock ERC-4626 inflation (contest class)

- **Year / chain / type:** 2024-2025; many contests
- **Class:** T13
- **Trusted:** `totalAssets` = `balanceOf`
- **Grep:** `_decimalsOffset`, `virtualShares`, `convertToShares`
- **Test idea:** Standard OZ inflation test
- **Sources:** https://github.com/sherlock-audit/2024-01-napier-judging/issues/125 ; Burve issues 219/335 ; Notional Exponent 622

### 7.64 Uranium k=1000 (hack)

- **Year / chain / type:** 2021; BSC; Uniswap fork
- **Class:** T35
- **Trusted:** `k` constant 1000 vs 10000
- **Grep:** `1000`, `10000`, `kLast`
- **Test idea:** Swap does not violate x*y=k with the **intended** fee
- **Sources:** https://rekt.news/uranium-rekt

### 7.65 Paid Network mint (hack)

- **Year / chain / type:** 2021; Ethereum
- **Class:** T01
- **Trusted:** Public mint
- **Grep:** `mint(`, `onlyOwner`
- **Test idea:** Random address cannot mint
- **Sources:** https://rekt.news/paid-rekt

### 7.66 Furucombo aTo (hack)

- **Year / chain / type:** 2021; Ethereum; aggregator
- **Class:** T31 / T02
- **Trusted:** `aTo` delegatecall target
- **Grep:** `delegatecall`, `aTo`
- **Test idea:** Whitelist of targets
- **Sources:** https://rekt.news/furucombo-rekt

### 7.67 Pickle jar (hack)

- **Year / chain / type:** 2020; Ethereum
- **Class:** T31
- **Trusted:** Crafted jar strategy
- **Grep:** `swapExactJarForJar`, `strategy`
- **Test idea:** Fake jar cannot drain
- **Sources:** https://rekt.news/pickle-rekt

### 7.68 Origin OUSD (hack)

- **Year / chain / type:** 2020; Ethereum
- **Class:** T16
- **Trusted:** Vault rebase + Uniswap callback
- **Grep:** `rebase`, `mint`
- **Test idea:** Callback cannot mint OUSD unbounded
- **Sources:** https://rekt.news/leaderboard

### 7.69 bZx 2020-2021 (hacks)

- **Year / chain / type:** Ethereum; lending
- **Class:** T06 / T23
- **Trusted:** Kyber/Uniswap spot, then later iToken
- **Grep:** `getPrice`, `iToken`
- **Test idea:** Same-tx price != TWAP for collateral
- **Sources:** https://rekt.news/bzx-rekt

### 7.70 The DAO / Parity (historical)

- **Year / chain / type:** 2016-2017; Ethereum
- **Class:** T16 / T02
- **Trusted:** Split DAO reentrancy; library `initWallet` + `selfdestruct`
- **Grep:** `call.value`, `initWallet`, `kill`
- **Test idea:** Checks-effects-interactions; library init locked
- **Sources:** SWC registry; Ethereum wiki

### 7.71 zkLend (hack)

- **Year / chain / type:** 2025; Starknet
- **Class:** T47 / T33
- **Trusted:** Market math in Cairo
- **Grep:** Cairo market, `felt252`
- **Test idea:** Rounding cannot drain
- **Sources:** https://rekt.news/leaderboard

### 7.72 Vesu rounding (disclosure)

- **Year / chain / type:** 2025; Starknet
- **Class:** T33
- **Trusted:** Rounding convention
- **Grep:** per Vesu docs
- **Test idea:** Official disclosure regression
- **Sources:** https://docs.vesu.xyz/security/disclosures-report/rounding-convention-bug-disclosure

### 7.73 Hedera 2023

- **Year / chain / type:** 2023; Hedera
- **Class:** T01 / config
- **Trusted:** (rekt $515k)
- **Grep:** chain-specific
- **Test idea:** Read rekt + Hashio postmortem
- **Sources:** https://rekt.news/leaderboard

### 7.74 AlexLab Stacks

- **Year / chain / type:** 2024-2025; Stacks Clarity
- **Class:** T46
- **Trusted:** Clarity contract auth / AMM
- **Grep:** Clarity `asserts!`, `contract-caller`
- **Test idea:** Clarinet tests: unauthorized mint fails
- **Sources:** https://rekt.news/leaderboard

### 7.75 Astroport Neutron

- **Year / chain / type:** 2024; CosmWasm
- **Class:** T45
- **Trusted:** Pool messages
- **Grep:** `ExecuteMsg`, `instantiate`
- **Test idea:** CosmWasm multi-test: spoofed msg fails
- **Sources:** https://rekt.news/leaderboard

### 7.76 Ionic Mode

- **Year / chain / type:** 2025; Mode OP-stack; Compound fork
- **Class:** T14
- **Trusted:** Empty market (Sonne class)
- **Grep:** `exchangeRate`
- **Test idea:** Same as Sonne
- **Sources:** https://rekt.news/leaderboard

### 7.77 Super Sushi Samurai Blast

- **Year / chain / type:** 2024; Blast
- **Class:** T35
- **Trusted:** Self-transfer doubles balance
- **Grep:** `transfer`, `from == to`
- **Test idea:** Transfer to self does not increase supply
- **Sources:** https://rekt.news/leaderboard ; DHL 20240321 SSS

### 7.78 Munchables Blast

- **Year / chain / type:** 2024; Blast
- **Class:** T01 / insider
- **Trusted:** Deployer keys
- **Grep:** `onlyOwner`
- **Test idea:** Ops
- **Sources:** https://rekt.news/leaderboard

### 7.79 BALD Base

- **Year / chain / type:** 2023; Base
- **Class:** T01 insider
- **Trusted:** LP / owner
- **Grep:** `removeLiquidity`
- **Test idea:** Timelock on LP
- **Sources:** https://rekt.news/leaderboard

### 7.80 PrismaFi (hack)

- **Year / chain / type:** 2024; Ethereum; LSD CDP
- **Class:** T36 / T01
- **Trusted:** `zap` / `migrate` input
- **Grep:** `migrate`, `zap`
- **Test idea:** Fake zaps cannot drain
- **Sources:** https://rekt.news/leaderboard ; DHL 20240328

---


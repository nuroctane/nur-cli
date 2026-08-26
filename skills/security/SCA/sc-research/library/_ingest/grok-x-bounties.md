# Public DeFi / smart-contract incidents and bounty wins (whitehat ingest)

NurCLI `sc-research` library ingest. Lawful / already-public only. No exploit PoCs, no drain scripts, no unpublished 0-days, no step-by-step attacks against live systems.

**Purpose:** map disclosed Immunefi / Hats / contest findings and public postmortems onto Nur playbooks so a reviewer can pick a class, grep in-scope source, and write a **local** Foundry / Anchor / Move **test idea**.

**Method:** Immunefi bugfix reviews, Immunefi research posts, rekt.news leaderboard, DeFiHackLabs public incident index, Solodit-adjacent contest issues (Sherlock GitHub judging repos), firm blogs (BlockSec, SlowMist, PeckShield, Spearbit, Trail of Bits, OpenZeppelin, OtterSec, Zellic, Neodyme, Verichains, iosiro, Trust Security), and X posts that **link** those writeups.

**Amounts:** USD as published by the cited source at the time of the article. Prices move. Treat figures as order-of-magnitude, not accounting.

**Dedup:** one incident = one name. A bounty and a later exploit of a *different* bug in the same protocol are separate rows.

---

## 1. Executive taxonomy (50 classes -> Nur playbooks)

Contest-winning classes that keep showing up (C4 / Sherlock / Cantina / Spearbit / CodeHawks / Immunefi comps): oracle/spot, first-depositor donation, ERC-4626 inflation, precision / empty-market, access control, uninitialized proxy, signature replay / `ecrecover==0`, L2 sequencer uptime, fee-on-transfer, read-only reentrancy, Vyper `@nonreentrant` storage, arbitrary router calldata, missing `minOut`, wrong decimals, share-price donate.

| ID | Class | What is wrongly trusted | Playbook | Grep / review seeds | Local test idea (not an exploit) |
|----|--------|-------------------------|----------|---------------------|----------------------------------|
| T01 | Access control / missing auth | `msg.sender`, `tx.origin`, public `initialize`, missing `onlyRole` | `reviewing-access-control-and-auth` | `onlyOwner`, `onlyRole`, `tx.origin`, `initialize`, `upgradeTo` | `vm.prank(attacker)` on every state changer; expect revert |
| T02 | Uninitialized proxy / UUPS impl | Implementation not initialized; anyone becomes owner then `upgradeTo` / `selfdestruct` | `reviewing-upgradeable-proxies` | `initialize(`, `upgradeTo`, `_authorizeUpgrade`, `UUPS` | After deploy, call `initialize` on **implementation** address as attacker; assert it reverts or is already initialized |
| T03 | Storage collision / slot clash | Proxy layout vs impl; constructor vs `initialize`; Vyper vs Solidity layout | `reviewing-upgradeable-proxies` | `delegatecall`, `gap`, `storage`, constructor assignments | Compare storage dumps before/after upgrade; assert admin slot unchanged |
| T04 | Signature replay / malleability | EIP-712 without `chainId`/`nonce`/`deadline`; `ecrecover==address(0)`; packed encoding | `reviewing-signatures-permit-and-eip712` | `ecrecover`, `permit(`, `_hashTypedDataV4`, `nonce`, `abi.encodePacked` | Replay same sig twice; flip `s` high bit; empty sig -> address(0) |
| T05 | Permit / allowance confusion | Custom `approve` that ignores spender; Permit2 vs ERC-20 permit mix | `reviewing-signatures-permit-and-eip712` | `permit(`, `allowance`, `transferFrom` | Victim signed A; spender B should not move funds |
| T06 | Oracle spot / AMM reserves | `getReserves` / `slot0` as collateral price | `reviewing-oracles-and-pricing` | `getReserves`, `slot0`, `consult`, `twap`, `latestRoundData` | Same-tx reserve skew then borrow/liquidate; assert protocol rejects or bounds loss |
| T07 | Stale Chainlink / round | No `updatedAt` / `answeredInRound` check | `reviewing-oracles-and-pricing` | `latestRoundData`, `latestAnswer`, `updatedAt` | Warp time past heartbeat; assert borrow/liquidate reverts |
| T08 | L2 sequencer downtime | Oracle still readable while sequencer is down; no grace period | `reviewing-l2-sequencer-and-finality` | `sequencer`, `uptime`, `startedAt`, `GRACE_PERIOD` | Mock sequencer-down feed; assert liquidations paused |
| T09 | L1-L2 address aliasing | L1 sender used as L2 `msg.sender` without `undoL1ToL2Alias` | `reviewing-l2-sequencer-and-finality` | `xDomainMessageSender`, `applyL1ToL2Alias`, `alias` | Cross-domain message from aliased address must not match EOA |
| T10 | Delayed inbox / forced inclusion | Assuming sequencer always includes txs; ignoring force-inclusion delay | `reviewing-l2-sequencer-and-finality` | `forceInclusion`, `delayedInbox`, `sequencerInbox` | After downtime > delay, stale price path must still be gated |
| T11 | Custom gas token / fee token | Treating gas token as 18-dec ETH; paying fees from wrong asset | `reviewing-l2-sequencer-and-finality` | `gasToken`, `feeToken`, `decimals` | Unit test fee accounting in native vs ERC-20 gas token |
| T12 | Proof lag / fraud window | Bridging on soft finality; minting before challenge period | `reviewing-bridges-and-messaging` | `finalize`, `challengePeriod`, `outputRoot` | Cannot withdraw on L1 before configured delay |
| T13 | ERC-4626 first depositor / donation | `totalAssets` from `balanceOf`; 1 wei shares then donate | `reviewing-erc4626-and-vaults` | `convertToShares`, `totalAssets`, `decimalsOffset`, `virtual` | First depositor 1 wei + donate; next depositor must not mint 0 shares |
| T14 | Empty-market / exchangeRate rounding | Compound-style `exchangeRate = cash/shares` when shares tiny | `reviewing-lending-and-liquidations` | `exchangeRate`, `accrueInterest`, `totalSupply` | Donate to empty cToken; assert borrow against inflated collateral fails |
| T15 | Share-price / NAV inflation | Vault NAV reads spot or donatable balance | `reviewing-erc4626-and-vaults` | `totalAssets`, `getPricePerFullShare`, `pps` | Donate underlying; previewDeposit must not jump unbounded |
| T16 | Classic reentrancy | External call before state update (ETH / ERC-777 / ERC-721) | `reviewing-reentrancy-and-callbacks` | `call{value`, `safeTransfer`, `onERC721Received`, `tokensReceived` | Callback tries same function twice; second must revert |
| T17 | Cross-function reentrancy | Function A not guarded; B reads dirty state | `reviewing-reentrancy-and-callbacks` | `nonReentrant`, `enter`, `exit` | Reenter B from A's callback; invariant holds |
| T18 | Read-only reentrancy | View price during callback before vault balances settle | `reviewing-reentrancy-and-callbacks` | `get_virtual_price`, `totalAssets`, `manageUserBalance` | During callback, view price must not be usable as collateral |
| T19 | ERC-777 / 721 / 1155 hooks | `safeMint` / `tokensReceived` hands over control | `reviewing-nft-and-marketplace` | `onERC721Received`, `onERC1155Received`, `tokensReceived` | Receiver hook must not drain marketplace before listing state updates |
| T20 | Uniswap callback spoof | Anyone can call `uniswapV3SwapCallback` / `uniswapV2Call` | `reviewing-amm-and-cl-pools` | `uniswapV3SwapCallback`, `uniswapV2Call`, `msg.sender` | Callback from non-pool address must revert |
| T21 | CL tick / empty range math | Tick math, zero liquidity, overflow in sqrtPrice | `reviewing-amm-and-cl-pools` | `tick`, `sqrtPrice`, `liquidity`, `mint(` | Extreme ticks + 1 wei; assert no unbounded liquidity credit |
| T22 | Fee-on-transfer / rebase / missing return | Pair reserves desync; `transfer` returns false | `reviewing-token-standard-pitfalls` | `fee`, `rebase`, `skim`, `sync`, `safeTransfer` | FoT token through router; recipient gets expected minOut |
| T23 | Flash-loan same-tx composition | Price and action in one tx | `reviewing-cross-function-and-composer` | `flashLoan`, `onFlashLoan`, `executeOperation` | Flash then borrow against manipulated mark; must fail health check |
| T24 | Liquidation bonus / bad debt | Bonus too high; self-liquidation; seized != debt | `reviewing-lending-and-liquidations` | `liquidate`, `closeFactor`, `liquidationIncentive` | Health factor edge; seizer cannot extract more than collateral+bonus |
| T25 | Missing insolvency check | Donate/repay path skips liquidity | `reviewing-lending-and-liquidations` | `donate`, `repay`, `liquidate` | After donate-to-zero, liquidation must not mint profit from thin air |
| T26 | Bridge message replay / decoder | Trusted relayer payload; wrong origin; out-of-order | `reviewing-bridges-and-messaging` | `lzReceive`, `processMessage`, `nonce`, `origin` | Replay same message id; wrong source chain; truncated payload |
| T27 | Merkle / IAVL / inclusion proof | Light client accepts spoofed proof | `auditing-blockchain-clients` | `verifyProof`, `IAVL`, `branchMask`, `merkle` | Mutated proof bytes must fail verification |
| T28 | Guardian / validator key ops | 5-of-9, 2-of-5, leaked operator keys | `reviewing-access-control-and-auth` | `guardian`, `validator`, `multisig`, `threshold` | Document threshold; simulate 1 key short of quorum cannot mint |
| T29 | Governance / flash-loan votes | Votes counted from current balance; delay too short | `reviewing-governance-and-timelocks` | `propose`, `queue`, `execute`, `getVotes` | Flash-loan votes cannot pass if snapshot/timelock correct |
| T30 | Timelock bypass | `execute` without `queue`; delay 0; admin is EOA | `reviewing-governance-and-timelocks` | `timelock`, `delay`, `admin` | Cannot execute proposal before delay |
| T31 | Arbitrary external call / router | User calldata + leftover approval | `reviewing-cross-function-and-composer` | `call(`, `delegatecall`, `userData`, `swap(` | Malicious calldata must not spend victim allowance |
| T32 | Missing slippage / deadline | No `minOut` / `deadline` | `reviewing-mev-ordering-and-slippage` | `amountOutMin`, `deadline`, `minAmount` | Zero minOut must be rejected or flagged in review |
| T33 | Rounding / precision / decimals | `mulDiv` direction; 2-dec tokens; empty pool | `writing-foundry-invariant-handlers` | `mulDiv`, `/ `, `% `, `decimals` | Amounts 1, 2, max-1; rounding must favor protocol |
| T34 | Off-by-one / tick boundary | `>` vs `>=`; inclusive ticks | `writing-foundry-invariant-handlers` | `+ 1`, `- 1`, `>=`, `<=` | Boundary ticks; assert no extra fee or liquidity |
| T35 | Incorrect calculation / unit mismatch | Wei vs ether; share vs asset; BPS vs 1e18 | `triaging-and-deduping-findings` | `1e18`, `10000`, `RAY`, `WAD` | Fuzz units; invariant conservation of value |
| T36 | Input validation / missing require | Happy path only | `triaging-and-deduping-findings` | early `return`, commented `require`, `TODO` | Fuzz skipped branch |
| T37 | Vyper compiler / `@nonreentrant` | Compiler bug; lock storage clash across contracts | `reviewing-reentrancy-and-callbacks` | `@nonreentrant`, `vyper` | Same-tx reenter through different pool; lock must hold **or** document compiler pin |
| T38 | Client / opcode / SELFDESTRUCT | L2 geth balance vs ERC-20 OVM_ETH | `auditing-blockchain-clients` | `SELFDESTRUCT`, `balance`, `OVM_ETH` | After selfdestruct, native + token balance both zero (regression) |
| T39 | zk circuit / proof soundness | Verifier accepts forged proof | `auditing-blockchain-clients` | `verifyProof`, `plonk`, `groth16` | Random proof bytes rejected; public inputs bound |
| T40 | Solana account confusion | Account by position, not pubkey/owner/type | (Solana) `reviewing-access-control-and-auth` | `AccountInfo`, `key ==`, `owner`, discriminator | Pass wrong account type; instruction must error |
| T41 | Solana signer / CPI / PDA | Missing `is_signer`; forged PDA seeds; arbitrary CPI | (Solana) `reviewing-access-control-and-auth` | `is_signer`, `invoke`, `find_program_address` | Unsigned authority; CPI to attacker program rejected |
| T42 | Solana sysvar / closing accounts | Fake instructions sysvar; closed account reuse | (Solana) `reviewing-signatures-permit-and-eip712` | `Sysvar1nstructions`, `load_instruction_at`, `close` | Fake sysvar rejected; reinit after close fails or is explicit |
| T43 | Move ability / object ownership | `store`/`key` misuse; shared object; `public` vs `entry` | (Move) `reviewing-access-control-and-auth` | `public entry`, `shared`, `has key`, `acquires` | Wrong signer cannot transfer shared object |
| T44 | Move arithmetic abort bypass | Overflow check skipped (shift-then-mul) | (Move) `writing-foundry-invariant-handlers` | `<<`, `*`, `checked`, `overflow` | Extreme u256; assert abort **or** documented wrap |
| T45 | Cosmos SDK / IBC / ICS-20 | Module account, ICA, packet replay, IAVL | `reviewing-bridges-and-messaging` | `RecvPacket`, `ICS20`, `moduleAcc`, `AnteHandler` | Replay packet; wrong denom; module account cannot be drained |
| T46 | Bitcoin script / ordinals / Lightning / BitVM / Clarity | Script path; inscription indexer; HTLC; Clarity traits | `auditing-blockchain-clients` | chain-specific | Replay HTLC; inscription id confusion; Clarity post-condition fail |
| T47 | Cairo felt252 / L1-L2 messaging | Overflow in felt; unvalidated `from_address` | `reviewing-bridges-and-messaging` | `felt252`, `l1_handler`, `from_address` | Unauthorized L1 handler cannot mint |
| T48 | TON FunC bounced / cell parse | Unchecked bounce; `end_parse` skipped | `reviewing-access-control-and-auth` | `bounced`, `end_parse`, `impure` | Bounce path does not credit twice |
| T49 | NFT listing replay / royalty bypass | Signature listing reusable; royalty not paid | `reviewing-nft-and-marketplace` | `orderHash`, `nonce`, `royalty` | Cancelled listing cannot fill; royalty invariant |
| T50 | Weak randomness / predictable RNG | `block.timestamp` / `blockhash` as seed | `reviewing-mev-ordering-and-slippage` | `blockhash`, `timestamp`, `random` | Same seed produces known outcome in test |
| T51 | Front-running / sandwich | Public mempool; missing commit-reveal | `reviewing-mev-ordering-and-slippage` | `deposit`, `commit`, `reveal` | Second tx cannot steal first depositor's validator key / bid |
| T52 | Client P2P / consensus / RPC | Node crash, eclipse, RPC desync | `auditing-blockchain-clients` | Go/Rust/C++ clients | Fuzz decoder; malformed packet rejected |

Immunefi Top 10 (2023 labeling): improper input validation, incorrect calculation, oracle/price, weak access control, replay/malleability, rounding, reentrancy, frontrunning, uninitialized proxy, governance. Source: [immunefi.com/immunefi-top-10/](https://immunefi.com/immunefi-top-10/).

---

## 2. Table of notable bounties

Paid or publicly disclosed whitehat payouts. "Max offered" programs (Uniswap v4 $15.5M, LayerZero $15M, Usual Sherlock $16M) are **ceilings**, not paid incidents.

| Protocol | Year | Chain / VM | $ (public) | Class | Researcher (if public) | Source |
|----------|------|------------|------------|-------|------------------------|--------|
| Wormhole (uninitialized proxy) | 2022 | Ethereum / Solidity | $10,000,000 | T02 uninitialized UUPS | satya0x | https://immunefi.com/blog/bug-fix-reviews/wormhole-uninitialized-proxy-bugfix-review/ |
| Aurora (infinite spend) | 2022 | Aurora / NEAR+EVM | $6,000,000 | T38 / T26 engine exit + DELEGATECALL | pwning.eth | https://immunefi.com/blog/bug-fix-reviews/aurora-inflation-spend-bugfix-review-6m-payout/ |
| Polygon Plasma double-spend (Wagner) | 2021 | Polygon | $2,000,000 | T27 / T04 branchMask malleability | Gerhard Wagner | https://immunefi.com/blog/bug-fix-reviews/polygon-double-spend-bugfix-review-2m-bounty/ |
| Polygon (Leon Spacewalker) | 2021 | Polygon | ~$2,200,000 | consensus / MATIC at risk | Leon Spacewalker | https://immunefi.com/blog/research/top-crypto-bounty-and-ransom-payments-report/ |
| Optimism OVM SELFDESTRUCT | 2022 | OP Stack / l2geth | $2,000,042 | T38 client balance | saurik | https://immunefi.com/blog/bug-fix-reviews/optimism-infinite-money-duplication-bugfix-review/ |
| Sei Network (2x critical) | 2024 | Sei / Cosmos | $2,000,000 + $75k | chain halt / consensus | usmannk | https://usmannkhan.com/bug%20reports/2024/06/17/sei-bug-report.html |
| Armor | ~2021 | Ethereum | $1,500,000 | (Immunefi top-5 list) | - | https://immunefi.com/blog/research/top-crypto-bounty-and-ransom-payments-report/ |
| Beanstalk insufficient validation | ~2022 | Ethereum | $1,100,000 | T36 input validation | nicole | https://medium.com/immunefi/beanstalk-insufficient-input-validation-bugfix-review-fc3fdbaab15b |
| Moonbeam / Astar / Acala library trunc | 2022 | Substrate / Frontier | $1,000,000 + $50k | client / EVM library | pwning.eth | https://pwning.mirror.xyz/okyEG4lahAuR81IMabYL5aUdvAsZ8cRCbYBXh8RHFuE |
| Polkadot Frontier EVM | 2022 | Polkadot / Frontier | $1,000,000 | T38 EVM | pwning.eth | https://pwning.mirror.xyz/RFNTSouIIlHVNmTNDThUVb1obIeN5c1LAiQuN9Ve-ok |
| Notional free collateral | 2022 | Ethereum | $1,000,000 + 100k NOTE | T35 double-count collateral | 0x60511e57 | https://medium.com/immunefi/notional-double-counting-free-collateral-bugfix-review-28b634903934 |
| Belt Finance logic error | 2021 | BSC | $1,000,000 + $50k | T35 logic | Bobface | https://medium.com/immunefi/belt-finance-logic-error-bug-fix-postmortem-39308a158291 |
| Balancer rounding (Immunefi) | 2023 | Ethereum | $1,000,000 | T33 rounding | GothicShanon89238 | https://medium.com/immunefi/balancer-rounding-error-bugfix-review-cbf69482ee3d |
| Scroll message spoofing (emergency upgrade) | 2025 | Scroll zkEVM | $1,000,000 | T26 / T39 message spoof | WhiteHatMage | https://forum.scroll.io/t/report-scroll-mainnet-emergency-upgrade-on-2025-04-25/666 |
| Story Network | 2025 | Story | $100,000 (one of two crits) | chain / messaging | WhiteHatMage, Jiri123 | https://www.story.foundation/blog/story-network-postmortem |
| Fei flashloan | 2021 | Ethereum | $800,000 | T23 flash composition | Bobface | https://medium.com/immunefi/fei-protocol-flashloan-vulnerability-postmortem-7c5dc001affb |
| Port Finance | 2022 | Solana | $180k + $450k | T40/T41 program logic | nojob | https://medium.com/immunefi/port-finance-logic-error-bugfix-review-29767aced446 |
| Redacted Cartel custom approval | 2022 | Ethereum | $560,000 | T05 custom approve | Tommaso Pifferi | https://medium.com/immunefi/redacted-cartel-custom-approval-logic-bugfix-review-9b2d039ca2c5 |
| Raydium tick manipulation | 2024 | Solana | $505,000 | T21 tick math | riproprip | https://medium.com/immunefi/raydium-tick-manipulation-bugfix-review-c6aae4527ed6 |
| Arbitrum delayed inbox | 2022 | Arbitrum | 400 ETH | T10 delayed inbox | riptide | https://medium.com/@0xriptide/hackers-in-arbitrums-inbox-ca23272641a2 |
| Enzyme missing privilege | ~2022 | Ethereum | $400,000 | T01 access | rootrescue | https://medium.com/immunefi/enzyme-finance-missing-privilege-check-bugfix-review-ddb5e87b8058 |
| The Graph rounding | 2024 | Ethereum | $290,497 | T33 rounding | GregadETH | https://medium.com/immunefi/the-graph-rounding-error-bugfix-review-c946ff470f65 |
| Sherlock Yield Strategy | ~2023 | Ethereum | $250,000 | strategy / accounting | GothicShanon89238 | https://mirror.xyz/0xE400820f3D60d77a3EC8018d44366ed0d334f93C/LOZF1YBcH1eBdxlC6HP223cAMeTpNgQ-Kc4EjQuxmGA |
| Balancer V2 (kankodu, Immunefi) | 2025 | Ethereum | $250,000 | T33 / pool math | kankodu | https://mirror.xyz/0x38F1416B9Ed3a5DA9C12c56cb4F74D9564844728/iv9_q74rSlK7gbvbJAECuDIbzfUtrSCO6mSWIHPskKI |
| zkSync Lite insufficient proof | 2023-24 | zkSync Lite | $200,000 | T39 proof verification | LonelySloth | https://medium.com/immunefi/zksync-insufficient-proof-verification-bugfix-review-dcd57944d0e2 |
| zkSync Lite (Ehsan, later) | 2026 | zkSync Lite | $200,000 | T39 (claimed on X) | Ehsan | https://x.com/Ehsan1579/status/2013482485175226811 |
| Interlay | 2022 | Interlay / Substrate | $200,000 | bridge / vault | pwning.eth | https://pwning.mirror.xyz/jlT8OgtwN3mQf3KdYmXdcSXbE4s95JzT3eR3wxiLmpw |
| Oasys | ~2024 | Oasys | $200,000 | chain | merkle_bonsai | https://mirror.xyz/0x333247F2e126954ed6428e9135Ae9dE06A76BA32/a6HqOCOjJ10Bosyi0cGz6Lxff8t68Uo4YvFsVg2tHaw |
| Tranchess (Flora) | ~2023 | BSC | $200,000 | staking / calculation | Flora | https://github.com/floranguyen0/tranchess-vulnerability-disclosure |
| Beanstalk logic error | 2022 | Ethereum | ~$182,000 | T36 / T35 | - | https://medium.com/immunefi/beanstalk-logic-error-bugfix-review-4fea17478716 |
| Evmos | 2024 | Evmos / Cosmos EVM | $150,000 | docs / chain | jayjonah.eth | https://medium.com/@jjordanjjordan/150-000-evmos-vulnerability-through-reading-documentation-d26328590a7a |
| Synthetix | ~2022 | Ethereum / Optimism | $150,000 | T35 logic | thunderdeep14 | https://medium.com/immunefi/synthetix-logic-error-bugfix-review-40da0ead5f4f |
| APWine delegations | ~2022 | Ethereum | $100,000 | T01 / T04 delegations | setuid0 | https://medium.com/immunefi/apwine-incorrect-check-of-delegations-bugfix-review-7e401a49c04f |
| DFX rounding | ~2023 | Ethereum | $100,000 | T33 EURS 2 decimals | perseverance | https://medium.com/immunefi/dfx-finance-rounding-error-bugfix-review-17ba5ffb4114 |
| Silo | 2023 | Ethereum | $100,000 | (X announcement) | kankodu | https://twitter.com/kankodu/status/1669833829203476480 |
| Yield Protocol | ~2023 | Ethereum | $95,000 | T35 logic | Paludo0x | https://medium.com/immunefi/yield-protocol-logic-error-bugfix-review-7e86741e6f50 |
| Polygon consensus bypass | ~2022 | Polygon | $75,000 | T27 consensus | Niv Yehezkel | https://medium.com/immunefi/polygon-consensus-bypass-bugfix-review-7076ce5047fe |
| Sei (Catchme) | 2024 | Sei | $75,000 | chain | Catchme | https://exvul.com/share-the-details-sei-protocol-vulnerability-worth-75k/ |
| Stacks DoS | ~2024 | Stacks / Clarity | ~$76,000 | T46 / T52 DoS | Catchme | https://medium.com/immunefi/stacks-dos-bugfix-review-dc0f2a75b276 |
| Acala block production | 2025 | Acala / Substrate | $70,000 | T52 block production | Lastc0de | https://immunefi.com/blog/all/acala-block-production-shutdown-bug-fix-review/ |
| Mushrooms | ~2021 | Ethereum | $60,000 | T35 logic | CKK Sec | https://medium.com/immunefi/mushrooms-finance-logic-error-bug-fix-postmortem-780122821621 |
| Sense access control | ~2022 | Ethereum | $50,000 | T01 | alephv.eth | https://medium.com/immunefi/sense-finance-access-control-issue-bugfix-review-32e0c806b1a0 |
| Fluidity | ~2022 | Ethereum | $50,000 | protocol halt / logic | Trust | https://www.trust-security.xyz/post/breaking-fluidity-for-glory-and-50k |
| Q Blockchain | ~2023 | Q | $50,000 | chain | Blockian | https://medium.com/@blockian/striking-gold-at-30-000-feet-uncovering-a-critical-vulnerability-in-q-blockchain-for-50-000-ab335042147b |
| BendDAO sewer pass flash claim | ~2023 | Ethereum | $50,000 | T19 NFT | - | https://medium.com/@BendDAO/sewer-pass-flash-claim-vulnerability-9d2b0b1e09ef |
| zkSync Era soundness | 2023 | zkSync Era | $50,000 | T39 | ChainLight | https://medium.com/chainlight/uncovering-a-zk-evm-soundness-bug-in-zksync-era-f3bc1b2a66d8 |
| Astar (Zellic) | ~2023 | Astar | $50,000 | chain / EVM | Zellic | https://www.zellic.io/blog/finding-a-critical-vulnerability-in-astar/ |
| Wormhole (Marco Nunes) | 2025 | Wormhole | $50,000 | T26 messaging | Marco Nunes | https://x.com/marcotnunes/status/1889707212450234629 |
| Axelar halt | 2025 | Axelar | $50,000 | T26 / T52 halt | Marco Nunes | https://marcotnunes.com/axelar-network-cross-chain-halt-vulnerability/ |
| VeChainThor VTHO accrual | 2025 | VeChain | $50,000 | T35 fee token | nnez | https://immunefi.com/blog/all/vechainthor-vtho-accrual-bypass-bug-fix-review/ |
| Sui network shutdown | 2025 | Sui / Move | $50,000 | T43/T52 halt | F4lt | https://immunefi.com/blog/bug-fix-reviews/sui-network-shutdown/ |
| Injective | 2026 | Injective | $50,000 | (X) | f4lc0n | https://x.com/al_f4lc0n/status/2033110168045568434 |
| Trust Security "permission denied" | ~2023 | 100+ projects | $50,000 total | T01 / T09 aliasing | Trust Security | https://www.trust-security.xyz/post/permission-denied |
| Cronos tx fee theft | ~2022 | Cronos | $40,000 | T01 / fees | zb3 | https://medium.com/immunefi/cronos-theft-of-transactions-fees-bugfix-review-b33f941b9570 |
| 88mph init | ~2021 | Ethereum | $42,000 | T02 initialize | Ashiq Amien | https://medium.com/immunefi/88mph-function-initialization-bug-fix-postmortem-c3a2282894d3 |
| Perpetual Protocol bad debt | 2023 | Optimism | $30,000 | T24 bad debt | banditx0x | https://securitybandit.com/2023/02/07/bad-debt-attack-for-perpetual-protocol/ |
| Alchemist admin brick | ~2023 | Ethereum | $28,000 | T01 forced revert | Dacian | https://dacian.me/28k-bounty-admin-brick-forced-revert |
| Ondo | ~2022 | Ethereum | $25,000 | high | Ashiq / iosiro | https://iosiro.com/blog/high-risk-vulnerability-disclosed-to-ondo-finance |
| Zapper arbitrary calldata | ~2021 | Ethereum | $25,000 | T31 | Lucash-dev | https://medium.com/immunefi/zapper-arbitrary-call-data-bug-fix-postmortem-d75a4a076ae9 |
| Tidal logic | ~2021 | Ethereum | $25,000 | T35 | csanuragjain | https://medium.com/immunefi/tidal-finance-logic-error-bug-fix-postmortem-3607d8b7ed1f |
| Thena (C4 miss) | 2023 | BSC | $20,000 | T35 / gauge | zzykxx | https://zzykxx.com/2023/02/02/the-bug-that-codearena-missed-,-twice/ |
| Thena (second) | 2023 | BSC | $20,000 | T35 | zzykxx | https://zzykxx.com/2023/02/27/a-very-helpful-sign/ |
| Oasis shutdown | ~2022 | Oasis | $20,000 | halt | Trust | https://www.trust-security.xyz/post/taking-home-a-20k-bounty-with-oasis-platform-shutdown-vulnerability |
| Optimism censorship (iosiro) | 2023 | OP Stack | $20,000 | T10 / T52 censorship | iosiro | https://www.iosiro.com/blog/optimism-censorship-bug-disclosure |
| Enzyme oracle | ~2021 | Ethereum | $19,000 | T06 | setuid0 | https://medium.com/immunefi/enzyme-finance-price-oracle-manipulation-bug-fix-postmortem-4e1f3d4201b5 |
| Sovryn | 2024 | Rootstock | $15,000 | (X) | gandu | https://x.com/gandu_whitehat/status/1803794103248806223 |
| Mt Pelerin double tx | ~2021 | Ethereum | $10,000 | T04 / replay | - | https://medium.com/immunefi/mt-pelerin-double-transaction-bugfix-review-503838db3d70 |
| Alchemix access | ~2021 | Ethereum | $7,500 | T01 | Ashiq | https://medium.com/immunefi/alchemix-access-control-bug-fix-debrief-a13d39b9f2e0 |
| Movement Labs chain split | 2025 | Movement / Move | $6,710 | T52 split | Yunus Emre | https://medium.com/@yemresaritoprak/permanent-chain-split-in-movement-full-node-anatomy-of-a-6-710-critical-vulnerability-that-fa75fe66a0c7 |
| Charged Particles griefing | ~2021 | Ethereum | $5,000 | T36 grief | janbro.eth | https://medium.com/immunefi/charged-particles-griefing-bug-fix-postmortem-d2791e49a66b |
| O3 bridge | ~2022 | Multi | $5,000 | T26 | Trust, 0xDjango | https://www.trust-security.xyz/post/critical-finding-stealing-tokens-from-o3-bridge-users |
| LayerZero (Trust, low) | ~2023 | Multi | $5,000 | T26 | Trust Security | https://www.trust-security.xyz/post/learning-by-breaking-a-layerzero-case-study-part-3 |
| xDai Stake arbitrary call | ~2021 | Gnosis | $5,000 | T31 | 0xadee028d | https://medium.com/immunefi/xdai-stake-arbitrary-call-method-bug-postmortem-f80a90ac56e3 |
| Bitswift unlimited mint | ~2021 | - | $4,500 | T01 mint | - | https://medium.com/immunefi/bitswift-unlimited-mint-bugfix-postmortem-147a1e57dca9 |
| Fringe.fi insolvency | ~2022 | Ethereum | $2,000 | T24 | Trust | https://www.trust-security.xyz/post/diving-deep-into-a-critical-protocol-insolvency-bug-in-fringe-fi-lending-platform |
| Hats: Velvet Capital competition | 2024 | EVM | $40,427 paid of ~$70k pool | T31 calldata | 16 wardens | https://github.com/Velvet-Capital/audits/blob/main/report.md |
| Hats: Raft later exploited | 2023 | Ethereum | (audit venue) | T14 inflation (hack) | - | https://rekt.news/raft-rekt |
| Scroll (Shabarkin) | 2025 | Scroll | $1,000 | (X) | Pavel Shabarkin | https://x.com/shabarkin/status/1917483039195816213 |
| Balancer (riptide, 50 ETH) | ~2022 | Ethereum | 50 ETH | pool | riptide | https://mirror.xyz/0x2719F6Dfb85086F87319079cC2f7EeFD0e40994D/NWDf5uW1Ve7-TrcPKwmM86xp8ploMSCRGC58A-NSoFY |
| Tranchess first-run (Jade) | ~2022 | BSC | 44.8 ETH | T13 first depositor | Jade | https://www.kalos.xyz/blog/tranchess-liquid-staking-deposit-firstrun-vulnerability-analysis |

Curated index of ~140 writeups: [sayan011/Immunefi-bug-bounty-writeups-list](https://github.com/sayan011/Immunefi-bug-bounty-writeups-list). Immunefi library: [immunefi-team/Web3-Security-Library BugFixReviews](https://github.com/immunefi-team/Web3-Security-Library/blob/main/BugFixReviews/README.md).

---

## 3. Table of notable hacks / postmortems (loss, not bounty)

rekt.news amounts unless noted. Opsec / key theft included when the industry treats them as DeFi incidents; they are **not** Solidity classes.

| Protocol | Year | Chain | Loss (public) | Class | Source |
|----------|------|-------|---------------|-------|--------|
| Ronin Network | 2022 | Ronin / ETH bridge | $624,000,000 | T28 validator keys | https://rekt.news/ronin-rekt |
| Poly Network | 2021 | ETH/BSC/Polygon | $611,000,000 | T01 / T26 keeper | https://rekt.news/poly-network-rekt |
| BNB Bridge (IAVL) | 2022 | BNB / Cosmos | $586,000,000 | T27 IAVL proof | https://rekt.news/bnbbridge-rekt |
| Wormhole (Solana sysvar) | 2022 | Solana + ETH | $326,000,000 | T42 fake instructions sysvar | https://rekt.news/wormhole-rekt |
| Drift Protocol | 2025 | Solana | $285,000,000 | (rekt; verify class in postmortem) | https://rekt.news/leaderboard |
| Cetus | 2025 | Sui / Move | $223,000,000 | T44 overflow check bypass in integer-mate | https://rekt.news/leaderboard |
| Euler Finance | 2023 | Ethereum | $197,000,000 | T25 donateToReserves / self-liq | https://rekt.news/euler-rekt |
| Nomad Bridge | 2022 | ETH / Nomad | $190,000,000 | T26 / T27 trusted root = 0x00 | https://rekt.news/nomad-rekt |
| Beanstalk | 2022 | Ethereum | $181,000,000 | T29 flash-loan governance | https://rekt.news/beanstalk-rekt |
| Compound COMP | 2021 | Ethereum | $147,000,000 | T35 proposal / distribution | https://rekt.news/compound-rekt |
| Cream Finance 2 | 2021 | Ethereum | $130,000,000 | T06 / T23 yUSD oracle | https://rekt.news/cream-rekt-2 |
| Balancer V2 (Nov) | 2025 | Ethereum / multi | $128,000,000 | T33 precision (DHL: precision-loss) | https://rekt.news/leaderboard |
| Multichain | 2023 | Multi | $126,300,000 | T28 / ops | https://rekt.news/multichain-rekt-2 |
| BonqDAO | 2023 | Polygon | $120,000,000 | T06 Tellor spot | https://rekt.news/bonq-rekt |
| Mango Markets | 2022 | Solana | $115,000,000 | T06 MNGO mark oracle | https://rekt.news/mango-rekt |
| Harmony Horizon | 2022 | Harmony | $100,000,000 | T28 2-of-5 keys | https://rekt.news/harmony-rekt |
| HECO Bridge | 2023 | HECO | $99,100,000 | T28 | https://rekt.news/leaderboard |
| WooFi | 2024 | Multiple | $85,000,000 | T06 sPMM oracle | https://rekt.news/woofi-rekt |
| Orbit Bridge | 2023 | Multi | $81,500,000 | T01 / T26 | https://rekt.news/orbit-bridge-rekt |
| Fei / Rari | 2022 | Ethereum | $80,000,000 | T16 / T17 reentrancy | https://rekt.news/fei-rari-rekt |
| Qubit | 2022 | BSC | $80,000,000 | T26 QBridge deposit proof | https://rekt.news/qubit-rekt |
| Curve / Vyper | 2023 | Ethereum | $69,300,000 | T37 compiler reentrancy lock | https://rekt.news/vyper-rekt |
| Radiant Capital II | 2024 | Multi | $53,000,000 | T28 malware / multisig (ops) | https://rekt.news/leaderboard |
| KyberSwap CL | 2023 | Multi | $48,000,000 | T21 / T33 CL rounding | https://rekt.news/kyberswap-rekt |
| Cashio | 2022 | Solana | $48,000,000 | T40 collateral account chain | https://rekt.news/cashio-rekt |
| PancakeBunny | 2021 | BSC | $45,000,000 | T06 flash oracle | https://rekt.news/pancakebunny-rekt |
| Hedgey | 2024 | Ethereum | $44,700,000 | T01 campaign claim | https://rekt.news/leaderboard |
| GMX | 2025 | Arbitrum | $42,000,000 | T15 share price (DHL) | https://rekt.news/leaderboard |
| Alpha Homora | 2021 | Ethereum | $37,500,000 | T06 / T23 | https://rekt.news/alpha-finance-rekt |
| Uranium | 2021 | BSC | $57,200,000 | T35 k constant (1000 vs 10000) | https://rekt.news/uranium-rekt |
| Harvest Finance | 2020 | Ethereum | $25,000,000 | T06 Curve spot | https://rekt.news/harvest-finance-rekt |
| Sonne Finance | 2024 | Optimism | $20,000,000 | T14 empty market | https://rekt.news/sonne-finance-rekt |
| Uwulend | 2024 | Ethereum | $19,400,000 | T06 oracle | https://rekt.news/uwulend-rekt |
| Cream AMP | 2021 | Ethereum | $18,800,000 | T16 ERC-777 | https://rekt.news/cream-rekt |
| Inverse Finance | 2022 | Ethereum | $15,600,000 | T06 INV spot | https://rekt.news/inverse-finance-rekt |
| Deus DAO (Apr) | 2022 | Ethereum | $13,400,000 | T06 | https://rekt.news/deus-dao-rekt |
| Ronin II | 2024 | Ronin | $12,000,000 | bridge / MEV gap | https://rekt.news/leaderboard |
| Hundred + Agave | 2022 | Gnosis / Fantom | $11,700,000 | T16 reentrancy | https://rekt.news/leaderboard |
| PrismaFi | 2024 | Ethereum | $11,600,000 | T36 / T01 | https://rekt.news/leaderboard |
| Yearn (2023) | 2023 | Ethereum | $11,400,000 | misconfig | https://rekt.news/yearn-rekt-2 |
| LiFi / Jumper | 2024 | Multi | $9,730,000 | T31 calldata | https://rekt.news/leaderboard |
| zkLend | 2025 | Starknet | $9,570,000 | T47 market | https://rekt.news/leaderboard |
| Platypus | 2023 | Avalanche | $8,500,000 | T24 USP solvency | https://rekt.news/platypus-rekt |
| Jimbo's | 2023 | Arbitrum | $7,500,000 | T06 / T21 floor price | https://rekt.news/jimbos-protocol-rekt |
| Hundred (4626) | 2023 | Ethereum | $7,400,000 | T13 / T14 inflation | https://rekt.news/hundred-rekt |
| Exactly | 2023 | Optimism | $7,200,000 | T04 permit / debt | https://rekt.news/exactly-protocol-rekt |
| Lodestar | 2022 | Arbitrum | $6,500,000 | T06 plvGLP donate | https://rekt.news/lodestar-rekt |
| Deus III | 2023 | Multi | $6,500,000 | T35 DEI | https://rekt.news/leaderboard |
| Penpie | 2024 | Ethereum | $27,000,000 | T16 reward reentrancy | https://rekt.news/leaderboard |
| Socket | 2024 | Multi | $3,300,000 | T31 gateway calldata | https://rekt.news/leaderboard |
| Raft | 2023 | Ethereum | $3,300,000 | T14 donate inflation | https://rekt.news/raft-rekt |
| Sushi router (Apr 2023) | 2023 | Ethereum | $3,300,000 | T31 unchecked input | https://rekt.news/leaderboard |
| TempleDAO | 2022 | Ethereum | $2,300,000 | T01 stale auth | https://rekt.news/templedao-rekt |
| Sturdy | 2023 | Ethereum | $800,000 | T18 read-only reentrancy | https://rekt.news/sturdy-rekt |
| Tornado Cash governance | 2023 | Ethereum | $750,000 | T29 proposal | https://rekt.news/leaderboard |
| bZx (2021) | 2021 | Ethereum | $55,000,000 | T06 / T23 | https://rekt.news/bzx-rekt |
| Poly Network 2 | 2023 | Multi | $4,400,000 | T26 | https://rekt.news/leaderboard |
| Radiant I | 2024 | Arbitrum | $4,500,000 | T33 precision | https://rekt.news/leaderboard |
| Conic | 2023 | Ethereum | $4,200,000 | T18 + T06 | https://rekt.news/conic-rekt |
| Velocore | 2024 | Linea / zk | $6,800,000 | T33 / pool | https://rekt.news/leaderboard |
| Abracadabra MIM | 2024 | Ethereum | $6,500,000 | T33 / insolvency | https://rekt.news/leaderboard |
| Unizen | 2024 | Multi | $21,000,000 | T31 unverified call | https://rekt.news/leaderboard |
| Gamma | 2024 | Multi | $4,500,000 | T06 | https://rekt.news/leaderboard |
| Bedrock uniBTC | 2024 | Multi | $2,000,000 | T35 1:1 mint | https://rekt.news/leaderboard |
| Pike Finance | 2024 | Multi | $1,900,000 | T02 / init | https://rekt.news/leaderboard |
| Super Sushi Samurai | 2024 | Blast | $4,800,000 | T35 self-transfer double | https://rekt.news/leaderboard |
| ZKasino | 2024 | ETH / zk | $33,000,000 | ops / withheld | https://rekt.news/leaderboard |
| Slope wallet | 2022 | Solana | ~$8M+ users | client logging mnemonics | OtterSec / Zellic via https://medium.com/@manuelpz.dev/a-not-so-brief-historical-review-of-solanas-major-security-incidents-ef09867cb6bf |
| Crema | 2022 | Solana | $8,800,000 | T21 tick / account | https://rekt.news/leaderboard |
| Nirvana | 2022 | Solana | $3,500,000 | T06 ANA | https://rekt.news/leaderboard |
| Moola | 2022 | Celo | $8,400,000 | T06 | https://rekt.news/leaderboard |
| Visor | 2021 | Ethereum | $8,200,000 | T16 | https://rekt.news/leaderboard |
| Anyswap | 2021 | Multi | $7,900,000 | T04 MPC / key | https://rekt.news/leaderboard |
| Meter | 2022 | Multi | $7,700,000 | T26 deposit | https://rekt.news/leaderboard |
| Warp | 2020 | Ethereum | $7,800,000 | T06 LP oracle | https://rekt.news/leaderboard |
| Origin OUSD | 2020 | Ethereum | $8,000,000 | T16 rebase / vault | https://rekt.news/leaderboard |
| Pickle | 2020 | Ethereum | $19,700,000 | T31 jar | https://rekt.news/pickle-rekt |
| Furucombo | 2021 | Ethereum | $14,000,000 | T02 / T31 aTo | https://rekt.news/furucombo-rekt |
| Paid Network | 2021 | Ethereum | $27,000,000 | T01 mint | https://rekt.news/paid-rekt |
| Meerkat | 2021 | BSC | $32,000,000 | T01 / rug-adjacent | https://rekt.news/leaderboard |
| Grim | 2021 | Fantom | $30,000,000 | T16 | https://rekt.news/grim-rekt |
| MonoX | 2021 | Ethereum | $31,400,000 | T06 vUSD | https://rekt.news/monox-rekt |
| Indexed | 2021 | Ethereum | $16,000,000 | T06 | https://rekt.news/indexed-finance-rekt |
| Popsicle | 2021 | Multi | $20,000,000 | T35 share | https://rekt.news/popsicle-rekt |
| xToken | 2021 | Ethereum | $24,000,000 | T06 | https://rekt.news/xtoken-rekt |
| bEarn | 2021 | BSC | $18,000,000 | T35 | https://rekt.news/leaderboard |
| EasyFi | 2021 | Polygon | $59,000,000 | T28 keys | https://rekt.news/leaderboard |
| Vee | 2021 | Avalanche | $34,000,000 | T06 | https://rekt.news/leaderboard |
| Spartan | 2021 | BSC | $30,500,000 | T06 | https://rekt.news/leaderboard |
| BurgerSwap | 2021 | BSC | $7,200,000 | T16 | https://rekt.news/leaderboard |
| DODO | 2021 | Ethereum | $2,000,000 | T20 init / callback | https://rekt.news/dodo-rekt |
| Akropolis | 2020 | Ethereum | $2,000,000 | T16 | https://rekt.news/leaderboard |
| Saddle (2022) | 2022 | Ethereum | (DHL) | T35 metapool | https://github.com/SunWeb3Sec/DeFiHackLabs |
| Team Finance | 2022 | Ethereum | $15,800,000 | T31 migration | https://rekt.news/leaderboard |
| Reaper Farm | 2022 | Fantom | (DHL) | T01 | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md |
| XCarnival | 2022 | BSC | (DHL) | T24 infinite borrow | https://github.com/SunWeb3Sec/DeFiHackLabs |
| Treasure DAO | 2022 | Arbitrum | $1,400,000 | T19 NFT | https://rekt.news/leaderboard |
| Omni NFT | 2022 | Ethereum | (Immunefi Top10 ex.) | T19 reentrancy | https://immunefi.com/immunefi-top-10/ |
| Quixotic | 2022 | Optimism | (DHL) | T04 listing sig | https://github.com/SunWeb3Sec/DeFiHackLabs |
| Wintermute (OP) | 2022 | Optimism | $27,600,000 | T02 Gnosis Safe uninit on L2 | https://rekt.news/leaderboard |
| Audius | 2022 | Ethereum | $6,000,000 | T03 storage + governance | https://rekt.news/audius-rekt |
| Transit Swap | 2022 | Multi | $21,200,000 | T31 | https://rekt.news/leaderboard |
| DFX (hack) | 2022 | Ethereum | (DHL reentrancy) | T16 | https://github.com/SunWeb3Sec/DeFiHackLabs |
| Mango vs Cashio vs Wormhole | 2022 | Solana | see rows | T06 / T40 / T42 | Neodyme pitfalls https://neodyme.io/en/blog/solana_common_pitfalls |
| Sentiment | 2023 | Arbitrum | (DHL) | T18 | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md |
| dForce | 2023 | Multi | $3,650,000 | T18 | https://rekt.news/leaderboard |
| Midas | 2023 | Multi | $660k / $600k | T18 | https://rekt.news/leaderboard |
| Conic 02 | 2023 | Ethereum | (DHL) | T06 | https://github.com/SunWeb3Sec/DeFiHackLabs |
| EraLend | 2023 | zkSync | $3,400,000 | T16 | https://rekt.news/leaderboard |
| Stars Arena | 2023 | Avalanche | $2,900,000 | T16 | https://rekt.news/leaderboard |
| Telcoin | 2023 | Polygon | (DHL) | T03 storage | https://github.com/SunWeb3Sec/DeFiHackLabs |
| Onyx | 2023 | Ethereum | $2,100,000 | T14 | https://rekt.news/leaderboard |
| Wise Lending | 2023 | Ethereum | (DHL) | T14 | https://github.com/SunWeb3Sec/DeFiHackLabs |
| kTAF | 2023 | Ethereum | (DHL) | T14 Compound inflation | https://github.com/SunWeb3Sec/DeFiHackLabs |
| Channels / MetaLend / MahaLend | 2023 | Multi | (DHL) | T14 | https://github.com/SunWeb3Sec/DeFiHackLabs |
| Balancer (Aug 2023) | 2023 | Ethereum | $2,100,000 | T33 rounding | https://rekt.news/leaderboard |
| Zunami | 2023 | Ethereum | $2,100,000 | T06 | https://rekt.news/leaderboard |
| Level Finance | 2023 | BSC | $1,100,000 | T01 | https://rekt.news/leaderboard |
| Atlantis Loans | 2023 | BSC | $2,500,000 | T29 governance | https://rekt.news/leaderboard |
| Shibarium bridge | 2023 | Shibarium | $2,600,000 | T26 / T28 | https://rekt.news/leaderboard |
| OKX DEX | 2023 | Multi | $2,700,000 | T31 | https://rekt.news/leaderboard |
| Unibot | 2023 | Ethereum | $640,000 | T31 | https://rekt.news/leaderboard |
| Hypr | 2023 | - | $220,000 | T35 | https://rekt.news/leaderboard |
| Levana | 2023 | Osmosis / CosmWasm | $1,146,000 | T35 | https://rekt.news/leaderboard |
| Acala aUSD | 2021 | Acala | $1,600,000 | T01 erroneous mint | https://rekt.news/leaderboard |
| Hedera | 2023 | Hedera | $515,000 | T01 / config | https://rekt.news/leaderboard |
| AlexLab (Stacks) | 2024 | Stacks | $4,300,000 | T46 Clarity | https://rekt.news/leaderboard |
| AlexLab II | 2025 | Stacks | $16,180,000 | T46 | https://rekt.news/leaderboard |
| Astroport | 2024 | Neutron / CosmWasm | $6,400,000 | T45 | https://rekt.news/leaderboard |
| Loopscale | 2025 | Solana | $5,800,000 | T40/T41 | https://rekt.news/leaderboard |
| Ionic | 2025 | Mode / OP | $6,943,774 | T14 fork | https://rekt.news/leaderboard |
| ResupplyFi | 2025 | Ethereum | $9,800,000 | T15 share | https://rekt.news/leaderboard |
| Cork | 2025 | Ethereum | $12,000,000 | T01 | https://rekt.news/leaderboard |
| Abracadabra II | 2025 | Ethereum | $12,913,691 | T25 insolvency | https://rekt.news/leaderboard |
| Yearn III | 2025 | Ethereum | $9,000,000 | vault | https://rekt.news/leaderboard |
| yETH | 2025 | Ethereum | (DHL unsafe math) | T33 | https://github.com/SunWeb3Sec/DeFiHackLabs |
| Moonwell | 2025-26 | Base / OP | $1,780,000 + later | T07 oracle | https://rekt.news/leaderboard |
| 1inch Fusion | 2025 | Ethereum | $5,000,000 | T31 yul calldata | https://rekt.news/leaderboard |
| ParaSwap DAI approval | 2025 | Ethereum | (DHL stale approval) | T05 | https://github.com/SunWeb3Sec/DeFiHackLabs |
| Venus THE | 2026 | BNB | (DHL) | T14 donation | https://github.com/SunWeb3Sec/DeFiHackLabs |
| Curve LlamaLend | 2026 | Ethereum | (DHL) | T15 share | https://github.com/SunWeb3Sec/DeFiHackLabs |
| Makina | 2026 | Ethereum | $4,130,000 | T06 | https://rekt.news/leaderboard |
| Truebit | 2026 | Ethereum | $26,200,000 | T33 overflow | https://rekt.news/leaderboard |
| SummerFi | 2026 | Ethereum | (DHL NAV) | T15 | https://github.com/SunWeb3Sec/DeFiHackLabs |
| KelpDAO | 2026 | Ethereum | $290,000,000 | (rekt; verify class) | https://rekt.news/leaderboard |
| The DAO (historical) | 2016 | Ethereum | ~$60M ETH | T16 | SWC / Ethereum history |
| Parity wallet | 2017 | Ethereum | library kill | T02 / T01 | historical |
| Bancor | 2018 | Ethereum | ~$13.5M | T01 | historical |
| bZx 2020 (x2) | 2020 | Ethereum | ~$1M then more | T06 | https://rekt.news |
| Value DeFi (series) | 2020-21 | ETH/BSC | $7M-$11M | T06 | https://rekt.news/leaderboard |
| Cover | 2020 | Ethereum | $9,400,000 | T35 rebase | https://rekt.news/leaderboard |
| Eminence | 2020 | Ethereum | $15,000,000 | T06 | https://rekt.news/leaderboard |
| LIFI 2022 (earlier) | 2022 | Multi | (DHL) | T26 | https://github.com/SunWeb3Sec/DeFiHackLabs |
| Across (bounty, not hack) | 2025 | Multi | paid finding | T26 | https://mirror.xyz/0x9D6b7f5e8d1b9dFea8dDD29c0DbD81687e721601/mrt70ckjaZymv9keUy_TzHVIzjBOQr-Hx_KI1ydFeoQ |
| Celer / Hyphen / Hop | various | Multi | (see DHL / rekt for each named incident) | T26 | search rekt.news + protocol name |
| Polter | 2024 | Fantom | (DHL flashloan) | T06 | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md |
| Clober | 2024 | Sei / EVM | $500,000 | T16 | https://rekt.news/leaderboard |
| Munchables | 2024 | Blast | $62,500,000 | T01 / insider | https://rekt.news/leaderboard |
| BALD | 2023 | Base | $23,000,000 | T01 / insider | https://rekt.news/leaderboard |
| Grand Base | 2024 | Base | $2,000,000 | T01 | https://rekt.news/leaderboard |
| Banana Gun | 2024 | Ethereum | $3,000,000 | T31 bot | https://rekt.news/leaderboard |
| Rho Market | 2024 | Scroll | $7,500,000 | T06 | https://rekt.news/leaderboard |
| Tapioca | 2024 | Multi | $4,400,000 | T01 | https://rekt.news/leaderboard |
| Onyx II | 2024 | Ethereum | $3,800,000 | T14 | https://rekt.news/leaderboard |
| Shezmu | 2024 | Ethereum | $4,900,000 | T01 | https://rekt.news/leaderboard |
| DeltaPrime I/II | 2024 | Avalanche / Arbitrum | $5.98M / $4.85M | T16 | https://rekt.news/leaderboard |
| NGP / unverified long tail | 2024-26 | mostly EVM | see DHL | mixed | https://github.com/SunWeb3Sec/DeFiHackLabs |

SlowMist: 2024 ~410 incidents / ~$2.01B; 2025 ~200 incidents / ~$2.94B (Bybit ops dominates). DeFi still majority of **count**. Sources: [SlowMist 2024](https://slowmist.medium.com/slowmist-2024-blockchain-security-and-anti-money-laundering-annual-report-d7fc94ccf624), [SlowMist 2025](https://slowmist.medium.com/2025-blockchain-security-and-aml-annual-report-9f85183d5461).

---

## 4. Chain / VM unique classes

### EVM L2 (OP Stack, Arbitrum, Base, Blast, Mantle, Mode, zkSync, Scroll, Linea, Polygon zkEVM)

| Class | Wrongly trusted | Named public cases | Test idea |
|-------|-----------------|--------------------|-----------|
| Sequencer uptime vs Chainlink | Spot still updates on L2 after downtime | Contest finding class; Chainlink docs | https://docs.chain.link/data-feeds/l2-sequencer-feeds ; https://medium.com/@lopotras/l2-sequencer-and-stale-oracle-prices-bug-54a749417277 |
| OVM ETH vs SELFDESTRUCT | Geth `stateObject` vs `OVM_ETH` | Optimism $2M bounty (saurik) | After selfdestruct, both balances zero |
| Censorship / forced inclusion | Sequencer as honest inclusion | iosiro Optimism $20k | https://www.iosiro.com/blog/optimism-censorship-bug-disclosure |
| Delayed inbox | Unvalidated delayed messages | riptide Arbitrum 400 ETH | https://medium.com/@0xriptide/hackers-in-arbitrums-inbox-ca23272641a2 |
| Alias | L1 contract address = L2 EOA | Trust "permission denied" $50k across 100+ | https://www.trust-security.xyz/post/permission-denied |
| Safe uninitialized on L2 | Same Safe as L1, empty init | Wintermute OP 2022 | Initialize check on L2 deployment |
| zk soundness | Verifier | zkSync Era ChainLight $50k; zkSync Lite $200k; Polygon zkEVM Verichains | https://medium.com/chainlight/uncovering-a-zk-evm-soundness-bug-in-zksync-era-f3bc1b2a66d8 |
| Scroll message spoof | Bridge message identity | WhiteHatMage $1M 2025 | https://forum.scroll.io/t/report-scroll-mainnet-emergency-upgrade-on-2025-04-25/666 |
| Blast / Mode / Base forks | Empty-market Compound forks | Sonne (OP), Ionic (Mode), SSS self-transfer (Blast) | Empty cToken donate test |
| Linea Velocore | Pool math | Velocore $6.8M | Rounding invariant |
| Mantle / custom gas | Fee token decimals | (review class; few public mega-hacks) | Fee token unit tests |
| Polygon zkEVM | Proof / log confusion | Verichains; 0xiczc X; Asymmetric log confusion | https://blog.verichains.io/p/discovering-and-fixing-a-critical |

### Solana

| Class | Wrongly trusted | Cases | Test idea (Anchor) |
|-------|-----------------|-------|---------------------|
| Sysvar account confusion | `load_instruction_at` unchecked | Wormhole $326M | Fake instructions sysvar must fail `load_instruction_at_checked` |
| Collateral account graph | Intermediate accounts | Cashio $48M | Fake Arrow/Saber chain cannot mint CASH |
| Signer / PDA | Missing `is_signer` | Neodyme pitfalls | https://neodyme.io/en/blog/solana_common_pitfalls |
| Tick / CL | Tick account | Raydium $505k bounty; Crema hack | Extreme tick rejected |
| Oracle mark | Illiquid spot | Mango $115M | MNGO-like mark cannot inflate free collateral |
| Wallet logging | Sentry mnemonics | Slope 2022 | Client, not program |
| Port Finance | Program logic | $630k bounty | https://medium.com/immunefi/port-finance-logic-error-bugfix-review-29767aced446 |
| Drift 2025 | (verify) | $285M rekt | Read official postmortem before mapping class |
| Loopscale 2025 | accounts | $5.8M | Account constraint tests |
| marginfi flash | flash + health | Asymmetric.re disclosure | https://blog.asymmetric.re/threat-contained-marginfi-flash-loan-vulnerability/ |

### Move (Sui, Aptos, Movement)

| Class | Wrongly trusted | Cases | Test idea |
|-------|-----------------|-------|-----------|
| Overflow check after shift | integer-mate | Cetus $223M | Extreme liquidity add aborts or credits 1:1 |
| Shared object / abilities | `key`+`store` | review class | Unauthorized `public` entry cannot extract |
| Network halt | validator / runtime | Sui $50k Immunefi (F4lt) | https://immunefi.com/blog/bug-fix-reviews/sui-network-shutdown/ |
| Chain split | full node | Movement $6.7k | https://medium.com/@yemresaritoprak/permanent-chain-split-in-movement-full-node-anatomy-of-a-6-710-critical-vulnerability-that-fa75fe66a0c7 |
| Aptos VM | stack / abort | historical VM CVEs (client) | Fuzz Move bytecode |

### Cosmos SDK / IBC / CosmWasm

| Class | Wrongly trusted | Cases | Test idea |
|-------|-----------------|-------|-----------|
| IAVL range proof | nil root / negative nodes | BNB Bridge $586M; Verichains CVE-2023-27575 | https://blog.verichains.io/p/vsa-2022-101-iavl-spoofing-attack |
| ICS-20 / packet | denom, replay | IBC hault class (review) | RecvPacket replay fails |
| Module account | bank send from module | (contest + audits) | Module cannot be msg.sender-spoofed |
| CosmWasm | Wasm instantiate | Astroport $6.4M; Levana $1.1M | Message validate |
| Evmos | docs vs impl | $150k | https://medium.com/@jjordanjjordan/150-000-evmos-vulnerability-through-reading-documentation-d26328590a7a |
| Sei | consensus | $2M + $75k | usmannk + Catchme writeups |
| Injective | (2026 X) | $50k | https://x.com/al_f4lc0n/status/2033110168045568434 |
| Gravity / Cronos bridge | attestation | Faith writeup | https://faith2dxy.xyz/2023-12-12/cronos-gravity-bridge-bugs/ |

### Bitcoin-adjacent

| Class | Cases / notes | Source |
|-------|---------------|--------|
| Stacks Clarity DoS | Catchme ~$76k Immunefi | https://medium.com/immunefi/stacks-dos-bugfix-review-dc0f2a75b276 |
| AlexLab exploits | 2024 $4.3M, 2025 $16.2M | https://rekt.news/leaderboard |
| Ordinals / inscriptions | indexer confusion (research class; few protocol-treasury drains at DeFi scale) | Gap: no single Immunefi mega-payout found |
| Lightning HTLC | historical thefts are often routing/htlc, not "Solidity" | Client playbook |
| BitVM / Babylon | young; audit reports more than public drains | Gap |
| DMM Bitcoin | $304M 2024 rekt (ops/custodial, not script) | https://rekt.news/leaderboard |

### Other VMs

| VM | Class | Public case or canon | Source |
|----|-------|----------------------|--------|
| Starknet / Cairo | felt252 overflow; L1 handler `from_address` | zkLend $9.57M; Vesu rounding disclosures; ToB cairo skill | https://rekt.news/leaderboard ; https://docs.vesu.xyz/security/disclosures-report/rounding-convention-bug-disclosure ; https://trailofbits.com/skills/cairo-vulnerability-scanner/ |
| TON FunC | bounce, end_parse, impure | TONScanner 8 defects (research) | https://arxiv.org/html/2501.06459 |
| NEAR | Rainbow / Aurora engine | Aurora $6M bounty | Immunefi Aurora review |
| ink! / Polkadot | Frontier EVM | pwning.eth $1M Frontier | Mirror writeup |
| Cardano Plutus | signature / validity interval | Plu-Stan rules (tooling) | https://github.com/input-output-hk/plu-stan |
| Tezos Michelson | untrusted entry | tool papers (MichelsonLiSA) | academic; few DeFiHackLabs rows |
| Fuel Sway | (young) | Spearbit/C4 contests more than mega-hacks | Gap |
| Tron | USDD / justlend style oracle | occasional SlowMist rows | SlowMist annual |
| ICP | canister auth | few public mega DeFi | Gap |
| Hedera | 2023 $515k | https://rekt.news/leaderboard | |
| Algorand | TEAL auth | sparse in rekt top 300 | Gap |
| XRPL | amendment / issued token | not EVM; sparse in this corpus | Gap |
| FVM (Filecoin) | actor methods | sparse | Gap |
| VeChain | VTHO accrual bounty | https://immunefi.com/blog/all/vechainthor-vtho-accrual-bypass-bug-fix-review/ | |

---

## 5. X-sourced recent (2024-2026) findings with links

Extracted from public X posts **and** the writeups they point to. Handles as published.

| When | Handle | Protocol | Payout (if stated) | Vuln type | Writeup |
|------|--------|----------|--------------------|-----------|---------|
| 2024 | @usmannk | Sei | $2M+$75k | 2x critical chain | https://usmannkhan.com/bug%20reports/2024/06/17/sei-bug-report.html |
| 2024 | @ma1fan Catchme | Sei | $75k | protocol | https://exvul.com/share-the-details-sei-protocol-vulnerability-worth-75k/ |
| 2024 | @riproprip | Raydium | $505k | tick manipulation | https://medium.com/immunefi/raydium-tick-manipulation-bugfix-review-c6aae4527ed6 |
| 2024 | @jayjonah_eth | Evmos | $150k | docs vs consensus | https://medium.com/@jjordanjjordan/150-000-evmos-vulnerability-through-reading-documentation-d26328590a7a |
| 2024 | @gandu_whitehat | Sovryn | $15k | (medium) | https://x.com/gandu_whitehat/status/1803794103248806223 |
| 2024 | @Gregadeth | The Graph | $290k | rounding | https://medium.com/immunefi/the-graph-rounding-error-bugfix-review-c946ff470f65 |
| 2025-02 | @marcotnunes | Wormhole | $50k | critical | https://x.com/marcotnunes/status/1889707212450234629 |
| 2025 | @marcotnunes | Axelar | $50k | cross-chain halt | https://marcotnunes.com/axelar-network-cross-chain-halt-vulnerability/ |
| 2025 | @kankodu | Balancer V2 | $250k | critical | https://mirror.xyz/0x38F1416B9Ed3a5DA9C12c56cb4F74D9564844728/iv9_q74rSlK7gbvbJAECuDIbzfUtrSCO6mSWIHPskKI |
| 2025-03 | @kankodu | Vesu | High | (X) | https://x.com/kankodu/status/1904821401510699389 |
| 2025 | @__alexxander_ | Vesu | - | rounding convention | https://docs.vesu.xyz/security/disclosures-report/rounding-convention-bug-disclosure |
| 2025 | @thel4stc0de | Acala | $70k | block production | https://immunefi.com/blog/all/acala-block-production-shutdown-bug-fix-review/ |
| 2025-04 | @shabarkin | Scroll | $1k | (X) | https://x.com/shabarkin/status/1917483039195816213 |
| 2025-04 | @WhiteHatMage | Scroll | $1M | message spoof | https://forum.scroll.io/t/report-scroll-mainnet-emergency-upgrade-on-2025-04-25/666 |
| 2025 | @zachobront @deadrosesxyz | Across V3 | High | messaging | https://mirror.xyz/0x9D6b7f5e8d1b9dFea8dDD29c0DbD81687e721601/mrt70ckjaZymv9keUy_TzHVIzjBOQr-Hx_KI1ydFeoQ |
| 2025 | @__nnez | VeChainThor | $50k | VTHO accrual | https://immunefi.com/blog/all/vechainthor-vtho-accrual-bypass-bug-fix-review/ |
| 2025 | @0xjuaan @0xSpearmint | Fraxlend | High | lending | https://mirror.xyz/0x22ce3c4ce1EC532437209efA79d05CD294651ec3/M6vD6XshTuZc53DFm0chQwYD15fxQ29G1mbxNi9ZLwU |
| 2025 | @WhiteHatMage @Jiri123_eth | Story | $100k / crit | network postmortem | https://www.story.foundation/blog/story-network-postmortem |
| 2025 | @yemresaritoprak | Movement | $6.71k | chain split | Medium writeup above |
| 2025 | @0xriptide | Lido | High | Dual Governance | https://research.lido.fi/t/security-disclosure-dg-weakness-reported-through-immunefi-funds-not-at-risk/10393 |
| 2025 | F4lt | Sui | $50k | shutdown | https://immunefi.com/blog/bug-fix-reviews/sui-network-shutdown/ |
| 2025 | @_fel1x | marginfi | Critical | flash loan | https://blog.asymmetric.re/threat-contained-marginfi-flash-loan-vulnerability/ |
| 2026 | @s4muraii77 | dHEDGE | High | (X) | https://x.com/s4muraii77/status/2012140371938070888 |
| 2026 | @Ehsan1579 | zkSync Lite | $200k | (X) | https://x.com/Ehsan1579/status/2013482485175226811 |
| 2026 | @therealgregoAI | Gnosis, Yearn, Reserve, Balancer, Uniswap | claimed crits | amounts often unstated | https://x.com/therealgregoAI (multiple status IDs in sayan011 list) |
| 2026 | @al_f4lc0n | Injective | $50k | critical | https://x.com/al_f4lc0n/status/2033110168045568434 |
| 2024-26 | @immunefi | platform | research: 93% of post-launch crits via Immunefi | stats | https://immunefi.com/blog/research/93-of-critical-crypto-vulns-are-disclosed-on-immunefi/ |
| 2024-26 | Sherlock GitHub | ERC-4626 inflation (Napier, Burve, Notional Exponent, New Scope) | contest | T13 | e.g. https://github.com/sherlock-audit/2024-01-napier-judging/issues/125 |
| 2025 | Immunefi reports.immunefi.com | Alchemix V3 audit comp | Critical | liquidation fee overpay | https://reports.immunefi.com/alchemix-v3/58772-sc-critical-resolverepaymentfee-overpays-liquidators-when-collateral-is-gone-letting-attackers |
| 2024 | Spearbit | Uniswap v4 periphery | 4 Medium | tick compression, unsubscribe | Spearbit draft PDF (v4-periphery audits) |
| 2024 | OpenZeppelin | Uniswap v4 periphery + Universal Router | 1 Crit + 1 High (resolved) | router / v4 | https://www.openzeppelin.com/news/uniswap-v4-periphery-and-universal-router-audit |

Accounts monitored for this ingest (from pack `references/sources.md`): `@immunefi` `@rektnews` `@samczsun` `@pcaversaccio` `@pashovkrum` `@bytes032` `@cmichelio` `@0xRajeev` `@officer_cia` `@BlockSecTeam` `@SlowMist_Team` `@trailofbits` `@spearbit` `@ottersec` `@Zellic_io` `@CyfrinAudits` `@code4rena` `@sherlockdefi` `@cantinaxyz` `@hatsfinance` `@DarkNavyOrg` `@AckeeBlockchain` `@sigma_prime` `@CertoraInc` `@neodyme`.

Direct X API scrape was **not** available in this session. Findings above are from indexed web copies of those posts and from the public writeup list that already stores X URLs.

---

## 6. Gaps (could not verify)

- **Exact X thread text** for 2024-2026 from `@samczsun`, `@pcaversaccio`, `@pashovkrum`, `@bytes032`, `@cmichelio`, `@0xRajeev`, `@officer_cia`, `@BlockSecTeam`, `@SlowMist_Team`, `@code4rena`, `@cantinaxyz`, `@hatsfinance`, `@DarkNavyOrg`, `@AckeeBlockchain`, `@sigma_prime`, `@CertoraInc`, `@neodyme` without a live X API. Used Immunefi/rekt/DHL/firm blogs instead.
- **KelpDAO $290M (Apr 2026)** and **Drift $285M (Apr 2025)** appear on rekt.news leaderboard; this ingest did **not** independently re-read a full technical postmortem, so class mapping is incomplete.
- **Usual $16M / Uniswap v4 $15.5M / LayerZero $15M** are program maxima, not paid bugs.
- **GregoAI 2026 X claims** (Gnosis, Yearn, Reserve, Balancer, Uniswap): writeup URLs are tweets; paid amounts mostly unverified.
- **Bitcoin ordinals, BitVM, Babylon, Lightning, Fuel Sway, ICP, Algorand, XRPL, FVM, Tezos, Cardano DeFi treasuries:** few Immunefi-style public mega-payouts found. Tooling papers exist; named **protocol** drains are sparse in rekt top 300.
- **IBC "hault"** as a named incident: IAVL/BNB is the clear mega-case; generic ICS-20 halt writeups are scattered across Cosmos security advisories.
- **Casino (user query):** no single canonical "Casino" protocol row; ZKasino is the closest rekt name (ops, not AMM math).
- **Celer / Hyphen / Hop / Portal / CCTP / LayerZero in-the-wild drains:** some are key/ops or integrator bugs; not all have Solidity-class postmortems at Euler/Nomad quality.
- **Hats Finance payout leaderboard** is on-chain / app-specific; only sample competitions (Velvet, Raft-as-venue) cited.
- **Code4rena 2026:** site shows remaining contests (Panoptic, Hybra) and a wind-down; historical winning reports live on Solodit / GitHub judging repos, too many to paste verbatim.
- **Loss vs bounty double-count:** Wormhole appears as **both** $326M hack (sysvar) and $10M bounty (uninitialized proxy) - different bugs.
- **DeFiHackLabs PoC folders** contain Foundry **reproductions**. This pack must use them as **regression tests only**, never as live runbooks.

---

## 7. Full case cards (required fields)

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

## 8. Compact catalog (DeFiHackLabs named incidents)

Every row is a **public** DeFiHackLabs heading. Chain defaults to **EVM** unless the name is a known L2/Solana/Move protocol. Protocol type inferred from class. **Trusted** and **test idea** are class templates (same as taxonomy T-IDs). Source base: `https://github.com/SunWeb3Sec/DeFiHackLabs` (year folders `past/YYYY/README.md` or main README for 2026).

**Templates**

- **AC (T01):** Trusted `msg.sender`. Grep `onlyOwner|initialize`. Test: prank attacker.
- **ORACLE (T06):** Trusted spot. Grep `getReserves|slot0|latestRoundData`. Test: same-tx skew then value transfer fails.
- **REENT (T16/T18):** Trusted callback. Grep `safeTransfer|onERC721|get_virtual_price`. Test: reenter or view-during-callback.
- **ARB CALL (T31):** Trusted user calldata. Grep `call(|userData`. Test: leftover approval.
- **INFL (T13/T14):** Trusted `totalAssets`/`exchangeRate`. Grep `convertToShares|exchangeRate`. Test: 1 wei + donate.
- **ROUND (T33):** Trusted division. Grep `mulDiv|/ `. Test: 1 wei.
- **SIG (T04):** Trusted `ecrecover`. Grep `permit|ecrecover`. Test: replay / address(0).
- **LOGIC:** Trusted happy path. Grep function name. Test: fuzz inverted invariant.
- **PROXY (T02/T03):** Trusted init/layout. Grep `initialize|upgradeTo`. Test: impl init.
- **SLIP (T32):** Trusted minOut. Grep `amountOutMin`. Test: zero minOut rejected.

### 8.1 2026 (main README)

USM (rounding/ORACLE), Atomic (ORACLE lending), UnistreetLaunchpad (ARB CALL), PantherBase (governance timeout; pre-prod), StrongBlock (AC abandoned governor), AIC (skim/reserves ORACLE), MOKE (AC / EIP-7702 claim), LpdFi (ORACLE interest), UnprotectedArbBot (ARB CALL + allowance), ExchangeIssuance Index Coop (hook TOCTOU INFL), LULA (ORACLE deflation), Pro Token / Projekt vault (self-dealing LOGIC), Lien Finance (bond misprice LOGIC), NFT Auction (double settlement NFT), RWT (deflation ORACLE), CompoundProvider (AC allowance), CrowdRingCircle (burn+sync ORACLE), Perpetual Protocol (AC), Lumi Finance (ERC-4337 paymaster SIG), Sodium (session key SIG), SummerFi (NAV INFL), edel-xstock (ORACLE), Vault4626 (LOGIC/INFL), AIDC (LOGIC), CookFinanceIssuance (ORACLE), LixirPermitDrain (SIG), OceanBPoolSideStaking (Balancer math ROUND), DLMC (livePrice ORACLE), RoyalRoyalties (ERC1155 zero-amount INFL), Aztec Escape Hatch / Aztec V1 / Aztec Connect (proof accounting; zk), ATM / OLPC / JB / WHALE / LBP / DIP (pair/reserve ORACLE or FoT), Thetanuts (share accounting INFL), TOPBPool (gov mint), NovaBox (constructor LOGIC), AmbientCrocSwapDex (surplus LOGIC), BOSS / DTXT / BYToken / ATM Token (tax/pair ORACLE), AISOTHPresale (fixed price LOGIC), AROS (SIG replay), YSDAO (ORACLE+tax), LegendaryMoneyMonNft (ecrecover 0 SIG), DxSale (AC), Joe Agent (REENT), SKP (backdoor), SquidRouterModule (AC), WUSD.fi (sybil LOGIC), FractalProtocol (LOGIC), MureDistribution (SIG), MAPProtocol (mint AC), ElevateFi (ORACLE), TesseraSwap (callback ORACLE), VerusBridge / AdsharesBridge (T26), SEAToken (LOGIC), SQTokenStaking (AC), INKFinance (LOGIC), HumaFinance (credit AC), Renegade (PROXY), TrustedVolumes (SIG), Ekubo (LOGIC), SharwaMarginTrading (ORACLE), RWAVault (ERC4626 allowance), JUDAO (hook ORACLE), Unverified_a152 (allowance ARB CALL), SingularityDynaVault (ORACLE+INFL), GiddyVaultV3 (SIG), KipseliPropAMM (decimals ROUND), JuiceboxREVLoans (validation LOGIC), ThetanutsVaultShareRounding (ROUND), AaveRebalancerCreditDelegation (ARB CALL), XLootStaking (double redeem LOGIC), MONA LisaVault (reward LOGIC), Saturn Protocol (disclosure), SubQuerySettings (AC), SquidMulticallAllowanceDrain (ARB CALL), PerpPair (vAMM ORACLE), WhalebitOracleManipulation (ORACLE), VTSwapHook (v4 hook LOGIC), EST Token (burn LOGIC), XocolatlLiquidator (AC), Univ3CollateralToken (LOGIC), BCE (deflation), ATMBlindBox (RNG T50), Revamp (reward), Venus THE (INFL), StakeOnMe (AC), AlkemiEarn (LOGIC), Curve LlamaLend (INFL), LAXO (burn), XDKRecycle (ORACLE), Moonwell (ORACLE), Makina (ORACLE), SynapLogic (LOGIC), MTToken (fee LOGIC), FutureSwap (units ROUND), Truebit TRU (overflow ROUND), PRXVT (LOGIC).

### 8.2 2025 (`past/2025/README.md`)

yETH (unsafe math ROUND), DRLVaultV3 (ORACLE), Moonwell (ORACLE), BalancerV2 (ROUND), SharwaFinance (insolvency T25), TokenHolder (AC), MIMSpell3 (insolvency T25), NGP (ORACLE), Kame (ARB CALL), Hexotic (validation), EverValueCoin (arb LOGIC), 0xf340 (AC), EquilibriaEPendle (reward LOGIC), ABCCApp (AC), Unverified_6f7a (AC), MulticallWithXera (AC), 0x8d2e (AC), PresaleV5 (ORACLE), AutoPooledTradingBot (LOGIC), d3xai (ORACLE), SizeFlashLoanLooping (ARB CALL), PDZ (ORACLE), SizeCredit (AC), BeefyZapRouter (ARB CALL), YuliAI (ORACLE), coinbase (misconfig), Grizzifi (LOGIC), BaseBebopSettlement / Bebop (ARB CALL), WXC (burn), ArbitrumBaseSwapper (LOGIC), MyCoinMaster (LOGIC), BscInitcodeToken (LOGIC), PendleReflector (LOGIC), AnyswapWETHPermit (SIG), SuperRare (AC), AvaxBIFKNPair (flash accounting), Unverified flash callbacks (T20), MulticallWithETH (ARB CALL), WhereIsMyDragonTreasure (reward), SWAPPStaking (reward), EmptySetReserve (fixed swap), BoJLeverageMarket (index INFL), Stepp2p (LOGIC), WETC/UPENG (burn), StrategyLlamaLendConvex (INFL), GMX (INFL), ActivePoolScrvUsd / UrgentRedemption (LOGIC), RANT/FPC (LOGIC), Stead (AC), InitcodeFactoryFees (AC), ResupplyFi (INFL), SiloFinance (ARB CALL), ParaSwapDAIApproval (T05), GradientMakerPool (ORACLE), TokenVault/HoldSafe/Gangsterfinance/SinstakeZombie/Bankroll* (dividends LOGIC), BasePricePool (ORACLE), MetaPool/WaleCoin (AC), FixedTokenBSwap (ORACLE), TSAggregatorGeneric (LOGIC), AAVEBoost (LOGIC), BitCrown/TokenFactory/PegaBall (AC/LOGIC), DogeAlliance (ORACLE), Corkprotocol (AC), UsualMoney (arb), YDT/Dumbo (LOGIC), RICE (AC), IRYSAI (rug), BetaPresale (AC), KRC (deflation), Bitallx (payout), Unwarp (AC), Nalakuvara lottery (ORACLE), Crosswise (trusted forwarder T09-like), FlyLong (balance forgery), tcdp (transferFrom), bitdog (AC), multitransferswap (LOGIC), AventaRewardClaim (claim), Lifeprotocol/ImpermaxV3 (ORACLE), BTNFT (claim), Roar (rug), YVToken (SLIP), Laundromat (LOGIC), AmpKashi (ORACLE), AIRWA (AC), LeverageSIR (T03 slot), Alkimiya (cast ROUND), YziAI (rug), BBX/DCF (ORACLE/SLIP), wKeyDAO (AC), H2O (RNG), ZeroExSettler (ARB CALL), DUCKVADER (free mint), StackMarket/UNI/SBR (LOGIC/ORACLE), RnsPay (ARB CALL), PTM (LOGIC), 1inch Fusion V1 (ARB CALL yul), Pump (SLIP), Venus_ZKSync (INFL), HenloKart (fake deposit), INVISTECH (tax ORACLE), HegicOptions (LOGIC), Unverified REENT, StepHeroNFTs (REENT), Bybit (phishing ops), Scorch (ORACLE), LimitOrderProtocol (AC), FourMeme (LOGIC), GMT7 (AC), Peapods (ORACLE), GoldReserve (LOGIC), MCAI (tax allowance), AIXBTForcedSwap (hardcoded auth).

### 8.3 2024 (`past/2024/README.md`) - selected named + class

Slur/LABUBU (LOGIC), CloberDEX (REENT), Pledge/NFTG/Ak1111/X319 (AC), Matez (trunc ROUND), MainnetSettler (AC), PolterFinance (ORACLE), vETH (ORACLE), DeltaPrime (REENT), CoW (LOGIC), RPP (LOGIC), BUBAI (rug), CompoundFork (ORACLE), Erc20transfer (AC), Morph (LOGIC), LavaLending (LOGIC), OnyxDAO (LOGIC), Bedrock_DeFi (1:1 mint LOGIC), AIRBTC (LOGIC), Penpiexyz (REENT), Convergence (LOGIC), Spectra (LOGIC), Minterest (REENT), DeFiPlaza (ROUND), MRP (REENT), Sonne (ROUND/INFL), PredyFinance (REENT), SAT/OSN (LOGIC), Yield (LOGIC), PikeFinance (PROXY), BNB (LOGIC), NGFS (LOGIC), Hackathon (LOGIC), SumerMoney (REENT), wsm (LOGIC), HoppyFrogERC (LOGIC), ATM/OpenLeverage (LOGIC), ETHFIN (AC), PrismaFi (validation), ZongZi (ORACLE), ARK (LOGIC), CGT (AC), SSS (self-transfer), Paraswap (AC), MO/IT/BBT (LOGIC), Binemon (ROUND), Juice (LOGIC), UnizenIO (ARB CALL), GHT (LOGIC), ALP (public internal AC), TGBS (LOGIC), WooFi (ORACLE), Seneca (ARB CALL), SMOOFSStaking (REENT), Zoomer (LOGIC), CompoundUni (ORACLE), Blueberry (LOGIC), SwarmMarkets (validation), DeezNutz404 (validation), GAIN (LOGIC), EGGX/RuggedArt (REENT), ParticleTrade (validation), DualPools (ROUND), Babyloogn/Miner (validation), MINER BSC (ORACLE), Game (REENT), FILX DN404 (AC), Pandora404 (underflow), BurnsDefi (ORACLE), ADC (AC), AffineDeFi (userData ARB CALL), XSIJ (LOGIC), MIMSpell (ROUND), Peapods/Barley (REENT), CitadelFinance (ORACLE), NBLGAME (REENT), DAO_SoulMate (AC), BmiZapper (ARB CALL), SocketGateway (ARB CALL), Shell_MEV (AC), WiseLending (health T24), Freedom/LQDX (AC), Gamma (ORACLE), MIC (LOGIC), RadiantCapital (ROUND), OrbitChain (validation T26).

### 8.4 2023 (`past/2023/README.md`) - selected

Channels (ORACLE), ChannelsFinance (INFL), CCV/DominoTT (ROUND), Telcoin (PROXY slot), PineProtocol (LOGIC), TransitFinance (pool validation), Bob/FloorProtocol (ORACLE/LOGIC), GoodDollar (REENT), NFTTrader (REENT), PHIL/HYPR (LOGIC), GoodCompound/BCT (ORACLE), HNet (LOGIC), TIME (address spoof), ElephantStatus/MAMO (ORACLE), BEARNDAO (LOGIC), bZxProtocol (INFL), EEE/CAROL (ORACLE/REENT), AIS (AC), FiberRouter (validation), MetaLend (INFL), KyberSwap (ROUND), LinkDAO (k check), Mahalend/Raft (INFL), SwampFinance (LOGIC), Onyx (ROUND), UniBotRouter/MaestroRouter2/DEXRouter (ARB CALL), HopeLend (ROUND), BelugaDex (ORACLE), WiseLending (INFL), Platypus (LOGIC), StarsArena (REENT), DePayRouter (LOGIC), uniclyNFT (REENT), QuantumWN/JumpFarm/HeavensGate/FloorDAO (rebase T22), Balancer (ROUND), ExactlyProtocol (validation), Zunami (ORACLE), CurveBurner (SLIP), Curve Vyper (REENT compiler), Palmswap (LOGIC), MintoFinance (SIG), Conic 01/02 (REENT+ORACLE), Platypus Jul (LOGIC), RodeoFinance (TWAP ORACLE), Libertify/ArcadiaFi (REENT), Biswap migrator (LOGIC), Themis (ORACLE), MIMSpell (ARB CALL), MidasCapital (ROUND), Sturdy (REENT view), Jimbo (ORACLE), DEI (LOGIC), 0vix (ORACLE), Silo (LOGIC), HundredFinance (INFL), yearnFinance (misconfig), SushiSwap (ARB CALL), Sentiment (REENT view), Allbridge (ORACLE), SafeMoon (AC), ParaSpace NFT (ORACLE), Poolz (overflow), EulerFinance (LOGIC T25), Phoenix (ARB CALL), LaunchZone/SwapX (AC), EFVault (PROXY), Dexible/CowSwap (ARB CALL), Platypusdefi (LOGIC), dForce (REENT view), Orion (REENT), BonqDAO (ORACLE), MidasCapital Jan (REENT view), ROE (ORACLE).

### 8.5 2022 (`past/2022/README.md`) - selected

DFS (validation+flash), JAY (REENT), Rubic (ARB CALL), Defrost (REENT), Nmbplatform (ORACLE), FPR (AC), ElasticSwap (LOGIC), BGLD/AES (deflation ORACLE), Lodestar (ORACLE), NOVAToken (mint rug), AnnexFinance (flash callback T20), Polynomial (validation), DFXFinance (REENT), Kashi (price cache), Team Finance (migration), N00d (REENT), Templedao (AC), EGD (ORACLE), Nomad Bridge (T26), Reaper Farm (AC), Audius (PROXY), Omni NFT (REENT), Quixotic (SIG), XCarnival (borrow), Harmony Horizon (keys), InverseFinance (ORACLE), Optimism Wintermute (PROXY), Fortress (gov+ORACLE), Saddle (metapool), Fei/Rari (REENT), DEUS (ORACLE), BeanstalkFarms (gov), Rikkei (AC+ORACLE), Ronin (bridge keys), Redacted Cartel (approve), Revest (REENT), Compound TUSD sweep (LOGIC), OneRing (ORACLE), LI.FI (bridge), plus Cashio/Wormhole/Qubit/Cream-class rows in section 3.

### 8.6 2020-2021 famous remainder

Harvest, Pickle, Value DeFi series, Cover, Eminence, Warp, Origin, Akropolis, DODO, Furucombo, Yearn v1, Alpha Homora, PancakeBunny, Uranium, Meerkat, Belt (hack vs later bounty), Cream x2, Poly Network, Indexed, Popsicle, Grim, MonoX, xToken, Spartan, BurgerSwap, EasyFi, Paid, Vee, bZx, Compound COMP, Badger (web2 API keys - ops), Anyswap, ChainSwap, Saddle small 2021, Eleven, Bondly, Levyathan, Punk Protocol, Rari 2021, Alchemix 2021, Roll, DAO Maker, JayPegs.

---

## 9. How to use this ingest

1. Pick a playbook from section 1.
2. Open matching case cards (section 7) plus compact rows (section 8).
3. Grep **in-scope** source only (`references/grep-patterns.md`).
4. Write a **local** Foundry/Anchor/Move test that asserts the invariant. Do not broadcast.
5. Report via `contest-and-bounty-reporting` if in bounty/contest scope.

Primary indexes to re-run on the next ingest:

- https://github.com/SunWeb3Sec/DeFiHackLabs
- https://rekt.news/leaderboard
- https://immunefi.com/blog/bug-fix-reviews/
- https://github.com/sayan011/Immunefi-bug-bounty-writeups-list
- https://solodit.xyz/
- https://github.com/immunefi-team/Web3-Security-Library/blob/main/BugFixReviews/README.md

---

## 10. Structured DeFiHackLabs catalog (one row per named incident)

Parsed from the public DeFiHackLabs README index (retrieved 2026-08-26).
Each row fills the required fields via the class template in section 1.
Chain is a best-effort guess from the name; confirm in the DHL entry.
**Do not** treat DHL PoC folders as live attack scripts; only local regression tests.

**Count:** 852 unique dated headings.

| Date | Name | Year | Chain/VM | Protocol type | Class | Wrongly trusted | Grep seeds | Local test idea | Playbook | Source |
|------|------|------|----------|---------------|-------|-----------------|------------|-----------------|----------|--------|
| 20260809 | USM | 2026 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260809-usm---defund-price-split-invariance-rounding-exploit |
| 20260807 | Atomic | 2026 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260807-atomic---flash-loan-price-oracle-manipulation-of-lending-collateral-valuation |
| 20260806 | UnistreetLaunchpad | 2026 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260806-unistreetlaunchpad---arbitrary-call-injection-via-unvalidated-launch-forwarding |
| 20260806 | PantherBase | 2026 | EVM (see DHL entry) | governance | T29/T30 | vote snapshot / delay | `propose(/queue(/execute(/getVotes` | flash-loan votes cannot pass if snapshot correct | `reviewing-governance-and-timelocks` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260806-pantherbase---realityeth-governance-timeout-exploit-pre-production-base-deployment-no-user-funds |
| 20260805 | StrongBlock | 2026 | Tron | governance | T29/T30 | vote snapshot / delay | `propose(/queue(/execute(/getVotes` | flash-loan votes cannot pass if snapshot correct | `reviewing-governance-and-timelocks` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260805-strongblock---governance-takeover-of-abandoned-governor |
| 20260803 | AIC | 2026 | EVM (see DHL entry) | weird ERC-20 | T22 | pair balance vs reserves | `skim/sync/fee/rebase/burn` | FoT / reflection path: recipient gets expected minOut | `reviewing-token-standard-pitfalls` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260803-aic---pair-skim--reserve-mismatch-exploit-flash-swap-leveraged |
| 20260802 | MOKE | 2026 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260802-moke---unprotected-claim-drained-via-eip-7702-self-delegation |
| 20260802 | LpdFi (LOOPSDAO) | 2026 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260802-lpdfi-loopsdao---spot-price-manipulation--issue-boundary-interest-exploit |
| 20260730 | UnprotectedArbBot | 2026 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260730-unprotectedarbbot---unprotected-arbitrary-call-forwarder-drained-via-pre-granted-weth-allowance |
| 20260730 | ExchangeIssuance (Index Coop) | 2026 | EVM (see DHL entry) | share / ERC-4626 inflation | T13/T14 | totalAssets or exchangeRate from donatable balance | `convertToShares/exchangeRate/totalAssets/donate` | 1 wei first deposit + donate; next depositor shares != 0 | `reviewing-erc4626-and-vaults` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260730-exchangeissuance-index-coop---toctou-positionmultiplier-inflation-via-malicious-pre-issue-hook |
| 20260726 | LULA | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260726-lula---reward-recycle-deflation-manipulation-via-flash-loan |
| 20260725 | Pro Token | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260725-pro-token---reward-on-transfer-self-dealing-winner-drain |
| 20260725 | Projekt Reward Vault | 2026 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260725-projekt-reward-vault---permissionless-purchase-tracking-self-dealing |
| 20260724 | Lien Finance | 2026 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260724-lien-finance---permissionless-bond-registration--payoff-mispricing |
| 20260719 | NFT Auction Marketplace | 2026 | EVM (see DHL entry) | NFT / marketplace | T19 | hook or listing sig | `onERC721Received/orderHash/nonce` | cancelled listing cannot fill; hook cannot drain before state update | `reviewing-nft-and-marketplace` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260719-nft-auction-marketplace---double-settlement--delist-refund |
| 20260719 | RWT Token | 2026 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260719-rwt-token---deflationary-burn-from-pair-price-manipulation |
| 20260716 | CompoundProvider | 2026 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260716-compoundprovider---allowance-sweep--missing-access-control |
| 20260716 | CrowdRingCircle | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260716-crowdringcircle---reserve-manipulation-via-burn-from-pair--sync |
| 20260716 | Perpetual Protocol | 2026 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260716-perpetual-protocol---access-control--missing-permission-check |
| 20260714 | Lumi Finance | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260714-lumi-finance---erc-4337-validation-phase-paymaster-approval |
| 20260712 | Sodium | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260712-sodium---erc-4337-session-key-validation-bypass |
| 20260706 | SummerFi | 2026 | EVM (see DHL entry) | share / ERC-4626 inflation | T13/T14 | totalAssets or exchangeRate from donatable balance | `convertToShares/exchangeRate/totalAssets/donate` | 1 wei first deposit + donate; next depositor shares != 0 | `reviewing-erc4626-and-vaults` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260706-summerfi---fleetcommander-nav-inflation-via-depegged-xusd |
| 20260701 | edel-xstock | 2026 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260701-edel-xstock---price-oracle-manipulation |
| 20260629 | Vault4626 | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260629-vault4626---business-logic-flaw |
| 20260628 | AIDC | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260628-aidc---business-logic-flaw |
| 20260627 | CookFinanceIssuance | 2026 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260627-cookfinanceissuance---price-oracle-manipulation |
| 20260625 | LixirPermitDrain | 2026 | EVM (see DHL entry) | signature | T04 | ecrecover / nonce / chainId | `ecrecover/permit(/_hashTypedDataV4/nonce` | replay same sig; empty sig != address(0) success | `reviewing-signatures-permit-and-eip712` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260625-lixirpermitdrain---broken-signature-verification |
| 20260625 | OceanBPoolSideStaking | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260625-oceanbpoolsidestaking---bpool-single-sided-joinexit-math-with-sidestaking-gulp-accounting |
| 20260624 | DLMC | 2026 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260624-dlmc---reserve-derived-liveprice-manipulation |
| 20260623 | RoyalRoyalties | 2026 | EVM (see DHL entry) | NFT / marketplace | T19 | hook or listing sig | `onERC721Received/orderHash/nonce` | cancelled listing cannot fill; hook cannot drain before state update | `reviewing-nft-and-marketplace` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260623-royalroyalties---zero-amount-erc1155-batch-transfer-inflated-royal-lda-tier-balance |
| 20260622 | Aztec Escape Hatch | 2026 | Aztec/zk | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260622-aztec-escape-hatch---proof_id-accounting-bypass-whitehat-reproduction |
| 20260622 | ATM | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260622-atm---lp-token-burn |
| 20260620 | OLPC | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260620-olpc---olpc-pair-reserve-manipulation |
| 20260618 | JB | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260618-jb---jb-helper-repeated-cycle-drains-jbusdt-pair |
| 20260617 | WHALE | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260617-whale---transfer-accounting-reserve-desync |
| 20260617 | LBP | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260617-lbp---lbp-balanceof-reward-accounting |
| 20260617 | Aztec V1 | 2026 | Aztec/zk | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260617-aztec-v1---escapehatch-proof-forgery-permissionless-rollupprocessor-exit |
| 20260616 | DIP | 2026 | EVM (see DHL entry) | weird ERC-20 | T22 | pair balance vs reserves | `skim/sync/fee/rebase/burn` | FoT / reflection path: recipient gets expected minOut | `reviewing-token-standard-pitfalls` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260616-dip---fee-on-transfer-reserve-manipulation |
| 20260615 | Thetanuts | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260615-thetanuts---index-vault-component-share-accounting-flaw |
| 20260614 | Aztec Connect | 2026 | Aztec/zk | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260614-aztec-connect---numrealtxs-proofsettlement-mismatch-permissionless-rollupprocessorv3 |
| 20260609 | TOPBPool | 2026 | EVM (see DHL entry) | governance | T29/T30 | vote snapshot / delay | `propose(/queue(/execute(/getVotes` | flash-loan votes cannot pass if snapshot correct | `reviewing-governance-and-timelocks` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260609-topbpool---governance-controlled-token-mint-and-balancer-pool-drain |
| 20260609 | NovaBox | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260609-novabox---constructor-dividend-checkpoint-bypass |
| 20260607 | AmbientCrocSwapDex | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260607-ambientcrocswapdex---native-surplus-accounting-flaw |
| 20260606 | BOSS | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260606-boss---boss-helper-mintburn-and-transfer-tax-pool-skew |
| 20260605 | DTXT | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260605-dtxt---liquidity-misclassification-fee-bypass |
| 20260605 | AISOTHPresale | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260605-aisothpresale---fixed-price-presale-arbitrage |
| 20260604 | BYToken | 2026 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260604-bytoken---permissionless-triggerautoburn-reserve-manipulation |
| 20260604 | ATM Token | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260604-atm-token---hidden-transferfrom-auto-swap-drain |
| 20260530 | AROS | 2026 | EVM (see DHL entry) | signature | T04 | ecrecover / nonce / chainId | `ecrecover/permit(/_hashTypedDataV4/nonce` | replay same sig; empty sig != address(0) success | `reviewing-signatures-permit-and-eip712` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260530-aros---signature-replay |
| 20260529 | YSDAO | 2026 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260529-ysdao---price-manipulation-and-tax-bypass |
| 20260528 | LegendaryMoneyMonNft | 2026 | EVM (see DHL entry) | signature | T04 | ecrecover / nonce / chainId | `ecrecover/permit(/_hashTypedDataV4/nonce` | replay same sig; empty sig != address(0) success | `reviewing-signatures-permit-and-eip712` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260528-legendarymoneymonnft---ecrecover-address0-signature-bypass |
| 20260528 | DxSale | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260528-dxsale---ownership-override-attack |
| 20260527 | Joe Agent | 2026 | Sui/Move | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260527-joe-agent---reentrancy-in-removeliquidityviacontract |
| 20260526 | SKP Token | 2026 | EVM (see DHL entry) | ops / rug | T01 | owner privileges | `onlyOwner/mint(/setFee` | assert non-owner cannot mint or pull LP | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260526-skp-token---owner-backdoor-lp-burn--price-manipulation |
| 20260525 | SquidRouterModule | 2026 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260525-squidroutermodule---missing-caller-check |
| 20260525 | New Market Trading | 2026 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260525-new-market-trading---squidroutermodule-missing-caller-check |
| 20260525 | WUSD.fi | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260525-wusdfi---_englove-sybil-incentive-abuse |
| 20260522 | FractalProtocol | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260522-fractalprotocol---business-logic-flaw |
| 20260521 | MureDistribution | 2026 | EVM (see DHL entry) | signature | T04 | ecrecover / nonce / chainId | `ecrecover/permit(/_hashTypedDataV4/nonce` | replay same sig; empty sig != address(0) success | `reviewing-signatures-permit-and-eip712` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260521-muredistribution---signature-verification-bypass |
| 20260520 | MAPProtocol | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260520-mapprotocol---arbitrary-mint |
| 20260519 | ElevateFi | 2026 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260519-elevatefi---reserve-price-manipulation |
| 20260518 | TesseraSwap | 2026 | EVM (see DHL entry) | callback spoof | T20 | callback caller is a real pool | `uniswapV2Call/uniswapV3SwapCallback` | callback from non-pool address reverts | `reviewing-amm-and-cl-pools` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260518-tesseraswap---callback-repayment-price-spread |
| 20260517 | VerusBridge | 2026 | EVM (see DHL entry) | bridge / message | T26 | payload origin / nonce | `processMessage/lzReceive/relay/origin` | replay message id reverts; wrong origin reverts | `reviewing-bridges-and-messaging` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260517-verusbridge---insufficient-validation |
| 20260517 | SEAToken | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260517-seatoken---business-logic-flaw |
| 20260515 | AdsharesBridge | 2026 | EVM (see DHL entry) | bridge / message | T26 | payload origin / nonce | `processMessage/lzReceive/relay/origin` | replay message id reverts; wrong origin reverts | `reviewing-bridges-and-messaging` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260515-adsharesbridge---insufficient-validation |
| 20260512 | SQTokenStaking | 2026 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260512-sqtokenstaking---access-control |
| 20260511 | INKFinance | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260511-inkfinance---business-logic-flaw |
| 20260511 | HumaFinance | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260511-humafinance---credit-approval-bypass |
| 20260510 | Renegade | 2026 | EVM (see DHL entry) | proxy / storage | T02/T03 | initialize / slot layout | `initialize(/upgradeTo/_authorizeUpgrade` | implementation initialize already disabled; admin slot stable | `reviewing-upgradeable-proxies` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260510-renegade---uninitialized-proxy |
| 20260507 | TrustedVolumes | 2026 | EVM (see DHL entry) | signature | T04 | ecrecover / nonce / chainId | `ecrecover/permit(/_hashTypedDataV4/nonce` | replay same sig; empty sig != address(0) success | `reviewing-signatures-permit-and-eip712` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260507-trustedvolumes---signature-replay |
| 20260505 | Ekubo | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260505-ekubo---business-logic-flaw |
| 20260501 | SharwaMarginTrading | 2026 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260501-sharwamargintrading---hegic-collateral-spot-price-manipulation |
| 20260428 | RWAVault | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260428-rwavault---missing-erc4626-allowance-check |
| 20260428 | JUDAO | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260428-judao---judao-sell-hook-reserve-drain |
| 20260427 | Unverified_a152 | 2026 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260427-unverified_a152---allowancetarget-approval-drain |
| 20260425 | SingularityDynaVault | 2026 | EVM (see DHL entry) | share / ERC-4626 inflation | T13/T14 | totalAssets or exchangeRate from donatable balance | `convertToShares/exchangeRate/totalAssets/donate` | 1 wei first deposit + donate; next depositor shares != 0 | `reviewing-erc4626-and-vaults` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260425-singularitydynavault---oracle-misconfiguration--share-inflation |
| 20260423 | GiddyVaultV3 | 2026 | EVM (see DHL entry) | signature | T04 | ecrecover / nonce / chainId | `ecrecover/permit(/_hashTypedDataV4/nonce` | replay same sig; empty sig != address(0) success | `reviewing-signatures-permit-and-eip712` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260423-giddyvaultv3---incomplete-signature-coverage |
| 20260421 | KipseliPropAMM | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260421-kipselipropamm---pricing--decimals-mismatch |
| 20260420 | JuiceboxREVLoans | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260420-juiceboxrevloans---fake-terminal-loan-source-validation-bypass |
| 20260420 | ThetanutsVaultShareRounding | 2026 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260420-thetanutsvaultsharerounding---vault-share-rounding-manipulation |
| 20260419 | AaveRebalancerCreditDelegation | 2026 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260419-aaverebalancercreditdelegation---arbitrary-external-call--credit-delegation-abuse |
| 20260415 | XLootStaking | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260415-xlootstaking---duplicate-xloot-redemption |
| 20260414 | MONA LisaVault | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260414-mona-lisavault---reward-farming--burnaddress-accounting-exploit |
| 20260414 | Saturn Protocol | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260414-saturn-protocol---vulnerability-disclosure |
| 20260412 | SubQuerySettings | 2026 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260412-subquerysettings---settings-access-control |
| 20260407 | SquidMulticallAllowanceDrain | 2026 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260407-squidmulticallallowancedrain---arbitrary-call--wrong-approval |
| 20260405 | PerpPair | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260405-perppair---virtual-amm-manipulation |
| 20260331 | WhalebitOracleManipulation | 2026 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260331-whalebitoraclemanipulation---algebra-spot-price-oracle-manipulation |
| 20260328 | VTSwapHook | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260328-vtswaphook---pricing-error-in-uniswapv4-hook |
| 20260327 | EST Token | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260327-est-token---incorrect-token-burn-mechanism |
| 20260324 | XocolatlLiquidator | 2026 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260324-xocolatlliquidator---access-control--input-validation |
| 20260324 | Univ3CollateralToken | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260324-univ3collateraltoken---logic-error |
| 20260323 | BCE | 2026 | EVM (see DHL entry) | weird ERC-20 | T22 | pair balance vs reserves | `skim/sync/fee/rebase/burn` | FoT / reflection path: recipient gets expected minOut | `reviewing-token-standard-pitfalls` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260323-bce---deflationary-token-logic-error |
| 20260319 | ATMBlindBox | 2026 | EVM (see DHL entry) | weak randomness | T50 | block variables as seed | `blockhash/timestamp/random` | same seed is deterministic in test (must not pay jackpot) | `reviewing-mev-ordering-and-slippage` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260319-atmblindbox---weak-randomness--predictable-rng |
| 20260319 | Revamp | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260319-revamp---reward-accounting-drain |
| 20260316 | unverified | 2026 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260316-unverified---checkoutpool-old-boc-missing-access-control |
| 20260315 | Venus THE | 2026 | EVM (see DHL entry) | share / ERC-4626 inflation | T13/T14 | totalAssets or exchangeRate from donatable balance | `convertToShares/exchangeRate/totalAssets/donate` | 1 wei first deposit + donate; next depositor shares != 0 | `reviewing-erc4626-and-vaults` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260315-venus-the---borrowbehalf--donation-attack |
| 20260315 | StakeOnMe | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260315-stakeonme---owner-privileged-jake-burn-reserve-drain |
| 20260310 | AlkemiEarn | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260310-alkemiearn---business-logic |
| 20260302 | Curve LlamaLend | 2026 | EVM (see DHL entry) | share / ERC-4626 inflation | T13/T14 | totalAssets or exchangeRate from donatable balance | `convertToShares/exchangeRate/totalAssets/donate` | 1 wei first deposit + donate; next depositor shares != 0 | `reviewing-erc4626-and-vaults` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260302-curve-llamalend---share-price-manipulation |
| 20260222 | LAXO Token | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260222-laxo-token---incorrect-burn-logic |
| 20260216 | XDKRecycle | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260216-xdkrecycle---xdk-recycle-reserve-manipulation |
| 20260215 | Moonwell | 2026 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260215-moonwell---faulty-oracle |
| 20260120 | Makina | 2026 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260120-makina---price-oracle-manipulation |
| 20260120 | SynapLogic | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260120-synaplogic---business-logic-flaw |
| 20260112 | MTToken | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260112-mttoken---incorrect-fee-logic |
| 20260110 | FutureSwap | 2026 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260110-futureswap---unit-mismatch |
| 20260109 | TRU | 2026 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260109-truebit---overflow |
| 20260101 | PRXVT | 2026 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs?tab=readme-ov-file#20260101-prxvt---bussiness-logic-flaw |
| 20251201 | yETH | 2025 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20251201-yeth---unsafe-math |
| 20251110 | DRLVaultV3 | 2025 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20251110-drlvaultv3---price-manipulation |
| 20251104 | Moonwell | 2025 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20251104-moonwell---faulty-oracle |
| 20251103 | BalancerV2 | 2025 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20251103-balancerv2---precision-loss |
| 20251020 | SharwaFinance | 2025 | EVM (see DHL entry) | liquidation / insolvency | T24/T25 | donate/repay skipping liquidity | `liquidate/donate/healthFactor` | after donate-to-zero, liquidate cannot mint profit | `reviewing-lending-and-liquidations` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20251020-sharwafinance---post-insolvency-check |
| 20251007 | TokenHolder | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20251007-tokenholder---access-control |
| 20251004 | MIMSpell3 | 2025 | EVM (see DHL entry) | liquidation / insolvency | T24/T25 | donate/repay skipping liquidity | `liquidate/donate/healthFactor` | after donate-to-zero, liquidate cannot mint profit | `reviewing-lending-and-liquidations` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20251004-mimspell3---bypassed-insolvency-check |
| 20250918 | NGP | 2025 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250918-ngp---price-manipulation |
| 20250913 | Kame | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250913-kame---arbitary-external-call |
| 20250831 | Hexotic | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250831-hexotic---incorrect-input-validation |
| 20250830 | EverValueCoin | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250830-evervaluecoin---arbitrage |
| 20250827 | 0xf340 | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250827-0xf340---access-control |
| 20250823 | EquilibriaEPendle | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250823-equilibriaependle---reward-accounting-flaw |
| 20250823 | ABCCApp | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250823-abccapp---lack-of-access-control |
| 20250822 | Unverified_6f7a | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250822-unverified_6f7a---access-control |
| 20250820 | MulticallWithXera | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250820-multicallwithxera---access-control |
| 20250820 | 0x8d2e | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250820-0x8d2e---access-control |
| 20250819 | PresaleV5 | 2025 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250819-presalev5---price-manipulation |
| 20250817 | AutoPooledTradingBot | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250817-autopooledtradingbot---business-logic-flaw |
| 20250816 | unverified | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250816-unverified---business-logic-flaw |
| 20250816 | d3xai | 2025 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250816-d3xai---price-manipulation |
| 20250815 | SizeFlashLoanLooping | 2025 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250815-sizeflashloanlooping---arbitrary-external-call |
| 20250815 | PDZ | 2025 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250815-pdz---price-manipulation |
| 20250815 | SizeCredit | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250815-sizecredit---access-control |
| 20250813 | BeefyZapRouter | 2025 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250813-beefyzaprouter---arbitrary-external-call |
| 20250813 | YuliAI | 2025 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250813-yuliai---price-manipulation |
| 20250813 | coinbase | 2025 | EVM (see DHL entry) | ops / key | T28 | operators / off-chain | n/a (ops) | process review: threshold and hardware isolation | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250813-coinbase---misconfiguration |
| 20250813 | Grizzifi | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250813-grizzifi---logic-flaw |
| 20250812 | BaseBebopSettlement | 2025 | OP-stack L2 | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250812-basebebopsettlement---arbitrary-external-call |
| 20250812 | Bebop | 2025 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250812-bebop---arbitrary-user-input |
| 20250811 | WXC | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250811-wxc---incorrect-token-burn-mechanism |
| 20250808 | ArbitrumBaseSwapper | 2025 | Arbitrum | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250808-arbitrumbaseswapper---business-logic-flaw |
| 20250805 | MyCoinMaster | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250805-mycoinmaster---business-logic-flaw |
| 20250804 | BscInitcodeToken | 2025 | BSC | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250804-bscinitcodetoken---business-logic-flaw |
| 20250801 | PendleReflector | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250801-pendlereflector---business-logic-flaw |
| 20250729 | AnyswapWETHPermit | 2025 | EVM (see DHL entry) | signature | T04 | ecrecover / nonce / chainId | `ecrecover/permit(/_hashTypedDataV4/nonce` | replay same sig; empty sig != address(0) success | `reviewing-signatures-permit-and-eip712` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250729-anyswapwethpermit---permit-validation-bypass |
| 20250728 | SuperRare | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250728-superrare---access-control |
| 20250728 | AvaxBIFKNPair | 2025 | Avalanche | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250728-avaxbifknpair---flash-swap-accounting |
| 20250726 | Unverified670471 | 2025 | EVM (see DHL entry) | callback spoof | T20 | callback caller is a real pool | `uniswapV2Call/uniswapV3SwapCallback` | callback from non-pool address reverts | `reviewing-amm-and-cl-pools` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250726-unverified670471---unchecked-flash-loan-callback |
| 20250726 | Unverified6883 | 2025 | EVM (see DHL entry) | callback spoof | T20 | callback caller is a real pool | `uniswapV2Call/uniswapV3SwapCallback` | callback from non-pool address reverts | `reviewing-amm-and-cl-pools` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250726-unverified6883---fake-uniswap-callback |
| 20250726 | MulticallWithETH | 2025 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250726-multicallwitheth---arbitrary-call |
| 20250725 | WhereIsMyDragonTreasure | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250725-whereismydragontreasure---fixed-reward-redemption |
| 20250724 | SWAPPStaking | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250724-swappstaking---incorrect-reward-calculation |
| 20250724 | EmptySetReserve | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250724-emptysetreserve---fixed-order-swap |
| 20250721 | BoJLeverageMarket | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250721-bojleveragemarket---liquidity-index-manipulation |
| 20250720 | Stepp2p | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250720-stepp2p---logic-flaw |
| 20250717 | unverified | 2025 | EVM (see DHL entry) | signature | T04 | ecrecover / nonce / chainId | `ecrecover/permit(/_hashTypedDataV4/nonce` | replay same sig; empty sig != address(0) success | `reviewing-signatures-permit-and-eip712` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250717-unverified---signature-verification |
| 20250717 | WETC | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250717-wetc---incorrect-burn-logic |
| 20250716 | StrategyLlamaLendConvex | 2025 | EVM (see DHL entry) | share / ERC-4626 inflation | T13/T14 | totalAssets or exchangeRate from donatable balance | `convertToShares/exchangeRate/totalAssets/donate` | 1 wei first deposit + donate; next depositor shares != 0 | `reviewing-erc4626-and-vaults` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250716-strategyllamalendconvex---share-price-manipulation |
| 20250716 | VDS | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250716-vds---logic-flaw |
| 20250713 | UPENG | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250713-upeng---incorrect-burn-logic |
| 20250709 | GMX | 2025 | EVM (see DHL entry) | share / ERC-4626 inflation | T13/T14 | totalAssets or exchangeRate from donatable balance | `convertToShares/exchangeRate/totalAssets/donate` | 1 wei first deposit + donate; next depositor shares != 0 | `reviewing-erc4626-and-vaults` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250709-gmx---share-price-manipulation |
| 20250705 | ActivePoolScrvUsd | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250705-activepoolscrvusd---urgent-redemption |
| 20250705 | Unverified_54cd | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250705-unverified---access-control |
| 20250705 | RANT | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250705-rant---logic-flaw |
| 20250702 | FPC | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250702-fpc---logic-flaw |
| 20250702 | ActivePoolUrgentRedemption | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250702-activepoolurgentredemption---urgent-redemption |
| 20250629 | Stead | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250629-stead---access-control |
| 20250628 | InitcodeFactoryFees | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250628-initcodefactoryfees---access-control |
| 20250626 | ResupplyFi | 2025 | EVM (see DHL entry) | share / ERC-4626 inflation | T13/T14 | totalAssets or exchangeRate from donatable balance | `convertToShares/exchangeRate/totalAssets/donate` | 1 wei first deposit + donate; next depositor shares != 0 | `reviewing-erc4626-and-vaults` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250626-resupplyfi---share-price-manipulation |
| 20250625 | Unverified_b5cb | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250625-unverified_b5cb---access-control |
| 20250625 | SiloFinance | 2025 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250625-silofinance---arbitrary-call |
| 20250625 | ParaSwapDAIApproval | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250625-paraswapdaiapproval---stale-approval |
| 20250623 | GradientMakerPool | 2025 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250623-gradientmakerpool---price-oracle-manipulation |
| 20250620 | TokenVault | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250620-tokenvault---incorrect-dividends-calculation |
| 20250620 | HoldSafe | 2025 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250620-holdsafe---price-manipulation |
| 20250620 | Gangsterfinance | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250620-gangsterfinance---incorrect-dividends |
| 20250619 | SinstakeZombie | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250619-sinstakezombie---incorrect-dividends-calculation |
| 20250619 | BasePricePool | 2025 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250619-basepricepool---price-manipulation |
| 20250619 | BankrollStack | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250619-bankrollstack---incorrect-dividends-calculation |
| 20250619 | BankrollNetwork | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250619-bankrollnetwork---incorrect-dividends-calculation |
| 20250618 | BankrollStackPlus | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250618-bankrollstackplus---incorrect-dividends-calculation |
| 20250617 | MetaPool | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250617-metapool---access-control |
| 20250615 | WaleCoin | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250615-walecoin---access-control |
| 20250615 | FixedTokenBSwap | 2025 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250615-fixedtokenbswap---price-oracle-manipulation |
| 20250614 | TSAggregatorGeneric | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250614-tsaggregatorgeneric---business-logic-flaw |
| 20250612 | AAVEBoost | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250612-aaveboost---logic-flaw |
| 20250610 | unverified_8490 | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250610-unverified_8490---access-control |
| 20250609 | BitCrown | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250609-bitcrown---access-control |
| 20250608 | TokenFactory | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250608-tokenfactory---business-logic-flaw |
| 20250603 | PegaBall | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250603-pegaball---business-logic-flaw |
| 20250531 | DogeAlliance | 2025 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250531-dogealliance---price-manipulation |
| 20250528 | Corkprotocol | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250528-corkprotocol---access-control |
| 20250528 | Unverified_91a1 | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250528-unverified_91a1---access-control |
| 20250527 | UsualMoney | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250527-usualmoney---arbitrage |
| 20250526 | YDT | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250526-ydt---logic-flaw |
| 20250525 | Unverified_0000 | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250525-unverified_0000---access-control |
| 20250525 | Dumbo | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250525-dumbo---business-logic-flaw |
| 20250524 | RICE | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250524-rice---lack-of-access-control |
| 20250520 | IRYSAI | 2025 | EVM (see DHL entry) | ops / rug | T01 | owner privileges | `onlyOwner/mint(/setFee` | assert non-owner cannot mint or pull LP | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250520-irysai---rug-pull |
| 20250519 | BetaPresale | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250519-betapresale---access-control |
| 20250518 | KRC | 2025 | EVM (see DHL entry) | weird ERC-20 | T22 | pair balance vs reserves | `skim/sync/fee/rebase/burn` | FoT / reflection path: recipient gets expected minOut | `reviewing-token-standard-pitfalls` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250518-krc---deflationary-token |
| 20250516 | Bitallx | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250516-bitallx---payout-amount-mismatch |
| 20250514 | Unwarp | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250514-unwarp---lack-of-access-control |
| 20250511 | MBUToken | 2025 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250511-mbutoken---price-manipulation-not-confirmed |
| 20250509 | Nalakuvara_LotteryTicket50 | 2025 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250509-nalakuvara_lotteryticket50---price-manipulation |
| 20250506 | Crosswise | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250506-crosswise---trusted-forwarder-spoof |
| 20250429 | FlyLong | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250429-flylong---balance-forgery |
| 20250428 | tcdp | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250428-tcdp---broken-transferfrom-allowance-check |
| 20250427 | bitdog | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250427-bitdog---access-control |
| 20250427 | multitransferswap | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250427-multitransferswap---business-logic-flaw |
| 20250427 | AventaRewardClaim | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250427-aventarewardclaim---claim-accounting |
| 20250426 | Lifeprotocol | 2025 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250426-lifeprotocol---price-manipulation |
| 20250426 | ImpermaxV3 | 2025 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250426-impermaxv3---flashloan-price-oracle-manipulation |
| 20250418 | BTNFT | 2025 | EVM (see DHL entry) | NFT / marketplace | T19 | hook or listing sig | `onERC721Received/orderHash/nonce` | cancelled listing cannot fill; hook cannot drain before state update | `reviewing-nft-and-marketplace` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250418-btnft---claim-rewards-without-protection |
| 20250416 | Roar | 2025 | EVM (see DHL entry) | ops / rug | T01 | owner privileges | `onlyOwner/mint(/setFee` | assert non-owner cannot mint or pull LP | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250416-roar---rug-pull |
| 20250416 | YVToken | 2025 | EVM (see DHL entry) | missing slippage | T32 | no minOut / deadline | `amountOutMin/deadline/minAmount` | zero minOut rejected or documented as user risk | `reviewing-mev-ordering-and-slippage` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250416-yvtoken---not-slippage-protection |
| 20250411 | Unverified 0x6077 | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250411-unverified-0x6077---lack-of-access-control |
| 20250408 | Laundromat | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250408-laundromat---logic-flaw |
| 20250407 | AmpKashi | 2025 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250407-ampkashi---amp-collateral-borrow-price-manipulation |
| 20250404 | AIRWA | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250404-airwa---access-control |
| 20250330 | LeverageSIR | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250330-leveragesir---storage-slot1-collision |
| 20250328 | Alkimiya_IO | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250328-alkimiya_io---unsafecast |
| 20250327 | YziAIToken | 2025 | EVM (see DHL entry) | ops / rug | T01 | owner privileges | `onlyOwner/mint(/setFee` | assert non-owner cannot mint or pull LP | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250327-yziai---rug-pull |
| 20250320 | BBXToken | 2025 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250320-bbxtoken---price-manipulation |
| 20250318 | DCFToken | 2025 | EVM (see DHL entry) | missing slippage | T32 | no minOut / deadline | `amountOutMin/deadline/minAmount` | zero minOut rejected or documented as user risk | `reviewing-mev-ordering-and-slippage` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250318-dcftoken---lack-of-slippage-protection |
| 20250318 | unverified | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250318-unverified---business-logic-flaw |
| 20250316 | wKeyDAO | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250316-wkeydao---unprotected-function |
| 20250314 | H2O | 2025 | EVM (see DHL entry) | weak randomness | T50 | block variables as seed | `blockhash/timestamp/random` | same seed is deterministic in test (must not pay jackpot) | `reviewing-mev-ordering-and-slippage` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250314-h2o---weak-random-mint |
| 20250311 | ZeroExSettler | 2025 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250311-zeroexsettler---arbitrary-external-call |
| 20250311 | DUCKVADER | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250311-duckvader---free-mint-bug |
| 20250307 | StackMarket | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250307-stackmarket---business-logic-flaw |
| 20250307 | UNI | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250307-uni---logic-flaw |
| 20250307 | SBRToken | 2025 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250307-sbr-token---price-manipulation |
| 20250306 | unverified | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250306-unverified---business-logic-flaw |
| 20250306 | RnsPay | 2025 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250306-rnspay---arbitrary-external-call |
| 20250306 | PTM | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250306-ptm---business-logic-flaw |
| 20250305 | 1inch Fusion V1 Settlement | 2025 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250305-1inch-fusionv1-settlement---arbitrary-yul-calldata |
| 20250304 | Pump | 2025 | EVM (see DHL entry) | missing slippage | T32 | no minOut / deadline | `amountOutMin/deadline/minAmount` | zero minOut rejected or documented as user risk | `reviewing-mev-ordering-and-slippage` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250304-pump---not-slippage-protection |
| 20250227 | Venus_ZKSync | 2025 | zkSync | share / ERC-4626 inflation | T13/T14 | totalAssets or exchangeRate from donatable balance | `convertToShares/exchangeRate/totalAssets/donate` | 1 wei first deposit + donate; next depositor shares != 0 | `reviewing-erc4626-and-vaults` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250227-venus_zksync---donation-attack |
| 20250226 | HenloKart | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250226-henlokart---fake-native-deposit-and-immediate-cancel |
| 20250224 | INVISTECH | 2025 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250224-invistech---pair-tax-price-manipulation |
| 20250223 | HegicOptions | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250223-hegicoptions---business-logic-flaw |
| 20250222 | Unverified_35bc | 2025 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250222-unverified_35bc---reentrancy |
| 20250221 | StepHeroNFTs | 2025 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250221-stepheronfts---reentrancy-on-sell-nft |
| 20250221 | Bybit | 2025 | EVM (see DHL entry) | ops / key | T28 | operators / off-chain | n/a (ops) | process review: threshold and hardware isolation | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250221-bybit---phishing-attack |
| 20250215 | unverified_d4f1 | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250215-unverified_d4f1---access-control |
| 20250211 | Scorch | 2025 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250211-scorch---price-oracle-manipulation |
| 20250211 | LimitOrderProtocol | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250211-limitorderprotocol---access-control |
| 20250211 | FourMeme | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250211-fourmeme---logic-flaw |
| 20250208 | GMT7 | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250208-gmt7---access-control |
| 20250208 | Peapods Finance | 2025 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250208-peapods-finance---price-manipulation |
| 20250201 | GoldReserve | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250201-goldreserve---business-logic-flaw |
| 20250128 | MCAI | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250128-mcai---tax-wallet-allowance-bypass |
| 20250126 | AIXBTForcedSwap | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250126-aixbtforcedswap---hardcoded-auth-key |
| 20250123 | ODOS | 2025 | EVM (see DHL entry) | signature | T04 | ecrecover / nonce / chainId | `ecrecover/permit(/_hashTypedDataV4/nonce` | replay same sig; empty sig != address(0) success | `reviewing-signatures-permit-and-eip712` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250123-odos---invalid-signature-verification |
| 20250121 | Ast | 2025 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250121-ast---price-manipulation |
| 20250118 | Paribus | 2025 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250118-paribus---bad-oracle |
| 20250114 | IdolsNFT | 2025 | EVM (see DHL entry) | NFT / marketplace | T19 | hook or listing sig | `onERC721Received/orderHash/nonce` | cancelled listing cannot fill; hook cannot drain before state update | `reviewing-nft-and-marketplace` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250114-idolsnft---logic-flaw |
| 20250113 | Mosca2 | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250113-mosca2---logic-flaw |
| 20250112 | Unilend | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250112-unilend---logic-flaw |
| 20250111 | RoulettePotV2 | 2025 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250111-roulettepotv2---price-manipulation |
| 20250110 | JPulsepot | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250110-jpulsepot---logic-flaw |
| 20250108 | HORS | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250108-hors---access-control |
| 20250108 | LPMine | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250108-lpmine---incorrect-reward-calculation |
| 20250107 | IPC | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250107-ipc-incorrect-burn-pairs---logic-flaw |
| 20250106 | Mosca | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250106-mosca---logic-flaw |
| 20250104 | SorStaking | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250104-sorstaking---incorrect-reward-calculation |
| 20250104 | 98#Token | 2025 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250104-98token---unprotected-public-function |
| 20250101 | LAURAToken | 2025 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2025/README.md#20250101-lauratoken---pair-balance-manipulation |
| 20241227 | Bizness | 2024 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241227-bizness---reentrancy |
| 20241223 | Moonhacker | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241223-moonhacker---improper-input-validation |
| 20241218 | Slurpy | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241218-slurpycoin---logic-flaw |
| 20241216 | BTC24H | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241216-btc24h---logic-flaw |
| 20241214 | JHY | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241214-jhy---logic-flaw |
| 20241210 | LABUBUToken | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241210-labubu-token---logic-flaw |
| 20241210 | CloberDEX | 2024 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241210-cloberdex---reentrancy |
| 20241203 | Pledge | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241203-pledge---access-control |
| 20241126 | NFTG | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241126-NFTG---access-control |
| 20241124 | Proxy_b7e1 | 2024 | EVM (see DHL entry) | proxy / storage | T02/T03 | initialize / slot layout | `initialize(/upgradeTo/_authorizeUpgrade` | implementation initialize already disabled; admin slot stable | `reviewing-upgradeable-proxies` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241124-proxy_b7e1---logic-flaw |
| 20241123 | Ak1111 | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241123-ak1111---access-control |
| 20241121 | Matez | 2024 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241121-matez---integer-truncation |
| 20241120 | MainnetSettler | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241120-mainnetsettler---access-control |
| 20241119 | PolterFinance | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241119-polterfinance---flashloan-attack |
| 20241117 | MFT | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241117-mft---logic-flaw |
| 20241114 | vETH | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241114-veth---vulnerable-price-dependency |
| 20241111 | DeltaPrime | 2024 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241111-deltaprime---reentrancy |
| 20241109 | X319 | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241109-X319---access-control |
| 20241107 | ChiSale | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241107-ChiSale---logic-flaw |
| 20241107 | CoW | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241107-CoW---access-control |
| 20241107 | UniV2 | 2024 | EVM (see DHL entry) | ops / rug | T01 | owner privileges | `onlyOwner/mint(/setFee` | assert non-owner cannot mint or pull LP | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241107-UniV2---rug-pull |
| 20241105 | RPP | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241105-rpp---logic-flaw |
| 20241029 | BUBAI | 2024 | EVM (see DHL entry) | ops / rug | T01 | owner privileges | `onlyOwner/mint(/setFee` | assert non-owner cannot mint or pull LP | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241029-BUBAI---rug-pull |
| 20241026 | CompoundFork | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241026-compoundfork---flashloan-attack |
| 20241022 | Erc20transfer | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241022-erc20transfer---access-control |
| 20241022 | VISTA | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241022-vista---flashmint-receive-error |
| 20241013 | MorphoBlue | 2024 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241013-morphoblue---overpriced-asset-in-oracle |
| 20241011 | P719Token | 2024 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241011-p719token---price-manipulation-inflate-attack |
| 20241006 | HYDT | 2024 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241010-hydt---oracle-price-manipulation |
| 20241006 | SASHAToken | 2024 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241006-sashatoken---price-manipulation |
| 20241005 | AIZPTToken | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241005-AIZPTToken---wrong-price-calculation |
| 20241002 | LavaLending | 2024 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241002-LavaLending---price-manipulation |
| 20241001 | FireToken | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20241001-firetoken---pair-manipulation-with-transfer-function |
| 20240926 | OnyxDAO | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240926-OnyxDAO---fake-market |
| 20240926 | Bedrock_DeFi | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240926-Bedrock_DeFi---swap-eth/btc-1/1-in-mint-function |
| 20240924 | MARA | 2024 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240924-MARA---price-manipulation |
| 20240923 | PestoToken | 2024 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240923-PestoToken---price-manipulation |
| 20240923 | Bankroll_Network | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240923-Bankroll_Network---incorrect-input-validation |
| 20240920 | DOGGO | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240920-DOGGO---logic-flaw |
| 20240920 | Shezmu | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240920-shezmu---access-control |
| 20240918 | Unverified_766a | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240918-unverified_766a---access-control |
| 20240915 | WXETA | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240915-WXETA---Logic-Flaw |
| 20240913 | Unverified_5697 | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240913-unverified_5697---access-control |
| 20240913 | OTSeaStaking | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240913-OTSeaStaking---Logic-Flaw |
| 20240912 | Unverified_03f9 | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240912-Unverified_03f9---access-control |
| 20240911 | INUMI | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240911-INUMI---access-control |
| 20240911 | INUMI_db27 | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240911-INUMI_db27---access-control |
| 20240911 | AIRBTC | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240911-AIRBTC---access-control |
| 20240910 | Caterpillar_Coin_CUT | 2024 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240910-Caterpillar_Coin_CUT---price-manipulation |
| 20240905 | Unverified_a89f | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240905-unverified_a89f---access-control |
| 20240905 | PLN | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240905-PLN---access-control |
| 20240905 | HANAToken | 2024 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240905-HANAToken---price-manipulation |
| 20240904 | Unverified_16d0 | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240904-unverified_16d0---access-control |
| 20240903 | Penpiexyz_io | 2024 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240903-Penpiexyz_io---reentrancy-and-reward-manipulation |
| 20240902 | Pythia | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240902-pythia---logic-flaw |
| 20240828 | Unverified_667d | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240828-unverified_667d---access-control |
| 20240828 | AAVE | 2024 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240828-aave---arbitrary-call-error |
| 20240820 | COCO | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240820-coco---logic-flaw |
| 20240816 | Zenterest | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240816-Zenterest---price-out-of-date |
| 20240816 | OMPxContract | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240816-ompx-contract---flashloan |
| 20240814 | YodlRouter | 2024 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240814-yodlrouter---arbitrary-call |
| 20240813 | VOW | 2024 | EVM (see DHL entry) | ops / key | T28 | operators / off-chain | n/a (ops) | process review: threshold and hardware isolation | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240813-vow---misconfiguration |
| 20240812 | iVest | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240812-iVest---business-logic-flaw |
| 20240806 | Novax | 2024 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240806-Novax---price-manipulation |
| 20240801 | Convergence | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240801-Convergence---incorrect-input-validation |
| 20240724 | Spectra_finance | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240724-spectra_finance---incorrect-input-validation |
| 20240723 | MEVbot_0xdd7c | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240723-mevbot_0xdd7c---incorrect-input-validation |
| 20240716 | Lifiprotocol | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240716-lifiprotocol---incorrect-input-validation |
| 20240714 | Minterest | 2024 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240714-minterest---Reentrancy |
| 20240712 | DoughFina | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240712-doughfina---incorrect-input-validation |
| 20240711 | SBT | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240711-sbt---business-logic-flaw |
| 20240711 | GAX | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240711-GAX---lack-of-access-control |
| 20240708 | LW | 2024 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240708-Lw---integer-underflow |
| 20240705 | DeFiPlaza | 2024 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240705-defiplaza---loss-of-precision |
| 20240703 | UnverifiedContr_0x452E25 | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240703-UnverifiedContr_0x452E25---lack-of-access-control |
| 20240702 | MRP | 2024 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240702-mrp---reentrancy |
| 20240628 | Will | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240628-Will---business-logic-flaw |
| 20240627 | APEMAGA | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240627-APEMAGA---business-logic-flaw |
| 20240618 | INcufi | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240618-incufi---business-logic-flaw |
| 20240617 | Dyson_money | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240617-dyson_money---business-logic-flaw |
| 20240616 | WIFCOIN_ETH | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240616-WIFCOIN_ETH---business-logic-flaw |
| 20240611 | Crb2 | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240616-Crb2---business-logic-flaw |
| 20240611 | JokInTheBox | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240611-JokInTheBox---business-logic-flaw |
| 20240610 | UwuLend - Price Manipulation | 2024 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240610-UwuLend---Price-Manipulation |
| 20240610 | Bazaar | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240610-bazaar---insufficient-permission-check |
| 20240608 | YYStoken | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240608-YYStoken---business-logic-flaw |
| 20240606 | SteamSwap | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240606-steamswap---logic-flaw |
| 20240606 | MineSTM | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240606-MineSTM---business-logic-flaw |
| 20240604 | NCD | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240604-NCD---business-logic-flaw |
| 20240601 | VeloCore | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240601-VeloCore---lack-of-access-control |
| 20240531 | Liquiditytokens | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240531-liquiditytokens---business-logic-flaw |
| 20240531 | MixedSwapRouter | 2024 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240531-MixedSwapRouter---arbitrary-call |
| 20240529 | SCROLL | 2024 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240529-SCROLL---integer-underflow |
| 20240529 | MetaDragon | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240529-metadragon---lack-of-access-control |
| 20240528 | Tradeonorion | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240528-Tradeonorion---business-logic-flaw |
| 20240528 | EXcommunity | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240528-EXcommunity---business-logic-flaw |
| 20240527 | RedKeysCoin | 2024 | EVM (see DHL entry) | weak randomness | T50 | block variables as seed | `blockhash/timestamp/random` | same seed is deterministic in test (must not pay jackpot) | `reviewing-mev-ordering-and-slippage` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240527-redkeyscoin---weak-rng |
| 20240526 | NORMIE | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240526-normie---business-logic-flaw |
| 20240522 | Burner | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240522-Burner---sandwich-ack |
| 20240516 | TCH | 2024 | EVM (see DHL entry) | signature | T04 | ecrecover / nonce / chainId | `ecrecover/permit(/_hashTypedDataV4/nonce` | replay same sig; empty sig != address(0) success | `reviewing-signatures-permit-and-eip712` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240516-tch---signature-malleability-vulnerability |
| 20240514 | Sonne Finance | 2024 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240514-sonne-finance---precision-loss |
| 20240514 | PredyFinance | 2024 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240514-predyfinance---reentrancy |
| 20240512 | TGC | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240512-tgc---business-logic-flaw |
| 20240510 | GFOX | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240510-gfox---lack-of-access-control |
| 20240510 | TSURU | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240510-tsuru---insufficient-validation |
| 20240508 | GPU | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240508-GPU---self-transfer |
| 20240507 | SATURN | 2024 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240507-saturn---price-manipulation |
| 20240506 | OSN | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240506-osn---reward-distribution-problem |
| 20240430 | Yield | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240430-yield---business-logic-flaw |
| 20240430 | PikeFinance | 2024 | EVM (see DHL entry) | proxy / storage | T02/T03 | initialize / slot layout | `initialize(/upgradeTo/_authorizeUpgrade` | implementation initialize already disabled; admin slot stable | `reviewing-upgradeable-proxies` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240430-pikefinance---uninitialized-proxy |
| 20240427 | BNBX | 2024 | BSC | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240427-BNBX---precision-loss |
| 20240425 | NGFS | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240425-ngfs---bad-access-control |
| 20240424 | XBridge | 2024 | EVM (see DHL entry) | bridge / message | T26 | payload origin / nonce | `processMessage/lzReceive/relay/origin` | replay message id reverts; wrong origin reverts | `reviewing-bridges-and-messaging` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240424-xbridge---logic-flaw |
| 20240424 | YIEDL | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240424-yiedl---input-validation |
| 20240422 | Z123 | 2024 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240422-z123---price-manipulation |
| 20240420 | Rico | 2024 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240420-rico---arbitrary-call |
| 20240419 | HedgeyFinance | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240419-hedgeyfinance---logic-flaw |
| 20240417 | UnverifiedContr_0x00C409 | 2024 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240417-UnverifiedContr_0x00C409---unverified-external-call |
| 20240416 | SATX | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240416-satx---logic-flaw |
| 20240416 | MARS_DEFI | 2024 | EVM (see DHL entry) | weird ERC-20 | T22 | pair balance vs reserves | `skim/sync/fee/rebase/burn` | FoT / reflection path: recipient gets expected minOut | `reviewing-token-standard-pitfalls` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240416-mars---bad-reflection |
| 20240415 | GFA | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240415-gfa---business-logic-flaw |
| 20240415 | ChaingeFinance | 2024 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240415-chaingeFinance---arbitrary-external-call |
| 20240414 | Hackathon | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240414-hackathon---business-logic-flaw |
| 20240412 | FIL314 | 2024 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240412-fil314---insufficient-validation-and-price-manipulation |
| 20240412 | SumerMoney | 2024 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240412-sumermoney---Reentrancy |
| 20240412 | GROKD | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240412-grokd---lack-of-access-control |
| 20240410 | BigBangSwap | 2024 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240410-BigBangSwap---precision-loss |
| 20240409 | UPS | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240409-ups---business-logic-flaw |
| 20240408 | SQUID | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240408-squid---sandwich-attack |
| 20240404 | WSM | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240404-wsm---manipulating-price |
| 20240402 | HoppyFrogERC | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240402-hoppyfrogerc---business-logic-flaw |
| 20240401 | ATM | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240401-atm---business-logic-flaw |
| 20240401 | OpenLeverage | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240401-openleverage---business-logic-flaw |
| 20240329 | ETHFIN | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240329-ethfin---lack-of-access-control |
| 20240329 | PrismaFi | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240329-prismaFi---insufficient-validation |
| 20240328 | LavaLending | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240328-lavalending---business-logic-flaw |
| 20240325 | ZongZi | 2024 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240325-zongzi---price-manipulation |
| 20240324 | ARK | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240324-ark---business-logic-flaw |
| 20240323 | CGT | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240323-cgt---incorrect-access-control |
| 20240321 | SSS | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240321-sss---token-balance-doubles-on-transfer-to-self |
| 20240320 | Paraswap | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240320-paraswap---incorrect-access-control |
| 20240314 | MO | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240314-mo---business-logic-flaw |
| 20240313 | IT | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240313-it---business-logic-flaw |
| 20240312 | BBT | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240312-bbt---business-logic-flaw |
| 20240311 | Binemon | 2024 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240311-Binemon---precission-loss |
| 20240309 | Juice | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240309-juice---business-logic-flaw |
| 20240309 | UnizenIO | 2024 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240309-unizenio---unverified-external-call |
| 20240307 | GHT | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240307-ght---business-logic-flaw |
| 20240306 | ALP | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240306-alp---public-internal-function |
| 20240306 | TGBS | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240306-tgbs---business-logic-flaw |
| 20240305 | Woofi | 2024 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240305-woofi---price-manipulation |
| 20240228 | Seneca | 2024 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240228-seneca---arbitrary-external-call-vulnerability |
| 20240228 | SMOOFSStaking | 2024 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240228-smoofsstaking---reentrancy |
| 20240223 | Zoomer | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240223-zoomer---business-logic-flaw |
| 20240223 | CompoundUni | 2024 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240223-CompoundUni---Oracle-bad-price |
| 20240223 | BlueberryProtocol | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240223-BlueberryProtocol---logic-flaw |
| 20240222 | SwarmMarkets | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240222-SwarmMarkets---lack-of-validation |
| 20240221 | DeezNutz404 | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240221-deeznutz-404---lack-of-validation |
| 20240221 | GAIN | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240221-GAIN---bad-function-implementation |
| 20240220 | EGGX | 2024 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240220-EGGX---reentrancy |
| 20240219 | RuggedArt | 2024 | EVM (see DHL entry) | ops / rug | T01 | owner privileges | `onlyOwner/mint(/setFee` | assert non-owner cannot mint or pull LP | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240219-RuggedArt---reentrancy |
| 20240216 | ParticleTrade | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240216-ParticleTrade---lack-of-validation-data |
| 20240215 | DualPools | 2024 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240215-DualPools---precision-truncation |
| 20240215 | Babyloogn | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240215-Babyloogn---lack-of-validation |
| 20240215 | Miner | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240215-Miner---lack-of-validation-dst-address |
| 20240213 | MINER BSC | 2024 | BSC | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240213-miner---price-manipulation |
| 20240211 | Game | 2024 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240211-game---reentrancy--business-logic-flaw |
| 20240210 | FILX DN404 | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240210-filx-dn404---access-control |
| 20240208 | Pandora404 | 2024 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240208-pandora---interger-underflow |
| 20240205 | BurnsDefi | 2024 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240205-burnsdefi---price-manipulation |
| 20240202 | ADC | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240202-adc---incorrect-access-control |
| 20240201 | AffineDeFi | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240201-affinedefi---lack-of-validation-userData |
| 20240130 | XSIJ | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240130-xsij---business-logic-flaw |
| 20240130 | MIMSpell | 2024 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240130-mimspell---precission-loss |
| 20240129 | PeapodsFinance | 2024 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240128-PeapodsFinance---reentrancy |
| 20240128 | BarleyFinance | 2024 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240128-barleyfinance---reentrancy |
| 20240127 | CitadelFinance | 2024 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240127-citadelfinance---price-manipulation |
| 20240125 | NBLGAME | 2024 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240125-nblgame---reentrancy |
| 20240122 | DAO_SoulMate | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240122-dao_soulmate---incorrect-access-control |
| 20240117 | BmiZapper | 2024 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240117-bmizapper---arbitrary-external-call-vulnerability |
| 20240117 | SocketGateway | 2024 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240112-socketgateway---lack-of-calldata-validation |
| 20240115 | Shell_MEV_0xa898 | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240115-Shell_MEV_0xa898---lack-of-access-control |
| 20240112 | WiseLending | 2024 | EVM (see DHL entry) | liquidation / insolvency | T24/T25 | donate/repay skipping liquidity | `liquidate/donate/healthFactor` | after donate-to-zero, liquidate cannot mint profit | `reviewing-lending-and-liquidations` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240112-wiselending---bad-healthfactor-check |
| 20240110 | Freedom | 2024 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240110-Freedom---lack-of-access-control |
| 20240110 | LQDX Alert | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240110-lqdx---unauthorized-transferfrom |
| 20240104 | Gamma | 2024 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240104-gamma---price-manipulation |
| 20240102 | MIC | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240102-mic---business-logic-flaw |
| 20240102 | RadiantCapital | 2024 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240102-radiantcapital---loss-of-precision |
| 20240101 | OrbitChain | 2024 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2024/README.md#20240101-orbitchain---incorrect-input-validation |
| 20231231 | Channels BUSD&USDC | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231231-channels---price-manipulation |
| 20231230 | ChannelsFinance | 2023 | EVM (see DHL entry) | share / ERC-4626 inflation | T13/T14 | totalAssets or exchangeRate from donatable balance | `convertToShares/exchangeRate/totalAssets/donate` | 1 wei first deposit + donate; next depositor shares != 0 | `reviewing-erc4626-and-vaults` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231230-channelsfinance---compoundv2-inflation-attack |
| 20231228 | CCV | 2023 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231225-CCV---precision-loss |
| 20231228 | DominoTT | 2023 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231228-dominott---precision-loss |
| 20231225 | Telcoin | 2023 | EVM (see DHL entry) | proxy / storage | T02/T03 | initialize / slot layout | `initialize(/upgradeTo/_authorizeUpgrade` | implementation initialize already disabled; admin slot stable | `reviewing-upgradeable-proxies` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231225-telcoin---storage-collision |
| 20231222 | PineProtocol | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231222-pineprotocol---business-logic-flaw |
| 20231220 | TransitFinance | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231220-transitfinance---lack-of-validation-pool |
| 20231217 | Bob | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231217-bob---price-manipulation |
| 20231217 | FloorProtocol | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231217-floorprotocol---business-logic-flaw |
| 20231216 | GoodDollar | 2023 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231216-gooddollar---lack-of-input-validation--reentrancy |
| 20231216 | KEST | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231216-kest---business-logic-flaw |
| 20231216 | NFTTrader | 2023 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231216-nfttrader---reentrancy |
| 20231214 | PHIL | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231214-PHIL---business-logic-flaw |
| 20231213 | HYPR | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231213-hypr---business-logic-flaw |
| 20231211 | GoodCompound | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231211-goodcompound---price-manipulation |
| 20231209 | BCT | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231209-bct---price-manipulation |
| 20231207 | HNet | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231207-HNet---business-logic-flaw |
| 20231206 | TIME | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231206-time---arbitrary-address-spoofing-attack |
| 20231206 | ElephantStatus | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231206-elephantstatus---price-manipulation |
| 20231205 | MAMO | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231205-mamo---price-manipulation |
| 20231205 | BEARNDAO | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231205-bearndao---business-logic-flaw |
| 20231202 | bZxProtocol | 2023 | EVM (see DHL entry) | share / ERC-4626 inflation | T13/T14 | totalAssets or exchangeRate from donatable balance | `convertToShares/exchangeRate/totalAssets/donate` | 1 wei first deposit + donate; next depositor shares != 0 | `reviewing-erc4626-and-vaults` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231202-bzxprotocol---inflation-attack |
| 20231201 | UnverifiedContr_0x431abb | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231201-unverifiedcontr_0x431abb---business-logic-flaw |
| 20231130 | EEE | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231130-eee---price-manipulation |
| 20231130 | CAROLProtocol | 2023 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231130-carolprotocol---price-manipulation-via-reentrancy |
| 20231129 | Burntbubba | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231129-burntbubba---price-manipulation |
| 20231129 | AIS | 2023 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231129-ais---access-control |
| 20231128 | FiberRouter | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231128-FiberRouter---input-validation |
| 20231125 | MetaLend | 2023 | EVM (see DHL entry) | share / ERC-4626 inflation | T13/T14 | totalAssets or exchangeRate from donatable balance | `convertToShares/exchangeRate/totalAssets/donate` | 1 wei first deposit + donate; next depositor shares != 0 | `reviewing-erc4626-and-vaults` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231125-metalend---compoundv2-inflation-attack |
| 20231125 | TheNFTV2 | 2023 | EVM (see DHL entry) | NFT / marketplace | T19 | hook or listing sig | `onERC721Received/orderHash/nonce` | cancelled listing cannot fill; hook cannot drain before state update | `reviewing-nft-and-marketplace` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231125-thenftv2---logic-flaw |
| 20231122 | KyberSwap | 2023 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231122-kyberswap---precision-loss |
| 20231117 | Token8633_9419 | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231117-token8633_9419---price-manipulation |
| 20231117 | ShibaToken | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231117-shibatoken---business-logic-flaw |
| 20231116 | WECO | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231116-weco---business-logic-flaw |
| 20231115 | EHX | 2023 | EVM (see DHL entry) | missing slippage | T32 | no minOut / deadline | `amountOutMin/deadline/minAmount` | zero minOut rejected or documented as user risk | `reviewing-mev-ordering-and-slippage` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231115-ehx---lack-of-slippage-control |
| 20231115 | XAI | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231115-xai---business-logic-flaw |
| 20231115 | LinkDAO | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231115-linkdao---bad-k-value-verification |
| 20231114 | OKC Project | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231114-OKC-Project---Instant-Rewards-Unlocked |
| 20231112 | MEV_0x8c2d | 2023 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231112-mevbot_0x8c2d---lack-of-access-control |
| 20231112 | MEV_0xa247 | 2023 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231112-mevbot_0xa247---incorrect-access-control |
| 20231111 | Mahalend | 2023 | EVM (see DHL entry) | share / ERC-4626 inflation | T13/T14 | totalAssets or exchangeRate from donatable balance | `convertToShares/exchangeRate/totalAssets/donate` | 1 wei first deposit + donate; next depositor shares != 0 | `reviewing-erc4626-and-vaults` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231111-mahalend---donate-inflation-exchangerate--rounding-error |
| 20231110 | Raft_fi | 2023 | EVM (see DHL entry) | share / ERC-4626 inflation | T13/T14 | totalAssets or exchangeRate from donatable balance | `convertToShares/exchangeRate/totalAssets/donate` | 1 wei first deposit + donate; next depositor shares != 0 | `reviewing-erc4626-and-vaults` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231110-raft_fi---donate-inflation-exchangerate--rounding-error |
| 20231110 | GrokToken | 2023 | EVM (see DHL entry) | missing slippage | T32 | no minOut / deadline | `amountOutMin/deadline/minAmount` | zero minOut rejected or documented as user risk | `reviewing-mev-ordering-and-slippage` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231110-grok---lack-of-slippage-protection |
| 20231107 | RBalancer | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231107-rbalancer---business-logic-flaw |
| 20231107 | MEVbot | 2023 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231107-mevbot---lack-of-access-control |
| 20231106 | TrustPad | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231106-trustpad---lack-of-msgsender-address-verification |
| 20231106 | TheStandard_io | 2023 | EVM (see DHL entry) | missing slippage | T32 | no minOut / deadline | `amountOutMin/deadline/minAmount` | zero minOut rejected or documented as user risk | `reviewing-mev-ordering-and-slippage` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231106-thestandard_io---lack-of-slippage-protection |
| 20231106 | KR | 2023 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231106-KR---precission-loss |
| 20231102 | BRAND | 2023 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231102-brand---lack-of-access-control |
| 20231102 | 3913Token | 2023 | EVM (see DHL entry) | weird ERC-20 | T22 | pair balance vs reserves | `skim/sync/fee/rebase/burn` | FoT / reflection path: recipient gets expected minOut | `reviewing-token-standard-pitfalls` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231102-3913token---deflationary-token-attack |
| 20231101 | SwampFinance | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231101-swampfinance---business-logic-flaw |
| 20231101 | OnyxProtocol | 2023 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231101-onyxprotocol---precission-loss-vulnerability |
| 20231031 | UniBotRouter | 2023 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231031-UniBotRouter---arbitrary-external-call |
| 20231030 | LaEeb | 2023 | EVM (see DHL entry) | missing slippage | T32 | no minOut / deadline | `amountOutMin/deadline/minAmount` | zero minOut rejected or documented as user risk | `reviewing-mev-ordering-and-slippage` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231030-laeeb---lack-slippage-protection |
| 20231028 | AstridProtocol | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231028-AstridProtocol---business-logic-flaw |
| 20231024 | MaestroRouter2 | 2023 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231024-maestrorouter2---arbitrary-external-call |
| 20231022 | OpenLeverage | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231022-openleverage---business-logic-flaw |
| 20231019 | kTAF | 2023 | EVM (see DHL entry) | share / ERC-4626 inflation | T13/T14 | totalAssets or exchangeRate from donatable balance | `convertToShares/exchangeRate/totalAssets/donate` | 1 wei first deposit + donate; next depositor shares != 0 | `reviewing-erc4626-and-vaults` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231019-ktaf---compoundv2-inflation-attack |
| 20231018 | HopeLend | 2023 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231018-hopelend---div-precision-loss |
| 20231018 | MicDao | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231018-micdao---price-manipulation |
| 20231013 | BelugaDex | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231013-belugadex---price-manipulation |
| 20231013 | WiseLending | 2023 | EVM (see DHL entry) | share / ERC-4626 inflation | T13/T14 | totalAssets or exchangeRate from donatable balance | `convertToShares/exchangeRate/totalAssets/donate` | 1 wei first deposit + donate; next depositor shares != 0 | `reviewing-erc4626-and-vaults` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231013-wiselending---donate-inflation-exchangerate--rounding-error |
| 20231012 | Platypus | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231012-platypus---business-logic-flaw |
| 20231011 | BH | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231011-bh---price-manipulation |
| 20231008 | ZS | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231008-zs---business-logic-flaw |
| 20231008 | pSeudoEth | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231008-pseudoeth---pool-manipulation |
| 20231007 | StarsArena | 2023 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231007-starsarena---reentrancy |
| 20231005 | DePayRouter | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20231005-depayrouter---business-logic-flaw |
| 20230930 | FireBirdPair | 2023 | EVM (see DHL entry) | missing slippage | T32 | no minOut / deadline | `amountOutMin/deadline/minAmount` | zero minOut rejected or documented as user risk | `reviewing-mev-ordering-and-slippage` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230930-FireBirdPair---lack-slippage-protection |
| 20230929 | DEXRouter | 2023 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230929-dexrouter---arbitrary-external-call |
| 20230926 | XSDWETHpool | 2023 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230926-XSDWETHpool---reentrancy |
| 20230924 | KubSplit | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230924-kubsplit---pool-manipulation |
| 20230921 | CEXISWAP | 2023 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230921-cexiswap---incorrect-access-control |
| 20230916 | uniclyNFT | 2023 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230916-uniclynft---reentrancy |
| 20230911 | 0x0DEX | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230911-0x0dex---parameter-manipulation |
| 20230909 | BFCToken | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230909-bfctoken---business-logic-flaw |
| 20230908 | APIG | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230908-apig---business-logic-flaw |
| 20230907 | HCT | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230907-hct---price-manipulation |
| 20230905 | QuantumWN | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230905-quantumwn---rebasing-logic-issue |
| 20230905 | JumpFarm | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230905-JumpFarm---rebasing-logic-issue |
| 20230905 | HeavensGate | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230905-HeavensGate---rebasing-logic-issue |
| 20230905 | FloorDAO | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230905-floordao---rebasing-logic-issue |
| 20230902 | DAppSocial | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230902-dappsocial---business-logic-flaw |
| 20230829 | EAC | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230829-eac---price-manipulation |
| 20230827 | Balancer | 2023 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230827-balancer---rounding-error--business-logic-flaw |
| 20230826 | SVT | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230826-svt---flawed-price-calculation |
| 20230824 | GSS | 2023 | EVM (see DHL entry) | weird ERC-20 | T22 | pair balance vs reserves | `skim/sync/fee/rebase/burn` | FoT / reflection path: recipient gets expected minOut | `reviewing-token-standard-pitfalls` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230824-gss---skim-token-balance |
| 20230821 | EHIVE | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230821-ehive---business-logic-flaw |
| 20230819 | BTC20 | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230819-btc20---price-manipulation |
| 20230818 | ExactlyProtocol | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230818-exactlyprotocol---insufficient-validation |
| 20230814 | ZunamiProtocol | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230814-zunamiprotocol---price-manipulation |
| 20230809 | EarningFram | 2023 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230809-earningfram---reentrancy |
| 20230802 | CurveBurner | 2023 | EVM (see DHL entry) | missing slippage | T32 | no minOut / deadline | `amountOutMin/deadline/minAmount` | zero minOut rejected or documented as user risk | `reviewing-mev-ordering-and-slippage` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230802-curveburner---lack-slippage-protection |
| 20230802 | Uwerx | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230802-uwerx---fault-logic |
| 20230801 | NeutraFinance | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230801-neutrafinance---price-manipulation |
| 20230801 | LeetSwap | 2023 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230801-leetswap---access-control |
| 20230731 | GYMNET | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230731-gymnet---insufficient-validation |
| 20230730 | Curve | 2023 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230730-curve---vyper-compiler-bug--reentrancy |
| 20230726 | Carson | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230726-carson---price-manipulation |
| 20230724 | Palmswap | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230724-palmswap---business-logic-flaw |
| 20230723 | MintoFinance | 2023 | EVM (see DHL entry) | signature | T04 | ecrecover / nonce / chainId | `ecrecover/permit(/_hashTypedDataV4/nonce` | replay same sig; empty sig != address(0) success | `reviewing-signatures-permit-and-eip712` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230723-mintofinance---signature-replay |
| 20230722 | ConicFinance02 | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230722-conic-finance-02---price-manipulation |
| 20230721 | ConicFinance | 2023 | EVM (see DHL entry) | read-only reentrancy | T18 | view price during callback | `get_virtual_price/totalAssets/manageUserBalance` | view price during callback must not be used as collateral | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230721-conic-finance---read-only-reentrancy--misconfiguration |
| 20230721 | SUT | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230721-sut---business-logic-flaw |
| 20230720 | Utopia | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230720-utopia---business-logic-flaw |
| 20230720 | FFIST | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230720-ffist---business-logic-flaw |
| 20230718 | APEDAO | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230718-apedao---business-logic-flaw |
| 20230718 | BNO | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230718-bno---invalid-emergency-withdraw-mechanism |
| 20230717 | NewFi | 2023 | EVM (see DHL entry) | missing slippage | T32 | no minOut / deadline | `amountOutMin/deadline/minAmount` | zero minOut rejected or documented as user risk | `reviewing-mev-ordering-and-slippage` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230717-newfi---lack-slippage-protection |
| 20230715 | USDTStakingContract28 | 2023 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230715-usdtstakingcontract28---lack-of-access-control |
| 20230712 | Platypus | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230712-platypus---bussiness-logic-flaw |
| 20230712 | WGPT | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230712-wgpt---business-logic-flaw |
| 20230711 | RodeoFinance | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230711-rodeofinance---twap-oracle-manipulation |
| 20230711 | Libertify | 2023 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230711-libertify---reentrancy |
| 20230710 | ArcadiaFi | 2023 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230710-arcadiafi---reentrancy |
| 20230708 | CIVNFT | 2023 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230708-civnft---lack-of-access-control |
| 20230708 | Civfund | 2023 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230708-civfund---lack-of-access-control |
| 20230707 | LUSD | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230707-LUSD---price-manipulation-attack |
| 20230704 | BambooIA | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230704-bambooia---price-manipulation-attack |
| 20230704 | BaoCommunity | 2023 | EVM (see DHL entry) | share / ERC-4626 inflation | T13/T14 | totalAssets or exchangeRate from donatable balance | `convertToShares/exchangeRate/totalAssets/donate` | 1 wei first deposit + donate; next depositor shares != 0 | `reviewing-erc4626-and-vaults` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230704-baocommunity---donate-inflation-exchangerate--rounding-error |
| 20230703 | AzukiDAO | 2023 | EVM (see DHL entry) | signature | T04 | ecrecover / nonce / chainId | `ecrecover/permit(/_hashTypedDataV4/nonce` | replay same sig; empty sig != address(0) success | `reviewing-signatures-permit-and-eip712` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230703-azukidao---invalid-signature-verification |
| 20230630 | Biswap | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230630-biswap---v3migrator-exploit |
| 20230630 | MyAi | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230630-MyAi---business-loigc |
| 20230628 | Themis | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230628-themis---manipulation-of-prices-using-flashloan |
| 20230627 | UnverifiedContr_9ad32 | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230627-unverifiedcontr_9ad32---business-loigc-flaw |
| 20230627 | STRAC | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230627-STRAC---business-loigc |
| 20230623 | SHIDO | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230623-shido---business-loigc |
| 20230621 | BabyDogeCoin02 | 2023 | EVM (see DHL entry) | missing slippage | T32 | no minOut / deadline | `amountOutMin/deadline/minAmount` | zero minOut rejected or documented as user risk | `reviewing-mev-ordering-and-slippage` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230621-babydogecoin02---lack-slippage-protection |
| 20230621 | BUNN | 2023 | EVM (see DHL entry) | weird ERC-20 | T22 | pair balance vs reserves | `skim/sync/fee/rebase/burn` | FoT / reflection path: recipient gets expected minOut | `reviewing-token-standard-pitfalls` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230621-bunn---reflection-tokens |
| 20230620 | MIM | 2023 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230620-mimspell---arbitrary-external-call-vulnerability |
| 20230619 | Contract_0x7657 | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230620-Contract_0x7657---business-loigc |
| 20230618 | ARA | 2023 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230618-ara---incorrect-handling-of-permissions |
| 20230617 | MidasCapitalXYZ | 2023 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230617-midascapitalxyz---precision-loss |
| 20230617 | Pawnfi | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230617-pawnfi---business-logic-flaw |
| 20230615 | CFC | 2023 | EVM (see DHL entry) | weird ERC-20 | T22 | pair balance vs reserves | `skim/sync/fee/rebase/burn` | FoT / reflection path: recipient gets expected minOut | `reviewing-token-standard-pitfalls` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230615-cfc---uniswap-skim-token-balance-attack |
| 20230615 | DEPUSDT_LEVUSDC | 2023 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230615-depusdt_levusdc---incorrect-access-control |
| 20230612 | Sturdy Finance | 2023 | EVM (see DHL entry) | read-only reentrancy | T18 | view price during callback | `get_virtual_price/totalAssets/manageUserBalance` | view price during callback must not be used as collateral | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230612-sturdy-finance---read-only-reentrancy |
| 20230611 | SellToken04 | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230611-sellToken04---Price-Manipulation |
| 20230607 | CompounderFinance | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230607-compounderfinance---manipulation-of-funds-through-fluctuations-in-the-amount-of-exchangeable-assets |
| 20230606 | VINU | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230606-vinu---price-manipulation |
| 20230606 | UN | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230606-un---price-manipulation |
| 20230602 | NST SimpleSwap | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230602-nst-simple-swap---unverified-contract-wrong-approval |
| 20230601 | DDCoin | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230601-ddcoin---flashloan-attack-and-smart-contract-vulnerability |
| 20230601 | Cellframenet | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230601-cellframenet---calculation-issues-during-liquidity-migration |
| 20230531 | ERC20TokenBank | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230531-erc20tokenbank---price-manipulation |
| 20230529 | Jimbo | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230529-jimbo---protocol-specific-price-manipulation |
| 20230529 | BabyDogeCoin | 2023 | EVM (see DHL entry) | missing slippage | T32 | no minOut / deadline | `amountOutMin/deadline/minAmount` | zero minOut rejected or documented as user risk | `reviewing-mev-ordering-and-slippage` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230529-babydogecoin---lack-slippage-protection |
| 20230529 | FAPEN | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230529-fapen---wrong-balance-check |
| 20230529 | NOON_NO | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230529-noon-no---wrong-visibility-in-function |
| 20230525 | GPT | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230525-gpt-token---fee-machenism-exploitation |
| 20230524 | LocalTrade | 2023 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230524-local-trade-lct---improper-access-control-of-close-source-contract |
| 20230524 | CS | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230524-cs-token---outdated-global-variable |
| 20230523 | LFI | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230523-lfi-token---business-logic-flaw |
| 20230514 | landNFT | 2023 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230514-landNFT---lack-of-permission-control |
| 20230514 | SellToken03 | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230514-selltoken03---unchecked-user-input |
| 20230513 | Bitpaidio | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230513-bitpaidio---business-logic-flaw |
| 20230513 | SellToken02 | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230513-selltoken02---price-manipulation |
| 20230512 | LW | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230512-lw---flashloan-price-manipulation |
| 20230511 | SellToken01 | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230511-selltoken01---business-logic-flaw |
| 20230510 | SNK | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230510-snk---reward-calculation-error |
| 20230509 | MCC | 2023 | EVM (see DHL entry) | weird ERC-20 | T22 | pair balance vs reserves | `skim/sync/fee/rebase/burn` | FoT / reflection path: recipient gets expected minOut | `reviewing-token-standard-pitfalls` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230509-mcc---reflection-token |
| 20230509 | HODL | 2023 | EVM (see DHL entry) | weird ERC-20 | T22 | pair balance vs reserves | `skim/sync/fee/rebase/burn` | FoT / reflection path: recipient gets expected minOut | `reviewing-token-standard-pitfalls` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230509-hodl---reflection-token |
| 20230506 | Melo | 2023 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230506-melo---access-control |
| 20230505 | DEI | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230505-dei---wrong-implemention |
| 20230503 | NeverFall | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230503-NeverFall---price-manipulation |
| 20230502 | Level | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230502-level---business-logic-flaw |
| 20230428 | 0vix | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230428-0vix---flashloan-price-manipulation |
| 20230427 | SiloFinance | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230427-Silo-finance---Business-Logic-Flaw |
| 20230424 | Axioma | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230424-Axioma---business-logic-flaw |
| 20230419 | OLIFE | 2023 | EVM (see DHL entry) | weird ERC-20 | T22 | pair balance vs reserves | `skim/sync/fee/rebase/burn` | FoT / reflection path: recipient gets expected minOut | `reviewing-token-standard-pitfalls` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230419-OLIFE---Reflection-token |
| 20230416 | Swapos V2 | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230416-swapos-v2---error-k-value-attack |
| 20230415 | HundredFinance | 2023 | EVM (see DHL entry) | share / ERC-4626 inflation | T13/T14 | totalAssets or exchangeRate from donatable balance | `convertToShares/exchangeRate/totalAssets/donate` | 1 wei first deposit + donate; next depositor shares != 0 | `reviewing-erc4626-and-vaults` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230415-hundredfinance---donate-inflation-exchangerate--rounding-error |
| 20230413 | yearnFinance | 2023 | EVM (see DHL entry) | ops / key | T28 | operators / off-chain | n/a (ops) | process review: threshold and hardware isolation | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230413-yearnFinance---misconfiguration |
| 20230412 | MetaPoint | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230412-metapoint---Unrestricted-Approval |
| 20230411 | Paribus | 2023 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230411-paribus---reentrancy |
| 20230409 | SushiSwap | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230409-SushiSwap---Unchecked-User-Input |
| 20230405 | Sentiment | 2023 | EVM (see DHL entry) | read-only reentrancy | T18 | view price during callback | `get_virtual_price/totalAssets/manageUserBalance` | view price during callback must not be used as collateral | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230405-sentiment---read-only-reentrancy |
| 20230402 | Allbridge | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230402-allbridge---flashloan-price-manipulation |
| 20230328 | SafeMoon Hack | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230328-safemoon-hack |
| 20230328 | THENA | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230328---thena---yield-protocol-flaw |
| 20230325 | DBW | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230325---dbw--business-logic-flaw |
| 20230322 | BIGFI | 2023 | EVM (see DHL entry) | weird ERC-20 | T22 | pair balance vs reserves | `skim/sync/fee/rebase/burn` | FoT / reflection path: recipient gets expected minOut | `reviewing-token-standard-pitfalls` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230322---bigfi---reflection-token |
| 20230317 | ParaSpace NFT | 2023 | EVM (see DHL entry) | NFT / marketplace | T19 | hook or listing sig | `onERC721Received/orderHash/nonce` | cancelled listing cannot fill; hook cannot drain before state update | `reviewing-nft-and-marketplace` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230317---paraspace-nft---flashloan--scaledbalanceof-manipulation |
| 20230315 | Poolz | 2023 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230315---poolz---integer-overflow |
| 20230313 | EulerFinance | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230313---eulerfinance---business-logic-flaw |
| 20230308 | DKP | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230308---dkp---flashloan-price-manipulation |
| 20230307 | Phoenix | 2023 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230307---phoenix---access-control--arbitrary-external-call |
| 20230227 | LaunchZone | 2023 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230227---launchzone---access-control |
| 20230227 | SwapX | 2023 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230227---swapx---access-control |
| 20230224 | EFVault | 2023 | EVM (see DHL entry) | proxy / storage | T02/T03 | initialize / slot layout | `initialize(/upgradeTo/_authorizeUpgrade` | implementation initialize already disabled; admin slot stable | `reviewing-upgradeable-proxies` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230224---efvault---storage-collision |
| 20230222 | DYNA | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230222---dyna---business-logic-flaw |
| 20230218 | RevertFinance | 2023 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230218---revertfinance---arbitrary-external-call-vulnerability |
| 20230217 | Starlink | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230217---starlink---business-logic-flaw |
| 20230217 | Dexible | 2023 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230217---dexible---arbitrary-external-call-vulnerability |
| 20230217 | Platypusdefi | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230217---platypusdefi---business-logic-flaw |
| 20230210 | Sheep Token | 2023 | EVM (see DHL entry) | weird ERC-20 | T22 | pair balance vs reserves | `skim/sync/fee/rebase/burn` | FoT / reflection path: recipient gets expected minOut | `reviewing-token-standard-pitfalls` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230210---sheep---reflection-token |
| 20230210 | dForce | 2023 | EVM (see DHL entry) | read-only reentrancy | T18 | view price during callback | `get_virtual_price/totalAssets/manageUserBalance` | view price during callback must not be used as collateral | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230210---dforce---read-only-reentrancy |
| 20230207 | CowSwap | 2023 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230207---cowswap---arbitrary-external-call-vulnerability |
| 20230206 | FDP Token | 2023 | EVM (see DHL entry) | weird ERC-20 | T22 | pair balance vs reserves | `skim/sync/fee/rebase/burn` | FoT / reflection path: recipient gets expected minOut | `reviewing-token-standard-pitfalls` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230206---fdp---reflection-token |
| 20230203 | Orion Protocol | 2023 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230203---orion-protocol---reentrancy |
| 20230203 | Spherax USDs | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230203---spherax-usds---balance-recalculation-bug |
| 20230202 | BonqDAO | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230202---BonqDAO---price-oracle-manipulation |
| 20230130 | BEVO | 2023 | EVM (see DHL entry) | weird ERC-20 | T22 | pair balance vs reserves | `skim/sync/fee/rebase/burn` | FoT / reflection path: recipient gets expected minOut | `reviewing-token-standard-pitfalls` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230130---bevo---reflection-token |
| 20230126 | TomInu Token | 2023 | EVM (see DHL entry) | weird ERC-20 | T22 | pair balance vs reserves | `skim/sync/fee/rebase/burn` | FoT / reflection path: recipient gets expected minOut | `reviewing-token-standard-pitfalls` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230126---tinu---reflection-token |
| 20230119 | SHOCO Token | 2023 | EVM (see DHL entry) | weird ERC-20 | T22 | pair balance vs reserves | `skim/sync/fee/rebase/burn` | FoT / reflection path: recipient gets expected minOut | `reviewing-token-standard-pitfalls` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230119---shoco---reflection-token |
| 20230119 | ThoreumFinance | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230119---thoreumfinance-business-logic-flaw |
| 20230118 | QTN Token | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230118---qtntoken---business-logic-flaw |
| 20230118 | UPS Token | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230118---upstoken---business-logic-flaw |
| 20230117 | OmniEstate | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230117---OmniEstate---no-input-parameter-check |
| 20230116 | MidasCapital | 2023 | EVM (see DHL entry) | read-only reentrancy | T18 | view price during callback | `get_virtual_price/totalAssets/manageUserBalance` | view price during callback must not be used as collateral | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230116---midascapital---read-only-reentrancy |
| 20230111 | UFDao | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230111---ufdao---incorrect-parameter-setting |
| 20230111 | ROE | 2023 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230111---roefinance---flashloan-price-manipulation |
| 20230110 | BRA | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230110---bra---business-logic-flaw |
| 20230103 | GDS | 2023 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2023/README.md#20230103---gds---business-logic-flaw |
| 20221230 | DFS | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221230---dfs---insufficient-validation--flashloan |
| 20221229 | JAY | 2022 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221229---jay---insufficient-validation--reentrancy |
| 20221225 | Rubic | 2022 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221225---rubic---arbitrary-external-call-vulnerability |
| 20221223 | Defrost | 2022 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221223---defrost---reentrancy |
| 20221214 | Nmbplatform | 2022 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221214---nmbplatform---flashloan-price-manipulation |
| 20221214 | FPR | 2022 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221214---fpr---access-control |
| 20221213 | ElasticSwap | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221213---elasticswap---business-logic-flaw |
| 20221212 | BGLD | 2022 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221212---bgld-deflationary-token---flashloan-price-manipulation |
| 20221211 | Lodestar | 2022 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221211---lodestar---flashloan-price-manipulation |
| 20221211 | MEVbot_0x28d9 | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221211---MEVbot_0x28d9---insufficient-validation |
| 20221210 | MUMUG | 2022 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221210---mumug---flashloan-price-manipulation |
| 20221210 | TIFIToken | 2022 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221210---tifitoken---flashloan-price-manipulation |
| 20221209 | NOVAToken | 2022 | EVM (see DHL entry) | ops / rug | T01 | owner privileges | `onlyOwner/mint(/setFee` | assert non-owner cannot mint or pull LP | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221209---novatoken---malicious-unlimted-minting-rugged |
| 20221207 | AES | 2022 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221207---aes-deflationary-token---business-logic-flaw--flashloan-price-manipulation |
| 20221205 | RFB | 2022 | EVM (see DHL entry) | weak randomness | T50 | block variables as seed | `blockhash/timestamp/random` | same seed is deterministic in test (must not pay jackpot) | `reviewing-mev-ordering-and-slippage` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221205---rfb---predicting-random-numbers |
| 20221205 | BBOX | 2022 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221205---bbox---flashloan-price-manipulation |
| 20221202 | OverNight | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221202---overnight---flashloan-attack |
| 20221201 | APC | 2022 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221201---apc---flashloan--price-manipulation |
| 20221129 | MBC & ZZSH | 2022 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221129---mbc--zzsh---business-logic-flaw--access-control |
| 20221129 | SEAMAN | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221129---seaman---business-logic-flaw |
| 20221123 | NUM | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221123---num---protocol-token-incompatible |
| 20221122 | AUR | 2022 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221122---aur---lack-of-permission-check |
| 20221121 | SDAO | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221121---sdao---business-logic-flaw |
| 20221119 | AnnexFinance | 2022 | EVM (see DHL entry) | callback spoof | T20 | callback caller is a real pool | `uniswapV2Call/uniswapV3SwapCallback` | callback from non-pool address reverts | `reviewing-amm-and-cl-pools` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221119---annexfinance---verify-flashloan-callback |
| 20221118 | Polynomial | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221118---polynomial---no-input-validation |
| 20221117 | UEarnPool | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221117---uearnpool---flashloan-attack |
| 20221116 | SheepFarm | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221116---sheepfarm---no-input-validation |
| 20221110 | DFXFinance | 2022 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221110---dfxfinance---reentrancy |
| 20221109 | brahTOPG | 2022 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221109-brahtopg---arbitrary-external-call-vulnerability |
| 20221108 | MEV_0ad8 | 2022 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221108-mev_0ad8---arbitrary-call |
| 20221108 | Kashi | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221108-kashi---price-caching-design-defect |
| 20221107 | MooCAKECTX | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221107-moocakectx---flashloan-attack |
| 20221105 | BDEX | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221105-bdex---business-logic-flaw |
| 20221027 | VTF | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221027-vtf-token---incorrect-reward-calculation |
| 20221027 | Team Finance | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221027-team-finance---liquidity-migration-exploit |
| 20221026 | N00d Token | 2022 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221026-n00d-token---reentrancy |
| 20221025 | ULME | 2022 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221025-ulme---access-control |
| 20221024 | Market | 2022 | EVM (see DHL entry) | read-only reentrancy | T18 | view price during callback | `get_virtual_price/totalAssets/manageUserBalance` | view price during callback must not be used as collateral | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221024-market---read-only-reentrancy |
| 20221024 | MulticallWithoutCheck | 2022 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221024-multicallwithoutcheck---arbitrary-external-call-vulnerability |
| 20221021 | OlympusDAO | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221021-olympusdao---no-input-validation |
| 20221020 | HEALTH Token | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221020-health---transfer-logic-flaw |
| 20221019 | BEGO Token | 2022 | EVM (see DHL entry) | signature | T04 | ecrecover / nonce / chainId | `ecrecover/permit(/_hashTypedDataV4/nonce` | replay same sig; empty sig != address(0) success | `reviewing-signatures-permit-and-eip712` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221020-bego---incorrect-signature-verification |
| 20221018 | HPAY | 2022 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221018-hpay---access-control |
| 20221018 | PLTD Token | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221018-pltd---transfer-logic-flaw |
| 20221017 | Uerii Token | 2022 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221017-uerii-token---access-control |
| 20221014 | INUKO Token | 2022 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221014-inuko---flashloan-price-manipulation |
| 20221014 | EFLeverVault | 2022 | EVM (see DHL entry) | callback spoof | T20 | callback caller is a real pool | `uniswapV2Call/uniswapV3SwapCallback` | callback from non-pool address reverts | `reviewing-amm-and-cl-pools` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221014-eflevervault---verify-flashloan-callback |
| 20221014 | MEVBOT a47b | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221014-mevbota47b---mevbot-a47b |
| 20221012 | ATK | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221012-atk---flashloan-manipulate-price |
| 20221011 | Rabby Wallet SwapRouter | 2022 | EVM (see DHL entry) | arbitrary call / router | T31 | user calldata + leftover approval | `call(/delegatecall/userData/swap(` | malicious calldata cannot spend a third-party allowance | `reviewing-cross-function-and-composer` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221011-rabby-wallet-swaprouter---arbitrary-external-call-vulnerability |
| 20221011 | Templedao | 2022 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221011-templedao---insufficient-access-control |
| 20221010 | Carrot | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221010-carrot---public-functioncall |
| 20221009 | Xave Finance | 2022 | EVM (see DHL entry) | governance | T29/T30 | vote snapshot / delay | `propose(/queue(/execute(/getVotes` | flash-loan votes cannot pass if snapshot correct | `reviewing-governance-and-timelocks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221009-xave-finance---malicious-proposal-mint--transfer-ownership |
| 20221006 | RES-Token | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221006-RES-Token---pair-manipulate |
| 20221002 | Transit Swap | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221002-transit-swap---incorrect-owner-address-validation |
| 20221001 | BabySwap | 2022 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221001-babyswap---parameter-access-control |
| 20221001 | RL | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221001-RL-Token---Incorrect-Reward-calculation |
| 20221001 | Thunder Brawl | 2022 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20221001-thunder-brawl---reentrancy |
| 20220929 | BXH | 2022 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220928-bxh---flashloan--price-oracle-manipulation |
| 20220928 | MEVBOT Badc0de | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220928-MEVBOT---Badc0de |
| 20220923 | RADT-DAO | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220923-RADT-DAO---pair-manipulate |
| 20220913 | MevBot Private TX | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220913-mevbot-private-tx |
| 20220909 | DPC | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220909-dpc---Incorrect-Reward-calculation |
| 20220908 | YYDS | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220908-YYDS---pair-manipulate |
| 20220908 | NewFreeDAO | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220908-newfreedao---flashloans-attack |
| 20220908 | Ragnarok Online Invasion | 2022 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220908-ragnarok-online-invasion---broken-access-control |
| 20220906 | NXUSD | 2022 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220906-NXUSD---flashloan-price-oracle-manipulation |
| 20220905 | ZoomproFinance | 2022 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220905-zoomprofinance---flashloans--price-manipulation |
| 20220902 | ShadowFi | 2022 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220902-shadowfi---access-control |
| 20220902 | Bad Guys by RPF | 2022 | EVM (see DHL entry) | NFT / marketplace | T19 | hook or listing sig | `onERC721Received/orderHash/nonce` | cancelled listing cannot fill; hook cannot drain before state update | `reviewing-nft-and-marketplace` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220902-bad-guys-by-rpf---business-logic-flaw--missing-check-for-number-of-nft-to-mint |
| 20220828 | DDC | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220828-ddc |
| 20220824 | LuckyTiger NFT | 2022 | EVM (see DHL entry) | NFT / marketplace | T19 | hook or listing sig | `onERC721Received/orderHash/nonce` | cancelled listing cannot fill; hook cannot drain before state update | `reviewing-nft-and-marketplace` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220824-luckytiger-nft---predicting-random-numbers |
| 20220816 | Circle_2 | 2022 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220816-circle---price-manipulation |
| 20220813 | Circle | 2022 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220813-circle---price-manipulation |
| 20220810 | XSTABLE Protocol | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220810-xstable-protocol---incorrect-logic-check |
| 20220809 | ANCH | 2022 | EVM (see DHL entry) | weird ERC-20 | T22 | pair balance vs reserves | `skim/sync/fee/rebase/burn` | FoT / reflection path: recipient gets expected minOut | `reviewing-token-standard-pitfalls` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220809-anch---skim-token-balance |
| 20220807 | EGD Finance | 2022 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220807-egd-finance---flashloans--price-manipulation |
| 20220804 | EtnProduct | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220804-etnproduct---business-logic-flaw |
| 20220803 | Qixi | 2022 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220803-qixi---underflow |
| 20220802 | Nomad Bridge | 2022 | EVM (see DHL entry) | bridge / message | T26 | payload origin / nonce | `processMessage/lzReceive/relay/origin` | replay message id reverts; wrong origin reverts | `reviewing-bridges-and-messaging` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220802-nomad-bridge---business-logic-flaw--incorrect-acceptable-merkle-root-checks |
| 20220801 | Reaper Farm | 2022 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220801-reaper-farm---business-logic-flaw--lack-of-access-control-mechanism |
| 20220725 | LPC | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220725-lpc---business-logic-flaw--incorrect-recipient-balance-check-did-not-check-senderrecipient-in-transfer |
| 20220723 | Audius | 2022 | EVM (see DHL entry) | proxy / storage | T02/T03 | initialize / slot layout | `initialize(/upgradeTo/_authorizeUpgrade` | implementation initialize already disabled; admin slot stable | `reviewing-upgradeable-proxies` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220723-audius---storage-collision--malicious-proposal |
| 20220713 | SpaceGodzilla | 2022 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220713-spacegodzilla---flashloans--price-manipulation |
| 20220710 | Omni NFT | 2022 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220710-omni-nft---reentrancy |
| 20220706 | FlippazOne NFT | 2022 | EVM (see DHL entry) | NFT / marketplace | T19 | hook or listing sig | `onERC721Received/orderHash/nonce` | cancelled listing cannot fill; hook cannot drain before state update | `reviewing-nft-and-marketplace` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220706-flippazone-nft---accesscontrol |
| 20220701 | Quixotic - Optimism NFT Marketplace | 2022 | OP-stack L2 | NFT / marketplace | T19 | hook or listing sig | `onERC721Received/orderHash/nonce` | cancelled listing cannot fill; hook cannot drain before state update | `reviewing-nft-and-marketplace` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220701-quixotic---optimism-nft-marketplace |
| 20220626 | XCarnival | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220626-xcarnival---infinite-number-of-loans |
| 20220624 | Harmony's Horizon Bridge | 2022 | EVM (see DHL entry) | bridge / message | T26 | payload origin / nonce | `processMessage/lzReceive/relay/origin` | replay message id reverts; wrong origin reverts | `reviewing-bridges-and-messaging` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220624-harmonys-horizon-bridge---private-key-compromised |
| 20220618 | SNOOD | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220618-snood---miscalculation-on-_spendallowance |
| 20220616 | InverseFinance | 2022 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220616-inversefinance---flashloan--price-oracle-manipulation |
| 20220608 | GYMNetwork | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220608-gymnetwork---accesscontrol |
| 20220608 | Optimism - Wintermute | 2022 | OP-stack L2 | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220608-optimism---wintermute |
| 20220606 | Discover | 2022 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220606-discover---flashloan--price-oracle-manipulation |
| 20220529 | NOVO Protocol | 2022 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220529-novo-protocol---flashloan--price-oracle-manipulation |
| 20220524 | HackDao | 2022 | EVM (see DHL entry) | weird ERC-20 | T22 | pair balance vs reserves | `skim/sync/fee/rebase/burn` | FoT / reflection path: recipient gets expected minOut | `reviewing-token-standard-pitfalls` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220524-HackDao---Skim-token-balance |
| 20220517 | ApeCoin | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220517-apecoin-ape---flashloan |
| 20220508 | Fortress Loans | 2022 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220508-fortress-loans---malicious-proposal--price-oracle-manipulation |
| 20220430 | Saddle Finance | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220430-saddle-finance---swap-metapool-attack |
| 20220430 | Rari Capital/Fei Protocol | 2022 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220430-rari-capitalfei-protocol---flashloan-attack--reentrancy |
| 20220428 | DEUS DAO | 2022 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220428-deus-dao---flashloan--price-oracle-manipulation |
| 20220424 | Wiener DOGE | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220424-wiener-doge---flashloan |
| 20220423 | Akutar NFT | 2022 | EVM (see DHL entry) | NFT / marketplace | T19 | hook or listing sig | `onERC721Received/orderHash/nonce` | cancelled listing cannot fill; hook cannot drain before state update | `reviewing-nft-and-marketplace` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220423-akutar-nft---denial-of-service |
| 20220421 | Zeed Finance | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220421-zeed-finance |
| 20220416 | BeanstalkFarms | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220416-beanstalkfarms---dao--flashloan |
| 20220415 | Rikkei Finance | 2022 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220415-rikkei-finance---access-control--price-oracle-manipulation |
| 20220412 | ElephantMoney | 2022 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220412-elephantmoney---flashloan--price-oracle-manipulation |
| 20220411 | Creat Future | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220411-creat-future |
| 20220409 | GYMNetwork | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220409-gymnetwork---flashloan--token-migrate-flaw |
| 20220329 | Ronin Network | 2022 | EVM (see DHL entry) | bridge / message | T26 | payload origin / nonce | `processMessage/lzReceive/relay/origin` | replay message id reverts; wrong origin reverts | `reviewing-bridges-and-messaging` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220329-ronin-network---Bridge |
| 20220329 | Redacted Cartel | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220329-redacted-cartel---custom-approval-logic |
| 20220327 | Revest Finance | 2022 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220327-revest-finance---reentrancy |
| 20220326 | Auctus | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220326-auctus |
| 20220322 | CompoundTUSDSweepTokenBypass | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220322-compoundtusdsweeptokenbypass |
| 20220321 | OneRing Finance | 2022 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220321-onering-finance---flashloan--price-oracle-manipulation |
| 20220320 | LI.FI | 2022 | EVM (see DHL entry) | bridge / message | T26 | payload origin / nonce | `processMessage/lzReceive/relay/origin` | replay message id reverts; wrong origin reverts | `reviewing-bridges-and-messaging` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220320-LiFi---bridges |
| 20220320 | Umbrella Network | 2022 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220320-umbrella-network---underflow |
| 20220315 | Agave Finance | 2022 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220315-agave-finance---erc667-reentrancy |
| 20220315 | Hundred Finance | 2022 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220313-hundred-finance---erc667-reentrancy |
| 20220313 | Paraluni | 2022 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220313-paraluni---flashloan--reentrancy |
| 20220309 | Fantasm Finance | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220309-fantasm-finance---business-logic-in-mint |
| 20220305 | Bacon Protocol | 2022 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220305-bacon-protocol---reentrancy |
| 20220303 | TreasureDAO | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220303-treasuredao---zero-fee |
| 20220214 | BuildFinance - DAO | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220214-buildfinance---dao |
| 20220208 | Sandbox LAND | 2022 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220208-sandbox-land---access-control |
| 20220205 | Meter | 2022 | EVM (see DHL entry) | bridge / message | T26 | payload origin / nonce | `processMessage/lzReceive/relay/origin` | replay message id reverts; wrong origin reverts | `reviewing-bridges-and-messaging` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220205-Meter---bridge |
| 20220204 | TecraSpace | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220204-TecraSpace---Any-token-is-destroyed |
| 20220128 | Qubit Finance | 2022 | EVM (see DHL entry) | bridge / message | T26 | payload origin / nonce | `processMessage/lzReceive/relay/origin` | replay message id reverts; wrong origin reverts | `reviewing-bridges-and-messaging` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220128-qubit-finance---bridge-address0safetransferfrom-does-not-revert |
| 20220118 | Multichain (Anyswap) | 2022 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2022/README.md#20220118-multichain-anyswap---insufficient-token-validation |
| 20211221 | Visor Finance | 2021 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20211221-visor-finance---reentrancy |
| 20211218 | Grim Finance | 2021 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20211218-grim-finance---flashloan--reentrancy |
| 20211214 | Nerve Bridge | 2021 | EVM (see DHL entry) | bridge / message | T26 | payload origin / nonce | `processMessage/lzReceive/relay/origin` | replay message id reverts; wrong origin reverts | `reviewing-bridges-and-messaging` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20211214-nerve-bridge---swap-metapool-attack |
| 20211130 | MonoX Finance | 2021 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20211130-monox-finance---price-manipulation |
| 20211123 | Ploutoz Finance | 2021 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20211123-ploutoz---flash-loan |
| 20211027 | Cream Finance | 2021 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20211027-creamfinance---price-manipulation |
| 20211015 | Indexed Finance | 2021 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20211015-indexed-finance---price-manipulation |
| 20210916 | SushiSwap Miso | 2021 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210916-sushiswap-miso |
| 20210915 | Nimbus Platform | 2021 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210915-nimbus-platform |
| 20210915 | NowSwap Platform | 2021 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210915-nowswap-platform |
| 20210912 | ZABU Finance | 2021 | EVM (see DHL entry) | weird ERC-20 | T22 | pair balance vs reserves | `skim/sync/fee/rebase/burn` | FoT / reflection path: recipient gets expected minOut | `reviewing-token-standard-pitfalls` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210912-ZABU-Finance---Deflationary-token-uncompatible |
| 20210903 | DAO Maker | 2021 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210903-dao-maker---bad-access-controal |
| 20210830 | Cream Finance | 2021 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210830-cream-finance---flashloan-attack--reentrancy |
| 20210817 | XSURGE | 2021 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210817-xsurge---flashloan-attack--reentrancy |
| 20210811 | Poly Network | 2021 | EVM (see DHL entry) | bridge / message | T26 | payload origin / nonce | `processMessage/lzReceive/relay/origin` | replay message id reverts; wrong origin reverts | `reviewing-bridges-and-messaging` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210811-poly-network---bridge-getting-around-modifier-through-cross-chain-message |
| 20210804 | WaultFinance | 2021 | EVM (see DHL entry) | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210804-waultfinace---flashloan-price-manipulation |
| 20210804 | Popsicle | 2021 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210804-popsicle---repeated-reward-claim---logic-flaw |
| 20210728 | Levyathan Finance | 2021 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210728-levyathan-finance---i-lost-keys-and-minting-ii-vulnerable-emergencywithdraw |
| 20210710 | Chainswap | 2021 | EVM (see DHL entry) | bridge / message | T26 | payload origin / nonce | `processMessage/lzReceive/relay/origin` | replay message id reverts; wrong origin reverts | `reviewing-bridges-and-messaging` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210710-chainswap---bridge-logic-flaw |
| 20210702 | Chainswap | 2021 | EVM (see DHL entry) | bridge / message | T26 | payload origin / nonce | `processMessage/lzReceive/relay/origin` | replay message id reverts; wrong origin reverts | `reviewing-bridges-and-messaging` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210702-chainswap---bridge-logic-flaw |
| 20210628 | SafeDollar | 2021 | EVM (see DHL entry) | weird ERC-20 | T22 | pair balance vs reserves | `skim/sync/fee/rebase/burn` | FoT / reflection path: recipient gets expected minOut | `reviewing-token-standard-pitfalls` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210628-safedollar---deflationary-token-uncompatible |
| 20210625 | xWin Finance | 2021 | BSC | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210625-xwin-finance---subscription-incentive-mechanism |
| 20210622 | Eleven Finance | 2021 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210622-eleven-finance---doesnt-burn-shares |
| 20210607 | 88mph NFT | 2021 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210607-88mph-nft---access-control |
| 20210603 | PancakeHunny | 2021 | BSC | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210603-pancakehunny---incorrect-calculation |
| 20210527 | JulSwap | 2021 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210527-julswap---flash-loan |
| 20210527 | BurgerSwap | 2021 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210527-burgerswap---mathematical-flaw--reentrancy |
| 20210519 | PancakeBunny | 2021 | BSC | oracle / spot | T06 | AMM reserves or donatable index as price | `getReserves/slot0/latestRoundData/consult/twap` | same-tx reserve skew then borrow/liquidate must fail health check | `reviewing-oracles-and-pricing` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210519-pancakebunny---price-oracle-manipulation |
| 20210516 | bEarn | 2021 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210516-bearn---logic-flaw |
| 20210508 | Rari Capital | 2021 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210509-raricapital---cross-contract-reentrancy |
| 20210508 | Value Defi | 2021 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210508-value-defi---cross-contract-reentrancy |
| 20210502 | Spartan | 2021 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210502-spartan---logic-flaw |
| 20210428 | Uranium | 2021 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210428-uranium---miscalculation |
| 20210308 | DODO | 2021 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210308-dodo---flashloan-attack |
| 20210305 | Paid Network | 2021 | EVM (see DHL entry) | ops / key | T28 | operators / off-chain | n/a (ops) | process review: threshold and hardware isolation | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210305-paid-network---private-key-compromised |
| 20210204 | Yearn YDai | 2021 | EVM (see DHL entry) | missing slippage | T32 | no minOut / deadline | `amountOutMin/deadline/minAmount` | zero minOut rejected or documented as user risk | `reviewing-mev-ordering-and-slippage` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210204-yearn-ydai---Slippage-proection-absent |
| 20210125 | Sushi Badger Digg | 2021 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2021/README.md#20210125-sushi-badger-digg---sandwich-attack |
| 20201229 | Cover Protocol | 2020 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2020/README.md#20201229-cover-protocol |
| 20201121 | Pickle Finance | 2020 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2020/README.md#20201121-pickle-finance |
| 20201026 | Harvest Finance | 2020 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2020/README.md#20201026-harvest-finance---flashloan-attack |
| 20200912 | bzx | 2020 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2020/README.md#20200912-bzx---incorrect-transfer |
| 20200804 | Opyn Protocol | 2020 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2020/README.md#20200804-opyn-protocol---msgValue-in-loop |
| 20200628 | Balancer Protocol | 2020 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2020/README.md#20200628-balancer-protocol---token-incompatible |
| 20200618 | Bancor Protocol | 2020 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2020/README.md#20200618-bancor-protocol---access-control |
| 20200419 | LendfMe | 2020 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2020/README.md#20200419-lendfme---erc777-reentrancy |
| 20200418 | UniSwapV1 | 2020 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2020/README.md#20200418-uniswapv1---erc777-reentrancy |
| 20181007 | SpankChain | 2018 | EVM (see DHL entry) | reentrancy | T16 | external call before state | `call{value/safeTransfer/onERC721Received/nonReentrant` | callback cannot complete the same value-moving function twice | `reviewing-reentrancy-and-callbacks` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2018/README.md#20181007-spankchain---reentrancy |
| 20180424 | SmartMesh | 2018 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2018/README.md#20180424-smartmesh---overflow |
| 20180422 | Beauty Chain | 2018 | EVM (see DHL entry) | rounding / precision | T33 | integer division direction or decimals | `mulDiv// /decimals` | amounts 1, 2, max-1; rounding favors protocol | `writing-foundry-invariant-handlers` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2018/README.md#20180422-beauty-chain---integer-overflow |
| 20171106 | Parity - 'Accidentally Killed It' | 2017 | EVM (see DHL entry) | business logic | T35/T36 | happy-path accounting | named function + `require/unchecked` | fuzz inverted conservation invariant on the named module | `triaging-and-deduping-findings` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2017/README.md#20171106-parity---accidentally-killed-it |
| 20170719 | Parity Multisig | 2017 | EVM (see DHL entry) | access control | T01 | msg.sender / missing modifier | `onlyOwner/onlyRole/initialize` | vm.prank(attacker) on state changer reverts | `reviewing-access-control-and-auth` | https://github.com/SunWeb3Sec/DeFiHackLabs/blob/main/past/2017/README.md#20170719-parity-multisig---delegatecall-to-unprotected-initwallet |

End of ingest. Re-run parser against DeFiHackLabs when the index grows.

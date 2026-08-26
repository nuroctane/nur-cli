# Notable paid bounties (public)

**Paid** whitehat reports. Loss-side incidents are `incident-timeline.md`. Ceilings that were never paid at cap are marked **program max**, not paid.

Dollar figures are what the writeup or Immunefi stated. Re-check before citing in a report. X posts are pointers (`x-and-public-signal.md`).

Primary indexes:
- [sayan011/immunefi-bug-bounty-writeups-list](https://github.com/sayan011/immunefi-bug-bounty-writeups-list)
- [Immunefi BugFixReviews](https://github.com/immunefi-team/Web3-Security-Library/blob/main/BugFixReviews/README.md)
- [Immunefi hackers](https://immunefi.com/hackers/)

## Program ceilings (2026, not the same as paid)

| Program | Stated max | Notes |
|---------|------------|-------|
| Usual (Sherlock) | ~$16M | coverage-style ceiling; confirm live |
| Uniswap v4 (Immunefi) | $15.5M | hooks + core; [Uniswap v4 bounty](https://immunefi.com/bug-bounty/uniswapv4/) |
| LayerZero | $15M | messaging; config bugs are in scope more often than people think |
| Wormhole | $10M (historically; W-token tiers later) | paid the record $10M |
| Sky (Maker) | $10M | |
| Kamino (Solana) | $1.5M | largest Solana DeFi ceiling cited Oct 2025 |
| Injective | $500k | Cosmos L1 |

Source: [Sherlock 2026 bounty roundup](https://sherlock.xyz/post/best-web3-bug-bounties-in-2026-the-highest-paying-programs-on-every-platform) plus program pages. Always open the live Immunefi asset list.

## Highest disclosed payouts

| $ (stated) | Year | Protocol | Chain / VM | Class (taxonomy) | Whitehat | Writeup |
|------------|------|----------|------------|------------------|----------|---------|
| $10M | 2022 | Wormhole | Solana + EVM portal | T12 uninitialized proxy | satya0x | [Immunefi](https://medium.com/immunefi/wormhole-uninitialized-proxy-bugfix-review-90250c41a43a) |
| $6M | 2022 | Aurora | Near + EVM | infinite ETH mint / withdrawal logic | pwning.eth | [Immunefi](https://medium.com/immunefi/aurora-infinite-spend-bugfix-review-6m-payout-e635d24273d) |
| $2.2M | 2021 | Polygon MRC20 | Polygon | lack of balance check (T02/T18) | Leon Spacewalker | [Immunefi](https://medium.com/immunefi/polygon-lack-of-balance-check-bugfix-postmortem-2-2m-bounty-64ec66c24c7d) |
| ~$2M | 2021 | Polygon Plasma | Polygon | double-spend / exit replay (T17) | Gerhard Wagner (often cited) | [Immunefi double-spend](https://medium.com/immunefi/polygon-double-spend-bug-fix-postmortem-2m-bounty-5a1db09db7f1) |
| $2M | 2022 | Optimism | OP Stack | infinite money duplication (T27/T33) | saurik | [Immunefi](https://medium.com/immunefi/optimism-infinite-money-duplication-bugfix-review-daa6597146a0) |
| $2M+$75k | 2024 | Sei | Sei / CosmWasm-adj | chain / module criticals (O04) | usmannk | [usmannkhan.com](https://usmannkhan.com/bug%20reports/2024/06/17/sei-bug-report.html) |
| $1.5M | 2021 | ArmorFi | Ethereum | coverage math (T01) | (Immunefi postmortem) | [Immunefi](https://medium.com/immunefi/armorfi-bug-bounty-postmortem-cf46eb650b38) |
| $1.1M | 2023 | Beanstalk | Ethereum | insufficient input validation (T03) | nicole | [Immunefi](https://medium.com/immunefi/beanstalk-insufficient-input-validation-bugfix-review-fc3fdbaab15b) |
| $1M+ | 2022 | Moonbeam | Polkadot Frontier | precompile + truncation (O02) | pwning.eth | [mirror](https://pwning.mirror.xyz/okyEG4lahAuR81IMabYL5aUdvAsZ8cRCbYBXh8RHFuE) |
| $1M | 2022 | Polkadot Frontier EVM | Polkadot | same family (O02) | pwning.eth | [mirror](https://pwning.mirror.xyz/RFNTSouIIlHVNmTNDThUVb1obIeN5c1LAiQuN9Ve-ok) |
| $1M | 2023 | Balancer | Ethereum | rounding + flashSwap (T04) | GothicShanon89238 | [Immunefi](https://medium.com/immunefi/balancer-rounding-error-bugfix-review-cbf69482ee3d) |
| $1M | 2025 | Scroll | Scroll zk | message spoofing (T19) | WhiteHatMage | [Scroll forum](https://forum.scroll.io/t/report-scroll-mainnet-emergency-upgrade-on-2025-04-25/666) |
| $1M+$50k | 2021 | Belt | BSC | logic / share inflation (T01) | Bobface | [Immunefi](https://medium.com/immunefi/belt-finance-logic-error-bug-fix-postmortem-39308a158291) |
| $800k | 2021 | Fei | Ethereum | flashloan / oracle (T05/T09) | Bobface | [Immunefi](https://medium.com/immunefi/fei-protocol-flashloan-vulnerability-postmortem-7c5dc001affb) |
| ~$630k | 2022 | Port Finance | Solana | withdraw collateral without full debt (S02/T14) | nojob | [Immunefi](https://medium.com/immunefi/port-finance-logic-error-bugfix-review-29767aced446) |
| $560k | 2022 | Redacted Cartel | Ethereum | custom approval logic (T02) | Tommaso Pifferi | [Immunefi](https://medium.com/immunefi/redacted-cartel-custom-approval-logic-bugfix-review-9b2d039ca2c5) |
| ~$560k (400 ETH) | 2022 | Arbitrum Nitro | Arbitrum | uninitialized inbox (T28) | 0xriptide | [medium](https://medium.com/@0xriptide/hackers-in-arbitrums-inbox-ca23272641a2) |
| $505k | 2024 | Raydium | Solana | tick manipulation (S09) | riproprip | [Immunefi](https://medium.com/immunefi/raydium-tick-manipulation-bugfix-review-c6aae4527ed6) |
| $400k | 2023 | Enzyme | Ethereum | missing privilege (T02) | rootrescue | [Immunefi](https://medium.com/immunefi/enzyme-finance-missing-privilege-check-bugfix-review-ddb5e87b8058) |
| $1M+ note | 2022 | Notional | Ethereum | free collateral double-count (T01/T14) | 0x60511e57 | [Immunefi](https://medium.com/immunefi/notional-double-counting-free-collateral-bugfix-review-28b634903934) |
| $250k | 2023 | Sherlock Yield Strategy | Ethereum | strategy logic | GothicShanon | [mirror](https://mirror.xyz/0xE400820f3D60d77a3EC8018d44366ed0d334f93C/LOZF1YBcH1eBdxlC6HP223cAMeTpNgQ-Kc4EjQuxmGA) |
| $250k | 2024 | Balancer V2 | Ethereum | (kankodu writeup) | kankodu | [mirror](https://mirror.xyz/0x38F1416B9Ed3a5DA9C12c56cb4F74D9564844728/iv9_q74rSlK7gbvbJAECuDIbzfUtrSCO6mSWIHPskKI) |
| $200k | 2022 | Interlay | Polkadot | Frontier-family | pwning.eth | [mirror](https://pwning.mirror.xyz/jlT8OgtwN3mQf3KdYmXdcSXbE4s95JzT3eR3wxiLmpw) |
| $200k | 2024 | zkSync Lite | zkSync | insufficient proof verification (T31) | LonelySloth | [Immunefi](https://medium.com/immunefi/zksync-insufficient-proof-verification-bugfix-review-dcd57944d0e2) |
| $200k | 2024 | Oasys | Oasys | (merkle_bonsai) | merkle_bonsai | [mirror](https://mirror.xyz/0x333247F2e126954ed6428e9135Ae9dE06A76BA32/a6HqOCOjJ10Bosyi0cGz6Lxff8t68Uo4YvFsVg2tHaw) |
| $200k | 2023 | Tranchess | BSC | (Flora) | Flora | [github](https://github.com/floranguyen0/tranchess-vulnerability-disclosure) |
| ~$182k | 2023 | Beanstalk | Ethereum | logic error (T03) | (Immunefi) | [Immunefi](https://medium.com/immunefi/beanstalk-logic-error-bugfix-review-4fea17478716) |
| $150k | 2024 | Evmos | Cosmos EVM | docs-driven critical | jayjonah.eth | [medium](https://medium.com/@jjordanjjordan/150-000-evmos-vulnerability-through-reading-documentation-d26328590a7a) |
| $150k | 2022 | Synthetix | Ethereum | fee reclamation logic (T01) | thunderdeep14 | [Immunefi](https://medium.com/immunefi/synthetix-logic-error-bugfix-review-40da0ead5f4f) |
| $100k | 2025 | Story Network | Story | (WhiteHatMage) | WhiteHatMage | [story.foundation](https://www.story.foundation/blog/story-network-postmortem) |
| $100k | 2022 | APWine | Ethereum | PT burn delegation (T03) | setuid0 | [Immunefi](https://medium.com/immunefi/apwine-incorrect-check-of-delegations-bugfix-review-7e401a49c04f) |
| $100k | 2023 | DFX | Ethereum | rounding / 2-decimal EURS (T04) | perseverance | [Immunefi](https://medium.com/immunefi/dfx-finance-rounding-error-bugfix-review-17ba5ffb4114) |
| $100k | 2023 | Silo | Ethereum | interest-rate logic (T14) | kankodu | [x](https://twitter.com/kankodu/status/1669833829203476480) |
| ~$95k | 2023 | Yield Protocol | Ethereum | pool token-balance logic (T01) | Paludo0x | [Immunefi](https://medium.com/immunefi/yield-protocol-logic-error-bugfix-review-7b86741e6f50) |
| ~$76k | 2024 | Stacks | Stacks / Clarity | DoS (B03) | Catchme | [Immunefi](https://medium.com/immunefi/stacks-dos-bugfix-review-dc0f2a75b276) |
| $75k | 2022 | Polygon | Polygon PoS | consensus bypass (O04) | Niv Yehezkel | [Immunefi](https://medium.com/immunefi/polygon-consensus-bypass-bugfix-review-7076ce5047fe) |
| $75k | 2024 | Sei | Sei | (Catchme / Exvul) | Catchme | [exvul](https://exvul.com/share-the-details-sei-protocol-vulnerability-worth-75k/) |
| $70k | 2025 | Acala | Polkadot | block production shutdown | Lastc0de | [Immunefi](https://immunefi.com/blog/all/acala-block-production-shutdown-bug-fix-review/) |
| $50k | 2024 | zkSync Era | zkSync | zkEVM soundness (T31) | ChainLight | [ChainLight](https://medium.com/chainlight/uncovering-a-zk-evm-soundness-bug-in-zksync-era-f3bc1b2a66d8) |
| $50k | 2025 | Sui | Move | network shutdown (M06) | F4lt | [Immunefi](https://immunefi.com/blog/bug-fix-reviews/sui-network-shutdown/) |
| $50k | 2025 | Axelar | Cosmos | cross-chain halt (C01) | Marco Nunes | [marcotnunes.com](https://marcotnunes.com/axelar-network-cross-chain-halt-vulnerability/) |
| $50k | 2025 | VeChainThor | VeChain | VTHO accrual bypass | nnez | [Immunefi](https://immunefi.com/blog/all/vechainthor-vtho-accrual-bypass-bug-fix-review/) |
| $50k | 2026 | Injective | Cosmos | (f4lc0n) | f4lc0n | [x](https://x.com/al_f4lc0n/status/2033110168045568434) |
| $50k | 2024 | Wormhole | multi | high (Marco Nunes) | Marco Nunes | [x](https://x.com/marcotnunes/status/1889707212450234629) |
| $50k | 2023 | Astar | Polkadot | (Zellic) | Zellic | [zellic.io](https://www.zellic.io/blog/finding-a-critical-vulnerability-in-astar/) |
| $50k | 2022 | Sense | Ethereum | `onSwap` access control (T02) | alephv.eth | [Immunefi](https://medium.com/immunefi/sense-finance-access-control-issue-bugfix-review-32e0c806b1a0) |
| $50k | 2022 | Fluidity | Ethereum | (Trust) | Trust | [trust-security](https://www.trust-security.xyz/post/breaking-fluidity-for-glory-and-50k) |
| $50k | 2023 | Q Blockchain | Q | (Blockian) | Blockian | [medium](https://medium.com/@blockian/striking-gold-at-30-000-feet-uncovering-a-critical-vulnerability-in-q-blockchain-for-50-000-ab335042147b) |
| $42k | 2021 | 88mph | Ethereum | unprotected `init` (T12) | Ashiq Amien | [Immunefi](https://medium.com/immunefi/88mph-function-initialization-bug-fix-postmortem-c3a2282894d3) |
| $40k | 2022 | Cronos | Cosmos EVM | theft of tx fees (C05) | zb3 | [Immunefi](https://medium.com/immunefi/cronos-theft-of-transactions-fees-bugfix-postmortem-b33f941b9570) |
| ~$290k | 2024 | The Graph | Ethereum | rounding (T04) | GregadETH | [Immunefi](https://medium.com/immunefi/the-graph-rounding-error-bugfix-review-c946ff470f65) |
| $20k | 2023 | Optimism | OP | censorship bug | iosiro | [iosiro](https://www.iosiro.com/blog/optimism-censorship-bug-disclosure) |
| $60k | ~2021 | Mushrooms | Ethereum | logic (T01) | CKK Sec | [Immunefi](https://medium.com/immunefi/mushrooms-finance-logic-error-bug-fix-postmortem-780122821621) |
| $50k | ~2023 | BendDAO | Ethereum | NFT flash claim (T32) | | [BendDAO](https://medium.com/@BendDAO/sewer-pass-flash-claim-vulnerability-9d2b0b1e09ef) |
| $50k | ~2023 | Trust alias cluster | 100+ L2 | L1-L2 alias (T27) | Trust Security | [trust-security](https://www.trust-security.xyz/post/permission-denied) |
| $40k of ~$70k | 2024 | Hats Velvet Capital | EVM | calldata (T34) | 16 wardens | [Velvet](https://github.com/Velvet-Capital/audits/blob/main/report.md) |
| $30k | 2023 | Perpetual Protocol | Optimism | bad debt (T14) | banditx0x | [securitybandit](https://securitybandit.com/2023/02/07/bad-debt-attack-for-perpetual-protocol/) |
| $28k | ~2023 | Alchemist | Ethereum | admin brick (T02) | Dacian | [dacian.me](https://dacian.me/28k-bounty-admin-brick-forced-revert) |
| $25k | ~2022 | Ondo | Ethereum | high | Ashiq / iosiro | [iosiro](https://iosiro.com/blog/high-risk-vulnerability-disclosed-to-ondo-finance) |
| $25k | ~2021 | Zapper | Ethereum | arbitrary calldata (T34) | Lucash-dev | [Immunefi](https://medium.com/immunefi/zapper-arbitrary-call-data-bug-fix-postmortem-d75a4a076ae9) |
| $25k | ~2021 | Tidal | Ethereum | logic (T01) | csanuragjain | [Immunefi](https://medium.com/immunefi/tidal-finance-logic-error-bug-fix-postmortem-3607d8b7ed1f) |
| $20k | 2023 | Thena | BSC | gauge logic (C4 miss) | zzykxx | [zzykxx](https://zzykxx.com/2023/02/02/the-bug-that-codearena-missed-,-twice/) |
| $20k | 2023 | Thena (second) | BSC | logic | zzykxx | [zzykxx](https://zzykxx.com/2023/02/27/a-very-helpful-sign/) |
| $20k | ~2022 | Oasis | Oasis | platform halt (O04) | Trust | [trust-security](https://www.trust-security.xyz/post/taking-home-a-20k-bounty-with-oasis-platform-shutdown-vulnerability) |
| $19k | ~2021 | Enzyme oracle | Ethereum | spot/oracle (T05) | setuid0 | [Immunefi](https://medium.com/immunefi/enzyme-finance-price-oracle-manipulation-bug-fix-postmortem-4e1f3d4201b5) |
| $15k | 2024 | Sovryn | Rootstock | (X) | gandu_whitehat | [x](https://x.com/gandu_whitehat/status/1803794103248806223) |
| $10k | ~2021 | Mt Pelerin | Ethereum | double tx / replay (T10) | | [Immunefi](https://medium.com/immunefi/mt-pelerin-double-transaction-bugfix-review-503838db3d70) |
| $7.5k | ~2021 | Alchemix access | Ethereum | T02 | Ashiq | [Immunefi](https://medium.com/immunefi/alchemix-access-control-bug-fix-debrief-a13d39b9f2e0) |
| $6.71k | 2025 | Movement Labs | Move | chain split (M06) | Yunus Emre | [medium](https://medium.com/@yemresaritoprak/permanent-chain-split-in-movement-full-node-anatomy-of-a-6-710-critical-vulnerability-that-fa75fe66a0c7) |
| $5k | ~2021 | Charged Particles | Ethereum | grief (T03) | janbro.eth | [Immunefi](https://medium.com/immunefi/charged-particles-griefing-bug-fix-postmortem-d2791e49a66b) |
| $5k | ~2022 | O3 bridge | Multi | T18 | Trust, 0xDjango | [trust-security](https://www.trust-security.xyz/post/critical-finding-stealing-tokens-from-o3-bridge-users) |
| $5k | ~2023 | LayerZero (Trust low) | Multi | T20 | Trust Security | [trust-security](https://www.trust-security.xyz/post/learning-by-breaking-a-layerzero-case-study-part-3) |
| $5k | ~2021 | xDai Stake | Gnosis | arbitrary call (T34) | 0xadee028d | [Immunefi](https://medium.com/immunefi/xdai-stake-arbitrary-call-method-bug-postmortem-f80a90ac56e3) |
| $4.5k | ~2021 | Bitswift | | unlimited mint (T02) | | [Immunefi](https://medium.com/immunefi/bitswift-unlimited-mint-bugfix-postmortem-147a1e57dca9) |
| $2k | ~2022 | Fringe.fi | Ethereum | insolvency (T14) | Trust | [trust-security](https://www.trust-security.xyz/post/diving-deep-into-a-critical-protocol-insolvency-bug-in-fringe-fi-lending-platform) |
| $1k | 2025 | Scroll (Shabarkin) | Scroll | (X) | Pavel Shabarkin | [x](https://x.com/shabarkin/status/1917483039195816213) |
| 50 ETH | ~2022 | Balancer (riptide) | Ethereum | pool | riptide | [mirror](https://mirror.xyz/0x2719F6Dfb85086F87319079cC2f7EeFD0e40994D/NWDf5uW1Ve7-TrcPKwmM86xp8ploMSCRGC58A-NSoFY) |
| 44.8 ETH | ~2022 | Tranchess first-run | BSC | T06 | Jade | [kalos](https://www.kalos.xyz/blog/tranchess-liquid-staking-deposit-firstrun-vulnerability-analysis) |

Every row is also in `hunting-x-linked-bounties/references/paid-payouts.md` grouped by **playbook**. X-only 2024-2026 pointers: `hunting-x-linked-bounties/references/x-linked-2024-2026.md`.

NEAR HackenProof (~$1.8M) is often **web/app mixed**. Do not file it as a Solidity analog without reading the report.

## Deep cards (top classes that keep paying)

### Wormhole uninitialized proxy (2022) - $10M

- **Chain/VM:** Solana guardian set + EVM/Solana portal contracts.
- **Class:** T12.
- **Wrongly trusted:** the upgradeable **implementation** was never initialized, so anyone who initialized it could `selfdestruct` / brick the portal.
- **Grep:** `initialize`, `initializer`, `upgradeTo`, implementation address vs proxy, `selfdestruct`.
- **Local test idea:** in your UUPS suite, `test_RevertWhen_AttackerInitializesImplementation`. Assert impl `initialize` reverts or is already locked. Do not target Wormhole mainnet.
- **Source:** [Immunefi bugfix](https://medium.com/immunefi/wormhole-uninitialized-proxy-bugfix-review-90250c41a43a).

### Aurora infinite spend (2022) - $6M

- **Chain/VM:** Aurora (EVM on Near).
- **Class:** bridge withdrawal / infinite mint (T18 + O01).
- **Wrongly trusted:** ETH withdrawal / NEP-141 mapping could be satisfied without a real burn.
- **Grep:** `withdraw`, `burn`, `submit`, token mapping, any "proof" that is just a bytes blob.
- **Local test idea:** dual-VM mock: mint on EVM must require a recorded burn on the other side (ghost). Related: Aurora improper NEP-141 sanitization review.
- **Source:** [Immunefi $6M](https://medium.com/immunefi/aurora-infinite-spend-bugfix-review-6m-payout-e635d24273d).

### Polygon MRC20 lack of balance check (2021) - $2.2M

- **Wrongly trusted:** `transfer` moved MATIC without balance/allowance.
- **Local test idea:** `test_RevertWhen_TransferWithoutBalance` on any custom native-wrapper.
- **Source:** [Immunefi](https://medium.com/immunefi/polygon-lack-of-balance-check-bugfix-postmortem-2-2m-bounty-64ec66c24c7d).

### Optimism infinite duplication (2022) - $2M (saurik)

- **Wrongly trusted:** OVM 2.0 ETH accounting across L1/L2 could duplicate.
- **Local test idea:** ghost: L2 minted ETH <= L1 locked + documented bridge mint. OP Stack custom gas token programs inherit this class.
- **Source:** [Immunefi](https://medium.com/immunefi/optimism-infinite-money-duplication-bugfix-review-daa6597146a0).

### Moonbeam / Frontier truncation (2023) - $1M class

- **Wrongly trusted:** Substrate Frontier precompile / library truncated values, minting depegged wrapped tokens (~$200M hypothetical across Moonbeam, Astar, Acala).
- **Local test idea:** fuzz precompile inputs at type-width boundaries (u64 vs u128 vs U256).
- **Source:** [Immunefi $1M payout review](https://medium.com/immunefi/moonbeam-astar-and-acala-library-truncation-bugfix-review-1m-payout-41a862877a5b).

### Balancer rounding + flashSwap (2023) - $1M

- **Wrongly trusted:** rounding direction + flash swap could drain boosted pools.
- **Local test idea:** invariant `swap` cannot decrease pool value for the attacker across 1-wei and max ticks; include a same-tx flash.
- **Source:** [Immunefi](https://medium.com/immunefi/balancer-rounding-error-bugfix-review-cbf69482ee3d). Related loss: Balancer V2 composable stables 2025 ~$128M (timeline).

### Scroll message spoofing (2025) - $1M

- **Wrongly trusted:** bridge message sender / decoder.
- **Local test idea:** `lzReceive`/`onMessage` with a spoofed sender must revert; leftover calldata cannot change amount.
- **Source:** [Scroll forum emergency upgrade](https://forum.scroll.io/t/report-scroll-mainnet-emergency-upgrade-on-2025-04-25/666).

### Raydium tick manipulation (2024) - $505k

- **Chain/VM:** Solana CL.
- **Wrongly trusted:** tick array / price could be moved without paying the CL invariant.
- **Local test idea:** Anchor test: after a swap, `sqrt_price` and liquidity net must match a ghost AMM; attacker-owned tick arrays cannot skip liquidity.
- **Source:** [Immunefi](https://medium.com/immunefi/raydium-tick-manipulation-bugfix-review-c6aae4527ed6).

### zkSync proof bugs (Era $50k, Lite $200k)

- **Wrongly trusted:** verifier public inputs / proof completeness.
- **Local test idea:** you will not re-implement the prover. Review: every public input is bound; stale roots cannot verify. Read ChainLight / Immunefi writeups.
- **Sources:** [Era soundness](https://medium.com/chainlight/uncovering-a-zk-evm-soundness-bug-in-zksync-era-f3bc1b2a66d8), [Lite proof verification](https://medium.com/immunefi/zksync-insufficient-proof-verification-bugfix-review-dcd57944d0e2).

### Sui network shutdown (2025) - $50k

- **Wrongly trusted:** a pathological tx cannot crash validators.
- **Local test idea:** Move test for abort paths; client-level DoS is `auditing-blockchain-clients`.
- **Source:** [Immunefi](https://immunefi.com/blog/bug-fix-reviews/sui-network-shutdown/).

### Vesu rounding (Cairo)

- **Wrongly trusted:** rounding convention on Starknet lending.
- **Local test idea:** Cairo test at 1 wei, `u256` div both directions.
- **Sources:** [Vesu disclosure](https://docs.vesu.xyz/security/disclosures-report/rounding-convention-bug-disclosure), kankodu X pointer.

## Also-paid (keep in the grep brain)

Enzyme oracle ($19k) vs Enzyme privilege ($400k) - same protocol, different class. Port Finance Solana logic. Notional free collateral. 88mph init. Zapper arbitrary call. OpenZeppelin Timelock reentrancy. Harvest uninitialized Uniswap V3 vaults. Teller beacon. Rocket Pool / Lido deposit front-run. Cronos fees. Polygon consensus. Evmos docs. Acala halt. Axelar halt. Movement Labs chain-split (~$6.7k, still a **class**: M06). marginfi flash loan (Asymmetric). Across V3 (Zach Obront + deadrosesxyz). Fraxlend. VeChain VTHO. Story Network. Injective. Stacks DoS. Hats Velvet calldata. Thena C4-miss pair. BendDAO flash claim. Mt Pelerin replay. Alchemix access. Bitswift mint. Charged Particles grief. O3 / LayerZero Trust lows. xDai Stake. Fringe.fi. Jade first-run. riptide Balancer 50 ETH. Shabarkin Scroll $1k. Lido Dual Governance (funds not at risk). Uniswap v4 periphery OZ+Spearbit. Alchemix V3 liq-fee contest. Sherlock 4626 inflation class.

Route each of those to a playbook via `hunting-x-linked-bounties`. Nothing in the ingest paid table is "also-paid trivia" only.

Not paid / out of scope still teach: Trust Security Morpho `delegatecall`, Compound "known issue" liquidations, RAI returndata bomb, LEVEL Finance (riptide, OOS). Read them so you do not waste a report slot.

## Industry payout mix (Immunefi, 2024 public)

When Immunefi crossed $100M paid: smart contracts **~$78M (77.5%)**, critical severity **~$88M (87.8%)**. Most of the money is still **direct loss of funds** in contracts and bridges, not XSS. Source: [Immunefi / GlobeNewswire Jun 2024](https://finance.yahoo.com/news/immunefi-reports-over-100-million-130000881.html).

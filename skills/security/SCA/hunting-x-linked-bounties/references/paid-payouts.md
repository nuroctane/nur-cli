# Paid payouts -> playbook

Every public paid row from the X/Immunefi ingest, classified onto a Nur skill. **Pack taxonomy** ids are `historical-smart-contract-vulns/references/taxonomy.md` (not ingest T-ids). Ingest-label map: `ingest-class-map.md`.

Copy the **invariant** into a local test on **in-scope** code. Never broadcast.

Indexes: [sayan011 writeups](https://github.com/sayan011/Immunefi-bug-bounty-writeups-list) · [Immunefi BugFixReviews](https://github.com/immunefi-team/Web3-Security-Library/blob/main/BugFixReviews/README.md)

## By playbook (load that SKILL.md)

### `reviewing-upgradeable-proxies`

| $ | Year | Protocol | Pack | Whitehat | Writeup |
|---|------|----------|------|----------|---------|
| $10M | 2022 | Wormhole uninitialized impl | T12 | satya0x | [Immunefi](https://medium.com/immunefi/wormhole-uninitialized-proxy-bugfix-review-90250c41a43a) |
| $42k | 2021 | 88mph unprotected `init` | T12 | Ashiq Amien | [Immunefi](https://medium.com/immunefi/88mph-function-initialization-bug-fix-postmortem-c3a2282894d3) |
| (also-paid) | | Harvest uninit Uniswap V3 vaults; Teller beacon | T12 | | Immunefi library |

### `reviewing-bridges-and-messaging`

| $ | Year | Protocol | Pack | Whitehat | Writeup |
|---|------|----------|------|----------|---------|
| $6M | 2022 | Aurora infinite spend / ExitToNear | T18 / O01 | pwning.eth | [Immunefi](https://medium.com/immunefi/aurora-infinite-spend-bugfix-review-6m-payout-e635d24273d) |
| ~$2.2M | 2021 | Polygon MRC20 no balance check | T18 | Leon Spacewalker | [Immunefi](https://medium.com/immunefi/polygon-lack-of-balance-check-bugfix-postmortem-2-2m-bounty-64ec66c24c7d) |
| ~$2M | 2021 | Polygon Plasma branchMask / double-spend | T17 | Gerhard Wagner | [Immunefi](https://medium.com/immunefi/polygon-double-spend-bug-fix-postmortem-2m-bounty-5a1db09db7f1) |
| $1M | 2025 | Scroll message spoof | T19 | WhiteHatMage | [Scroll forum](https://forum.scroll.io/t/report-scroll-mainnet-emergency-upgrade-on-2025-04-25/666) |
| $200k | 2022 | Interlay vault / bridge | T19 | pwning.eth | [mirror](https://pwning.mirror.xyz/jlT8OgtwN3mQf3KdYmXdcSXbE4s95JzT3eR3wxiLmpw) |
| $50k | 2025 | Wormhole messaging (Marco) | T19 | Marco Nunes | [x](https://x.com/marcotnunes/status/1889707212450234629) confirm Immunefi |
| $50k | 2025 | Axelar cross-chain halt | C01 / T17 | Marco Nunes | [marcotnunes.com](https://marcotnunes.com/axelar-network-cross-chain-halt-vulnerability/) |
| High | 2025 | Across V3 messaging | T19 | zachobront, deadrosesxyz | [mirror](https://mirror.xyz/0x9D6b7f5e8d1b9dFea8dDD29c0DbD81687e721601/mrt70ckjaZymv9keUy_TzHVIzjBOQr-Hx_KI1ydFeoQ) |
| $5k | ~2022 | O3 bridge token steal | T18 | Trust, 0xDjango | [trust-security](https://www.trust-security.xyz/post/critical-finding-stealing-tokens-from-o3-bridge-users) |
| $5k | ~2023 | LayerZero (Trust, low) | T20 | Trust Security | [trust-security](https://www.trust-security.xyz/post/learning-by-breaking-a-layerzero-case-study-part-3) |
| $100k | 2025 | Story Network (messaging/chain) | T19 | WhiteHatMage, Jiri123 | [story.foundation](https://www.story.foundation/blog/story-network-postmortem) |

### `reviewing-l2-sequencer-and-finality`

| $ | Year | Protocol | Pack | Whitehat | Writeup |
|---|------|----------|------|----------|---------|
| $2M | 2022 | Optimism OVM SELFDESTRUCT / ETH duplication | T27 / O04 | saurik | [Immunefi](https://medium.com/immunefi/optimism-infinite-money-duplication-bugfix-review-daa6597146a0) |
| ~400 ETH | 2022 | Arbitrum delayed inbox | T28 | 0xriptide | [medium](https://medium.com/@0xriptide/hackers-in-arbitrums-inbox-ca23272641a2) |
| $200k | 2023-24 | zkSync Lite proof verification | T31 | LonelySloth | [Immunefi](https://medium.com/immunefi/zksync-insufficient-proof-verification-bugfix-review-dcd57944d0e2) |
| $200k | 2026 | zkSync Lite (Ehsan, X) | T31 | Ehsan | [x](https://x.com/Ehsan1579/status/2013482485175226811) confirm Immunefi |
| $50k | 2023 | zkSync Era soundness | T31 | ChainLight | [ChainLight](https://medium.com/chainlight/uncovering-a-zk-evm-soundness-bug-in-zksync-era-f3bc1b2a66d8) |
| $20k | 2023 | Optimism censorship / forced inclusion | T30 | iosiro | [iosiro](https://www.iosiro.com/blog/optimism-censorship-bug-disclosure) |
| $1k | 2025 | Scroll (Shabarkin, X) | T31 | Pavel Shabarkin | [x](https://x.com/shabarkin/status/1917483039195816213) |
| $50k total | ~2023 | Trust "permission denied" L1-L2 alias | T27 | Trust Security | [trust-security](https://www.trust-security.xyz/post/permission-denied) |

### `auditing-blockchain-clients`

| $ | Year | Protocol | Pack | Whitehat | Writeup |
|---|------|----------|------|----------|---------|
| $2M+$75k | 2024 | Sei 2x critical chain | O04 | usmannk | [usmannkhan.com](https://usmannkhan.com/bug%20reports/2024/06/17/sei-bug-report.html) |
| $75k | 2024 | Sei (Catchme) | O04 | Catchme | [exvul](https://exvul.com/share-the-details-sei-protocol-vulnerability-worth-75k/) |
| $1M+ | 2022 | Moonbeam / Astar / Acala Frontier trunc | O02 | pwning.eth | [mirror](https://pwning.mirror.xyz/okyEG4lahAuR81IMabYL5aUdvAsZ8cRCbYBXh8RHFuE) |
| $1M | 2022 | Polkadot Frontier EVM | O02 | pwning.eth | [mirror](https://pwning.mirror.xyz/RFNTSouIIlHVNmTNDThUVb1obIeN5c1LAiQuN9Ve-ok) |
| $150k | 2024 | Evmos docs vs consensus | O04 | jayjonah.eth | [medium](https://medium.com/@jjordanjjordan/150-000-evmos-vulnerability-through-reading-documentation-d26328590a7a) |
| $75k | ~2022 | Polygon consensus bypass | O04 | Niv Yehezkel | [Immunefi](https://medium.com/immunefi/polygon-consensus-bypass-bugfix-review-7076ce5047fe) |
| $70k | 2025 | Acala block production halt | O04 | Lastc0de | [Immunefi](https://immunefi.com/blog/all/acala-block-production-shutdown-bug-fix-review/) |
| $50k | ~2023 | Astar (Zellic) | O02 | Zellic | [zellic.io](https://www.zellic.io/blog/finding-a-critical-vulnerability-in-astar/) |
| $50k | ~2023 | Q Blockchain | O04 | Blockian | [medium](https://medium.com/@blockian/striking-gold-at-30-000-feet-uncovering-a-critical-vulnerability-in-q-blockchain-for-50-000-ab335042147b) |
| $50k | 2026 | Injective (f4lc0n, X) | O04 | f4lc0n | [x](https://x.com/al_f4lc0n/status/2033110168045568434) |
| $20k | ~2022 | Oasis platform shutdown | O04 | Trust | [trust-security](https://www.trust-security.xyz/post/taking-home-a-20k-bounty-with-oasis-platform-shutdown-vulnerability) |
| $200k | ~2024 | Oasys | O04 | merkle_bonsai | [mirror](https://mirror.xyz/0x333247F2e126954ed6428e9135Ae9dE06A76BA32/a6HqOCOjJ10Bosyi0cGz6Lxff8t68Uo4YvFsVg2tHaw) |
| $6.71k | 2025 | Movement Labs chain split | M06 | Yunus Emre | [medium](https://medium.com/@yemresaritoprak/permanent-chain-split-in-movement-full-node-anatomy-of-a-6-710-critical-vulnerability-that-fa75fe66a0c7) |

### `reviewing-amm-and-cl-pools`

| $ | Year | Protocol | Pack | Whitehat | Writeup |
|---|------|----------|------|----------|---------|
| $1M | 2023 | Balancer rounding + flashSwap | T04 / T15 | GothicShanon89238 | [Immunefi](https://medium.com/immunefi/balancer-rounding-error-bugfix-review-cbf69482ee3d) |
| $250k | 2025 | Balancer V2 (kankodu) | T15 | kankodu | [mirror](https://mirror.xyz/0x38F1416B9Ed3a5DA9C12c56cb4F74D9564844728/iv9_q74rSlK7gbvbJAECuDIbzfUtrSCO6mSWIHPskKI) |
| $505k | 2024 | Raydium tick manipulation | S09 | riproprip | [Immunefi](https://medium.com/immunefi/raydium-tick-manipulation-bugfix-review-c6aae4527ed6) |
| $100k | ~2023 | DFX EURS 2-decimal rounding | T04 | perseverance | [Immunefi](https://medium.com/immunefi/dfx-finance-rounding-error-bugfix-review-17ba5ffb4114) |
| ~$290k | 2024 | The Graph rounding | T04 | GregadETH | [Immunefi](https://medium.com/immunefi/the-graph-rounding-error-bugfix-review-c946ff470f65) |
| 50 ETH | ~2022 | Balancer (riptide) | T15 | riptide | [mirror](https://mirror.xyz/0x2719F6Dfb85086F87319079cC2f7EeFD0e40994D/NWDf5uW1Ve7-TrcPKwmM86xp8ploMSCRGC58A-NSoFY) |

### `reviewing-erc4626-and-vaults`

| $ | Year | Protocol | Pack | Whitehat | Writeup |
|---|------|----------|------|----------|---------|
| $1M+$50k | 2021 | Belt Finance logic / share | T01 / T06 | Bobface | [Immunefi](https://medium.com/immunefi/belt-finance-logic-error-bug-fix-postmortem-39308a158291) |
| $1.5M | ~2021 | ArmorFi coverage math | T01 | | [Immunefi](https://medium.com/immunefi/armorfi-bug-bounty-postmortem-cf46eb650b38) |
| $250k | ~2023 | Sherlock Yield Strategy | T01 | GothicShanon | [mirror](https://mirror.xyz/0xE400820f3D60d77a3EC8018d44366ed0d334f93C/LOZF1YBcH1eBdxlC6HP223cAMeTpNgQ-Kc4EjQuxmGA) |
| 44.8 ETH | ~2022 | Tranchess first-run (Jade) | T06 / T36 | Jade | [kalos](https://www.kalos.xyz/blog/tranchess-liquid-staking-deposit-firstrun-vulnerability-analysis) |
| $200k | ~2023 | Tranchess (Flora) | T01 | Flora | [github](https://github.com/floranguyen0/tranchess-vulnerability-disclosure) |
| contest | 2024 | Sherlock ERC-4626 inflation (Napier, Burve, Notional Exponent) | T06 | wardens | e.g. [Napier #125](https://github.com/sherlock-audit/2024-01-napier-judging/issues/125) |

### `reviewing-lending-and-liquidations`

| $ | Year | Protocol | Pack | Whitehat | Writeup |
|---|------|----------|------|----------|---------|
| $1M+ | 2022 | Notional free collateral double-count | T14 | 0x60511e57 | [Immunefi](https://medium.com/immunefi/notional-double-counting-free-collateral-bugfix-review-28b634903934) |
| ~$630k | 2022 | Port Finance (Solana) withdraw vs debt | S02 / T14 | nojob | [Immunefi](https://medium.com/immunefi/port-finance-logic-error-bugfix-review-29767aced446) |
| $100k | 2023 | Silo interest-rate | T14 | kankodu | [x](https://twitter.com/kankodu/status/1669833829203476480) |
| ~$95k | ~2023 | Yield Protocol pool-balance | T01 / T14 | Paludo0x | [Immunefi](https://medium.com/immunefi/yield-protocol-logic-error-bugfix-review-7e86741e6f50) |
| $50k | ~2022 | Sense `onSwap` access (lending-adj) | T02 | alephv.eth | [Immunefi](https://medium.com/immunefi/sense-finance-access-control-issue-bugfix-review-32e0c806b1a0) |
| $30k | 2023 | Perpetual Protocol bad debt | T14 | banditx0x | [securitybandit](https://securitybandit.com/2023/02/07/bad-debt-attack-for-perpetual-protocol/) |
| $2k | ~2022 | Fringe.fi insolvency | T14 | Trust | [trust-security](https://www.trust-security.xyz/post/diving-deep-into-a-critical-protocol-insolvency-bug-in-fringe-fi-lending-platform) |
| High | 2025 | Fraxlend | T14 | 0xjuaan, 0xSpearmint | [mirror](https://mirror.xyz/0x22ce3c4ce1EC532437209efA79d05CD294651ec3/M6vD6XshTuZc53DFm0chQwYD15fxQ29G1mbxNi9ZLwU) |
| Crit | 2025 | marginfi flash + health | S10 | Felix Wilhelm | [asymmetric.re](https://blog.asymmetric.re/threat-contained-marginfi-flash-loan-vulnerability/) |
| Crit | 2025 | Alchemix V3 liq-fee overpay | T14 | Immunefi boost | [reports.immunefi.com](https://reports.immunefi.com/alchemix-v3/58772-sc-critical-resolverepaymentfee-overpays-liquidators-when-collateral-is-gone-letting-attackers) |
| High | 2025 | Vesu rounding (Cairo) | K01 | kankodu / alexxander | [Vesu docs](https://docs.vesu.xyz/security/disclosures-report/rounding-convention-bug-disclosure) |

### `reviewing-access-control-and-auth`

| $ | Year | Protocol | Pack | Whitehat | Writeup |
|---|------|----------|------|----------|---------|
| $400k | ~2022 | Enzyme missing privilege | T02 | rootrescue | [Immunefi](https://medium.com/immunefi/enzyme-finance-missing-privilege-check-bugfix-review-ddb5e87b8058) |
| $100k | ~2022 | APWine delegations | T02 / T10 | setuid0 | [Immunefi](https://medium.com/immunefi/apwine-incorrect-check-of-delegations-bugfix-review-7e401a49c04f) |
| $50k | ~2022 | Sense access | T02 | alephv.eth | [Immunefi](https://medium.com/immunefi/sense-finance-access-control-issue-bugfix-review-32e0c806b1a0) |
| $40k | ~2022 | Cronos tx-fee theft | C05 / T02 | zb3 | [Immunefi](https://medium.com/immunefi/cronos-theft-of-transactions-fees-bugfix-review-b33f941b9570) |
| $28k | ~2023 | Alchemist admin brick / forced revert | T02 | Dacian | [dacian.me](https://dacian.me/28k-bounty-admin-brick-forced-revert) |
| $7.5k | ~2021 | Alchemix access | T02 | Ashiq | [Immunefi](https://medium.com/immunefi/alchemix-access-control-bug-fix-debrief-a13d39b9f2e0) |
| $4.5k | ~2021 | Bitswift unlimited mint | T02 | | [Immunefi](https://medium.com/immunefi/bitswift-unlimited-mint-bugfix-postmortem-147a1e57dca9) |
| $50k | ~2025 | VeChainThor VTHO accrual | T01 | nnez | [Immunefi](https://immunefi.com/blog/all/vechainthor-vtho-accrual-bypass-bug-fix-review/) |

### `reviewing-signatures-permit-and-eip712`

| $ | Year | Protocol | Pack | Whitehat | Writeup |
|---|------|----------|------|----------|---------|
| $560k | 2022 | Redacted Cartel custom approval | T23 / T10 | Tommaso Pifferi | [Immunefi](https://medium.com/immunefi/redacted-cartel-custom-approval-logic-bugfix-review-9b2d039ca2c5) |
| $10k | ~2021 | Mt Pelerin double tx / replay | T10 | | [Immunefi](https://medium.com/immunefi/mt-pelerin-double-transaction-bugfix-review-503838db3d70) |

### `reviewing-oracles-and-pricing`

| $ | Year | Protocol | Pack | Whitehat | Writeup |
|---|------|----------|------|----------|---------|
| $800k | 2021 | Fei flashloan composition | T05 / T09 | Bobface | [Immunefi](https://medium.com/immunefi/fei-protocol-flashloan-vulnerability-postmortem-7c5dc001affb) |
| $19k | ~2021 | Enzyme oracle | T05 | setuid0 | [Immunefi](https://medium.com/immunefi/enzyme-finance-price-oracle-manipulation-bug-fix-postmortem-4e1f3d4201b5) |

### `reviewing-cross-function-and-composer`

| $ | Year | Protocol | Pack | Whitehat | Writeup |
|---|------|----------|------|----------|---------|
| $25k | ~2021 | Zapper arbitrary calldata | T34 | Lucash-dev | [Immunefi](https://medium.com/immunefi/zapper-arbitrary-call-data-bug-fix-postmortem-d75a4a076ae9) |
| $5k | ~2021 | xDai Stake arbitrary call | T34 | 0xadee028d | [Immunefi](https://medium.com/immunefi/xdai-stake-arbitrary-call-method-bug-postmortem-f80a90ac56e3) |
| $40k of ~$70k | 2024 | Hats Velvet Capital calldata | T34 | 16 wardens | [Velvet report](https://github.com/Velvet-Capital/audits/blob/main/report.md) |
| $1.1M | ~2022 | Beanstalk insufficient validation | T03 | nicole | [Immunefi](https://medium.com/immunefi/beanstalk-insufficient-input-validation-bugfix-review-fc3fdbaab15b) |
| ~$182k | 2022 | Beanstalk logic error | T03 | | [Immunefi](https://medium.com/immunefi/beanstalk-logic-error-bugfix-review-4fea17478716) |
| $150k | ~2022 | Synthetix logic | T01 | thunderdeep14 | [Immunefi](https://medium.com/immunefi/synthetix-logic-error-bugfix-review-40da0ead5f4f) |
| $60k | ~2021 | Mushrooms logic | T01 | CKK Sec | [Immunefi](https://medium.com/immunefi/mushrooms-finance-logic-error-bug-fix-postmortem-780122821621) |
| $25k | ~2021 | Tidal logic | T01 | csanuragjain | [Immunefi](https://medium.com/immunefi/tidal-finance-logic-error-bug-fix-postmortem-3607d8b7ed1f) |
| $20k | 2023 | Thena gauge (C4 miss) | T01 | zzykxx | [zzykxx](https://zzykxx.com/2023/02/02/the-bug-that-codearena-missed-,-twice/) |
| $20k | 2023 | Thena second | T01 | zzykxx | [zzykxx](https://zzykxx.com/2023/02/27/a-very-helpful-sign/) |
| $50k | ~2022 | Fluidity halt / logic | T01 | Trust | [trust-security](https://www.trust-security.xyz/post/breaking-fluidity-for-glory-and-50k) |
| $25k | ~2022 | Ondo high | T01 | Ashiq / iosiro | [iosiro](https://iosiro.com/blog/high-risk-vulnerability-disclosed-to-ondo-finance) |
| $5k | ~2021 | Charged Particles griefing | T03 | janbro.eth | [Immunefi](https://medium.com/immunefi/charged-particles-griefing-bug-fix-postmortem-d2791e49a66b) |
| High | 2025 | Lido Dual Governance (funds not at risk) | T21 | 0xriptide | [lido research](https://research.lido.fi/t/security-disclosure-dg-weakness-reported-through-immunefi-funds-not-at-risk/10393) |
| High | 2026 | dHEDGE (X) | T01 | s4muraii77 | [x](https://x.com/s4muraii77/status/2012140371938070888) confirm writeup |

### `reviewing-nft-and-marketplace`

| $ | Year | Protocol | Pack | Whitehat | Writeup |
|---|------|----------|------|----------|---------|
| $50k | ~2023 | BendDAO sewer-pass flash claim | T32 | | [BendDAO](https://medium.com/@BendDAO/sewer-pass-flash-claim-vulnerability-9d2b0b1e09ef) |

### `reviewing-governance-and-timelocks`

| $ | Year | Protocol | Pack | Whitehat | Writeup |
|---|------|----------|------|----------|---------|
| (also-paid) | | OpenZeppelin Timelock reentrancy | T21 | | Immunefi / OZ advisory |
| High | 2025 | Lido Dual Governance | T21 | 0xriptide | [lido research](https://research.lido.fi/t/security-disclosure-dg-weakness-reported-through-immunefi-funds-not-at-risk/10393) |

Flash-loan governance **loss**: Beanstalk 2022 hack is `incident-timeline.md` / T22, not this paid table.

### `reviewing-solana-programs`

| $ | Year | Protocol | Pack | Whitehat | Writeup |
|---|------|----------|------|----------|---------|
| $505k | 2024 | Raydium ticks | S09 | riproprip | [Immunefi](https://medium.com/immunefi/raydium-tick-manipulation-bugfix-review-c6aae4527ed6) |
| ~$630k | 2022 | Port Finance | S02 | nojob | [Immunefi](https://medium.com/immunefi/port-finance-logic-error-bugfix-review-29767aced446) |
| Crit | 2025 | marginfi flash | S10 | Felix Wilhelm | [asymmetric.re](https://blog.asymmetric.re/threat-contained-marginfi-flash-loan-vulnerability/) |

Wormhole **hack** $326M sysvar is S05 / `solana.md`, not a bounty.

### `reviewing-move-modules`

| $ | Year | Protocol | Pack | Whitehat | Writeup |
|---|------|----------|------|----------|---------|
| $50k | 2025 | Sui network shutdown | M06 | F4lt | [Immunefi](https://immunefi.com/blog/bug-fix-reviews/sui-network-shutdown/) |
| $6.71k | 2025 | Movement chain split | M06 | Yunus Emre | [medium](https://medium.com/@yemresaritoprak/permanent-chain-split-in-movement-full-node-anatomy-of-a-6-710-critical-vulnerability-that-fa75fe66a0c7) |

Cetus overflow is a **hack** (`M07`), not a bounty. Still grep integer-mate.

### `reviewing-cosmos-and-ibc`

| $ | Year | Protocol | Pack | Whitehat | Writeup |
|---|------|----------|------|----------|---------|
| $2M+$75k | 2024 | Sei | O04 / C01 | usmannk, Catchme | usmannkhan.com + exvul |
| $150k | 2024 | Evmos | O04 | jayjonah.eth | medium above |
| $50k | 2025 | Axelar halt | C01 | Marco Nunes | marcotnunes.com |
| $50k | 2026 | Injective | O04 | f4lc0n | X above |
| $40k | ~2022 | Cronos fees | C05 | zb3 | Immunefi above |

### `reviewing-cairo-and-starknet`

| $ | Year | Protocol | Pack | Whitehat | Writeup |
|---|------|----------|------|----------|---------|
| High | 2025 | Vesu rounding | K01 | kankodu, __alexxander_ | [Vesu](https://docs.vesu.xyz/security/disclosures-report/rounding-convention-bug-disclosure) |

zkLend is a **hack** (`move-cosmos-cairo.md`).

### `reviewing-bitcoin-adjacent`

| $ | Year | Protocol | Pack | Whitehat | Writeup |
|---|------|----------|------|----------|---------|
| ~$76k | ~2024 | Stacks Clarity DoS | B03 | Catchme | [Immunefi](https://medium.com/immunefi/stacks-dos-bugfix-review-dc0f2a75b276) |
| $15k | 2024 | Sovryn (Rootstock) | B01 / T01 | gandu_whitehat | [x](https://x.com/gandu_whitehat/status/1803794103248806223) |

### `reviewing-mev-ordering-and-slippage`

| Note | Protocol | Pack | Writeup |
|------|----------|------|---------|
| Deposit front-run class | Rocket Pool / Lido | T25 | Immunefi library (also-paid cluster) |
| Tick compression / unsubscribe | Uniswap v4 periphery (Spearbit 4 Medium) | T15 / T25 | Spearbit v4-periphery draft |
| 1 Crit + 1 High resolved | Uniswap v4 periphery + Universal Router (OZ 2024) | T34 / T15 | [OpenZeppelin](https://www.openzeppelin.com/news/uniswap-v4-periphery-and-universal-router-audit) |

### `contest-and-bounty-reporting` (venue, not a class)

| Note | Protocol | Writeup |
|------|----------|---------|
| Hats paid ~$40k of ~$70k | Velvet Capital 2024 | GitHub report above |
| Hats venue; later **hacked** | Raft 2023 | [rekt](https://rekt.news/raft-rekt) |
| Immunefi 93% of post-launch crits | platform research | [Immunefi](https://immunefi.com/blog/research/93-of-critical-crypto-vulns-are-disclosed-on-immunefi/) |

## Unverified / do not cite as paid

GregoAI 2026 X claims (Gnosis, Yearn, Reserve, Balancer, Uniswap): tweet-only, amounts unstated. Usual $16M / Uniswap v4 $15.5M / LayerZero $15M are **program maxima**. NEAR HackenProof ~$1.8M is often web/app mixed.

## Next

Playbook SKILL.md -> local test -> `contest-and-bounty-reporting`. Deep cards: `historical-smart-contract-vulns/references/case-cards.md`.

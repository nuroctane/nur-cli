# Named public losses (Rekt-scale and cousins)

Loss-side rows from public Rekt / Immunefi / DeFiHackLabs citations. **Not** paid bounties (`notable-bounties.md`). Amounts as the cited article stated.

Use for analog search. Reconstruct only via `reconstructing-public-postmortems`. Never replay against live funds.

Custodial / key theft is still listed so you do **not** hunt a Solidity ghost (Bybit, Radiant II malware, Slope mnemonic, ZKasino ops). Full zachxbt + tayvano_ catalog: `investigator-ops.md`.

The **Class** column in this table uses ingest labels (T28 keys, T42 sysvar, etc.). Map to **this pack's** `taxonomy.md` ids before filing. Example: Wormhole $326M is **S05**, not T12 (T12 is the $10M uninitialized-proxy *bounty*).

Source ingest: `skills/security/SCA/sc-research/library/_ingest/grok-x-bounties.md` (optional raw notes; not loaded by default).

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


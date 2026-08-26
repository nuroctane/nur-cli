# Incident timeline (public losses and postmortems)

This is the **loss** side. Paid preventions are `notable-bounties.md`. Named DeFiHackLabs rows (858) are `incident-corpus.md`.

Amounts are what public trackers stated. Custodial / key theft is labelled **ops** so you do not hunt a Solidity ghost.

Reconstruct only with `reconstructing-public-postmortems`. Never replay against live funds.

## How the mix moved (Immunefi scoreboard)

| Era | What dominated losses | Review implication |
|-----|----------------------|--------------------|
| 2016-2019 | Reentrancy, parity wallet, integer overflow (pre-0.8) | Still grep T07, T12; compilers changed, patterns did not vanish |
| 2020 | Flash-loan oracle (bZx, Harvest) | T05/T09 - now <1% of 2025 losses, still a contest finding |
| 2021 | Forked lending, mint bugs, Poly Network | T14, T02, T18 |
| 2022 | **Bridges** (~73% of DeFi losses): Ronin, BNB, Wormhole, Nomad, Harmony, Qubit | T17-T20, S05 |
| 2023 | Vyper/Curve, Euler, Mango (Solana gov), Multichain ops | T35, T14, S10, ops |
| 2024 | Smaller median; protocol logic; some zk/L2 bounties | T04, T31, T33 |
| 2025 | ~89% protocol logic inside DeFi; custodial Bybit $1.5B **outside** DeFi-protocol bucket; Balancer V2 ~$128M | T04, T33, T15 |
| 2026 H1 | Messaging **config** (KelpDAO LZ single DVN ~$292M), Solana **ops+oracle** (Drift ~$285M), 4337/7702/CCTP titles in DeFiHackLabs | T20, S10, T39, T40, T17 |

Primary: [Immunefi scoreboard](https://immunefi.com/blog/research/the-ecosystem-vulnerability-scoreboard-6-years-of-defi-loss-data/).

## 2016-2019 (formation)

| Year | Incident | Chain | Class | Notes / source |
|------|----------|-------|-------|----------------|
| 2016 | The DAO | Ethereum | T07 | Reentrancy. SWC-107 textbook |
| 2017 | Parity wallet freeze / library suicide | Ethereum | T12 | `selfdestruct` library |
| 2018 | BeautyChain overflow | Ethereum | arithmetic | pre-0.8 |
| 2018 | BatchOverflow family | Ethereum | arithmetic | |
| 2019 | Binance DEX / early exchange | mixed | ops + contract | |

## 2020-2021

| Year | Incident | Chain | Class | Stated loss | Source |
|------|----------|-------|-------|-------------|--------|
| 2020 | bZx (multiple) | Ethereum | T05/T09 | tens of M | Rekt / PeckShield era posts |
| 2020 | Harvest | Ethereum | T05 | ~$34M | [Rekt](https://rekt.news) Harvest |
| 2020 | Value DeFi | Ethereum | T05 | | |
| 2021 | Uranium / Cream / Spartan | BSC / ETH | T01/T14 | | DeFiHackLabs |
| 2021 | Poly Network | multi | T18/T02 | ~$600M (mostly returned) | public IR |
| 2021 | Cream (multiple) | Ethereum | T14/T09 | | |
| 2021 | Compound liquidation (DAI) | Ethereum | T14 | | market + code |
| 2021 | AnubisDAO / other rug-adj | Ethereum | ops | | not a class to "audit in" |

## 2022 - bridge year

| Incident | Chain | Class | Stated loss | What was wrongly trusted |
|----------|-------|-------|-------------|--------------------------|
| Wormhole / Portal | Solana + ETH | S05 sysvar spoof | ~$326M | `load_instruction_at` / sysvar account was not the real instructions sysvar |
| Ronin | Ronin / ETH | T20 validator set | ~$624M | 5-of-9 validator compromise (ops + design) |
| BNB Bridge | BSC | C02/T18 IAVL | ~$570M | forged proof / mint |
| Nomad | multi | T17 | ~$190M | uninitialized / trusted root `0x00` replica |
| Harmony Horizon | Harmony | T20 | ~$100M | 2-of-5 multisig |
| Qubit | BSC | T18 | ~$80M | `deposit` on ETH path minted without lock |
| Beanstalk | Ethereum | T22 | ~$80M+ | flashloan governance |
| Mango | Solana | S10 | ~$110M | oracle + thin market + gov |
| Cashio | Solana | S02 | ~$48M | fake collateral account |
| Wintermute | Ethereum | ops | ~$160M | Profanity vanity key (not a protocol AMM bug) |

Nomad, Wormhole, Ronin, BNB, Harmony, Qubit are the **six names** to memorize for T17-T20. Immunefi: nine 2022 bridge events summed ~$1.9B.

## 2023

| Incident | Chain | Class | Stated loss | Notes |
|----------|-------|-------|-------------|-------|
| Euler | Ethereum | T14 | ~$197M (mostly recovered) | donate / liquidation logic |
| Curve / Vyper | Ethereum | T35 | ~$70M+ | reentrancy in specific compiler versions |
| Multichain | multi | ops | ~$100M+ | MPC / key, not a Solidity callback |
| Bonq | Polygon | T05 | | oracle |
| Platypus | Avalanche | T14 | | |
| KyberSwap Elastic | multi | T15 | ~$50M | CL tick / compute |
| Exactly / Sturdy / Hundred / Geist / Lodestar | various | T14 | | Compound-fork liquidation families |
| Socket / other approvals | EVM | T34 | | infinite approve aggregators |
| Mixin / others | mixed | ops | | |

## 2024

| Incident | Chain | Class | Notes |
|----------|-------|-------|-------|
| Radiant (multiple) | Arbitrum / BSC | T14 / ops | lending + key |
| Sonne | OP | T14 | empty-market / donation-style |
| Prisma | Ethereum | T07 | | 
| Munchables / Blast-adj | Blast | ops + contract | |
| DMM Bitcoin | Bitcoin exchange | ops | Immunefi: **not** a Bitcoin Script bug |
| WazirX | ETH Safe / Liminal | OPS01 | ~$235M display/multisig class; zachxbt Lazarus markings. `investigator-ops.md` |
| Many 2024 DeFiHackLabs rows | mixed | T33 | logic; 189 public titles in corpus |

## 2025

| Incident | Chain | Class | Stated loss | Notes |
|----------|-------|-------|-------------|-------|
| Bybit | ETH wallets / Safe | **OPS01/18** | ~$1.5B | Safe{Wallet} JS / UI lie, not ETH consensus. zachxbt Lazarus hours later; FBI confirmed. `investigator-ops.md` |
| Balancer V2 composable stables | ETH, Arb, Base, Polygon, Sonic, OP | T04/T15 | ~$128M | multi-chain same pool class |
| Cetus | Sui | M07 | ~$223M | integer-mate overflow (rekt) |
| (others) | mixed | T33 | Immunefi DeFi protocol bucket **$680M** | 89% logic |

## 2026 (through ~August; public trackers disagree on totals)

| Incident | Chain | Class | Stated loss | Notes |
|----------|-------|-------|-------------|-------|
| KelpDAO rsETH / LayerZero | ETH + LZ | T20 | ~$292M | **single DVN / verifier**. Aave froze rsETH markets (contagion). Immunefi scoreboard |
| Drift | Solana | S10 / OPS15 | ~$285M | protocol/ops+oracle **and** Circle CCTP ~$232M unfrozen ~6h. Not "forgot `Signer`". `investigator-ops.md` |
| Circle Files | USDC issuer | OPS15 | ~$420M alleged freeze lag / 15 cases | zachxbt; also froze 16 legitimate wallets ~Mar 2026 |
| USMS seized-crypto | custody | OPS16 | ~$46M | contractor insider; FBI arrest. Not a token bug |
| Step Finance | Solana | mixed / ops | ~$27M | Q1; phishing/treasury in some writeups |
| Resolv / Truebit | EVM | T05/T03 | $25M+ each (altfins table) | confirm on Rekt/IR before citing |
| Hyperbridge | mixed | T18 | ~$12M | mint function |
| Verus-ETH bridge | mixed | T17 | ~$11.5M | weak source-tx verification |
| TAC | TON/EVM | O03 | ~$2.8M | cross-chain path |
| June 14 multi-protocol | mixed | T17 | ~$127M (some trackers) | signature replay / missing chain nonce - **verify** before repeating |
| Coinsbuy | TRON/ETH | mixed | ~$8M | exchange |
| DeFiHackLabs 2026 titles | mixed | T39, T40, T17, T12 | n/a (many whitehat reproductions) | LayerZero delegate, CCTP attestation, EIP-7702, ERC-4337 paymaster, Aztec escape hatch / `proof_id`, uninitialized proxy Renegade, SandboxOFT, Allbridge CCTP, TermFinance, ArrakisGUNI |

H1 2026 industry: Immunefi ~**$972M / 207 incidents**; on-platform **$13.45M** for 837 valid reports. PeckShield cited ~$328M across eight 2026 bridge-related events. Trackers disagree; prefer Immunefi + official postmortems for reports.

## Pattern -> playbook (after a named incident)

1. Name the taxonomy id.
2. Open the VM file.
3. Grep in-scope code for the **same wrongly-trusted thing**.
4. Local test. Do not replay the historical tx from a hot wallet.

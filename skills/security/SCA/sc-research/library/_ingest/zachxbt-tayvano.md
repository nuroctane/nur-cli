# ZachXBT + tayvano / Fire: public ops ingest (whitehat)

NurCLI `sc-research` library ingest. **Lawful / already-public only.** No exploit PoCs, no drain scripts, no unpublished 0-days, no step-by-step attacks against live systems.

**Purpose:** keep in-scope auditors from hunting a missing `onlyOwner` when the loss was a **drainer, stolen key, fake Safe UI, Telegram malware, or a rug**. These two investigators mostly cover **ops**. When they covered a real protocol/smart-contract bug, this file says so and names the class.

**Handles (as of 2026-08):**
- ZachXBT: [x.com/zachxbt](https://x.com/zachxbt) · Telegram [t.me/investigations](https://t.me/investigations) · [investigation.io](https://investigation.io/dprk-itw-breach/) · Wikipedia [ZachXBT](https://en.wikipedia.org/wiki/ZachXBT)
- Taylor Monahan: historical `@tayvano`; current X is **[@tayvano_](https://x.com/tayvano_)** (underscore). GitHub [github.com/tayvano](https://github.com/tayvano). MyCrypto Medium [medium.com/@tayvano](https://medium.com/@tayvano). MetaMask Security Monthly (Consensys). Fire.xyz as a dedicated public blog was **not found** in this pass (see Gaps).

**Amounts:** USD as published at the time. Prices move. Order-of-magnitude only.

**Dedup:** one named incident = one row. A later, different bug in the same protocol is a second row. A fund-flow follow-up of the same theft is not a new incident.

**Coverage bar:** a row is included if (a) ZachXBT or tayvano published a primary thread/Telegram/MetaMask/MyCrypto writeup, or (b) a reputable outlet explicitly attributes the public coverage to them, plus a confirming article. Rows marked `coverage: tracing` mean they tracked stolen funds / attributed an actor; the **original root cause** may still be a contract class (named in the ops-vs-contract column).

---

## 1. How to use (ops vs protocol)

Agents must not open the lending pool looking for a missing `onlyOwner` when the users signed Permit2 on a fake airdrop page.

| If the public story looks like… | It is almost certainly… | Do **not** treat as… | In-scope product review seed |
|--------------------------------|-------------------------|----------------------|------------------------------|
| Exchange "hack", hot wallet emptied, Lazarus/DPRK named | **ops**: stolen operator keys, malware on signers, fake Safe UI | Ethereum/Bitcoin consensus bug; AMM math; missing `nonReentrant` | Hardware-wallet **clear signing**; Safe transaction simulation; threshold / geographic diversity of signers; never sign what the dapp HTML shows without decoding calldata |
| Users connected wallet, signed "login", tokens gone | **ops**: drainer (Permit / Permit2 / `setApprovalForAll` / ETH transfer) | Uniswap v3 pool bug; Permit2 **contract** 0-day | Decode EIP-712: spender, token, amount, deadline. SIWE login is plaintext. Typed data with token fields is an authorization |
| NFT "animation" / "reveal" / "claim" site | **ops**: phishing / `setApprovalForAll` | ERC-721 standard bug | Marketplace `isApprovedForAll`; listing signatures; domain allowlists |
| Team member downloaded a "job interview" test | **ops**: Contagious Interview / DPRK malware | Protocol oracle bug | Separate signing laptops; never run recruiter zips; treat deployer keys as production secrets |
| Influencer token / NFT goes to zero after promo | **ops**: rug / paid shill / insider dump | "Honeypot Solidity" unless source shows a hidden mint/blacklist | Vesting, mint authority, LP lock, KOL disclosure. If there is a hidden mint, **then** it is a contract class |
| Address looks like yours, you pasted it | **ops**: address poisoning / clipboard malware | Token contract decimal bug | Explorer hide zero-value transfers; wallet address book; never copy from recent ERC-20 transfer list |
| WalletConnect QR / "verify wallet" popup on a news site | **ops**: malicious WC session + signature | WalletConnect protocol crypto break | Session origin, method allowlist, disconnect unused sessions |
| EIP-7702 "upgrade wallet" / CrimeEnjoyor | **ops**: user signed type-4 delegation **or** key was already stolen | Pectra consensus bug (tayvano: **private-key problem, not Pectra**) | Decode 7702 authorization tuple: delegate address, chainId. Reject `chainId=0` unless intended |
| Bridge drained after 5-of-9 keys stolen | **ops** (Harmony Horizon, Ronin): validator/guardian keys | Light-client proof forgery unless a postmortem names the verifier | Threshold, key ceremony, HSM, social-engineering resistance of operators |
| Radiant Jan 2024 ~$4.5M | **contract**: lending math (separate from Oct 2024) | The October Safe-malware theft | Rounding / empty-market / `transferFrom` path in **that** incident's postmortem only |
| Radiant Oct 2024 / WazirX / Bybit Safe UI | **ops**: malware showed a benign tx; signers authorized a malicious one | Aave-style insolvency; Safe singleton bytecode 0-day | Review **what the hardware wallet screen shows** vs Safe Transaction Service payload; `DELEGATECALL` to attacker impl is a **signed** ops fail, not a missing modifier in the lending market |

**Bybit is not an Ethereum consensus bug.** Signers authorized a transaction that replaced Safe logic via `DELEGATECALL`. The EVM did what the signed payload said.

**Permit2 the Uniswap contract is not the vulnerability in drainer campaigns.** Victims signed `PermitSingle` / `PermitBatch` to an attacker spender. Review the **wallet UI and frontend**, not Uniswap's Permit2 bytecode, unless the in-scope repo **is** a wallet or a Permit2 integrator.

---

## 2. ZachXBT-covered incidents (named, verified)

Columns: name, year, chain, ops vs contract, what was wrongly trusted, review seed, sources.

### 2.1 Lazarus / DPRK thefts and attributions (keys, malware, Safe UI)

| Name | Year | Chain | Ops vs contract | What was wrongly trusted | Review seed | Sources |
|------|------|-------|-----------------|--------------------------|-------------|---------|
| Bybit ETH cold/ops wallet | 2025 | Ethereum | **ops** (Safe UI / signer malware family; FBI: Lazarus). Not a consensus bug | Safe frontend / Transaction Service showed a benign transfer; signers authorized a malicious payload that `DELEGATECALL`ed attacker logic | Do not hunt ETH protocol bugs. Review clear-signing of Safe hashes on the hardware device; treat Bybit as an **operator signing** incident | [Wikipedia ZachXBT](https://en.wikipedia.org/wiki/ZachXBT); [The Block Arkham/Lazarus](https://www.theblock.co/post/342769/north-koreas-lazarus-group-responsible-for-bybit-hack-resulting-in-losses-of-over-1-5-billion-arkham); [Decrypt](https://decrypt.co/307304/north-korea-lazarus-group-bybit-hack-arkham); [FBI via BleepingComputer](https://www.bleepingcomputer.com/news/security/fbi-confirms-lazarus-hackers-were-behind-15b-bybit-crypto-heist/); [SlowMist](https://slowmist.medium.com/slowmist-hacker-techniques-and-questions-behind-bybits-nearly-1-5-billion-theft-09f0b59da2e2) |
| WazirX | 2024 | Ethereum (Safe) | **ops** (Safe / `DELEGATECALL` impl swap; ZachXBT Arkham bounty on cash-out KYC; Lazarus suspected) | Multisig owners signed a tx that overwrote Safe storage / logic | Same family as Bybit/Radiant: signed payload != displayed intent | [The Bit Times / Arkham bounty](https://thebittimes.com/crypto-sleuth-zachxbt-wins-second-arkham-bounty-after-wazirx-hack-tbt97121.html); [SlowMist Bybit note linking WazirX](https://slowmist.medium.com/slowmist-hacker-techniques-and-questions-behind-bybits-nearly-1-5-billion-theft-09f0b59da2e2) |
| Radiant Capital (Oct) | 2024 | Arbitrum, BSC (also staged on Base/ETH) | **ops** (INLETDRIFT / Safe UI deception; Mandiant: UNC4736 / Citrine Sleet / DPRK). **Not** the Jan 2024 lending-math bug | Hardware wallets + Safe UI + Tenderly sim while malware swapped the tx | Review **device integrity** of signers; Telegram ZIP/PDF lures; do not re-audit the RDNT market for this loss | [Radiant post-mortem](https://medium.com/@RadiantCapital/radiant-post-mortem-fecd6cd38081); [Mandiant update](https://medium.com/@RadiantCapital/radiant-capital-incident-update-e56d8c23829e); [CoinDesk](https://www.coindesk.com/tech/2024/10/16/radiant-capital-loses-50m-to-blockchain-exploit); [BleepingComputer](https://www.bleepingcomputer.com/news/security/radiant-links-50-million-crypto-heist-to-north-korean-hackers/) |
| Radiant Capital (Jan) | 2024 | BNB Chain | **contract** (lending / `transferFrom` / rounding; separate incident) | Protocol math / market config, not Safe malware | If in-scope **is** Radiant v2 markets, review that postmortem's class; do not conflate with Oct ops | [CoinDesk (notes prior $4.5M)](https://www.coindesk.com/tech/2024/10/16/radiant-capital-loses-50m-to-blockchain-exploit) |
| Atomic Wallet | 2023 | Multi (BTC, ETH, others) | **ops** (client/key compromise; FBI: Lazarus). Not a Bitcoin script bug | Wallet software / update / server-side key material (public details incomplete) | Review wallet update signing, server-held secrets, phishing-refund follow-on | [The Register](https://www.theregister.com/security/2023/06/05/miscreants-pick-the-crypto-pockets-of-atomic-wallet-users/927105); [247wallst / ZachXBT tally](https://247wallst.com/investing/2023/06/05/atomic-wallet-hack-affected-1-of-users-up-to-50m-estimated-to-be-drained/); [FBI Stake release naming Atomic](https://www.fbi.gov/news/press-releases/fbi-identifies-lazarus-group-cyber-actors-as-responsible-for-theft-of-41-million-from-stakecom) |
| Stake.com | 2023 | ETH, BSC, Polygon | **ops** (hot-wallet keys; FBI: Lazarus). ZachXBT first public size + attribution lean | Hot-wallet private keys | CEX/casino key ceremony; not a Stake token contract 0-day | [FBI](https://www.fbi.gov/news/press-releases/fbi-identifies-lazarus-group-cyber-actors-as-responsible-for-theft-of-41-million-from-stakecom); [SecurityWeek citing ZachXBT](https://www.securityweek.com/fbi-blames-north-korean-hackers-for-41-million-stake-com-heist/); [TRM](https://www.trmlabs.com/resources/blog/fbi-confirms-that-north-korea-was-behind-41-million-stake-com-exploit) |
| Alphapo | 2023 | Multi | **ops** (payment-processor keys; FBI: Lazarus) | Processor hot keys | Same as other 2023 TraderTraitor CEX/processor hits | [FBI Stake release](https://www.fbi.gov/news/press-releases/fbi-identifies-lazarus-group-cyber-actors-as-responsible-for-theft-of-41-million-from-stakecom); [Elliptic Lazarus tactics](https://www.elliptic.co/insights/how-the-lazarus-group-is-stepping-up-crypto-hacks-and-changing-its-tactics/) |
| CoinsPaid (Jul 2023) | 2023 | Multi | **ops** (processor keys; FBI: Lazarus) | Processor hot keys | Same cluster as Alphapo | [FBI](https://www.fbi.gov/news/press-releases/fbi-identifies-lazarus-group-cyber-actors-as-responsible-for-theft-of-41-million-from-stakecom); [Elliptic](https://www.elliptic.co/insights/how-the-lazarus-group-is-stepping-up-crypto-hacks-and-changing-its-tactics/) |
| CoinsPaid (Jan 2024 second incident) | 2024 | EVM | **ops** (hot-wallet outflows ~$6.1M; Hypedrop withdrawals stalled). Distinct from Jul 2023 Lazarus hit | Processor hot keys again | Same: payment-processor key ceremony | [t.me/s/investigations?before=90](https://t.me/s/investigations?before=90) (6 Jan 2024) |
| CoinEx | 2023 | ETH, TRON, BSC, BTC, others | **ops** (hot-wallet keys; ZachXBT live-traced to Stake Lazarus cluster via 0x75) | Exchange hot keys | Do not treat as a CoinEx listing-token bug | [t.me/s/investigations?before=54](https://t.me/s/investigations?before=54) (CoinEX hot-wallet drain + NK update); [AVOID.NET CoinEx (cites ZachXBT)](https://avoid.net/coinex); [Elliptic](https://www.elliptic.co/insights/how-the-lazarus-group-is-stepping-up-crypto-hacks-and-changing-its-tactics/) |
| DMM Bitcoin | 2024 | Bitcoin | **ops** (exchange keys; ZachXBT tracked laundering to Huione; Lazarus suspected) | Exchange BTC keys / ops | Not a Bitcoin consensus bug; review custodian key ceremony | [Web3 Universe / ZachXBT Huione](https://web3universe.today/lazarus-moves-millions-from-305m-dmm-bitcoin-hack-zachxbt/); SlowMist Bybit note lists DMM as related Gnosis/Safe-adjacent ops |
| Harmony Horizon bridge | 2022 | Harmony, Ethereum | **ops** (multisig keys of bridge operators; FBI: Lazarus). Light client was not the published root cause | 2-of-5 (or similar) guardian keys | Review **operator key** threshold; do not start at IAVL proofs unless a later paper says so | [FBI](https://www.fbi.gov/news/press-releases/fbi-confirms-lazarus-group-cyber-actors-responsible-for-harmonys-horizon-bridge-currency-theft); [Elliptic](https://www.elliptic.co/insights/the-100-million-horizon-hack-following-the-trail-through-tornado-cash-to-north-korea/); [Bitcoin.com on ZachXBT Railgun move](https://news.bitcoin.com/onchain-researchers-discover-63m-in-ethereum-from-harmony-bridge-attack-moved-hackers-attempt-to-launder-funds-on-major-exchanges/) |
| Harmony later incident (ZachXBT refused to trace) | 2023 | Harmony | Mixed: team described unauthorized issuance / emergency patch (likely **contract** mint). ZachXBT publicly declined because 2022 helpers were unpaid | Protocol mint / bridge pause (confirm class from Harmony postmortem, not from his refusal thread) | Do not skip a Harmony mint review if **in-scope is Harmony**; do not treat his "I will not track this" as a root-cause class | [NullTX](https://nulltx.com/the-100m-good-job-why-zachxbt-refuses-to-save-harmony-a-second-time); [Leviathan News](https://leviathannews.xyz/291223/harmonyprotocol-i-will-not-be-tracking-this-incident-and-think-no-one-should-assist-them-for-free-harmony-took-advantage-of-people-who-assisted-during-the-100m-harmony-bridge-e) |
| Lazarus $200M fiat-offramp cluster (25+ hacks, 2020-2023) | 2024 (report) | Multi | **ops** (laundering writeup, not a new protocol 0-day) | Mixers, CEX KYC, nested services | Tracing playbook; freeze lists | [x.com/zachxbt/status/1784935501935390930](https://x.com/zachxbt/status/1784935501935390930); [t.me/s/investigations?before=130](https://t.me/s/investigations?before=130) |
| Ronin (Sky Mavis) | 2022 | Ronin, Ethereum | **ops** (validator keys; Lazarus). ZachXBT coverage is mainly cluster/laundering, not the validator compromise writeup | 5-of-9 validator keys after social engineering of Sky Mavis / Axie DAO | Same: operator keys, not NFT marketplace Solidity | FBI Harmony release cross-cites Ronin; Elliptic Horizon article |
| Mixin Network | 2023 | Multi (custodial kernel) | **ops** (cloud-provider **database** / key material). Not Mixin VM bytecode | Cloud DB holding deposit keys | Review cloud IAM, secrets in third-party DBs; do not hunt a Mixin kernel opcode | [t.me/s/investigations?before=54](https://t.me/s/investigations?before=54) (ZachXBT: Mixin announced $200M ETH/BTC/USDT, h/t SlowMist); [The Block](https://www.theblock.co/post/252698/mixin-network-suspends-services-after-hack-involving-200-million-in-funds); [Elliptic](https://www.elliptic.co/insights/mixin-network-hacked-for-200-million/); [SlowMist via ForkLog](https://forklog.com/en/mixin-network-hacked-for-200-million/) |
| Poloniex | 2023 | Multi | **ops** (hot-wallet keys; widely Lazarus-attributed). ZachXBT tracked related Justin Sun venue cluster in public discourse | Hot keys | Custodian keys | [Halborn](https://www.halborn.com/blog/post/explained-the-poloniex-hack-november-2023); [CoinDesk](https://www.coindesk.com/business/2023/11/10/poloniex-hot-wallets-hacked-65m-seemingly-stolen-on-chain-data) |
| HTX / Huobi (2023 hot wallet) | 2023 | Multi | **ops** (hot wallet). ZachXBT later commented UK sanctions / taint of innocent wallets | Exchange hot keys | Same | [SCMP](https://www.scmp.com/tech/big-tech/article/3242535/crypto-exchange-htx-suffers-us30-million-hack-after-another-platform-linked-entrepreneur-justin-sun); [Yahoo / ZachXBT on HTX sanctions](https://finance.yahoo.com/markets/crypto/articles/zachxbt-says-uk-htx-sanctions-151348797.html) |
| HTX + Heco chain | 2023 | Heco, ETH | Mixed: Heco **bridge/chain control** after related HTX incident; confirm class before labeling Solidity | Trusted chain operator / bridge keys | If in-scope is Heco bridge, review validator set; do not assume Uniswap-style reentrancy | [CNBC](https://www.cnbc.com/2023/11/23/htx-heco-chain-crypto-hack-115-million-stolen-so-far.html) |
| BitoPro | 2025 | Multi | **ops** (AWS tokens / malware during hot-wallet update; Lazarus). MetaMask report cites ZachXBT on related Tron laundering | Cloud tokens + hot-wallet update window | Review AWS key scope during wallet migrations | [MetaMask June 2025](https://metamask.io/news/metamask-security-report-june-2025); [BleepingComputer](https://www.bleepingcomputer.com/news/security/bitopro-exchange-links-lazarus-hackers-to-11-million-crypto-heist/) |
| AscendEX (2021 hack) | 2021 | Multi | **ops** (hot wallets; reported Lazarus). ZachXBT 2026 coverage is **solvency / withdrawal halt**, not the 2021 root cause | Exchange hot keys (2021); later, users trusted a CEX that still accepted deposits while withdrawals failed | 2026 review: proof-of-reserves vs hot-wallet tags, not a 2021 bytecode bug | [CoinDesk 2021 hack](https://www.coindesk.com/business/2021/12/13/crypto-exchange-ascendex-hacked-losses-estimated-at-77m); [t.me/investigations/346](https://t.me/investigations/346) |
| DPRK fake IT workers / US company infiltration | 2024-2025 | Off-chain + stablecoins | **ops** | Fake identities / remote-hire payroll in crypto | HR identity, not a DEX pool | [MetaMask June 2025](https://metamask.io/news/metamask-security-report-june-2025); [investigation.io DPRK ITW](https://investigation.io/dprk-itw-breach/) |
| DPRK Tron OTC laundering (Bybit/WazirX proceeds) | 2025 | Tron | **ops** (laundering, not a TRON VM bug) | Tron P2P / swap pools / small OTC | Sanctions screening; not a JustLend math hunt | [MetaMask June 2025 citing ZachXBT](https://metamask.io/news/metamask-security-report-june-2025); [CCN](https://www.ccn.com/news/crypto/lazarus-group-small-otc-brokers-launder-bybit-wazirx-funds/) |
| Huione Guarantee (DMM proceeds) | 2024 | Tron / multi | **ops** (cash-out marketplace) | OTC marketplace | Compliance, not a contract class | [ZachXBT via Web3 Universe](https://web3universe.today/lazarus-moves-millions-from-305m-dmm-bitcoin-hack-zachxbt/) |
| Sinbad mixer (CoinEx etc.) | 2023 | Bitcoin | **ops** (mixer; OFAC) | Mixing service | Not a Bitcoin script 0-day | [AVOID.NET CoinEx](https://avoid.net/coinex) |
| Railgun (Harmony proceeds) | 2023 | Ethereum | **ops** (privacy pool used for laundering) | Privacy protocol as tumbler | Do not treat Railgun as the Harmony root cause | [FBI Harmony](https://www.fbi.gov/news/press-releases/fbi-confirms-lazarus-group-cyber-actors-responsible-for-harmonys-horizon-bridge-currency-theft); ZachXBT quoted on 41k ETH move |
| Tornado Cash (Harmony / many) | 2022+ | Ethereum | **ops** (mixer) | Mixer | Tracing, not a Tornado circuit bug | Elliptic Horizon; FBI |
| Kelp DAO LayerZero bridge | 2026 | Multi | **ops** (compromised infra; Lazarus alleged). ZachXBT later tied fund overlap with Humanity Protocol | Bridge operator infra, not a generic OFT math claim until a postmortem says so | [t.me/investigations/347](https://t.me/investigations/347); [Chainalysis Kelp](https://www.chainalysis.com/blog/kelpdao-bridge-exploit-april-2026/) |
| Humanity Protocol team/deployer | 2026 | EVM | **ops** (developer device / laptop). ZachXBT: overlap with Kelp proceeds argues against a simple insider dump | One laptop held multisig | [t.me/investigations/347](https://t.me/investigations/347); [CoinDesk](https://www.coindesk.com/tech/2026/06/09/humanity-s-usd36-million-exploit-happened-because-a-multisig-wallet-lived-on-one-laptop) |

### 2.2 Contagious Interview / fake-job malware (named victims ZachXBT clustered)

All **ops**. Wrongly trusted: recruiter ZIP / VS Code / "coding test". Review seed: deployer and admin keys must not live on the same OS image that opens Telegram job files. Sources: [t.me/investigations/167](https://t.me/investigations/167) (Tapioca + cluster); Palo Alto [Unit 42](https://unit42.paloaltonetworks.com/two-campaigns-by-north-korea-bad-actors-target-job-hunters/).

| Name | Year | Chain | Ops vs contract | What was wrongly trusted | Review seed | Sources |
|------|------|-------|-----------------|--------------------------|-------------|---------|
| Tapioca DAO | 2024 | Arbitrum | **ops** (malware on team member) | Fake job / malware | Not a Tapioca AMM invariant until proven | [t.me/investigations/167](https://t.me/investigations/167) |
| Nexera | 2024 | EVM | **ops** (same cluster) | Fake job | Same | Telegram cluster (ZachXBT cites prior coverage) |
| Concentric | 2024 | EVM | **ops** | Fake job | Same | Telegram cluster |
| Masa | 2024 | EVM | **ops**; ZachXBT: undisclosed for weeks | Fake job / deployer outflows | Disclose deployer drains; don't call it a "market dump" | [t.me/investigations/168](https://t.me/investigations/168) |
| SpaceCatch | 2024 | EVM | **ops** | Fake job | Same | Telegram cluster |
| Reach | 2024 | EVM | **ops** | Fake job | Same | Telegram cluster |
| Serenity Shield | 2024 | EVM | **ops** | Fake job | Same | Telegram cluster |
| MurAll | 2024 | EVM | **ops** | Fake job | Same | Telegram cluster |

### 2.3 Social engineering of individuals (SIM, fake support, remote desktop, home invasion)

| Name | Year | Chain | Ops vs contract | What was wrongly trusted | Review seed | Sources |
|------|------|-------|-----------------|--------------------------|-------------|---------|
| Genesis creditor $243M (Malone Lam / Veer Chetal / Jeandiel Serrano / later Danish Zulfiqar) | 2024 | Bitcoin (Gemini/Genesis creditor) | **ops** (fake Google + Gemini support; remote desktop; Bitcoin Core keys) | Phone/2FA reset + AnyDesk-class remote access | Not a Bitcoin Core consensus bug. Review: never install remote desktop for "support"; exchange 2FA reset process | [Wired](https://www.wired.com/story/meet-zachxbt-243-million-crypto-theft/); [NYT Magazine](https://www.nytimes.com/2025/04/24/magazine/crybercrime-crypto-minecraft.html); [The Block](https://www.theblock.co/post/317349/blockchain-sleuth-zachxbt-alleges-three-perps-involved-in-243-million-theft-of-single-genesis-creditor); [CoinDesk arrests](https://www.coindesk.com/business/2024/09/19/police-arrests-two-people-related-to-243m-crypto-heist-targeting-genesis-creditor); [Krebs](https://krebsonsecurity.com/2024/10/lamborghini-carjackers-lured-by-243m-cyberheist/) |
| USMS seized-crypto / "Lick" / John Daghita | 2026 | Ethereum + others | **ops** (insider/contractor-family access to seized wallets) | Government contractor custody | Not a USDC contract bug | [TRM](https://www.trmlabs.com/resources/blog/zachxbt-uncovers-crypto-theft-network-linked-to-us-government-seizure-funds); [The Block](https://www.theblock.co/post/386945/individual-behind-40-million-government-wallet-theft-is-son-of-seized-crypto-contractor-executive-zachxbt); [CoinDesk](https://www.coindesk.com/business/2026/01/26/u-s-marshals-investigate-claims-that-son-of-government-contractor-stole-usd40-million-of-seized-crypto); Wikipedia ZachXBT |
| US government seized funds ~$20M (Oct 2024; partial return) | 2024 | Ethereum | **ops** (compromised gov-linked address) | Custody of seized assets | Instant-exchange off-ramps | [t.me/investigations/172](https://t.me/investigations/172), [173](https://t.me/investigations/173) |
| Chase Senecal "HZ" NFT phishing | 2021-22 | Ethereum | **ops** (phishing; FBI seizure) | Fake NFT site / Discord | `setApprovalForAll` UX | [CCN](https://www.ccn.com/education/crypto/zachxbt-crypto-detective-uncovering-blockchain-scams/) |
| French BAYC "animate my ape" ring | 2021 | Ethereum | **ops** (phishing; French convictions) | Fake animation dapp | Same | [CMC Academy](https://coinmarketcap.com/academy/article/who-is-zachxbt-cryptos-anonymous-scam-hunter); Wikipedia |
| Home-invasion / fake delivery (CCN FAQ $4.3M) | ~2024 | Multi | **ops** (physical + coerced transfer) | Physical security / leaked PII | Not a smart-contract class | [CCN FAQ](https://www.ccn.com/education/crypto/zachxbt-crypto-detective-uncovering-blockchain-scams/) |
| Wojtek Kulisz "Merry" (Polish social-engineering TA) | 2026 | Multi | **ops** (SE; later raid) | Social engineering of victims | [t.me/investigations/344](https://t.me/investigations/344) | Telegram + Polish CBZC press release linked there |
| Coinbase support impersonation (~$2M cluster) | 2025-26 | Multi | **ops** | Fake support | [Cointelegraph/Facebook cite](https://www.facebook.com/cointelegraph/posts/-alert-onchain-investigator-zachxbt-linked-a-polished-coinbase-support-impersona/1190012826638877/) |
| BitcoinIRA / iTrustCapital alleged DB leaks enabling SE | 2026 | Off-chain + withdrawals | **ops** (data breach -> targeted theft; example victim $1.2M) | IRA platform PII | [t.me/investigations/361](https://t.me/investigations/361) |
| Transak ransomware / KYC leak | 2024 | Off-chain | **ops** (PII; later SE risk) | On-ramp KYC store | [t.me/investigations/169](https://t.me/investigations/169), [171](https://t.me/investigations/171) |
| Atomic stealer victim $250K via KuCoin mule KYC | 2025 | EVM | **ops** (infostealer + CEX mules) | Desktop malware + purchased KYC | [t.me/investigations/348](https://t.me/investigations/348) |
| Early Solana whale 180.9k SOL | 2026 | Solana | **ops** (key compromise suspected; unstake+bridge pattern) | Solana key | [t.me/investigations/353](https://t.me/investigations/353) |
| JRNY influencer wallets ~$4M | 2024 | EVM | **ops** (key compromise suspected) | Hot wallet keys | [t.me/investigations/184](https://t.me/investigations/184) |
| Solana meme-coin whale $2.2M (PNUT etc.) via WallStreetBets X ATO | 2024 | Solana | **ops** (compromised X account + phishing; passkey bug on X mobile) | Twitter ATO + drain site | [t.me/investigations/187](https://t.me/investigations/187), [188](https://t.me/investigations/188) |
| Andy Ayrey / Truth Terminal account hack + bundled memecoins | 2024 | Solana | **ops** (account/key); not a Pump.fun program 0-day | Social account + deployer | [t.me/investigations/175](https://t.me/investigations/175), [177](https://t.me/investigations/177) |
| Wiz Khalifa Pump.fun (same TA as Ayrey) | 2024 | Solana | **ops** | Compromised celebrity/account narrative | [t.me/investigations/177](https://t.me/investigations/177) |
| Multisig exploiter 9980 ETH to eXch | 2024 | Ethereum | **ops** (multisig compromise; tracing) | Multisig keys | [t.me/investigations/183](https://t.me/investigations/183) |
| Hashflare proceeds movement | 2026 | Ethereum | **ops** / fraud proceeds (not a DeFi bug) | Historical Ponzi wallets | [t.me/investigations/342](https://t.me/investigations/342); DOJ Hashflare |
| Tron 120M USDT cluster / Tether blacklist | 2026 | Tron | **ops** (illicit movement; XMR spike) | Instant exchanges + Near Intents | [t.me/investigations/341](https://t.me/investigations/341) |
| Brazil IT worker ~$100M cyber theft (ZachXBT tracking) | 2025 | Multi | **ops** | Corporate IT / insider | [The Record via Wikipedia](https://therecord.media/brazil-police-arrest-worker-theft) |
| Hundreds of wallets drained for <$2k each (multi-chain campaign) | 2026 | Multi | **ops** (drainer/malware; class not fully public in snippets) | Unknown client vector | [Yellow.com](https://yellow.com/news/zachxbt-reports-hundreds-of-wallets-drained-for-under-dollar2000-each-across-multiple-chains) |
| Inferno Drainer **operator** payout hijack $2.5M | 2024 | EVM | **ops** (criminals' admin panel; ZachXBT leaked chat). Victims still lost to phishing | DaaS admin | [MetaMask March 2024](https://metamask.io/news/metamask-security-monthly-march-2024) |

### 2.4 CEX, casino, bridge, processor incidents (ZachXBT public)

| Name | Year | Chain | Ops vs contract | What was wrongly trusted | Review seed | Sources |
|------|------|-------|-----------------|--------------------------|-------------|---------|
| M2 exchange ~$13M hot wallets | 2024 | ETH, BTC, SOL | **ops** (hot wallets) | Exchange keys | [t.me/investigations/174](https://t.me/investigations/174) |
| Metawin casino ~$4M | 2024 | ETH, SOL | **ops** or mixed (ZachXBT listed 115+ theft addrs; confirm before calling it a VRF bug) | Casino hot wallets / signing | [t.me/investigations/176](https://t.me/investigations/176) |
| TeleSwap ~$735k | 2026 | Bitcoin, ETH | Unknown publicly in ZachXBT post (hot wallet stopped; Tornado). Treat as **ops-or-contract TBD**; do not invent a class | BTC hot wallet | [t.me/investigations/356](https://t.me/investigations/356) |
| FixedFloat repeat drains | 2023-24 | ETH | Often **ops** (swap-service keys) or disputed; ZachXBT-adjacent public tracking | Instant-exchange hot wallets | [Reddit incident discussion](https://www.reddit.com/r/CryptoCurrency/comments/1btvof9/fixedfloat_reportedly-suffers-another-exploit/) - confirm ZachXBT primary before treating as his exclusive case (Gaps) |
| KuCoin mule-KYC laundering (ZachXBT alerts, 2026) | 2025-26 | Arbitrum, ETH | **ops** (stolen-fund cashout) | CEX KYC integrity | [t.me/investigations/348](https://t.me/investigations/348), [350](https://t.me/investigations/350); [bitcoinfoundation.org/news](https://bitcoinfoundation.org/news/crimes-and-fraud-news/zachxbt-kucoin/) |
| Circle CCTP used to move stolen USDC (ZachXBT critique) | 2026 | Multi | **ops** (bridge as pipe). CCTP working as designed | Attacker already had keys; bridge did not freeze | Do not treat as a Circle mint bug unless a postmortem says so | [Yahoo](https://finance.yahoo.com/markets/crypto/articles/zachxbt-slams-circle-letting-millions-052847479.html) |
| EigenLayer X account compromise | 2024 | n/a (phishing vector) | **ops** | Hijacked project Twitter | Official X handle allowlist | [t.me/investigations/166](https://t.me/investigations/166) |
| CoinSpot hot wallet ~$2M | 2023 | Ethereum -> BTC | **ops** (hot wallet; bridged via THORSwap / Wan) | Exchange hot keys | Not a CoinSpot listing-token bug | [t.me/s/investigations?before=90](https://t.me/s/investigations?before=90) (8 Nov 2023) |
| Rain exchange ~$14.8M | 2024 | BTC, ETH, SOL, XRP | **ops** (hot-wallet outflows; team silent at time of post) | Exchange hot keys | Custodian keys | [t.me/s/investigations?before=130](https://t.me/s/investigations?before=130) (29 Apr 2024) |
| Thunder Terminal | 2023 | Ethereum | Mixed: ZachXBT reported exploit + Railgun; **do not invent a class** without a postmortem | Unknown at time of alert | Confirm contract vs key before auditing a fork | [t.me/s/investigations?before=90](https://t.me/s/investigations?before=90) (27 Dec 2023) |
| Velodrome + Aerodrome frontends (twice) | 2023 | Optimism / Base | **ops** (compromised frontend; Porkbun registrar SE, twice) | DNS / registrar account | Pin frontend hashes; DNSSEC; registrar 2FA. Not a veAERO math bug | [t.me/s/investigations?before=90](https://t.me/s/investigations?before=90); [x.com/velodromefi/status/1729771762752135463](https://x.com/velodromefi/status/1729771762752135463) |
| Across Protocol Discord vanity (`gg/across`) | 2023 | Ethereum | **ops** (stolen Discord invite; ~$880k) | Docs-linked Discord | Vanity Discord invites are takeover-prone; official guild ID | [t.me/s/investigations?before=90](https://t.me/s/investigations?before=90) (26 Dec 2023) |

### 2.5 Influencer rugs, paid shills, insider dumps (ops / market, not Solidity unless noted)

| Name | Year | Chain | Ops vs contract | What was wrongly trusted | Review seed | Sources |
|------|------|-------|-----------------|--------------------------|-------------|---------|
| BitBoy / Ben Armstrong paid shills | 2021-23 | Multi | **ops** (paid promo of exit scams) | YouTube authority | Disclosure, not a token bug | [CMC Academy](https://coinmarketcap.com/academy/article/who-is-zachxbt-cryptos-anonymous-scam-hunter) |
| Logan Paul CryptoZoo | 2021-22 | Ethereum | **ops** (abandonware / mismanagement allegations) | Celebrity NFT | Delivery vs mint proceeds | CMC Academy |
| Logan Paul ElonGate / DinkDoink pumps | 2021 | BSC/ETH | **ops** (pump after promo) | Celebrity tweets | Same | CMC Academy |
| Ansem (Zion Thomas) memecoin pump-and-dumps | 2024 | Solana | **ops** | KOL calls | [Yahoo Finance](https://finance.yahoo.com/news/crypto-influencer-ansem-accused-promoting-133112274.html) |
| Crypto Beast / $ALT ~$11M | 2024-25 | EVM | **ops** (coordinated dump) | KOL | [CryptoRank](https://cryptorank.io/news/feed/a0cba-zachxbt-exposes-crypto-kol-crypto-beast) |
| MustStopMurad wallet cluster ~$27.75M | 2024 | ETH, SOL | **ops** (cluster mapping; market) | KOL concentration | [t.me/investigations/165](https://t.me/investigations/165); [x.com/zachxbt/status/1843940648430493906](https://x.com/zachxbt/status/1843940648430493906) |
| Laurent Correia (French reality) dubious promos | 2022-24 | Multi | **ops** | Celebrity | [CCN](https://www.ccn.com/education/crypto/zachxbt-crypto-detective-uncovering-blockchain-scams/) |
| MachiBigBrother / Jeffrey Huang / Formosa Financial | 2018 ICO + 2022 article | Ethereum | **ops** / alleged treasury misconduct (defamation suit later settled with wording edits). Not a Uniswap bug | ICO treasury + influencer | [ZachXBT Medium](https://medium.com/@investigationsbyzachxbt/a-story-of-machi-big-brother-jeff-huang-a1ad073fcfa8); [CoinDesk defense fund](https://www.coindesk.com/business/2023/06/19/crypto-community-donates-1m-to-sleuth-zachxbt-after-defamation-lawsuit/) |
| DJT token / Martin Shkreli (Arkham bounty) | 2024 | Ethereum | **ops** (attribution of deployer) | Meme-token deployer identity | [The Bit Times](https://thebittimes.com/crypto-sleuth-zachxbt-wins-second-arkham-bounty-after-wazirx-hack-tbt97121.html) |
| LAB token insider/MM / Bitget / Aster dumps | 2026 | BSC | **ops** (supply control, OTC, CEX MM) | Team-controlled float | [t.me/investigations/354](https://t.me/investigations/354); [x.com/zachxbt/status/2054898749923860819](https://x.com/zachxbt/status/2054898749923860819) |
| MemeCore (M) CEX listing vs no on-chain liquidity | 2026 | BSC + CEX | **ops** (listing / inorganic supply) | CEX perps listing | [t.me/investigations/343](https://t.me/investigations/343) |
| ESPORTS / RIVER / LIGHT / Sablier-linked MM cluster | 2026 | BSC | **ops** | Vesting + CEX | [t.me/investigations/331](https://t.me/investigations/331) referenced from 354 |
| ZACHXBT impersonation memecoins | 2026 | Multi | **ops** (impersonation tokens; he dumped donations to charity) | Fake affiliation | [t.me/investigations/351](https://t.me/investigations/351) |
| Prediction-market insider (Israeli national, ongoing) | 2026 | Off-chain + PM | **ops** | Insider information | [t.me/investigations/340](https://t.me/investigations/340) |
| Axiom Exchange insider tools (Broox Bauer / @WheresBroox) | 2026 | Solana (product) + memecoins | **ops** (abuse of internal user-wallet dashboards; not an AMM 0-day) | BD staff could look up private wallets / referral / UID | In-scope trading terminals: least privilege, audit logs, no BD access to full wallet maps | [CoinDesk](https://www.coindesk.com/markets/2026/02/26/zachxbt-alleges-axiom-employee-conducted-insider-trading); [Decrypt](https://decrypt.co/359223/y-combinator-backed-axiom-employees-accused-insider-trading-zachxbt); [crypto.news](https://crypto.news/zachxbt-axiom-employees-insider-trading-report-2026/) |
| Axiom Polymarket leak-ahead betting | 2026 | Off-chain | **ops** (information leak about an upcoming ZachXBT report) | Prediction market as leak detector | Not a contract class | Decrypt; crypto.news |
| RaveDAO (RAVE) | 2026 | EVM + CEX | **ops** (ZachXBT: pump-and-dump / concentrated holdings; not a DEX invariant) | Insider float + CEX listing | Vesting, MM, listing due-diligence | [Cointelegraph](https://cointelegraph.com/news/investigator-concentration-concerns-memecore) |
| SIREN / MYX / COAI / PIPPIN (questionable CEX PA cluster) | 2026 | Multi + CEX | **ops** (ZachXBT flagged alongside RAVE/MemeCore) | Thin CEX books | Same as MemeCore | Cointelegraph / Yahoo MemeCore articles |
| NFTMachine / @scaredofboobs | 2022-23 | ETH then Solana | **ops** (court-ordered $275k restitution unpaid; continued grifts) | Repeat operator | Do not treat as a new Solana program bug | [t.me/s/investigations?before=90](https://t.me/s/investigations?before=90) (28 Dec 2023) |

### 2.6 NFT rugs and phishing rings (public ZachXBT era 2021-2023)

These are **ops** (rug / Discord hijack / phishing) unless a hidden mint is in source. Review seed: mint authority, withdraw on treasury, `setApprovalForAll` on fake "reveal" sites. Confirming roundups: [CMC Academy](https://coinmarketcap.com/academy/article/who-is-zachxbt-cryptos-anonymous-scam-hunter), [CCN](https://www.ccn.com/education/crypto/zachxbt-crypto-detective-uncovering-blockchain-scams/), contemporaneous CT coverage. Rows below are **named projects he is widely reported to have covered**; where a dedicated URL was not re-fetched this pass, treat as "ZachXBT-era public case; re-verify thread before citing in a legal memo."

| Name | Year | Chain | Ops vs contract | What was wrongly trusted | Review seed | Sources |
|------|------|-------|-----------------|--------------------------|-------------|---------|
| Frosties NFT | 2022 | Ethereum | **ops** (rug; later US charges) | Mint / Discord | Treasury withdraw, freeze | Public DOJ Frosties case + ZachXBT-era threads (re-verify X) |
| Evolved Apes | 2021 | Ethereum | **ops** (rug) | Game NFT | Same | Contemporary reporting |
| Baller Ape Club | 2021 | Ethereum | **ops** | Celebrity ape clone | Same | Contemporary reporting |
| Big Daddy Ape Club | 2021 | Ethereum | **ops** | Clone mint | Same | Contemporary reporting |
| Ape Kids Club | 2021 | Ethereum | **ops** | Clone mint | Same | Contemporary reporting |
| Pixelmon | 2022 | Ethereum | **ops** / delivery failure (not a reentrancy) | High mint vs art | Product delivery | Contemporary reporting |
| Gala Games x 888 "The Orbs" | 2021-22 | Ethereum | **ops** (alleged NFT fiasco / failed compensation; not an ERC-721 bug) | Burn-3-to-mint path + auction vs promised refunds | Delivery/refund process, not `transferFrom` | [InsertCoinsLV / ZachXBT thread recap](https://www.insertcoinslv.com/crypto/zachxbt-details-a-gala-games-rug-pull-with-888-inner-circle/) |
| GALA token privileged mint (~5B) | 2024 | Ethereum | **contract** (access control: privileged minter). Dump was ops after mint | `mint` role on token | If in-scope is GALA token, review minter roles. Do not treat as Uniswap | [CryptoScamsWiki Gala note](https://cryptoscams.wiki/entries/evolved-apes-nft) |
| ZKasino / ZKAS "bridge-to-earn" | 2023-24 | Ethereum | **ops** (exit: deposits not returned as ETH; ZachXBT called founders proven bad actors Dec 2023) | Bridge-to-earn marketing | Not a zkEVM proof bug | [CryptoScamsWiki ZKasino](https://cryptoscams.wiki/entries/zkasino-exit-scam) |
| Sorta Finance | ~2023 | Arbitrum | **ops** (serial lending rug; ZachXBT warned) | Fake lending protocol | Same operator cluster | CryptoScamsWiki ZKasino related |
| Magnate | ~2023 | EVM | **ops** (same serial rug operator) | Fake lending | Same | CryptoScamsWiki |
| Kokomo | ~2023 | EVM | **ops** (same cluster) | Fake lending | Same | CryptoScamsWiki |
| Solfire | ~2023 | Solana | **ops** (same cluster) | Fake lending | Same | CryptoScamsWiki |
| Glori Finance | 2024 | Arbitrum | **ops** (copy-paste lending rug; LP bait from prior rugs) | Fake lending + recycled LP | Same serial operator | [t.me/s/investigations?before=130](https://t.me/s/investigations?before=130) |
| Crolend | ~2023 | EVM | **ops** (same serial lending-rug group) | Fake lending | Same | Telegram Glori alert |
| HellhoundFi | ~2023 | EVM | **ops** (same group) | Fake lending | Same | Telegram Glori alert |
| Lendora | ~2023 | EVM | **ops** (same group) | Fake lending | Same | Telegram Glori alert |
| HashDAO | ~2023 | EVM | **ops** (same group) | Fake lending | Same | Telegram Glori alert |
| Leaper | ~2023 | EVM | **ops** (same group) | Fake lending | Same | Telegram Glori alert |
| Zebra (lending clone, not Zcash) | ~2023 | EVM | **ops** (same group) | Fake lending | Same | Telegram Glori alert |
| AnubisDAO | 2021 | Ethereum | Mixed: treasury failure; ZachXBT: **gross negligence / lying**, not proven theft. Not a Uniswap bug | DAO treasury ops | Do not treat as a classic reentrancy without a postmortem | [Protos](https://protos.com/from-60m-failure-to-crypto-scam-cop-the-reinvention-of-0xsisyphus/) |
| Trollz / 6ix9ine-adjacent NFT promo | 2021 | Ethereum | **ops** (promo / alleged dump) | Celebrity NFT | Same as other celeb mints | Contemporary CT; confirm thread (partial) |
| Save the Kids NFT | 2021 | Ethereum | **ops** (promo / dump allegations) | Influencer mint | Same | Contemporary reporting |
| Monkey Kingdom | 2021 | Solana | **ops** | Solana mint | Same | Contemporary reporting |
| The Association NFT | 2022 | Ethereum | **ops** | Sports NFT | Same | Contemporary reporting |
| Premint / Discord hijack wave (many collections) | 2022 | Ethereum | **ops** | Discord mods / fake verify | Bot token, webhook | Campaign family; not one contract |
| OpenSea phishing / fake collection | 2021-23 | Ethereum | **ops** | Google ads / Discord | Domain | Campaign family |
| Blur airdrop phishing | 2023 | Ethereum | **ops** | Airdrop claim UX | Permit / Seaport sig | Campaign family |
| Otherside / Bored Ape phishing clones | 2022 | Ethereum | **ops** | Yuga brand | Same | Campaign family |
| Honeyland | 2023 | Solana | **ops** (alleged rug; ZachXBT-era) | GameFi mint | Treasury | Contemporary reporting (re-verify thread) |
| Goblintown / derivative rugs | 2022 | Ethereum | **ops** | Derivative mint | Same | Contemporary CT |
| Loot derivative rugs | 2021-22 | Ethereum | **ops** | Loot brand | Same | Campaign family |

### 2.7 Protocol/smart-contract bugs where ZachXBT's public role was mainly **tracing** (name the class)

Do not skip the contract review if **your in-scope repo is that protocol**. Do not start there if you are reviewing an unrelated lending fork "because ZachXBT tweeted Euler."

| Name | Year | Chain | Ops vs contract | What was wrongly trusted | Review seed | Sources |
|------|------|-------|-----------------|--------------------------|-------------|---------|
| Euler Finance (ZachXBT fund-flow commentary in 2023 news cycle) | 2023 | Ethereum | **contract** (donation / liquidation / empty-market class). ZachXBT: tracing | `donate` / liquidation math | If in-scope is Euler-like, review empty-market; his tweets are not the class definition | Rekt / Euler postmortem (primary); ZachXBT secondary |
| Curio MakerDAO-fork | 2024 | Ethereum | **contract** (Maker-style). MetaMask monthly; not a tayvano/ZachXBT "drainer" | Curio's own adapters | Audit Maker clones | [MetaMask March 2024](https://metamask.io/news/metamask-security-monthly-march-2024) |
| Wintermute (2022) | 2022 | Ethereum | **ops** (Profanity vanity key). Not a Wintermute MM contract 0-day | Weak vanity ETH key | Never use Profanity; not Uniswap math | Public Wintermute postmortem; ZachXBT-era tracing |
| Slope wallet (Solana 2022) | 2022 | Solana | **ops** (telemetry/server stored seeds). Not a token program bug | Mobile wallet backend | Never upload seeds | Public Slope/Solana incident; confirm ZachXBT primary (Gaps) |
| BadgerDAO 2021 | 2021 | Ethereum | **ops** (compromised frontend / Cloudflare). Contract was used as designed via approvals | Website integrity | Subresource integrity, allowlists | Public Badger postmortem |
| Ledger Connect Kit npm | 2023 | EVM frontends | **ops** (supply chain; Angel Drainer). Not Ledger device crypto | npm token of ex-employee | Pin Connect Kit; treat dapp JS as in-scope | CryptoScamsWiki / public Ledger incident (tayvano/MetaMask also) |
| Onyx Protocol | 2023 | Ethereum | **contract** (empty-market / rounding family; ZachXBT: tracing of exploiter gifts). Not a drainer | Lending math | If in-scope is Onyx-like, review empty-market; his post is fund-flow | [t.me/s/investigations?before=70](https://t.me/s/investigations?before=70) (2 Nov 2023); Rekt / public Onyx postmortem |

### 2.8 Named Telegram alerts (2023-2024) not already in 2.1-2.7

Primary: [t.me/s/investigations](https://t.me/s/investigations) pages `?before=54`, `70`, `90`, `110`, `130`. All **ops** unless noted. Review seed for unnamed whale drains: wallet UI (Permit / poisoning / ATO), not a random Uniswap pool.

| Name | Year | Chain | Ops vs contract | What was wrongly trusted | Review seed | Sources |
|------|------|-------|-----------------|--------------------------|-------------|---------|
| 10k BTC Binance-origin mixer cluster ~$265M | 2023 | Bitcoin | **ops** (laundering; not a BTC script 0-day) | Mixers after 2018 Binance withdrawal | Compliance tracing | [t.me/s/investigations?before=54](https://t.me/s/investigations?before=54) |
| Unreported ~$24M BTC ransomware payment | 2023 | Bitcoin | **ops** (ransomware) | Victim paid; laundered via MEXC/OKX/Huobi/Binance + THORChain | Not a protocol bug | Same page |
| stETH/rETH whale phish ~$24.2M | 2023 | Ethereum | **ops** (phishing) | Signed approval / drain | Permit / setApprovalForAll UX | Same page |
| Vitalik.eth X compromise | 2023 | n/a | **ops** (ATO) | Hijacked celebrity X | Do not click "giveaway" | Same page |
| burgel.eth ~$3M | 2023 | Ethereum | **ops** (key compromise; Tornado) | Private key | Seed/device hygiene | Same page |
| HyPC OTC dump warning | 2023 | Ethereum | **ops** (OTC scammer unloading) | OTC counterparty | Do not treat as HyPC bytecode | Same page |
| PPT (BSC) OTC scam ~$194k | 2023 | BSC | **ops** (same OTC TA) | OTC + MEXC dump | Same | [t.me/s/investigations?before=70](https://t.me/s/investigations?before=70) |
| Trezor / Evri phishing-email cluster | 2023 | Off-chain | **ops** (possible shipper/vendor email leak) | Hardware-wallet purchase email | Never type SRP from email | Same page |
| Strike (Lightning) phishing-email cluster | 2023 | Off-chain | **ops** (possible account-email leak) | Strike account email | Same | Same page |
| Nelly (rapper) X ATO phishing DMs | 2023 | n/a | **ops** | Celebrity DM | Same as other ATO | Same page |
| 27M USDT individual loss | 2023 | Ethereum -> BTC | **ops** (scam then THORSwap / instant exchanges) | Unknown SE vector in post | Instant-exchange off-ramps | [t.me/s/investigations?before=90](https://t.me/s/investigations?before=90) (12 Nov 2023) |
| Fake ZachXBT Discord `gg/investigations` | 2023 | n/a | **ops** | Impersonation Discord | He has no official Discord | Same page (11 Dec 2023) |
| ~275,700 LINK phish ~$4.4M | 2023 | Ethereum | **ops** (phishing; h/t ScamSniffer; eXch) | Permit-class / drain site | Decode spender | Same page (29 Dec 2023) |
| CoinTelegraph + WalletConnect + Token Terminal + De.Fi phishing emails ~$580k | 2024 | EVM | **ops** (brand-spoof email) | Email "from" those brands | SPF/DKIM; never sign from email | Same page (23 Jan 2024) |
| Ripple / Chris Larsen cluster ~213M XRP (~$112.5M) | 2024 | XRP Ledger | **ops** (insider/personal keys). Not an XRPL consensus bug | Personal/hot XRP keys | Custodial key ceremony for insiders | [t.me/s/investigations?before=110](https://t.me/s/investigations?before=110) (31 Jan 2024); Chris Larsen confirmation cited there |
| LastPass vault victims (Oct 2023 ~$4.4M; Feb 2024 +$6.2M) | 2023-24 | EVM -> BTC | **ops** (password-manager vault from 2022 LastPass incident) | Cloud password vault + reused crypto passwords | Not a token bug; seed never in LastPass | Same page (21 Feb 2024) |
| Serenity Shield 6.9M SERSH (Feb 2024) | 2024 | BSC | **ops** (wallet hack; on-chain to OKX Dex / Concentric / Contagious Interview cluster) | Team wallet | Same as 2.2 | Same page (27 Feb 2024) |
| PAAL phish ~$736k + stale-approval drain | 2024 | EVM | **ops** (phishing; forgot to revoke) | Permit + leftover allowance | Revoke UX; allowance expiry | Same page (6 Mar 2024) |
| USDT sent to USDT contract (clown-of-the-day) | 2024 | Ethereum | **ops** (user error / poisoning-adjacent) | Copy-paste / no address book | Address book; poison filter | Same page (15 Mar 2024) |
| Netmind AI holder + Webaverse Nov 2022 link | 2024 | EVM | **ops** (holder hack linked to older compromise) | Keys / old cluster | Cluster reuse | Same page (15 Mar 2024) |
| Inferno Drainer operator $2.3M hijack (Telegram leak) | 2024 | EVM | **ops** (criminals' panel). **Same incident** as §2.3 $2.5M row (amounts differ by source) | DaaS admin | Not a victim-side contract class | Same page (18 Mar 2024); MetaMask Mar 2024 |
| TON Blockchain X compromise | 2024 | n/a | **ops** (ATO) | Project Twitter | Same | Same page (19 Mar 2024) |
| Trezor X compromise | 2024 | n/a | **ops** (ATO) | Hardware-wallet Twitter | Same | Same page (19 Mar 2024) |
| Cointelegraph X compromise | 2024 | n/a | **ops** (ATO) | News-org Twitter | Same | Same page (23 Mar 2024) |
| Tom Holland X ATO (Bonad phishing) | 2024 | n/a | **ops** | Celebrity X | Same | [t.me/s/investigations?before=130](https://t.me/s/investigations?before=130) |
| $68M (1155 WBTC) address-poisoning copy-paste | 2024 | Ethereum | **ops** (address poisoning). Partial return via on-chain message | Explorer recent-transfer lookalike | Hide zero-value transfers; address book | Same page; confirming press widely reprinted this case |
| ~$18M Coinbase-account drain (~5800 ETH) | 2024 | Ethereum | **ops** (CEX account; instant exchanges + THORChain / Defiway / Wan) | Coinbase login / session | Not an ETH consensus bug | Same page |
| Three BAYC phish | 2024 | Ethereum | **ops** | Fake NFT UX | `setApprovalForAll` | Same page |
| wstETH phish ~$1.25M | 2024 | Ethereum | **ops** | Drain site | Permit / approval | Same page |
| Ether.fi weETH/Liquid1 phish ~$6.9M (repeat victim) | 2024 | Ethereum | **ops** | Drain site; leftover allowance from prior year | LRT frontends must decode spender; revoke | Same page |
| Pink Drainer admin shutdown after $75M+ | 2024 | EVM | **ops** (DaaS exit announcement; ZachXBT alert) | DaaS still ops | Same as Pink family in §3 | Same page |
| Caitlyn Jenner X ATO | 2024 | n/a | **ops** | Celebrity X | Same | Same page |

---

## 3. tayvano / Fire / MyCrypto / MetaMask Security: incidents and campaign families

Taylor's public work is **wallet and signing UX**, phishing taxonomy, DaaS tracking, and DPRK malware-on-Telegram - not Solidity contests. Fire.xyz blog: **not located** (Gaps). Use MyCrypto Medium, MetaMask Security Monthly, X [@tayvano_](https://x.com/tayvano_), GitHub [tayvano](https://github.com/tayvano), Blockspace podcast.

### 3.1 Drainer-as-a-service families (ops)

Permit2/Permit/`setApprovalForAll` are **signing** classes. The Uniswap Permit2 **deployment** is not "hacked."

| Name | Year | Chain | Ops vs contract | What was wrongly trusted | Review seed | Sources |
|------|------|-------|-----------------|--------------------------|-------------|---------|
| Inferno Drainer | 2022-25 | EVM multi | **ops** (DaaS; spoofed Seaport, WalletConnect, Coinbase, 100+ brands) | Fake mint/airdrop + malicious script | Decode Seaport/WC/Permit payloads; never sign "connect" that includes spender | [Group-IB](https://www.group-ib.com/blog/inferno-drainer/); [AVOID.NET](https://avoid.net/inferno-drainer); [CryptoScamsWiki](https://cryptoscams.wiki/entries/inferno-drainer); MetaMask Mar 2024 (service itself drained $2.5M) |
| Pink Drainer | 2023-24 | EVM | **ops** (DaaS; FTX/BlockFi email campaign >$7M) | Stolen email lists (MailerLite) + Permit-class sigs | Email is not an auth factor for signatures | [MetaMask March 2024](https://metamask.io/news/metamask-security-monthly-march-2024); [Cointelegraph MailerLite](https://cointelegraph.com/news/mailerlite-confirms-hack-crypto-phishing-email-3m-attacks) |
| Angel Drainer | 2023-24 | EVM | **ops**; used in Ledger Connect Kit supply chain | Compromised npm + dapp JS | Pin wallet-connect kits | CryptoScamsWiki Ledger Connect Kit entry |
| Monkey Drainer | 2022-23 | EVM | **ops** (predecessor; replaced by Venom) | Same DaaS UX | Same | [MetaMask April 2023](https://metamask.io/news/metamask-security-monthly-april-2023) |
| Venom Drainer | 2023 | EVM | **ops** (~$27M, ~15k victims, ~170 brands) | Phishing kit | Same | MetaMask April 2023; [Scam Sniffer / Dune](https://dune.com/scamsniffer/venom-drainer-stats) |
| Medusa Drainer | 2024+ | EVM | **ops** | DaaS | Same | Hypernative industry roundups |
| CrimeEnjoyor (EIP-7702 script) | 2025 | Ethereum | **ops** (7702 delegation). tayvano: **private key problem, not Pectra** | Type-4 authorization / already-stolen keys | Decode delegate; reject unknown 7702 | [MetaMask June 2025](https://metamask.io/news/metamask-security-report-june-2025); Wintermute analysis via The Block |
| OG-wallet multichain drain (Dec 2022-Apr 2023, 5000+ ETH) | 2022-23 | 11+ chains | **ops** (unknown vector at time; tayvano+Harry Denley). MetaMask denied a MetaMask-only 0-day | Unknown: seeds from 2014-2022 wallets | Do not invent a MetaMask RPC 0-day; investigate seed storage, password managers, old dumps | [x.com/tayvano_/status/1648187031468781568](https://x.com/tayvano_/status/1648187031468781568); [BeInCrypto](https://beincrypto.com/metamask-deny-wallet-draining-exploit/); [CryptoTimes](https://www.cryptotimes.io/2023/04/19/metamask-denies-accusations-of-wallet-hack-worth-10m/) |
| Google search-ad phishing | 2023 | EVM | **ops** (~$4M Scam Sniffer) | Sponsored search result | Bookmark official URLs; review your **frontend ads** | MetaMask April 2023; Scam Sniffer Notion cited there |
| Fake Uniswap-hack hashtag -> fake Revoke.cash | 2023 | Ethereum | **ops** | Panic + fake revoke site | Official revoke.cash only | MetaMask April 2023; Hayden Adams / Evan Van Ness |
| Address poisoning | 2023+ | EVM | **ops** | Explorer recent-transfers copy-paste | Hide 0-value transfers; address book | [CoinDesk / Etherscan](https://www.coindesk.com/tech/2023/04/10/etherscan-reconfigures-blockchain-explorer-settings-to-filter-out-potential-scams/); MetaMask April 2023 |
| Clipboard / lookalike address malware | 2018+ | EVM, BTC | **ops** | OS clipboard | Hardware confirm; MyCrypto security guide | [MyCrypto security guide](https://medium.com/mycrypto/mycryptos-security-guide-for-dummies-and-smart-people-too-ab178299c82e) |
| Fake MetaMask extension / clone apps | ongoing | EVM | **ops** | App store / sideload | Official extension ID | MyCrypto guide; MetaMask help |
| SparkKitty (OCR of seed photos) | 2025 | Mobile | **ops** | Photo gallery seed screenshots | Never photograph SRP | [MetaMask June 2025](https://metamask.io/news/metamask-security-report-june-2025) |
| Crocodilus Android banker | 2025 | Android | **ops** | Fake Chrome + accessibility | Same | MetaMask June 2025 |
| CoinMarketCap homepage doodle injector | 2025 | EVM | **ops** (supply chain on **price site**) | Trusted domain JS | Treat third-party widgets as in-scope XSS | MetaMask June 2025; Cointelegraph |
| Cointelegraph fake CTG airdrop popup | 2025 | EVM | **ops** (compromised news frontend) | News site wallet popup | Same | MetaMask June 2025 |
| Trezor support-form phishing emails | 2025 | Off-chain | **ops** | Support email asking for backup | Never type SRP | MetaMask June 2025 |
| Collab.Land fake Discord bot (Inferno) | 2023-24 | EVM | **ops** | Discord verify bot | Official bot ID | AVOID.NET Inferno timeline |
| DPRK Telegram / fake Zoom / Trojan (~$300M class, tayvano podcast) | 2024-25 | Multi | **ops** | Telegram DM + malware | [Blockspace](https://blockspace.media/podcast/how-north-korean-hackers-stole-300m-via-telegram-w-taylor-monahan/) |
| Password-manager-stored SRP | ongoing | n/a | **ops** | Cloud password manager | tayvano: never store SRP in a password manager | [Blockfence interview](https://www.youtube.com/watch?v=_1iMxd7ldUQ) |
| MailerLite crypto-customer list theft | 2024 | Off-chain | **ops** (enabler for Pink Drainer emails) | ESP account | Vendor IR | Cointelegraph; MetaMask March 2024 |

### 3.2 Permit / Permit2 / WalletConnect / EIP-712 / clear-signing (product review, not a Uniswap 0-day)

| Name | Year | Chain | Ops vs contract | What was wrongly trusted | Review seed | Sources |
|------|------|-------|-----------------|--------------------------|-------------|---------|
| Permit2 signature phishing (campaign class) | 2023+ | EVM | **ops**. Permit2 **bytecode not exploited** | Blind `eth_signTypedData`; fake "login" | Wallet must render spender, token, amount, expiration, nonce. SIWE != Permit2 | [MetaMask Help: signature phishing](https://support.metamask.io/privacy-and-security/staying-safe-in-web3/signature-phishing/); [ChainScore Permit2 phishing](https://chainscorelabs.com/protocol/uniswap/incidents-and-security-advisories/permit2-phishing-vector-analysis) |
| EIP-2612 Permit phishing | 2020+ | EVM | **ops** | Gasless permit looks like "free signature" | Same; `permit(` on the token | MetaMask help |
| `setApprovalForAll` NFT drain | 2021+ | EVM | **ops** | Marketplace-looking popup | Decode operator; warn unlimited NFT approval | Inferno/Venom kits |
| Unlimited `approve` ERC-20 | 2018+ | EVM | **ops** | Default max uint | Cap allowances; Permit2 with limits is the **intended** fix if UI is honest | MyCrypto guide |
| WalletConnect session phishing | 2022+ | EVM | **ops**. WC crypto not broken | QR to attacker relay + later sig requests | Domain match, method allowlist, session expiry | Group-IB Inferno `wallet-connect.js`; MetaMask WC UX |
| `eth_sign` raw hash | 2016+ | EVM | **ops** | Opaque hash | Disable `eth_sign` or show "you can lose everything" | Historical MEW/MyCrypto stance |
| EIP-7702 phishing tuples | 2025+ | Ethereum | **ops** | "Upgrade wallet" typed auth | Show delegate contract; chainId; not a Pectra bug | tayvano via MetaMask June 2025 |
| Clear signing / ERC-7730 advocacy | 2024+ | EVM | Defense (not an incident) | Blind hardware screens | In-scope wallets: decode Permit2, Safe, 7702 | Industry + MetaMask |
| Seaport signature spoof (Inferno seaport.js) | 2023 | Ethereum | **ops** | Fake listing / bulk transfer sig | Decode Seaport orders | Group-IB |
| Coinbase Wallet Kit spoof (Inferno coinbase.js) | 2023 | EVM | **ops** | Fake CB widget | Same | Group-IB |

### 3.3 Historical MyCrypto / MEW / tayvano (2016-2020)

| Name | Year | Chain | Ops vs contract | What was wrongly trusted | Review seed | Sources |
|------|------|-------|-----------------|--------------------------|-------------|---------|
| The DAO 2016 | 2016 | Ethereum | **contract** (reentrancy). tayvano community/MEW era commentary, not a drainer | Splitter/reentrancy | If in-scope is a vault with callbacks, review reentrancy. She is not the primary postmortem author | Public DAO history; [Decential interview](https://www.decential.io/articles/qa-with-taylor-monahan-the-mother-of-the-dao-and-co-founder-of-the-first-ethereum-wallet) |
| MyEtherWallet Google-ad phishing | 2017-18 | Ethereum | **ops** | Sponsored "myetherwallet" | Same as 2023 Google ads | MyCrypto security guide |
| Fake Mist / fake desktop wallets | 2017 | Ethereum | **ops** | Sideloaded wallet | Code signing | MyCrypto guide |
| ICO contribution-address poisoning | 2017 | Ethereum | **ops** | Slack/Telegram fake ETH addr | Official announcement channels | MyCrypto guide |
| Parity wallet freeze / library suicide | 2017 | Ethereum | **contract** (Parity library `selfdestruct` / init). tayvano-era user support, not her bug | Uninitialized library | If in-scope is a wallet library, review `selfdestruct` and init | Public Parity postmortems |
| DeFi "dancing off a cliff" warnings | 2020 | Ethereum | Education | Infinite approvals, unaudited vaults | [Dear community](https://medium.com/@tayvano/dear-community-2579a3a697e0); [Risky Business](https://medium.com/mycrypto/risky-business-defi-and-ethereums-coming-of-age-story-4d99465ad102) |

### 3.4 Inferno / Venom / Pink impersonation targets (named lures)

Each row is a **named phishing lure**, not a unique root-cause 0-day. Class: **ops**. Wrongly trusted: lookalike domain + WalletConnect/Permit/`setApprovalForAll`. Review seed: official-domain allowlist + EIP-712 decode. Sources for the **family**: [Group-IB Inferno](https://www.group-ib.com/blog/inferno-drainer/) (Seaport, WalletConnect, Coinbase scripts; 100+ brands); Scam Sniffer Venom (~170 brands, [Dune](https://dune.com/scamsniffer/venom-drainer-stats)); [AVOID.NET Inferno](https://avoid.net/inferno-drainer); MetaMask monthlies.

Do not open the real protocol's Solidity looking for `onlyOwner` because a fake "Aave airdrop" site drained someone.

| Impersonated name | Typical year | Chain lure | Ops vs contract | What was wrongly trusted | Review seed | Source family |
|-------------------|--------------|------------|-----------------|--------------------------|-------------|---------------|
| Seaport / OpenSea | 2022-24 | ETH | ops | Fake listing / bulk transfer | Decode Seaport | Group-IB seaport.js |
| WalletConnect | 2022-24 | EVM | ops | Fake WC script | Session origin | Group-IB wallet-connect.js |
| Coinbase (dapp kit / wallet) | 2022-24 | EVM | ops | Fake Coinbase widget | coinbase.js | Group-IB |
| MetaMask | 2018-26 | EVM | ops | Fake extension / connect | Official extension ID | MyCrypto + Inferno |
| Uniswap | 2022-24 | EVM | ops | Fake swap / v4 hook claim | Permit2 decode | Inferno/Venom |
| PancakeSwap | 2022-24 | BSC | ops | Fake connect | Same | Chainabuse-class reports |
| Ledger Live / Connect Kit | 2023-24 | EVM | ops | npm kit + fake Live | Pin Connect Kit | Angel Drainer incident |
| Trezor | 2023-25 | Multi | ops | Support email / fake firmware | Never type SRP | MetaMask June 2025 |
| Blur | 2023-24 | ETH | ops | Fake airdrop / Blend | Seaport-like sig | Venom-era |
| Premint | 2022-23 | ETH | ops | Discord verify | Bot ID | NFT hijack wave |
| Art Blocks | 2023 | ETH | ops | Fake mint | setApprovalForAll | AVOID.NET Inferno |
| Pepe / meme claim | 2023-24 | EVM | ops | Token claim | Permit | Inferno |
| Collab.Land | 2023-24 | EVM | ops | Fake Discord bot | Official bot | AVOID.NET |
| Revoke.cash (clone) | 2023 | EVM | ops | Panic revoke | Bookmark real site | Hayden Adams 2023 |
| Etherscan (clone) | 2022-24 | EVM | ops | Fake explorer connect | ethereum.org links | Drainers |
| CoinMarketCap | 2025 | EVM | ops | Homepage JS injector | Third-party widgets | MetaMask June 2025 |
| Cointelegraph | 2025 | EVM | ops | Fake CTG airdrop popup | News sites must not WC | MetaMask June 2025 |
| ENS renew | 2023-24 | ETH | ops | Fake registrar | Official app.ens.domains | Drainers |
| Safe{Wallet} | 2024-25 | EVM | ops | Fake Safe app (distinct from Bybit signer malware) | Official app.safe.global | Drainers |
| zkSync airdrop | 2024 | zkSync | ops | Fake claim | Official domain | Venom-era |
| LayerZero airdrop | 2024 | Multi | ops | Fake claim | Same | Venom-era |
| EigenLayer airdrop | 2024 | ETH | ops | Fake claim + hijacked X | [t.me/investigations/166](https://t.me/investigations/166) | ZachXBT + drainers |
| Friend.tech | 2023-24 | Base | ops | Fake keys/claim | Same | Drainers |
| Pump.fun | 2024-26 | Solana | ops | Fake verify | Phantom blind sign | Drainers |
| Jupiter | 2024-26 | Solana | ops | Fake LFG/claim | Same | Drainers |
| Phantom verify | 2022-26 | Solana | ops | Fake wallet connect | Official phantom.app | Drainers |
| Aave | 2022-24 | ETH | ops | Fake airdrop / v4 claim | Permit | Inferno/Venom |
| Compound | 2022-24 | ETH | ops | Fake COMP claim | Same | Inferno |
| Curve | 2022-24 | ETH | ops | Fake CRV lock | Same | Inferno |
| Lido | 2022-24 | ETH | ops | Fake stETH wrap | Same | Inferno |
| Maker / Sky | 2022-25 | ETH | ops | Fake DAI/USDS claim | Same | Inferno |
| Balancer | 2022-24 | ETH | ops | Fake BAL claim | Same | Inferno |
| 1inch | 2022-24 | EVM | ops | Fake aggregation airdrop | Same | Inferno |
| Sushi | 2022-24 | EVM | ops | Fake SUSHI claim | Same | Inferno |
| Yearn | 2022-24 | ETH | ops | Fake vault | Same | Inferno |
| Convex | 2022-24 | ETH | ops | Fake CVX | Same | Inferno |
| Synthetix | 2022-24 | ETH | ops | Fake SNX | Same | Inferno |
| dYdX | 2022-24 | ETH | ops | Fake DYDX claim | Same | Inferno |
| GMX | 2022-24 | Arbitrum | ops | Fake GMX | Same | Inferno |
| Chainlink | 2022-24 | ETH | ops | Fake LINK staking | Same | Inferno |
| LooksRare | 2022-23 | ETH | ops | Fake LOOKS | Seaport-like | Inferno |
| Rarible | 2022-24 | ETH | ops | Fake RARI | Same | Inferno |
| SuperRare | 2022-24 | ETH | ops | Fake mint | Same | Inferno |
| Foundation | 2022-24 | ETH | ops | Fake drop | Same | Inferno |
| Zora | 2023-25 | Base | ops | Fake mint | Same | Drainers |
| Magic Eden | 2022-26 | SOL/ETH | ops | Fake listing | Same | Drainers |
| Tensor | 2023-26 | SOL | ops | Fake NFT | Same | Drainers |
| Galxe | 2023-25 | Multi | ops | Fake quest | OAuth + WC | Drainers |
| Zealy / Crew3 | 2023-24 | Multi | ops | Fake quest | Same | Drainers |
| Snapshot | 2023-24 | Off-chain | ops | Fake vote connect | Never sign vote via WC from ads | Drainers |
| Arbitrum airdrop | 2023 | Arbitrum | ops | Fake ARB claim | Official portal | Venom-era |
| Optimism airdrop | 2022-24 | OP | ops | Fake OP claim | Same | Inferno |
| Base airdrop | 2024-25 | Base | ops | Fake BASE claim | Same | Drainers |
| Polygon zkEVM | 2023-24 | Polygon | ops | Fake claim | Same | Drainers |
| Starknet airdrop | 2024 | Starknet | ops | Fake STRK | Same | Drainers |
| Celestia airdrop | 2024 | TIA | ops | Fake TIA | Same | Drainers |
| Aptos airdrop | 2022-24 | Aptos | ops | Fake APT | Petra clone | Drainers |
| Sui airdrop | 2023-24 | Sui | ops | Fake SUI | Suiet clone | Drainers |
| Immutable | 2023-24 | Immutable | ops | Fake IMX | Same | Drainers |
| Rainbow Wallet | 2023-24 | EVM | ops | Fake Rainbow | Official build | Drainers |
| Rabby | 2023-25 | EVM | ops | Fake Rabby | Same | Drainers |
| Trust Wallet | 2022-25 | Multi | ops | Fake TW | Same | Drainers |
| Exodus | 2022-25 | Multi | ops | Fake Exodus | Same | Drainers |
| Atomic Wallet (phishing, distinct from 2023 Lazarus) | 2023-25 | Multi | ops | Fake Atomic | Same | Drainers |
| Keplr | 2023-25 | Cosmos | ops | Fake Keplr | Same | Drainers |
| Leap | 2023-25 | Cosmos | ops | Fake Leap | Same | Drainers |
| Backpack | 2023-26 | SOL | ops | Fake Backpack | Same | Drainers |
| Solflare | 2023-26 | SOL | ops | Fake Solflare | Same | Drainers |
| OKX Web3 | 2023-25 | Multi | ops | Fake OKX dapp | Same | Drainers |
| Binance Web3 | 2023-25 | Multi | ops | Fake Binance | Same | Drainers |
| Bybit Web3 | 2024-25 | Multi | ops | Fake Bybit dapp (distinct from Feb 2025 CEX ops) | Same | Drainers |
| Kraken | 2023-25 | Multi | ops | Fake Kraken support | Never SRP to support | Drainers |
| Gemini | 2023-25 | Multi | ops | Fake Gemini | Same | Drainers |
| Crypto.com | 2023-25 | Multi | ops | Fake CDC | Same | Drainers |
| MoonPay | 2023-25 | Fiat | ops | Fake on-ramp | Same | Drainers |
| OpenSea Pro | 2023-24 | ETH | ops | Fake aggregator | Same | Inferno |
| Jito | 2024-25 | SOL | ops | Fake JTO claim | Same | Drainers |
| Marinade | 2023-25 | SOL | ops | Fake MNDE | Same | Drainers |
| Kamino | 2024-26 | SOL | ops | Fake KMNO | Same | Drainers |
| Meteora | 2024-26 | SOL | ops | Fake MET | Same | Drainers |
| Raydium | 2023-26 | SOL | ops | Fake RAY | Same | Drainers |
| Orca | 2023-26 | SOL | ops | Fake ORCA | Same | Drainers |
| Drift | 2024-26 | SOL | ops | Fake DRIFT | Same | Drainers |
| Mad Lads | 2023-24 | SOL | ops | Fake mint | Same | Drainers |
| Hyperliquid airdrop | 2024-25 | HyperEVM | ops | Fake HYPE claim | Same | Drainers |
| Ethena | 2024-25 | ETH | ops | Fake ENA | Same | Drainers |
| Pendle | 2024-25 | ETH | ops | Fake PENDLE | Same | Drainers |
| Morpho | 2024-25 | ETH | ops | Fake MORPHO | Same | Drainers |
| Aavegotchi | 2022-23 | Polygon | ops | Fake GHST | Same | Inferno |
| Decentraland | 2022-23 | ETH | ops | Fake MANA | Same | Inferno |
| The Sandbox | 2022-23 | ETH | ops | Fake SAND | Same | Inferno |
| Axie Infinity | 2022-23 | Ronin | ops | Fake Axie (distinct from Ronin bridge keys) | Same | Inferno |
| STEPN | 2022-23 | SOL | ops | Fake GMT | Same | Drainers |
| Yuga / Otherside | 2022-23 | ETH | ops | Fake ApeCoin claim | Same | Inferno |
| Pudgy Penguins | 2023-25 | ETH | ops | Fake Pengu | Same | Drainers |
| Azuki | 2022-24 | ETH | ops | Fake elemental | Same | Inferno |
| CloneX | 2022-23 | ETH | ops | Fake RTFKT | Same | Inferno |
| Doodles | 2022-23 | ETH | ops | Fake DOODLE | Same | Inferno |
| Moonbirds | 2022-23 | ETH | ops | Fake PROOF | Same | Inferno |
| Worldcoin / World App | 2023-25 | OP/World | ops | Fake World ID | Same | Drainers |
| Polymarket | 2024-26 | Polygon | ops | Fake PM connect | Same | Drainers |
| Kalshi-crypto bridges | 2025-26 | Off-chain | ops | Fake prediction UX | Same | Drainers |
| Solflare verify | 2023-26 | Solana | ops | Same | Official | Drainers |
| Base airdrop | 2024-25 | Base | ops | Fake claim | Same | Drainers |
| Scroll airdrop | 2024 | Scroll | ops | Fake claim | Same | Drainers |
| Linea airdrop | 2024 | Linea | ops | Fake claim | Same | Drainers |
| Blast points | 2024 | Blast | ops | Fake claim | Same | Drainers |
| Arbitrum airdrop | 2023-24 | Arbitrum | ops | Fake claim | Same | Drainers |
| Optimism retro | 2022-24 | OP | ops | Fake claim | Same | Drainers |
| Polygon zkEVM | 2023-24 | Polygon | ops | Fake claim | Same | Drainers |
| Starknet / Braavos / Argent X | 2023-24 | Starknet | ops | Fake connect | Same | Drainers |
| dYdX | 2023-24 | Multi | ops | Fake claim | Same | Drainers |
| Hyperliquid airdrop | 2024-25 | Hyperliquid | ops | Fake claim | Same | Drainers |
| Aave / Compound / Maker / Sky | 2022-26 | ETH | ops | Fake governance / COMP / burn | Not a lending-math hunt | Drainers |
| Lido / Rocket Pool / ether.fi / Puffer / Renzo / Kelp | 2023-26 | ETH | ops | Fake restake / wrap | Permit2 | Drainers |
| Pendle / Penpie / Equilibria | 2024-26 | ETH | ops | Fake YT claim | Same | Drainers |
| Ethena / Usual / Resolv | 2024-26 | ETH | ops | Fake sUSDe claim | Same | Drainers |
| Morpho / Fluid / Euler v2 / Silo / Gearbox | 2024-26 | ETH | ops | Fake points | Same | Drainers |
| Curve / Convex / Yearn / Aura / Balancer | 2022-26 | ETH | ops | Fake CRV/airdrop | Same | Drainers |
| GMX / Gains / Kwenta / Lyra / Aevo / Vertex | 2023-26 | Arbitrum | ops | Fake perps airdrop | Same | Drainers |
| Velodrome / Aerodrome / Camelot / Ramses | 2023-26 | OP/Base/Arb | ops | Fake veAERO | Same | Drainers |
| Stargate / Across / Hop / Celer / Socket / LI.FI / Jumper | 2022-26 | Multi | ops | Fake bridge UI | WC + Permit | Inferno-class |
| Wormhole / Axelar / LayerZero scan | 2022-26 | Multi | ops | Fake airdrop | Same | Drainers |
| Magic Eden / Tensor / Blur Blend / NFTfi / BendDAO | 2022-26 | ETH/SOL | ops | Fake bid/lend | setApprovalForAll | Drainers |
| Bybit / Binance / OKX / HTX / KuCoin / Gate / Bitget KYC | 2023-26 | Multi | ops | Fake "unfreeze" / Megadrop | Never sign to "unlock CEX" | Drainers |
| USDT recovery / AML refund / OFAC unfreeze | 2022-26 | Multi | ops | Recovery narrative | No legitimate OFAC-unfreeze dapp | Drainers |
| Rainbow / Argent / MetaMask Portfolio / Snaps install | 2023-26 | EVM | ops | Fake wallet feature | Official snaps registry | Drainers |
| Jito / Kamino / Marginfi / Drift / Raydium / Orca / Meteora | 2024-26 | Solana | ops | Fake airdrop | Same | Drainers |
| Banana Gun / Maestro / Unibot / Photon / BullX / GMGN / Trojan | 2023-26 | EVM/SOL | ops | Fake bot license | Same | Drainers |
| Farcaster / Warpcast / Zora / Degen | 2024-26 | Base | ops | Fake mint | Same | Drainers |
| Polymarket / Azuro / Overtime | 2024-26 | Multi | ops | Fake claim | Same | Drainers |
| Babylon / Lombard / Solv / PumpBTC | 2024-26 | BTC/EVM | ops | Fake BTC restake | Same | Drainers |
| Keplr / IBC / Gravity | 2023-26 | Cosmos | ops | Fake connect | Same | Drainers |
| Ronin / Immutable | 2022-24 | Sidechain | ops | Fake claim | Same | Drainers |
| FTX / BlockFi email (Pink) | 2024 | ETH | ops | Stolen MailerLite lists | Email != signature auth | MetaMask Mar 2024 |

### 3.5 MetaMask Security Monthly named incidents (tayvano-adjacent org publishing)

Include when the monthly explicitly names Taylor or the wallet-phishing class she owns.

| Name | Year | Chain | Ops vs contract | What was wrongly trusted | Review seed | Sources |
|------|------|-------|-----------------|--------------------------|-------------|---------|
| OpenSea+Blockaid MetaMask warnings (4k blocked txs) | 2023 | EVM | Defense | Simulation | In-scope wallets: blocklists | MetaMask April 2023 |
| Interpol infostealer takedown | 2025 | Off-chain | **ops** (malware) | Stolen cookies/seeds | Endpoint | MetaMask June 2025 |
| Pig-butchering $225M DOJ recovery | 2025 | Multi | **ops** (investment fraud) | Romance/investment chat | Not AMM math | MetaMask June 2025 |
| Web3Auth / social recovery (Consensys acquire) | 2025 | Product | Defense | Seed backup UX | Recovery vs phishing tradeoff | MetaMask June 2025 |

---

## 4. Recurring ops classes (grep / review seeds for **in-scope product**)

Use these when the in-scope system is a **wallet, Safe UI, frontend, Permit2 integrator, WC dapp, or EIP-712 surface**. Do not paste these as Solidity contest findings against an AMM unless that AMM's UI is in scope.

| ID | Ops class | What is wrongly trusted | Grep / review seeds (product) | Local test idea (not an exploit) | Example public refs |
|----|-----------|-------------------------|-------------------------------|----------------------------------|---------------------|
| O01 | Permit2 phishing | User thinks SIWE/login | `PermitSingle`, `PermitBatch`, `permit(`, `AllowanceTransfer`, `SignatureTransfer`, spender, expiration | Wallet fixture: typed-data with attacker spender must render **red** unlimited amount | MetaMask signature-phishing help |
| O02 | EIP-2612 Permit | Gasless = harmless | `permit(address,address,uint256,uint256,uint8,bytes32,bytes32)`, `nonces`, `DOMAIN_SEPARATOR` | Same as O01 for DAI/USDC permit | Drainers |
| O03 | Unlimited ERC-20 `approve` | Max uint is "normal" | `approve(`, `type(uint256).max`, `increaseAllowance` | UI warns on max; integrator uses Permit2 with cap | MyCrypto guide |
| O04 | `setApprovalForAll` | NFT marketplace UX | `setApprovalForAll`, `isApprovedForAll` | Warn if operator is not the known marketplace | Inferno |
| O05 | Seaport / listing sig | Bulk transfer looks like list | `Seaport`, `consideration`, `offer`, `conduit` | Decode NFT going **out** at ~0 price | Group-IB seaport.js |
| O06 | WalletConnect phishing | QR = safe connection | `wc:`, session proposal, `eth_signTypedData_v4`, `personal_sign` | Session origin must match dapp domain; leftover sessions cannot request Permit2 | Inferno wallet-connect.js |
| O07 | Blind `eth_sign` | Raw hash | `eth_sign`, `personal_sign` of hex | Wallet refuses or shows "full account control" | Historical MEW |
| O08 | Address poisoning | Copied from ERC-20 transfer list | zero-value transfer, lookalike address | Explorer hides 0-value; wallet suggests address book only | Etherscan 2023 |
| O09 | Clipboard hijack | OS clipboard | clipboard listeners in malware (do not implement malware) | Hardware wallet shows full address; user must verify | MyCrypto guide |
| O10 | Google ads / typosquat | First search result | n/a (marketing) | Official domain pinning; HSTS | Scam Sniffer $4M |
| O11 | Fake revoke / fake "hack" panic | Revoke.cash clone | `increaseAllowance` disguised as revoke | Bookmark revoke.cash; simulation | Hayden Adams 2023 |
| O12 | EIP-7702 delegation | "Smart wallet upgrade" | type-4 tx, authorization tuple, `chainId=0`, CrimeEnjoyor | Render delegate implementation; test that unknown delegate is blocked | tayvano / MetaMask June 2025 |
| O13 | Safe UI / malicious `DELEGATECALL` | HTML vs hardware screen | Safe Transaction Service, `operation: 1`, `DELEGATECALL`, `setup`, singleton | **Hardware must show** operation type and `to`; Tenderly on the **same** payload the device hashes | Bybit, WazirX, Radiant Oct |
| O14 | Signer malware (INLETDRIFT class) | PDF/ZIP from Telegram | n/a in Solidity | Air-gapped / dedicated signing laptops; no Telegram on signer OS | Radiant Mandiant |
| O15 | Contagious Interview | Recruiter coding test | n/a | Deployer keys on a machine that never opens attachments | Tapioca cluster |
| O16 | Fake job / DPRK IT worker | Remote-hire identity | payroll in USDT | HR + wallet segregation | DOJ / MetaMask June 2025 |
| O17 | Supply-chain npm (Connect Kit) | `package.json` tag | `ledgerhq`, `connect-kit`, lockfile pin | CI pins hash; runtime SRI | Ledger Dec 2023 |
| O18 | Compromised frontend (Badger, CMC, Cointelegraph) | HTTPS site JS | script integrity, CSP | CSP + SRI; treat CMS XSS as in-scope | Badger; MetaMask June 2025 |
| O19 | Discord verify bot | Collab.Land clone | bot application ID | Pin official bot | Inferno Discord |
| O20 | X/Twitter ATO + passkeys | Hijacked brand account | n/a | Project: hardware keys for social; users: don't click | WallStreetBets / ZachXBT Dec 2024 |
| O21 | SIM swap / fake exchange support | Phone 2FA | SMS OTP | Hardware keys (Yubi); exchange: no reset via SIM | Genesis $243M |
| O22 | Remote desktop "support" | AnyDesk/TeamViewer | n/a | Support must never ask RDP | Genesis $243M |
| O23 | Seed in photos / OCR malware | Camera roll | n/a | Never screenshot SRP | SparkKitty |
| O24 | Seed in password manager | 1Password/LastPass vault | n/a | tayvano: don't | Blockfence interview |
| O25 | Infostealer (RedLine, Atomic, Raccoon) | Desktop malware | n/a | Separate signing machine | ZachXBT KuCoin Atomic stealer case |
| O26 | Pig butchering | Investment chat | n/a | Not a contract review | DOJ $225M |
| O27 | Rug / paid KOL | Vesting + mint authority | `mint`, `owner`, LP lock | If mint is privileged, it's **ops+auth**; still grep `onlyOwner mint` | BitBoy, Ansem, LAB |
| O28 | CEX hot wallet | Operator keys | n/a | PoR, cold threshold; not ERC-20 | Stake, CoinEx, Atomic (server), M2 |
| O29 | Bridge guardian keys | 5-of-9 social graph | `submitRoot`, validator set | Threshold + ceremony; Harmony/Ronin | FBI |
| O30 | Cloud DB secrets (Mixin) | AWS/GCP dump | secrets in SQL | No keys in cloud SQL | SlowMist Mixin |
| O31 | Instant-exchange cashout | Swap shop KYC | n/a | LE freeze path | ZachXBT eXch, HitBTC, FixedFloat mentions |
| O32 | Nested CEX mule KYC | Purchased identities | n/a | KuCoin alerts | t.me/investigations/348 |
| O33 | 7702 + ERC-4337 bundler | Stolen key + automation | EntryPoint, UserOp | If key is stolen, 7702 only speeds drain - don't "fix Pectra" | CrimeEnjoyor |
| O34 | Fake hardware-wallet firmware | Sideload live | n/a | Official Live only | Ledger phishing |
| O35 | WC v2 leftover session | Session still live | disconnect | Auto-expire sessions | Product review |
| O36 | Permit2 allowance leftover | Old Permit2 grant | `allowance(owner,token,spender)` on Permit2 | In-app revoke; expiry | Integrators |
| O37 | `PermitBatch` multi-token | One sig many tokens | `PermitBatch` | UI lists **each** token | Drainers |
| O38 | DAI `permit` vs USDC `permit` vs Permit2 | Mixed allowance systems | three different spenders | Document which pipeline the dapp uses | Wallet UX |
| O39 | `increaseAllowance` phishing | Looks like revoke | `increaseAllowance` | Decode | Fake revoke sites |
| O40 | EIP-712 domain separator mixup | Wrong verifyingContract | `EIP712Domain`, `verifyingContract` | Test that a Uniswap Permit2 domain cannot be used as "login" to your app | Wallets |
| O41 | `chainId=0` 7702 | Cross-chain replay-like auth | authorization `chainId` | Reject unless documented | Research papers 2025 |
| O42 | Safe off-chain vs on-chain hash | Sign what HTML shows | `safeTxHash` | Device displays `safeTxHash` matching independently computed hash | Bybit family |
| O43 | Malicious Safe module / guard | Module looks like helper | `enableModule`, `setGuard` | Hardware shows module address | Ops signing |
| O44 | Compromised Transaction Service | Cloud API | Safe client gateway | Pin gateway; compare hash locally | Bybit-class |
| O45 | Hardware wallet that doesn't clear-sign ERC-20 | Screen shows "blind" | firmware | In-scope HW: ERC-20 full decode | ZachXBT HW skepticism (Telegram 355) is opinion, not a CVE |
| O46 | Airdrop claim site | Eligibility checker | `claim(`, merkle | Official domain only; merkle is fine if site is honest | Inferno |
| O47 | Fake CertiK/audit badge | PNG on phish site | n/a | Don't trust badges | Cointelegraph CTG popup |
| O48 | ENS / DNS hijack of dapp | DNS registrar | n/a | DNSSEC; monitor | Historical |
| O49 | Browser extension clone | Chrome store | extension ID | Pin ID in docs | Fake MetaMask |
| O50 | Mobile overlay / accessibility | Android permission | n/a | Crocodilus class | MetaMask June 2025 |

---

## 5. Gaps (could not verify in this pass)

- **Fire.xyz blog:** no durable public blog matching Taylor Monahan was retrieved. Do not cite fire.xyz as a primary corpus until a URL is confirmed. Her public corpus here is MyCrypto Medium, MetaMask Security Monthly, X `@tayvano_`, GitHub `tayvano`, talks, Blockspace DPRK podcast.
- **`@tayvano` vs `@tayvano_`:** user-facing docs should mention both. Current posts used in MetaMask embeds are `@tayvano_`.
- **ZachXBT Mixin primary:** Telegram public preview [t.me/s/investigations?before=54](https://t.me/s/investigations?before=54) now cited (announcement + SlowMist h/t). Exact `t.me/investigations/<id>` numeric permalink not captured. Mixin itself was not FBI-attributed in the snippets used here.
- **ZachXBT Substack/Mirror:** not found as a large public archive. Primary long-form: Telegram + X + occasional Medium (`investigationsbyzachxbt`). [zachxbt.tech](https://www.zachxbt.tech/) and [zachxbt.live](https://zachxbt.live/) are thin marketing pages, not a case database.
- **Telegram completeness:** recent channel head, `?before=189`, and 2023-24 pages `?before=54/70/90/110/130` were scraped. The channel has 300+ posts; 2021-early-2023 NFT rugs and many one-off victim traces are **not fully enumerated**. Treat section 2.6 named NFT rugs as "widely associated" pending thread URLs.
- **Rekt.news:** used as confirming culture for protocol bugs (Euler, etc.); ZachXBT's own Rekt author pages were not scraped this pass.
- **Chainalysis / Elliptic / TRM full Lazarus ledgers:** used as confirmers (Stake, Harmony, CoinEx, Bybit). Their PDF threat reports were not ingested row-by-row.
- **Slope 2022, Orbit Bridge, Multichain 2023, Heco details, Nomad, Poly Network:** famous public incidents; ZachXBT **may** have traced funds; primary threads not confirmed here. Do not list them as "ZachXBT cases" without a URL.
- **Radiant January 2024 contract class:** CoinDesk notes a prior $4.5M **contract** exploit; this file does not re-derive the exact Solidity class (likely rounding / market). Separate from October **ops**.
- **TeleSwap 2026:** ZachXBT flagged outflows; root cause (key vs contract) unpublished in the Telegram post.
- **Hundreds of <$2k drains 2026:** campaign flagged; vector not in the news snippet.
- **Axiom 2026:** verified via CoinDesk/Decrypt (Broox Bauer / internal dashboards). Still no primary X URL archived in this file.
- **Individual victim cases:** ZachXBT publishes many one-wallet drains that are **ops** but unnamed. This ingest prefers **named** incidents. Hundreds of unnamed Telegram victim posts exist and were not copied (PII).
- **X search API:** live `x.com/zachxbt` search was not exhaustively crawled (rate limits / JS). Prefer Telegram + reputable reprints.
- **Firecrawl MCP:** hit free rate limit mid-pass; later searches used general web search.

---

## 6. Source index (primary)

**ZachXBT:** [x.com/zachxbt](https://x.com/zachxbt) · [t.me/s/investigations](https://t.me/s/investigations) · [en.wikipedia.org/wiki/ZachXBT](https://en.wikipedia.org/wiki/ZachXBT) · [Wired](https://www.wired.com/story/meet-zachxbt-243-million-crypto-theft/) · [CoinDesk profile](https://www.coindesk.com/tech/2024/12/10/zach-xbt-master-sleuth-of-crypto) · [CMC Academy](https://coinmarketcap.com/academy/article/who-is-zachxbt-cryptos-anonymous-scam-hunter) · [CCN](https://www.ccn.com/education/crypto/zachxbt-crypto-detective-uncovering-blockchain-scams/) · FBI Stake / Harmony / Bybit confirmers · TRM Beacon / USMS blog · SlowMist Bybit · Elliptic Lazarus tactics / Horizon / Mixin / CoinEx

**tayvano:** [x.com/tayvano_](https://x.com/tayvano_) · [medium.com/@tayvano](https://medium.com/@tayvano) · [MyCrypto security guide](https://medium.com/mycrypto/mycryptos-security-guide-for-dummies-and-smart-people-too-ab178299c82e) · [MetaMask April 2023](https://metamask.io/news/metamask-security-monthly-april-2023) · [MetaMask March 2024](https://metamask.io/news/metamask-security-monthly-march-2024) · [MetaMask June 2025](https://metamask.io/news/metamask-security-report-june-2025) · [MetaMask signature phishing](https://support.metamask.io/privacy-and-security/staying-safe-in-web3/signature-phishing/) · [Group-IB Inferno](https://www.group-ib.com/blog/inferno-drainer/) · [github.com/tayvano](https://github.com/tayvano)

**Method note:** compiled 2026-08-26 from public web, Telegram public previews, MetaMask/MyCrypto posts, FBI/Elliptic/TRM/SlowMist, and Wikipedia. Whitehat ingest only.

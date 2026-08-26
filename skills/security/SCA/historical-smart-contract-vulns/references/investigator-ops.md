# Investigator ops (@zachxbt + @tayvano_)

This slice is the **ops vs code** discriminator. Most dollar losses these two cover are **not** Solidity/Anchor bugs. Load this **before** `reviewing-erc4626-and-vaults` whenever the user names a "hack" that looks like Bybit, a drainer, phishing, a stolen key, or a Lazarus exchange hit.

Do **not** also load `x-and-public-signal.md` in the same turn. One slice.

Lawful/whitehat only. This is a classification library, not an IR runbook, not a drainer cookbook, not a doxxing file. Cite public writeups. Never reconstruct unpublished 0-days. Never paste drainer JS.

## Why this slice exists

Industry mix (Immunefi scoreboard + SlowMist): protocol-logic losses fell while **custodial / phishing / key / UI** losses exploded. Bybit Feb 2025 ~$1.5B is larger than almost every Solidity class in `named-losses.md` combined for that year. Agents that grep `onlyOwner` on a Safe-UI supply-chain incident waste the review.

`@zachxbt` traces **where the money went** and who operated the wallets.
`@tayvano_` (Taylor Monahan; Fire / MyCrypto / MetaMask security; SEAL-911 volunteer) traces **what the victim signed** and which wallet-UX lie produced that signature.

They collaborate. Wikipedia quotes tayvano_ on zachxbt's published findings increasingly leading to arrests. Treat their X threads as **pointers**. Confirm class on FBI / Sygnia / protocol / Rekt / SEAL notes before filing.

Handles (public):

| Who | X | Longer form | Site |
|-----|---|-------------|------|
| ZachXBT | [@zachxbt](https://x.com/zachxbt) | [t.me/investigations](https://t.me/investigations) | [zachxbt.live](http://zachxbt.live/) |
| tayvano_ | [@tayvano_](https://x.com/tayvano_) | SEAL-911 [t.me/seal_911_bot](https://t.me/seal_911_bot) | MetaMask / Fire product security |

200+ zachxbt investigations exist. Wikipedia and CoinMarketCap only name a handful. This file is the **packaged** catalog (headline cases + compact extra table + tayvano families). The full named Telegram/X ingest is `_ingest/zachxbt-tayvano.md` (optional; not `include_str`). Gaps are listed at the bottom. Do not invent rows.

## Hard rule (read this twice)

| If the public story is... | Taxonomy | Next skill | Do **not** |
|---------------------------|----------|------------|------------|
| Signers approved a tx that was not what the UI showed (Safe / Liminal / malicious JS) | OPS01 / OPS18 | `reviewing-frontend-and-ops-surfaces` | File as Ethereum consensus, vault inflation, or `onlyOwner` missing |
| Victim signed Permit / Permit2 / `setApprovalForAll` on a fake site | OPS02 / OPS03 / OPS13 | `reviewing-frontend-and-ops-surfaces` + `reviewing-signatures-permit-and-eip712` | File as the impersonated protocol's vault bug |
| Malware on a **signer workstation** swapped calldata (Radiant) | OPS17 | ops / key-ceremony review | File as Aave-fork liquidation math (unless a second incident is actually math) |
| Validator / exchange hot-wallet keys stolen | OPS10 | custody / client, not AMM | Call it a "bridge bug" without a message-decoder finding |
| Social engineering + RDP / 2FA reset (Genesis creditor) | OPS06 | product support + wallet UX | Hunt Bitcoin Script |
| LastPass / password-manager dump -> keys | OPS08 | secrets hygiene | Hunt XRP ledger consensus |
| Insider of a **custodian contractor** | OPS16 | ops / vendor review | Hunt USDC token contract |
| Issuer freeze lag on CCTP after a **separate** exploit | OPS15 | stablecoin / IR policy; Drift code is a different card | Blame Circle for Drift's oracle/ops bug or vice versa |
| Rug, paid shill, insider dump, staged mint | OPS11 | tokenomics / disclosure | Invent a reentrancy |
| DPRK fake Zoom / fake IT worker | OPS07 | org security | File as a Solidity finding |

If you cannot name an OPS id **or** a T/S/M/C/K/B/O id from `taxonomy.md`, you do not have a case.

## OPS taxonomy

Grep seeds are **review seeds** for in-scope frontends, wallets, Safe modules, Permit2 integrations, and signer runbooks. They are not exploit recipes.

| ID | Class | Wrongly trusted | Review seeds (in-scope products) | Analog |
|----|-------|-----------------|----------------------------------|--------|
| OPS01 | Safe / multisig **UI** lie | What the signer sees is the calldata they sign | Safe tx simulation vs decoded `to`/`data`/`value`; Liminal/custody display vs raw; hardware wallet clear-sign | **Bybit 2025**; WazirX 2024 (Liminal display class) |
| OPS02 | Drainer-as-a-service | "Connect wallet" + one typed-data popup is a mint/claim | Simulation of `transferFrom` / batch after signature; domain separator vs UI copy | Inferno, Pink, Angel, Monkey, Venom |
| OPS03 | Permit / Permit2 phishing | Off-chain EIP-712 is the same as an on-chain swap | Permit2 `PermitSingle`/`PermitBatch` spender, amount=`type(uint160).max`, `sigDeadline`; leftover Permit2 allowance | tayvano_ / Scam Sniffer dominant 2024-2026 vector |
| OPS04 | WalletConnect / session | WC session is the real dapp | Session origin, chain id, method allowlist, disconnect on navigation | Fake WC connect pages |
| OPS05 | Address poison / clipboard | Last-seen address is the intended payee | Poisoning of similar checksums; clipboard malware | Retail drains; tayvano_ warnings |
| OPS06 | Support impersonation + RDP / 2FA | Google/Gemini/exchange "support" is support | Out-of-band verify; never AnyDesk/TeamViewer on key machines | **Genesis creditor 2024 ~4064 BTC** |
| OPS07 | DPRK fake IT worker / fake Zoom | Recruiter + "install this" is a job | Device image, no seed on work laptop | tayvano_ [1999942459368104073](https://x.com/tayvano_/status/1999942459368104073); 2026 IT-worker network |
| OPS08 | Password-manager downstream | LastPass vault is an HSM | Keys never in a consumer password manager | LastPass 2022 breach -> 2023-2025 thefts; **Chris Larsen XRP** |
| OPS09 | Hardware-wallet blind sign / fake support | Device screen matches intent; "Ledger support" is Ledger | Clear-signing; official support URLs only | Tiffany Milanovich-class CEX/HW support impersonation (~$5M, zachxbt X); Atomic Wallet 2023 client |
| OPS10 | Exchange / bridge **keys** | n-of-m actually needs n honest, online, unphished signers | Key ceremony, airgap, no shared Windows box | Ronin 2022; Harmony Horizon 2022; DMM Bitcoin 2024; Phemex; BingX; Poloniex; linked by zachxbt to **same Lazarus cluster** as Bybit |
| OPS11 | Rug / shill / insider tokenomics | Team + KOL is not a conservation law | Vesting, mint authority, LP lock, paid promo disclosure | BitBoy paid shill; Logan Paul / CryptoZoo; Ansem memecoin pumps; Pixelmon / Impact Theory / Rogue Society NFT; RAVE/MemeCore/LAB 2026; Humanityprot alleged staged mint |
| OPS12 | SIM swap | SMS 2FA is the owner | Hardware 2FA; exchange withdrawal whitelist | Classic CEX retail |
| OPS13 | NFT phishing / `setApprovalForAll` | "Animate my BAYC" site is the marketplace | Approval UI; revoke; operator allowlist | **BAYC 2021 ~$2.5M**, French arrests |
| OPS14 | Leftover infinite approve | Old spender is revoked | Allowance table; Permit2 `lockdown` | Every drainer aftermath |
| OPS15 | Issuer freeze / CCTP lag | USDC blacklist will save the protocol in minutes | IR runbook vs issuer policy; CCTP burn/mint windows | **Circle Files 2026** (~$420M alleged across 15 cases); Drift CCTP ~$232M / 6h unfrozen |
| OPS16 | Custodian **contractor insider** | Vendor that holds seized/user funds is the principal | Vendor ACL, dual control, no family-laptop keys | **USMS seized-crypto 2026 ~$46M** (FBI arrest, handle Lick) |
| OPS17 | Signer-machine malware | Hardware wallet confirms what the **infected host** displayed | Airgap; simulate on a second machine; Safe hash independently | **Radiant Capital II 2024 ~$50-53M** |
| OPS18 | Supply-chain JS on the **wallet UI** | app.safe.global / wallet frontend is the signed binary you think | SRI, review deploy of Safe Web, extension store | Bybit: Sygnia Safe{Wallet} JS; not ETH consensus |
| OPS19 | Adapter / oracle **integration** (mixed) | UMA/CTF adapter cannot be gamed as "the market" | Adapter auth + resolution; still may be T05 | Polymarket UMA CTF adapter ~$520k (zachxbt-adjacent public coverage) |
| OPS20 | Dust / taunt / secondary | Dust to an investigator is not a protocol drain | Ignore; do not chase dust as the root cause | USMS case dust-to-zachxbt |

In-scope product audits (wallets, Safe modules, Permit2 apps, dapp frontends): playbook `reviewing-frontend-and-ops-surfaces`.

## @zachxbt named incidents

Amounts are **public reporting**, not Immunefi payouts. Class is ops unless noted. Confirm `$` on Rekt / FBI / protocol blog.

### Lazarus / exchange / Safe-UI cluster (do not call these Solidity)

| Incident | When | Public $ (approx) | OPS | What zachxbt did | Confirm |
|----------|------|-------------------|-----|------------------|---------|
| Ronin bridge (Axie) | 2022-03 | ~$624M | OPS10 | Laundering / Lazarus pattern in later cluster posts | [rekt](https://rekt.news/ronin-rekt) |
| Harmony Horizon | 2022 | ~$100M | OPS10 | Lazarus tracing; **2026:** publicly refused unpaid follow-up on a later Harmony incident | [rekt](https://rekt.news/harmony-rekt) |
| Atomic Wallet | 2023-06 | ~$100M / 4100+ addresses | OPS09 / client | [X](https://x.com/zachxbt/status/1666115739764285445); Lazarus-like laundering | TRM / public |
| WazirX | 2024-07 | ~$230-235M | OPS01 / OPS10 | Traced from exploiter; Tornado test txs; Arkham KYC-deposit bounty; Lazarus markings | [Quill](https://quillaudits.medium.com/another-lazarus-group-attack-decoding-wazirx-multisig-wallets-235m-exploit-8a1218a88593); [archive thread](https://archive.md/3Ct1v) |
| Radiant Capital II | 2024-10 | ~$50-53M | OPS17 | Industry + zachxbt cluster: malware on signer PCs, 3-of-11 Safe, `upgradeAndCall` not Aave math | [named-losses](named-losses.md); [guide](https://smartcontractaudit.com/guides/radiant-capital-2024-multisig-hack) |
| DMM Bitcoin | 2024 | ~$305M BTC | OPS10 | Exchange keys, **not** Bitcoin Script | TRM 2024 DPRK totals |
| Phemex | 2024-25 | exchange | OPS10 | On-chain habit match later used for **Bybit attribution** | zachxbt X; FBI later confirmed Lazarus on Bybit |
| BingX | 2024-25 | exchange | OPS10 | Same Lazarus cluster as Bybit/Phemex/Poloniex | [PANews](https://www.panewslab.com/en/articles/74ld127b) |
| Poloniex | 2023-25 | exchange | OPS10 | Address-linked to Bybit exploiter (zachxbt Feb 2025) | PANews |
| **Bybit** | **2025-02-21** | **~$1.5B ETH-related** | **OPS01 + OPS18** | Hours-later Lazarus attribution (test txs, Phemex graph); 920+ addresses; Arkham bounty; FBI [TraderTraitor / Lazarus](https://www.fbi.gov/investigate/cyber/alerts/2025/north-korea-responsible-for-1-5-billion-bybit-hack) | Sygnia: Safe{Wallet} JS, not Bybit ETH consensus |

Bybit is the **canonical** Safe-UI supply-chain card. Signers thought they were moving funds as usual. The UI showed a benign tx. The signed payload was not. Do not file "Bybit hack" as `reviewing-erc4626-and-vaults`.

### Social engineering, insider, password manager

| Incident | When | Public $ | OPS | Notes | Confirm |
|----------|------|----------|-----|-------|---------|
| BAYC "animate my ape" phishing | 2021 | ~$2.5M NFTs | OPS13 | Five-person ring; French arrests/convictions | [CMC profile](https://coinmarketcap.com/academy/article/who-is-zachxbt-cryptos-anonymous-scam-hunter); Wikipedia |
| Genesis creditor / Gemini | 2024-08 | ~4064 BTC / ~$243M | OPS06 | Google+Gemini support impersonation, 2FA reset, RDP, Bitcoin Core keys; DOJ arrests | [NYT Mag](https://www.nytimes.com/2025/04/24/magazine/crybercrime-crypto-minecraft.html); [The Block](https://www.theblock.co/post/317349/blockchain-sleuth-zachxbt-alleges-three-perps-involved-in-243-million-theft-of-single-genesis-creditor) |
| LastPass threat actor (many victims) | 2023-25 | many wallets; $4.4M / $5.36M / $6.2M slices reported | OPS08 | Consumer password manager != HSM | The Block LastPass series |
| Chris Larsen / XRP | 2025 | ~$100-150M XRP (reporting range) | OPS08 | zachxbt: keys in LastPass-class manager; forfeiture complaint | [The Block](https://www.theblock.co/news/regulation/2025-03-07-ripple-co-founder-chris-larsen-losing-over-100-million-of-xrp-tied-to-lastpass-hack-says-zachxbt-345212) |
| USMS seized-crypto / "Lick" | 2026-01..03 | ~$46M | OPS16 | Contractor-insider of seized-asset custody; FBI + French Gendarmerie arrest St. Martin | [CoinDesk](https://www.coindesk.com/business/2026/03/05/son-of-u-s-government-contractor-accused-of-stealing-millions-in-seized-crypto-arrested-in-france); [TRM](https://www.trmlabs.com/resources/blog/zachxbt-uncovers-crypto-theft-network-linked-to-us-government-seizure-funds) |
| HW/CEX support impersonation (Tiffany Milanovich) | 2026 (X) | >=~$5M attributed | OPS09 | zachxbt X profile recent: hardware wallet + CEX support scams | [x.com/zachxbt](https://x.com/zachxbt) |

### Drift, Circle Files, freeze policy (ops **response**, not the root drain)

| Incident | When | Public $ | OPS | Notes | Confirm |
|----------|------|----------|-----|-------|---------|
| Drift Protocol | 2026-04-01 (news); some tables say 2025 | ~$280-285M protocol; ~$232M USDC via CCTP | S10 + OPS15 | Protocol exploit/ops+oracle is **not** "Circle wrote a bug". zachxbt: 100+ CCTP txs over ~6h unfrozen during US hours | [crypto.news](https://crypto.news/circle-faces-heat-from-zachxbt-over-inaction/); `named-losses.md` Drift row |
| Circle Files | 2026-04 | ~$420M alleged freeze failures across **15** cases since 2022 | OPS15 | Also: Circle froze 16 **legitimate** business wallets ~2026-03 (incl. DFINITY ckETH minter), later unfrozen | [CoinGape](https://coingape.com/zachxbt-releases-circle-files-alleges-usdc-issuer-compliance-failures-across-multiple-crypto-hacks/); [Bitcoin.com](https://news.bitcoin.com/usdc-freeze-controversy-zachxbt-says-circle-froze-16-legitimate-wallets-missed-real-hacks/) |

When auditing **Drift program source**: use `reviewing-solana-programs` + S10. When the user asks "why wasn't USDC frozen": OPS15, not an Anchor missing-signer.

### Influencer, NFT rug, alleged treasury, market structure

These are **not** contest-style smart-contract findings unless a second source shows a code bug.

| Incident | When | OPS | Notes | Confirm |
|----------|------|-----|-------|---------|
| BitBoy (Ben Armstrong) paid shill packages | 2022-24 | OPS11 | $2.5k-$40k promo; projects later rugged | CMC profile |
| Logan Paul pumps / CryptoZoo | 2021-23 | OPS11 | ElonGate, DinkDoink, NFT game stall | CMC profile |
| Pixelmon, Impact Theory, Rogue Society | 2021-22 | OPS11 | NFT rug / delivery failure class | public X-era coverage |
| Machi Big Brother / Formosa Financial | 2022-23 | OPS11 | Alleged 22k of 44k ETH ICO treasury; **defamation suit dropped** after wording edits; $1M+ defense fund then returned | [Medium](https://medium.com/@investigationsbyzachxbt/a-story-of-machi-big-brother-jeff-huang-a1ad073fcfa8); CoinDesk defense fund |
| Ansem / KOL memecoin pumps | 2024 | OPS11 | Public accusation of pump-and-dump promotion | Yahoo/CMC citations |
| $GROK / Worldcoin / Hayes-adjacent market threads | various | OPS11 | Market/influencer, not a compiler bug | X |
| Axiom insider-trading tease / Polymarket leak | 2026-02 | OPS11 | Front-end/CEX staff alleged misuse of order flow; Polymarket bet leakage **before** the thread | Phemex/Yahoo/BingX news |
| RAVE / MemeCore / LAB | 2026 | OPS11 | Insider/tokenomics, **not** a code bug | X / Telegram investigations |
| Humanityprot H | | OPS11 | Possibly staged mint (treat as unconfirmed until a postmortem) | X |
| THORChain multi-chain pause | | mixed | Protocol pause after >$10M alleged; may have a code card elsewhere | confirm Rekt |
| Polymarket UMA CTF adapter | Polygon | OPS19 / T05 | ~$520k adapter/integration; **this one can be code** | confirm contest/postmortem |
| DSJ/BG Ponzi, JuCoin reserves, bucket shops | | OPS11 / OPS10 | Exchange/ops fraud | Telegram investigations |
| DJT token creator bounty | | OPS11 | Arkham bounty; Martin Shkreli attribution (Arkham accepted) | Protos |
| Brazilian IT-worker ~$100M cyber theft (Telegram tracking) | 2025 | OPS07 / OPS10 | The Record: zachxbt tracking conversions | The Record 2025-07-07 |

### Methods (so agents know what a zachxbt thread **is**)

Public methods: fund-flow graphs, address clustering, test-tx timing before the big steal, OSINT (domains, court, Discord leaks), Telegram/Discord criminal chat monitoring, Beacon Network (TRM, 2025) for freeze coordination.

Paradigm incident-response advisor (2025-). Wired: most prolific independent crypto detective. CoinDesk Most Influential 2024.

A zachxbt thread that **only** shows mixer hops is **not** a Foundry test. Promote to a T/S id only when a decoder/oracle/invariant broke.

## @tayvano_ campaign families (not one protocol)

tayvano_ does not usually publish "Protocol X lost $Y to reentrancy." She publishes **how victims were lied to**. Catalog families so you map a user report onto OPS02-OPS05/OPS07/OPS09 instead of a random vault.

Background (public): co-founded MyEtherWallet (2015), founded MyCrypto (2018), MetaMask/ConsenSys after 2022 acquisition; Fire (phishing/drainer intel); SEAL-911 volunteer ([ACTIVITY](https://github.com/security-alliance/seal-911)). Frequent collaborator with zachxbt on **victim totals**.

### Drainer kits (DaaS)

Commercial kits sold to affiliates. Sites clone MetaMask, OpenSea, Coinbase, mints, claims. After `eth_requestAccounts` the kit fingerprints the wallet and picks a payload.

| Kit | Active (public) | Stolen (public ranges) | Typical payload | Status (public claims) |
|-----|-----------------|------------------------|-----------------|------------------------|
| Monkey Drainer | 2022-23 | ~$13M | Fake KOL + NFT mint | Shut down / succeeded by others |
| Inferno Drainer | 2022-25 (reload) | ~$80-88M first wave; operators claimed $250M+ including covert | Approve + Permit/Permit2 + `transferFrom`; 100-229 brand clones; later 28 chains | "Shutdown" then reload (Checkpoint 2025); transfer claims toward Angel |
| Pink Drainer | 2023-24 | ~$85M; NFT single-victim records (BAYC/MAYC) | Discord-token theft + drain; email campaigns (FTX/BlockFi users, MetaMask Mar 2024 note ~$7M) | Announced exit |
| Angel Drainer | 2023-26 | $25M+ then successor | English/Russian sales; Claim flow: Approve + Permit/Permit2 + `transferFrom` | Active successor class |
| Venom / unnamed forks | 2024-present | combined $100M+ cited | Same six-stage anatomy | Active |

Industry: Scam Sniffer ~$295.5M wallet-drainer category in 2023 (Inferno majority). SEAL/tayvano_ public remarks: **>$250M stolen via drainers since Dec 2022** (her posts; treat as campaign total, not one tx).

Checkpoint 2025 Inferno reload log types include `connect_but_seed_prompt` (OPS09 / EIP-7702-era UX). tayvano_ 2026-08-22: **never turn on Chrome dev mode** on a daily driver ([2091233406382440804](https://x.com/tayvano_/status/2091233406382440804)).

### Signature / UX families (map to playbook)

| Family | What the wallet showed vs what was signed | OPS | In-scope review |
|--------|-------------------------------------------|-----|-----------------|
| ERC-20 `approve` / `increaseAllowance` unlimited | "Enable USDC" -> spender is the drainer | OPS14 | Allowance UI; default cap |
| `setApprovalForAll` | "List NFT" / "animate" | OPS13 | Operator allowlist |
| ERC-2612 Permit | Gasless "sign to claim" | OPS03 | Domain, spender, value, deadline |
| Uniswap Permit2 `PermitSingle` / `PermitBatch` | One popup, many tokens, `uint160.max` | OPS03 | Permit2 at `0x000000000022D473030F116dDEE9F6B43aC78BA3`; leftover allowance after "disconnect" |
| Raw `eth_sign` / undecodable digest | Hex blob | OPS09 | MetaMask MIP-3: blind sign of arbitrary data |
| WalletConnect to a clone | Connect looks like Uniswap | OPS04 | Session origin |
| Fake MetaMask / fake Ledger Live | Install from ads | OPS09 | Official download paths |
| Address poison | Copy-paste last address | OPS05 | Checksum warnings |
| Clipboard malware | Paste is attacker | OPS05 | OS hygiene |
| EIP-7702 / account abstraction "seed prompt" | Connect then "enter seed to upgrade" | OPS09 / T40 adjacent | Never seed in a webpage |
| Clear-signing gap | Device says Swap, calldata is Permit2 | OPS01 / OPS09 | Hardware clear-sign + simulation |

Permit2 is **T10-shaped on the frontend**. The Uniswap Permit2 **contracts** may be in-scope in a contest; the phishing site is not a vault bug on the victim protocol.

### Org / DPRK

tayvano_ warns on fake recruiters, fake Zoom, "install our security agent." Same bucket as zachxbt's IT-worker network coverage (OPS07). Not a smart-contract class.

## Review seeds for **in-scope** product audits

Use when the repo is a wallet, Safe module, Permit2 integration, dapp frontend, or signer runbook you are paid to review.

1. **EIP-712**: `name`, `version`, `chainId`, `verifyingContract` equal what the UI string says. Cross-chain replay if `chainId` omitted.
2. **Permit2**: spender != the documented router; amount max; `sigDeadline` far future; batch includes tokens the UI did not list.
3. **Simulation**: Safe / wallet simulation equals decoded calldata (Bybit class). A second machine must hash the same Safe tx.
4. **Approvals**: leftover `allowance` and Permit2 allowances after "done"; revoke path exists and is documented.
5. **WC / injected provider**: origin binding; no silent `eth_signTypedData` on navigation.
6. **Signer malware**: documented airgap; "what Ledger showed" vs host Electron app (Radiant class).
7. **Do not** report Bybit/WazirX/Radiant-malware as `reviewing-erc4626-and-vaults`.

Local tests: Foundry `vm.sign` for permit domain mismatch; frontend e2e that a simulated Permit2 batch is visible in copy. No live drain.

## More named public cases (compact)

The 2026-08-26 ingest (`_ingest/zachxbt-tayvano.md`) names **hundreds** of Telegram/X rows. This table is the rest of the **high-signal named** ones not already in the tables above. Same rule: ops unless a second source says code.

| Name | Year | OPS / note | Confirm |
|------|------|------------|---------|
| Stake.com | 2023 | OPS10; FBI Lazarus ~$41M; zachxbt size+lean | [FBI](https://www.fbi.gov/news/press-releases/fbi-identifies-lazarus-group-cyber-actors-as-responsible-for-theft-of-41-million-from-stakecom) |
| Alphapo / CoinsPaid (Jul 2023 + Jan 2024) | 2023-24 | OPS10 processor keys | FBI Stake release; Telegram |
| CoinEx | 2023 | OPS10; traced to Stake Lazarus cluster | Elliptic; Telegram |
| Mixin Network | 2023 | OPS10 cloud-DB keys ~$200M; not Mixin VM | SlowMist / The Block |
| HTX / Huobi + Heco | 2023 | OPS10 / mixed chain control | public |
| Poloniex | 2023 | OPS10 hot wallet | Halborn / CoinDesk |
| BitoPro | 2025 | OPS10 AWS tokens during hot-wallet update | MetaMask Jun 2025 |
| CoinSpot / Rain / M2 / Metawin | 2023-24 | OPS10 CEX/casino hot wallets | Telegram investigations |
| Velodrome + Aerodrome frontends (twice) | 2023 | OPS18 DNS/Porkbun SE; not veAERO math | Velodrome X |
| Across Discord vanity `gg/across` | 2023 | Discord invite ~$880k | Telegram |
| BadgerDAO 2021 | 2021 | OPS18 Cloudflare/frontend | public postmortem |
| Ledger Connect Kit npm | 2023 | OPS18 + OPS02 Angel | public |
| CoinMarketCap / Cointelegraph JS injectors | 2025 | OPS18 trusted-domain XSS | MetaMask Jun 2025 |
| Contagious Interview cluster (Tapioca, Nexera, Concentric, Masa, SpaceCatch, Reach, Serenity Shield, MurAll) | 2024 | OPS07 recruiter ZIP | [t.me/investigations/167](https://t.me/investigations/167); Unit 42 |
| $68M WBTC address poisoning | 2024 | OPS05 | Etherscan/press |
| stETH/rETH ~$24.2M phish; Ether.fi weETH ~$6.9M; LINK ~$4.4M | 2023-24 | OPS02/03 | Telegram |
| CrimeEnjoyor / EIP-7702 | 2025 | OPS09; tayvano: stolen key, **not** Pectra | MetaMask Jun 2025 |
| Venom Drainer | 2023 | OPS02 ~$27M / ~15k victims | MetaMask Apr 2023 |
| GALA privileged mint ~5B | 2024 | **T02** minter role, then ops dump | not Uniswap |
| Radiant **Jan** 2024 ~$4.5M | 2024 | **T14** lending math; do **not** merge with Oct OPS17 | CoinDesk |
| Wintermute 2022 | 2022 | OPS09 Profanity vanity key | postmortem |
| Slope 2022 | 2022 | OPS09 telemetry/seeds | public |
| Humanity Protocol laptop multisig | 2026 | OPS17 one laptop; zachxbt overlap with Kelp proceeds | CoinDesk |
| Kelp DAO 2026 | 2026 | zachxbt: ops/infra/Lazarus lean; this pack's protocol card remains **T20** until a postmortem says otherwise | Chainalysis + `incident-timeline.md` |
| Serial fake-lending rugs (Glori, Sorta, Magnate, Kokomo, Solfire, Crolend, HellhoundFi, Lendora, HashDAO, Leaper, Zebra) | 2023-24 | OPS11 | Telegram / CryptoScamsWiki |
| Frosties / Evolved Apes / Baller Ape / Pixelmon / ZKasino | 2021-24 | OPS11 / NFT | DOJ Frosties; CMC |
| Axiom employee dashboards | 2026 | OPS11 product insider tools | CoinDesk / Decrypt |
| 100+ Inferno/Venom **brand lures** (OpenSea, Uniswap, airdrops, wallets, CEX "unfreeze") | 2022-26 | OPS02; **do not audit the real protocol** | ingest §3.4 |

Full row-level sources: `_ingest/zachxbt-tayvano.md`.

## What this slice is not

- Not a list of unpublished 0-days.
- Not every Telegram investigation (200+). If it is not in the tables, search X/`t.me/investigations` this session and **promote** only with a URL.
- Not legal advice. Allegations (Machi, Ansem, Axiom, Humanityprot, Circle Files) stay labeled **allegation** until court/FBI/protocol blog.
- Not personal dossiers. Use public handles and incident names.

## Sources (primary-ish)

- [Wikipedia: ZachXBT](https://en.wikipedia.org/wiki/ZachXBT) (notable investigations; identity from court filings is out of scope here)
- [CMC: Who is ZachXBT](https://coinmarketcap.com/academy/article/who-is-zachxbt-cryptos-anonymous-scam-hunter)
- [FBI Bybit](https://www.fbi.gov/investigate/cyber/alerts/2025/north-korea-responsible-for-1-5-billion-bybit-hack)
- [Wired 2024 profile](https://www.wired.com/story/meet-zachxbt-243-million-crypto-theft/)
- Inferno/Pink/Angel: Group-IB, Scam Sniffer, Checkpoint 2025, AVOID.NET, SlowMist Angel
- [SEAL-911](https://github.com/security-alliance/seal-911)
- Raw session notes (optional, not bundled): `skills/security/SCA/sc-research/library/_ingest/zachxbt-tayvano.md`

## Honest gaps

- Most zachxbt threads are **one victim / one mixer graph**. They never become Rekt leaderboard rows. Do not pad this table with rumors.
- Dollar figures disagree (Bybit $1.4 vs $1.5B; Drift 2025 vs 2026 tables; Inferno $87M vs $250M operator claims). Prefer FBI / protocol / Rekt.
- Direct X API scrape is incomplete (Firecrawl rate limits, no Nur X login). Re-fetch `from:zachxbt` and `from:tayvano_` this session if the user names a missing incident.
- tayvano_ volume is **campaign** coverage. Brand-lure names live in ingest §3.4. Do not archive malware kits in this repo.
- Fire.xyz was not a durable public blog in the 2026-08-26 pass. Use MetaMask monthlies + MyCrypto Medium.
- Telegram `t.me/investigations` has 300+ posts; unnamed one-wallet drains are omitted (PII). Re-fetch if the user names a missing case.

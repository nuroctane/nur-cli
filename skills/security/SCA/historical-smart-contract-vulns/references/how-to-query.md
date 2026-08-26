# How to query the public corpora

Re-fetch live pages this session. Indexes rot. This is the **query grammar**, not a cache of findings.

## Order

1. Confirm in-scope asset + commit + known issues.
2. Label VM + protocol type (`taxonomy.md`).
3. Search **one** corpus below with that label.
4. Extract the case card (`index.md`). Pattern only.
5. Grep in-scope source. Write a local test. File via `contest-and-bounty-reporting`.

## Solodit

[solodit.xyz](https://solodit.xyz/) - contest findings (C4, Sherlock, Hats, Cantina, Spearbit, others).

Queries that work:

```
protocol-type + function     vault deposit harvest
exact dependency             ERC4626 OpenZeppelin 4.9
callback name                uniswapV3SwapCallback
class keyword                first depositor inflation
L2 keyword                   sequencer grace period
known-issue title            (to AVOID reporting)
```

For each hit: what was trusted, not the report prose. Dedup against the contest README known issues.

## Immunefi

| Surface | URL | Use |
|---------|-----|-----|
| Programs | https://immunefi.com/bug-bounty/ | live ceilings, assets, known issues |
| Bugfix reviews | https://github.com/immunefi-team/Web3-Security-Library/blob/main/BugFixReviews/README.md | paid, patched, classified |
| Writeup index | https://github.com/sayan011/immunefi-bug-bounty-writeups-list | researcher + $ + link |
| Scoreboard | https://immunefi.com/blog/research/the-ecosystem-vulnerability-scoreboard-6-years-of-defi-loss-data/ | industry mix, not a finding |
| Boost reports | https://reports.immunefi.com/ | public boost reports |

Search the writeup index by protocol name, then open the **Medium / Mirror / Immunefi blog** link. Do not cite the GitHub table cell as the primary source.

## DeFiHackLabs

[SunWeb3Sec/DeFiHackLabs](https://github.com/SunWeb3Sec/DeFiHackLabs) - public incident **tests**. Snapshot of their README TOC lives in `incident-corpus.md` (**858** named incidents, 2017 through 2026-08-23). Live count may be higher.

Workflow: find the date+name in the corpus -> open the upstream README anchor -> copy the **invariant that broke** into your mock. Do not paste their attacker contract onto a wallet with funds.

## Rekt / firm IR

- [rekt.news](https://rekt.news) - loss-side postmortems. Cross-check tx hashes.
- BlockSec, SlowMist, PeckShield, CertiK, Cyvers, Hexagate blogs - IR writeups.
- Trail of Bits, OpenZeppelin, Spearbit, OtterSec, Zellic, ChainLight, Trust Security, Asymmetric - bounty / audit blogs.

Public tx reconstruction: `reconstructing-public-postmortems` + `onchain-read-recon`. DarkNavy `/exploit-investigator` if installed.

## Contests

| Platform | What to search |
|----------|----------------|
| Code4rena | `code4rena.com` reports + Solodit |
| Sherlock | contest findings + judging docs |
| Cantina / Spearbit | portfolio reports when public |
| Hats | on-chain hats / competition writeups |
| CodeHawks | Cyfrin contest reports |
| Immunefi comps | Firedancer-style **client** comps too (`auditing-blockchain-clients`) |

Recurring winning classes: `contest-winning-classes.md`.

## SCV / SWC / EthTrust

- [sirhashalot/scv-list](https://github.com/sirhashalot/scv-list) - living vuln catalog
- [SWC registry](https://swcregistry.io/) - weakness IDs (incomplete for modern DeFi)
- [EEA EthTrust](https://entethalliance.org/specs/ethtrust-sl/) - living Solidity spec
- [SCSVS](https://github.com/ComposableSecurity/SCSVS)

Map SWC -> playbook in `sc-research/references/swc-map.md`. Prefer taxonomy ids in this library for economic bugs SWC never named.

## Chain-specific search

| VM | Extra queries |
|----|----------------|
| Solana | OtterSec / Neodyme writeups; `missing signer`, `arbitrary CPI`, `canonical bump`, `sysvar spoof` |
| Move | Aptos / Sui security posts; `borrow_global`, `public entry`, shared object ACL |
| Cosmos | IBC GitHub security advisories (`ASA-20xx`), Informal / Interchain posts; Dragonberry, ibc-hooks |
| Cairo | Vesu / Starknet disclosures; `u256` rounding, overflow |
| Bitcoin-adj | Stacks Immunefi; Lightning CVE tracker; BitVM research posts |
| zk / L2 | Scroll / zkSync / Polygon zkEVM forum posts; proof verification, message spoofing |

## X (Twitter)

X is a **pointer**. Confirm on a writeup before it enters a report.

Handles and example posts: `hunting-x-linked-bounties` (paid + X tables) then `x-and-public-signal.md`. zachxbt / tayvano_ **ops vs code**: `investigator-ops.md`. Raw ingest is **not** a skill: `skills/security/SCA/sc-research/library/_ingest/` (do not `skill(read)`).

Query templates:

```
from:immunefi (paid OR payout OR bugfix)
from:rektnews
"Immunefi" (Critical OR "bugfix review")
from:ottersec (signer OR CPI OR PDA)
from:BlockSecTeam (postmortem OR exploit)
from:zachxbt (Bybit OR Lazarus OR Safe)
from:tayvano_ (drainer OR Permit2 OR phishing)
```

## What not to query for

- Unpublished 0-days, "how do I drain X on mainnet", Telegram leak screenshots without a public postmortem.
- Live mempool copy-trading of an in-progress hack (that is IR / law, not this skill).

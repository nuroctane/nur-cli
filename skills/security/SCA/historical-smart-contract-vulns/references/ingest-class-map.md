# Ingest class map (Grok T-ids -> pack taxonomy -> skill)

`skills/security/SCA/sc-research/library/_ingest/grok-x-bounties.md` uses its **own** T01-T52. Pack ids live in `taxonomy.md`. `case-cards.md` still prints ingest labels. **Always map before filing.**

Do not load the ingest file into a review prompt. This table is the packaged map.

| Ingest id / label | Pack id | Skill |
|-------------------|---------|-------|
| T01 access / mint | T02 | `reviewing-access-control-and-auth` |
| T02 uninitialized UUPS | T12 | `reviewing-upgradeable-proxies` |
| T04 signature / replay / branchMask | T10 / T11 / T17 | `reviewing-signatures-permit-and-eip712` or bridges |
| T05 custom approve | T23 | `reviewing-token-standard-pitfalls` + signatures |
| T06 oracle spot | T05 | `reviewing-oracles-and-pricing` |
| T09 aliasing | T27 | `reviewing-l2-sequencer-and-finality` |
| T10 delayed inbox / censorship | T28 / T30 | L2 |
| T13 first depositor | T06 / T36 | `reviewing-erc4626-and-vaults` |
| T14 inflation / donate | T06 / T14 | vaults or lending |
| T16 / T18 reentrancy | T07 / T08 | `reviewing-reentrancy-and-callbacks` |
| T19 NFT | T32 | `reviewing-nft-and-marketplace` |
| T20 callback spoof | T16 | `reviewing-amm-and-cl-pools` |
| T21 tick math | T15 / S09 | AMM / Solana |
| T22 / T23 flash composition | T09 | oracles |
| T24 bad debt / insolvency | T14 | `reviewing-lending-and-liquidations` |
| T25 donate/liq Euler-class | T14 / T09 | lending |
| T26 messaging / LayerZero | T17-T20 | `reviewing-bridges-and-messaging` |
| T27 consensus / IAVL / Plasma proof | C02 / T17 / O04 | Cosmos / bridges / clients |
| T28 validator keys | OPS10 | `investigator-ops.md` (not Solidity) |
| T29 / T30 flash gov / snapshot | T22 / T21 | `reviewing-governance-and-timelocks` |
| T31 arbitrary call / calldata | T34 | `reviewing-cross-function-and-composer` |
| T33 rounding / mulDiv | T04 | AMM / vaults / Cairo |
| T35 logic / accounting | T01 | matching protocol playbook |
| T36 input validation / grief | T03 | composer / triage |
| T37 Vyper | T35 | reentrancy |
| T38 client / OVM / Aurora engine | O04 / T27 / O01 | `auditing-blockchain-clients` + L2 + bridges |
| T39 zk proof | T31 | L2 |
| T40-T42 Solana (signer / accounts / sysvar) | S01-S05 | `reviewing-solana-programs` |
| T43-T44 Move | M01-M07 | `reviewing-move-modules` |
| T45 IBC | C01-C05 | `reviewing-cosmos-and-ibc` |
| T46 Bitcoin-adj / Clarity | B01-B03 | `reviewing-bitcoin-adjacent` |
| T47 Cairo | K01-K02 | `reviewing-cairo-and-starknet` |
| T52 halt / chain split / DoS | M06 / O04 / B03 | Move / clients / Bitcoin-adj |

Full paid `$` table with playbook column: `hunting-x-linked-bounties/references/paid-payouts.md`.

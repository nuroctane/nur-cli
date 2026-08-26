# How to pick one slice

Never dump this library into a prompt. Answer three questions, then open **one** file.

## 1. What is the VM?

| VM | File | Playbook |
|----|------|----------|
| EVM (Ethereum, L2s, BSC, Polygon PoS, Avalanche C, Fantom, Gnosis, …) | `evm-and-l2.md` | matching protocol playbook |
| Solana / SVM (Eclipse, Sonic, …) | `solana.md` | `reviewing-solana-programs` |
| Move (Aptos, Sui, Movement) | `move-cosmos-cairo.md` | `reviewing-move-modules` |
| Cosmos SDK / CosmWasm / IBC | `move-cosmos-cairo.md` | `reviewing-cosmos-and-ibc` |
| Cairo / Starknet | `move-cosmos-cairo.md` | `reviewing-cairo-and-starknet` |
| Bitcoin Script, Lightning, Stacks, BitVM, Babylon | `bitcoin-and-other-vms.md` | `reviewing-bitcoin-adjacent` |
| Other (TON, Near, ink!, Plutus, Michelson, Sway, TVM, FVM, Hedera, Algorand, XRPL, ICP) | `bitcoin-and-other-vms.md` + `chain-catalog.md` | client skill if it is a node bug |

Unknown chain: `chain-catalog.md` first.

## 2. What is the question?

| Question | File |
|----------|------|
| Which class is this? | `taxonomy.md` |
| What have whitehats been **paid** for? | `hunting-x-linked-bounties` `paid-payouts.md` then `notable-bounties.md` |
| Ingest T-id (Grok file) vs pack id | `ingest-class-map.md` |
| Need grep seeds + a local test idea for a named case | `case-cards.md` (80 cards; map ingest T-ids via `ingest-class-map.md`) |
| What did X accounts point at? | `hunting-x-linked-bounties` `x-linked-2024-2026.md` then `x-and-public-signal.md` |
| What actually **lost** money? | `incident-timeline.md` then `named-losses.md` |
| Is this incident in the public corpus? | `incident-corpus.md` (858 names, DeFiHackLabs TOC snapshot) |
| What keeps winning contests? | `contest-winning-classes.md` |
| Where do I search Solodit / Immunefi / Rekt? | `how-to-query.md` |
| zachxbt / tayvano_ / Bybit / drainer / phishing / keys | `investigator-ops.md` (ops vs code; then `reviewing-frontend-and-ops-surfaces` if the repo is a wallet/UI) |

## 3. Case card (required when you cite a historical bug)

Every citation you use in a review must fit this card. If a field is missing, you do not have a case, you have a rumor.

```
name:
year:
chain / VM:
protocol type:
bug class:          (taxonomy id)
what was wrongly trusted:
grep / review seeds:
local test idea:    (Foundry / Anchor / Move / CosmWasm; not a working exploit)
source URL:         (Immunefi, official postmortem, Rekt, DeFiHackLabs, firm blog)
```

## Industry shape (Immunefi scoreboard, public)

Use this to **prioritize**, not to skip classes.

- DeFi protocol losses: **$2.62B (2022) -> $534M (2024) -> $680M (2025)**.
- Median incident: **$6M -> $1.5M**.
- Flash-loan / oracle / reentrancy share of losses: **~19% (2022) -> <1% (2025)**.
- Purpose-built bridges: **73% of 2022 losses -> 3% in 2025**. Risk moved into **messaging configs** (KelpDAO LayerZero single-verifier, Apr 2026, ~$292M).
- 2025 DeFi protocol losses: **~89% application-specific logic**.
- Custodial / ops is a **different bucket**: Bybit Feb 2025 ~$1.5B Safe UI / JS supply-chain is not an Ethereum consensus bug. Full zachxbt + tayvano_ catalog: `investigator-ops.md`.
- Immunefi paid whitehats the record **$10M** (Wormhole, satya0x) and **$6M** (Aurora, pwning.eth). Cumulative platform payouts crossed **$100M** (2024) and were cited ~**$134M** by Q1 2026.
- H1 2026: Immunefi cited ~**$972M** across 207 hack incidents industry-wide, **$13.45M** paid for 837 valid pre-exploit reports on-platform.

Primary: [Immunefi Ecosystem Vulnerability Scoreboard](https://immunefi.com/blog/research/the-ecosystem-vulnerability-scoreboard-6-years-of-defi-loss-data/), [Immunefi hackers page](https://immunefi.com/hackers/).

## Anti-patterns

- Do not treat a tweeted dollar figure as paid until Immunefi / the protocol blog agrees.
- Do not reconstruct unpublished exploits from "a friend of a friend."
- Do not run DeFiHackLabs attacker contracts against a fork that can broadcast.
- Do not merge this into the 817 cybersecurity router. Crypto there means cryptography.
- Do not file Bybit / WazirX / Radiant-malware / Inferno drainers as vault inflation.

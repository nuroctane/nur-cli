---
name: historical-smart-contract-vulns
description: "Historical smart-contract vulnerability library across EVM L1/L2, Solana, Move, Cosmos/IBC, Cairo, Bitcoin-adjacent, other VMs, plus zachxbt/tayvano ops vs code. Use for bounty-class mapping, public incident lookup, and in-scope review seeds. Lawful/whitehat only. Load one reference slice, never the whole pack."
---

# Historical smart-contract vulns (whitehat library)

A **lookup**, not an exploit cookbook. Every named case is already public (Immunefi bugfix review, contest writeup, official postmortem, Rekt, DeFiHackLabs README). Copy the **pattern** into a local Foundry / Anchor / Move / CosmWasm test you own. Never broadcast. Never paste attacker contracts onto a funded host.

Do **not** load every file. Pick one slice from `references/index.md`.

This pack is the memory. `sc-research` is the router. `hunting-x-linked-bounties` is the **paid-writeup router** (Immunefi/X `$` -> playbook). Protocol playbooks (`reviewing-erc4626-and-vaults`, `reviewing-solana-programs`, …) are how you hunt the same class on **in-scope** code.

## Lawful gate

1. In-scope source only (your repo, contest README, published bounty assets).
2. Out of scope = stop. Read `sc-research/references/disclosure.md`.
3. PoCs are **local tests**. No live drain, no unpublished 0-day against production.
4. Custodial / phishing / key-theft incidents (Bybit 2025, many 2026 ops) are catalogued so you do **not** misclassify them as Solidity bugs.

## Fast path

| You have | Load |
|----------|------|
| A chain or VM name | `references/chain-catalog.md` then that VM file |
| A dollar amount / famous bounty | `hunting-x-linked-bounties` `references/paid-payouts.md` (then this pack's `notable-bounties.md` for deep cards) |
| A 2024-2026 X handle / payout tweet | `hunting-x-linked-bounties` `references/x-linked-2024-2026.md` |
| A researcher name (satya0x, saurik, …) | `hunting-x-linked-bounties` `references/researcher-index.md` |
| Ingest T01-T52 vs pack ids | `references/ingest-class-map.md` |
| A named incident with grep/test idea | `references/case-cards.md` (80 cards) |
| A year or "what landed in 2026" | `references/incident-timeline.md` then `named-losses.md` then `incident-corpus.md` |
| A protocol type (vault, AMM, bridge) | `references/taxonomy.md` then one playbook |
| zachxbt / tayvano_ / Bybit / drainer / phishing / "was this even code?" | `references/investigator-ops.md` then maybe `reviewing-frontend-and-ops-surfaces` |
| "Where do I search next?" | `references/how-to-query.md` |
| Solana / Move / IBC / Cairo / Bitcoin | the matching playbook, not the EVM vault skill |

## Progressive disclosure

```
SKILL.md  (this file)
  -> references/index.md          how to pick one slice
  -> references/taxonomy.md       70+ classes + OPS -> playbooks
  -> references/ingest-class-map.md   Grok ingest T-ids -> pack ids
  -> references/notable-bounties.md
  -> hunting-x-linked-bounties        paid rows grouped by playbook (separate skill)
  -> references/case-cards.md         80 cards: grep + local test idea
  -> references/incident-timeline.md
  -> references/named-losses.md      Rekt-scale named losses
  -> references/incident-corpus.md   858 public DeFiHackLabs names
  -> references/chain-catalog.md
  -> references/evm-and-l2.md
  -> references/solana.md
  -> references/move-cosmos-cairo.md
  -> references/bitcoin-and-other-vms.md
  -> references/contest-winning-classes.md
  -> references/how-to-query.md
  -> references/x-and-public-signal.md
  -> references/investigator-ops.md   zachxbt + tayvano_ (ops vs code)
```

Next skill after a hit: the mapped playbook, then `writing-foundry-invariant-handlers` or `contest-and-bounty-reporting`.

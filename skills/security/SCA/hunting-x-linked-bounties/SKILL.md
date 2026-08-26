---
name: hunting-x-linked-bounties
description: "Route Immunefi/X-linked paid writeups to the matching protocol playbook. Use for bounty intel, whitehat payouts, Immunefi bugfix reviews, and X-announced crits. Lawful/in-scope only. Load one reference slice, never the whole pack."
---

# Hunting X-linked bounties (whitehat)

This is the **paid-writeup index**. Every public Immunefi / Mirror / X-linked bounty in the Grok ingest is mapped to a Nur playbook so the agent loads the **review skill**, not a rumor.

Do **not** load every file. Pick one slice.

`historical-smart-contract-vulns` is the incident library (hacks + bounties + ops). This skill is the **payout router**: dollar figure, researcher handle, or X post -> class -> playbook.

## Lawful gate

1. In-scope source only. A writeup is a **pattern**, not a license to hit production.
2. Tweeted `$` is not paid until Immunefi / protocol blog / firm post agrees.
3. Local Foundry / Anchor / Move / CosmWasm tests. No broadcast.

## Fast path

| You have | Load |
|----------|------|
| A dollar figure or protocol name | `references/paid-payouts.md` then the **Playbook** column |
| A 2024-2026 X handle / status | `references/x-linked-2024-2026.md` |
| A researcher name (satya0x, pwning.eth, saurik, …) | `references/researcher-index.md` |
| Ingest T01-T52 vs pack taxonomy | `historical-smart-contract-vulns` `references/ingest-class-map.md` |
| A named case with grep + test idea | `historical-smart-contract-vulns` `references/case-cards.md` |
| Ops / phishing / keys | `investigator-ops.md` + `reviewing-frontend-and-ops-surfaces` |

## Progressive disclosure

```
SKILL.md  (this file)
  -> references/paid-payouts.md          every paid row + playbook
  -> references/x-linked-2024-2026.md    X-sourced findings
  -> references/researcher-index.md      handle -> skill
```

After a hit: `skill(read)` the playbook in the Playbook column. File via `contest-and-bounty-reporting`.

## Do not

- Cite an X screenshot as a contest analog.
- Load `skills/security/SCA/sc-research/library/_ingest/grok-x-bounties.md` into the prompt (raw ingest, not a skill).
- File Bybit / drainers / Permit2 phishing as vault inflation.

---
name: battlechain-safe-harbor-whitehat
description: "Cyfrin BattleChain and Safe Harbor whitehat flow: deploy to the pre-mainnet L2, request attack mode, read on-chain bounty terms. Use with Cyfrin/solskill. Lawful attack-mode only on contracts that opted in."
---

# BattleChain and Safe Harbor

Cyfrin [BattleChain](https://docs.battlechain.com/) is a pre-mainnet L2 for battle-testing with real funds **and published bounty terms**. X: [@PatrickAlphaC](https://x.com/PatrickAlphaC) [@CyfrinAudits](https://x.com/CyfrinAudits).

Install `npx skills add cyfrin/solskill --skill battlechain` (ecosystem ensure also pulls `Cyfrin/solskill`).

## Rules

- Only contracts that requested attack mode / Safe Harbor.
- Read the on-chain bounty terms before touching them.
- PoCs stay within BattleChain. Do not pivot to Ethereum mainnet.

If the kit is installed, follow its skill body. If not: fetch BattleChain docs, list opted-in contracts, stop if terms are unclear.

---
name: reconstructing-public-postmortems
description: "Turn already-public DeFi incidents (rekt, DeFiHackLabs, BlockSec) into Foundry regression tests. Use for learning and for pinning a similar pattern on in-scope code. Never replay against live funds."
---

# Reconstructing public post-mortems

Only **already-published** incidents: rekt.news, BlockSec/SlowMist posts, Immunefi disclosed, DeFiHackLabs writeups, mined tx hashes.

This is a gym + regression skill, not IR against an undisclosed hack.

## Steps

1. Pin sources: article URL, tx hash, chain, victim address.
2. `onchain-read-recon` on the mined tx (`cast receipt` / `cast run`). No broadcast.
3. Name the bug class (`bug-classes.md` or `historical-smart-contract-vulns` taxonomy). Check `case-cards.md` for grep seeds + a local test idea, then `incident-corpus.md` if the name is a 2020s DeFi incident. If zachxbt/tayvano_ covered it as phishing/keys/UI, load `investigator-ops.md` and do **not** reconstruct a Solidity ghost.
4. Write a **minimal** Foundry test that reproduces the accounting break on a local mock or a fork **without** sending a tx.
5. Write the invariant that would have failed.
6. Grep the **current in-scope** repo for the same pattern. If present, file via `contest-and-bounty-reporting`.

DeFiHackLabs is a corpus of tests. Copy the **pattern**, do not paste their attacker contracts onto a funded host.

DarkNavy `/exploit-investigator` (if installed) is the heavy loop for a public tx. Same rules.

## Paid vs loss

If the user named a **bounty** (Wormhole $10M, Aurora $6M, Scroll $1M, Raydium $505k), load `hunting-x-linked-bounties` and the Playbook column. This skill is for **already-mined loss txs**. Do not mix a payout writeup with a Rekt drain.

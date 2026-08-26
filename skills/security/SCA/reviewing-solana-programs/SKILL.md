---
name: reviewing-solana-programs
description: "Review Solana/SVM programs: missing signer, account confusion, arbitrary CPI, PDA bumps, sysvar spoof, close/reinit, Token-2022 hooks. Use for Anchor/native programs and SVM L2s. Lawful/in-scope only."
---

# Reviewing Solana programs

In-scope program source only. Tests on localnet / LiteSVM. Never broadcast a drain.

Load `historical-smart-contract-vulns` `references/solana.md` for historical analogs (Wormhole sysvar, Cashio, Raydium ticks, Drift ops vs code).

## Conservation

Token accounts the program intends to custody cannot be withdrawn without the documented signer + correct mint + correct vault PDA.

## Account checklist (every instruction)

1. **Signer** where authority is claimed (`S01`).
2. **Owner** is this program (or the documented Token program) (`S02`).
3. **Discriminator** / `has_one` / expected address (`S02`).
4. **Callee program id** hardcoded or from a verified config PDA (`S03`).
5. **Canonical bump** stored and checked (`S04`).
6. **Sysvar keys** equal the real sysvar (`S05`) if you parse instructions.
7. **Close** zeros data; reinit cannot take over (`S06`).
8. **Token-2022** hooks cannot rewrite custody (`S07`).
9. **`invoke_signed` seeds** match the PDA you mean (`S08`).

## CL / lending / vaults on SVM

Tick math: `solana.md` S09 (Raydium $505k analog). Lending: Port Finance bounty analog (withdraw without debt). Oracle + admin: say **ops** when it is keys (Drift 2026), not a missing `Signer`.

## Output

Table: `(ix, account, check missing, taxonomy S0x, local Anchor test idea)`. Next: `contest-and-bounty-reporting`.

## Paid bounty analogs

Full `$` table: `hunting-x-linked-bounties` `references/paid-payouts.md`. Sysvar **hack** (Wormhole $326M) is S05, not a bounty.

| $ | Protocol | Pack | Whitehat |
|---|----------|------|----------|
| **$505k** | Raydium tick manipulation | S09 | riproprip |
| ~$630k | Port Finance withdraw vs debt | S02 / T14 | nojob |
| Crit | marginfi flash + health | S10 | Felix Wilhelm |

Drift 2025/2026 ~$285M is **ops+oracle** until a postmortem says otherwise. Cashio / Mango / Slope are hack or client analogs (`solana.md`).

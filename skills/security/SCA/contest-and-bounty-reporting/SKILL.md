---
name: contest-and-bounty-reporting
description: "Write in-scope smart-contract contest and Immunefi bounty reports: severity, impact, root cause, local Foundry test, fix. Use for Code4rena, Sherlock, Cantina, Hats, Immunefi. No live exploits."
---

# Contest and bounty reporting

A finding that cannot be filed is not a finding. This playbook is the **writeup shape** researchers get paid for.

**Scope first.** Read the program's assets, known issues, and out-of-scope list this session. If the user points at a live protocol with no published program, load `sc-research` `references/disclosure.md` and stop.

## Platforms (process only)

| Platform | What to copy from their docs |
|----------|------------------------------|
| Immunefi | assets in scope, severity table (critical = direct loss), PoC requirements |
| Code4rena | judging criteria, duplicate window, QA vs valid, known issues |
| Sherlock | Watson vs Lead, issue validity, hierarchical judging |
| Cantina | severity, portfolio, contest vs private |
| Hats | hats.finance / audit competitions, Safe Harbor if present |

Do not invent their rules. Fetch the live docs if unsure.

## Report body (always this order)

1. **Title** - impact first, not the gadget. Bad: "reentrancy in `withdraw`". Better: "Read-only reentrancy on `totalAssets` lets an attacker mint extra shares".
2. **Severity** - program scale. State impact (who loses what) and likelihood (one tx? needs admin? needs stalled oracle?).
3. **In scope** - chain, address or repo path, commit hash.
4. **Root cause** - the trust assumption that is false (spot price, missing initializer, share math).
5. **Local proof** - Foundry test in-repo. `setUp` + attacker actor + `assert` on the broken invariant or `expectRevert` after the fix. Fork tests may read public RPC; they must **not** `--broadcast`.
6. **Fix** - the smallest change (CEI, `nonReentrant`, staleness check, `_disableInitializers`, virtual shares).
7. **References** - SWC id, one Solodit analog if it exists.

Never attach a drain script, flashbots bundle, or mainnet calldata.

## Foundry test shape (allowed)

```solidity
function test_Finding_ShareInflationOnEmptyVault() public {
    // local deploy or vm.createSelectFork of an in-scope address
    vm.prank(attacker);
    // sequence that demonstrates extra shares / stolen assets
    assertLt(vault.convertToAssets(victimShares), victimDeposit);
}
```

If the test needs an "attacker contract", keep it in `test/` and use it only under `forge test`. That is a regression test, not an exploit payload.

## Duplicate / QA hygiene

- Search Solodit + the contest's known issues before filing.
- One root cause per report. Do not split the same bug across functions unless impact differs.
- If you only have a detector hit and no economic impact, it is QA / Low - say so.

## Expected output

A markdown report matching the program form, plus a passing (demonstrating) or failing-invariant Foundry test path. Ready to paste into Immunefi / C4. No public tweet until the program allows disclosure.

## Paid analog lookup (before you file)

Do not invent a historical analog from memory. Load **one**:

| You need | Skill / slice |
|----------|----------------|
| `$` + playbook for a named Immunefi payout | `hunting-x-linked-bounties` `references/paid-payouts.md` |
| 2024-2026 X handle | `hunting-x-linked-bounties` `references/x-linked-2024-2026.md` |
| Researcher name | `hunting-x-linked-bounties` `references/researcher-index.md` |
| Grep + local test idea | `historical-smart-contract-vulns` `references/case-cards.md` |
| Ingest T-id vs pack id | `historical-smart-contract-vulns` `references/ingest-class-map.md` |

Hats Velvet (~$40k paid of ~$70k) and Immunefi Alchemix V3 liq-fee overpay are **contest-shaped** paid rows. Raft-as-Hats-venue was later **hacked** - do not cite the Hats report as a successful invariant. Uniswap v4 $15.5M / LayerZero $15M / Usual $16M are **ceilings**. GregoAI 2026 X claims are unverified.

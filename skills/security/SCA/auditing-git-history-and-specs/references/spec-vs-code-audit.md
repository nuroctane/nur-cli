# Spec vs code audit (docs as obligations)

Whitepapers and design docs are checklists the code is quietly graded against. The audit work is separating "promised" from "implemented" without letting either contaminate the other.

## Method

1. **Collect every testable claim** from whitepaper/docs/gitbook/announcement posts: fee amounts, caps, ratios, redemption guarantees, "fully collateralized", "no admin mint", slippage bounds, timelock promises.
2. **Tag provenance on every claim**: `(per spec)` until the code confirms it. A spec claim the code contradicts is a finding class of its own (implementation drift); a code behavior absent from the spec is a disclosure gap - both get reported, neither gets assumed.
3. **Grade each claim**: CONFIRMED (code enforces), CONTRADICTED (code breaks it - finding), UNENFORCEABLE (nothing in code could enforce it - design finding), ABSENT (code behavior with no spec basis - flag for the team).
4. **Trust assumptions table**: every external dependency with what the docs assume (price correctness, token standard compliance, bridge liveness) vs what the dependency actually guarantees. This table feeds the trust-gap lens.

## Threat profiling overlay

Profile by protocol type before reading code (feeds `senior-sop` lens order):

- **Temporal threats**: what is safe within one tx but not across txs (rates, rewards, vesting).
- **Composability threats**: what other protocols do with your outputs - your LP token as someone's collateral, your price as someone's oracle (read-only reentrancy lives here).
- **Hybrid classification**: protocols spanning two types (vault+lending, AMM+bridge) get BOTH type playbooks' checklists; the seam between them is where findings concentrate (mirrors the gap-hunter lenses).

## Doc sources worth checking (agents and humans forget these)

Gitbook/changelog diffs (docs change without code - or the reverse), governance forum proposals that promised parameters, audit PDFs of PREVIOUS versions (their findings recur in forks), deploy announcements stating guarantees the code never encoded.

## Output

One spec-vs-code table in the report: claim | source | provenance tag | grade | evidence link. Keep it factual; the drama writes itself.

---
name: defi-security-trends-standards
description: "Current-year smart-contract security briefing: what loss classes dominate right now, OWASP Smart Contract Top 10 mapping, audit tooling status, and new attack surfaces (EIP-7702 drainers, DVN/verifier bridge configs, intents, RWA rails). Use for 'what should I check in 2026', scoping a review to current trends, or refreshing stale checklists. Lawful/in-scope only. Load one reference slice."
---

# DeFi security trends and standards (current-year briefing)

The "what is happening THIS year" slice of the pack. Checklists age; this is the refresh point. Refresh cadence: quarterly, or immediately after any top-ten-scale incident.

## Fast path

| You want | Load |
|----------|------|
| Top-vuln-class ranking / checklist mapping | `references/owasp-sc-top10.md` |
| Loss stats by era/month, tracker disagreement notes | `references/incident-trends-2025-2026.md` |
| Which fuzzer/static tool is current, what it now catches | `references/tooling-status.md` |
| Genuinely new attack surfaces post-2024 | `references/new-attack-surfaces.md` |

## Standing pointers

- Chain-by-chain work: `reviewing-major-chain-surfaces` (pick one chain file).
- Class deep-dives: matching `reviewing-*` playbook via `sc-research`.
- Incident rows land permanently in `historical-smart-contract-vulns/references/incident-timeline.md`; this skill holds only the trend frame.

## How to use in a review

1. Open `owasp-sc-top10.md`, map each ranked class onto your scope's modules - that is your minimum coverage set.
2. Skim `new-attack-surfaces.md`; anything intersecting your stack (7702-aware flows? bridge security-module config? RWA policy hooks?) gets a dedicated checklist pass.
3. Confirm tooling currency against `tooling-status.md` before trusting old detector output.

---
name: static-analysis-slither-aderyn-semgrep-wake
description: "Run and triage Slither, Aderyn, Semgrep Solidity rules, and Wake on a Foundry/Hardhat repo. Use as the fast static layer before invariants. Complements auditing-foundry-smart-contract-security."
---

# Static analysis (Slither, Aderyn, Semgrep, Wake)

Fast layer. Does not replace handlers. In-scope source. Prefer the Foundry skill orchestrator (`scripts/agent.py`) when present.

## Commands

```bash
forge build
slither . --json slither-report.json
aderyn . -o aderyn-report.json
# optional
semgrep --config p/solidity --config trailofbits ...
wake detect
```

Triage: dedupe by `(file, line)`, drop informational, keep High/Medium that map to SWC or a bug class in `bug-classes.md`.

False positives: view reentrancy on non-value paths, `solc-version` on a pinned pragma, `naming-convention`.

Every kept hit needs a Foundry test or an explicit "not exploitable because X". Detector output is not a report.

Next: `triaging-and-deduping-findings`.

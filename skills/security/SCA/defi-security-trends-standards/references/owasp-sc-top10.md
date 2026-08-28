# OWASP Smart Contract Top 10 (2025 vs 2026) mapped to pack playbooks

Primary: [scs.owasp.org/sctop10](https://scs.owasp.org/sctop10/) (current), [2025 archive](https://scs.owasp.org/sctop10/archive/2025/Top10%3A2025). The 2026 edition is derived from 122 deduplicated 2025 incidents (~$905M losses) projected forward. Use the CURRENT edition for coverage planning; keep 2025 in mind when reading older reports.

## SC10:2026 -> pack coverage

| # | Category | Pack playbook |
|---|----------|---------------|
| SC01 | Access Control Vulnerabilities | `reviewing-access-control-and-auth` (+ upgradeable-proxy init via `reviewing-upgradeable-proxies`) |
| SC02 | Business Logic Vulnerabilities | `reviewing-cross-function-and-composer` + protocol-invariant sweep (`writing-foundry-invariant-handlers`) |
| SC03 | Price Oracle Manipulation | `reviewing-oracles-and-pricing` |
| SC04 | Flash Loan-Facilitated Attacks | `reviewing-mev-ordering-and-slippage` + oracle playbook combination tests |
| SC05 | Lack of Input Validation | `static-analysis-slither-aderyn-semgrep-wake` detector pass + grep seeds per playbook |
| SC06 | Unchecked External Calls | `reviewing-reentrancy-and-callbacks` |
| SC07 | Arithmetic Errors (Rounding & Precision) | `reviewing-amm-and-cl-pools` rounding sections + Balancer-class invariant tests |
| SC08 | Reentrancy Attacks | `reviewing-reentrancy-and-callbacks` (+ read-only reentrancy via oracle playbook) |
| SC09 | Integer Overflow and Underflow | Solidity>=0.8 default-checked; hunt unchecked blocks/wrapping casts (see Cetus card in case-cards) |
| SC10 | Proxy & Upgradeability Vulnerabilities | `reviewing-upgradeable-proxies` |

## Movement worth explaining to report readers

- Business Logic jumped to #2: whole-market pivot from syntax bugs to incentive/mechanism flaws (matches Immunefi's "89% of 2025 DeFi losses were logic" line).
- Flash loans rose #7 -> #4 as an enabler class rather than a root cause.
- Reentrancy sank #5 -> #8: compilers+patterns matured; still present but no longer headline.
- Proxy/Upgradeability ENTERED at #10: init-grabs and unverified delegate targets normalized alongside EIP-7702 delegated EOAs.
- Dropped from 2025: Insecure Randomness (#9) and DoS (#10) - still real, no longer top-tier by dollars; DoS remains mandatory on zero-gas chains (Plasma note in `reviewing-major-chain-surfaces/alt-evm-l1s.md`).

## Coverage rule

Every engagement checklist must show a disposition (tested-clean or finding) for all ten classes of the current edition before submission. One-line rationale per N/A.

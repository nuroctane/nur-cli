# Trail of Bits building-secure-contracts (map)

Upstream: https://github.com/crytic/building-secure-contracts (AGPLv3). Do not vendor the repo. Load it when the user is on a Crytic workflow. Map:

## Development guidelines

| Doc | Nur use |
|-----|---------|
| Code maturity | Gate: no contest report if the repo has no tests, no pin, no natspec on value paths |
| High-level best practices | CEI, pull over push, explicit visibility, events |
| Incident response | Out of pack scope except: public post-mortems as recon |
| Secure development workflow | Matches Foundry PASS/FAIL gate |
| Token integration checklist | `reviewing-token-standard-pitfalls` |

## Program analysis

| Doc | Nur playbook |
|-----|--------------|
| Echidna | `fuzzing-with-echidna-and-medusa` |
| Medusa | same |
| Slither | `static-analysis-slither-aderyn-semgrep-wake` |
| Manticore | optional; prefer Halmos/Mythril in this pack |

## Learn EVM

Opcodes, tracing, arithmetic checks, yellow paper, fork/EIP maps. Use when a finding depends on gas stipend, `RETURNDATA`, or a specific hard fork.

## Not So Smart Contracts

Multi-chain examples (Solana, Cairo, Cosmos, Substrate, Sui, TON, Algorand). If the user is off-EVM, load this + `auditing-blockchain-clients` rather than Solidity playbooks.

X: [@trailofbits](https://x.com/trailofbits) [@crytic_](https://x.com/crytic_)

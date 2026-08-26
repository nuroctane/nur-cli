# Tooling matrix (defense / analysis only)

Run on in-scope source. Never against untrusted bytecode on a host with funded keys unlocked.

## Layer 0 - reading

| Tool | Use |
|------|-----|
| Foundry `cast` | `call`, `storage`, `code`, `sig`, `run` (mined txs) |
| Sourcify / Etherscan-family | verified source vs bytecode |
| Tenderly / Phalcon / Openchain | traces of **already-mined** txs |
| `forge inspect` | storage layout |

Playbook: `onchain-read-recon`

## Layer 1 - static

| Tool | Use |
|------|-----|
| Slither (Trail of Bits) | 90+ detectors, printers, triage-mode |
| Aderyn (Cyfrin) | Rust, complementary detectors |
| Semgrep + ToB Solidity rules | custom pattern hunt |
| Wake (Ackee) | detectors + fuzz + deployment |
| solhint | style + a few security rules |
| 4naly3er-style reports | bulk contest triage (noisy) |

Playbook: `static-analysis-slither-aderyn-semgrep-wake` and the Foundry gate.

## Layer 2 - symbolic / formal

| Tool | Use |
|------|-----|
| Mythril | path-symbolic, slow, critical contracts only |
| Halmos (a16z) | Foundry-compatible symbolic tests (`HALMOS`) |
| SMTChecker (solc) | `pragma experimental SMTChecker` / `--model-checker-engine` |
| Certora CVL | spec language, paid/prover |
| Kontrol (Runtime Verification) | Foundry + K |

Playbook: `formal-verification-halmos-certora-kontrol`

## Layer 3 - property fuzz

| Tool | Use |
|------|-----|
| Foundry `invariant_*` + handlers | default in this pack |
| Echidna (Crytic) | `echidna_*` properties |
| Medusa (Crytic) | coverage-guided, assertion mode |

Playbooks: `writing-foundry-invariant-handlers`, `fuzzing-with-echidna-and-medusa`

## Layer 4 - keys / deploy

| Tool | Use |
|------|-----|
| gitleaks | no keys in git |
| `cast wallet import` | encrypted keystore, never plaintext `PRIVATE_KEY` |
| `forge script --account` | deploy via keystore |
| OpenZeppelin upgrades plugin | storage-layout checks on proxy deploys |

See Foundry skill `references/secure-deployment-and-keys.md`.

## Layer 5 - corpora (read, then test)

| Corpus | Use |
|--------|-----|
| Solodit | historical findings by protocol type |
| DeFiHackLabs | **regression tests** from public post-mortems |
| rekt.news / BlockSec / SlowMist writeups | incident patterns |
| Immunefi disclosed / C4 reports | report shape + severity |
| Trail of Bits building-secure-contracts | properties + token checklist |
| Cyfrin Updraft / Secureum / RareSkills | gym |

Playbooks: `reviewing-with-solodit-and-swc`, `reconstructing-public-postmortems`, `researcher-gym-and-curriculum`

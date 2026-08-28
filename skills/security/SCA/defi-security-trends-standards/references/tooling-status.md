# Tooling status refresh (verified against public repos/blogs at 2026-08)

"Is my checker actually current?" table. Versions move fast - confirm at links before pinning in CI.

## Static analysis

| Tool | Status at refresh | Notes |
|------|-------------------|-------|
| Slither | Actively maintained; ~95+ detectors; Solidity AND Vyper ([repo](https://github.com/crytic/slither)) | Late-2025 addition: **Slither-MCP** exposing Slither to LLM agents ([ToB blog](https://blog.trailofbits.com/2025/11/15/level-up-your-solidity-llm-tooling-with-slither-mcp/)) - pairs with triaging-and-deduping-findings workflow |
| Aderyn | Cyfrin Rust analyzer, detector count past 100, editor extension available ([repo](https://github.com/Cyfrin/aderyn)) | Fast first-pass; keep severity labels but not blind trust |
| Wake | Ackee framework - check maintenance cadence before CI reliance `[verify]` | Framework-grade: tests+lint+detection combined |

## Fuzzing

| Tool | Status | Guidance |
|------|--------|----------|
| Medusa | Trail-of-Bits-family parallelized coverage-guided fuzzer ([crytic/medusa](https://github.com/crytic/medusa)); Feb-2025 scalability writeup | Default recommendation for long-running campaigns over Echidna when infra allows |
| Echidna | Mature ToB property fuzzer, still maintained | Fine default; single-thread economics |
| Foundry invariants | v1.x line current (v1.7.x era at refresh with embedded Solar LSP noted in releases; track [releases](https://github.com/foundry-rs/foundry/releases)) | Handler patterns via `writing-foundry-invariant-handlers` remain the base skill; Recon-class platforms standardize contest fuzzing infra |
| Comparison reference | devdacian's fuzzing-comparison repo maintained as a practical guide | Use to justify tool choice in methodology sections |

## Symbolic / formal

- **Halmos** (a16z): runs Foundry-style tests symbolically; v0.3.0-era feature notes published. Best for targeted math cores.
- **Certora/Kontrol**: commercial prover + open Kontrol (founded on Foundry). Budget-dependent adoption remains the norm for lending/vault invariant suites.

## Workflow standards worth adopting publicly

- OWASP SC Top 10 (current edition) as minimum coverage set - mapped one-to-one onto pack playbooks in `owasp-sc-top10.md`.
- Methodology transparency norms from contest platforms (labeling AI-assisted findings explicitly, publishing test PoCs) - follow `contest-and-bounty-reporting` templates.
- Detector output NEVER auto-filed: dedupe/triage via `triaging-and-deduping-findings`; LLM-augmented tooling changed volume, not signal quality.

## Deprecations/slowdowns to reflect in old SOPs

- Mythril-class legacy symbolic executors recede from modern pipelines (still fine as secondary signals).
- Any doc telling you to run 2023-era Foundry flags or pre-Holocene gas assumptions needs chain-delta cross-checks against `reviewing-major-chain-surfaces` dossiers.

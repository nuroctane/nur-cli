# AI-audit tooling landscape (2026 refresh)

Catalog of AI x web3-security tooling, distilled from pashov's ai-web3-security hub (85 tools cataloged: 51 free/open-source, 34 paid) as of 2026-08. Use for tool selection, competitive awareness, and knowing what attackers and contest judges might run. Everything here is context - the pack's own playbooks remain the working method. `[verify]` anything version-specific before relying on it.

## Free / open-source highlights

| Tool | Publisher | What it is |
|------|-----------|------------|
| pashov/skills | pashov | Multi-pass Solidity audit skills (fizz fuzzing suite, solidity-auditor, x-ray git analysis) - methodology distilled into this pack |
| PlamenTSV/plamen | PlamenTSV | Autonomous web3 audit agent framework (18-100 agents, 40+ phases; methodology distilled here, orchestration deliberately excluded) |
| solskill | Cyfrin | Secure-dev guardrails while writing Solidity |
| contract-auditor | DarkNavy | Smart-contract audit skill (also in this pack's external kits) |
| zk-skills | zkSecurity | Circom circuit soundness/completeness/constraint review |
| solana-token-extensions-security | zzzuhaibmohd | Token-2022 extensions audit |
| sui-move-skill | ExVul | Sui Move audit skill |
| GPTScan | GPTScan | LLM + static analysis for logic bugs |
| grimoire | Joran Honig | Co-auditor skill that pairs with the user |
| weasel | slvDev | Conversational Solidity static analyzer |
| skills | Trail of Bits | Security dev/testing skills collection |
| flounder | adshao | Target prep / audit / exploit / proof automation |
| finite-monkey-engine | Brad Moon (UESTC) | AI audit engine |
| HackenProof skills | HackenProof | Bug-bounty triage skills |
| scoping-bee | 0xRayaa | Pre-audit scoping assistant |

Paid / closed (names to know, not endorsements): Sherlock Audit Engine + SherlockAI, Cantina Apex, CertiK AI Auditor, Zellic V12, Nethermind AuditAgent, Octane, ChainGPT Auditor, SolidityScan, Kritt.ai, Almanax. One-dollar-tier automated reports exist; treat their output as detector noise to triage, never as audits.

## Selection guidance

1. Pair LLM reviewers with static analysis ALWAYS (GPTScan-class + Slither/Aderyn) - either alone underperforms either combined.
2. For invariant/fuzz campaigns, prefer the pack's own pipeline (`discovering-protocol-properties` + `writing-foundry-invariant-handlers` + `fuzzing-with-echidna-and-medusa`); external suites are comparison baselines.
3. For zk/Move/Solana surfaces, the specialist skills above fill gaps this pack's playbooks reference (`reviewing-cairo-and-starknet`, `reviewing-move-modules`, `reviewing-solana-programs`).
4. Anything agentic that fans out subagent swarms (Plamen-style 18-100 agent runs) is reference-only: this pack's working rule is single-session sequential discipline (token-spend guardrails; AGENTS.md swarm-approval rule).

## Related

- `sc-research/references/sources.md` - X accounts + kits (human intel layer)
- `sc-research/references/tools-matrix.md` - static/symbolic/fuzz/recon tool matrix (non-AI)
- `defi-security-trends-standards/references/tooling-status.md` - currency status of the core toolchain

# CLI integration triage

Every item in the Raindrop `#cli-integration` tag, accounted for once,
with what nur-cli already had, what this commit added, and what was
deliberately left alone. No item is silently dropped.

## Count correction

The tag held **68 items** on 2026-11-04, not 61. The 61 figure was the
2026-11-02 count; 7 items were added 2026-11-03/04. The full list was
saved to `~/workspace/cli-integration-full.json` (id, title, link,
created, tags) outside the repo, so it survives `/tmp` wipes.

## Scope note

On 2026-11-02 the user said agent-desktop and everything below it had
already been taken care of by Tara. This pass accounts for those items
too, but it does not re-implement them: where the clone already contains
the integration, the verdict says so with evidence, and where it does
not, the verdict is research-only or unknown, not a new implementation
claimed on Tara's behalf.

## Verdict codes

- **INT** integrated already, with evidence in the clone
- **NEW** integrated in this commit
- **PAT** pattern adopted, not the upstream itself
- **SRC** market/source/registry only
- **JEV** Jev model-catalog entry only
- **RES** research-only, no code to integrate
- **UNK** unknown from title/excerpt alone, no clone evidence
- **CAND** candidate, not integrated in this commit

## New in this commit

| Item | What was added |
|---|---|
| cloudflare/security-audit-skill | `skills/security-audit/` adapted: six-phase coverage-ledger workflow, adversarial verifier rule, confirmed/needs_validation/rejected verdicts, `skills/security-audit/scripts/validate_findings.py`. Evidence layer is the existing `secscan` tool, not a new scanner. |
| dennisonbertram/developer-skill-pack | Subset only, adapted: `debug`, `regression-guard`, `research-spike`, `setup-repo`, `deploy-check`, `walk-the-issues`, `ux-flow` (paths + flow method, nur-cli browser driver). Skipped: native macOS skills, chief, coordinator agent files, Claude/Codex launchers and hooks. |
| peteromallet/desloppify | Ecosystem detection (`nur ecosystem ensure` reports it, detect-only) + `skills/desloppify/` thin skill (scan/plan/next/rescan, `.desloppify/` state). Not a replacement for `slop_scan`. Porting its Rust smell detectors into `slop_scan` is a follow-up code change, not in this commit. |
| lightpanda-io/browser | Ecosystem detection (detect-only, AGPL flagged) + `skills/lightpanda/` (fetch/CDP/MCP, WSL + telemetry caveats). Not a replacement for the `browser` tool, which keeps the user's logged-in Chrome session. |
| nandorojo/nando-skill | Pattern only: `skills/skill-authoring/` (entry point, references, executable lint, acceptance profile, eval). The upstream content is React/Effect-specific and was not vendored; no nando-skill directory exists in the clone. |
| scrawl.kitlangton.com | Merged into `skills/show-me-the-bug/`: Scrawl bundle read order, mark/click vocabulary, optional `scrawl` CLI zoom commands, Mac-first caveat. The `send` command was omitted (targets other agents' sessions). |
| deepseek-ai/dsh-superagent-prototype | Design doc only: `docs/superagent-design.md` (Mandate, Eval gate, structured Attention, Subscription resume, slot accounting). No code import; the prototype is explicitly research-only. |
| Secret wiring | Verified already shipped (masked modal, `$NAME` injection, redaction, pasted-credential catch); the gap closed in this commit was prompt guidance when no secrets are saved yet (see `src/secrets.rs`, `src/agent/prompt.rs`). |

## Full disposition

| Item | Date | Verdict |
|---|---|---|
| cloudflare/security-audit-skill | Nov 4 | NEW, skill |
| anthropics/claude-code CHANGELOG | Nov 4 | RES, parity watchlist only |
| dennisonbertram/developer-skill-pack | Nov 4 | NEW, subset only |
| bebuu_ai X: Claude Code usage-limit summary bug | Nov 4 | RES, design lesson: do not re-send a full subagent transcript on a timer to write a summary |
| peteromallet/desloppify | Nov 4 | NEW, detection + thin skill; Rust detector port is a follow-up |
| twid X: why Claude Code sounds human | Nov 3 | RES, prompt-craft note |
| owengretzinger X: CI speed TL;DR | Nov 3 | RES, checklist against `.github/workflows/validate.yml` at most |
| nandorojo/nando-skill | Nov 2 | NEW, pattern only (skill-authoring); content not vendored |
| lightpanda-io/browser | Nov 1 | NEW, detection + skill |
| deepseek-ai/dsh-superagent-prototype | Nov 1 | RES, design doc only |
| Scrawl | Nov 1 | NEW, merged into show-me-the-bug |
| ctatedev X: promo post | Oct 31 | RES |
| vercel-labs/agent-desktop | Oct 31 | INT as a backend, not the app UI (`src/tools/desktop.rs`, `docs/tools.md`) |
| Dagger for GitHub | Oct 31 | RES, not integrated |
| owengretzinger X: CI 20x faster | Oct 31 | RES, checklist only |
| Agentaps | Oct 30 | CAND, low: duplicates the TUI and `desktop` tool; not integrated |
| mazeincoding X: codebase slop indicators | Oct 29 | PAT/INT (`skills/codebase-slop/`, `src/tools/slop_scan.rs`) |
| Aidentity | Oct 28 | SRC/INT (public skill registry in `src/skills_market.rs`, `skills/market-sources.json`) |
| indexable-inc/blaze | Oct 27 | INT (cargo builds routed through blaze, opt-in, `src/tools/rust_builder.rs`) |
| neilsuperduper X: security tools | Oct 26 | PAT/INT (same scanner set `src/tools/secscan.rs` runs) |
| morphllm X: DeepSeek V4.1 Flash abliterated | Oct 23 | RES, model announcement |
| jc_builds X: fewer tokens via a codebase tool | Oct 23 | RES, low: goal already covered by `sgrep` + `repo_map` |
| lyc-aon/eros | Oct 23 | UNK |
| peter_szilagyi X: scratch workspace in /tmp | Oct 23 | PAT/INT (scratch rule in `skills/agents-md/SKILL.md`; nur uses `.nur/scratch/`) |
| treg.to/web-access | Oct 16 | INT (`src/tools/treg.rs`, `skills/web-access-treg/`) |
| openbin-ai/binny | Oct 15 | INT, pointer skill only (`skills/binny/`; nur does not decompile) |
| shipper_now X: epstein.md | Oct 15 | RES, design-style file |
| Ultramarine 1 Flash | Oct 15 | UNK/RES |
| Amos | Oct 14 | JEV/INT (Amos engines in `src/jev_catalog.json`) |
| mukaddamzaid/amos-1 collection | Oct 14 | JEV/INT, same source as above |
| interfaze-ai/interfaze-1-lite | Oct 14 | JEV/INT (`src/jev_catalog.json`, `nur jev start --engine interfaze-1-lite`) |
| context-dot-dev/sgrep | Oct 14 | INT (`src/tools/sgrep.rs`, ecosystem install) |
| S1Bench | Oct 13 | RES, benchmark scores only |
| Subquadratic | Oct 13 | RES |
| Liquid Inference | Oct 13 | JEV only, not a product integration |
| unreallabsai/unreal-agent | Oct 13 | RES |
| jia_seed X: GEO for devtool founders | Oct 1 | RES, marketing advice |
| JevBench | Oct 1 | RES, scores reflected in the Jev catalog |
| bisheshabramhacharya/token-ledger | Oct 1 | PAT/INT (parser-informed, license in `docs/ledger.md` + `third-party/token-ledger-LICENSE`; the macOS app is not integrated) |
| mizchi/jev-playground fit.md | Sep 24 | RES |
| trendtech X: startup-skill promo | Sep 22 | PAT (`skills/startup-skill/` is integrated; the post is promo) |
| Sherlockwz/T-Mem | Sep 22 | PAT/INT (bounded adaptation: future-query cues in `connectome`, not the upstream pipeline) |
| ryana/jevify | Sep 22 | RES, no clone evidence found |
| completeskeptic X: typesafe notes | Sep 22 | RES (the `typesafe` tool exists, but no evidence this post was its source) |
| FluidInference/FluidUse 0.2.0 | Sep 22 | RES, explicitly not integrated (`docs/jev-local.md`: Swift-only, no Python/HTTP bridge) |
| mizorewww X: CoreML power | Sep 20 | UNK/RES |
| brobro X: "Will work for stars" | Sep 3 | UNK |
| coworkerapp X: OM2 launch | Sep 3 | RES |
| Teknium X: Hermes /loop | Aug 14 | RES (nearest nur equivalents: `bg` jobs and heartbeat) |
| AutoDesign arXiv | Aug 14 | RES |
| c3ll256/dsh-toy | Aug 14 | RES (toy protocol, distinct from the DeepSeek CLI import) |
| gfodor/legal-skills | Aug 13 | RES, low CAND: a legal skill pack is a market-pack candidate only if the user wants that domain |
| droidrun/mobile-harness | Aug 10 | INT, ecosystem pack (`src/ecosystem/packs.rs`, delivered via `ecosystem ensure`, not vendored) |
| HelixDB/helix-db | Aug 10 | INT, optional backend (`src/agent/helix_memory.rs`, `mem` acceleration; the DB itself is external) |
| texttocad CAD Skills | Aug 9 | INT, ecosystem pack, partial (`skills/cad/` holds only scripts) |
| AWS Dogwood intro | Aug 8 | INT (`src/tools/dogwood_tool.rs`, `src/dogwood.rs`, ecosystem install) |
| Claude Code cross-session messaging docs | Aug 7 | PAT/INT (nur equivalent: `message` tool / mailbox) |
| nicopreme X: pi-subagents code mode | Aug 7 | INT (`skills/subagent-orchestration/`) |
| aienginerd X: MSW Kernel | Aug 7 | INT (`skills/msw-contract/`) |
| kane50613/takumi | Aug 7 | INT, skill only (`skills/takumi-usage/`); the Rust crate path was blocked upstream, so do not claim a `render_card` tool exists |
| mintlify/index | Aug 7 | RES, low CAND (retrieval engine would compete with `sgrep`/`graphify`) |
| cursor/plugins pstack | Aug 7 | RES |
| shepherd-agents/shepherd | Aug 6 | RES, no clone evidence found |
| nickscamara_ X: anydoc | Aug 6 | INT (`anydoc` tool in the roster) |
| PrimeIntellect-ai/prime-agent | Aug 5 | RES, no clone evidence found |
| kunchenguid X: pi agent dotfiles | Aug 2 | RES |
| ddoemonn/interior | Jul 31 | INT, skill (`skills/interior/`) |
| DeusData/codebase-memory-mcp | Jun 27 | RES, low CAND (code-graph MCP would overlap `graphify`/`sgrep`) |

## 2026-11-06 follow-up

The tag grew to **73 items** (68 + 5, added Nov 5–6). The full list is
saved to `~/workspace/cli-integration-current.json` outside the repo.
None of the 5 hits the Tara boundary. Full triage report:
`~/workspace/cli-integration-triage-2026-11-06.md`.

| Item | Date | Verdict |
|---|---|---|
| Executor — executor.sh | Nov 5 | INT already (`skills/executor-gateway/`, `executor` tool) |
| trashpandaemoji X: memory systems are mostly garbage | Nov 5 | PAT/INT — all three legs already exist (`agents-md`, skills, `mem`) |
| danlovesproofs X: unpinned GitHub Actions | Nov 6 | NEW — `workflows` check in `secscan` + CI/supply-chain ledger unit in `skills/security-audit/` |
| alezander9 X: subagents are the biggest cause of AI failures | Nov 6 | RES — docs-only note in `skills/subagent-orchestration/` |
| mainplane-ai/mainplane | Nov 6 | RES — competing harness, nothing to integrate |

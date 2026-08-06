# Integration gaps - status after deep pass

Written after the deep implementation pass (second substantial wave). Items marked **[DONE]** are implemented, compiled, and tested in this pass. **[PARTIAL]** = a safe subset landed. **[OPEN]** = still worth doing, best-effort deferred.

---

## 1. RLM paper (arXiv:2512.24601)

| Gap | Status | Notes |
|-----|--------|-------|
| Context-as-variable store | DONE (prior) | `context` tool, peek/slice/search/register |
| Compaction preserves store | DONE (prior) | inventory re-injected |
| **Budgeted recursion depth >1** | **[DONE]** | `config.subagent_depth` (default 1); `AgentRunner.subagent_depth` threaded through spawn; guard returns clear error at limit |
| Async subagent admission handles | **[OPEN]** | Returns report synchronously today; admission-handle model worth a follow-up |
| Programmatic REPL as *the* tool | OPEN | multi-provider friction; optional sidecar later |
| Explicit `code` context-manipulation tool | OPEN | `context` covers most |

## 2. Prime Agent

| Gap | Status | Notes |
|-----|--------|-------|
| Goals + quality gates | DONE (prior) + **[DONE `/gate`]** | `/gate <cmd>` view/set/clear; continuous DONE gated |
| Continual harness refine + rollback | DONE (prior) + **[DONE `/refine` / `/lessons`]** | slash routes added |
| **Heartbeat / schedule re-entry** | **[DONE `/heartbeat`]** | bg job pushes periodic steer; `/heartbeat off` stops |
| Handoff reason/role | DONE (prior) | agent schema + packet |
| TUI autonomous gate | **[DONE via `/gate` + continuous]** | documented |
| Agent-to-agent messaging | OPEN | gateway/daemon future |
| Skill creator packaging | OPEN | nur already has skills ecosystem |

## 3. AnyDoc (nickscamara)

| Gap | Status | Notes |
|-----|--------|-------|
| Local anydoc tool + feature | DONE (prior) | crate `anydoc` |
| Auto-register md into context | DONE (prior) | |
| Hosted OCR fallback for scanned PDFs | **[OPEN]** | needs Firecrawl API key handling; local anydoc correctly fails on scans |
| Batch convert directory | OPEN | minor |

## 4. Shepherd

| Gap | Status | Notes |
|-----|--------|-------|
| Proposal mode write_file/edit_file | DONE (prior) | |
| **Proposal mode apply_patch + multi_edit** | **[DONE]** | stage instead of mutate when `proposal_mode` on |
| RunStatus/Handoff receipt events | DONE (prior) | |
| Single-file `proposal apply <path>` | **[DONE via apply-all]** | apply_all merges staged set |
| Unix Landlock jail | **[DONE config flag]** | `config.landlock` (Linux-only enforcement; no-op elsewhere) |
| Three-way merge on moved workspace | OPEN | niche |
| Signature-as-permissions | OPEN | permissions.toml + plan mode cover |

## 5. OpenAI Agents SDK patterns

| Gap | Status | Notes |
|-----|--------|-------|
| Input/output/tool-arg guardrails | DONE (prior) | |
| **Handoff input filter** | **[DONE]** | `agent.context_files` lists files for child |
| **Structured span/OTLP export** | **[DONE]** | `receipt.export_spans` → `*.spans.jsonl`; wired into `/receipt` |
| Configurable guardrail packs | OPEN | |
| Handoff conversation surgery | OPEN (context_files is the safe subset) | |

## 6. Agent-native memory (arXiv:2606.24775)

| Gap | Status | Notes |
|-----|--------|-------|
| M1 hierarchical store | DONE (prior) | |
| M2 heuristic extract + explicit remember | DONE (prior) | |
| **M4 conflict/supersede** | **[DONE]** | `connectome supersede` demotes contradicting older memories |
| **Ops/cost counters (RQ5)** | **[DONE]** | `connectome status` shows remembers/recalls/consolidations/bytes |
| Model-based extraction (Mem0-class) | OPEN | heuristic is the no-dependency path |
| Vector route via ruflo | OPEN | optional embedding recall |
| Temporal KG / conflict graph | OPEN | overkill for v1 |

## 7. Connectome (animalabs.ai)

| Gap | Status | Notes |
|-----|--------|-------|
| Hierarchical tiers + first-person | DONE (prior) | |
| Append-only chronicle + checkpoints | DONE (prior) + **[DONE `/checkpoint`]** | |
| Soft restore as-of | DONE (prior) | `connectome restore` |
| On-policy model L1 at compact | **[PARTIAL]** | consolidation writes first-person L2; true model-written L1 needs a model call at compact (invasive) |
| **Wake / heartbeat** | **[DONE `/heartbeat`]** | schedule re-entry via bg |
| **Lessons library** | **[DONE `/lessons` + `/refine`]** | harness supplemental |
| KV-stable solver for chat prefix | OPEN | cache-optimization engineering |
| Recipe agent configs | OPEN | config.toml + skills approximate |

---

## Backlog (still OPEN, lowest marginal value / highest effort)

1. Async subagent admission handles (RLM `rlm()` model) — high value, moderate effort
2. Firecrawl OCR fallback — needs API key plumbing
3. Model-based memory extraction — opt-in extra model call
4. Programmatic REPL as optional single tool — multi-provider risk
5. Agent-to-agent messaging + daemon detach — product-scale
6. Full KV-stable compact solver — cache engineering

---

## Verification (this pass)

```bash
cargo check --bin nur          # clean
cargo test --bin nur           # 646 passed, 0 failed, 2 ignored
```

New in this pass:
- `config.subagent_depth` + recursion guard + `AgentRunner.subagent_depth`
- `/goal` persistent store bridge, `/checkpoint`, `/lessons`, `/refine`, `/proposal`, `/heartbeat`, `/gate`
- `agent.context_files` handoff input filter
- OTLP-flavoured span export (`receipt::export_spans`) wired into `/receipt`
- Proposal mode for `apply_patch` + `multi_edit`
- `connectome supersede` (conflict/supersede) + ops/cost counters in `connectome status`
- `config.landlock` opt-in (Linux)

# Integration report: RLM · Prime · Shepherd · AnyDoc · OpenAI Agents → nur-cli

This is the satisfaction report for the amalgamation work. Do not ship until you have re-read this and the tests green on your machine.

## What was implemented (and why it was worth it)

### 1. RLM context store (from arXiv:2512.24601 + Prime RLM runtime)

**Worth it:** Highest leverage for every provider. Long tool dumps and documents no longer only live as lossy chat tokens.

| Piece | Location |
|-------|----------|
| Named variables, peek/slice/search/register/delete | `src/agent/context_store.rs` |
| Tool surface | `src/tools/context_tool.rs` (`context`) |
| Auto-register large tool results before compress/spill | `src/headroom.rs` `prepare_tool_body` |
| Survive compaction (inventory re-injected) | `src/agent/loop.rs` `compact_session` |
| Prompt inventory | `src/agent/prompt.rs` |

**Edge cases handled (cross-checked with paper/Prime):**
- Sensitive bodies refused (no sk-/pem into store)
- Oversized vars spill under `~/.nur/context-store/` and stay addressable
- Max 256 vars/session with oldest eviction
- Session-scoped; subagents do not inherit parent vars unless shared session id
- Compaction does **not** clear the store (Prime: kernel/state survives)

**Better later:** true async `rlm()` admission handles (non-blocking children), optional REPL tool, recursive depth >1 with budgets.

### 2. Persistent goals + quality gates (Prime long-running)

| Piece | Location |
|-------|----------|
| Goal state machine | `src/agent/goal.rs` |
| Tool `goal` | `src/tools/goal_tool.rs` |
| Prompt injection when active | `src/agent/prompt.rs` |
| Token attribution | `src/agent/loop.rs` end-of-turn |
| Quality gate on continuous DONE | `src/agent/continuous.rs` + `src/main.rs` |
| Config `quality_gate` | `src/config.rs` |

**Worth it:** Continuous mode no longer trusts bare "DONE" when a gate is configured (Prime autonomous gate pattern).

**Edge cases:** empty gate = no-op; failed gate injects failure text and continues; budget exhaustion → `Exhausted`.

### 3. Continual harness refine (Prime `/refine` lite)

| Piece | Location |
|-------|----------|
| Supplemental notes + snapshots + rollback | `src/agent/harness.rs` |
| Tool `harness` | `src/tools/harness_tool.rs` |

**Worth it:** Evidence-backed operating notes without rewriting the base system prompt (Prime invariant).

**Better later:** promote refined notes to OptMem/PLUR only with explicit user approval; global vs session scope switch.

### 4. Shepherd retained outputs + run markers

| Piece | Location |
|-------|----------|
| Stage/list/apply/discard | `src/agent/proposal.rs` |
| Tool `proposal` | `src/tools/proposal_tool.rs` |
| `write_file` stages when `proposal_mode=true` | `src/tools/write_file.rs` |
| Receipt `RunStatus` / `Handoff` events | `src/agent/receipt.rs` |
| Config `proposal_mode` | `src/config.rs` |

**Worth it:** Windows-safe "retained output → apply/discard" without Seatbelt/Landlock. Matches Shepherd product intent, not the Unix jail implementation.

**Edge cases:** apply resolves paths through workspace sandbox; discard removes proposal tree; default off so existing UX unchanged.

**Better later:** proposal mode for `edit_file`/`apply_patch`/`multi_edit`; three-way merge like Shepherd `apply` on moved workspaces; optional Landlock on Linux.

### 5. Portable guardrails (OpenAI Agents SDK)

| Piece | Location |
|-------|----------|
| Input / output / tool-arg checks | `src/agent/guardrails.rs` |
| Input block at turn start | `src/agent/loop.rs` |
| Tool-arg block before hooks | `src/agent/loop.rs` |
| Output warn on final answer | `src/agent/loop.rs` |

**Worth it:** Provider-agnostic tripwires without OpenAI SDK lock-in.

**Edge cases:** PEM private keys block; sk-/ghp-/xox warn; `rm -rf /` and mkfs patterns block.

**Better later:** configurable guardrail packs; model-based tripwires as opt-in.

### 6. Handoff packet fields (OpenAI Agents SDK)

`agent` tool accepts optional `reason` + `handoff_role`; appended as a handoff packet on the child prompt (`src/agent/loop.rs` parse + `src/tools/mod.rs` schema).

### 7. AnyDoc document conversion (Firecrawl / nickscamara)

| Piece | Location |
|-------|----------|
| Optional crate feature `anydoc` (default on) | `Cargo.toml` |
| Tool `anydoc` | `src/tools/anydoc_tool.rs` |
| API: `anydoc::to_markdown` (verified against crates.io 0.1.6 lib.rs) | crate |
| CLI fallback | `anydoc` / `firecrawl-anydoc` on PATH |
| Auto-register into context store | tool `register` default true |

**Edge cases from upstream:** scanned PDFs unsupported (OCR is hosted Firecrawl Parse); CSV needs extension; format detected from bytes first.

### 8. Cross-provider subagent reliability (earlier this session)

NL "OpenAI agent strategies" no longer routes; OAuth > stale keys; Grok JWT fingerprint headers; failures report route.

---

## How layers now touch the model (any provider)

```
User message
  → input guardrails
  → system prompt = base + project + skills + goal + harness + context inventory
  → model (Responses / Chat / Anthropic / Gemini / Cursor CLI / …)
  → tool calls
       → tool-arg guardrails → hooks → tools
       → large results → context_store register → headroom compress → spill
  → final text → output guardrails
  → goal token attribution
Compaction
  → summary + context_store inventory (vars preserved)
Continuous
  → DONE only if quality_gate passes
Writes (optional)
  → proposal stage until apply
```

---

## Config knobs (new)

```toml
proposal_mode = false
quality_gate = ""                 # e.g. "cargo test --bin nur -- agent::"
context_register_min_chars = 8000 # 0 disables auto-register
```

---

## Tests run (representative)

- `agent::` suite: **107 passed** (includes context_store, goal, proposal, harness, loop routing, swarm, subagent)
- Prior targeted: cross-provider NL routing, credential pick order

Re-run before ship:

```bash
cargo test --bin nur
cargo test --bin nur tools::
```

---

## What was intentionally not forked wholesale

| Stack | Not pulled | Why |
|-------|------------|-----|
| Prime TypeScript + IPython kernel | Full REPL as only tool | Multi-provider wire formats + Windows; optional later as sidecar |
| Prime daemon supervisor | Process isolation for detach | Fractal/bg exist; full daemon is Unix product work |
| Shepherd OS jails | Seatbelt/Landlock | Windows primary; proposal mode is the portable half |
| OpenAI Agents Python SDK | Runner/SDK types | Nur owns multi-provider loop; primitives only |
| AnyDoc OCR | Hosted only | Local crate correctly refuses scanned PDFs |

---

## How it could be better (before you have to say it)

1. **Proposal mode for all mutators** (`edit_file`, `apply_patch`, `bash` redirects) - only `write_file` today.
2. **Async subagent handles** - Prime `rlm()` returns admission, not answer; nur still sync-report by default.
3. **Handoff input filters** - OpenAI SDK can filter conversation history per handoff; we only add reason/role text.
4. **Trace export** - receipts gained events but no OTLP/JSON-L span export for external viewers.
5. **REPL tool** - starlark/python sandbox as single optional tool would complete the RLM programming model.
6. **anydoc binary size** - feature is default-on; document `--no-default-features --features image-peek` for lean builds if size bites.
7. **Quality gate workspace dirty check** - Prime avoids re-running the same failed gate when workspace unchanged; we re-run every DONE.

---

## Source cross-check summary

| Claim in code | Upstream check |
|---------------|----------------|
| RLM prompt-as-variable | arXiv 2512.24601 abstract + §2 |
| Compaction preserves external state | Prime docs/rlm.md + long-running-agents.md |
| Goal.complete is success signal | Prime long-running-agents.md |
| Refine never rewrites base prompt | Prime README Continual Harness |
| Retained outputs select/apply/discard | Shepherd README + concepts/runs.md |
| anydoc::to_markdown | crates.io anydoc-0.1.6 `src/lib.rs` lines 100–112 |
| Handoffs / guardrails | openai.github.io/openai-agents-python |

---

## 9. Agent-native memory + Connectome (added)

### Implemented

| Module | Path | Upstream |
|--------|------|----------|
| M1–M4 native memory | `src/agent/native_memory.rs` | arXiv:2606.24775 |
| Append-only chronicle + checkpoints | `src/agent/chronicle.rs` | Connectome / Chronicle lite |
| Tool `connectome` | `src/tools/connectome_tool.rs` | both |
| Prompt inject + compact maintenance | `prompt.rs`, `loop.rs` compact | M3 + M4 + KV-stable edge |
| Config `native_memory` | `config.rs` (default true) | |

### Worth it

- Paper: treat memory as a **lifecycle data plane**, not one more RAG call.
- Connectome: **first-person hierarchical memory** + **append-only chronicle** so compaction no longer equals identity death.
- Localized consolidation matches the paper's cost finding and Connectome's "loss of resolution ≠ loss of record".

### Edge cases

- Secrets refused on remember
- Soft-delete (retired) keeps audit trail in JSONL
- restore is **describe as-of**, never rewrites events
- Scope is `project:session` for multi-project machines
- Complements memory.md / OptMem / PLUR / ruflo — does not replace

### Better later

- On-policy L1 writing via a cheap model call (true Connectome autobiographical strategy)
- Full Chronicle branch/fork (Rust N-API package) as optional ecosystem binary
- Query routing by workload class (paper: structure matches bottleneck)
- Multi-participant membrane for gateway chat

### Tests

- `native_memory` 3 tests, `chronicle` 1 test, agent suite **111 passed**

*Implementation lives in-tree. Ship only after full `cargo test --bin nur` and a manual smoke of `context` / `anydoc` / `goal` / `proposal` / `harness` / `connectome`.*


## 10. Deep pass: ipython REPL + async admission + model memory + KV-stable

All implemented except Firecrawl (explicitly excluded).

### ipython / RLM REPL (`repl` tool + `src/agent/repl.rs`)
- Prime-faithful persistent Python REPL: one long-lived interpreter per session whose
  variables/imports/functions/state survive across turns AND compaction.
- Actions: exec / expr / bash (%%bash temp subshell, python state persists) / cd / status/list/kill.
- Newline-framed sentinel protocol (OK ... ERR ... END) - reliable on Windows.
- `python_bin()` probes for a real interpreter (avoids MS Store stub) via `NUR_REPL_PYTHON` override.
- Tests prove state persistence: define `answer=42`, later `expr answer` → `42`.

### Async subagent admission (`admission` tool + `src/agent/admission.rs`)
- `agent async=true` returns a handle id immediately and runs the child in the
  background (Prime `rlm()` admission model); parent keeps working.
- Results land in `~/.nur/admissions/<session>/<id>.json`; poll with `admission get/list`.

### Model-based memory extraction (paper M2, Mem0-class)
- `memory_model_extract=true` (opt-in): at turn end the active model extracts
  durable first-person memories with a cheap minimal-effort call.
- `model_extract_prompt` / `parse_model_extraction` ([I] first-person / [O] observed).

### Agent-to-agent messaging (`message` tool + `src/agent/mailbox.rs`)
- Durable mailbox per project scope: `message send to=<agent|all>`, `message recv`
  (marks delivered), `message status`. No daemon required.

### KV-stable compaction (Connectome)
- `kv_stable_compact=true` (default): compact rebuilds as [stable summary prefix] +
  [recent verbatim tail]; recent edge carried verbatim, prompt-cache prefix preserved.

### Slash/TUI wiring (prior gap items)
- `/checkpoint`, `/lessons`, `/refine`, `/proposal`, `/heartbeat [interval] [note]`, `/gate [cmd]`
- `/goal` now backed by persistent goal store (set/get/pause/resume/complete/clear)

### Other prior gaps now closed
- Proposal mode for apply_patch + multi_edit (Shepherd)
- `agent.context_files` handoff input filter (OpenAI SDK)
- OTLP-flavoured span export (`receipt::export_spans`) wired into `/receipt`
- `connectome supersede` (conflict/supersede) + ops/cost counters
- `config.landlock` opt-in (Linux-only)
- `config.subagent_depth` RLM recursion + guard

### Still OPEN (deferred, per user instruction to skip Firecrawl only; these are
   candidates for future passes)
- Firecrawl OCR fallback (EXCLUDED by user)
- Full single-thread REPL autosave of variables across process restarts (state is
  per-process today; survives turns/compaction, not a full kernel restart)
- Vector/graph memory indexes (paper M1 advanced options)

Verification: `cargo build --bin nur` clean, `cargo test --bin nur` = 652 passed / 0 failed.

## 11. Final pass: remaining open items + bug review cross-reference

Closed the two remaining open items and re-verified against source specs.

### REPL state across process restarts (was open)
- Kernel now pickles module-level user state to `~/.nur/repl/<name>/state.pkl` after
  each successful exec; on spawn it restores from the pickle.
- History of executed cells appended to `history.jsonl` (audit/rollback).
- `kill` clears persisted state ("state cleared" contract); a real process exit
  (crash/detach) preserves it so the next session resumes.
- Test `state_persists_across_process_restart` kills the live subprocess (no wipe)
  then re-spawns and confirms `persisted == 99` restored.
- Cross-check: Prime rlm.md "Python state survives across tool calls and
  compaction" — now extended to survive a full process restart (the Connectome
  "same agent in March and November" continuity goal).

### Vector/graph memory index (was open; paper M1)
- Inverted index (term → doc frequency) built per recall; rare/specific terms get
  an idf-like boost so niche facts route better (no vector-DB dependency).
- Entity-triple graph (`connectome graph`): heuristic `subject -[rel]-> object`
  extraction (`uses`, `depends on`, `requires`, …) for graph-style traversal.
- Tests `inverted_index_boosts_rare_term_recall` and
  `graph_extracts_entity_triples`.
- Cross-check: arXiv:2606.24775 M1.3.1.2 (physical storage + indexing) and the
  paper's finding that "no single architecture dominates — structure must match
  the workload" → we add index + graph as complements, not replacements.

### Bug review / spec cross-reference (s3)
- **RLM paper** "prompt as external env + recursive subcalls" → context store +
  `subagent_depth` recursion + async `agent async=true` admission. ✓
- **Prime rlm.md** subagent admission handle / result-via-poll → `admission` tool. ✓
- **Prime** fan-out is the "await all for synthesis" path; single-call supports
  async — a faithful split, not a bug. ✓
- **Connectome** "nothing is ever deleted; loss of resolution ≠ loss of record" →
  consolidate/supersede mark `retired`/demote confidence but KEEP rows in the
  JSONL archive; `load_entries` only filters for recall. Verified. ✓
- **Connectome** "agent writes its own memories first-person" → model-extract
  `[I]`→FirstPerson; heuristic→Observed (honest default). ✓
- **Prime** "refine never rewrites base system prompt" → harness only supplements. ✓
- Clippy clean in new code (fixed sort_by_key/clamp/char-compare nits).

Verification: `cargo test --bin nur` = 655 passed / 0 failed; clean build.

## 12. Real embeddings + real knowledge graph + central memory router

The "vector/graph" system is now a genuine embedding/KG system, and the memory
routing problem is solved with a central router.

### What changed (honest)
- **Real embeddings** (`embed.rs`): API embeddings via the active provider +
  honest local n-gram fallback (labeled NOT semantic). L2-normalized, 256-dim,
  comparable across paths.
- **Real vector store** (`memory_vector.rs`): persistent cosine top-k per scope.
- **Real knowledge graph** (`memory_graph.rs`): persistent nodes/edges, neighbors,
  BFS shortest-path. The old keyword "triple" heuristic was removed as dead code —
  the graph is now real storage+traversal (extraction remains heuristic, honestly).
- **Central router** (`memory_router.rs`) + `mem` tool: intent-classified read/write
  that fans out across vector+graph+hierarchical and auto-indexes on write.
- `native_memory::remember` auto-embeds + absorbs into the graph.
- Prompt injects routing_guidance + a bounded routed-memory snapshot.

### Tests
- embed (3), memory_vector (2), memory_graph (2), memory_router (2) all green.
- Full suite **664 passed / 0 failed**. Clean build (0 warnings).

### Honesty notes
- API embeddings are real semantic vectors when a key/model is available; the
  local fallback is a hash embedding with a documented caveat.
- KG entity/relation extraction is heuristic; graph STORAGE/TRAVERSAL is real.

## 13. V0.27.11 re-evaluation fixes (post-release audit)

After shipping v0.27.10, a hard re-audit closed these real gaps:

### e1 — embedding honesty / mode
- Added `nur.memory_embed_mode` = `auto|api|local`. `auto` (default) uses the
  provider embeddings API and falls back to the honest local n-gram hash embed;
  `local` never calls the provider. `embed_with_source()` reports api|local per
  vector for honest telemetry; `vector` tool output shows source distribution.

### e2 — knowledge-graph extraction
- Expanded relation vocabulary (adds is_a, kind_of, owned_by, managed_by, builds,
  installs, replaces, supersedes) so more phrasings become edges. Extraction
  remains heuristic (documented honestly) but the graph storage/traversal is real.

### e3 — routing completeness
- `mem read` now actually wires `ExactDoc` intent → RLM context_store inventory and
  `Message` intent → mailbox, instead of only recognizing them in classification.

### e4 — vector/graph coherence
- `consolidate_localized` now removes vectors for retired entries AND indexes the
  new L2 era note, so the vector store never surfaces archived/consolidated rows.
  (Previously retired memories kept stale vectors and the new note wasn't indexed.)

### e5 — efficiency + dedup + bloat
- `mem write` now semantically dedups (vector above() threshold 0.92) — a
  near-identical existing memory is reported instead of duplicated.
- `routing_guidance` prompt block trimmed ~60% to cut per-turn token cost.
- Full suite 665 passed / 0 failed.

Reshipped as v0.27.11 (patch) so the already-live v0.27.10 release stays immutable.

# How memory systems are picked today (and how we'll optimize)

Investigation into the current routing/selection of nur's memory systems, written
before building the real embedding/KG layer so the "before" is honest.

## The residents (who exists today, roughly oldest → newest)

| System | Where | Kind | What it stores |
|--------|-------|------|----------------|
| `memory.md` | `~/.nur/memory.md` | plain journal | durable cross-session notes, user prefs, project facts (excerpt injected) |
| **OptMem** | `~/.optmem` (upstream-pure) | weighted token memory | peeled/consolidated permanent memory; `/memo` |
| **PLUR** | `~/.plur` | engram YAML | corrections/preferences, recency+BM25 recall; injected each session |
| **ruflo** | `~/.nur/ruflo/` (Meta DB) | AgentDB vector store (via CLI) | embedding/pattern memory, swarm/hive |
| **context_store** | `~/.nur/context-store/` | RLM prompt-as-variable | large tool results / docs kept outside the window; peek/slice/search |
| **native_memory** | `~/.nur/native-memory/` | hierarchical tiers (recent/l1/l2/l3) | agent-native memoirs from M1–M4 framework |
| **chronicle** | `~/.nur/chronicle/` | append-only event log | every event; checkpoints; time-travel |
| **harness** (Continual) | `~/.nur/harness/` | refined supplemental lessons | `/refine` notes + rollback |
| **goal** | `~/.nur/goals/` | persistent goal state | active objective + token budget |
| **mailbox** | `~/.nur/messages/` | durable agent-to-agent messages | sibling/orchestration |

## How the model "picks" one today

There is **no central router**. Selection is emergent from two implicit channels:

1. **Prompt injection stacking** (`src/agent/prompt.rs`): every turn the system
   prompt concatenates, in order:
   - `memory` (memory.md excerpt),
   - `plur` inject,
   - `optmem` prompt block,
   - `goal::prompt_block(sid)`,
   - `harness::prompt_block(sid)`,
   - `context_store::prompt_inventory(sid)`,
   - `native_memory::prompt_block(mem_scope, "", 2500)` (empty query → recency/confidence),
   - (new) any routed memory we add.

   The model "sees" whatever is injected and has to synthesize across them.
   Nothing tells it which to *write* to or query.

2. **Tool descriptions**: `connectome`, `memory`, `ruflo`, `optmem`, `plur`,
   `context`, `repl`, `message`, `goal`, `harness`, `proposal` each advertise
   themselves independently. The model is expected to read the descriptions and
   choose. There is overlap (ruflo is "vector", connectome is "memory + graph",
   native_memory is "hierarchical") with no disambiguation.

### Problems this creates

- **Ambiguity**: when the model decides "remember X," it can land in
  `memory.md`, `connectome remember`, `ruflo memory_store`, `optmem note`, or
  `harness refine` — each is plausible. Result is duplicated/parallel memories.
- **Prompt bloat**: every injected block costs tokens every turn, and
  per-turn hidden work (model-extract, auto-register) can stack.
- **No single read path**: a query has no one "ask the memory" entry point; the
  model must decide which tool to query and merge results.
- **No efficiency guardrail**: nothing routes by *retention vs. cost* — a trivial
  fact and a critical one cost the same to store/recall.

## What we're building to fix it (m2–m5)

1. **Real embeddings in a real vector store** (m2): pluggable embedding (any
   provider via the resolvable API client, plus an honest local fallback) stored
   with cosine similarity — the "real vector graph system" asked for. No longer
   keyword heuristics relabeled as vectors.
2. **Real knowledge graph** (m3): persistent nodes/edges, neighbors + path
   traversal — replaces the string-match "triples" with a real graph store.
3. **Central memory router** (m4): a single decision table that maps an intent
   (retention class × retrieval need × cost tier) to the correct resident(s),
   and a harmonized `mem write` / `mem read` tool + prompt block that lists the
   residents and *which one to route where*. The model stops guessing.

### Optimization axes (efficiency vs. contextual fullness)

| Query/intent kind | Cheapest faithful resident | Richest resident |
|-------------------|---------------------------|------------------|
| "what did I decide / prefer" | memory.md / PLUR | native_memory vector + KG relation |
| "relationship / connections" | PLUR | knowledge graph (neighbors/path) |
| "semantic similarity" (not keyword) | — | vector store (embeddings) |
| "exact doc / tool dump I saw" | context_store grep/peek | context_store slice |
| "long-term arc / identity" | OptMem | native_memory L1–L3 + chronicle |
| "what another agent said" | mailbox | mailbox + chronicle |

The router will pick by retention class and let cost tier decide depth: injection
vs. on-demand tool query. Efficiency means *not* slamming every resident into the
prompt; contextual fullness means the right resident for the right question, on
demand.

---

## Implementation status (post-build, honest)

### Real embeddings (m2)
- `src/agent/embed.rs`: pluggable embedding. `embed_api` POSTs `<active base>/embeddings`
  with the provider key + model (`nur.memory_embed_model`, default `text-embedding-3-small`).
  `embed_local` is an **honest bag-of-character-n-grams hash embedding** — dimensionally
  valid for cosine and fully offline, but **NOT semantic/neural**. It is labeled as such
  in code and docs. `embed()` prefers API, falls back to local; every vector is
  L2-normalized and fixed at 256 dims so both paths are comparable.
- `src/agent/memory_vector.rs`: persistent cosine top-k vector store per scope
  (`vectors.json`), keyed by memory entry id. `/recall` and `mem vector` use it.

### Real knowledge graph (m3)
- `src/agent/memory_graph.rs`: persistent nodes + typed edges (`graph.json`) with
  neighbors() and BFS shortest-path traversal. **Replaces the old string-match
  "triples"** entirely — the heuristic MemoryTriple/extract_triples code was
  removed as dead. Node/edge *extraction* is still heuristic (no NLP dep), but the
  *storage + traversal* is a real graph: `mem graph neighbors|path` and `connectome graph`.

### Central router (m4)
- `src/agent/memory_router.rs`: `intent_class()` maps a query to a retention class;
  `write()` stores once (dedup) and auto-indexes vector+graph; `read()` fans out
  across vector + graph + hierarchical and merges. `mem` tool action=read/write/
  vector/graph/guidance. Prompt injects `routing_guidance()` so the model stops
  guessing which resident to use.
- `native_memory::remember` now auto-embeds into the vector store and absorbs into
  the knowledge graph on every write.

### Optimization realized
- The model no longer has to pick among 6+ memory systems blindly; the router
  decides by intent and the `mem` tool is the one-stop read. Efficiency: injection is
  bounded (snapshot only when content exists); fullness: vector/graph on demand.

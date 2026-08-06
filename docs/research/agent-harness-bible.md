# NurCLI Agent Harness Bible

Primary-source research for folding the best of Prime Agent (RLM + Continual Harness), Shepherd (reversible traces), Firecrawl AnyDoc, OpenAI Agents SDK patterns, the RLM paper, **agent-native memory (arXiv:2606.24775)**, and **Anima Connectome** into nur-cli.

This document is the integration map. Claims cite the source that owns them. It is not an implementation plan with dates.

---

## 0. What "best of all worlds" means for nur-cli

Nur already has a strong multi-provider coding loop (`src/agent/loop.rs`), cross-provider subagents, skills, swarm, headroom compaction, fractal worktrees, OptMem/PLUR/ruflo memory, and a large tool surface.

The sources in this bible push five **orthogonal** axes that nur only partially covers:

| Axis | Best source | Nur today | Gap |
|------|-------------|-----------|-----|
| **Programmatic context control** (prompt-as-variable, REPL state survives compaction) | RLM paper + Prime Agent RLM runtime | Transcript + headroom summarize; tools are schema-fixed | No persistent REPL; context is chat history, not addressable variables |
| **Self-improving harness state** (CRUD on prompts/skills/memory/subagent specs with rollback) | Prime Continual Harness `/refine` | Skills + memory are external; no trajectory-backed refine | No evidence-gated harness mutation with snapshots |
| **Long-running continuity** (daemon workers, goals, heartbeats, schedules, detach) | Prime long-running agents | bg jobs, fractal, continuous; no daemon session worker | Detach/reattach and scheduled re-entry are weaker |
| **Reversible, reviewable execution** (retained outputs, OS-enforced grants, traces) | Shepherd | Approvals + undo + receipts; writes hit workspace live | No retained-output / select-apply-discard gate; Windows-first limits OS jails |
| **Document ingestion as a first-class local tool** | Firecrawl anydoc (nickscamara) | web_fetch + look; no office→markdown engine | PDF/DOCX/PPTX local parse missing |
| **Portable multi-agent orchestration primitives** | OpenAI Agents SDK | agent tool + handoff-ish subagents | Guardrails, formal handoffs, tracing/sessions as first-class types |
| **Agent-native memory (M1–M4 data plane)** | arXiv:2606.24775 | memory.md + OptMem + PLUR + ruflo | No unified extract/retrieve/maintain lifecycle |
| **Long-horizon identity continuity** | Anima Connectome | Session files + compaction | Compaction rewrites live edge; no hierarchical self-authored memory curve |

**Integration principle:** absorb **patterns**, not whole Python/TS stacks. Nur stays a Rust multi-provider TUI agent. Foreign runtimes (IPython kernel, Shepherd placement jails, Connectome Bun host) may be optional sidecars, never hard requirements on Windows.

---

## 1. Recursive Language Models (paper)

**Source:** Zhang, Kraska, Khattab. *Recursive Language Models*. arXiv:2512.24601v3 (11 May 2026). https://arxiv.org/abs/2512.24601 · https://arxiv.org/pdf/2512.24601 · code https://github.com/alexzhang13/rlm

### 1.1 Definition (owned by the paper)

> "We propose Recursive Language Models (RLMs), a general inference paradigm that treats long prompts as part of an external environment and allows the LLM to programmatically examine, decompose, and recursively call itself over snippets of the prompt."

Key properties claimed:

- Process inputs **more than an order of magnitude beyond** model context window limits.
- Even for shorter prompts, quality beats vanilla frontier models and common scaffolds (compaction, CodeAct-with-subcalls, Claude Code) at **comparable cost** (median deltas reported for GPT-5 baseline comparisons in abstract).
- Post-trained small model **RLM-Qwen3-8B** outperforms base Qwen3-8B (median ~28%) and approaches vanilla GPT-5 on some long-context tasks.

### 1.2 Core mechanism

1. **Prompt lives outside the window** as an environment object (variable), not stuffed into the transformer context.
2. The model writes **code** (REPL / programmatic actions) to slice, search, filter, and transform that object.
3. The model can **recursively invoke** an RLM/LLM over chosen snippets (`rlm(...)` style subcalls).
4. Final answer is assembled from programmatic aggregation, not from a single forward pass over the full corpus.

This is **inference-time scaling for context**, complementary to architecture/training progress and complementary to file-based agent scaffolding (Claude Code / Codex style).

### 1.3 Contrast with common scaffolds (paper framing + Prime RLM blog)

| Scaffold | How it fights context rot | RLM difference |
|----------|---------------------------|----------------|
| Summarization / compaction | Compress history into shorter text | Lossy; RLM keeps full prompt addressable outside the window |
| File-system agent loops | State in files; succession of agents | RLM keeps a continual rollout with programmatic view of the prompt |
| Context folding | Branch/return inside context | Compatible idea; RLM emphasizes external env + recursive subcalls |
| Fixed tool schemas | Model picks from harness-defined tools | RLM prefers **code as the tool surface** |

### 1.4 Portable implementation sketch for nur-cli

Not a full IPython port. Minimal RLM-compatible substrate:

1. **`context_store`**: large inputs (files, tool dumps, PDFs-as-md, session artifacts) registered as named variables with ids, sizes, and peek/slice APIs.
2. **`rlm_peek` / `rlm_slice` / `rlm_search` tools** (or one `context` tool with actions) so any provider can programmatically examine stores without stuffing them into the next request.
3. **Recursive `agent` already exists** - align it with RLM semantics:
   - admission handle (do not block parent on child completion by default - optional async mode);
   - results via explicit messages/files (nur already returns a report string - keep that, add optional artifact path);
   - depth limit already 1 - make configurable for RLM-style depth.
4. **Compaction policy**: when headroom fires, **do not drop** context_store variables; only summarize chat turns. Kernel/state continuity is the RLM invariant Prime implements with IPython.
5. Optional later: embed a sandboxed JS/Python REPL as a **single** model tool (Prime's `ipython`) for providers that benefit from CodeAct-style programming.

**Acceptance signal from the paper:** quality on long-context tasks should hold as input grows past the model window, with cost comparable to compaction baselines - not "always more tools."

---

## 2. Prime Agent (Prime Intellect)

**Sources:**

- Repo: https://github.com/PrimeIntellect-ai/prime-agent (MIT)
- README (main)
- Docs: `packages/coding-agent/docs/{architecture,rlm,long-running-agents,skills,quickstart}.md`
- Blog: https://www.primeintellect.ai/blog/prime-agent (2026-08-05)
- RLM product blog: https://www.primeintellect.ai/blog/rlm
- Related: verifiers, prime-rl, pi-mono / pi TUI base
- X thread cited by user: https://x.com/PrimeIntellect/status/2085086999267144083 (promotes the launch; authoritative detail is the blog + docs - full tweet body not machine-readable here without X session)

### 2.1 Product definition (README + blog)

Prime Agent is an open-source **coding and research agent** for general and **long-running** work, built on two abstractions:

1. **RLM** - context as variables (*prompt-as-a-variable*); recursive subagents as function calls (*programmatic tool / sub-agent calling*) inside a **persistent REPL**.
2. **Continual Harness** - supplemental prompts, memories, skill descriptions, and reusable subagent specs as **durable state** the agent can refine via small, evidence-backed updates (session-local by default). `/refine` never rewrites the immutable base system prompt; snapshots support rollback.

Blog thesis (paraphrase of first-party text): modern harnesses were built for earlier models - fixed tool schemas and compaction force the model to work **around** scaffolding; static subagents/prompts/skills/memory never adapt. Prime wants harnesses that **extrapolate** on current model capability.

Claimed eval highlight (blog): with Opus 5, **95.5% on ARC-AGI-3**, above reported human expert baseline (treat as product claim; verify methodology on their eval writeup before marketing).

### 2.2 Architecture (docs/architecture.md)

Separation of concerns:

| Layer | Owns |
|-------|------|
| Interactive TUI / headless clients | Rendering, input, UI prefs - **not** execution |
| Daemon supervisor | Routing, attachments, recovery, cross-agent messages |
| Session worker | One root `AgentSessionRuntime`, scheduler, kernels, RLM children |
| `AgentSession` | Provider calls, queues, tools, compaction, goals, children, transcript |
| IPython kernel | Model-facing control environment (not a security sandbox) |
| Storage | JSONL transcripts + session artifacts |

Prompt flow: UI → AgentConnection → Supervisor → Worker → AgentSession → Provider; IPython tool calls execute in kernel; typed host requests return to TypeScript session for authoritative ops.

**Nur mapping:** nur's TUI + `AgentRunner` are closer to a **single-process** AgentSession. `bg` / fractal are partial worker isolation. A future "daemon mode" would be the largest structural borrow.

### 2.3 RLM programming model (docs/rlm.md)

Invariants:

1. **Execution is programmatic** - default built-in model tool is `ipython`. Files, shell, skills, subagents start from the kernel. Python state survives tool calls and compaction.
2. **Subagents are native `rlm(...)` calls** - return **admission handles**, not answers. Results via `agent_message` or files. Parent-scoped child registry survives compaction/restart. Configurable recursion depth.
3. **Skills are progressive + optionally executable** - Agent Skills `SKILL.md` plus Python packages importable in-kernel. Metadata in startup prompt; full skill loaded on match.
4. **State outlives one turn** - compaction, daemon workers, child registries, heartbeats, goals, autonomous mode.

Host bridge: Python skills call `rlm.host_request(...)` for goals, messages, heartbeats, compact - TypeScript owns credentials, providers, transcripts, scheduling.

Trust model: **not a security sandbox** - same OS perms as user.

### 2.4 Long-running agents (docs/long-running-agents.md)

| Feature | Behavior |
|---------|----------|
| Daemon-backed sessions | Detach TUI without killing worker; `attach` / `agents` / `status` / `doctor` |
| Agent-to-agent messages | `auto` / `steer` / `follow_up` delivery; family roster; rate limits |
| Heartbeats | User `/heartbeat`, agent `rlm_heartbeat`, cron `prime-agent schedule` |
| Persistent goals | `/goal` durable objective; `goal.complete()` is success signal |
| Autonomous mode | Bounded continuations + optional quality gates (e.g. `npm run check`) |
| Compaction | Summarize older messages; **kernel state preserved**; not a completion signal |

### 2.5 Skills (docs/skills.md)

- Locations: `~/.prime/agent/skills`, `~/.agents/skills`, project `.prime` / `.agents`, packages, CLI `--skill`, built-ins.
- Progressive disclosure per agentskills.io.
- Python-backed skills: `pyproject.toml` + importable package; optional CLI console script.
- Continual harness skill **descriptions** ≠ installed executable skills (`/refine` vs skill-creator).

### 2.6 What to absorb into nur (priority order)

**P0 - pattern fit, mostly in-process**

1. **Context-as-variable + compaction that does not destroy it** (RLM paper + Prime).
2. **Async subagent admission** (spawn returns handle; parent continues; `/swarm` already shows children - extend lifecycle).
3. **Goals** as durable session state (lighter than full daemon).
4. **Quality gates** for autonomous / continuous mode (run check command before Done).
5. **Refine-with-rollback** for supplemental prompts/memories only (never base system prompt) - maps to OptMem/PLUR + session artifacts.

**P1 - optional sidecars**

6. Persistent REPL tool (Python or starlark) behind explicit config; Windows-safe sandbox story required.
7. Daemon supervisor for detach/reattach (Unix-first, like fractal).
8. Heartbeats/schedules as first-class session re-entry (beyond `bg`).

**P2 - product surface**

9. Agent-to-agent messaging across sessions.
10. Python-backed skills (nur is Rust - prefer WASM/native skill scripts or keep markdown+scripts).

### 2.7 Explicit non-goals for nur

- Replacing the multi-provider Rust loop with Prime's TypeScript + IPython stack.
- Treating the REPL as a security boundary (Prime documents it is not).
- ARC-AGI marketing numbers without independent harness evals.

---

## 3. nickscamara thread → Firecrawl AnyDoc

**Sources:**

- X: https://x.com/nickscamara_/status/2084669934194266370  
  Title/body (from page meta): *introducing anydoc - now your agents get 100x faster local parsing for pdf, docx, pptx & 10 more formats - sub-5ms md conversion - 500 docx files in 1.7s - top quality across all 13 formats - rust-based - open source - already powering @firecrawl /parse*
- Repo: https://github.com/firecrawl/anydoc
- Blog: https://www.firecrawl.dev/blog/anydoc-and-pdf-inspector
- Hosted: https://www.firecrawl.dev/parse
- crates.io: `anydoc` · npm `@firecrawl/anydoc` · PyPI `firecrawl-anydoc`
- Agent skill: `npx skills add firecrawl/anydoc` → convert-documents-to-markdown

### 3.1 What it is

A **Rust library** (plus Node/Python/WASM bindings) that converts office documents (Word, PowerPoint, Excel, OpenDocument, RTF, EPUB, CSV, PDF, …) into **clean GitHub-Flavored Markdown** in single-digit milliseconds when text is extractable. Hosted Firecrawl Parse adds OCR for scanned pages anydoc cannot read alone.

### 3.2 Why it matters for Prime + nur

RLM and Prime both assume the model can **programmatically examine large inputs**. Agents currently choke on binary office formats. AnyDoc is the missing **ingestion edge**:

```
binary doc → anydoc → markdown variable in context_store → RLM slice/search → answer
```

### 3.3 Nur integration sketch

1. **`anydoc` tool** (or `read_file` enhancement): if path is `.pdf/.docx/.pptx/.xlsx/...`, convert to markdown (cap size, spill to tool-results like large fetches).
2. Prefer **crate dependency** or vendored CLI - nur is already Rust; avoid Node spawn when possible.
3. Optional skill pack wrapping CLI for ecosystem users.
4. Pair with RLM context_store so converted docs become named variables, not one-shot tool dumps that get compacted away.
5. For scanned PDFs: document fallback to Firecrawl hosted `/parse` when user has API key - never silent network without config.

### 3.4 Integration with "Prime Agent integration"

AnyDoc is not a harness. It is a **capability Prime-style agents need** under programmatic tool use. In a Prime-like REPL world it would be `import anydoc; md = anydoc.convert(path)`. In nur it is a first-class tool + optional skill.

---

## 4. Shepherd

**Sources:**

- Repo: https://github.com/shepherd-agents/shepherd (MIT, alpha)
- README
- Concepts: https://github.com/shepherd-agents/shepherd/blob/main/docs/shepherd/concepts/index.md (+ tasks, effects, runs, permissions, placements)
- Paper: arXiv:2605.10913 *Shepherd: Enabling Programmable Meta-Agents via Reversible Agentic Execution Traces*
- Site/docs: https://shepherd-agents.ai/ · https://docs.shepherd-agents.ai/
- Experiments companion: shepherd-agents/shepherd-experiments

### 4.1 Product definition (README)

Shepherd is a **runtime substrate** for agent work that needs **inspection, reversibility, and supervision**. It records runs as durable, inspectable **execution traces**, with **retained workspace outputs** that can be reviewed before select / apply / release / discard.

Platform constraints (README):

- Python 3.11+
- OS grant enforcement: **macOS Seatbelt**, **Linux Landlock** (privileged container)
- **Windows unsupported** for enforcement (advisory only) - use WSL

### 4.2 Five interlocking concepts (docs)

| Concept | One-liner |
|---------|-----------|
| **Tasks** | Typed function as contract; signature carries meaning **including permissions** |
| **Effects** | Everything that touches the world crosses one explicit, typed, recorded channel |
| **Runs** | Durable record of one execution: outcome, trace, artifacts, usage |
| **Permissions** | Per-repository grants on the signature (`May[GitRepo, ReadOnly|ReadWrite]`); signature **is** the permission surface |
| **Placements** | Where the body runs - decides OS-enforced vs advisory |

### 4.3 Runs (concepts/runs.md)

Every run ends in exactly one of: **finished**, **failed**, **exhausted** (budget), **stopped** (cancel). All four are inspectable values.

- Trace: ordered boundary crossings (effects, model I/O, nested tasks, artifacts)
- Artifacts: side-channel outputs distinct from return value
- Debugging = reading the record, not reproducing under a debugger
- Compare runs as data; replay is directionally planned, not full public API yet

### 4.4 Permissions model (README)

```python
@task
def apply_documented_fix(
    docs:    May[GitRepo, ReadOnly],
    backend: May[GitRepo, ReadWrite],
    issue:   str,
) -> None: ...
```

On a jailed device, grants compile to writable roots and are **enforced at syscall**. Writes to ReadOnly or ungranted paths are refused before merge gates. Settlement: `select` / `apply` (3-way merge when path-disjoint) / `release` / `discard`.

### 4.5 Contrast with Prime

| | Prime Agent | Shepherd |
|--|-------------|----------|
| Primary bet | Model programs its own context + self-improves harness | Runtime makes agent work **reviewable and reversible** |
| Default tool surface | IPython REPL | Task functions + effects channel |
| Subagents | `rlm(...)` children | Nested tasks / meta-agents (paper) |
| Safety | Explicitly **not** a sandbox | OS jail + retained outputs |
| Windows | Supported enough for coding agent | Enforcement unsupported |

They compose: Prime-style RLM agent **as the body of a Shepherd task**, with retained diffs until human select/apply.

### 4.6 What to absorb into nur

**P0 patterns (Windows-friendly)**

1. **Retained outputs / proposal mode**: destructive tools write to a shadow worktree or patch buffer; user `apply`/`discard` (extends existing undo + plan mode).
2. **Typed run records**: expand `receipt` into a full trace (tools, model rounds, artifacts, usage, terminal status enum: finished|failed|exhausted|stopped).
3. **Signature-as-permissions**: map task/tool profiles to permission profiles (explore = read-only roots; general = write cwd; never-implicit home/drive roots - nur already refuses FS root).

**P1 (Unix/WSL)**

4. Optional Landlock/Seatbelt placement for high-risk runs (fractal nodes already isolate worktrees).
5. Interop CLI: run a Shepherd task from nur when `shepherd` is installed (ecosystem pack), not a rewrite.

**Do not** block nur's default Windows UX on Shepherd jails.

---

## 5. OpenAI agent strategies (portable)

**Sources (first-party):**

- Agents guide: https://developers.openai.com/api/docs/guides/agents
- Agents SDK docs: https://openai.github.io/openai-agents-python/
- Agents: https://openai.github.io/openai-agents-python/agents/
- Handoffs: https://openai.github.io/openai-agents-python/handoffs/
- Guardrails: https://openai.github.io/openai-agents-python/guardrails/
- Related: Responses API (tool loop), multi-agent, sessions, tracing, sandbox agents

### 5.1 SDK primitives (portable names)

| Primitive | Meaning | Nur analogue | Portable lift |
|-----------|---------|--------------|---------------|
| **Agent** | Instructions + tools + model + handoffs + guardrails | `AgentRunner` + prompt + tools | Keep; avoid OpenAI-only types in core |
| **Runner** | Owns the tool loop until final output or pause | `run_turn` / `spawn_turn` | Already present |
| **Tools** | Function tools, hosted tools, agents-as-tools | `ToolHost` | Keep multi-provider schemas |
| **Handoffs** | Transfer control to another agent (specialized peer) | `agent` tool + provider override | Add explicit handoff metadata (reason, target role, input filter) |
| **Guardrails** | Input/output tripwires (block or warn) | permissions.toml + hooks + plan mode | Add structured input/output validators as hooks |
| **Sessions** | Conversation state across turns | `Session` JSON | Align export/trace format |
| **Tracing** | Spans for model/tool/handoff | receipts + usage.jsonl + swarm | Structured span export (OTLP-friendly) |
| **Approvals / human-in-the-loop** | Pause for approval | `ApprovalRequest` | Already strong |
| **Sandbox agents** | Isolated workspace + resumable session | fractal worktrees | Deepen, do not copy OpenAI sandbox API |

### 5.2 SDK vs raw Responses API (docs framing)

Use Agents SDK when you want built-in sessions, tracing, guardrails, handoffs, resumable approval flows. Use Responses API when you own the loop.

**Nur already owns the loop** (correct for multi-provider). Portable strategy: **steal the primitives**, not the Python SDK.

### 5.3 Patterns to implement for *all* providers

1. **Handoff packet**: `{target_role, target_provider?, reason, context_filter}` - not only free-text prompt.
2. **Input guardrails**: cheap checks before spend (secrets in prompt, path escapes, jailbreak patterns) - provider-agnostic.
3. **Output guardrails**: block shipping secrets / policy violations before TUI display or tool apply.
4. **Trace spans**: each model call, tool, subagent, handoff as a node; `/swarm` and receipts consume the same graph.
5. **Session resume** with explicit branch/fork (Prime has `/fork`/`/clone`; nur has sessions - unify UX).
6. **Agent-as-tool** vs **handoff**: document when to fan-out (parallel agent tool) vs transfer (single specialist continues).

### 5.4 What not to lock to OpenAI

- Hosted computer-use / Codex-only backends as the only sandbox.
- Responses-API-only tool shapes - nur must keep Chat Completions + Anthropic + Gemini paths.
- Assuming OpenAI tracing backend - export open formats.

---

## 6. Conflicts and tensions

| Tension | Resolution for nur |
|---------|-------------------|
| REPL-everything (Prime) vs fixed tools (nur multi-provider reliability) | Keep tools; add optional REPL + context_store tools. Fixed tools remain default. |
| Self-modifying harness (Prime refine) vs stability/security | Refine only supplemental layers; immutable base prompt; snapshots + user confirm for global skills. |
| OS jails (Shepherd) vs Windows-first users | Retained outputs + permissions on all platforms; real jails Unix/WSL optional. |
| RLM recursion depth vs cost/latency | Default depth 1 (current); opt-in higher for research mode with budgets. |
| Async admission handles vs simple report string | Keep sync report default; add async job id mode for long children. |
| AnyDoc local purity vs OCR quality | Local first; hosted OCR optional with explicit auth. |
| OpenAI handoffs vs multi-provider | Handoff is a nur concept; provider is a field, not the architecture. |

---

## 7. Nur integration map (modules)

| Pattern | Primary sources | Likely nur touchpoints |
|---------|-----------------|------------------------|
| Context store / prompt-as-variable | RLM paper, Prime RLM | new `src/agent/context_store.rs`; tools; headroom skip rules |
| RLM-style recursive spawn | RLM, Prime rlm.md | `subagent.rs`, `swarm.rs`, `loop.rs` agent tool |
| Continual harness refine | Prime blog/README | `optmem.rs`, `plur`, skills, session artifacts, new `/refine` |
| Goals + quality gates | Prime long-running | `continuous.rs`, config, TUI status |
| Daemon detach/attach | Prime architecture | new supervisor (Unix); `bg_jobs.rs` lessons |
| Retained outputs | Shepherd runs/permissions | `tools/undo.rs`, sandbox, plan mode, new apply gate |
| Run traces | Shepherd runs, OpenAI tracing | `receipt.rs`, usage, export |
| Signature permissions | Shepherd permissions | `permissions.rs`, tool capabilities |
| Document parse | anydoc | new tool or `read_file` branch; Cargo feature |
| Handoffs + guardrails | OpenAI Agents SDK | `hooks.rs`, agent tool schema, prompt |
| Skills progressive disclosure | Prime skills, agentskills.io | already strong (`skill_cache`, intents) - keep |
| Multi-provider client correctness | nur primary | `auth.rs`, `loop.rs` resolve_subagent_target, `api/client.rs` |

### 7.1 Suggested layering (mental model)

```
┌─────────────────────────────────────────────────────────┐
│ TUI / CLI / gateway (presentation)                      │
├─────────────────────────────────────────────────────────┤
│ Session policy: goals, gates, refine, permissions mode  │
├─────────────────────────────────────────────────────────┤
│ Agent loop (multi-provider) + handoffs + guardrails     │
├──────────────┬──────────────────────┬───────────────────┤
│ ToolHost     │ Context store (RLM)  │ Subagents/swarm   │
│ + anydoc     │ peek/slice/search    │ admission/async   │
├──────────────┴──────────────────────┴───────────────────┤
│ Trace/receipts + retained outputs + undo                │
├─────────────────────────────────────────────────────────┤
│ Optional: REPL sidecar │ Shepherd CLI │ fractal/daemon  │
└─────────────────────────────────────────────────────────┘
```

---

## 8. Concrete "best of" checklist (implementation backlog seed)

Use this as the bible checklist when implementing - not a schedule.

### A. RLM / Prime core

- [ ] Named context variables for large tool results and files
- [ ] Compaction never drops context variables
- [ ] Subagent admission handles + optional non-blocking children
- [ ] Goal object with complete/pause/budget
- [ ] Autonomous quality gate command before success
- [ ] `/refine` for supplemental memory/prompt with snapshot/rollback

### B. Shepherd core

- [ ] Proposal/retained-output mode for write tools
- [ ] Run status enum finished|failed|exhausted|stopped
- [ ] Full ordered trace export
- [ ] Permission profiles bound to tool bundles
- [ ] Optional Unix jail placement

### C. AnyDoc

- [ ] Local office→markdown conversion in tool path
- [ ] Spill large markdown to tool-results
- [ ] Optional Firecrawl Parse OCR fallback

### D. OpenAI-portable

- [ ] Structured handoff fields on `agent`
- [ ] Input/output guardrail hooks
- [ ] Unified span tracing (swarm + receipts)
- [ ] Document agent-as-tool vs handoff

### E. Reliability (already hit in this research session)

- [x] Cross-provider subagent NL routing must not treat topical "OpenAI agent strategies" as a route
- [x] Prefer OAuth sessions over stale `provider_keys` for non-active providers
- [x] Grok CLI-proxy fingerprint headers on JWT/cli-chat-proxy routes
- [x] Subagent failures report routed provider/model/base_url

---

## 9. Source index

| Source | URL | Role |
|--------|-----|------|
| RLM paper | https://arxiv.org/abs/2512.24601 | Defines RLM |
| RLM code | https://github.com/alexzhang13/rlm | Reference impl |
| Prime Agent repo | https://github.com/PrimeIntellect-ai/prime-agent | Harness source |
| Prime Agent blog | https://www.primeintellect.ai/blog/prime-agent | Product priorities |
| Prime RLM blog | https://www.primeintellect.ai/blog/rlm | Scaffolding philosophy |
| Prime RLM docs | packages/coding-agent/docs/rlm.md | Programming model |
| Prime architecture | packages/coding-agent/docs/architecture.md | Process model |
| Prime long-running | packages/coding-agent/docs/long-running-agents.md | Continuity features |
| Prime X launch | https://x.com/PrimeIntellect/status/2085086999267144083 | Launch thread (see blog for detail) |
| Shepherd repo | https://github.com/shepherd-agents/shepherd | Trace/permissions substrate |
| Shepherd paper | https://arxiv.org/abs/2605.10913 | Meta-agents + reversible traces |
| Shepherd concepts | docs/shepherd/concepts/* | Mental model |
| nickscamara anydoc | https://x.com/nickscamara_/status/2084669934194266370 | Doc parse announcement |
| anydoc repo | https://github.com/firecrawl/anydoc | Rust converter |
| Firecrawl parse | https://www.firecrawl.dev/parse | Hosted OCR path |
| OpenAI Agents SDK | https://openai.github.io/openai-agents-python/ | Handoffs/guardrails/tracing |
| OpenAI agents guide | https://developers.openai.com/api/docs/guides/agents | When to use SDK vs API |
| nur agent loop | `src/agent/loop.rs` | Current host |
| nur subagents | `src/agent/subagent.rs` | Child runs |
| nur receipts | `src/agent/receipt.rs` | Partial trace |

---

## 10. Session notes (research run)

- Research skill required primary sources + one markdown deliverable - this file.
- Cross-provider subagent spawn failed mid-research with OpenAI `sk-vup1x…` 401 because NL recovery treated **"OpenAI agent strategies"** in the research prompt as a provider route, then preferred a stale API key over OAuth. Fixed in-tree (see §8.E); requires rebuild/install to affect the running binary.
- Active auth at research time: xAI OAuth on `cli-chat-proxy.grok.com`; many other provider OAuth sessions expired - refresh on use for cross-provider fan-out.
- Browser snapshot of X threads was unreliable; launch priorities taken from Prime first-party blog/docs. AnyDoc content taken from X page title/meta + GitHub README.

---

*This bible is the standing reference for merging RLM, Prime, Shepherd, AnyDoc, and OpenAI-portable agent patterns into nur-cli. Prefer updating this file when sources move, rather than scattering notes.*

---

## 11. Agent-native memory (arXiv:2606.24775)

**Source:** Zhou et al. *Are We Ready For An Agent-Native Memory System?* arXiv:2606.24775v1 (23 Jun 2026). https://arxiv.org/abs/2606.24775 · code https://github.com/OpenDataBox/MemoryData

### 11.1 Core claim

Agent memory is a **data management system** (store, retrieve, update, consolidate, lifecycle) — not a black-box RAG add-on. End-to-end F1 alone is insufficient; evaluate module trade-offs and cost.

### 11.2 Four modules (paper framework)

| Module | Role | Nur implementation |
|--------|------|--------------------|
| **M1 Representation & storage** | How memories are structured and indexed | Hierarchical tiers `recent/l1/l2/l3` in `~/.nur/native-memory/<scope>/entries.jsonl` (`src/agent/native_memory.rs`) |
| **M2 Extraction** | How experience becomes memory | Heuristic extract + explicit `connectome remember` (first-person preferred) |
| **M3 Retrieval & routing** | What enters the live window | Keyword + recency + confidence + voice scoring → prompt inject |
| **M4 Maintenance** | Lifecycle under growth | **Localized** L1→L2 consolidation (paper: cheaper than global reorg) |

### 11.3 Paper findings used as design constraints

- **No single architecture dominates** — hybrid tiers + multiple stores (context_store + native_memory + memory.md + OptMem) by design.
- **Align structure to bottleneck** — coding agent bottleneck is long-horizon project identity + tool dumps → hierarchical diary + RLM variables.
- **Localized maintenance > global reorganization** — `consolidate_localized` only retires oldest L1 batch.

---

## 12. Anima Connectome (animalabs.ai/connectome)

**Sources:** https://animalabs.ai/connectome · https://github.com/anima-research/connectome-host · ecosystem-overview (Chronicle, context-manager, membrane)

### 12.1 Continuity thesis (product page)

- Context window is the **focus**, not the identity; history is the permanent record.
- Standard summarization **at the live edge** is the failure mode (perturbation compounds).
- **Agent writes its own memories** (first person, on-policy).
- **Live text is never rewritten** for compression behind the active edge.
- Lossy by design (L1→L3), with workspace files + full archive as escape hatches.

### 12.2 Nur mapping

| Connectome idea | Nur piece |
|-----------------|-----------|
| Hierarchical resolution curve | `Tier::{Recent,L1,L2,L3}` |
| On-policy self-authored memory | `Voice::FirstPerson` + `connectome remember` |
| Append-only life archive | `src/agent/chronicle.rs` |
| Checkpoints / time-travel lite | `connectome checkpoint|restore` (describe as-of; no rewrite) |
| Recipes | Future: JSON agent recipes (not yet); config.toml + skills cover part |
| KV-stable compaction | Compact chat edge only; inject memory inventory; do not rewrite native memory entries in place |
| Multi-participant membrane | Future (gateway/Discord); membrane not forked |

### 12.3 Tool surface

`connectome` actions: `remember|recall|list|consolidate|extract|chronicle|chronicle_tail|checkpoint|restore|status`

Config: `native_memory = true` (default).

---

## 13. Source index additions

| Source | URL |
|--------|-----|
| Agent-native memory paper | https://arxiv.org/abs/2606.24775 |
| MemoryData | https://github.com/OpenDataBox/MemoryData |
| Connectome | https://animalabs.ai/connectome |
| connectome-host | https://github.com/anima-research/connectome-host |
| ecosystem-overview | https://github.com/anima-research/ecosystem-overview |


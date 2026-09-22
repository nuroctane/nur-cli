---
title: TypeSafe · Jev
---

# TypeSafe (Jev) - typed judgments for the harness

nur routes every request through the model you chose. A large part of what an
agent loop does around that request, though, is not writing at all: which tool
next, is this chunk relevant, is this diff risky, did that call work, does this
need a human, does this still need to be in context. Those are if statements, and
paying a frontier model to answer them in prose is the most expensive way to get
a yes/no.

[TypeSafe](https://docs.typesafe.ai)'s **System One** models - flagship **Jev** -
answer exactly that shape: typed questions in, typed answers out, with
probabilities. nur treats it as a **boost layer** for whatever provider you are
already using. It is provider-agnostic by construction: the judgments happen in
the harness, not in the model, so a cheap model and an expensive one get the same
one.

```text
                 your request
                      │
        ┌─────────────▼──────────────┐
        │  harness decisions (Jev)   │  Choice · Noul · Score
        │  gate · judge · compact    │  one batched request per tool batch
        └─────────────┬──────────────┘
                      │ survivors kept verbatim
                      ▼
   any provider · any model (Meta, OpenAI, Anthropic, Groq, Ollama, …)
```

## The primitives

| Need | Primitive | Answer |
|------|-----------|--------|
| One of a defined set | `choice` | the picked option + full probability distribution + `confidence` |
| Whether a condition holds | `noul` | `p(yes)` in `0..=1` |
| Degree along a scale | `score` | probability-weighted position on 2-10 ordered levels + `confidence` |

`choice` takes the caller's option set (up to 255), `score` takes ordered levels
with concrete meanings. **Options always come from code** - the retriever, the
tool trace, the model registry - so a judgment can only *select* a value nur
already had. An answer that matches no candidate yields no judgment instead of an
invented one.

## Confidence decides, not the answer

One threshold policy gates all three primitives (`[typesafe] act_confidence`,
`escalate_confidence`):

| confidence | band | behavior |
|-----------|------|----------|
| `>= 0.85` | act | the harness may act on the answer |
| `0.50 - 0.85` | confirm | informs, never acts alone |
| `< 0.50` | escalate | route to a bigger model or a human |

`noul` answers carry no upstream confidence, so nur derives the equivalent
distance from the coin flip (`|p - 0.5| * 2`), which is why a 0.51 probability is
treated as *no evidence* rather than a slight preference. That measure lines up
with the compaction thresholds by construction: `p = 0.75` (or `0.25`) is exactly
confidence `0.5`, the escalation floor, so "act on this keep/drop answer" and
"do not act on this judgment" are the same boundary everywhere.

## Where it is wired in

| Layer | What Jev decides | Effect |
|-------|------------------|--------|
| **Tool gate** | does this call need a person, how risky is it, is it a redundant repeat, is it the same failure again | a confident duplicate of a call whose result is still in context is answered from that result instead of re-running the tool; risk and "needs you" judgments are surfaced, never enforced |
| **Tool results** | did that call accomplish what it was for | failures are flagged in the transcript; bodies stay byte-identical |
| **Compaction** | which tool calls and results are still worth their tokens | stale calls and results are dropped, survivors stay verbatim, and no summary is written - so the frontier summarization call is skipped entirely |
| **Skills** | which of a triggered skill's own rules this request needs, and whether the skill was actually carried out | the applicable rules are injected as a short checklist; on a confident shortfall the skill layer steers the turn with the specific rule that was skipped |
| **Routing** | the cheapest adequate model for a task | the `route` tool action returns a suggestion; automatic child routing is not connected to the execution loop |
| **Retrieval / ranking** | which candidates are actually relevant | the `rank` action scores every candidate in one request; the model calls it when it is juggling candidates, and context-store search reranks its hits through it |
| **Escalation** | nothing - this is local policy | below the floor the answer is reported and never obeyed; the caller falls back to its own default, and the transcript says so |

## Compaction

Live tool-result checks ask only whether execution succeeded. Retention questions
are deferred until compaction, where their answers can actually prune the current
transcript. This removes two of the former three questions per live result check
without changing its verdict. Split requests use bounded workers that immediately
take the next batch, so a slow request does not hold up a whole later group.
Result truncation also checks its estimated token saving before adding a note;
an expanded replacement is left untouched.

The highest-value wiring, and the one to start with. Instead of summarizing old
turns (lossy: a path, an exact error, or a constraint can vanish), Jev sees the
whole conversation with each tool result replaced by a short note
(`ok, 4213 chars (omitted)`) and answers two questions per non-pinned call:

- should the **call** stay, knowing it was made with these arguments?
- should the **result** stay verbatim, or can the tool just be re-run?

The two answers are compared against `keep_threshold` (default `0.5`, the
reference's), with no hidden floor above it:

| `p(keep_result)` | `p(keep_call)` | action |
|---|---|---|
| `>= keep_threshold` | any | keep both, byte for byte |
| below | `>= keep_threshold` | keep the call, truncate the result to a head + one-line note |
| below | below | drop call and result together |
| missing | any | **keep** - an unjudged call is never deleted |

`keep_threshold` is a *pruning* bar, not a policy band, and the two answer
different questions. `act_confidence` (0.85) decides whether an answer may change
what the agent **does**; this decides whether a tool result may leave the
context. The risk is bounded and recoverable - the tool can be re-run, user and
assistant text is never touched, and the pre-compaction transcript is written to
`.precompact.bak` - which is why the reference ships 0.5. Raise it to `0.925`
(the Act band) if you want pruning to demand the same confidence as acting; the
`prune` action and the automatic path read the same key, so they always agree.

Budgeting follows the reference too: the state is fitted to `max_state_tokens`
in stages (tool inputs 1000 → 200 → 60 characters, long texts abridged head+tail,
old texts collapsed, then dropped), and questions are split so that **state plus
one batch of questions** stays under `max_request_tokens` (30000, under System
One's ~32k request limit). The state is re-sent with every batch, so a large
state means smaller batches - or a fallback, never a request the endpoint would
reject. A call is only judged when its result is present and neither side of the
pair is pinned, and the result note says `error` or `ok`, because an error is the
result most worth keeping.

Text written by the user or the model is never touched, no result is ever left
without its call, and any failure (no key, transport error, unfittable state)
falls back to normal compaction instead of deleting anything.

If the reduction is at least `min_reduction` (0.25 default), the summarizing
model call is skipped: no frontier tokens, no lossy summary. Otherwise the pruned
items feed the normal summarizer, so it is cheaper either way. A result barely
longer than the head it would keep is left alone rather than churned, and the
report names what happened - calls kept, truncated, dropped and pinned, the
fitting stage, the state size, how many requests it took and how long in ms.

This is a port of [fast-jev-compaction](https://github.com/tamaratran/fast-jev-compaction);
the operation/target selection shape follows
[jev-ultrafast](https://github.com/browser-use/jev-ultrafast) (Jev picks from an
indexed, code-built candidate list; the model is only called when text must
actually be generated).

### Where else this can pay off (measured, not wired)

The survey that produced the seams above ranked what is still unwired by
(tokens or turns saved) per unit of risk:

1. **The tool-result spill** (`src/tools/spill.rs`, 12 000 chars, head-only
   preview). It clamps *every* tool body, and the head is the wrong window for
   stack traces, log tails and search results. A `score` over cheap code-built
   slices (head, tail, match windows) choosing the 12 000 that matter is the
   largest single lever; the full body is on disk and re-readable, so a wrong
   call costs one turn.
2. **The per-turn prompt blocks** (`src/agent/prompt.rs`): memory, PLUR, OptMem,
   project instructions. Judged in aggregate today only for skills.
3. **Compaction's own thinning** (`src/agent/loop.rs`): the summarizer path still
   head-thins old bodies to 800 chars, losing tails the way (1) does.
4. **Shell retention** (`src/tools/shell.rs`): 80k/40k heads that the spill then
   cuts to 12k anyway.
5. **"First N" caps** in `web_search` (8 results, unranked), `grep`, `glob`.
6. **Per-field verify on extraction** (`harness::verify_result` judges the whole
   output today): check each extracted field against its cited evidence and
   bounce only the unsupported fields, AgentRun-style, instead of one
   pass/fail over the result. (The same shape already ships for goals:
   `goal complete` / DONE claims are judged part by part and only a confident
   not-done reopens the goal.)
7. **Learning notes from traces** (shipped for goals): a verified `goal
   complete` files a code-built lesson draft (goal head, verified count,
   tools used, turns/tokens) into the session's refined notes via
   `/refine` machinery, unless already noted. Generalize to any verified
   outcome when the win proves out.
8. **Node-level replay evals** (shipped for judgments): `NUR_JEV_RECORD=<set>`
   records every batched ask to `~/.nur/jev/evals/<set>.jsonl`;
   `nur jev eval --set <dev> [--reserved <heldout>]` replays through the
   current layer and reports agreement, `expected`-label accuracy, and cost.
   Next: promote recorded traces into labeled workflow evals.

Each is a candidate, not a promise: they change what the model sees, so they
want the same treatment as compaction - a documented bar, a fallback when the
judgment is unavailable, and a test that pins the recovery path.

## Use it

### Local engines (no key)

The same typed contract is served by three open, local decision engines through
one bridge - openJev-verdict-2.0 (CPU), Bespoke-Nimble-9B (NVIDIA GPU / Apple
Silicon MLX), and Laya Core ML (Apple Silicon Neural Engine) - plus a `mock`
backend for tests. Loopback needs no credential, so the whole harness boost runs
offline:

```bash
nur jev status                  # what this machine can run, and why
nur jev start --backend verdict # or nimble | laya | mock
nur jev use --port 8788         # [typesafe] base_url = the local bridge
```

Details, per-device support and limits: [jev-local.md](./jev-local.md).

### Key

Any of these, in order of precedence:

```bash
export TYPESAFE_API_KEY=...        # or TYPESAFE_KEY / JEV_API_KEY
[typesafe] api_key = "..."         # config.toml wins over the environment
nur auth login --provider typesafe # or /auth → `TypeSafe · Jev` (pinned at the top)
# ~/.config/typesafe/key, ~/.nur/typesafe.key, ~/.nur/keys/typesafe.key, ~/.typesafe_key
```

`/auth` shows it **at the top of the provider list with its own borders**,
because it is not a chat model: it is a credential that upgrades every provider
you use. Picking it never changes your active provider - the same is true of
`nur auth login --provider typesafe --key …`, which stores the key scoped to
`typesafe`. Setting `provider = "typesafe"` in `config.toml` is rejected at
startup with an explanation, and `typesafe` is refused as a failover target and
as a subagent route.

### Tool

The tool is part of the full tool surface from the second model request onward,
and a **judgment-shaped task** (classify, rank, which model, needs a human,
risky, typesafe/jev) gets it on the first round too, so such a task never waits a
round for it. Subagents run without it - they already get every harness-level
judgment (gate, result judge, compaction, skill checks) and their schemas are
kept lean on purpose.

```
typesafe action=ask state="..." questions=[
  {"id":"kind","type":"choice","instructions":"What is this?","criteria":{"bug":null,"feature":"new capability"}},
  {"id":"urgent","type":"noul","instructions":"Is it time-sensitive?"},
  {"id":"risk","type":"score","instructions":"How risky is the fix?","criteria":["trivial","local","wide","destructive"]}
]
```

Independent questions in one `ask` share a state, run in parallel, and cost one
round trip. Also: `pick`, `rank`, `classify`, `risk`, `verify`, `spam`,
`needs_human`, `route`, `status`.

### Slash

```
/typesafe            # status + what the layer did this session
/typesafe on|off     # enable/disable for this machine
/typesafe ask <text> # one judgment by hand (/jev is an alias)
/typesafe-ai         # the *skill*: how to design and use judgments (not the layer)
```

### Config

```toml
[typesafe]
enabled = true
model = "jev-latest"                  # TypeSafe's flagship System One model
timeout_ms = 20000
max_questions_per_request = 24        # batched; split + parallel beyond this
max_parallel = 4
max_request_tokens = 30000            # one request: state + its questions
act_confidence = 0.85
escalate_confidence = 0.50

[typesafe.compaction]
enabled = true
replace_summary = true                # skip the summarizing call when a prune is enough
preserve_recent = 6                   # newest items never touched (the first always is)
keep_threshold = 0.5
truncate_head_chars = 300
max_state_tokens = 25000              # token ceiling for the state Jev sees
min_reduction = 0.25                  # below this, fall back to normal compaction
goal_prompts = 3                      # recent user turns shown as the ongoing goal

[typesafe.tool_gate]
enabled = true
judge_results = true
skip_redundant = true                 # safe: the earlier result is still in context
skip_repeated_failures = false        # surfaced, not enforced (a retry after a fix is often right)
flag_risky = true                     # show risk / needs-human notices (never blocks)
preview_chars = 1200                  # chars of args/result shown to Jev per call
trace_window = 16                     # recent calls kept in the gate's window

[typesafe.skills]
enabled = true
rerank = true                         # choose among code-scored skill candidates
narrow_requirements = true            # add selected-rule checklist alongside full skill
check_usage = true                    # steer when a triggered skill is not followed
max_requirements = 8                  # most rules injected from a narrowed skill

[typesafe.routing]
enabled = false                       # reserved; automatic child routing is not connected
suggest = true
```

### Capability class

Every `typesafe` action is **read-only** in capability terms: nothing mutates the
repository, so plan mode allows it and it needs no approval - the same class as
`web_fetch` / `web_search`, with one extra guard those do not have. State handed
to the tool is refused outright when it looks like a credential, and every
preview the harness itself sends (tool arguments, result bodies, skill evidence,
the goal line, retrieved text) is passed through a redactor that replaces
secret-shaped content with a marker instead of shipping it. Redaction keeps the
item's position, so index-keyed questions stay aligned and the rest is still
judged.

### Prompt cache

Pruning removes items from the middle of the transcript, which changes the
prefix a provider's prompt cache keys on. That only happens through the
compaction path, which by definition fires when the window is full - the same
moment every other strategy rewrites the prefix too.

### Without a key

Nothing is required and nothing changes. No key means no client, every policy
returns "no judgment", and each call site keeps its previous behavior. There is
no path that invents a judgment, and no path that blocks on a missing key.

## Verify with a real key

Six checks are kept in the tree but **ignored by default** - they cost real
requests, run a real child process, or spawn a local engine, so the normal suite
never touches the network:

```bash
export TYPESAFE_API_KEY=...                       # or /auth → `TypeSafe · Jev`
cargo test --bin nur typesafe_live -- --ignored --nocapture   # the four below
cargo test --bin nur optmem_nap_drain_against_real_memo -- --ignored --nocapture
cargo test --bin nur jev_local_bridge_answers_keyless -- --ignored --nocapture
# every opt-in check in the tree:
cargo test --bin nur -- --ignored --list
```

| Check | Proves |
|-------|--------|
| `typesafe_live_primitives` | Noul / Choice / Score in **one** request, usage reported, and the Choice resolving verbatim onto code-supplied options |
| `typesafe_live_compaction_keeps_survivors_verbatim` | the compaction path against the real endpoint: text untouched, survivors byte-identical, no orphaned call/result pair, and the error that *is* the task not lost |
| `typesafe_live_risk_band` | a force-push grades `high`+ rather than `safe`, and the band maps onto stakes |
| `typesafe_live_tool_batch_is_one_request` | a whole tool batch (risk + verification + keep questions for three calls) still costs one round trip |
| `optmem_nap_drain_against_real_memo` | the nap fix against the real memo binary, in a scratch `MEMORY_DIR` (the user's memory is never touched) |
| `jev_local_bridge_answers_keyless` | a real client round trip through a locally spawned bridge, with no key at all |

Then watch it happen in a real turn:

```bash
nur -v -y "summarise what src/tools/mod.rs does"
#  -v prints the `typesafe · …` status lines (gate skips, risk flags, verdicts)
#  - the session receipt gets one `AuxiliaryInference` entry per batch:
#      ~/.nur/receipts/<session>.jsonl
/typesafe                                        # counters: requests, tokens, pruned, escalations
```

## Accounting

`/typesafe` and the footer chip report what the layer did: requests, questions,
input/output tokens, decisions, escalations, gated calls, pruned calls/results,
tokens kept out of context, and frontier compaction calls avoided. Each batch is
also written to the session receipt as auxiliary inference, so the cost of a
judgment and the tokens it saved sit in the same ledger.

## Wired, and where the next wins are

Wired today (all in the loop, all provider-agnostic): tool gate, result judge,
Jev-scored compaction with no summary, skill requirement narrowing + usage check
+ candidate rerank, batched candidate ranking, inbound
peer-mail spam labeling, and session accounting.

Also wired, and worth knowing about:

- **Local engines** (openJev-verdict-2.0, Bespoke-Nimble-9B, Laya Core ML) behind
  one bridge, so every judgment above can run keyless and offline on supported
  devices: [jev-local.md](./jev-local.md).
- **Inbound peer-mail spam labeling** (one batched request, labels never drops).
- **On-demand pruning and ranking** through the `typesafe` tool (`prune`, `rank`).
- **Routing** via the `route` tool action. The cache-aware child routing helpers
  are currently separate from subagent execution; the routing flag alone does
  not switch a child or parent model.

Two more, on request and off by default where the trade-off is real:

- **Retrieval rerank** (`context_store` search): hit lines come back in *file*
  order, which for a long document is close to random with respect to the
  question. When the judgment layer is available and there are at least four
  hits, they are ranked by relevance (one batched request); only confident
  placements move, everything else keeps its line position, and the header says
  the order came from a judgment.
- **Per-turn tool-schema narrowing** (`[typesafe.tools] subset = true`, default
  **false**): on the *first* round of a turn, one Noul per specialist tool asks
  whether the task needs it, and only tools the answer confidently rules out are
  dropped. Rounds after the first keep the full surface, so definitions are only
  ever added within a turn and a tool-call replay stays valid. It is off by
  default because the tool schemas ride the provider's prompt cache - a live
  session showed 231k of 232k input tokens cached - so the blocks are nearly free
  per turn, while hiding a tool the model wanted costs a round trip.

Still open, with the reason measured rather than assumed:

| Idea | Why it is not done | Shape it would take |
|------|--------------------|---------------------|
| Browser element picking beyond `browser action=pick` | the picker is implemented and unit-tested (snapshot → indexed `@e` table → one request for operation + target), but the Chrome extension on this machine reports `extension_upgrade_required` (min 2.1), so it has not been exercised against a live page. It refuses to pick when fewer than two candidates parse | nothing further in the harness; `nur browser setup` upgrades the extension, then it works |
| Independent verification pass | nur must never end a turn except on a user budget, so it stays a nudge rather than a hard gate | `Choice` over {continue, verify, stop} with a high bar, feeding a steer - never a block |
| A local engine as the *chat* model | openJev/Nimble/Laya answer typed questions, they do not write. They boost any chat model rather than replacing one | not planned: that is the distinction the sidecar entry already encodes |

The rule those follow: a judgment may only *select* what code already produced,
and anything that changes the user's work needs near-certainty, not a coin flip.

## Limits

- TypeSafe is an external service: the state you send leaves the machine.
  Secret-shaped content is refused by the `typesafe` tool, and compaction sends
  tool *notes* rather than result bodies.
- Judgments are calibrated probabilities, not proofs. The defaults are
  conservative on purpose: ambiguous answers keep content, and nothing that
  touches the user's actual work is decided by a coin flip.
- Rate limits: `429` / `529` (and 5xx) are retried with exponential backoff up to
  `[typesafe] retries`. There is no published numeric limit, so nur does not
  invent one.

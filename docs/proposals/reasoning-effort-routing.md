# PR DRAFT — Route the single reasoning-effort setting to every provider's native control

> Status: draft for implementation by the user's agent. Not opened on GitHub.
> Repo: `nuroctane/nur-cli` (reviewed at main branch, Oct 2026).
> Question answered: can reasoning efforts for each provider be
> appropriately researched and routed? Yes — and ~80% of the machinery
> already exists. This PR is a wiring job plus rung-table updates, not
> a redesign.

## 1. Answer first

A single user-facing effort setting (`reasoning_effort`, default
`high`, `/effort` menu `low … max, ultracode`) CAN be routed to each
provider's native reasoning control, because the unified ladder, the
nearest-rung clamping, and per-route rung tables already exist in
`src/providers.rs`. What is missing is the last mile: the Chat
Completions request builder silently drops the setting on every route
except three special cases. Users on xAI key auth, Groq, Mistral,
Together, Fireworks, DeepSeek, OpenRouter, and generic OpenAI-compat
routes are setting an effort that never reaches the wire — and nothing
tells them.

## 2. Current state (verified in code)

### 2a. The setting and the ladder

- `reasoning_effort: String` on config, default `"high"`
  (`src/config.rs:10` `DEFAULT_REASONING`, `:101`, `:1106`).
- `/effort` TUI command, menu `low … max, ultracode`
  (`src/tui/app.rs:287`; `EFFORT_MENU` at `src/providers.rs:742`).
- Per-directory overrides via `[[path_models]] effort`
  (`src/config.rs:1867-1868`), canonicalized at `:1514` via
  `canonical_effort` (aliases: `med`→`medium`, `extra`/`x-high`→
  `xhigh`, `maximum`→`max`, `ultra`→`ultracode`).
- `EFFORT_LADDER` (`src/providers.rs:732`):
  `none, minimal, low, medium, high, xhigh, max, ultracode`.
  `ultracode` is nur's own top rung — strongest rung the route has,
  plus parallel-subagent orchestration in the prompt; it is NEVER sent
  as a wire value. Any routing design that sends `ultracode` to a
  vendor is wrong.

### 2b. The routing layer that already exists

- `effort_levels(provider_id, model)` (`src/providers.rs:786`):
  per-route accepted rungs. Notable current entries: `google |
  antigravity | google-oauth | minimax` → empty (no effort control);
  `anthropic` → model-dependent via `claude_effort_levels`
  (empty for Haiku / Sonnet ≤4.5 — sending one is a 400); `xai` →
  grok-3-mini gets only `low/high`, grok-4.5 gets low/medium/high,
  else low/medium/high/xhigh; `openai` gpt-5.6 → incl. `max`, gpt-6 →
  no `none`/`minimal`; `nous`/`paperinstruments` → `low/high/max`;
  `morph` DeepSeek snapshots → `low/high/xhigh/max` on top-level
  `reasoning_effort`, other Morph models → `low/medium/high` on
  `reasoning.effort`; `perplexity` → full six-rung ladder; default
  `_ => EFFORT_OPENAI` (`minimal/low/medium/high/xhigh`) for unknown
  routes.
- `nearest_effort(provider_id, model, want)`
  (`src/providers.rs:871`): `None` when the route has no control
  (caller must omit the field); exact rung when listed; **unknown
  names forwarded verbatim** (a rung the vendor added this morning
  must work this morning); otherwise nearest rung by ladder position,
  **ties round up** (medium on a low/high provider → high).
- Unrecognized levels are a warning, never a hard error
  (`src/providers.rs:718-727` header comment).
- Failover re-clamps per target route (`src/agent/loop.rs:683-688`).
  The effective rung must be recomputed for each failover target, not
  carried over — this already works and must not regress.
- Per-turn `ultracode`/`ultrathink` (`src/agent/loop.rs:906-918`):
  ultrathink → `"max"` for the turn.

### 2c. Where effort reaches the wire today — and where it does not

Reaches the wire:

- Responses path: `ResponseRequest.reasoning: ReasoningConfig
  { effort, summary }` serializes natively (`src/api/types.rs:20,36-41`).
- Anthropic Messages path: `output_config.effort`, clamped per model
  (`src/api/anthropic.rs:142-152`). Deliberately does NOT auto-attach
  `thinking`: adaptive thinking is default on current models, and
  forcing it 400s on Haiku/Sonnet tiers. That behavior is correct;
  do not "fix" it.
- Chat Completions path (`build_body_for_provider`,
  `src/api/chat.rs`): ONLY three special cases consume
  `req.reasoning.effort` — Kimi (`thinking: {type: enabled}`,
  `:277`), Morph (`reasoning_effort` top-level for DeepSeek
  snapshots, `reasoning.effort` otherwise, `:281-295`), Paper
  Instruments (top-level `reasoning_effort`, `:296-308`; omitted =
  API runs at `max`).

Does NOT reach the wire:

- Every other Chat Completions route: xAI key auth, Groq, Mistral,
  Together, Fireworks, DeepSeek, OpenRouter, generic OpenAI-compat.
- Gemini Cloud Code path (`src/api/gemini.rs`): no thinking field is
  sent at all.
- `sends_effort` (`src/api/client.rs:517-522`) pins this in code:
  Responses → true; first-party Anthropic Messages → true; Chat
  Completions and Gemini Cloud Code → false. Its test (`:4137-4199`)
  asserts OpenRouter and xAI-key Chat Completions do NOT send effort.
  That assertion becomes false by design under this PR and must be
  updated, not worked around.
- DeepSeek thinking-mode wire rules (`src/api/chat.rs:489-494, 657+`;
  tests `:1824-2092`): `reasoning_content` replay, `tool_choice`
  clamped to `auto`. Any DeepSeek effort work must preserve these.

## 3. Per-provider research (2026, web-verified; URLs in §9)

| Provider (nur id) | Native parameter | Accepted values | Constraints the router must respect |
|---|---|---|---|
| OpenAI (`openai`, Responses) | `reasoning.effort` | `none, minimal, low, medium, high, xhigh, max` (model-dependent) | GPT-6 Astra rejects `none`; GPT-6.1 Sol rejects `none`/`minimal`. Reasoning tokens bill as output tokens; leave room in `max_output_tokens`. |
| OpenAI (Chat Completions) | `reasoning_effort` (top-level) | Same set, model-dependent | `none` only on gpt-5.1+/5.6+/Luna-class models. |
| Anthropic (`anthropic`) | `output_config.effort` + `thinking: {type: "adaptive"}` | `low, medium, high, xhigh, max` (model-dependent) | Current models (Opus/Sonnet 4.6+, 5.x): adaptive thinking, `budget_tokens` deprecated there; `max` Opus-only in some generations; `xhigh` arrived Opus 4.7. Errors on Sonnet 4.5/Haiku 4.5. Older models: `thinking: {type:"enabled", budget_tokens ≥1024, < max_tokens}`. Default effort = `high` (omitting ≡ high). |
| Google Gemini (`google`, `antigravity`) | `thinkingConfig.thinkingLevel` (native) / `thinkingBudget` (legacy) | `MINIMAL, LOW, MEDIUM, HIGH` (model-dependent) | Gemini 2.5: integer `thinkingBudget` (default auto ≤8192). Gemini 3+: `thinkingLevel` replaces budget; 3.5/3.8 Flash accept `low/medium/high` (`MINIMAL` unsupported on some); Gemini 4: `thinkingBudget` is a 400, `MINIMAL` unsupported. Caveat: nur's `google` route uses the OpenAI-compat host and its Cloud Code route uses the native protocol — neither currently sends any thinking field (§8.3). |
| xAI (`xai`) | `reasoning_effort` (Chat) / `reasoning.effort` (Responses) | Grok 4.7/4.6: `low, medium, high, xhigh` (default `high`); 4.5: `low, medium, high`; 4.3: `none, low, medium, high`; 3-mini: `low, high` (+`none` on some) | Reasoning **cannot be disabled** on 4.5+ (no `none`); `thinking=False` maps to `low`. Sending `none` to a model that rejects it is a 400 — exactly what `nearest_effort` + per-model tables must prevent. |
| DeepSeek (`deepseek`) | `reasoning_effort` (Chat) / `reasoning.effort` (Anthropic-format) / `output_config.effort` (Responses-format) + `thinking: {type: enabled/disabled}` toggle | `low, high, max` (server maps: `minimal/low`→`low`, `medium/high/xhigh`→`high`, `max/ultra`→`max`) | Thinking on by default, default effort `high`. Legacy `deepseek-reasoner` (R1) always reasons and ignores effort. Preserve DeepSeek wire rules (§2c). |
| Groq (`groq`) | `reasoning_effort` (top-level, Chat) | Model-dependent: GPT-OSS `low/medium/high`; Qwen `none/default` (toggle, not a scale) | `reasoning_format` (`parsed/raw/hidden`) is separate — controls surfacing, not depth. Non-reasoning models reject the parameter. Qwen models are binary: map nur `none`→`none`, anything else→`default`. |
| Mistral (`mistral`) | `reasoning_effort` (top-level, Chat) | Current Small/Medium/Large: `none, high` (binary toggle); zai-glm-5-3 via Mistral: `low, high, max` | Thinking **off by default** on current models — sending nothing means no thinking. Non-reasoning models reject the parameter. Retired Magistral line thought unconditionally. |
| OpenRouter (`openrouter`) | `reasoning: {effort}` or `reasoning: {max_tokens}` (unified; mutually exclusive) | `effort`: `low/medium/high` (+`minimal`, `xhigh` per some docs) | OpenRouter translates effort↔budget per underlying model (effort→budget ≈ low 20%, medium 50%, high 80% of max_tokens; budget→effort by ratio). Anthropic-format requests normalize `output_config.effort`/`thinking` into the same unified field. `:thinking` model variants default to high effort. |
| Together (`together`) | `reasoning_effort` (GPT-OSS, DeepSeek V4 Pro) or `reasoning` toggle (hybrid models) | GPT-OSS: `low/medium/high`; DeepSeek V4 Pro: only `high/max` (others auto-mapped: low/medium→high, high/xhigh→max) | Most hosted reasoning models are hybrid with reasoning **on by default**; toggle off via `reasoning` param or `chat_template_kwargs`. Models without the param: steer via prompt only. |
| Fireworks (`fireworks`) | `reasoning_effort` (Chat) or Anthropic-compat `output_config.effort` / `thinking.budget_tokens` | `low, medium, high, max` (and `none` to disable); `xhigh`→`max` on the Anthropic-compat path | Budget→effort bands on Anthropic-compat path: ≥10000→`high`, 5000–9999→`medium`, 1024–4999→`low`. `output_config.effort` wins over `thinking` when both present; `thinking.type: disabled` → `none`. |
| Cohere (`cohere`) | OpenAI-compat: `reasoning_effort`; native Chat API: `thinking: {type, token_budget}` | Compat: **only `none` and `high`** (`low`/`medium` rejected); native: budget ~1K–31K tokens | Only `command-a-reasoning` supports thinking (enabled by default). Budget mapping used by others: minimal→512, low→2048, medium→8192, xhigh/max→31000. |
| Moonshot/Kimi (`kimi`) | `reasoning_effort` (K3/K3.1) / legacy `thinking` toggle (K2) | K3/K3.1: `low, high, max` (default `max`, no `medium`); K2: `thinking` on/off only | **Thinking cannot be disabled on K3** — `thinking=False` maps to `low`. Switching effort mid-session invalidates prefix cache. Temperature/top_p fixed on K3 — do not send them. (Nur currently sends `thinking: {type: enabled}` for all kimi — stale for K3, see §8.1.) |
| Zhipu/Z.AI (`zhipu`) | `reasoning_effort` (+ `enable_thinking` on some surfaces) | GLM-5.3/5.2: `low, high, max` (default `max`); GLM-5.2 also accepts the wider set, mapped to nearest | GLM-5.3 **always reasons**; sending disabled fails. Older GLM: effort collapses to enabled. (Nur's `zhipu` route is the Coding Plan endpoint — verify which params that surface accepts, see §8.2.) |
| Meta Model API (`meta`) | Unknown / unverified | — | No public doc found for a reasoning-effort control on Meta's API (Muse Spark line). Treat as no-control until verified. |
| Cerebras, Baseten, Nebius, etc. (OpenAI-compat) | Usually `reasoning_effort` passthrough where the hosted model supports it | Model-dependent | GPT-OSS on Cerebras: `low/medium/high`. No verified per-provider table; default OpenAI-shaped mapping + omit-on-400-retry (§5.3) is the safe route. |
| Local / self-hosted (`ollama`, `llamacpp`, `lmstudio`, `vllm`) | Engine-dependent: vLLM `thinking_token_budget` / `chat_template_kwargs: {enable_thinking}`; Ollama/SGLang: model template decides | — | No universal knob. Best effort: send `reasoning_effort` only if the engine is known to accept it (vLLM), else omit. Never error. |
| Always-reason models (R1-class, K3, GLM-5.3, Grok 4.5+, GPT-6 Astra) | Floor, not a switch | — | `none`/`minimal` must never be sent; clamp to the model's lowest rung (`low`). This is the single most important 400-avoidance rule in the design. |
| Never-reason models (GPT-4-class, Claude Haiku, non-reasoning Mistral, base Qwen) | None | — | Omit the field entirely. Sending it is a 400 on strict APIs (Mistral) or silently ignored on lenient ones. |

## 4. Design: one route effort spec, not a longer if-chain

Keep the existing ladder and `nearest_effort` semantics. Add one new
internal concept: a **route effort spec** — `(field, value_map, floor,
omit_when)` — resolved per (provider, model, wire style), replacing the
current per-provider `if` chain in `build_body_for_provider` with a
table:

| Route shape | Wire field | Value mapping from nur rung |
|---|---|---|
| Responses API (OpenAI, xAI OAuth, others) | `reasoning.effort` | `nearest_effort` on the route's rung table; `none` only if the route's table contains it |
| Anthropic Messages (first-party only) | `output_config.effort` | Same, per-model Claude table (already exists); do not auto-attach `thinking` (current behavior is correct for adaptive models); for pre-adaptive models, additionally send `thinking: {type:"enabled", budget_tokens}` from the token-budget table below |
| OpenAI-compat Chat Completions (the gap) | `reasoning_effort` (top-level) | `nearest_effort` on a new per-provider/model table (Groq, Mistral, Together, Fireworks, Cohere, Kimi, Zhipu, DeepSeek, xAI-key, generic OpenAI-compat). Binary-toggle models: `none`→`none`, anything else→their "on" value (`default` for Groq Qwen, `high` for Mistral) |
| OpenRouter Chat | `reasoning: {effort}` | `nearest_effort` restricted to OpenRouter's accepted set; `none` → omit the field (OpenRouter treats omitted as default, and `none` semantics vary by underlying model) |
| Gemini native (Cloud Code path) | `generationConfig.thinkingConfig.thinkingLevel` | `minimal→MINIMAL, low→LOW, medium→MEDIUM, high/xhigh/max→HIGH`; omit `MINIMAL` for Gemini 4; omit entirely for models without thinking support. Gemini 2.5-era: `thinkingBudget` from the token table |
| Token-budget providers (Anthropic pre-adaptive, Cohere native, OpenRouter `max_tokens` mode) | `thinking.budget_tokens` / `reasoning.max_tokens` | Fixed table: `minimal→1024 (floor), low→2048, medium→8192, high→16384, xhigh→32768, max→65536` (clamped to the model's max and to `< max_tokens`); never below 1024; never ≥ max_tokens |

## 5. Fallback behavior (matches existing repo philosophy)

1. **Unknown rung → forward verbatim** (existing `nearest_effort`
   rule; a vendor-added rung must work the same day).
2. **Route has no control → omit silently, no warning** (existing
   behavior for Gemini/MiniMax; warning on every turn would be noise
   in a coding agent).
3. **Model rejects the value (400) → do not hard-error the turn.**
   Retry once with the field omitted (provider default), and surface
   a one-line status note naming the rejected value and the route.
   This is the only safe behavior for aggregators (OpenRouter /
   LiteLLM-style routes) where the model behind the id is not known in
   advance. Never silently downgrade and claim the requested effort
   was applied.
4. **Floor rule:** if the model's table lacks `none`,
   `none`/`minimal` clamp to the model's lowest rung — never send a
   value the table doesn't contain, except rule 1 (unknown names).
5. **Always-reason models:** effort is a floor, not a switch; `none`
   → lowest rung, and the UI should show the effective rung
   (consider logging effective effort per turn for audit).
6. **Failover:** keep the existing per-target re-clamp
   (`loop.rs:683-688`) — the effective rung must be recomputed for
   each failover target, not carried over.

## 6. Code changes (file by file)

1. `src/providers.rs` — extend `effort_levels` with the new Chat
   Completions tables (Groq binary vs GPT-OSS, Mistral binary, Cohere
   none/high, Kimi low/high/max, Zhipu low/high/max, DeepSeek
   low/high/max, Together GPT-OSS/DeepSeek-Pro). Most are 3-line match
   arms. Do not change the ladder, `nearest_effort` semantics, or the
   `ultracode`-never-sent rule.
2. `src/api/chat.rs` — replace the `kimi` / `morph` /
   `paperinstruments` special cases in `build_body_for_provider` with
   a single `apply_chat_effort(body, provider_id, model, effort)`
   that consults the table and writes the right field (or nothing).
   Preserve the DeepSeek thinking-mode wire rules (§2c) — add
   regression tests if touching nearby code.
3. `src/api/client.rs` — update `sends_effort` (`:517`) to return
   true for Chat Completions routes that now carry effort, and extend
   its test (`:4137-4199`). The current assertions that OpenRouter /
   xAI-key Chat Completions do NOT send effort become false by design.
4. `src/api/gemini.rs` — thread `req.reasoning.effort` into
   `thinkingConfig.thinkingLevel` on the Cloud Code native path (new
   work; currently dropped). Do NOT add thinking controls to the
   OpenAI-compat Gemini host until §8.3 is verified.
5. `src/api/chat.rs:277` (Kimi) — replace the blanket `thinking:
   {type: enabled}` with `reasoning_effort` for K3-family models;
   keep the toggle only for K2-era ids (see §8.1).
6. No config change. No new user-facing knob. `reasoning_effort` +
   `/effort` already are the single setting; this PR routes it.
   Default stays `high`.

## 7. Tests / acceptance criteria

Unit tests (extend existing suites, do not fork them):

1. For each new table row: `nearest_effort` returns the documented
   rung for every ladder input, including the floor rule
   (`none`→lowest rung on always-reason models) and binary toggles
   (Groq Qwen: `none`→`none`, `medium`→`default`; Mistral:
   `medium`→`high`).
2. `build_body_for_provider` per route: field present with the right
   name/value for Groq GPT-OSS, Mistral, Together, Fireworks, Cohere,
   Kimi K3, DeepSeek, xAI-key, OpenRouter (`reasoning.effort`);
   field absent for never-reason models, no-control routes (Meta,
   local engines without support), and `none` on OpenRouter.
3. `sends_effort` updated test: Chat Completions routes that carry
   effort now return true; Gemini Cloud Code returns true once §6.4
   lands; Responses/Anthropic behavior unchanged.
4. DeepSeek thinking-mode tests (`chat.rs:1824-2092`) still pass
   untouched, plus one new test: effort + thinking mode together
   keeps `reasoning_content` replay and `tool_choice: auto`.
5. Failover test: a turn failing over from a `low/high/max` route to
   a `low/high` route re-clamps (does not carry `max` across).
6. 400-retry test: a rejected effort value retries once with the
   field omitted and surfaces the status note; it never hard-errors
   the turn and never claims the requested effort was applied.
7. `ultracode` never appears in any request body on any route
   (existing invariant — add an assertion per new route if cheap).

Manual acceptance (agent must demo, not just assert):

- `/effort high` on Groq GPT-OSS, Mistral, and OpenRouter routes shows
  the effort field in the outgoing body (via existing request logging
  or a test harness dump), and the model response is unaffected
  (no 400).
- `/effort none` on Grok 4.5+ and Kimi K3 does NOT 400 (floor rule).
- `/effort none` on a never-reason model sends no effort field.

## 8. Verify before merging (open questions, not blockers for drafting)

1. **Kimi in nur is stale for K3:** `chat.rs:277` sends `thinking:
   {type: enabled}` for all kimi requests, but K3 removed the
   `thinking` parameter in favor of `reasoning_effort` (and K2 is
   discontinued). Verify whether the Kimi Code backend nur speaks
   still accepts the toggle, or whether this is already silently
   ignored/broken.
2. **Zhipu Coding Plan surface:** nur's `zhipu` route is
   `api.z.ai/api/coding/paas/v4` (Coding Plan), not the public
   `api.z.ai` Chat API. The public GLM `reasoning_effort` values may
   not apply to the Coding Plan surface — verify before adding a
   Zhipu table.
3. **Gemini via OpenAI-compat host:** unverified whether
   `generativelanguage.googleapis.com/v1beta/openai` accepts any
   thinking control (native `thinkingConfig` is a generateContent
   concept; the compat host may ignore or reject it). The Cloud Code
   native path is the safer place to implement Gemini effort.
4. **OpenRouter `reasoning.effort` accepted set:** sources disagree
   on whether `minimal`/`xhigh`/`max` are accepted vs. only
   `low/medium/high`. OpenRouter translates effort→budget internally,
   so sending the unified ladder and letting it map is defensible,
   but the exact accepted enum needs a live doc check.
5. **DeepSeek v4-pro `low`→`high` server-side behavior:** reported by
   a third party (Inspect docs), not DeepSeek's own docs. Treat as
   unverified; do not encode a special case for it.
6. **Perplexity:** nur already grants it the full ladder, but its
   current `reasoning_effort` values were not verified against live
   docs — its Sonar reasoning models have changed repeatedly. Verify
   before touching that arm.
7. **Meta Model API:** no reasoning-effort doc found. Leave as
   no-control (omit) until Meta documents one.
8. **Model-id drift:** every table above is model-generation-dependent
   (Claude, GPT, Grok, Gemini, Mistral all gate rungs by model family).
   The tables need the same "unknown model → forward verbatim /
   default OpenAI-shaped" escape hatch `nearest_effort` already has,
   plus a maintenance note: rung tables are data, and vendors add
   rungs quarterly.
9. **Cost interaction:** effort is not free — reasoning tokens bill
   as output tokens (OpenAI, DeepSeek, Kimi, Together all confirm). A
   `max`/`ultracode` default on expensive routes can surprise; the
   existing default `high` is reasonable, and this PR keeps it. Also:
   raising effort without raising `max_output_tokens` truncates
   answers (Anthropic counts thinking against `max_tokens`; OpenAI
   reasoning consumes the output budget).

## 9. Doc URLs used (research)

- OpenAI: https://platform.openai.com/docs/guides/reasoning
- Anthropic: https://docs.anthropic.com/en/docs/build-with-claude/extended-thinking
- Google Gemini: https://docs.cloud.google.com/gemini-enterprise-agent-platform/reference/models/inference
- xAI: https://docs.x.ai/ (per-model values via promptfoo/pydantic provider docs)
- DeepSeek: https://api-docs.deepseek.com/guides/thinking_mode/
- Groq: https://console.groq.com/docs (model pages) and https://www.promptfoo.dev/docs/providers/groq/
- Mistral: https://docs.mistral.ai/studio/conversations/reasoning
- OpenRouter: https://openrouter.ai/docs/guides/best-practices/reasoning-tokens
- Together: https://docs.together.ai/docs/inference/chat/reasoning
- Fireworks: https://docs.fireworks.ai/guides/reasoning and https://docs.fireworks.ai/tools-sdks/anthropic-compatibility
- Cohere: https://docs.cohere.com/docs/compatibility-api
- Kimi/Moonshot: https://reapi.ai/models/kimi-k3 and Kimi K2/K3 provider docs
- Zhipu/Z.AI: https://pydantic.dev/docs/ai/models/zai/
- Cross-provider sanity checks: https://inspect.aisi.org.uk/reasoning.html, https://gptme.org/docs/providers.html

## 10. Implementation order for the agent

1. §6.1 rung tables + §7 tests 1–2 (data only, highest value, lowest
   risk). Verify §8.1/§8.2/§8.6 before adding Kimi/Zhipu/Perplexity
   rows; leave those rows out rather than guessing.
2. §6.2 `apply_chat_effort` + §7 tests 2–4. Keep the three existing
   special cases working until the table covers them, then delete
   them in the same PR (no dead branches left behind).
3. §6.3 `sends_effort` + test updates.
4. §6.4 Gemini native path (§8.3 decides whether the compat host is
   in or out; default out).
5. §6.5 Kimi toggle replacement (gated on §8.1 verification).
6. Docs: one paragraph in the provider/effort docs stating the
   routing table, the floor rule, and the 400-retry behavior, so the
   next vendor rung addition is a table row, not a redesign.

## 11. Why this is the right shape (one paragraph for the PR body)

Nur already decided the hard parts: one user-facing scale, nearest
rung wins, unknown rungs pass through, unrecognized levels warn
instead of erroring, and failover re-clamps per target. What it never
did was finish the wiring: the Chat Completions builder only knows
three providers by name, so every other route silently drops the
setting users think they set. This PR completes the routing the
architecture already promised — a per-route table consulted by one
function, a floor rule that makes 400s structurally unlikely, and a
single retry-with-omission for the aggregator cases no table can
predict. After it, `/effort` means the same thing on every provider,
including the ones where today it means nothing.

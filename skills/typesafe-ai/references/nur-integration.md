# nur integration notes (TypeSafe / Jev)

Where each piece lives, so a future session does not have to rediscover it.

| Piece | File |
| --- | --- |
| Typed primitives (Choice 255 max, Score 2..=10, Noul) | `src/typesafe/questions.rs` |
| HTTP client, batching, threads, retries, key resolution | `src/typesafe/client.rs` |
| Confidence bands, `Judgment`, escalation | `src/typesafe/policy.rs` |
| Harness policies (gate, verify, rank, route, skill checks) | `src/typesafe/harness.rs` |
| Jev-scored compaction (port of fast-jev-compaction) | `src/typesafe/compact.rs` |
| Accounting + TUI chip | `src/typesafe/telemetry.rs` |
| Config section | `src/config.rs` (`TypesafeConfig`) |
| In-loop tool | `src/tools/typesafe_tool.rs` |
| Loop wiring | `src/agent/loop.rs` (`typesafe_pre_gate`, `typesafe_judge_results`, `typesafe_skill_nudge`, `typesafe_narrow_tools`, `compact_session`) |
| Retrieval rerank | `src/agent/context_store.rs` (`rank_hits_by_relevance` → `reorder_by_scores`) |
| Local engines (no key) | `src/jev_local.rs` + `scripts/jev_local_bridge.py` (`nur jev start\|use\|status\|selftest`) |
| Browser picker (operation + target) | `src/tools/browser.rs` (`pick`, `parse_snapshot_elements`) |
| Skill layer | `src/agent/skills.rs` (`extract_requirements`, `jev_pick_skill`, activation checklist) |
| Pinned provider entry | `src/providers.rs` (`TYPESAFE_PROVIDER`, `is_sidecar_provider`) |

## Key resolution order

1. `[typesafe] api_key` (discouraged - plain text on disk)
2. `TYPESAFE_API_KEY`, then `TYPESAFE_KEY`, then `JEV_API_KEY`
3. nur credential store: `nur auth login --provider typesafe` (the `/auth` picker)
4. `~/.nur/typesafe.key`, `~/.nur/keys/typesafe.key`, `~/.typesafe_key`,
   `~/.config/typesafe/key`

## Behavior without a key

`client::client()` returns `Availability::Unavailable`, every policy returns
`Judgment::unavailable`, and each call site keeps its pre-Jev behavior. There is no
fallback path that invents a judgment, and nothing blocks on a missing key.

## Compaction

`compact_session` runs Jev first. Jev sees the whole conversation with tool results
replaced by `ok, N chars (omitted)` notes and answers two Noul questions per
non-pinned call: keep the call, keep the result verbatim. Decisions:

| `p(keep_result)` | `p(keep_call)` | action |
| --- | --- | --- |
| `>= 0.75` | any | keep both, byte for byte |
| `<= 0.25` | `>= 0.75` | keep call, truncate result to a head + note |
| `<= 0.25` | `<= 0.25` | drop call and result together |
| in between | any | **keep** - never delete on a coin flip |

If the reduction is at least `[typesafe.compaction] min_reduction` (0.25), the
summarizing model call is skipped entirely. Otherwise the pruned items feed the
normal summarizer, so it is cheaper either way.

## Local engines

A loopback endpoint needs no key, so the whole layer can run offline:

```bash
nur jev status                          # what this device can run, and why
nur jev start --backend verdict         # openJev | nimble | laya | mock
nur jev use --port 8788                 # [typesafe] base_url = the bridge
nur jev selftest                        # 53 mapping checks incl. a served request
```

The bridge translates nur's Choice/Noul/Score questions into each engine's own
schema and never invents an option; unanswerable questions come back as refusals,
which nur reads as "no judgment". See `docs/jev-local.md`.

## Useful commands

```bash
nur doctor                      # includes the typesafe block
/typesafe                       # status + session accounting
/typesafe ask <state>           # one typed judgment by hand
/typesafe off                   # disable the layer for this machine
cargo test --bin nur typesafe:: # the layer's own tests (fake transport, no network)
```

## Tests

Every policy is exercised against an injected transport (`Transport::Fake`), so the
suite needs no key and no network:

```bash
cargo test --bin nur typesafe::
```

What the tests pin down:

- one request for a whole batch of calls (`judge_calls`, 6 calls → 1 request)
- the call table the questions refer to is part of the state, not implied
- pre-scope asks about duplicates/failures *only* when code found them
- a failed judgment never skips a call and never prunes content
- Choice answers map back onto the caller's strings; an invented option yields no
  judgment; `none_of_these` is not a pick
- a `confirm`-band answer informs but may not act
- compaction's decision table, including "never delete on a coin flip"
- skill checks answer with one of the skill's own rules, and a skill with no
  stated rule is never judged (no request is sent)

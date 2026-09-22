# Jev performance evaluation - 2026-09-22

Scope: hosted Jev transport, live tool judgments, compaction, and the shared
Verdict/Nimble/Laya bridge. These changes preserve the acting thresholds and
the configured primary provider/model.

## Findings and changes

| Path | Finding | Change and evidence |
| --- | --- | --- |
| Live tool results | Three questions were asked per result; only verification was consumed | Verification-only scope asks one question. Regression checks that retention probabilities remain absent. This is 67% fewer questions for this pass, not a 67% reduction in primary-provider tokens. |
| Split hosted/local requests | A slow request held up all later groups even when other workers were idle | Bounded workers take the next chunk immediately. A synchronization test requires chunk three to run while chunk one waits; partial failures still preserve other answers. |
| Local engines | Identical state/question decisions repeated inference | Instance-local, 256-entry, 60-second cache with exact hashed keys and inference under the existing lock. Duplicate questions are also evaluated once per request. Tests cover changed state/schema, eviction, expiry, disabled caching, malformed answers and transient failures. |
| Local capacity checks | Python counted all whitespace and aggregated letters, unlike Rust | Match Rust's run-based estimator. Backend exact capacity limits remain authoritative. No state is truncated by this change. |
| Local Noul answers | Missing/non-finite probabilities could become a fabricated answer | Refuse these distributions; never cache them. Explicit 0.5 abstentions remain supported. |
| Compaction | The truncation note could cost more tokens than the result it replaced | Require a positive estimated token saving before rewriting a result. Whitespace-heavy regression keeps the original. |
| Routing | Cost-aware child helpers exist, but are not connected to execution | Corrected the tool response and documentation; no automatic routing savings are claimed. |

## What currently saves primary-provider tokens

The existing compaction path can remove stale tool bodies and avoid a frontier
summarization request when the configured reduction threshold is met. Optional
tool-schema narrowing can reduce schemas sent to the primary provider. These
changes reduce judgment overhead and avoid compaction growth; they do not
establish a measured end-to-end provider-token saving percentage.

Skill requirement selection currently adds a checklist alongside the full skill
body. It improves emphasis but should not be counted as skill-body compression.
Replacing complete skill instructions requires a separate quality evaluation.

The 96-token Laya ANE bundle and 512-token Verdict budget cannot accommodate
many full harness states. The corrected estimate avoids some false refusals,
but does not make those engines interchangeable with hosted Jev on long traces.
The Nimble reference helper processes fields individually; combining schema
fields alone is not evidence of parallel inference acceleration.

## Validation and limits

The default-feature Rust binary suite passed 1,026 tests, with 13 explicitly
ignored integration/preview tests. The bridge adapter suite and model-free HTTP
selftest passed. Tests use injected judgments and backend doubles to verify
contracts, request scheduling and conservative fallback behavior.

The active Python environment has no Verdict package or Nimble model directory.
Laya requires Apple Silicon macOS and cannot run on this Windows host. Therefore
real-model accuracy, calibration, GPU latency and primary-provider billing gains
were not measured. No model weights were downloaded and no credentials or
provider configuration were changed.

For real-model evaluation, replay representative recorded sets with
`nur jev eval --set <name> --reserved <held-out-name>`. Start the bridge with
`--cache-size 0` when measuring inference latency or repeatability; use the cache
enabled for a separate workload measurement of reuse. Compare judged coverage,
expected-label accuracy, latency, and the session's actual primary-provider usage.

Upstream contracts consulted:
[Nimble model card](https://huggingface.co/bespokelabs/Bespoke-Nimble-9B) and
[Laya Core ML](https://github.com/mizorewww/laya-coreml).

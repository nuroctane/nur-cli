---
title: Local engines (Jev contract, no key)
---

# Local typed decisions

nur's TypeSafe layer speaks one contract: a `state` plus typed **Choice / Noul /
Score** questions in, probabilities and `confidence` out. Three open, local
decision engines implement exactly that shape, so the whole harness boost (tool
gate, result judge, compaction, skill checks, routing) can run **on this machine,
with no API key and nothing leaving the box**.

`scripts/jev_local_bridge.py` translates between the two schema families and
serves the System One HTTP shape on loopback. It is embedded in the binary and
materialized under `~/.nur/jev/` at start time, so an installed nur does not need
this repository next to it.

| Backend | Engine | What it is | Device |
|---------|--------|------------|--------|
| `verdict` | [openJev-verdict-2.0](https://github.com/Heman10x-NGU/openJev-verdict-2.0) | ModernBERT-base + GLiClass head, ~150M, non-autoregressive, ~20-25 ms per decision, calibrated confidence | CPU (any), GPU optional |
| `nimble` | [Bespoke-Nimble-9B](https://huggingface.co/bespokelabs/Bespoke-Nimble-9B) | Qwen3.5-9B LoRA that scores the allowed answer tokens directly (no reasoning, no free-form output) | NVIDIA GPU with BF16; upstream also ships an MLX path for Apple Silicon |
| `laya` | [Laya Core ML](https://github.com/mizorewww/laya-coreml) | Core ML + Neural Engine, ~5 ms per short decision, 96-token cap on the ANE bundle | Apple Silicon, macOS 15+ |
| `mock` | built in | Deterministic keyword scorer: exercises the entire path with no model, no downloads | anywhere |

`nur jev status` reports which of these this machine can actually run, with the
install step that is missing for each. That is the honest answer to "for
supported devices": `laya` says Apple Silicon is required on a Windows box
instead of failing later, and `nimble` says what to download.

## Quick start

```bash
nur jev status                       # endpoint, credential, bridge, per-device support
nur jev probe                        # the same, as JSON (scripts and CI)
nur jev selftest                     # mapping checks incl. a real served request - no model needed
nur jev start --backend mock         # or verdict | nimble | laya
# engine options are forwarded to the bridge verbatim:
#   --verdict-model <id|path>   --nimble-dir <dir>   --laya-model <bundle>   --device cpu|cuda|mps
nur jev use --port 8788              # point [typesafe] at it (loopback needs no key)
nur jev stop                         # stop it
nur jev use --hosted                 # go back to api.typesafe.ai
```

`nur jev use` writes `[typesafe] base_url`, which survives restarts. The
environment alternative touches no config:

```bash
export NUR_JEV_LOCAL_URL=http://127.0.0.1:8788/v1/systemone
```

A loopback endpoint needs no credential, and `/typesafe` says so
(`typesafe: ready (local engine), no key needed`). A hosted endpoint still
requires a real key - the loopback check runs first, so a local placeholder can
never be sent to `api.typesafe.ai`.

## Per-backend setup

`verdict` (CPU, what most machines can run). This is the only backend that runs
without a GPU or Apple Silicon:

```bash
pip install "git+https://github.com/Heman10x-NGU/openJev-verdict-2.0"
nur jev start --backend verdict
# optional: --verdict-model knowledgator/gliclass-modern-base-v2.0  (or a local checkout)
#           --device cuda | mps   (cpu is the default; a GPU needs the matching torch build)
```

The engine is constructed once and reused; weights come from Hugging Face on
first use, then stay cached. Its own context budget (512 tokens) is enforced by
the bridge as a *refusal* per question, so an oversized request yields "no
judgment" rather than a truncated one.

`nimble` (NVIDIA GPU):

```bash
hf download bespokelabs/Bespoke-Nimble-9B --local-dir nimble-model
pip install -r nimble-model/requirements.txt
nur jev start --backend nimble          # add --nimble-dir if not ./nimble-model
```

Upstream limits, enforced by the bridge: at most 26 choices per question and no
truncation past 2048 tokens.

`laya` (Apple Silicon / macOS 15+):

```bash
pip install laya-coreml
nur jev start --backend laya
# optional: --laya-model aac6fef/laya-multilingual-coreml  (the 1024-token model)
```

The ANE bundle allows 96 tokens including question, options and state; the bridge
refuses anything longer instead of letting the engine raise a capacity error.

## What the bridge guarantees

- **Answers are the caller's own options.** A `choice` answer is keyed by the
  option labels nur supplied, never by a model-authored string; an unknown string
  cannot appear because the option set is the scoring set.
- **Probabilities are honest.** Non-finite values are dropped, negatives clamp to
  zero, distributions are renormalised, and `confidence` is derived from the shape
  of the distribution (a coin flip reads low, a concentrated answer reads high) -
  so nur's `act` / `confirm` / `escalate` bands mean something.
- **Refusals, not guesses.** An unsupported question type, an over-cap request, or
  a missing option set is reported per question. nur treats a missing answer as
  "no judgment" and keeps its own behavior for that question.
- **Partial batches.** One bad question does not sink the others.

## Accounting

Judgments served locally are recorded in the session receipt like any other:

```text
~/.nur/receipts/<session>.jsonl
  auxiliary_inference · purpose "typesafe system one judgments"
  route http://127.0.0.1:8788/v1/systemone · model jev-local:mock
```

The route and model are what actually served the request, so a local engine and
the hosted API are never confused in the audit trail.

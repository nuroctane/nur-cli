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
| `verdict` | [openJev-verdict-2.0 repository](https://github.com/Heman10x-NGU/openJev-verdict-2.0) | The repository's exposed `core.DecisionEngine` path: ModernBERT + GLiClass (not a separately published Verdict 2.0 architecture); model selected by `--verdict-model` | CPU; optional verified torch device |
| `nimble` | [Bespoke-Nimble-9B](https://huggingface.co/bespokelabs/Bespoke-Nimble-9B) | Qwen3.5-9B LoRA that scores the allowed answer tokens directly (no reasoning, no free-form output) | NVIDIA GPU with BF16; upstream also ships an MLX path for Apple Silicon |
| `laya` | [Laya Core ML](https://github.com/mizorewww/laya-coreml) | Core ML + Neural Engine, ~5 ms per short decision, 96-token cap on the ANE bundle | Apple Silicon, macOS 15+ |
| `mock` | built in | Deterministic keyword scorer: exercises the entire path with no model, no downloads | anywhere |

`nur jev status` reports import/device prerequisites, not a successful model
load or inference check. No weights are loaded by the probe. `laya` checks Apple
Silicon, macOS 15+ and Python 3.11-3.13; `nimble` checks for its helper and a
CUDA device with native BF16 support. Model files, dependencies, memory capacity
and inference still need verification on the target machine.

## Quick start

```bash
nur jev status                       # endpoint, credential, bridge, per-device support
nur jev probe                        # the same, as JSON (scripts and CI)
nur jev selftest                     # mapping checks incl. a real served request - no model needed
nur jev start --backend mock         # or verdict | nimble | laya
# engine options are forwarded to the bridge verbatim:
#   --verdict-model <id|path>   --nimble-dir <dir>   --laya-model <bundle>   --laya-max-tokens <n>   --device cpu|cuda|mps
nur jev use --port 8788              # point [typesafe] at it (loopback needs no key)
nur jev stop                         # stop it
nur jev use --hosted                 # go back to api.typesafe.ai
nur jev eval --set <name>            # replay a recorded judgment set, report agreement
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
first use, then stay cached. This adapter calls the repository's `core.DecisionEngine`
(GLiClass Verdict), not the separate `verdict2` architecture or confidence head.
The default is the upstream generic `knowledgator/gliclass-modern-base-v2.0`;
select `--verdict-model heman10x/rlcd-modernbert-151m` for its published Verdict
checkpoint. Published Verdict 2.0 accuracy/calibration numbers do not describe
this bridge's default model. The bridge uses a rough character-based 512-token
budget check, not the model tokenizer; it is not proof against upstream truncation.

`nimble` (NVIDIA GPU):

```bash
hf download bespokelabs/Bespoke-Nimble-9B --local-dir nimble-model
pip install -r nimble-model/requirements.txt
nur jev start --backend nimble          # add --nimble-dir if not ./nimble-model
```

Upstream limits: at most 26 choices per question and no truncation past 2048
tokens (the reference helper performs the exact token check). The bridge uses
that model-card CUDA helper only; the separate MLX implementation in the Nimble
GitHub repository is **not wired into this bridge**. The ~165 MiB download is a
LoRA adapter, not a complete model: first load also requires the Qwen3.5-9B base
checkpoint and enough GPU memory for the reference BF16 model.

`laya` (Apple Silicon / macOS 15+):

```bash
pip install laya-coreml
nur jev start --backend laya
# optional: --laya-model aac6fef/laya-multilingual-coreml  (the 1024-token model)
#           --laya-max-tokens 1024                        (capacity of that bundle)
```

The default ANE bundle allows 96 tokens including question, options and state.
The bridge applies an approximate preflight; the ANE runtime performs the exact
capacity check and rejects oversized prompts. General-purpose bundles can
truncate state upstream, so the bridge's character estimate is not a guarantee
of lossless input. Serving a bigger bundle? Pass its capacity explicitly -
[FluidInference/laya-coreml](https://huggingface.co/FluidInference/laya-coreml)
ships fixed buckets of 128/256/512/1024 tokens x 32 options (fp16, plus `e8`
int8-embedding variants ~30% smaller at the same accuracy), and the bridge
picks up whatever `--laya-max-tokens` says. Reported numbers on Apple silicon:
3.7 ms per short question, 5.2 ms median over laya's ten published suites
(3,899 questions) at PyTorch-identical accuracy - see
[FluidUse](https://github.com/FluidInference/FluidUse) and its
[Benchmarks.md](https://github.com/FluidInference/FluidUse/blob/main/Benchmarks.md).
That project is Swift-only (a Swift package + CLI, no Python import or HTTP
server), so nur cannot use it as a bridge backend directly. Its published
buckets are not verified compatible with `laya_coreml.load`; use a documented
`aac6fef` bundle or a local export compatible with that Python loader.

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
- **Partial validation batches.** A malformed question does not sink valid ones.
  An inference exception can still refuse the accepted batch.

### Verification scope

`python -m unittest discover -s scripts -p test_jev_local_bridge.py` exercises
adapter schemas and device gates with small doubles; `python
scripts/jev_local_bridge.py --selftest` exercises the model-free HTTP bridge.
Neither establishes real model accuracy, GPU memory requirements, latency,
calibration, nor Core ML fidelity. Those require target-device model runs.

## Eval sets: record, replay, promote

Every batched ask can be recorded and replayed - the measured-iteration loop,
without re-running the agent:

```bash
NUR_JEV_RECORD=goal-triage nur <goal> --continuous   # record this run's judgments
nur jev eval --set goal-triage                       # replay through the current layer
nur jev eval --set goal-triage --reserved goal-holdout --limit 20
```

Replay re-asks each record through whichever layer is configured (hosted key or
local engine) and reports per-primitive agreement (same pick, same side of the
coin flip, score within half a level), accuracy against hand-added `expected`
labels where a record carries them (`{qid: {"choice"|"noul"|"score": ...}}`),
and what the replay cost. Tune wording and thresholds on the dev set; promote
only when the reserved run agrees too. Records live at
`~/.nur/jev/evals/<set>.jsonl` - plain JSONL, so a set can be hand-built,
trimmed, or labeled with any editor. Agreement is stability, not correctness:
100% agreement with a bad reference only proves determinism, which is exactly
why `expected` labels exist.

## Accounting

Judgments served locally are recorded in the session receipt like any other:

```text
~/.nur/receipts/<session>.jsonl
  auxiliary_inference · purpose "typesafe system one judgments"
  route http://127.0.0.1:8788/v1/systemone · model jev-local:mock
```

The route and model are what actually served the request, so a local engine and
the hosted API are never confused in the audit trail.

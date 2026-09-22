#!/usr/bin/env python3
"""Local System One bridge: typed decisions on this machine, no cloud key.

nur's TypeSafe layer speaks one contract - `POST /v1/systemone` with a `state` and
typed `questions` (choice / noul / score), answered with probabilities and
confidence. That contract is exactly what three open, local decision engines
implement:

| backend  | engine                                                              | device |
|----------|---------------------------------------------------------------------|--------|
| `verdict`| openJev-verdict-2.0 (ModernBERT-base + GLiClass, ~150M)             | CPU / any GPU |
| `nimble` | Bespoke-Nimble-9B (Qwen3.5-9B LoRA, scores answer tokens)            | NVIDIA CUDA GPU with BF16 |
| `laya`   | Laya Core ML (Core ML + Neural Engine)                              | Apple Silicon, macOS 15+ |
| `mock`   | deterministic keyword scorer                                         | anywhere (tests, demos) |

Run it, point nur at it, and the whole harness boost (tool gate, result judge,
compaction, skill checks, routing) works with no API key and no data leaving the
machine:

    python scripts/jev_local_bridge.py --backend verdict --port 8788
    # then: [typesafe] base_url = "http://127.0.0.1:8788/v1/systemone"
    #   or:  export NUR_JEV_LOCAL_URL=http://127.0.0.1:8788/v1/systemone

Subcommands
    --selftest     check the schema mapping without any model (exit 0/1)
    --probe        report which backends are usable on this machine, then exit

The bridge never invents an option: an answer is always one of the caller's own
candidate strings, or the request is refused.
"""

from __future__ import annotations

import argparse
import json
import math
import platform
import sys
import threading
import time
import zlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any

# TypeSafe's own naming, so a mismatch is a refusal rather than a wrong answer.
INSUFFICIENT = "none_of_these"
# openJev signals abstention with its own sentinel; comparing that against nur's
# `none_of_these` is how a "the model says it cannot tell" answer used to slip
# through as "no probability" and get reported as a backend failure.
UPSTREAM_INSUFFICIENT = "__insufficient_evidence__"
DEFAULT_MODEL = "jev-local"

# The harness sends at most 255 Choice options; Score levels are 2..=10.
MAX_CHOICE_OPTIONS = 255
MIN_SCORE_LEVELS = 2
MAX_SCORE_LEVELS = 10


def is_loopback_host(host: str) -> bool:
    """A bind address that is only reachable from this machine."""
    return host.strip().strip("[]").lower() in (
        "127.0.0.1",
        "localhost",
        "::1",
        "0.0.0.0",  # all-interfaces is NOT private, but it IS the local alias set
    ) and host.strip() != "0.0.0.0"


class ContractError(Exception):
    """A request that cannot be answered as asked.

    The bridge refuses instead of guessing: nur treats a refusal as "no
    judgment" and falls back to its own behavior, which is always safe.
    """


# --------------------------------------------------------------------------- #
# Question / answer mapping
# --------------------------------------------------------------------------- #
def question_kind(q: dict) -> str:
    kind = (q.get("type") or "").lower()
    if kind not in ("choice", "noul", "score"):
        raise ContractError(f"unsupported question type {q.get('type')!r}")
    return kind


def choice_options(q: dict) -> list[tuple[str, str]]:
    criteria = q.get("criteria")
    if not isinstance(criteria, dict) or not criteria:
        raise ContractError("choice needs a criteria map of option -> rubric")
    options = [(str(k), str(v) if v is not None else "") for k, v in criteria.items()]
    if len(options) > MAX_CHOICE_OPTIONS:
        raise ContractError(f"choice has {len(options)} options (max {MAX_CHOICE_OPTIONS})")
    return options


def score_levels(q: dict) -> list[str]:
    criteria = q.get("criteria")
    if not isinstance(criteria, list) or len(criteria) < MIN_SCORE_LEVELS:
        raise ContractError("score needs at least two ordered levels")
    if len(criteria) > MAX_SCORE_LEVELS:
        raise ContractError(f"score has {len(criteria)} levels (max {MAX_SCORE_LEVELS})")
    return [str(level) for level in criteria]


def instructions(q: dict) -> str:
    value = q.get("instructions")
    if isinstance(value, str):
        return value
    if value is None:
        raise ContractError("question has no instructions")
    return json.dumps(value, ensure_ascii=False)


def flatten_state(state: Any) -> str:
    if isinstance(state, str):
        return state
    return json.dumps(state, ensure_ascii=False)


def normalise(probabilities: dict[str, float]) -> dict[str, float]:
    """Clamp, drop non-finite values, and renormalise so the distribution sums to 1.

    A zero is kept: for a choice it is a real statement ("this option has no
    mass") and dropping it would silently shrink the caller's option set.
    """
    clean = {
        str(k): max(0.0, float(v))
        for k, v in probabilities.items()
        if isinstance(v, (int, float))
        and not isinstance(v, bool)
        and math.isfinite(float(v))
    }
    total = sum(clean.values())
    if total <= 0:
        # No usable mass: spread evenly rather than claiming certainty.
        if not clean:
            return {}
        even = 1.0 / len(clean)
        return {k: even for k in clean}
    return {k: v / total for k, v in clean.items()}


def confidence_of(probabilities: dict[str, float]) -> float:
    """Distribution concentration, on TypeSafe's 0..1 reading.

    `p_max` alone would call a two-option 0.5/0.5 answer "confident", so this is
    the normalised margin between the top answer and the runner-up - the same
    shape the hosted model reports (a flat distribution is low confidence).
    """
    if not probabilities:
        return 0.0
    ordered = sorted(probabilities.values(), reverse=True)
    top = ordered[0]
    runner_up = ordered[1] if len(ordered) > 1 else 0.0
    if len(ordered) == 1:
        return 1.0
    # Entropy-based, then mapped so a coin flip reads ~0 and certainty ~1.
    entropy = -sum(p * math.log(p) for p in ordered if p > 0)
    max_entropy = math.log(len(ordered))
    if max_entropy <= 0:
        return 1.0
    spread = 1.0 - (entropy / max_entropy)
    margin = top - runner_up
    return max(0.0, min(1.0, 0.6 * spread + 0.4 * margin))


def expected_score(probabilities: dict[str, float], levels: list[str]) -> float:
    """Probability-weighted position on the level scale (TypeSafe's `score`)."""
    total = 0.0
    weight = 0.0
    for index, level in enumerate(levels):
        p = probabilities.get(str(index), probabilities.get(level, 0.0))
        total += index * p
        weight += p
    if weight <= 0:
        return 0.0
    return total / weight


def answer_for(kind: str, q: dict, probabilities: dict[str, float]) -> dict:
    """Build the TypeSafe answer body for one question."""
    if kind == "choice" and UPSTREAM_INSUFFICIENT in probabilities:
        # Abstention, before any re-projection onto the caller's options: mapping
        # a sentinel-only distribution back onto real labels would fabricate a
        # pick (every option at zero mass renormalises to an even spread, and the
        # best of an even spread looks like an answer). nur reads a value that is
        # not one of its candidates as "no judgment", which is the honest result.
        return {
            "type": "choice",
            "choice": UPSTREAM_INSUFFICIENT,
            "probabilities": {UPSTREAM_INSUFFICIENT: 1.0},
            "confidence": 0.0,
        }
    if kind == "noul":
        p_yes = probabilities.get("true", probabilities.get("yes", 0.5))
        return {"type": "noul", "noul": max(0.0, min(1.0, float(p_yes)))}
    if kind == "choice":
        options = [label for label, _ in choice_options(q)]
        resolved = {label: probabilities.get(label, 0.0) for label in options}
        resolved = normalise(resolved)
        if not resolved:
            raise ContractError("choice produced no distribution")
        best = max(resolved.items(), key=lambda kv: kv[1])[0]
        return {
            "type": "choice",
            "choice": best,
            "probabilities": resolved,
            "confidence": confidence_of(resolved),
        }
    levels = score_levels(q)
    ordered = {str(i): probabilities.get(str(i), 0.0) for i in range(len(levels))}
    ordered = normalise(ordered)
    if not ordered:
        raise ContractError("score produced no distribution")
    return {
        "type": "score",
        "score": expected_score(ordered, levels),
        "legend": {str(i): level for i, level in enumerate(levels)},
        "probabilities": ordered,
        "confidence": confidence_of(ordered),
    }


def estimate_tokens(text: str) -> int:
    """Same tokenizer-free estimate nur uses, so both sides agree on sizes."""
    letters = sum(1 for c in text if c.isascii() and c.isalpha())
    digits = sum(1 for c in text if c.isascii() and c.isdigit())
    other = len(text) - letters - digits
    return (letters + 5) // 6 + (digits + 1) // 2 + other


# --------------------------------------------------------------------------- #
# Backends
# --------------------------------------------------------------------------- #
class Backend:
    """One local decision engine.

    `decide(kind, question, state)` returns a probability map keyed by the
    caller's own option labels (choice), `true`/`false` (noul), or level index
    strings (score).

    `decide_batch` exists because the strongest backends evaluate a whole batch in
    one pass (openJev's `DecisionEngine.evaluate(context, queries)` is explicitly
    "a single batched forward pass"). The default is one call per question, which
    is correct for every backend; the ones that can do better override it.
    """

    name = "base"
    max_state_tokens = 512
    note = ""
    # Per-question option/level ceilings, enforced as refusals so an engine never
    # silently truncates the caller's candidate set.
    max_options = MAX_CHOICE_OPTIONS
    max_levels = MAX_SCORE_LEVELS

    def available(self) -> tuple[bool, str]:
        return True, ""

    def decide(self, kind: str, question: dict, state: str) -> dict[str, float]:
        raise NotImplementedError

    def decide_batch(
        self, items: list[tuple[str, dict]], state: str
    ) -> list[dict[str, float]]:
        """Answer several validated questions. Order matches `items`."""
        return [self.decide(kind, question, state) for kind, question in items]


class MockBackend(Backend):
    """Deterministic keyword scorer: no model, no downloads, no surprises.

    Every option is scored by how many of its own words appear in the state, plus
    a stable per-option jitter so ties break consistently across runs. It is not
    intelligent, and it is honest about that: its job is to exercise the whole
    path (bridge, mapping, client, harness) in tests and demos.
    """

    name = "mock"
    max_state_tokens = 100_000
    note = "deterministic keyword scorer - for tests and demos, not for real judgments"
    max_options = MAX_CHOICE_OPTIONS
    max_levels = MAX_SCORE_LEVELS

    def decide(self, kind: str, question: dict, state: str) -> dict[str, float]:
        lowered = state.lower()
        tokens = {t for t in "".join(c if c.isalnum() else " " for c in lowered).split() if t}
        if kind == "noul":
            words = {
                w.strip("?.,:;")
                for w in instructions(question).lower().split()
                if len(w.strip("?.,:;")) > 3
            }
            hits = sum(1 for w in words if w in tokens)
            # Saturating evidence: no mention of the topic is a no, each mention
            # adds less than the last (1 - 0.9*e^-hits: 0.10 / 0.67 / 0.88 / 0.96).
            p_yes = 1.0 - 0.9 * math.exp(-1.0 * hits)
            return {"true": p_yes, "false": 1.0 - p_yes}
        if kind == "choice":
            options = choice_options(question)
            scores: dict[str, float] = {}
            for label, rubric in options:
                words = {
                    w
                    for w in (label + " " + rubric).lower().split()
                    if len(w) > 3
                }
                hits = sum(1 for w in words if w.strip("?.,") in tokens)
                # A stable digest, not `hash()`: Python randomises string hashing
                # per process (PYTHONHASHSEED), so the "deterministic" mock would
                # answer differently run to run and its documented tie-break would
                # not hold.
                jitter = 0.05 * (zlib.crc32(label.encode("utf-8")) % 7) / 7.0
                scores[label] = 0.25 + 0.2 * hits + jitter
            return normalise(scores)
        levels = score_levels(question)
        scored: dict[str, float] = {}
        for index, level in enumerate(levels):
            words = {w for w in level.lower().split() if len(w) > 3}
            hits = sum(1 for w in words if w.strip("?.,") in tokens)
            scored[str(index)] = 0.2 + 0.15 * hits + 0.02 * index
        return normalise(scored)


class VerdictBackend(Backend):
    """openJev-verdict-2.0: non-autoregressive Choice/Score/Noul at ~20-25 ms.

    Uses the upstream package when it is importable (`core`, Apache-2.0)
    and calls its **batched** entry point once per request:

        DecisionEngine(...).evaluate(context, queries) -> DecisionBatchResult

    The engine is constructed once and reused - loading the model per request
    would defeat the point of a 20 ms decision.

    Install (one of):

        pip install "git+https://github.com/Heman10x-NGU/openJev-verdict-2.0"
        git clone https://github.com/Heman10x-NGU/openJev-verdict-2.0 ~/openJev

    Weights: `heman10x/rlcd-modernbert-151m` (a GLiClass base also works).
    `--verdict-model mock` uses upstream's own mock mode: the real code path with
    deterministic logits, no checkpoint, no GPU - which is how the mapping below
    is tested.
    """

    name = "verdict"
    max_state_tokens = 512
    max_options = 24  # upstream Choice/Option cap
    max_levels = 24  # upstream Score/Level cap
    note = "openJev-verdict-2.0 (ModernBERT-base + GLiClass, ~150M)"

    def __init__(self, model: str | None = None, device: str = "cpu"):
        self.model_name = model or "knowledgator/gliclass-modern-base-v2.0"
        self.device = device
        self._engine = None

    def available(self) -> tuple[bool, str]:
        if self.device not in ("cpu", "cuda", "mps"):
            return False, f"unsupported torch device {self.device!r}; use cpu, cuda, or mps"
        if self.device != "cpu":
            try:
                import torch
            except Exception as exc:
                return False, f"cannot verify torch device {self.device}: {exc}"
            if self.device == "cuda" and not torch.cuda.is_available():
                return False, "CUDA requested but torch reports no CUDA device"
            if self.device == "mps" and not (
                hasattr(torch.backends, "mps") and torch.backends.mps.is_available()
            ):
                return False, "MPS requested but torch reports no usable MPS device"
        try:
            from core import DecisionEngine, Choice, Level, Noul, Option, Score  # noqa: F401
        except Exception as exc:
            return False, (
                'pip install "git+https://github.com/Heman10x-NGU/openJev-verdict-2.0" '
                f"(core API import failed: {exc})"
            )
        return True, ""

    def _engine_or_raise(self):
        if self._engine is not None:
            return self._engine
        try:
            from core import DecisionEngine  # type: ignore
        except Exception as exc:
            raise ContractError(f"openJev is not importable: {exc}")
        if self.model_name == "mock":
            # Upstream's own mock path: no weights, deterministic logits.
            self._engine = DecisionEngine(model="mock", device="cpu")
        else:
            self._engine = DecisionEngine(
                model_name_or_path=self.model_name, device=self.device
            )
        return self._engine

    def _queries_or_raise(self, items: list[tuple[str, dict]]) -> list:
        try:
            from core import Choice, Level, Noul, Option, Score  # type: ignore
        except Exception as exc:
            raise ContractError(f"openJev primitives unavailable: {exc}")
        queries = []
        for index, (kind, question) in enumerate(items):
            qid = f"q{index}"
            text = _plain_text(instructions(question))
            if kind == "noul":
                # Upstream's Noul carries a proposition plus its semantics tag.
                queries.append(
                    Noul(
                        id=qid,
                        proposition=text,
                        semantics="conditional_on_sufficient_evidence_v2",
                    )
                )
            elif kind == "choice":
                options = choice_options(question)
                queries.append(
                    Choice(
                        id=qid,
                        question=text,
                        options=tuple(
                            Option(id=label, description=rubric or label)
                            for label, rubric in options
                        ),
                    )
                )
            else:
                levels = score_levels(question)
                queries.append(
                    Score(
                        id=qid,
                        question=text,
                        levels=tuple(
                            Level(id=str(i), description=level, value=float(i))
                            for i, level in enumerate(levels)
                        ),
                    )
                )
        return queries

    def decide_batch(
        self, items: list[tuple[str, dict]], state: str
    ) -> list[dict[str, float]]:
        engine = self._engine_or_raise()
        queries = self._queries_or_raise(items)
        try:
            batch = engine.evaluate(state, queries)
        except TypeError:
            # A positional signature drift (context/queries order) should not turn
            # into a wrong answer: retry the other order, then fail loudly.
            batch = engine.evaluate(queries, state)
        results = list(getattr(batch, "results", batch) or [])
        if len(results) != len(items):
            raise ContractError(
                f"openJev returned {len(results)} results for {len(items)} questions"
            )
        return [
            _verdict_result_to_probabilities(kind, question, result)
            for (kind, question), result in zip(items, results)
        ]


def _plain_text(value: Any) -> str:
    """Upstream's fields are plain strings; a structured instruction is flattened."""
    return value if isinstance(value, str) else json.dumps(value, ensure_ascii=False)


def _get(result: Any, *names: str, default: Any = None) -> Any:
    """Read a field from a dataclass-ish result, with a dict fallback."""
    for name in names:
        if hasattr(result, name):
            return getattr(result, name)
        if isinstance(result, dict) and name in result:
            return result[name]
    return default


def _verdict_result_to_probabilities(
    kind: str, question: dict, result: Any
) -> dict[str, float]:
    """Translate an upstream result into nur's probability maps.

    Mapping decisions that matter:

    - **Abstention is not a pick.** Upstream reports `is_abstention` (and
      `__insufficient_evidence__` for Noul). nur's contract expresses "nothing
      fits" by an answer that is not one of the caller's options, which nur then
      reads as *no judgment* - so abstention is passed through faithfully instead
      of being forced onto the least-bad option.
    - **Noul is conditional.** `p_true_given_sufficient_evidence` is the
      probability nur wants; when the model says the evidence is insufficient,
      the honest translation is the coin flip (0.5), which nur bands as
      `escalate` rather than acting on.
    """
    probabilities = _get(result, "probabilities", default=None)
    abstained = bool(
        _get(result, "is_abstention", default=False)
        or _get(result, "selected_outcome", default="") == UPSTREAM_INSUFFICIENT
        or _get(result, "selected_id", default="") == UPSTREAM_INSUFFICIENT
    )

    if kind == "noul":
        if abstained:
            return {"true": 0.5, "false": 0.5}
        p_true = _get(result, "p_true_given_sufficient_evidence", default=None)
        if p_true is None:
            outcome = _get(result, "selected_outcome", default=None)
            if outcome in ("true", "false"):
                return {"true": 1.0 if outcome == "true" else 0.0}
            if probabilities:
                mapping = {str(k).lower(): float(v) for k, v in dict(probabilities).items()}
                p_true = mapping.get("true", mapping.get("yes"))
        if p_true is None:
            raise ContractError("openJev returned no yes/no probability")
        return {"true": float(p_true)}

    if abstained:
        if kind == "choice":
            # Not one of the caller's options, so nur reads it as "no judgment"
            # instead of forcing a pick the model said it could not make.
            return {UPSTREAM_INSUFFICIENT: 1.0}
        # A score abstention has no level: report an even spread, which nur bands
        # as low confidence instead of inventing a level.
        levels = score_levels(question)
        return normalise({str(i): 1.0 for i in range(len(levels))})

    if probabilities is None:
        raise ContractError("openJev returned no probabilities for this question")
    return {str(k): float(v) for k, v in dict(probabilities).items()}


class NimbleBackend(Backend):
    """Bespoke-Nimble-9B: a Qwen3.5-9B LoRA that scores answer tokens directly.

    Upstream ships the exact prompt builder and scoring helper with the adapter,
    so this uses it rather than reimplementing the prompt:

        hf download bespokelabs/Bespoke-Nimble-9B --local-dir nimble-model
        pip install -r nimble-model/requirements.txt
        python scripts/jev_local_bridge.py --backend nimble --nimble-dir nimble-model

    Limits, from the model card: at most 26 choices per field, prompts over 2048
    tokens are rejected (not truncated), no free-form generation.
    """

    name = "nimble"
    max_state_tokens = 2048
    max_options = 26  # the model card's own limit per field
    max_levels = 26
    note = "Bespoke-Nimble-9B (Qwen3.5-9B LoRA, scores allowed answer tokens)"

    def __init__(self, model_dir: str | None = None):
        self.model_dir = model_dir or "nimble-model"
        self._model = None
        self._error = ""

    def available(self) -> tuple[bool, str]:
        import os

        inference = os.path.join(self.model_dir, "inference.py")
        if not os.path.isfile(inference):
            return False, (
                f"no model at {self.model_dir} - "
                "hf download bespokelabs/Bespoke-Nimble-9B --local-dir nimble-model"
            )
        try:
            import torch  # noqa: F401
        except Exception as exc:
            return False, f"PyTorch is required for Nimble (import failed: {exc})"
        if not (torch.cuda.is_available() if hasattr(torch, "cuda") else False):
            # The card targets CUDA BF16; Apple Silicon runs the upstream MLX path
            # (see the repo README). Say what is missing instead of failing later.
            return False, "no CUDA device visible - Nimble expects an NVIDIA GPU (or use the MLX path upstream)"
        try:
            # Exclude software emulation: the reference helper loads BF16 weights.
            if not torch.cuda.is_bf16_supported(including_emulation=False):
                return False, "Nimble requires an NVIDIA CUDA GPU with native BF16 support"
        except Exception as exc:
            return False, f"cannot verify CUDA BF16 support: {exc}"
        return True, ""

    def _load(self):
        if self._model is not None:
            return self._model
        import sys as _sys

        _sys.path.insert(0, self.model_dir)
        from inference import NimbleModel  # type: ignore

        self._model = NimbleModel(self.model_dir)
        return self._model

    def decide(self, kind: str, question: dict, state: str) -> dict[str, float]:
        model = self._load()
        field = "decision"
        score_fields: list[str] = []
        if kind == "noul":
            schema = {field: {"type": "boolean", "description": instructions(question)}}
        elif kind == "choice":
            options = choice_options(question)
            schema = {
                field: {
                    "type": "enum",
                    "description": instructions(question),
                    "choices": [label for label, _ in options],
                    "choice_descriptions": [rubric or label for _, rubric in options],
                }
            }
        else:
            # The card's rubric form: integer-valued enum strings plus the field
            # name in `score_fields`, which is what returns the expected score.
            levels = score_levels(question)
            schema = {
                field: {
                    "type": "enum",
                    "description": instructions(question),
                    "choices": [str(i) for i in range(len(levels))],
                    "choice_descriptions": levels,
                }
            }
            score_fields = [field]
        if score_fields:
            result = model.score(context=state, schema=schema, score_fields=score_fields)
        else:
            result = model.score(context=state, schema=schema)
        fields = result.get("fields", {}) if isinstance(result, dict) else {}
        entry = fields.get(field, {}) or {}
        probabilities = entry.get("probabilities")
        if not probabilities:
            chosen = entry.get("output")
            if chosen is None:
                raise ContractError("Nimble returned no probabilities for this question")
            probabilities = {str(chosen): 1.0}
        if kind == "noul":
            mapping = {str(k).lower(): float(v) for k, v in probabilities.items()}
            p_yes = mapping.get("true", mapping.get("yes"))
            if p_yes is None:
                raise ContractError("Nimble returned no yes/no probability")
            return {"true": p_yes}
        if kind == "score":
            return {str(k): float(v) for k, v in probabilities.items()}
        return {str(k): float(v) for k, v in probabilities.items()}


class LayaBackend(Backend):
    """Laya Core ML: typed decisions on the Neural Engine, Apple Silicon only.

        pip install laya-coreml
        python scripts/jev_local_bridge.py --backend laya

    Upstream API is `laya.load(model_id)` then `agent.predict(text, schema)` with
    `noul` / `choice` / `score` question types - the same primitives nur uses.

    Capacity follows the bundle, not the code. The default ANE bundle caps a
    request at 96 tokens (question + options + state) and raises a capacity
    error beyond that. The `aac6fef/laya-multilingual-coreml` bundle has a
    1024-token context; pass `--laya-max-tokens 1024` for that bundle.
    The bridge's preflight is approximate; general bundles may truncate upstream.
    Third-party FluidInference bundles are not verified with this Python loader.
    """

    name = "laya"
    max_state_tokens = 96  # the default ANE bundle's 96-token total (question + options + state)
    max_options = 32
    max_levels = 32
    note = "Laya Core ML (Core ML + Neural Engine, macOS/Apple Silicon)"

    def __init__(self, model_id: str = "aac6fef/laya-multilingual-coreml-ane",
                 max_tokens: int | None = None):
        self.model_id = model_id
        if max_tokens is not None and max_tokens > 0:
            self.max_state_tokens = max_tokens
        self._agent = None

    def available(self) -> tuple[bool, str]:
        machine = platform.machine().lower()
        system = platform.system().lower()
        if system != "darwin" or machine not in ("arm64", "aarch64"):
            return False, f"Apple Silicon macOS required (this is {system}/{machine})"
        version = platform.mac_ver()[0]
        try:
            major = int(version.split(".")[0])
        except ValueError:
            return False, f"cannot verify macOS 15+ requirement (reported {version!r})"
        if major < 15:
            return False, f"macOS 15+ required (this is {version})"
        if not (3, 11) <= sys.version_info[:2] <= (3, 13):
            return False, "Laya Core ML supports Python 3.11-3.13"
        try:
            import laya_coreml  # noqa: F401
        except Exception as exc:
            return False, f"pip install laya-coreml (import failed: {exc})"
        return True, ""

    def _load(self):
        if self._agent is not None:
            return self._agent
        import laya_coreml as laya  # type: ignore

        self._agent = laya.load(self.model_id)
        return self._agent

    def decide(self, kind: str, question: dict, state: str) -> dict[str, float]:
        agent = self._load()
        spec: dict[str, Any] = {"instructions": instructions(question)}
        field = "decision"
        if kind == "noul":
            spec["type"] = "noul"
        elif kind == "choice":
            options = choice_options(question)
            spec["type"] = "choice"
            spec["criteria"] = {label: rubric or label for label, rubric in options}
        else:
            spec["type"] = "score"
            spec["criteria"] = score_levels(question)
        result = agent.predict(state, {field: spec})
        answers = result.get("answers", {}) if isinstance(result, dict) else {}
        entry = answers.get(field, {}) or {}
        probabilities = entry.get("probabilities")
        if kind == "noul":
            value = entry.get("noul", entry.get("value"))
            if value is None and probabilities:
                mapping = {str(k).lower(): float(v) for k, v in probabilities.items()}
                value = mapping.get("true", mapping.get("yes"))
            if value is None:
                raise ContractError("Laya returned no yes/no probability")
            return {"true": float(value)}
        if not probabilities:
            chosen = entry.get("choice", entry.get("score"))
            if chosen is None:
                raise ContractError("Laya returned no probabilities for this question")
            probabilities = {str(chosen): 1.0}
        if kind == "score":
            return {str(k): float(v) for k, v in probabilities.items()}
        return {str(k): float(v) for k, v in probabilities.items()}


BACKENDS = {
    "mock": MockBackend,
    "verdict": VerdictBackend,
    "nimble": NimbleBackend,
    "laya": LayaBackend,
}


_BACKEND_LOCKS: dict[str, "threading.RLock"] = {}
_BACKEND_LOCKS_GUARD = threading.Lock()


def backend_lock(name: str) -> "threading.RLock":
    """The per-backend mutex that serializes lazy load and inference."""
    with _BACKEND_LOCKS_GUARD:
        lock = _BACKEND_LOCKS.get(name)
        if lock is None:
            lock = _BACKEND_LOCKS[name] = threading.RLock()
        return lock


def build_backend(name: str, args: argparse.Namespace) -> Backend:
    if name == "verdict":
        return VerdictBackend(model=args.verdict_model, device=args.device)
    if name == "nimble":
        return NimbleBackend(model_dir=args.nimble_dir)
    if name == "laya":
        return LayaBackend(model_id=args.laya_model, max_tokens=args.laya_max_tokens)
    return MockBackend()


# --------------------------------------------------------------------------- #
# Request handling
# --------------------------------------------------------------------------- #
def handle_evaluate(backend: Backend, body: dict) -> dict:
    if not isinstance(body, dict):
        raise ContractError("request body must be an object")
    state = body.get("state")
    if state is None:
        raise ContractError("request has no state")
    text = flatten_state(state)
    questions = body.get("questions")
    if not isinstance(questions, dict) or not questions:
        raise ContractError("request has no questions")

    tokens = estimate_tokens(text)
    answers: dict[str, dict] = {}
    errors: list[str] = []

    # Validate every question first, so only well-formed ones are handed to the
    # engine (and so a batch keeps its indices aligned with `accepted`).
    accepted: list[tuple[str, str, dict]] = []  # (qid, kind, question)
    for qid, q in questions.items():
        if not isinstance(q, dict):
            errors.append(f"{qid}: question must be an object")
            continue
        try:
            kind = question_kind(q)
            if kind == "choice" and len(choice_options(q)) > backend.max_options:
                raise ContractError(
                    f"choice has {len(choice_options(q))} options; {backend.name} allows "
                    f"{backend.max_options}"
                )
            if kind == "score" and len(score_levels(q)) > backend.max_levels:
                raise ContractError(
                    f"score has {len(score_levels(q))} levels; {backend.name} allows "
                    f"{backend.max_levels}"
                )
            # Per-question budget: the state, the question, and its options all
            # count against a local model's context.
            overhead = estimate_tokens(instructions(q) + json.dumps(q.get("criteria") or ""))
            if tokens + overhead > backend.max_state_tokens:
                raise ContractError(
                    f"state+question is ~{tokens + overhead} tokens; {backend.name} allows "
                    f"{backend.max_state_tokens}"
                )
            accepted.append((qid, kind, q))
        except ContractError as exc:
            errors.append(f"{qid}: {exc}")

    if accepted:
        try:
            # One request per backend at a time. The server is threading and nur
            # splits >24 questions into parallel requests (`max_parallel`, default
            # 4), so two threads can reach a cold backend at once: without this
            # lock each would build its own copy of the weights (two Qwen3.5-9B
            # instances for Nimble, one silently discarded) and a HF forward pass
            # is not thread-safe to begin with.
            with backend_lock(backend.name):
                distributions = backend.decide_batch(
                    [(kind, q) for _, kind, q in accepted], text
                )
        except ContractError as exc:
            errors.extend(f"{qid}: {exc}" for qid, _, _ in accepted)
            distributions = []
        except Exception as exc:  # a backend crash must not take the bridge down
            errors.extend(
                f"{qid}: {type(exc).__name__}: {exc}" for qid, _, _ in accepted
            )
            distributions = []
        if len(distributions) != len(accepted):
            errors.append(
                f"backend returned {len(distributions)} distributions for "
                f"{len(accepted)} questions"
            )
            distributions = []
        for (qid, kind, q), probabilities in zip(accepted, distributions):
            try:
                answers[qid] = answer_for(kind, q, probabilities)
            except ContractError as exc:
                errors.append(f"{qid}: {exc}")
            except Exception as exc:
                errors.append(f"{qid}: {type(exc).__name__}: {exc}")

    return {
        "model": f"{DEFAULT_MODEL}:{backend.name}",
        "answers": answers,
        "errors": errors,
        "usage": {"input_tokens": tokens, "output_tokens": len(answers)},
    }


# Largest request body the bridge will read. nur's own client caps state at
# `MAX_STATE_CHARS` (60k chars) plus questions; 8 MiB leaves an order of
# magnitude of headroom while making a hostile `content-length` harmless.
MAX_BODY_BYTES = 8 * 1024 * 1024


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    backend: Backend = MockBackend()  # replaced at startup
    stats = {"requests": 0, "questions": 0, "errors": 0, "started": time.time()}
    # Requests are served on threads; the counters are reporting-only but should
    # still be accurate.
    stats_lock = threading.Lock()

    @classmethod
    def bump(cls, key: str, by: int = 1) -> None:
        with cls.stats_lock:
            cls.stats[key] = cls.stats.get(key, 0) + by

    def log_message(self, *args):
        pass

    def _json(self, status: int, payload: dict) -> None:
        data = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(data)))
        self.send_header("connection", "close")
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.path.startswith("/health"):
            self._json(
                200,
                {
                    "status": "ok",
                    "backend": self.backend.name,
                    "note": self.backend.note,
                    "max_state_tokens": self.backend.max_state_tokens,
                    "stats": {
                        "requests": self.stats["requests"],
                        "questions": self.stats["questions"],
                        "errors": self.stats["errors"],
                        "uptime_s": round(time.time() - self.stats["started"], 1),
                    },
                },
            )
            return
        if self.path.startswith("/backends"):
            self._json(200, {"backends": describe_backends(), "active": self.backend.name})
            return
        self._json(404, {"detail": {"error_type": "not_found", "message": self.path}})

    def do_POST(self):
        if not self.path.startswith("/v1/systemone"):
            self._json(404, {"detail": {"error_type": "not_found", "message": self.path}})
            return
        try:
            length = int(self.headers.get("content-length") or 0)
        except ValueError:
            length = 0
        if length > MAX_BODY_BYTES:
            self._json(
                413,
                {
                    "detail": {
                        "error_type": "request_too_large",
                        "message": f"body is {length} bytes; this bridge accepts up to "
                        f"{MAX_BODY_BYTES}",
                    }
                },
            )
            return
        raw = self.rfile.read(length) if length else b""
        try:
            body = json.loads(raw.decode("utf-8"))
        except Exception as exc:
            self._json(
                422,
                {"detail": {"error_type": "invalid_request", "message": f"invalid JSON: {exc}"}},
            )
            return
        self.bump("requests")
        try:
            result = handle_evaluate(self.backend, body)
        except ContractError as exc:
            self.bump("errors")
            self._json(
                422,
                {"detail": {"error_type": "invalid_request", "message": str(exc)}},
            )
            return
        self.bump("questions", len(result["answers"]))
        self.bump("errors", len(result["errors"]))
        # Partial answers are normal: nur treats a missing answer as "no
        # judgment" and keeps its own behavior for that question.
        self._json(200, result)


def describe_backends(args=None) -> dict[str, dict]:
    out: dict[str, dict] = {}
    for name in BACKENDS:
        try:
            if args is None:
                instance = BACKENDS[name]()
            else:
                instance = build_backend(name, args)
            ok, why = instance.available()
        except Exception as exc:  # pragma: no cover
            ok, why = False, f"{type(exc).__name__}: {exc}"
        out[name] = {
            "available": ok,
            "reason": why,
            "note": instance.note,
            "max_state_tokens": instance.max_state_tokens,
            "device": _device_note(name),
        }
    return out


def _device_note(name: str) -> str:
    machine = platform.machine().lower()
    system = platform.system()
    if name == "laya":
        return "Apple Silicon + macOS 15+ (Neural Engine)"
    if name == "nimble":
        return "NVIDIA CUDA GPU with native BF16 (MLX is not integrated)"
    if name == "verdict":
        return "CPU (any); GPU optional"
    return f"{system}/{machine}"


# --------------------------------------------------------------------------- #
# Self-test: the mapping must hold with no model present
# --------------------------------------------------------------------------- #
def _selftest_backends(check) -> None:
    """Exercise every adapter against a *simulated* upstream API.

    The engines themselves are not installable here (no CUDA, no Apple Silicon,
    no weights), so the useful test is the part that is ours: how a question is
    translated into upstream's schema, and how upstream's result is translated
    back. Both directions are faked against the shapes read from upstream source,
    which is what keeps the adapters from drifting while claiming to be tested.
    """
    import sys
    import types

    def fresh(name: str):
        module = types.ModuleType(name)
        sys.modules[name] = module
        return module

    def drop(*names: str) -> None:
        for name in names:
            sys.modules.pop(name, None)

    # ---------------------------------------------------------------- openJev
    core = fresh("core")
    seen: dict[str, Any] = {}

    class Option:  # upstream: id + description
        def __init__(self, id, description):
            self.id, self.description = id, description

    class Level(Option):  # upstream: + numeric value
        def __init__(self, id, description, value):
            self.id, self.description, self.value = id, description, value

    class Choice:
        def __init__(self, id, question, options):
            self.id, self.question, self.options = id, question, tuple(options)

    class Score:
        def __init__(self, id, question, levels):
            self.id, self.question, self.levels = id, question, tuple(levels)

    class Noul:
        def __init__(self, id, proposition, semantics):
            self.id, self.proposition, self.semantics = id, proposition, semantics

    class ChoiceResult:
        def __init__(self, selected_id, probabilities, is_abstention=False):
            self.selected_id = selected_id
            self.probabilities = probabilities
            self.is_abstention = is_abstention
            self.selected_probability = probabilities.get(selected_id, 0.0)
            self.concentration = 0.5

    class ScoreResult:
        def __init__(self, probabilities, is_abstention=False):
            self.probabilities = probabilities
            self.is_abstention = is_abstention
        @property
        def expected_score(self):
            return float(sum(int(k) * v for k, v in self.probabilities.items()))

    class NoulResult:
        def __init__(self, outcome, p_true=None):
            self.selected_outcome = outcome
            self.p_true_given_sufficient_evidence = p_true

    class _Batch:
        def __init__(self, results):
            self.results = tuple(results)

    class DecisionEngine:
        def __init__(self, model_name_or_path=None, model=None, tokenizer=None,
                     calibrator=None, device="cpu", max_length=512):
            seen["engine"] = {"model": model, "path": model_name_or_path, "device": device}
            seen["mock_mode"] = model == "mock"

        def evaluate(self, context, queries):
            seen["calls"] = seen.get("calls", 0) + 1
            seen["context"] = context
            seen["queries"] = list(queries)
            results = []
            for q in queries:
                if isinstance(q, Noul):
                    # Abstention and a real probability, one of each.
                    results.append(
                        NoulResult("__insufficient_evidence__", None)
                        if "unsure" in q.proposition
                        else NoulResult("true", 0.88)
                    )
                elif isinstance(q, Choice):
                    results.append(ChoiceResult(q.options[0].id, {q.options[0].id: 0.9, q.options[1].id: 0.1}))
                else:
                    results.append(ScoreResult({str(i): 1.0 / len(q.levels) for i in range(len(q.levels))}))
            return _Batch(results)

    core.DecisionEngine = DecisionEngine
    core.Option, core.Level, core.Choice, core.Score, core.Noul = Option, Level, Choice, Score, Noul
    core.ChoiceResult, core.ScoreResult, core.NoulResult = ChoiceResult, ScoreResult, NoulResult

    verdict = VerdictBackend(model="mock")
    ok, why = verdict.available()
    check("openJev adapter reports available with `core` importable", ok, why)
    verdict_result = handle_evaluate(
        verdict,
        {
            "state": "the customer reports a duplicate charge",
            "questions": {
                "urgent": {"type": "noul", "instructions": "Is this urgent?"},
                "unsure": {"type": "noul", "instructions": "Is this unsure about anything?"},
                "team": {
                    "type": "choice",
                    "instructions": "Which team?",
                    "criteria": {"billing": "payments", "technical": "bugs"},
                },
                "tone": {"type": "score", "instructions": "Tone?", "criteria": ["calm", "angry"]},
            },
        },
    )
    check("openJev: one batched evaluate call for four questions", seen.get("calls") == 1, str(seen.get("calls")))
    check("openJev: mock mode reached upstream's own model='mock' sentinel", seen.get("mock_mode") is True, str(seen.get("engine")))
    q = seen.get("queries", [])
    check(
        "openJev: Noul is built with proposition+semantics, not `question`",
        len(q) == 4 and hasattr(q[0], "proposition") and q[0].semantics == "conditional_on_sufficient_evidence_v2",
        str(getattr(q[0], "__dict__", {}))[:160],
    )
    check(
        "openJev: Choice options carry the caller's own labels",
        [o.id for o in q[2].options] == ["billing", "technical"],
        str([o.id for o in q[2].options]),
    )
    check(
        "openJev: Score levels carry index ids and numeric values",
        [l.id for l in q[3].levels] == ["0", "1"] and q[3].levels[1].value == 1.0,
        str([(l.id, l.value) for l in q[3].levels]),
    )
    a = verdict_result["answers"]
    check("openJev: noul p(true) is read from the conditional field", a["urgent"]["noul"] == 0.88, str(a.get("urgent")))
    check(
        "openJev: an abstained noul becomes the coin flip nur bands as escalate",
        a["unsure"]["noul"] == 0.5,
        str(a.get("unsure")),
    )
    check(
        "openJev: choice keeps the option keys and a confidence",
        a["team"]["choice"] == "billing" and 0.0 <= a["team"]["confidence"] <= 1.0,
        str(a.get("team")),
    )
    check(
        "openJev: score uses upstream's expected value",
        abs(a["tone"]["score"] - 0.5) < 1e-9,
        str(a.get("tone")),
    )

    # An abstaining *choice* must not be forced onto a real option.
    core.DecisionEngine = type(
        "DecisionEngine",
        (),
        {"__init__": lambda self, **kw: None,
         "evaluate": lambda self, context, queries: _Batch([
             ChoiceResult("__insufficient_evidence__",
                          {"billing": 0.5, "technical": 0.5}, is_abstention=True)
         ])},
    )
    verdict2 = VerdictBackend(model="mock")
    abstained = handle_evaluate(
        verdict2,
        {"state": "s", "questions": {"team": {"type": "choice", "instructions": "Which team?",
                                             "criteria": {"billing": "payments", "technical": "bugs"}}}},
    )
    check(
        "openJev: a choice abstention reaches nur as a non-option (no judgment, not a guess)",
        abstained["answers"]["team"]["choice"] == UPSTREAM_INSUFFICIENT,
        str(abstained.get("answers")),
    )
    check(
        "openJev: the abstention sentinel is upstream's, not nur's no-match word",
        UPSTREAM_INSUFFICIENT != INSUFFICIENT,
        f"{UPSTREAM_INSUFFICIENT!r} vs {INSUFFICIENT!r}",
    )
    drop("core")

    # ----------------------------------------------------------------- Nimble
    inference = fresh("inference")
    nimble_seen: dict[str, Any] = {}

    class NimbleModel:
        def __init__(self, path):
            nimble_seen["path"] = path

        def score(self, context=None, schema=None, score_fields=None):
            nimble_seen["context"] = context
            nimble_seen["schema"] = schema
            nimble_seen["score_fields"] = score_fields
            field = next(iter(schema))
            spec = schema[field]
            if spec["type"] == "boolean":
                probs = {"true": 0.8, "false": 0.2}
            else:
                probs = {str(choice): (1.0 if i == 0 else 0.0)
                         for i, choice in enumerate(spec["choices"])}
            return {"output": next(iter(probs)), "fields": {field: {"probabilities": probs}}}

    inference.NimbleModel = NimbleModel
    nimble = NimbleBackend(model_dir="nimble-model")
    nimble._model = NimbleModel("nimble-model")  # bypass the CUDA gate
    nimble_items = [
        ("noul", {"type": "noul", "instructions": "Is it urgent?"}),
        ("choice", {"type": "choice", "instructions": "Which team?",
                    "criteria": {"billing": "payments", "technical": "bugs"}}),
        ("score", {"type": "score", "instructions": "Tone?", "criteria": ["calm", "angry"]}),
    ]
    n_out = nimble.decide_batch(nimble_items, "the customer reports a duplicate charge")
    check("nimble: boolean question maps to a boolean field", n_out[0]["true"] == 0.8, str(n_out[0]))
    check(
        "nimble: choice maps to enum choices and reads probabilities back",
        n_out[1] == {"billing": 1.0, "technical": 0.0},
        str(n_out[1]),
    )
    check(
        "nimble: a rubric score passes the field in score_fields",
        nimble_seen.get("score_fields") == ["decision"],
        str(nimble_seen.get("score_fields")),
    )
    check(
        "nimble: the context is the state, not a re-serialized blob",
        "duplicate charge" in str(nimble_seen.get("context")),
        str(nimble_seen.get("context"))[:60],
    )
    too_many = handle_evaluate(
        nimble,
        {"state": "s", "questions": {"pick": {"type": "choice", "instructions": "pick",
                                             "criteria": {f"o{i}": None for i in range(30)}}}},
    )
    check(
        "nimble: more than 26 choices is refused, not truncated",
        too_many["answers"] == {} and any("allows 26" in e for e in too_many["errors"]),
        str(too_many["errors"]),
    )
    drop("inference")

    # ------------------------------------------------------------------- Laya
    laya_mod = fresh("laya_coreml")
    laya_seen: dict[str, Any] = {}

    class LayaAgent:
        def predict(self, text, schema):
            laya_seen["text"] = text
            laya_seen["schema"] = schema
            field = next(iter(schema))
            spec = schema[field]
            if spec["type"] == "noul":
                answer = {"type": "noul", "noul": 0.77}
            elif spec["type"] == "choice":
                label = next(iter(spec["criteria"]))
                answer = {"type": "choice", "choice": label,
                          "probabilities": {label: 1.0}}
            else:
                answer = {"type": "score", "score": 1.0,
                          "probabilities": {str(i): 0.5 for i in range(len(spec["criteria"]))}}
            return {"answers": {field: answer}}

    laya_mod.load = lambda model_id, **kw: (laya_seen.__setitem__("model_id", model_id), LayaAgent())[1]
    laya = LayaBackend(model_id="test-bundle")
    laya._agent = LayaAgent()  # bypass the Apple-Silicon gate
    l_out = laya.decide_batch(nimble_items, "the customer reports a duplicate charge")
    check("laya: noul maps to type=noul and reads the probability", l_out[0]["true"] == 0.77, str(l_out[0]))
    check("laya: choice maps to criteria and keeps the labels", l_out[1] == {"billing": 1.0}, str(l_out[1]))
    check(
        "laya: score maps its ordered levels onto criteria strings",
        laya_seen["schema"]["decision"]["criteria"] == ["calm", "angry"],
        str(laya_seen["schema"]["decision"]["criteria"]),
    )
    laya.decide("noul", nimble_items[0][1], "the customer reports a duplicate charge")
    check(
        "laya: instructions travel with the question",
        "urgent" in str(laya_seen["schema"]["decision"]["instructions"]),
        str(laya_seen["schema"]["decision"]),
    )
    drop("laya_coreml")

    # Every adapter must refuse cleanly when its engine is absent.
    for backend_name, module_names in (
        ("verdict", ("core", "openjev", "rlcd")),
        ("nimble", ("inference",)),
        ("laya", ("laya_coreml",)),
    ):
        drop(*module_names)
        cls = {"verdict": VerdictBackend, "nimble": NimbleBackend, "laya": LayaBackend}[backend_name]
        instance = cls()
        ok, why = instance.available()
        check(
            f"{backend_name}: reports unavailable with an actionable reason",
            (not ok) and bool(why),
            f"available={ok} why={why!r}",
        )


def selftest() -> int:
    """Assert the contract mapping. Returns 0 on success, 1 on failure."""
    import http.client
    import threading

    failures: list[str] = []

    def check(name: str, condition: bool, detail: str = "") -> None:
        if condition:
            print(f"  ok   {name}")
        else:
            failures.append(f"{name} {detail}".strip())
            print(f"  FAIL {name} {detail}")

    print("local System One bridge selftest")
    backend = MockBackend()

    # --- noul
    noul_q = {"type": "noul", "instructions": "Does the message report an outage?"}
    probs = backend.decide("noul", noul_q, "Our payments are down, an outage is ongoing.")
    answer = answer_for("noul", noul_q, probs)
    check("noul answer has the right shape", answer["type"] == "noul" and "noul" in answer)
    check(
        "noul probability is in range",
        0.0 <= answer["noul"] <= 1.0,
        str(answer),
    )
    check(
        "noul reacts to the state",
        answer["noul"] > 0.5,
        f"p={answer['noul']:.2f} for a state that mentions an outage",
    )

    # --- choice never invents an option
    choice_q = {
        "type": "choice",
        "instructions": "Which team should handle this?",
        "criteria": {
            "billing": "payments, invoices, refunds",
            "technical": "bugs, outages, integrations",
            "sales": "pricing, upgrades",
        },
    }
    probs = backend.decide("choice", choice_q, "There is an outage, the integration is down.")
    answer = answer_for("choice", choice_q, probs)
    labels = set(choice_q["criteria"])
    check("choice picks one of the caller's options", answer["choice"] in labels)
    check(
        "choice distribution covers every option",
        set(answer["probabilities"]) == labels,
        str(sorted(answer["probabilities"])),
    )
    check(
        "choice distribution sums to 1",
        abs(sum(answer["probabilities"].values()) - 1.0) < 1e-9,
        str(sum(answer["probabilities"].values())),
    )
    check(
        "choice confidence is in range",
        0.0 <= answer["confidence"] <= 1.0,
        str(answer["confidence"]),
    )

    # --- a refusal keeps the caller safe
    refused = handle_evaluate(
        backend,
        {"state": "x", "questions": {"q": {"type": "label", "instructions": "?"}}},
    )
    check(
        "unknown question type is refused",
        refused["answers"] == {}
        and any("unsupported question type" in e for e in refused["errors"]),
        str(refused),
    )

    # --- score is an ordered expectation over the caller's levels
    score_q = {
        "type": "score",
        "instructions": "How frustrated is the customer?",
        "criteria": ["Calm", "Frustrated", "Very angry"],
    }
    probs = backend.decide("score", score_q, "This is the third time I have asked, very angry.")
    answer = answer_for("score", score_q, probs)
    check("score stays inside the level range", 0.0 <= answer["score"] <= 2.0, str(answer["score"]))
    check(
        "score legend maps every level",
        answer["legend"] == {"0": "Calm", "1": "Frustrated", "2": "Very angry"},
        str(answer["legend"]),
    )
    check(
        "score probabilities are keyed by level index",
        set(answer["probabilities"]) == {"0", "1", "2"},
        str(sorted(answer["probabilities"])),
    )

    # --- confidence distinguishes a coin flip from a decision
    flat = normalise({"a": 1.0, "b": 1.0})
    sharp = normalise({"a": 0.97, "b": 0.03})
    check(
        "confidence is low for a flat distribution",
        confidence_of(flat) < 0.25,
        f"{confidence_of(flat):.3f}",
    )
    check(
        "confidence is high for a concentrated distribution",
        confidence_of(sharp) > 0.75,
        f"{confidence_of(sharp):.3f}",
    )

    # --- hostile numeric input cannot produce a fake certainty
    dirty = normalise({"a": float("nan"), "b": -1.0, "c": 2.0})
    check("non-finite probabilities are dropped", set(dirty) == {"b", "c"}, str(dirty))
    check("negatives clamp to zero mass instead of inverting", dirty.get("b") == 0.0, str(dirty))
    check("normalisation still sums to 1", abs(sum(dirty.values()) - 1.0) < 1e-9, str(dirty))
    check("an empty distribution is empty, not certain", normalise({}) == {})

    # --- per-backend token caps are enforced as refusals
    class Tiny(Backend):
        name = "tiny"
        max_state_tokens = 16

    result = handle_evaluate(
        Tiny(),
        {
            "state": "word " * 200,
            "questions": {"q": {"type": "noul", "instructions": "Is it?"}},
        },
    )
    check(
        "a state over the backend cap is refused, not truncated",
        result["answers"] == {} and any("allows" in e for e in result["errors"]),
        str(result["errors"]),
    )

    # --- partial answers: one bad question does not sink the batch
    mixed = handle_evaluate(
        backend,
        {
            "state": "an outage is ongoing",
            "questions": {
                "good": {"type": "noul", "instructions": "Is there an outage?"},
                "bad": {"type": "score", "instructions": "rate", "criteria": ["only"]},
            },
        },
    )
    check("a batch keeps the answers it could produce", "good" in mixed["answers"], str(mixed))
    check("a batch reports the questions it refused", len(mixed["errors"]) == 1, str(mixed["errors"]))

    # --- the caller's option order is preserved end to end
    ordered_q = {
        "type": "choice",
        "instructions": "pick",
        "criteria": {"z": None, "a": None, "m": None},
    }
    ordered = answer_for("choice", ordered_q, backend.decide("choice", ordered_q, "z"))
    check(
        "option labels are used verbatim",
        set(ordered["probabilities"]) == {"z", "a", "m"},
        str(sorted(ordered["probabilities"])),
    )

    # --- hardening: the edges a long-running local server actually meets.
    check(
        "an over-cap body is refused with 413, not allocated",
        MAX_BODY_BYTES >= 4 * 1024 * 1024,
        str(MAX_BODY_BYTES),
    )
    check("loopback hosts are recognised", is_loopback_host("127.0.0.1") and is_loopback_host("localhost"))
    check(
        "a wildcard bind is not treated as loopback",
        not is_loopback_host("0.0.0.0"),
        "0.0.0.0 is every interface",
    )
    check(
        "a public bind is not treated as loopback",
        not is_loopback_host("10.0.0.5"),
    )
    try:
        import socket as _socket

        Handler.backend = MockBackend()
        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        port = server.server_address[1]
        # Announce an over-cap body and send only a little of it: the guard is on
        # the declared size, and this avoids moving 8 MiB to prove a header check.
        with _socket.create_connection(("127.0.0.1", port), timeout=10) as sock:
            head = (
                "POST /v1/systemone HTTP/1.1\r\n"
                "Host: 127.0.0.1\r\n"
                "content-type: application/json\r\n"
                f"content-length: {MAX_BODY_BYTES + 1}\r\n"
                "connection: close\r\n\r\n"
            ).encode()
            sock.sendall(head + b'{"state":')
            sock.settimeout(10)
            raw = b""
            # Bounded read: the server answers with 413 and closes.
            for _ in range(64):
                chunk = sock.recv(4096)
                if not chunk:
                    break
                raw += chunk
        text = raw.decode("utf-8", "replace")
        body = text.split("\r\n\r\n", 1)[-1]
        payload = json.loads(body) if body.strip().startswith("{") else {}
        server.shutdown()
        check(
            "an oversized request gets 413 with a reason",
            text.startswith("HTTP/1.1 413")
            and payload.get("detail", {}).get("error_type") == "request_too_large",
            text[:160],
        )
    except Exception as exc:  # pragma: no cover
        check("an oversized request gets 413 with a reason", False, f"{type(exc).__name__}: {exc}")

    # --- every adapter against a simulated upstream API.
    _selftest_backends(check)

    # --- the real server, exercised over loopback.
    #
    # `--selftest` and `--probe` both return before the serve path runs, so a bug
    # there (a lost `backend = build_backend(...)` line, say) stayed invisible
    # until `nur jev start` died. This closes that gap.
    try:
        Handler.backend = MockBackend()
        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        port = server.server_address[1]
        body = json.dumps(
            {
                "state": "our payments are down, an outage is ongoing",
                "questions": {
                    "q": {"type": "noul", "instructions": "Is there an outage?"},
                    "pick": {
                        "type": "choice",
                        "instructions": "Which team?",
                        "criteria": {"billing": "payments", "technical": "outages"},
                    },
                },
            }
        )
        conn = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        conn.request(
            "POST", "/v1/systemone", body, {"content-type": "application/json"}
        )
        response = conn.getresponse()
        payload = json.loads(response.read())
        conn.close()
        server.shutdown()
        check(
            "the server answers a real request",
            response.status == 200 and payload["answers"]["q"]["type"] == "noul",
            str(payload)[:200],
        )
        check(
            "a served choice stays inside the caller's options",
            payload["answers"]["pick"]["choice"] in ("billing", "technical"),
            str(payload["answers"]["pick"])[:200],
        )
        check("the served response reports the backend", ":mock" in payload["model"], payload["model"])
    except Exception as exc:  # pragma: no cover - reported as a failure
        check("the server answers a real request", False, f"{type(exc).__name__}: {exc}")

    print(f"\n{len(failures)} failure(s)" if failures else "\nall checks passed")
    return 1 if failures else 0


# --------------------------------------------------------------------------- #
# CLI
# --------------------------------------------------------------------------- #
def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Local System One bridge for nur (Jev contract)")
    parser.add_argument("--backend", choices=sorted(BACKENDS), default="mock")
    parser.add_argument("--port", type=int, default=8788)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--device", default="cpu", help="verdict backend device (cpu/cuda/mps)")
    parser.add_argument("--verdict-model", default=None, help="openJev/GLiClass model name or path")
    parser.add_argument("--nimble-dir", default="nimble-model", help="Bespoke-Nimble-9B directory")
    parser.add_argument("--laya-model", default="aac6fef/laya-multilingual-coreml-ane")
    parser.add_argument("--laya-max-tokens", type=int, default=None,
                        help="request-token capacity of the laya bundle (default 96, the ANE "
                             "bundle's total; use 1024 for aac6fef/laya-multilingual-coreml)")
    parser.add_argument("--selftest", action="store_true", help="check the mapping, no model needed")
    parser.add_argument("--probe", action="store_true", help="report usable backends and exit")
    parser.add_argument("--allow-unavailable", action="store_true",
                        help="serve even when the backend is not usable (it will refuse requests)")
    args = parser.parse_args(argv)

    if args.selftest:
        return selftest()

    if args.probe:
        info = describe_backends(args)
        print(json.dumps(info, indent=2))
        return 0 if any(v["available"] for v in info.values()) else 1

    backend = build_backend(args.backend, args)
    ok, why = backend.available()
    if not ok and not args.allow_unavailable:
        print(f"backend '{args.backend}' is not usable here: {why}", file=sys.stderr)
        print("hint: --probe lists what this machine can run", file=sys.stderr)
        return 2

    if not is_loopback_host(args.host):
        print(
            f"warning: binding {args.host} answers typed questions - including the state you "
            "send it - to anything that can reach that address. The bridge is designed for "
            "loopback; nur only treats a loopback endpoint as keyless.",
            file=sys.stderr,
        )

    Handler.backend = backend
    try:
        server = ThreadingHTTPServer((args.host, args.port), Handler)
    except OSError as exc:
        print(
            f"cannot bind {args.host}:{args.port}: {exc}\n"
            "another bridge is probably already there - check `nur jev status`, or pass "
            "--port with a free one",
            file=sys.stderr,
        )
        return 2
    endpoint = f"http://{args.host}:{args.port}/v1/systemone"
    print(
        json.dumps(
            {
                "bridge": "ok",
                "backend": backend.name,
                "note": backend.note,
                "endpoint": endpoint,
                "max_state_tokens": backend.max_state_tokens,
                "nur_config": f'[typesafe]\nbase_url = "{endpoint}"',
            },
            indent=2,
        ),
        flush=True,
    )
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

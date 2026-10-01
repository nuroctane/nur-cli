"""GLiNER2 (Fastino) adapter: open weights loaded in-process with the author's `gliner2` package.

Checkpoint: fastino/gliner2.5-base-v1, the README's recommended English checkpoint (read 2026-09-19).

GLiNER2 is a schema-conditioned classifier: text in, a named classification task with labels (optionally with
descriptions) in. It has no instruction field and no yes/no or ordinal primitive, so the mapping below was written
down BEFORE the run (docs/v1.2-additions.md, "GLiNER2 mapping") and is the same for every item:

  text    = "Question: <instructions>\n\n<state>"      (the question travels with the text, as in the author's
                                                         "Classification with Descriptions" examples it has no
                                                         other slot)
  labels  = {label: description}                       choice: the option keys and their rubric text
                                                         noul:   {"yes": criteria.true or "Yes",
                                                                  "no":  criteria.false or "No"}
                                                         score:  {"0": level 0 text, "1": level 1 text, ...}

Distribution (the calibration-relevant choice): GLiNER2's single-label classification ("auto" activation, its
default) is a SOFTMAX over the per-label logits and returns only the argmax and its probability. We read out the
whole of that same softmax through the documented parameters class_act="softmax", multi_label=True,
cls_threshold=0.0, which return every label with its softmax probability. Verified on the smoke item: the top
probability equals the default single-label confidence. So the distribution is the model's own, not a
normalization we chose; the sigmoid ("multi-label") scores are NOT used.

Local weights have no provider tariff: price is null here and estimated later by size class, never 0.
"""

from __future__ import annotations

import json
import time

from .base import DecisionResult


def gliner2_labels(task) -> dict:
    qtype, crit = task.question["type"], task.question.get("criteria")
    if qtype == "noul":
        crit = crit or {}
        return {"yes": crit.get("true") or "Yes", "no": crit.get("false") or "No"}
    if qtype == "score":
        return {str(i): str(d) for i, d in enumerate(crit)}
    return {k: (v or k) for k, v in crit.items()}


def gliner2_text(task) -> str:
    state = task.state if isinstance(task.state, str) else json.dumps(task.state, ensure_ascii=False)
    return f"Question: {task.question['instructions']}\n\n{state}"


class Gliner2LocalAdapter:
    name = "gliner2_local"
    cost_basis = "local_cpu_no_provider_tariff"

    def __init__(self, endpoint=None, model=None, key_env="", timeout_s=None,
                 price_input_per_m=None, price_output_per_m=None, threads=4, revision=None):
        self.path = endpoint
        self.model = model or "fastino/gliner2.5-base-v1"
        self.key_env = key_env
        self.price_input_per_m = price_input_per_m
        self.price_output_per_m = price_output_per_m
        self.threads = threads
        self.revision = revision
        self._ex = None

    def load(self):
        if self._ex is None:
            import torch
            from gliner2 import AutoExtractor

            torch.set_num_threads(self.threads)
            self._ex = AutoExtractor.from_pretrained(self.path)
        return self._ex

    def build_request(self, task) -> dict:
        return {"text": gliner2_text(task), "task": "decision", "labels": gliner2_labels(task),
                "class_act": "softmax", "multi_label": True, "cls_threshold": 0.0}

    def run(self, task) -> DecisionResult:
        res = DecisionResult(adapter=self.name, ok=False, probs_source="native", model=self.model)
        body = self.build_request(task)
        res.request_body = body
        try:
            ex = self.load()
        except Exception as e:  # noqa: BLE001
            res.error = f"load failed: {type(e).__name__}: {str(e)[:250]}"
            return res
        t0 = time.perf_counter()
        try:
            schema = ex.create_schema().classification(body["task"], body["labels"], multi_label=True,
                                                       cls_threshold=0.0, class_act="softmax")
            out = ex.extract(body["text"], schema, include_confidence=True)
        except Exception as e:  # noqa: BLE001
            res.latency_s = time.perf_counter() - t0
            res.error = f"{type(e).__name__}: {str(e)[:300]}"
            return res
        res.latency_s = time.perf_counter() - t0
        res.raw = {"response": out, "runtime": {"device": "cpu", "threads": self.threads, "revision": self.revision,
                                                "probability_origin": "native-softmax (single-label head, all labels)"}}
        try:
            items = out["decision"]
            probs = {str(d["label"]): float(d["confidence"]) for d in items}
            if set(probs) != set(body["labels"]):
                raise ValueError(f"labels returned {sorted(probs)} != asked {sorted(body['labels'])}")
        except (KeyError, TypeError, ValueError) as e:
            res.error = f"answer parse failed: {e}"
            return res
        res.probs = probs
        res.ok = True
        return res

    def reserve_estimate(self, task) -> float:
        return 0.0

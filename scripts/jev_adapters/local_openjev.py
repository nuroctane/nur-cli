"""In-process adapter for open-weights rebuilds with a local `decide()` call.

Written against the `typed_decisions.open_jev.OpenJev` interface published with
com-kotobalabs/open-jev-deberta-v3-large: one forward pass over a state plus a
list of typed questions, returning the model's own softmax. Nothing is
generated, so the distribution is native, not verbalized.

The author's API takes options as bare strings with no description field, so
the rubric is appended to the instruction text - the same rubric every other
adapter sends, never less of it. The encoder truncates the state (256 tokens
in this checkpoint); the adapter measures the tokenized length and reports
`state_truncated` in the runtime block instead of hiding it.

Local runs have no provider tariff. Price is reported as `null`, never as
zero, because compute is not free - only unbilled here.
"""

from __future__ import annotations

import json
import time

from .base import DecisionResult


class LocalOpenJevAdapter:
    name = "local_openjev"
    cost_basis = "local_cpu_no_provider_tariff"

    def __init__(self, endpoint=None, model=None, key_env="", timeout_s=None,
                 price_input_per_m=None, price_output_per_m=None,
                 device="cpu", threads=2, revision=None):
        self.path = endpoint  # local snapshot directory
        self.model = model or endpoint or ""
        self.key_env = key_env
        self.timeout_s = timeout_s
        self.price_input_per_m = price_input_per_m
        self.price_output_per_m = price_output_per_m
        self.device = device
        self.threads = threads
        self.revision = revision
        self._model = None

    def load(self):
        if self._model is None:
            import sys
            import torch
            from typed_decisions.open_jev import OpenJev

            torch.set_num_threads(self.threads)
            t0 = time.perf_counter()
            self._model = OpenJev.from_pretrained(self.path, device=self.device)
            self.load_s = time.perf_counter() - t0
            self.torch_version = torch.__version__
            self.python = sys.version.split()[0]
        return self._model

    def _labels_and_rubric(self, task):
        qtype = task.question["type"]
        crit = task.question.get("criteria")
        if qtype == "noul":
            crit = crit or {}
            return ["no", "yes"], {"no": crit.get("false", "No"),
                                   "yes": crit.get("true", "Yes")}
        if qtype == "score":
            labels = [str(i) for i in range(len(crit))]
            return labels, dict(zip(labels, crit))
        labels = list(crit)
        return labels, {k: (v or k) for k, v in crit.items()}

    def build_request(self, task) -> dict:
        labels, rubric = self._labels_and_rubric(task)
        instructions = (task.question["instructions"]
                        + "\nAllowed answers and rubric: "
                        + json.dumps(rubric, ensure_ascii=False))
        state = task.state if isinstance(task.state, str) else json.dumps(
            task.state, ensure_ascii=False)
        q = {"type": task.question["type"], "instructions": instructions,
             "options": labels}
        return {"state": state, "questions": [q], "labels": labels}

    def run(self, task) -> DecisionResult:
        res = DecisionResult(adapter=self.name, ok=False, probs_source="native",
                             model=self.model)
        body = self.build_request(task)
        res.request_body = {k: v for k, v in body.items() if k != "labels"}
        try:
            model = self.load()
        except Exception as e:  # noqa: BLE001 - a failed load is a failed attempt
            res.error = f"load failed: {type(e).__name__}: {str(e)[:250]}"
            return res
        max_state = getattr(model.collator, "max_state", None)
        n_state = len(model.tok(body["state"], add_special_tokens=False)["input_ids"])
        t0 = time.perf_counter()
        try:
            answers = model.decide(body["state"], body["questions"])
        except Exception as e:  # noqa: BLE001
            res.latency_s = time.perf_counter() - t0
            res.error = f"{type(e).__name__}: {str(e)[:300]}"
            return res
        res.latency_s = time.perf_counter() - t0
        ans = answers[0] if answers else {}
        res.raw = {"answers": answers, "runtime": {
            "device": self.device, "threads": self.threads,
            "revision": self.revision, "state_tokens": n_state,
            "max_state_tokens": max_state,
            "state_truncated": bool(max_state and n_state > max_state),
            "probability_origin": "native-softmax",
            "torch": getattr(self, "torch_version", None)}}
        try:
            if task.question["type"] == "noul":
                p = float(ans["noul"])
                if not (0.0 <= p <= 1.0):
                    raise ValueError(f"noul out of range: {p}")
                res.probs = {"yes": p, "no": 1.0 - p}
            else:
                probs = ans["probabilities"]
                if not isinstance(probs, dict):
                    raise ValueError("missing probabilities")
                res.probs = {k: float(v) for k, v in probs.items()}
        except (KeyError, TypeError, ValueError) as e:
            res.error = f"answer parse failed: {e}"
            return res
        res.ok = True
        return res

    def reserve_estimate(self, task) -> float:
        return 0.0

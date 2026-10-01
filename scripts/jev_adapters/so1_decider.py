"""In-process adapter for open-alternative-jev (IkerMoel, import name `so1`).

Loads the model exactly like the author's Hugging Face Space (`demo/app.py`):
`HFBackend.from_pretrained("Qwen/Qwen3.5-4B", dtype=bfloat16, device_map="cuda",
batch_size=1)`, `Decider(backend)`, temperature 1.0, one `Choice` per decision,
`mode="separate"` (the author's recommendation for small models). The mapping is
byte-for-byte the one our v1.1 `gradio_space` adapter sent to that Space, so the
v1.1 attempt and this run ask the same questions:
  * noul  -> options "no", "yes"; rubric {"no": false-text, "yes": true-text}
  * score -> options "0".."k-1"; rubric maps each to its level text
  * choice-> the option keys; rubric maps each key to its description
  question = instructions + "\\nRubric: " + JSON(rubric)
Distribution: the library's own softmax over the option letters (native).
"""

from __future__ import annotations

import json
import time

from .base import DecisionResult
from .gradio_space import GradioSpaceAdapter


class So1DeciderAdapter:
    name = "so1_decider"
    cost_basis = "self_hosted_gpu"

    def __init__(self, endpoint=None, model=None, key_env="", timeout_s=None,
                 price_input_per_m=None, price_output_per_m=None, revision=None,
                 mode="separate", temperature=1.0):
        self.endpoint = endpoint or "Qwen/Qwen3.5-4B"
        self.model = model or self.endpoint
        self.revision = revision
        self.mode = mode
        self.temperature = temperature
        self.price_input_per_m = price_input_per_m
        self.price_output_per_m = price_output_per_m
        self._decider = None
        self._space = GradioSpaceAdapter(endpoint="local", mode=mode, temperature=temperature)

    def load(self):
        if self._decider is None:
            import torch
            from so1 import Decider
            from so1.backends.hf import HFBackend
            kw = dict(dtype=torch.bfloat16, device_map="cuda", batch_size=1)
            if self.revision:
                kw["revision"] = self.revision
            backend = HFBackend.from_pretrained(self.endpoint, **kw)
            self._decider = Decider(backend)
            self._decider.temperature = float(self.temperature)
        return self._decider

    def run(self, task) -> DecisionResult:
        from so1 import Choice
        res = DecisionResult(adapter=self.name, ok=False, probs_source="native", model=self.model)
        body = self._space.build_request(task)
        labels, question = body["labels"], body["fields"][0]
        if (getattr(self, "request_options", None) or {}).get("noul_order") == "author_yes_no" \
                and task.question["type"] == "noul":
            # Post-hoc variant (added after the frozen run): the author's own
            # `so1.yes_no()` option order, "yes" before "no". Rubric unchanged.
            labels = ["yes", "no"]
        res.request_body = {"question": question, "options": labels, "mode": self.mode}
        try:
            decider = self.load()
        except Exception as e:  # noqa: BLE001
            res.error = f"load failed: {type(e).__name__}: {str(e)[:250]}"
            return res
        t0 = time.perf_counter()
        try:
            d = decider.decide(body["state"].strip() or None, [Choice(question, labels, name="q1")],
                               mode=self.mode)[0]
        except Exception as e:  # noqa: BLE001
            res.latency_s = time.perf_counter() - t0
            res.error = f"{type(e).__name__}: {str(e)[:300]}"
            return res
        res.latency_s = time.perf_counter() - t0
        probs = {lab: float(p) for lab, p in zip(labels, d.probabilities)}
        res.raw = {"answer": {"options": labels, "probabilities": list(map(float, d.probabilities))},
                   "runtime": {"mode": self.mode, "temperature": self.temperature,
                               "probability_origin": "native-option-letter-softmax"}}
        res.probs = {"yes": probs["yes"], "no": probs["no"]} if task.question["type"] == "noul" else probs
        res.ok = True
        return res

    def reserve_estimate(self, task) -> float:
        return 0.0

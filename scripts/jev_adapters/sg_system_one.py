"""In-process adapter for Sean Goedecke's `system-one` (github.com/sgoedecke/system-one).

The library takes TypeSafe-SDK `Choice` questions directly and reads one constrained
next-token distribution over option indexes (no generation). Loaded like the author's
demos: `SystemOne.from_pretrained("Qwen/Qwen3-8B", model_kwargs={"torch_dtype": bfloat16,
"device_map": "cuda"})`, one question per call.

Only Choice is native. Mappings for the other primitives, fixed before any run:
  * noul  -> Choice(criteria={"yes": <true text or "Yes">, "no": <false text or "No">})
             ("yes" first, i.e. index 0 = the proposition holds, as in Jev's noul = P(true))
  * score -> Choice(criteria={"0": level0, "1": level1, ...})
  * choice-> Choice(instructions, criteria) unchanged (a None description stays None)
"""

from __future__ import annotations

import json
import time

from .base import DecisionResult


class SgSystemOneAdapter:
    name = "sg_system_one"
    cost_basis = "self_hosted_gpu"

    def __init__(self, endpoint=None, model=None, key_env="", timeout_s=None,
                 price_input_per_m=None, price_output_per_m=None, revision=None):
        self.endpoint = endpoint or "Qwen/Qwen3-8B"
        self.model = model or self.endpoint
        self.revision = revision
        self.price_input_per_m = price_input_per_m
        self.price_output_per_m = price_output_per_m
        self._engine = None

    def load(self):
        if self._engine is None:
            import torch
            from system_one import SystemOne
            self._engine = SystemOne.from_pretrained(
                self.endpoint, revision=self.revision,
                model_kwargs={"torch_dtype": torch.bfloat16, "device_map": "cuda"})
        return self._engine

    @staticmethod
    def criteria(task) -> dict:
        qtype, crit = task.question["type"], task.question.get("criteria")
        if qtype == "noul":
            crit = crit or {}
            return {"yes": crit.get("true") or "Yes", "no": crit.get("false") or "No"}
        if qtype == "score":
            return {str(i): lvl for i, lvl in enumerate(crit)}
        return dict(crit)

    def run(self, task) -> DecisionResult:
        from typesafe_sdk import Choice
        res = DecisionResult(adapter=self.name, ok=False, probs_source="native", model=self.model)
        crit = self.criteria(task)
        res.request_body = {"instructions": task.question["instructions"], "criteria": crit}
        try:
            engine = self.load()
        except Exception as e:  # noqa: BLE001
            res.error = f"load failed: {type(e).__name__}: {str(e)[:250]}"
            return res
        state = task.state if isinstance(task.state, str) else json.loads(json.dumps(task.state))
        t0 = time.perf_counter()
        try:
            out = engine.system_one(state, {"decision": Choice(instructions=task.question["instructions"],
                                                               criteria=crit)})
        except Exception as e:  # noqa: BLE001
            res.latency_s = time.perf_counter() - t0
            res.error = f"{type(e).__name__}: {str(e)[:300]}"
            return res
        res.latency_s = time.perf_counter() - t0
        ans = out.answers["decision"]
        probs = {k: float(v) for k, v in ans.probabilities.items()}
        res.usage = {"input_tokens": out.usage.input_tokens, "output_tokens": out.usage.output_tokens}
        res.raw = {"answer": {"choice": ans.choice, "probabilities": probs},
                   "runtime": {"probability_origin": "native-constrained-index-softmax"}}
        res.probs = probs
        res.ok = True
        return res

    def reserve_estimate(self, task) -> float:
        return 0.0

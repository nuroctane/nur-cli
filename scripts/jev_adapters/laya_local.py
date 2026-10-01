"""Laya (Convai Innovations) adapter: open weights loaded in-process with the author's `laya` package.

Interface (https://huggingface.co/convaiinnovations/laya, `pip install laya`, read 2026-09-19):
  agent = laya.load(<checkpoint>)            # English checkpoint = repo root (ModernBERT-large, 421M)
  agent.predict(state, {"decision": {type, instructions, criteria}})
It takes Jev's typed questions unchanged and answers in Jev's shape, so every JevBench record maps 1:1:
  noul   answer["noul"] = P(true)            -> {"yes": p, "no": 1-p}
  choice answer["probabilities"]            -> used as-is (keys = options)
  score  answer["probabilities"]            -> used as-is (keys = level indices)
Probabilities are the model's own softmax over option markers (native). The checkpoint's budget is 512 tokens
per question (its documented limit); longer states are truncated by the package itself, as its users would see.

Local weights have no provider tariff: price is null here and estimated later by size class, never 0.
"""

from __future__ import annotations

import time

from .base import DecisionResult, build_question


class LayaLocalAdapter:
    name = "laya_local"
    cost_basis = "local_cpu_no_provider_tariff"

    def __init__(self, endpoint=None, model=None, key_env="", timeout_s=None,
                 price_input_per_m=None, price_output_per_m=None, threads=4, revision=None):
        self.path = endpoint  # local snapshot of convaiinnovations/laya
        self.model = model or "convaiinnovations/laya"
        self.key_env = key_env
        self.price_input_per_m = price_input_per_m
        self.price_output_per_m = price_output_per_m
        self.threads = threads
        self.revision = revision
        self._agent = None

    def load(self):
        if self._agent is None:
            import torch
            import laya

            torch.set_num_threads(self.threads)
            self._agent = laya.load(self.path)
            self.laya_version = getattr(laya, "__version__", None)
        return self._agent

    def build_request(self, task) -> dict:
        return {"state": task.state, "questions": {"decision": build_question(task)}}

    def run(self, task) -> DecisionResult:
        res = DecisionResult(adapter=self.name, ok=False, probs_source="native", model=self.model)
        body = self.build_request(task)
        res.request_body = body
        try:
            agent = self.load()
        except Exception as e:  # noqa: BLE001 - a failed load is a failed attempt
            res.error = f"load failed: {type(e).__name__}: {str(e)[:250]}"
            return res
        t0 = time.perf_counter()
        try:
            out = agent.predict(body["state"], body["questions"])
        except Exception as e:  # noqa: BLE001
            res.latency_s = time.perf_counter() - t0
            res.error = f"{type(e).__name__}: {str(e)[:300]}"
            return res
        res.latency_s = time.perf_counter() - t0
        res.raw = {"response": out, "runtime": {"device": "cpu", "threads": self.threads, "revision": self.revision,
                                                "laya": getattr(self, "laya_version", None),
                                                "probability_origin": "native-softmax"}}
        res.usage = dict((out or {}).get("usage") or {})
        ans = ((out or {}).get("answers") or {}).get("decision")
        try:
            if not isinstance(ans, dict) or ans.get("type") != task.question["type"]:
                raise ValueError("missing or mistyped answers.decision")
            if task.question["type"] == "noul":
                p = float(ans["noul"])
                if not (0.0 <= p <= 1.0):
                    raise ValueError(f"noul out of range: {p}")
                res.probs = {"yes": p, "no": 1.0 - p}
            else:
                probs = ans["probabilities"]
                if not isinstance(probs, dict):
                    raise ValueError("missing probabilities")
                res.probs = {str(k): float(v) for k, v in probs.items()}
        except (KeyError, TypeError, ValueError) as e:
            res.error = f"answer parse failed: {e}"
            return res
        res.ok = True
        return res

    def reserve_estimate(self, task) -> float:
        return 0.0

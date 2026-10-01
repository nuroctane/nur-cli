"""Local adapter for AltSlate Labs' public Certo v1 checkpoint.

Certo accepts one state string and runtime option descriptions. JevBench's
question instruction is prepended to the state because Certo has no separate
instruction field. Option descriptions use the supplied rubric verbatim. The
checkpoint's own 64-token state and 48-token option limits remain unchanged.
"""
from __future__ import annotations

import time

from .base import DecisionResult


class CertoLocalAdapter:
    name = "certo_local"
    cost_basis = "self_hosted_gpu"

    def __init__(self, endpoint=None, model=None, key_env="", timeout_s=None,
                 price_input_per_m=None, price_output_per_m=None, revision=None):
        self.endpoint = endpoint or "altslate/certo-decision-model"
        self.model = model or "altslate/certo-decision-model"
        self.revision = revision
        self.price_input_per_m = price_input_per_m
        self.price_output_per_m = price_output_per_m
        self._loaded = None

    def load(self):
        if self._loaded is None:
            from infer import DecisionModel
            import torch
            device = "cuda" if torch.cuda.is_available() else "cpu"
            started = time.perf_counter()
            self._loaded = DecisionModel.load(self.endpoint, device=device)
            self.load_s = time.perf_counter() - started
        return self._loaded

    @staticmethod
    def build_request(task):
        state = task.state if isinstance(task.state, str) else __import__("json").dumps(
            task.state, ensure_ascii=False, sort_keys=True)
        state = task.question["instructions"] + "\n\n" + state
        criteria = task.question.get("criteria")
        if task.question["type"] == "noul":
            criteria = criteria or {}
            options = [
                {"id": "no", "description": criteria.get("false", "The proposition is false.")},
                {"id": "yes", "description": criteria.get("true", "The proposition is true.")},
            ]
        elif task.question["type"] == "choice":
            options = [{"id": label, "description": (criteria or {}).get(label) or label}
                       for label in task.labels]
        else:
            options = [{"id": label, "description": criteria[int(label)]}
                       for label in task.labels]
        return {"state": state, "options": options}

    def run(self, task):
        result = DecisionResult(adapter=self.name, ok=False, probs_source="native", model=self.model)
        row = self.build_request(task)
        result.request_body = {"state_sha256": __import__("hashlib").sha256(
            row["state"].encode()).hexdigest(), "options": row["options"]}
        try:
            decision_model = self.load()
            started = time.perf_counter()
            output = decision_model.decide(row["state"], row["options"])
            result.latency_s = time.perf_counter() - started
            # Count the tokens actually retained by the checkpoint's fixed limits.
            se = decision_model.tok(row["state"], truncation=True, max_length=64)
            oe = decision_model.tok([o["description"] for o in row["options"]],
                                    truncation=True, max_length=48)
            result.usage = {"input_tokens": len(se["input_ids"]) + sum(map(len, oe["input_ids"])),
                            "output_tokens": 0}
            result.probs = output["probs"]
            result.raw = {"answer": output, "runtime": {
                "checkpoint": self.model, "revision": self.revision,
                "state_max_tokens": 64, "option_max_tokens": 48,
                "probability_origin": "native-softmax",
            }}
            result.ok = True
        except Exception as exc:  # noqa: BLE001
            result.error = f"{type(exc).__name__}: {str(exc)[:300]}"
        return result

    def reserve_estimate(self, task):
        return 0.0

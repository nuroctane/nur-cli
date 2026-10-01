"""In-process adapter for SemIf (formerly OpenJev, TheoLeeCJ): direct option-letter logits.

Uses the author's own code path (`semif_phase1.core.load_causal_model` and
`semif_phase1.direct.score`) on the author's pinned model, Qwen/Qwen3.5-4B in
BF16 on one CUDA GPU. One forward pass per decision, no generation; the
distribution is the author's softmax over the declared answer-letter logits.

Mapping from the canonical record follows the author's own conversion of
TypeSafe records (`benchmarks/build_typesafe.py`):
  * noul   -> options "true"/"false" (in that order), description = criterion text
              or "The proposition is <id>."; each description prefixed "<id>: "
  * choice -> one option per criteria key, description "<key>: <description>"
  * score  -> the author excludes Score. We extend the same rule: options "0".."k-1",
              description "<i>: <level text>". Marked as our extension in the results.
The endpoint argument is the model id; `revision` pins the commit.
"""

from __future__ import annotations

import time

from .base import DecisionResult


class SemIfDirectAdapter:
    name = "semif_direct"
    cost_basis = "self_hosted_gpu"

    def __init__(self, endpoint=None, model=None, key_env="", timeout_s=None,
                 price_input_per_m=None, price_output_per_m=None, revision=None,
                 max_tokens=4096):
        self.endpoint = endpoint or "Qwen/Qwen3.5-4B"
        self.model = model or self.endpoint
        self.revision = revision
        self.max_tokens = max_tokens
        self.price_input_per_m = price_input_per_m
        self.price_output_per_m = price_output_per_m
        self._loaded = None

    def load(self):
        if self._loaded is None:
            from semif_phase1.core import load_causal_model
            t0 = time.perf_counter()
            self._loaded = load_causal_model(self.endpoint, self.revision)
            self.load_s = time.perf_counter() - t0
        return self._loaded

    def build_request(self, task) -> dict:
        qtype = task.question["type"]
        crit = task.question.get("criteria")
        if qtype == "noul":
            options = [{"id": k, "description": (crit or {}).get(k, f"The proposition is {k}.")}
                       for k in ("true", "false")]
        elif qtype == "choice":
            options = [{"id": k, "description": v or k} for k, v in crit.items()]
        else:
            options = [{"id": str(i), "description": lvl} for i, lvl in enumerate(crit)]
        for o in options:
            o["description"] = o["id"] + ": " + o["description"]
        return {"id": task.id, "state": task.state, "question": task.question["instructions"],
                "options": options}

    def run(self, task) -> DecisionResult:
        res = DecisionResult(adapter=self.name, ok=False, probs_source="native", model=self.model)
        row = self.build_request(task)
        res.request_body = {k: v for k, v in row.items() if k != "state"}
        try:
            model, tok, meta = self.load()
        except Exception as e:  # noqa: BLE001
            res.error = f"load failed: {type(e).__name__}: {str(e)[:250]}"
            return res
        from semif_phase1.direct import score
        t0 = time.perf_counter()
        try:
            out = score(model, tok, row, meta, self.max_tokens)
        except Exception as e:  # noqa: BLE001 - e.g. over the author's token limit
            res.latency_s = time.perf_counter() - t0
            res.error = f"{type(e).__name__}: {str(e)[:300]}"
            return res
        res.latency_s = time.perf_counter() - t0
        probs = dict(zip(out["option_ids"], out["probabilities"]))
        res.usage = {"input_tokens": out["input_tokens"], "output_tokens": 0}
        res.raw = {"answer": {k: out[k] for k in ("option_ids", "probabilities", "option_logits",
                                                   "input_tokens", "forward_seconds", "prompt_sha256",
                                                   "prompt_version")},
                   "runtime": {"model": out["model"], "readout": out["readout"],
                               "probability_origin": "native-option-logit-softmax"}}
        if task.question["type"] == "noul":
            res.probs = {"yes": probs["true"], "no": probs["false"]}
        else:
            res.probs = probs
        res.ok = True
        return res

    def reserve_estimate(self, task) -> float:
        return 0.0

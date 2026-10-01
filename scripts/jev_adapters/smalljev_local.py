"""In-process adapter for smalljev semantic-v9's public weights.

The mapping is frozen in docs/v1.2-additions-smalljev.md. Imports of the entrant
package and ML dependencies are lazy so the normal JevBench test environment
does not execute foreign code.
"""
from __future__ import annotations

import json
import os
import time

from .base import DecisionResult


class SmallJevLocalAdapter:
    name = "smalljev_local"
    cost_basis = "self_hosted_gpu_estimated_input_tokens"

    def __init__(self, endpoint=None, model=None, key_env="", timeout_s=None,
                 price_input_per_m=None, price_output_per_m=None, revision=None,
                 source_path=None, adapter_dir=None, scorer_ckpt=None,
                 noulscore_ckpt=None):
        self.endpoint = endpoint or "openbmb/MiniCPM5-2B-Base"
        self.model = model or "smalljev-semantic-v9"
        self.revision = revision
        self.price_input_per_m = price_input_per_m
        self.price_output_per_m = price_output_per_m
        self.source_path = source_path or os.environ.get("SMALLJEV_SOURCE")
        self.adapter_dir = adapter_dir or os.environ.get("SMALLJEV_ADAPTER")
        self.scorer_ckpt = scorer_ckpt or os.environ.get("SMALLJEV_SCORER")
        self.noulscore_ckpt = noulscore_ckpt or os.environ.get("SMALLJEV_NOULSCORE")
        self._loaded = None

    @staticmethod
    def build_request(task):
        state = task.state if isinstance(task.state, str) else json.dumps(
            task.state, ensure_ascii=False, sort_keys=True)
        criteria = task.question.get("criteria") or {}
        if task.question["type"] == "noul":
            labels = ["no", "yes"]
            rubric = {"no": criteria.get("false", "No"),
                      "yes": criteria.get("true", "Yes")}
            options = labels
        elif task.question["type"] == "score":
            labels = list(task.labels)
            rubric = {label: criteria[int(label)] for label in labels}
            options = [rubric[label] for label in labels]
        else:
            labels = list(task.labels)
            rubric = {label: criteria.get(label) or label for label in labels}
            options = labels
        question = (task.question["instructions"] +
                    "\nAllowed answers and rubric: " +
                    json.dumps(rubric, ensure_ascii=False))
        if task.question["type"] == "noul":
            question += "\nAnswer with yes or no."
        return {"state": state, "question": question, "labels": labels,
                "options": options, "qtype": task.question["type"],
                "rubric": rubric}

    def load(self):
        if self._loaded is not None:
            return self._loaded
        if not all((self.source_path, self.adapter_dir, self.scorer_ckpt,
                    self.noulscore_ckpt)):
            raise ValueError("smalljev source and all three checkpoint paths are required")
        import sys
        sys.path.insert(0, self.source_path)
        import torch
        from peft import PeftModel
        from transformers import AutoModelForCausalLM, AutoTokenizer
        from smalljev.heads import OptionScorerHead
        started = time.perf_counter()
        tokenizer = AutoTokenizer.from_pretrained(self.endpoint,
                                                  trust_remote_code=False)
        if tokenizer.pad_token is None:
            tokenizer.pad_token = tokenizer.eos_token
        base = AutoModelForCausalLM.from_pretrained(
            self.endpoint, torch_dtype=torch.bfloat16, device_map="auto",
            trust_remote_code=False, attn_implementation="eager").eval()
        model = PeftModel.from_pretrained(base, self.adapter_dir).eval()
        scorer = OptionScorerHead.load(self.scorer_ckpt).cuda().eval()
        # Load and validate the published auxiliary heads even though v9 routes
        # all three JevBench primitives through the semantic scorer.
        torch.load(self.noulscore_ckpt, map_location="cpu", weights_only=True)
        self.load_s = time.perf_counter() - started
        self._loaded = tokenizer, model, scorer, torch
        return self._loaded

    def _probabilities(self, state, question, options):
        tokenizer, model, scorer, torch = self.load()
        from smalljev.semantic import build_semantic_ids
        ids, spans = build_semantic_ids(tokenizer, state, question, options,
                                        max_len=2560)
        input_ids = torch.tensor([ids], device=next(model.parameters()).device)
        with torch.no_grad():
            hidden = model(input_ids=input_ids, use_cache=False,
                           output_hidden_states=True).hidden_states[-1][0].float()
            reps = torch.stack([hidden[a:b].mean(0) for a, b in spans])
            probs = scorer.probs(reps.unsqueeze(0)).float().cpu().numpy()[0]
        return probs / probs.sum(), len(ids)

    def run(self, task):
        result = DecisionResult(adapter=self.name, ok=False, probs_source="native",
                                model=self.model)
        request = self.build_request(task)
        result.request_body = request
        started = time.perf_counter()
        try:
            probs, input_tokens = self._probabilities(
                request["state"], request["question"], request["options"])
            result.latency_s = time.perf_counter() - started
            result.probs = {label: float(probs[i])
                            for i, label in enumerate(request["labels"])}
            if len(probs) != len(request["labels"]):
                raise ValueError("probability count does not match labels")
            if not (0.999 <= sum(result.probs.values()) <= 1.001):
                raise ValueError("probabilities do not sum to one")
            result.usage = {"input_tokens": input_tokens, "output_tokens": 0}
            result.raw = {"runtime": {"checkpoint": self.model,
                "revision": self.revision, "generated_tokens": 0,
                "max_input_tokens": 2560,
                "probability_origin": "native-option-span-softmax",
                "load_s": self.load_s}}
            result.ok = True
        except Exception as exc:  # noqa: BLE001
            result.latency_s = time.perf_counter() - started
            result.error = f"{type(exc).__name__}: {str(exc)[:300]}"
        return result

    def reserve_estimate(self, task):
        return 0.0

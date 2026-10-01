"""Adapter for rebuilds whose only public interface is a Gradio demo.

Some authors publish weights plus a Hugging Face Space and no HTTP decision
API. The Space still exposes the model's *native* distribution (a softmax over
the option tokens), so the numbers stay native - only the transport is the
demo's own form: one free-text question plus its options as a comma-separated
list, repeated for up to eight question slots.

Mapping rules, all lossless with respect to the canonical record:
  * noul  -> options "no, yes", rubric appended to the instruction
  * score -> options "0, 1, ..", level descriptions appended to the instruction
  * choice-> the option keys, descriptions appended to the instruction
A label containing a comma or newline cannot survive the demo's delimiter and
is refused rather than silently rewritten.

Requires `gradio_client`. Pass `--key-env HF_TOKEN` to use the larger quota the
Space grants a signed-in Hugging Face account; without it the caller is
anonymous and gets a handful of runs. Latency is measured end to end by the
runner, so the queue wait of a shared Space is included - which is the honest number for a
free public demo and is reported as such.
"""

from __future__ import annotations

import json
import re
import time

from .base import DecisionResult

LIMIT_PATTERN = re.compile(
    r"quota|rate.?limit|too many|\b429\b|\b403\b|access.denied|sign.?in", re.I)


class GradioSpaceAdapter:
    name = "gradio_space"

    def __init__(self, endpoint, model=None, key_env="", timeout_s=180.0,
                 price_input_per_m=None, price_output_per_m=None,
                 api_name="/decide", mode="separate", temperature=1.0,
                 question_slots=8):
        self.endpoint = endpoint
        self.model = model or endpoint
        self.key_env = key_env
        self.timeout_s = timeout_s
        self.price_input_per_m = price_input_per_m
        self.price_output_per_m = price_output_per_m
        self.api_name = api_name
        self.mode = mode
        self.temperature = temperature
        self.question_slots = question_slots
        self.blocked = False
        self._client = None

    def client(self):
        if self._client is None:
            import os

            from gradio_client import Client

            # A free Space gives an anonymous caller a handful of GPU runs and
            # then says, in its own words, "authenticate with a Hugging Face
            # token for more quota". Signing in as ourselves is the quota the
            # service offers; it is not a way around the limit, and the token
            # goes to huggingface.co, never into a request body or a log.
            token = os.environ.get(self.key_env) if self.key_env else None
            self._client = Client(self.endpoint, verbose=False,
                                  **({"token": token} if token else {}))
        return self._client

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
        for lab in labels:
            if "," in lab or "\n" in lab:
                raise ValueError(f"label {lab!r} collides with the demo's comma delimiter")
        state = task.state if isinstance(task.state, str) else json.dumps(
            task.state, ensure_ascii=False)
        question = (task.question["instructions"]
                    + "\nRubric: " + json.dumps(rubric, ensure_ascii=False))
        fields = [question, ", ".join(labels)] + [""] * (2 * self.question_slots - 2)
        return {"state": state, "mode": self.mode,
                "temperature": self.temperature, "fields": fields,
                "labels": labels}

    def run(self, task) -> DecisionResult:
        res = DecisionResult(adapter=self.name, ok=False, probs_source="native",
                             model=self.model)
        if self.blocked:
            res.error = "public demo halted after a rate/access-limit response"
            res.status = 429
            return res
        try:
            body = self.build_request(task)
        except ValueError as e:
            res.error = str(e)
            return res
        res.request_body = {k: v for k, v in body.items() if k != "labels"}
        t0 = time.perf_counter()
        try:
            job = self.client().submit(
                body["state"], body["mode"], body["temperature"], *body["fields"],
                api_name=self.api_name)
            raw = job.result(timeout=self.timeout_s)
        except Exception as e:  # noqa: BLE001 - transport errors are data here
            res.latency_s = time.perf_counter() - t0
            res.error = f"{type(e).__name__}: {str(e)[:300]}"
            if LIMIT_PATTERN.search(str(e)):
                self.blocked = True
                res.status = 429
            return res
        res.latency_s = time.perf_counter() - t0
        res.raw = {"note": str(raw[1]) if len(raw) > 1 else None,
                   "answers": raw[2] if len(raw) > 2 else None}
        answers = res.raw["answers"]
        if not isinstance(answers, list) or len(answers) != 1:
            res.error = "expected exactly one answer"
            return res
        probs = (answers[0] or {}).get("probabilities")
        if not isinstance(probs, dict):
            res.error = "demo returned no probability map"
            return res
        if set(probs) != set(body["labels"]):
            res.error = "demo answered over different labels than supplied"
            return res
        if task.question["type"] == "noul":
            res.probs = {"yes": probs["yes"], "no": probs["no"]}
        else:
            res.probs = probs
        res.ok = True
        return res

    def reserve_estimate(self, task) -> float:
        if self.price_input_per_m is None or self.price_output_per_m is None:
            return None
        return (100_000 * self.price_input_per_m / 1e6
                + 4_000 * self.price_output_per_m / 1e6)

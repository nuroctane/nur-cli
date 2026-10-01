"""Cactus Needle 3, in-process on CPU, through its own tool-call interface.

Needle is a span-grounded function-calling / extraction model. It does not
return a probability distribution over a label set: it returns one tool call
(or a refusal) plus a single confidence scalar for the whole call. So this
adapter asks the question the way Needle is built to be asked - one tool,
`record_decision`, whose one argument is typed as the exact label enum
(boolean for yes/no, integer enum for ordinal levels) - and reports the label
only. `probs` stays None and `probs_source` says why: we never turn a single
confidence into a distribution, so Brier and ECE are not computed for Needle.

Two modes, both reported, never merged:
  "record_decision" (default, frozen before the v1.1 run): one tool whose one
      argument is the typed label.
  "options_as_tools" (request_options {"choice_mode": "tools"}; added after the
      easy-tier run showed Needle declining record_decision for requests like
      "Where is my package?" because no tool could *serve* the request): for
      choice questions every option becomes its own tool, named by the label
      and described by its criterion, and the called tool is the answer. This
      is how Needle is meant to be used for tool selection. Yes/no and ordinal
      questions are unchanged in this mode.

A suppressed call (Needle's "the request does not fit the tool" refusal) is an
abstention and scores as wrong. What the suppressed call would have said is kept
in the raw record for inspection, not scored.
"""

from __future__ import annotations

import json
import os
import time

from .base import DecisionResult

NO_DISTRIBUTION = "label_only_no_calibrated_distribution"


class NeedleLocalAdapter:
    name = "needle_local"
    cost_basis = "local_cpu_no_provider_tariff"

    def __init__(self, endpoint=None, model=None, key_env="", price_input_per_m=None,
                 price_output_per_m=None, max_new_tokens=128, **_):
        os.environ.setdefault("NEEDLE_TELEMETRY", "0")  # no benchmark data leaves the box
        import needle  # imported late: only the Needle venv has it
        self._needle = needle
        self.version = getattr(needle, "__version__", "unknown")
        self.model = model or f"cactus-needle {self.version} (generation 3)"
        self.endpoint = endpoint or "in-process CPU"
        self.max_new_tokens = max_new_tokens
        self.price_input_per_m = price_input_per_m
        self.price_output_per_m = price_output_per_m

    def reserve_estimate(self, task):
        return 0.0

    @staticmethod
    def build_tool(task) -> dict:
        qtype = task.question["type"]
        crit = task.question.get("criteria")
        labels = [str(x) for x in task.labels]
        if qtype == "noul":
            legend = ""
            if isinstance(crit, dict):
                legend = f" true: {crit.get('true', '')}; false: {crit.get('false', '')}"
            param = {"type": "boolean",
                     "description": task.question["instructions"] + legend}
        elif qtype == "score":
            legend = "; ".join(f"{i}: {crit[i]}" for i in range(len(labels))) if isinstance(crit, list) else ""
            param = {"type": "integer", "enum": [int(x) for x in labels],
                     "description": task.question["instructions"] + " Levels: " + legend}
        else:
            legend = "; ".join(f"{lab}: {crit.get(lab)}" if isinstance(crit, dict) and crit.get(lab) else lab
                               for lab in labels)
            param = {"type": "string", "enum": labels,
                     "description": task.question["instructions"] + " Options: " + legend}
        return {"name": "record_decision",
                "description": "Record the answer to this question about the text: "
                               + task.question["instructions"],
                "parameters": {"type": "object", "properties": {"decision": param},
                               "required": ["decision"]}}

    @staticmethod
    def to_label(value, task):
        qtype = task.question["type"]
        if qtype == "noul":
            if isinstance(value, bool):
                return "yes" if value else "no"
            if isinstance(value, str) and value.lower() in ("true", "false", "yes", "no"):
                return "yes" if value.lower() in ("true", "yes") else "no"
            return None
        if qtype == "score":
            try:
                s = str(int(value))
            except (TypeError, ValueError):
                return None
            return s if s in task.labels else None
        return value if value in task.labels else None

    @staticmethod
    def build_option_tools(task) -> list:
        crit = task.question.get("criteria")
        return [{"name": str(lab),
                 "description": (crit.get(lab) if isinstance(crit, dict) and crit.get(lab) else str(lab)),
                 "parameters": {"type": "object", "properties": {}, "required": []}}
                for lab in task.labels]

    def run(self, task) -> DecisionResult:
        mode = (getattr(self, "request_options", None) or {}).get("choice_mode", "record_decision")
        as_tools = mode == "tools" and task.question["type"] == "choice"
        tool = self.build_tool(task)
        tools = self.build_option_tools(task) if as_tools else [tool]
        text = task.state if isinstance(task.state, str) else json.dumps(task.state, ensure_ascii=False)
        body = {"system": task.question["instructions"], "tools": tools, "mode": "options_as_tools" if as_tools else "record_decision"}
        res = DecisionResult(adapter=self.name, ok=False, probs=None, probs_source=NO_DISTRIBUTION,
                             model=self.model, request_body=body)
        agent = None
        t0 = time.perf_counter()
        try:
            agent = self._needle.Needle(tools=tools, system=task.question["instructions"])
            out = agent.complete(text, max_new_tokens=self.max_new_tokens)
        except Exception as e:  # engine error is an infrastructure failure, not an answer
            res.latency_s = time.perf_counter() - t0
            res.error = f"{type(e).__name__}: {str(e)[:200]}"
            return res
        finally:
            if agent is not None:
                agent.close()
        res.latency_s = time.perf_counter() - t0
        calls = out.get("function_calls") or []
        suppressed = out.get("suppressed_calls") or []
        def pick(c):
            if as_tools:
                return c.get("name")
            return (c.get("arguments") or {}).get("decision")
        value = pick(calls[0]) if calls else None
        label = self.to_label(value, task) if calls else None
        res.raw = {"response": out, "runtime": {
            "engine": f"cactus-needle {self.version}", "suppressed": bool(not calls and suppressed),
            "confidence_scalar": out.get("confidence"), "state_truncated": False,
            "mode": body["mode"], "n_calls": len(calls),
            "suppressed_label": self.to_label(pick(suppressed[0]), task)
            if (not calls and suppressed) else None}}
        res.usage = {k: out.get(k) for k in ("prefill_tps", "decode_tps", "peak_ram_mb") if k in out}
        res.ok = True  # the engine answered; an abstention or off-enum value is a wrong answer
        res.label = label
        if label is None:
            res.error = ("abstained (suppressed call)" if suppressed else "abstained (no call)") if not calls else f"value outside label set: {value!r}"
        return res

"""ProgramAsWeights (Yuntian Deng) adapter: one compiled neural program per rubric, run locally.

Interface (github.com/programasweights/programasweights-python 0.4.6, AGENTS.md, read 2026-09-19):
  program = paw.compile(spec)        # hosted compile: English spec -> LoRA "program" for a shared interpreter
  fn = paw.function(program.id)      # local llama.cpp runtime, Qwen3-0.6B (Q6_K) + the program's LoRA
  fn(input_text) -> str              # one text in, one text out
Default compiler (paw-4b-qwen3-0.6b), default n_ctx (2048: "spec + input + output share a ~2048 token context
window. Inputs that exceed it will error"), default threads, CPU (PAW_GPU_LAYERS=0, there is no GPU here).

The compile step (documented in docs/v1.2-additions.md): every distinct JevBench question (instructions + rubric)
is compiled ONCE, before its items run, from the spec paw_spec() builds below, following the author's spec guide
("State output constraints explicitly: Return ONLY one of: X, Y, Z"). No examples are added: no entrant gets
examples. The compile runs on PAW's hosted API (only the rubric text leaves our machine; the item state never
does) and is paced under the published anonymous quota (20 compiles/hour). Compile and program load are NOT
decision latency: the runner calls prepare() before its clock starts, and the time is kept as prepare_s.

The answer is a label (text), so this is a label-only system (no distribution, calibration 0). The output is
matched to the label set after trimming whitespace, quotes and a trailing full stop, case-insensitively;
anything else is "label outside the label set" and counts wrong.
"""

from __future__ import annotations

import hashlib
import json
import os
import time
from collections import OrderedDict

from .base import DecisionResult


def paw_labels(task) -> "OrderedDict[str, str]":
    qtype, crit = task.question["type"], task.question.get("criteria")
    if qtype == "noul":
        crit = crit or {}
        return OrderedDict([("yes", crit.get("true") or "Yes"), ("no", crit.get("false") or "No")])
    if qtype == "score":
        return OrderedDict((str(i), str(d)) for i, d in enumerate(crit))
    return OrderedDict((k, v or k) for k, v in crit.items())


def paw_spec(task) -> str:
    labels = paw_labels(task)
    lines = [task.question["instructions"].strip(), "",
             "Return ONLY one of: " + ", ".join(labels) + ".", ""]
    lines += [f"{k}: {v}" for k, v in labels.items()]
    return "\n".join(lines)


def spec_key(spec: str) -> str:
    return hashlib.sha256(spec.encode("utf-8")).hexdigest()


def match_label(text, labels):
    if not isinstance(text, str):
        return None
    t = text.strip().strip("`\"' ").rstrip(".").strip("`\"' ").lower()
    for lab in labels:
        if t == str(lab).lower():
            return str(lab)
    return None


class PawLocalAdapter:
    name = "paw_local"
    cost_basis = "local_cpu_no_provider_tariff"

    def __init__(self, endpoint=None, model=None, key_env="", timeout_s=None,
                 price_input_per_m=None, price_output_per_m=None, keep_loaded=3, revision=None):
        self.programs_path = endpoint  # JSON: spec sha256 -> program id (written by the compile step)
        self.model = model or "paw-4b-qwen3-0.6b"
        self.key_env = key_env
        self.price_input_per_m = price_input_per_m
        self.price_output_per_m = price_output_per_m
        self.keep_loaded = keep_loaded
        self.revision = revision
        self._fns = OrderedDict()
        self._programs = None

    def _program_id(self, task):
        if self._programs is None:
            with open(self.programs_path, encoding="utf-8") as fh:
                self._programs = json.load(fh)
        entry = self._programs.get(spec_key(paw_spec(task)))
        return entry["id"] if isinstance(entry, dict) else entry

    def prepare(self, task):
        """Load (never compile) the task's program; called by the runner before the clock starts."""
        import programasweights as paw

        pid = self._program_id(task)
        if pid is None or pid in self._fns:
            if pid in self._fns:
                self._fns.move_to_end(pid)
            return
        while len(self._fns) >= self.keep_loaded:
            _, old = self._fns.popitem(last=False)
            close = getattr(old, "close", None)
            if close:
                close()
        self._fns[pid] = paw.function(pid, offline=os.environ.get("PAW_OFFLINE") == "1")

    def run(self, task) -> DecisionResult:
        res = DecisionResult(adapter=self.name, ok=False, probs_source="label_only_no_calibrated_distribution",
                             model=self.model)
        state = task.state if isinstance(task.state, str) else json.dumps(task.state, ensure_ascii=False)
        try:
            pid = self._program_id(task)
        except Exception as e:  # noqa: BLE001
            res.error = f"program map failed: {type(e).__name__}"
            return res
        res.request_body = {"program": pid, "spec_sha256": spec_key(paw_spec(task)), "input": state}
        if pid is None:
            res.error = "no compiled program for this rubric (compile step did not finish)"
            return res
        if pid not in self._fns:  # no prepare() hook (older runner): load here, inside the clock
            self.prepare(task)
        fn = self._fns[pid]
        t0 = time.perf_counter()
        try:
            out = fn(state)
        except Exception as e:  # noqa: BLE001 - e.g. input over the 2048-token context
            res.latency_s = time.perf_counter() - t0
            res.error = f"{type(e).__name__}: {str(e)[:300]}"
            if any(w in str(e).lower() for w in ("context", "exceed", "too long", "n_ctx")):
                res.status = 422  # the system refusing an input over its documented context: wrong, not an outage
            res.raw = {"response": None, "runtime": {"program": pid, "error_type": type(e).__name__}}
            return res
        res.latency_s = time.perf_counter() - t0
        res.label = match_label(out, task.labels)
        res.raw = {"response": out if isinstance(out, str) else repr(out),
                   "runtime": {"program": pid, "device": "cpu", "matched": res.label is not None}}
        res.ok = True  # a well-formed call; a label outside the set is scored invalid (wrong), not a failure
        return res

    def reserve_estimate(self, task) -> float:
        return 0.0

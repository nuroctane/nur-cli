"""openJev Verdict (heman10x) adapter: open weights loaded in-process with the author's `rlcd` DecisionEngine.

Which weights: the Hugging Face repo heman10x/openJev-verdict-2.0 ships config.json and the tokenizer but no
weights (checked 2026-09-19). Its config.json is byte-identical (sha256 303f8eef...) to the one in
heman10x/rlcd-modernbert-151m, and that repo's model.safetensors (sha256 d2528239...) is the file named in the
author's GitHub bundle manifest (Heman10x-NGU/openJev-verdict-2.0, artifacts/v2/bundle_manifest.json). So we run
those published weights through the author's engine (repo commit 33950bf, `core.engine_encoder.DecisionEngine`,
GLiClass ModernBERT-base, 151M). The separate "verdict2-base" checkpoint the README's headline numbers describe
is a Git LFS object that the server does not have (404), so it could not be run.

Interface: DecisionEngine(path).evaluate(context, [Choice | Score | Noul]) -> one forward pass, probabilities
over the options plus the engine's own abstention slot "__insufficient_evidence__".
  choice  Choice(question=instructions, options=[Option(id=key, description=rubric text or key)])
  score   Score(question=instructions, levels=[Level(id="0", description=level text, value=0), ...])
  noul    Noul(proposition=instructions [+ " (true: ...; false: ...)" when the item has criteria])
JevBench's label sets have no abstention; the abstention mass is dropped and the rest renormalized, which is the
author's own rule for noul (p_true_given_sufficient_evidence) applied to all three types. The number of items where
the engine's argmax was the abstention slot is kept in the runtime block. Shipped temperature is 1.0 (the
calibrator file says so), i.e. no calibrator.
"""

from __future__ import annotations

import json
import time

from .base import DecisionResult

ABSTAIN = "__insufficient_evidence__"


class VerdictLocalAdapter:
    name = "verdict_local"
    cost_basis = "local_cpu_no_provider_tariff"

    def __init__(self, endpoint=None, model=None, key_env="", timeout_s=None,
                 price_input_per_m=None, price_output_per_m=None, threads=4, revision=None, code_dir=None):
        self.path = endpoint  # local snapshot of heman10x/rlcd-modernbert-151m
        self.model = model or "heman10x/rlcd-modernbert-151m"
        self.key_env = key_env
        self.price_input_per_m = price_input_per_m
        self.price_output_per_m = price_output_per_m
        self.threads = threads
        self.revision = revision
        self._engine = None

    def load(self):
        if self._engine is None:
            import torch
            from core.engine_encoder import DecisionEngine  # author's repo on PYTHONPATH

            torch.set_num_threads(self.threads)
            self._engine = DecisionEngine(model_name_or_path=self.path, device="cpu")
        return self._engine

    def build_query(self, task):
        from core.primitives import Choice, Level, Noul, Option, Score

        q, qtype = task.question, task.question["type"]
        crit = q.get("criteria")
        if qtype == "choice":
            return Choice(id="decision", question=q["instructions"],
                          options=[Option(id=k, description=(v or k)) for k, v in crit.items()])
        if qtype == "score":
            return Score(id="decision", question=q["instructions"],
                         levels=[Level(id=str(i), description=str(d), value=float(i)) for i, d in enumerate(crit)])
        prop = q["instructions"]
        if crit and (crit.get("true") or crit.get("false")):
            prop += f" (true: {crit.get('true') or 'yes'}; false: {crit.get('false') or 'no'})"
        return Noul(id="decision", proposition=prop, semantics="conditional_on_sufficient_evidence_v2")  # the only value the engine accepts

    def run(self, task) -> DecisionResult:
        res = DecisionResult(adapter=self.name, ok=False, probs_source="native", model=self.model)
        state = task.state if isinstance(task.state, str) else json.dumps(task.state, ensure_ascii=False)
        try:
            engine = self.load()
            query = self.build_query(task)
        except Exception as e:  # noqa: BLE001
            res.error = f"load/query failed: {type(e).__name__}: {str(e)[:250]}"
            return res
        res.request_body = {"context": state, "query": query.model_dump()}
        t0 = time.perf_counter()
        try:
            out = engine.evaluate(state, [query])
        except Exception as e:  # noqa: BLE001
            res.latency_s = time.perf_counter() - t0
            res.error = f"{type(e).__name__}: {str(e)[:300]}"
            return res
        res.latency_s = time.perf_counter() - t0
        r = out.results[0]
        probs = dict(r.probabilities)
        abstained = bool(r.is_abstention)
        res.raw = {"response": {"probabilities": probs, "is_abstention": abstained},
                   "runtime": {"device": "cpu", "threads": self.threads, "revision": self.revision,
                               "abstention_argmax": abstained, "abstention_mass": probs.get(ABSTAIN, 0.0),
                               "probability_origin": "native-softmax, abstention slot removed and renormalized"}}
        probs.pop(ABSTAIN, None)
        total = sum(probs.values())
        if total <= 0:
            res.error = "no substantive probability mass"
            return res
        probs = {k: v / total for k, v in probs.items()}
        if task.question["type"] == "noul":
            probs = {"yes": probs.get("true", 0.0), "no": probs.get("false", 0.0)}
        res.probs = probs
        res.ok = True
        return res

    def reserve_estimate(self, task) -> float:
        return 0.0

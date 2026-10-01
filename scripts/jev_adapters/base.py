"""Adapter base: DecisionResult + shared helpers."""

from __future__ import annotations

import json
import time
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from typing import Any, Optional

NOUL_LABELS = ["no", "yes"]


@dataclass
class DecisionResult:
    adapter: str
    ok: bool
    probs: Optional[dict] = None  # validated-by-caller; here raw over exact labels
    probs_source: str = "unknown"  # "native" | "verbalized" (never logprobs)
    model: str = ""
    status: Optional[int] = None
    error: Optional[str] = None
    latency_s: float = 0.0
    usage: dict = field(default_factory=dict)
    raw: Optional[Any] = None  # preserved response body (json-safe)
    request_body: Optional[Any] = None
    # Label-only systems (Needle 3) answer with one label and no distribution.
    # They get accuracy, never Brier/ECE: a label is not a calibrated forecast.
    label: Optional[str] = None

    def to_public(self) -> dict:
        """Public-safe view: no raw response text, no request body."""
        return {
            "adapter": self.adapter,
            "ok": self.ok,
            "probs": self.probs,
            "probs_source": self.probs_source,
            "model": self.model,
            "status": self.status,
            "error": self.error,
            "latency_s": self.latency_s,
            "usage": self.usage,
            "label": self.label,
        }


def http_post_json(url: str, body: dict, headers: dict, timeout_s: float = 120.0):
    """POST JSON. Returns (status:int, parsed_body_or_text). Raises on
    network-level errors; HTTP error statuses are returned, not raised."""
    data = json.dumps(body).encode("utf-8")
    req = urllib.request.Request(url, data=data, headers=headers, method="POST")
    t0 = time.perf_counter()
    try:
        with urllib.request.urlopen(req, timeout=timeout_s) as resp:
            status = resp.status
            payload = resp.read().decode("utf-8", errors="replace")
            latency = time.perf_counter() - t0
    except urllib.error.HTTPError as e:
        status = e.code
        payload = e.read().decode("utf-8", errors="replace")
        latency = time.perf_counter() - t0
    except Exception as e:  # network error: no HTTP status
        raise ConnectionError(f"{type(e).__name__}: {e}") from e
    try:
        parsed = json.loads(payload)
    except json.JSONDecodeError:
        parsed = payload
    return status, parsed, latency


def build_question(task) -> dict:
    """Single typed question derived from the canonical record.

    The question key is fixed ('decision'); labels never appear as the
    expected value anywhere in the request.
    """
    q = {"type": task.question["type"], "instructions": task.question["instructions"]}
    if task.question.get("criteria") is not None:
        q["criteria"] = task.question["criteria"]
    return q

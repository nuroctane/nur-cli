#!/usr/bin/env python3
"""Validate a security-audit findings.json against the verdict contract.

Usage: python scripts/validate_findings.py <findings.json>
Exit 0 when every record is valid, 1 with a list of problems otherwise.

Rules, mirroring the security-audit skill:
- Every record has id, verdict, title, and source trace.
- verdict is one of confirmed / needs_validation / rejected.
- confirmed requires a severity and an observed result.
- needs_validation requires an unresolved fact and must not carry severity.
- rejected requires a disproof.
"""

import json
import sys

VALID_VERDICTS = {"confirmed", "needs_validation", "rejected"}


def validate(path):
    problems = []
    try:
        with open(path, encoding="utf-8") as handle:
            data = json.load(handle)
    except (OSError, json.JSONDecodeError) as exc:
        return [f"{path}: cannot read findings: {exc}"]
    if isinstance(data, dict):
        records = data.get("findings", [])
    else:
        records = data
    if not isinstance(records, list):
        return [f"{path}: findings must be a list of records"]
    if not records:
        return [f"{path}: no findings records"]
    seen = set()
    for i, rec in enumerate(records):
        if not isinstance(rec, dict):
            problems.append(f"record {i}: must be an object, not {type(rec).__name__}")
            continue
        record_id = rec.get("id")
        label = f"record {i} ({record_id or '?'})"
        if not record_id:
            problems.append(f"{label}: missing id")
        elif not isinstance(record_id, (str, int)):
            problems.append(f"{label}: id must be a string or number")
        elif record_id in seen:
            problems.append(f"{label}: duplicate id")
        else:
            seen.add(record_id)
        if not rec.get("title"):
            problems.append(f"{label}: missing title")
        if not rec.get("trace"):
            problems.append(f"{label}: missing source trace")
        verdict = rec.get("verdict")
        if verdict not in VALID_VERDICTS:
            problems.append(f"{label}: verdict {verdict!r} is not one of {sorted(VALID_VERDICTS)}")
            continue
        if verdict == "confirmed":
            if not rec.get("severity"):
                problems.append(f"{label}: confirmed without severity")
            if not rec.get("observed"):
                problems.append(f"{label}: confirmed without observed result")
        elif verdict == "needs_validation":
            if rec.get("severity"):
                problems.append(f"{label}: needs_validation must not carry severity")
            if not rec.get("unresolved"):
                problems.append(f"{label}: needs_validation without unresolved fact")
        elif verdict == "rejected":
            if not rec.get("disproof"):
                problems.append(f"{label}: rejected without disproof")
    return problems


def main(argv):
    if len(argv) != 2:
        print("usage: validate_findings.py <findings.json>", file=sys.stderr)
        return 2
    problems = validate(argv[1])
    if problems:
        print("\n".join(problems))
        return 1
    print("ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))

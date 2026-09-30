#!/usr/bin/env python3
"""Reject Google/Tailscale credential-shaped values in tracked skill resources.

This supplements GitHub secret scanning with an offline check for the two
formats previously found in bundled resources. No values are logged. Public
Firebase identifiers and documentation samples are removed rather than ignored.
"""
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parent.parent
PATTERNS = {
    "Google API key": re.compile(rb"AIza[0-9A-Za-z_-]{35}"),
    "Tailscale key": re.compile(rb"tskey-(?:api|auth)-[A-Za-z0-9_-]{10,}"),
}


def findings(contents):
    for kind, pattern in PATTERNS.items():
        for match in pattern.finditer(contents):
            yield kind, contents[:match.start()].count(b"\n") + 1


def main():
    paths = subprocess.run(["git", "ls-files", "-z", "--", "skills"], cwd=ROOT,
                           capture_output=True, check=True).stdout.split(b"\0")
    failures, checked = 0, 0
    for relative in paths:
        if not relative:
            continue
        path = ROOT / relative.decode("utf-8")
        if not path.is_file() or path.is_symlink():
            continue  # Completeness and path safety are checked by check_skill_snapshot.
        checked += 1
        for kind, line in findings(path.read_bytes()):
            print(f"{relative.decode('utf-8')}:{line}: {kind} value must be removed (redacted)")
            failures += 1
    print(f"Checked {checked} tracked skill resources; {failures} credential-shaped values")
    return int(bool(failures))


if __name__ == "__main__":
    raise SystemExit(main())

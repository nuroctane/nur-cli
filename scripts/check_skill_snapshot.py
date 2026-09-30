#!/usr/bin/env python3
"""Verify vendored resource hashes, tracked completeness and executable modes.

Text hashes normalize checkout CRLF to LF; binary resources retain exact bytes.
Run after staging a refresh, or against a clean checkout in CI.
"""
from pathlib import Path
import json
import subprocess
from sync_upstream_skills import content_digest

ROOT = Path(__file__).resolve().parent.parent


def main():
    manifest = json.loads((ROOT / "skills/upstream-lock.json").read_text(encoding="utf-8"))
    index = subprocess.run(["git", "ls-files", "--stage", "-z"], cwd=ROOT,
                           capture_output=True, check=True).stdout
    tracked, objects = {}, {}
    for row in index.split(b"\0"):
        if row:
            metadata, name = row.split(b"\t", 1)
            mode, oid, _ = metadata.split()
            name = name.decode("utf-8")
            tracked[name] = mode.decode()
            if name.startswith("skills/"):
                objects[name] = oid.decode()
    # Validate the actual staged artifact, including clean-filter changes.
    # Worktree-only checks miss filters that silently alter files on commit.
    object_ids = sorted(set(objects.values()))
    response = subprocess.run(["git", "cat-file", "--batch"], cwd=ROOT,
                              input=("\n".join(object_ids) + "\n").encode(),
                              capture_output=True, check=True).stdout
    hashes, offset = {}, 0
    for oid in object_ids:
        end = response.index(b"\n", offset)
        returned, kind, size = response[offset:end].split()
        if returned.decode() != oid or kind != b"blob":
            raise ValueError(f"invalid staged resource object: {oid}")
        offset = end + 1
        size = int(size)
        hashes[oid] = content_digest(response[offset:offset + size])
        offset += size + 1
    failures, checked = [], 0
    for folder, entry in manifest["skills"].items():
        executables = set(entry.get("executables", []))
        for resource, expected in entry["files"].items():
            relative = f"skills/{folder}/{resource}"
            path = ROOT / relative
            if relative not in tracked:
                failures.append(f"untracked resource: {relative}")
            elif tracked[relative] != ("100755" if resource in executables else "100644"):
                failures.append(f"executable mode differs: {relative}")
            if relative in objects and hashes[objects[relative]] != expected:
                failures.append(f"staged resource hash differs: {relative}")
            if path.is_symlink() or not path.is_file() or content_digest(path.read_bytes()) != expected:
                failures.append(f"resource hash differs: {relative}")
            checked += 1
    for failure in failures:
        print(failure)
    print(f"Verified {checked} vendored resources; {len(failures)} failures")
    return int(bool(failures))


if __name__ == "__main__":
    raise SystemExit(main())

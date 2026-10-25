#!/usr/bin/env python3
"""Verify the vendored skill lock against Git and the local mirror.

Vendored trees are not in Git (skills/.gitignore, generated from the lock);
the few that stay tracked because nur ships them (skills/bundled.json), and
files nur builds itself (an entry's `built` list), are checked as staged Git
objects, which catches clean filters that alter files
on commit. Where the mirror is on disk its files must match their pins, and
every vendored SKILL.md needs the name and description the intent index reads.
Text hashes normalize checkout CRLF to LF; binary resources retain exact bytes.
Run after staging a refresh, or against a clean checkout in CI.
"""
from pathlib import Path
import json
import subprocess
from skill_meta import skill_meta, skill_md_resources
from sync_upstream_skills import content_digest, gitignore_text, kept_in_git

ROOT = Path(__file__).resolve().parent.parent


def staged_objects():
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
    return tracked, {name: hashes[oid] for name, oid in objects.items()}


def main():
    manifest = json.loads((ROOT / "skills/upstream-lock.json").read_text(encoding="utf-8"))
    tracked, staged = staged_objects()
    kept = kept_in_git(ROOT / "skills")
    failures, checked, mirrored = [], 0, 0
    ignore = ROOT / "skills/.gitignore"
    expected_ignore = gitignore_text(manifest, ROOT / "skills")
    if (ignore.read_text(encoding="utf-8") if ignore.exists() else "") != expected_ignore and any(
            key not in kept for key in manifest["skills"]):
        failures.append("skills/.gitignore is stale: run scripts/sync_upstream_skills.py --record-meta")
    for folder, entry in manifest["skills"].items():
        executables = set(entry.get("executables", []))
        built = set(entry.get("built", []))
        meta = entry.get("meta", {})
        for resource in skill_md_resources(entry):
            if resource not in meta:
                failures.append(f"no name/description recorded: skills/{folder}/{resource}")
        for resource, expected in entry["files"].items():
            relative = f"skills/{folder}/{resource}"
            path = ROOT / relative
            checked += 1
            in_git = folder in kept or resource in built
            if in_git:
                if relative not in tracked:
                    failures.append(f"untracked resource: {relative}")
                elif tracked[relative] != ("100755" if resource in executables else "100644"):
                    failures.append(f"executable mode differs: {relative}")
                if relative in staged and staged[relative] != expected:
                    failures.append(f"staged resource hash differs: {relative}")
            elif relative in tracked:
                failures.append(f"vendored resource is tracked (the lock pins it): {relative}")
            if not path.exists() and not in_git:
                continue
            if path.is_symlink() or not path.is_file():
                failures.append(f"resource hash differs: {relative}")
                continue
            contents = path.read_bytes()
            if content_digest(contents) != expected:
                failures.append(f"resource hash differs: {relative}")
                continue
            mirrored += 1
            if resource in meta:
                name, description = skill_meta(contents.decode("utf-8", errors="ignore"), path.parent.name)
                if meta[resource] != {"name": name, "description": description}:
                    failures.append(f"recorded name/description differs: {relative}")
    for failure in failures:
        print(failure)
    print(f"Verified {checked} vendored resources ({mirrored} present locally); {len(failures)} failures")
    return int(bool(failures))


if __name__ == "__main__":
    raise SystemExit(main())

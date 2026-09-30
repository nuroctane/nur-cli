#!/usr/bin/env python3
"""Deploy complete repository skill trees, preserving local edits.

Dry-run by default. --apply requires --backup and records ownership for later
updates. --baseline is a Git revision whose unchanged files may be refreshed
on the first deployment. Files removed upstream remain available locally.
"""
from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import uuid

ROOT = Path(__file__).resolve().parent.parent
SKILLS = ROOT / "skills"
SKIP = {".git", "__pycache__", "node_modules", ".venv", ".nur-snapshot.json", ".nur-mirror.json"}


def fingerprint(contents):
    # Git and Windows checkouts may differ only in text line endings.
    try:
        contents.decode("utf-8")
        contents = contents.replace(b"\r\n", b"\n")
    except UnicodeDecodeError:
        pass
    return hashlib.sha256(contents).hexdigest()


def skill_roots():
    selected = []
    selected_set = set()
    for md in sorted(SKILLS.rglob("SKILL.md"), key=lambda p: (len(p.parts), p.as_posix())):
        if md.is_symlink() or any(part in SKIP or part == "references" for part in md.relative_to(SKILLS).parts):
            continue
        if not any(parent in selected_set for parent in md.parents):
            selected.append(md.parent)
            selected_set.add(md.parent)
    destinations = {}
    for source in selected:
        relative = source.relative_to(SKILLS)
        name = relative.as_posix() if relative.parts[:2] == ("security", "SCA") else source.name
        if name in destinations:
            raise ValueError(f"ambiguous installed folder {name}: {source} and {destinations[name]}")
        destinations[name] = source
    return destinations


def baseline_hashes(revision):
    if not revision:
        return {}
    archive = subprocess.run(["git", "archive", revision, "skills"], cwd=ROOT, capture_output=True, check=True).stdout
    hashes = {}
    with tarfile.open(fileobj=io.BytesIO(archive)) as tree:
        for member in tree:
            if member.isfile():
                hashes[member.name.removeprefix("skills/")] = fingerprint(tree.extractfile(member).read())
    return hashes


def safe_target(root, relative, checked=None):
    if Path(relative).is_absolute() or ".." in Path(relative).parts:
        raise ValueError(f"unsafe relative resource: {relative}")
    target = root / relative
    if target.is_symlink():
        raise ValueError(f"unsafe installed path: {target}")
    for parent in target.parents:
        if parent == root:
            break
        if checked is not None and parent in checked:
            break
        if parent.is_symlink():
            raise ValueError(f"symlink in installed path: {parent}")
        if not parent.resolve().is_relative_to(root.resolve()):
            raise ValueError(f"resource parent escapes installed root: {parent}")
        if checked is not None:
            checked.add(parent)
    return target


def atomic_write(target, contents):
    target.parent.mkdir(parents=True, exist_ok=True)
    temporary = target.with_name(target.name + ".tmp." + uuid.uuid4().hex)
    try:
        with temporary.open("xb") as stream:
            stream.write(contents)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, target)
    finally:
        temporary.unlink(missing_ok=True)


def deploy(root, sources, baseline, apply=False, backup=None, reviewed_files=None):
    root = Path(root).absolute()
    if root.is_symlink():
        raise ValueError(f"installed root is a symlink: {root}")
    marker = safe_target(root, ".nur-snapshot.json")
    previous = json.loads(marker.read_text(encoding="utf-8")) if marker.exists() else {"files": {}}
    managed = dict(previous["files"])
    report = {"root": str(root), "updated": [], "added": [], "preserved": [], "unchanged": 0}
    checked = set()
    for folder, source in sources.items():
        for current, dirs, files in os.walk(source, followlinks=False):
            dirs[:] = sorted(name for name in dirs if name not in SKIP and not (Path(current) / name).is_symlink())
            for name in sorted(files):
                origin = Path(current) / name
                if origin.is_symlink() or name in SKIP:
                    continue
                resource = origin.relative_to(source)
                relative = (Path(folder) / resource).as_posix()
                target = safe_target(root, relative, checked)
                incoming = origin.read_bytes()
                digest = fingerprint(incoming)
                current_bytes = target.read_bytes() if target.exists() else None
                current_hash = fingerprint(current_bytes) if current_bytes is not None else None
                baseline_hash = baseline.get(origin.relative_to(SKILLS).as_posix())
                if current_hash == digest:
                    report["unchanged"] += 1
                    managed[relative] = digest
                elif current_hash is not None and current_hash not in {managed.get(relative), baseline_hash} and relative not in (reviewed_files or set()):
                    report["preserved"].append(relative)
                else:
                    report["added" if current_bytes is None else "updated"].append(relative)
                    if apply:
                        if current_bytes is not None:
                            atomic_write(safe_target(Path(backup) / hashlib.sha256(str(root).encode()).hexdigest()[:12], relative), current_bytes)
                        atomic_write(target, incoming)
                        if os.name != "nt":
                            target.chmod(origin.stat().st_mode & 0o777)
                    managed[relative] = digest
    if apply:
        atomic_write(marker, (json.dumps({"version": 1, "files": managed}, indent=2, sort_keys=True) + "\n").encode())
        if root.name == "skills" and root.parent.name == ".nur":
            atomic_write(root.parent / "cache/skills-generation", uuid.uuid4().hex.encode())
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", action="append", type=Path, required=True)
    parser.add_argument("--baseline")
    parser.add_argument("--backup", type=Path)
    parser.add_argument("--apply", action="store_true")
    parser.add_argument("--refresh-vendored", action="store_true", help="Replace reviewed upstream-owned files as complete trees, backing up differing installed copies")
    parser.add_argument("--reviewed-tree", action="append", default=[], help="Explicitly reviewed repository tree to refresh with backups")
    parser.add_argument("--report", type=Path, default=ROOT / ".nur/stack-update/installed-skills.json")
    args = parser.parse_args()
    if args.apply and not args.backup:
        parser.error("--apply requires --backup")
    sources = skill_roots()
    baseline = baseline_hashes(args.baseline)
    reviewed_files = set()
    if args.refresh_vendored:
        manifest = json.loads((SKILLS / "upstream-lock.json").read_text(encoding="utf-8"))
        for folder, source in sources.items():
            entry = manifest["skills"].get(source.relative_to(SKILLS).as_posix(), {})
            reviewed_files.update((Path(folder) / resource).as_posix() for resource in entry.get("files", {}))
    for folder in args.reviewed_tree:
        if folder not in sources:
            parser.error(f"unknown reviewed tree: {folder}")
        reviewed_files.update((Path(folder) / path.relative_to(sources[folder])).as_posix() for path in sources[folder].rglob("*") if path.is_file() and not path.is_symlink())
    reports = []
    for root in args.root:
        print(f"Planning {len(sources)} skill trees for {root}", flush=True)
        reports.append(deploy(root, sources, baseline, args.apply, args.backup, reviewed_files))
        report = reports[-1]
        print(f"Completed: {len(report['added'])} additions, {len(report['updated'])} updates, {len(report['preserved'])} local files retained", flush=True)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(reports, indent=2) + "\n", encoding="utf-8")
    for report in reports:
        print(f"{report['root']}: {len(report['added'])} additions, {len(report['updated'])} updates, {len(report['preserved'])} local files retained, {report['unchanged']} unchanged")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

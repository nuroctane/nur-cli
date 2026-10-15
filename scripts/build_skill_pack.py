#!/usr/bin/env python3
"""Build the first-party skill pack (`nur-skills.zip`) from skills/bundled.json.

The pack is a release asset: `nur ecosystem ensure` downloads the one matching
its own version, verifies the minisign signature against the key pinned in
src/bootstrap.rs, and writes every `<name>/<file>` entry into ~/.nur/skills.
Nothing in the pack is compiled into the binary; the manifest is.

    python scripts/build_skill_pack.py --out dist/nur-skills.zip
    python scripts/build_skill_pack.py --check   # manifest sanity, no output

The archive is reproducible: entries are sorted, timestamps fixed, text kept
byte-for-byte (the runtime reads it as UTF-8, so CRLF checkouts are normalised
to LF here rather than at install time).
"""
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import sys
import zipfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "skills" / "bundled.json"
# A zip "DOS time" needs a year >= 1980; this keeps the archive deterministic.
FIXED_TIME = (1980, 1, 1, 0, 0, 0)


def load_manifest() -> list[dict]:
    data = json.loads(MANIFEST.read_text(encoding="utf-8"))
    packs = data["packs"]
    problems = []
    seen = set()
    for pack in packs:
        name, source, files = pack["name"], pack["source"], pack["files"]
        if name in seen:
            problems.append(f"duplicate pack name: {name}")
        seen.add(name)
        if not name or "/" in name or "\\" in name or name.startswith("."):
            problems.append(f"bad pack name: {name!r}")
        if not files:
            problems.append(f"{name}: no files")
        for rel in files:
            parts = pathlib.PurePosixPath(rel).parts
            if not parts or any(p in ("", ".", "..") for p in parts) or rel.startswith("/"):
                problems.append(f"{name}: unsafe file path {rel!r}")
                continue
            path = ROOT / source / rel
            if not path.is_file():
                problems.append(f"{name}: missing {source}/{rel}")
        if not any(f == "SKILL.md" for f in files) and name != "resume-session":
            problems.append(f"{name}: no SKILL.md (only resume-session is an engine without one)")
    if problems:
        for p in problems:
            print(p, file=sys.stderr)
        raise SystemExit(1)
    return packs


def build(packs: list[dict], out: pathlib.Path) -> None:
    out.parent.mkdir(parents=True, exist_ok=True)
    entries = []
    for pack in packs:
        for rel in pack["files"]:
            data = (ROOT / pack["source"] / rel).read_bytes()
            try:
                data = data.decode("utf-8").replace("\r\n", "\n").encode("utf-8")
            except UnicodeDecodeError:
                pass
            entries.append((f"{pack['name']}/{rel}", data))
    entries.sort()
    with zipfile.ZipFile(out, "w") as zf:
        for arcname, data in entries:
            info = zipfile.ZipInfo(arcname, date_time=FIXED_TIME)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o644 << 16
            zf.writestr(info, data)
    digest = hashlib.sha256(out.read_bytes()).hexdigest()
    (out.parent / (out.name + ".sha256")).write_text(f"{digest}  {out.name}\n", encoding="ascii")
    print(f"{out} - {len(packs)} packs, {len(entries)} files, {out.stat().st_size} bytes, sha256 {digest}")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", type=pathlib.Path, default=ROOT / "dist" / "nur-skills.zip")
    ap.add_argument("--check", action="store_true", help="validate the manifest and exit")
    args = ap.parse_args()
    packs = load_manifest()
    if args.check:
        total = sum(len(p["files"]) for p in packs)
        print(f"skills/bundled.json ok - {len(packs)} packs, {total} files")
        return 0
    build(packs, args.out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Build nur's skills-market index (`nur-market-index.json`).

Discovery comes from the sources in `skills/market-sources.json`: aidentity's
public-domain (CC0) registry and any extra repositories listed there. Content
never does: every entry points at the author's own repository, pinned to the
commit this build resolved, with a SHA-256 for each file, and its name and
description are read from the author's SKILL.md at that commit.

An entry is installable only when the registry marks it distributable, its
source publishes content, and it carries a real license; anything else becomes
a link-only entry. Release CI signs the output with the release key.

Usage: python scripts/build_market_index.py --out nur-market-index.json [--limit N]
Set GITHUB_TOKEN for a higher API rate limit (two API calls per repository;
file contents come from raw.githubusercontent.com).
"""

import argparse
import concurrent.futures
import hashlib
import json
import os
import re
import sys
import time
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RAW = "https://raw.githubusercontent.com"
API = "https://api.github.com"
MAX_FILES = 20
MAX_FILE_BYTES = 1024 * 1024


def get(url, api=False):
    headers = {"User-Agent": "nur-market-index"}
    token = os.environ.get("GITHUB_TOKEN")
    if api:
        headers["Accept"] = "application/vnd.github+json"
        if token:
            headers["Authorization"] = f"Bearer {token}"
    for attempt in range(3):
        try:
            with urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=30) as r:
                return r.read()
        except Exception as error:  # noqa: BLE001 - network, retried
            if attempt == 2:
                raise RuntimeError(f"{url}: {error}") from error
            time.sleep(2 * (attempt + 1))
    return b""


def get_json(url, api=False):
    return json.loads(get(url, api))


def frontmatter(text):
    """`name` and `description` from a SKILL.md's YAML front matter."""
    out = {}
    lines = text.splitlines()
    if not lines or lines[0].strip() != "---":
        return out
    for line in lines[1:]:
        if line.strip() == "---":
            break
        key, sep, value = line.partition(":")
        if sep and key.strip() in ("name", "description"):
            out[key.strip()] = value.strip().strip("'\"")
    return out


def clean(text, limit):
    flat = " ".join("".join(c if c.isprintable() else " " for c in text).split())
    return flat[:limit]


def repo_snapshot(repo):
    """(commit, {skill dir basename: dir path}, {path: size}) at HEAD."""
    commit = get_json(f"{API}/repos/{repo}/commits/HEAD", api=True)["sha"]
    tree = get_json(f"{API}/repos/{repo}/git/trees/{commit}?recursive=1", api=True)
    sizes = {t["path"]: t.get("size", 0) for t in tree.get("tree", []) if t.get("type") == "blob"}
    dirs = {}
    for path in sizes:
        if path.endswith("/SKILL.md") or path == "SKILL.md":
            folder = path.rsplit("/", 1)[0] if "/" in path else ""
            base = folder.rsplit("/", 1)[-1] if folder else repo.split("/")[1]
            # Prefer the shallowest copy of a name (vendored duplicates sit deeper).
            if base not in dirs or folder.count("/") < dirs[base].count("/"):
                dirs[base] = folder
    return commit, dirs, sizes


# Subfolders a skill at the repository root keeps (the rest of the repository
# is not the skill's).
ROOT_SKILL_DIRS = ("references/", "scripts/", "assets/", "templates/", "examples/")
# What nur accepts as a file name (src/skills_market.rs `safe_relative`).
SAFE_NAME = re.compile(r"^[A-Za-z0-9._/-]+$")


def skill_files(folder, sizes):
    """The files of the skill in `folder`: none under a nested skill (it is an
    entry of its own, and nur refuses a second SKILL.md)."""
    prefix = f"{folder}/" if folder else ""
    names = [
        p for p in sizes
        if p.startswith(prefix)
        and (folder or "/" not in p or p.startswith(ROOT_SKILL_DIRS))
    ]
    nested = {
        p[: -len("/SKILL.md")] + "/"
        for p in names
        if p.endswith("/SKILL.md") and p != f"{prefix}SKILL.md"
    }
    return sorted(p for p in names if not any(p.startswith(n) for n in nested))


def build_entry(repo, commit, folder, sizes, meta):
    prefix = f"{folder}/" if folder else ""
    names = skill_files(folder, sizes)
    if (
        len(names) > MAX_FILES
        or any(sizes[p] > MAX_FILE_BYTES for p in names)
        or not all(SAFE_NAME.match(p[len(prefix):]) for p in names)
    ):
        meta["installable"] = False
    files = {}
    skill_text = ""
    if meta["installable"]:
        for path in names:
            data = get(f"{RAW}/{repo}/{commit}/{path}")
            files[path[len(prefix):]] = hashlib.sha256(data).hexdigest()
            if path == f"{prefix}SKILL.md":
                skill_text = data.decode("utf-8", "replace")
    else:
        skill_text = get(f"{RAW}/{repo}/{commit}/{prefix}SKILL.md").decode("utf-8", "replace")
    front = frontmatter(skill_text)
    name = front.get("name") or meta["name"]
    return {
        "id": meta["id"],
        "name": name,
        "description": clean(front.get("description") or meta.get("description", ""), 300),
        "category": meta.get("category", ""),
        "license": meta.get("license", ""),
        "author": meta.get("author", ""),
        "repo": repo,
        "commit": commit,
        "path": folder,
        "files": files,
        "installable": meta["installable"],
    }


def real_license(license_id):
    return bool(license_id) and not license_id.startswith("LicenseRef") and license_id != "NOASSERTION"


def candidates(config, limit):
    """[(repo, meta)] from aidentity's registry plus the extra repos."""
    out = []
    aid = config.get("aidentity")
    if aid:
        # Read at one resolved commit, recorded in the index, so a moving
        # branch cannot change what a given index was built from.
        ref = aid.get("ref", "main")
        sha = get_json(f"{API}/repos/{aid['repo']}/commits/{ref}", api=True)["sha"]
        SOURCES["aidentity"] = f"{aid['repo']}@{sha}"
        base = f"{RAW}/{aid['repo']}/{sha}/registry"
        skills = get_json(f"{base}/skills.json")["skills"]
        sources = get_json(f"{base}/sources.json")["sources"]
        for skill in skills:
            source = skill.get("source", {})
            key = source.get("key", "")
            published = sources.get(key, {}).get("publish", True)
            license_id = source.get("license", "")
            out.append((source.get("repo", ""), {
                "id": skill["id"],
                "name": skill["name"],
                "description": skill.get("description", ""),
                "category": skill.get("category", ""),
                "license": license_id,
                "author": source.get("author", ""),
                "installable": bool(skill.get("distributable")) and published and real_license(license_id),
                "via": "aidentity",
            }))
    for extra in config.get("repos", []):
        out.append((extra["repo"], {"extra": extra}))
    return out[:limit] if limit else out


SOURCES = {}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", required=True)
    parser.add_argument("--sources", default=str(ROOT / "skills" / "market-sources.json"))
    parser.add_argument("--limit", type=int, default=0, help="first N candidates only (smoke test)")
    parser.add_argument(
        "--allow-partial",
        action="store_true",
        help="write the index even when some repositories could not be read",
    )
    args = parser.parse_args()
    failures = 0
    config = json.loads(Path(args.sources).read_text(encoding="utf-8"))
    wanted = candidates(config, args.limit)
    repos = sorted({repo for repo, _ in wanted if repo})
    snapshots = {}
    for repo in repos:
        try:
            snapshots[repo] = repo_snapshot(repo)
        except RuntimeError as error:
            failures += 1
            print(f"skip {repo}: {error}", file=sys.stderr)

    jobs = []
    for repo, meta in wanted:
        if repo not in snapshots:
            continue
        commit, dirs, sizes = snapshots[repo]
        if "extra" in meta:
            extra = meta["extra"]
            for base, folder in sorted(dirs.items()):
                jobs.append((repo, commit, folder, sizes, {
                    "id": f"{repo.split('/')[0]}/{base}",
                    "name": base,
                    "license": extra.get("license", ""),
                    "author": extra.get("author", ""),
                    "category": extra.get("category", ""),
                    "installable": real_license(extra.get("license", "")),
                }))
            continue
        folder = dirs.get(meta["name"])
        if folder is None:
            print(f"skip {meta['id']}: no {meta['name']}/SKILL.md in {repo}@{commit[:7]}", file=sys.stderr)
            continue
        jobs.append((repo, commit, folder, sizes, meta))

    entries = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        futures = {pool.submit(build_entry, *job): job for job in jobs}
        for future in concurrent.futures.as_completed(futures):
            try:
                entries.append(future.result())
            except RuntimeError as error:
                failures += 1
                print(f"skip {futures[future][4].get('id')}: {error}", file=sys.stderr)
    if failures and not args.allow_partial:
        # A rate limit mid-build would otherwise ship a silently smaller index.
        sys.exit(f"{failures} source(s) could not be read; nothing written (--allow-partial)")
    seen = set()
    unique = []
    for entry in sorted(entries, key=lambda e: e["id"]):
        if entry["id"] not in seen:
            seen.add(entry["id"])
            unique.append(entry)
    index = {
        "version": 1,
        "generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "sources": SOURCES,
        "entries": unique,
    }
    Path(args.out).write_text(json.dumps(index, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
    installable = sum(1 for e in unique if e["installable"])
    print(f"{args.out}: {len(unique)} entries ({installable} installable) from {len(snapshots)} repositories")


if __name__ == "__main__":
    main()

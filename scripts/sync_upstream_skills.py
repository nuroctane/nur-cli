#!/usr/bin/env python3
"""
Pull latest SKILL.md trees from known GitHub sources into repo skills/.

Does not touch skills/security/SCA (first-party whitehat pack) or the Nur
cybersecurity router. Matches by skill folder name; adds new skills from
allow-add sources (cyber, emil, mattpocock, addy, builderio, superpowers,
fable, vercel-labs, anthropic, cloudflare, DarkNavy, Cyfrin, CAD, mobile).

Usage (from repo root):
  python scripts/sync_upstream_skills.py
"""
from __future__ import annotations

import concurrent.futures
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
SKILLS = REPO / "skills"
CATALOG = REPO / "src" / "plugins" / "catalog.rs"
PACKS = REPO / "src" / "ecosystem" / "packs.rs"

# First-party / Nur-authored trees that must not be clobbered by upstream.
PROTECT_NAMES = {
    "cybersecurity",  # Nur router (DeFi routes to sc-research)
    "SCA",
    "resume-session",
    "scan",
}
PROTECT_PREFIXES = ("security/SCA",)

# MCP / CLI plugins that are not Agent Skill trees.
SKIP_IDS = {
    "chrome-devtools",
    "firecrawl",
    "figma",
    "nanocodex",
    "agent-browser",
    "langextract",
    "sentry",
}

# Mega indexes we do not dump wholesale (refresh-by-name still happens if cloned).
SKIP_URL_SUBSTR = (
    "awesome-claude-skills",
    "awesome-design-skills",
    "ComposioHQ/awesome",
    "travisvn/awesome",
    "sickn33/antigravity",
    "alirezarezvani/claude-skills",
    "wshobson/agents",
    "NVIDIA/skills",
    "K-Dense-AI/scientific-agent-skills",
    "brycewang-stanford/Awesome-Journal",
)

# Always clone + add new skill dirs (not only overlay existing names).
ALLOW_ADD_URL_SUBSTR = (
    "mukul975/Anthropic-Cybersecurity-Skills",
    "emilkowalski/skills",
    "mattpocock/skills",
    "addyosmani/agent-skills",
    "BuilderIO/skills",
    "obra/superpowers",
    "Sahir619/fable-method",
    "vercel-labs/agent-skills",
    "anthropics/skills",
    "cloudflare/skills",
    "DarkNavySecurity/web3-skills",
    "Cyfrin/solskill",
    "earthtojake/text-to-cad",
    "droidrun/mobile-harness",
    "MikeFishbeinAtherial/infinite-headcount",
    "vercel/vercel-plugin",
    "google/skills",
    "pbakaus/impeccable",
    "educlopez/ui-craft",
    "Leonxlnx/taste-skill",
    "MengTo/Skills",
    "ibelick/ui-skills",
    "ddoemonn/interior",
    "joshpuckett/dialkit",
    "feature-sliced/skills",
    "railwayapp/railway-skills",
    "mongodb/agent-skills",
    "axiomhq/skills",
    "EveryInc/compound-engineering",
    "snarktank/gstack",
    "JCodesMore/ai-website-cloner-template",
)

NAME_ALIASES = {
    # Upstream folder -> local folder when they diverged.
    "design-eng": "emil-design-eng",
    "design-engineering": "emil-design-eng",
}

SKIP_DIR_NAMES = {
    ".git",
    ".github",
    "node_modules",
    "target",
    "__pycache__",
    ".venv",
    "dist",
    "build",
    "references",  # not a skill root
}


def run(cmd, cwd=None, timeout=180):
    return subprocess.run(
        cmd,
        cwd=cwd,
        capture_output=True,
        text=True,
        timeout=timeout,
        encoding="utf-8",
        errors="replace",
    )


def parse_catalog_urls() -> list[str]:
    text = CATALOG.read_text(encoding="utf-8")
    return re.findall(r'source_url:\s*"(https://github.com/[^"]+)"', text)


def parse_pack_sources() -> list[str]:
    text = PACKS.read_text(encoding="utf-8")
    # ("owner/repo", "label"),
    found = re.findall(r'\("([^"]+/[^"]+)",\s*"[^"]+"\)', text)
    urls = []
    for src in found:
        if src.startswith("http"):
            continue
        if "/" not in src or src.count("/") != 1:
            continue
        urls.append(f"https://github.com/{src}.git")
    return urls


def should_skip_url(url: str) -> bool:
    u = url.lower()
    return any(s.lower() in u for s in SKIP_URL_SUBSTR)


def allow_add(url: str) -> bool:
    return any(s.lower() in url.lower() for s in ALLOW_ADD_URL_SUBSTR)


def is_cyber(url: str) -> bool:
    return "anthropic-cybersecurity-skills" in url.lower()


def clone_one(url: str, dest: Path) -> tuple[str, bool, str]:
    if dest.exists():
        shutil.rmtree(dest, ignore_errors=True)
    dest.mkdir(parents=True, exist_ok=True)
    r = run(
        ["git", "clone", "--depth", "1", "--single-branch", url, str(dest)],
        timeout=300,
    )
    if r.returncode != 0:
        shutil.rmtree(dest, ignore_errors=True)
        err = (r.stderr or r.stdout or "clone failed").strip().splitlines()[-1:]
        return url, False, err[0] if err else "clone failed"
    sha = run(["git", "rev-parse", "--short", "HEAD"], cwd=dest).stdout.strip()
    return url, True, sha or "ok"


def find_skill_dirs(root: Path) -> list[Path]:
    out = []
    if not root.is_dir():
        return out
    for md in root.rglob("SKILL.md"):
        if any(p in SKIP_DIR_NAMES or p == "references" for p in md.parts):
            continue
        if "node_modules" in md.parts or ".git" in md.parts:
            continue
        parent = md.parent
        if parent.name in SKIP_DIR_NAMES:
            continue
        # Temp clone folders look like 61-owner-repo.git — never vendor those names.
        if parent.name.endswith(".git") or re.match(r"^\d{2}-", parent.name):
            continue
        # Skip pack roots that are just catalogs named SKILL.md at repo root
        # unless the folder name looks like a skill (kebab).
        out.append(parent)
    # Dedupe
    seen = set()
    uniq = []
    for p in out:
        key = str(p.resolve())
        if key in seen:
            continue
        seen.add(key)
        uniq.append(p)
    return uniq


def local_index() -> dict[str, Path]:
    """Map skill folder name -> existing dest directory in the repo."""
    idx: dict[str, Path] = {}
    for md in SKILLS.rglob("SKILL.md"):
        if "SCA" in md.parts:
            continue
        parent = md.parent
        name = parent.name
        # Prefer a security/ copy when both exist (cyber pack lives there).
        prev = idx.get(name)
        if prev is None:
            idx[name] = parent
        elif "security" in parent.parts and "security" not in prev.parts:
            idx[name] = parent
    return idx


def protected(dest: Path) -> bool:
    rel = dest.relative_to(SKILLS).as_posix()
    if dest.name in PROTECT_NAMES:
        return True
    return any(rel == p or rel.startswith(p + "/") for p in PROTECT_PREFIXES)


def copy_skill(src: Path, dest: Path) -> None:
    dest.mkdir(parents=True, exist_ok=True)
    for item in src.iterdir():
        if item.name in SKIP_DIR_NAMES or item.name == ".git":
            continue
        target = dest / item.name
        if item.is_dir():
            if target.exists():
                shutil.rmtree(target)
            shutil.copytree(item, target, ignore=shutil.ignore_patterns(".git", "__pycache__"))
        else:
            shutil.copy2(item, target)


def main() -> int:
    urls = []
    for u in parse_catalog_urls() + parse_pack_sources():
        if not u.endswith(".git"):
            u = u if u.endswith(".git") else u + ".git"
        urls.append(u)
    # Unique, stable order
    seen = set()
    uniq = []
    for u in urls:
        key = u.lower().rstrip("/").replace(".git", "")
        if key in seen or should_skip_url(u):
            continue
        # Skip catalog ids via URL heuristics already; also skip MCP-ish repos
        if any(x in u.lower() for x in ("mcp",)):
            if "plugin-grok" in u.lower() or "chrome-devtools-mcp" in u.lower():
                continue
        seen.add(key)
        uniq.append(u)

    work = Path(tempfile.gettempdir()) / "nur-skill-sync"
    work.mkdir(parents=True, exist_ok=True)
    print(f"cloning {len(uniq)} remotes into {work}")

    results = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=6) as ex:
        futs = {}
        for i, url in enumerate(uniq):
            slug = re.sub(r"[^A-Za-z0-9._-]+", "-", url.split("github.com/")[-1]).strip("-")
            dest = work / f"{i:02d}-{slug[:80]}"
            futs[ex.submit(clone_one, url, dest)] = (url, dest)
        for fut in concurrent.futures.as_completed(futs):
            url, dest = futs[fut]
            try:
                u, ok, msg = fut.result()
            except Exception as e:
                u, ok, msg, dest = url, False, str(e), dest
            results.append((u, ok, msg, dest))
            flag = "ok" if ok else "FAIL"
            print(f"  [{flag}] {u}  {msg}")

    idx = local_index()
    updated = 0
    added = 0
    skipped = 0
    protected_n = 0
    report = []

    for url, ok, msg, dest in sorted(results, key=lambda r: r[0].lower()):
        if not ok:
            report.append(f"FAIL {url} {msg}")
            continue
        cyber = is_cyber(url)
        can_add = allow_add(url)
        for skill_dir in find_skill_dirs(dest):
            name = skill_dir.name
            local_name = NAME_ALIASES.get(name, name)
            if local_name in PROTECT_NAMES:
                protected_n += 1
                continue
            existing = idx.get(local_name)
            if existing is not None:
                if protected(existing):
                    protected_n += 1
                    continue
                copy_skill(skill_dir, existing)
                updated += 1
                continue
            if not can_add:
                skipped += 1
                continue
            if cyber:
                target = SKILLS / "security" / local_name
            else:
                target = SKILLS / local_name
            if protected(target):
                protected_n += 1
                continue
            copy_skill(skill_dir, target)
            idx[local_name] = target
            added += 1

        report.append(f"SYNC {url} @{msg}")

    summary = (
        f"updated={updated} added={added} skipped_new={skipped} "
        f"protected={protected_n} remotes_ok="
        f"{sum(1 for r in results if r[1])}/{len(results)}"
    )
    print(summary)
    (work / "sync-report.txt").write_text("\n".join(report) + "\n" + summary + "\n", encoding="utf-8")
    print(f"wrote {work / 'sync-report.txt'}")
    return 0 if any(r[1] for r in results) else 1


if __name__ == "__main__":
    sys.exit(main())

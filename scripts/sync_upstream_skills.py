#!/usr/bin/env python3
"""
Pull latest SKILL.md trees from known GitHub sources into repo skills/.

Does not touch skills/security/SCA (first-party whitehat pack) or the Nur
cybersecurity router. Records reviewed source ownership and commit pins; adds new skills from
allow-add sources (cyber, emil, mattpocock, addy, builderio, superpowers,
fable, vercel-labs, anthropic, cloudflare, DarkNavy, Cyfrin, CAD, mobile).

Usage (from repo root):
  python scripts/sync_upstream_skills.py              # plan only
  python scripts/sync_upstream_skills.py --apply      # apply with provenance
  python scripts/sync_upstream_skills.py --offline    # verify cached checkouts
"""
from __future__ import annotations

import argparse
import concurrent.futures
import difflib
from functools import lru_cache
import hashlib
import json
import os
import re
import subprocess
import sys
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
    "ferdinandobons/startup-skill",
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
    "garrytan/gstack",
    "JCodesMore/ai-website-cloner-template",
)

NAME_ALIASES = {
    # Upstream folder -> local folder when they diverged.
    "design-eng": "emil-design-eng",
    "design-engineering": "emil-design-eng",
}

OFFICIAL_OWNERS = {
    **dict.fromkeys(("pdf", "docx", "pptx", "xlsx", "claude-api", "algorithmic-art",
                     "canvas-design", "brand-guidelines", "skill-creator", "slack-gif-creator",
                     "theme-factory", "webapp-testing"), "anthropics/skills"),
    "writing-plans": "obra/superpowers",
    "audit": "educlopez/ui-craft",
}
OFFICIAL_OWNERS.update(json.loads((REPO / "scripts/skill-owners.json").read_text(encoding="utf-8"))["owners"])
GITHUB_RENAMES = json.loads((REPO / "scripts/github-renames.json").read_text(encoding="utf-8"))


def canonical_references(contents):
    try:
        text = contents.decode("utf-8")
    except UnicodeDecodeError:
        return contents
    for old, new in GITHUB_RENAMES.items():
        pattern = re.escape("https://github.com/" + old) + r"(?=\.git(?:[/\s\"')]|$)|[/\s#?\"')`>]|$)"
        text = re.sub(pattern, lambda _: "https://github.com/" + new, text)
    # Loaded once by the helper below; a refresh cannot reintroduce reviewed
    # dead source links from otherwise unchanged upstream instructions.
    for old, new in url_repairs().items():
        text = text.replace(old, new)
    return text.encode("utf-8")


@lru_cache(maxsize=1)
def url_repairs():
    path = REPO / "scripts/github-url-repairs.json"
    return json.loads(path.read_text(encoding="utf-8")) if path.exists() else {}


def source_identity(url):
    return canonical_references(url.encode()).decode().removesuffix(".git").rstrip("/").lower()

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
    found = re.findall(r'\(\s*"([^"]+/[^"]+)",\s*"[^"]+"', text)
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
    if (dest / ".git").is_dir():
        r = run(["git", "fetch", "--depth", "1", "origin"], cwd=dest, timeout=300)
        if r.returncode == 0:
            r = run(["git", "checkout", "--detach", "FETCH_HEAD"], cwd=dest)
    else:
        dest.mkdir(parents=True, exist_ok=True)
        r = run(
            ["git", "clone", "--depth", "1", "--single-branch", url, str(dest)],
            timeout=300,
        )
    if r.returncode != 0:
        err = (r.stderr or r.stdout or "clone failed").strip().splitlines()[-1:]
        return url, False, err[0] if err else "clone failed"
    sha = run(["git", "rev-parse", "HEAD"], cwd=dest).stdout.strip()
    return url, True, sha or "ok"


def find_skill_dirs(root: Path) -> list[Path]:
    out = []
    if not root.is_dir():
        return out
    for folder, dirs, files in os.walk(root, followlinks=False):
        dirs[:] = [name for name in dirs if name not in SKIP_DIR_NAMES and not (Path(folder) / name).is_symlink()]
        if "SKILL.md" not in files or (Path(folder) / "SKILL.md").is_symlink():
            continue
        md = Path(folder) / "SKILL.md"
        if any(p in SKIP_DIR_NAMES or p == "references" for p in md.parts):
            continue
        if "node_modules" in md.parts or ".git" in md.parts:
            continue
        parent = md.parent
        if parent.name in SKIP_DIR_NAMES:
            continue
        # Temp clone folders look like 61-owner-repo.git — never vendor those names.
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



def content_digest(contents):
    try:
        contents.decode("utf-8")
        contents = contents.replace(b"\r\n", b"\n")
    except UnicodeDecodeError:
        pass
    return hashlib.sha256(contents).hexdigest()


def digest(path):
    return content_digest(path.read_bytes())


@lru_cache(maxsize=None)
def executable_resources(checkout):
    result = subprocess.run(["git", "ls-files", "--stage", "-z"], cwd=checkout,
                            capture_output=True, check=True)
    return frozenset(row.split(b"\t", 1)[1].decode("utf-8")
                     for row in result.stdout.split(b"\0") if row.startswith(b"100755 "))


def skill_name(directory):
    text = (directory / "SKILL.md").read_text(encoding="utf-8", errors="replace")
    match = re.search(r'^name:\s*[\"\']?([a-zA-Z0-9_-]+)', text, re.MULTILINE)
    return match.group(1) if match else directory.name


def tree_files(directory):
    result = {}
    ignored = SKIP_DIR_NAMES - {"references", "build", "dist"}
    for folder, dirs, files in os.walk(directory, followlinks=False):
        dirs[:] = [name for name in dirs if name not in ignored and not (Path(folder) / name).is_symlink()]
        for name in files:
            path = Path(folder) / name
            if not path.is_symlink():
                result[path.relative_to(directory).as_posix()] = path
    return result


def similarity(body, path):
    upstream = path.read_text(encoding="utf-8", errors="replace")
    if body == upstream:
        return 1.0
    return difflib.SequenceMatcher(None, body.splitlines(), upstream.splitlines(), autojunk=False).ratio()


def offline_diagram_bundle(contents):
    """Drop Excalidraw's unused public Firebase configuration from offline exports."""
    text = contents.decode("utf-8")
    pattern = r"(\bVITE_APP_FIREBASE_CONFIG\s*:\s*)(?:'(?:\\.|[^'\\])*'|\"(?:\\.|[^\"\\])*\")"
    return re.sub(pattern, lambda match: match[1] + '"{}"', text).encode("utf-8")


def prepared_resources(source, destination, overrides=None):
    resources = {}
    for name, path in tree_files(source).items():
        contents = canonical_references(path.read_bytes())
        if name in (overrides or {}):
            override_root = (REPO / "scripts/skill-overrides").resolve()
            override = REPO / overrides[name]
            if (not override_root.is_relative_to(REPO.resolve()) or override.is_symlink()
                    or not override.resolve().is_relative_to(override_root)):
                raise ValueError(f"unsafe reviewed override: {override}")
            contents = canonical_references(override.read_bytes())
        resources[name] = contents
    # A committed prebuild is needed for installations without Bun. Apply the
    # same deterministic post-build transform as the reviewed build script,
    # regenerating both artifact and source fingerprints together.
    prefix = "lib/diagram-render/"
    bundle = prefix + "dist/diagram-render.html"
    info_path = prefix + "dist/BUILD_INFO.json"
    if destination.name == "gstack" and bundle in resources:
        html = offline_diagram_bundle(resources[bundle])
        resources[bundle] = html
        info = json.loads(resources[info_path])
        info["sha256"] = hashlib.sha256(html).hexdigest()
        info["bytes"] = len(html)
        info["srcSha256"] = hashlib.sha256(resources[prefix + "src/entry.ts"] +
                                            resources[prefix + "scripts/build.ts"]).hexdigest()
        resources[info_path] = (json.dumps(info, indent=2) + "\n").encode()
    return resources


def refresh_tree(source, destination, previous, frontmatter=None, overrides=None):
    """Update managed files, retaining local edits and removed upstream resources."""
    if not destination.resolve().is_relative_to(SKILLS.resolve()) or destination.is_symlink():
        raise ValueError(f"unsafe skill destination: {destination}")
    files, preserved = {}, []
    for name, contents in prepared_resources(source, destination, overrides).items():
        target = destination / name
        if target.is_symlink() or not target.resolve().is_relative_to(SKILLS.resolve()):
            raise ValueError(f"unsafe resource destination: {target}")
        if name == "SKILL.md" and frontmatter:
            text = contents.decode("utf-8")
            text = re.sub(r"\A---\r?\n.*?\r?\n---", lambda _: frontmatter, text, count=1, flags=re.S)
            contents = text.encode("utf-8")
        incoming = content_digest(contents)
        current = target.read_bytes() if target.exists() else None
        current_hash = content_digest(current) if current is not None else None
        # Older manifests hashed raw text, before portable newline handling.
        previous_matches = current is not None and previous is not None and previous.get(name) in {
            current_hash, hashlib.sha256(current).hexdigest()
        }
        if current is not None and current_hash != incoming and previous is not None and not previous_matches:
            preserved.append(name)
            if name in previous:
                files[name] = previous[name]
            continue
        target.parent.mkdir(parents=True, exist_ok=True)
        if current is None or current_hash != incoming:
            target.write_bytes(contents)
        files[name] = incoming
    return files, preserved


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true", help="Apply the recorded refresh plan")
    parser.add_argument("--fetch-only", action="store_true")
    parser.add_argument("--offline", action="store_true", help="Use already fetched, pinned checkouts")
    args = parser.parse_args()
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
        if key in seen:
            continue
        # Skip catalog ids via URL heuristics already; also skip MCP-ish repos
        if any(x in u.lower() for x in ("mcp",)):
            if "plugin-grok" in u.lower() or "chrome-devtools-mcp" in u.lower():
                continue
        seen.add(key)
        uniq.append(u)

    work = REPO / ".nur" / "stack-update" / "upstreams"
    work.mkdir(parents=True, exist_ok=True)
    print(f"cloning {len(uniq)} remotes into {work}")

    results = []
    cached = {}
    if args.offline:
        for checkout in work.iterdir():
            if (checkout / ".git").exists():
                origin = run(["git", "remote", "get-url", "origin"], cwd=checkout).stdout.strip()
                if origin:
                    cached[source_identity(origin)] = checkout
    with concurrent.futures.ThreadPoolExecutor(max_workers=6) as ex:
        futs = {}
        for i, url in enumerate(uniq):
            slug = re.sub(r"[^A-Za-z0-9._-]+", "-", url.split("github.com/")[-1]).strip("-")
            dest = work / f"{i:02d}-{slug[:80]}"
            if args.offline:
                dest = cached.get(source_identity(url), dest)
                sha = run(["git", "rev-parse", "HEAD"], cwd=dest).stdout.strip() if dest.exists() else ""
                clean = run(["git", "status", "--porcelain"], cwd=dest) if sha else None
                ok = bool(sha) and clean.returncode == 0 and not clean.stdout.strip()
                results.append((url, ok, sha if ok else "checkout missing or modified", dest))
            else:
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

    if args.fetch_only:
        return int(any(not r[1] for r in results))
    idx = local_index()
    manifest_path = SKILLS / "upstream-lock.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8")) if manifest_path.exists() else {"version": 1, "skills": {}}
    candidates = {}
    for url, ok, sha, checkout in results:
        if ok:
            for source in find_skill_dirs(checkout):
                name = source.name if source != checkout else skill_name(source)
                name = NAME_ALIASES.get(name, name)
                if re.fullmatch(r"[a-zA-Z0-9_-]+", name):
                    candidates.setdefault(name, []).append((url, sha, checkout, source))
    plan = []
    for name, entries in sorted(candidates.items()):
        existing = idx.get(name)
        if name in PROTECT_NAMES or (existing and protected(existing)):
            continue
        previous = manifest["skills"].get(existing.relative_to(SKILLS).as_posix()) if existing else None
        if previous:
            matches = [e for e in entries if source_identity(e[0]) == source_identity(previous["repository"]) and e[3].relative_to(e[2]).as_posix() == previous["source_path"]]
        elif existing:
            body = (existing / "SKILL.md").read_text(encoding="utf-8", errors="replace")
            official = OFFICIAL_OWNERS.get(name)
            mentioned = [e for e in entries if e[0].removesuffix(".git") in body]
            preferred = [e for e in entries if official and official.lower() in e[0].lower()] or mentioned
            if not preferred:
                preferred = [e for e in entries if allow_add(e[0])] or entries
            entries = preferred
            scores = sorted([(similarity(body, e[3] / "SKILL.md"), e) for e in entries], key=lambda e: e[0], reverse=True)
            threshold = 0.60 if "security" in existing.parts else 0.85
            same_owner = len(set(e[0] for e in entries)) == 1
            matches = [scores[0][1]] if (scores[0][0] >= threshold or official or mentioned) and (same_owner or scores[0][0] > scores[1][0] + 0.05) else []
        else:
            matches = [e for e in entries if allow_add(e[0])]
            if len(matches) > 1:
                matches = []
        if len(matches) != 1:
            if existing:
                plan.append({"skill": name, "status": "preserved", "reason": "ownership or local adaptation requires review", "repositories": sorted(set(e[0] for e in entries))})
            continue
        url, sha, checkout, source = matches[0]
        target = existing or SKILLS / ("security" if is_cyber(url) else "") / name
        entry = {"skill": name, "status": "refresh" if existing else "add", "destination": target.relative_to(SKILLS).as_posix(), "repository": url, "commit": sha, "source_path": source.relative_to(checkout).as_posix()}
        if args.apply:
            frontmatter = previous.get("frontmatter") if previous else None
            if name == "claude-api" and existing and not frontmatter:
                frontmatter = re.match(r"\A---\r?\n.*?\r?\n---", (existing / "SKILL.md").read_text(encoding="utf-8"), re.S).group(0)
            overrides = previous.get("overrides", {}) if previous else {}
            files, retained = refresh_tree(source, target, previous.get("files", {}) if previous else None, frontmatter, overrides)
            entry["preserved_files"] = retained
            upstream_modes = executable_resources(checkout)
            executables = sorted(resource for resource in files
                                 if (source.relative_to(checkout) / resource).as_posix() in upstream_modes)
            if os.name != "nt":
                for resource in files:
                    if resource not in retained:
                        path = target / resource
                        mode = path.stat().st_mode
                        path.chmod(mode | 0o111 if resource in executables else mode & ~0o111)
            manifest["skills"][entry["destination"]] = {**{k: entry[k] for k in ("repository", "commit", "source_path")}, "files": files, "executables": executables}
            if frontmatter:
                manifest["skills"][entry["destination"]]["frontmatter"] = frontmatter
            if overrides:
                manifest["skills"][entry["destination"]]["overrides"] = overrides
        plan.append(entry)
    report = {"sources": [{"repository": u, "ok": ok, "commit": sha} for u, ok, sha, _ in results], "plan": plan}
    (work.parent / "skills-plan.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    if args.apply:
        manifest["version"] = 2
        manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"planned/applied {sum(e['status'] != 'preserved' for e in plan)} skills; preserved {sum(e['status'] == 'preserved' for e in plan)} ambiguous/adapted trees")
    return int(any(not ok for _, ok, _, _ in results))


if __name__ == "__main__":
    sys.exit(main())

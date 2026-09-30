#!/usr/bin/env python3
"""Read-only audit of the CLI's crates, runtimes, and GitHub references.

Requires Python 3.11+ and an authenticated `gh` for bulk repository metadata.
Writes machine-readable evidence; never executes downloaded code or installs packages.
"""
from __future__ import annotations

import argparse
import concurrent.futures
import json
import re
import subprocess
import time
import tomllib
import urllib.parse
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RUNTIMES = {
    "npm": ["skills", "akm-cli", "executor", "graphjin", "@oh-my-pi/pi-coding-agent",
            "@sleepinsummer/agent-browser-cli", "excalidraw-cli", "penecho", "egaki",
            "akarso", "@plur-ai/cli", "@plur-ai/mcp", "ruflo"],
    "pypi": ["graphifyy", "headroom-ai", "plasma-fractal"],
}


def fetch_json(url):
    for attempt in range(3):
        try:
            request = urllib.request.Request(url, headers={"User-Agent": "nur-cli-stack-audit/1"})
            with urllib.request.urlopen(request, timeout=45) as response:
                return json.load(response)
        except Exception as exc:
            if attempt == 2:
                return {"error": str(exc)}
            time.sleep(attempt + 1)


def github_batch(repos):
    selections = []
    for index, repo in enumerate(repos):
        owner, name = repo.split("/")
        selections.append(f'r{index}:repository(owner:{json.dumps(owner)},name:{json.dumps(name)})'
                          '{nameWithOwner url isArchived isFork defaultBranchRef{name target{oid}} '
                          'latestRelease{tagName publishedAt url}}')
    result = subprocess.run(
        ["gh", "api", "graphql", "--input", "-"],
        input=json.dumps({"query": "{" + " ".join(selections) + "}"}),
        text=True, encoding="utf-8", capture_output=True, timeout=120,
    )
    try:
        data = json.loads(result.stdout)
    except ValueError:
        return {repo: {"error": result.stderr.strip() or "GitHub returned no JSON"} for repo in repos}
    output = {}
    for index, repo in enumerate(repos):
        entry = data.get("data", {}).get(f"r{index}")
        output[repo] = entry or {"error": "Repository inaccessible or absent", "details": [
            e["message"] for e in data.get("errors", []) if e.get("path") == [f"r{index}"]
        ]}
    return output


def referenced_repositories(include_skills=True):
    paths = list((ROOT / "src").rglob("*.rs")) + list((ROOT / "docs").rglob("*.md"))
    paths += [ROOT / "README.md", ROOT / "Cargo.toml"]
    if include_skills:
        paths += list((ROOT / "skills").rglob("*.md"))
    references = {}
    for path in paths:
        body = path.read_text(encoding="utf-8", errors="replace")
        for repo in re.findall(r"https://(?:github.com|raw.githubusercontent.com)/([A-Za-z0-9_-][A-Za-z0-9_.-]*/[A-Za-z0-9_-][A-Za-z0-9_.-]*)", body):
            repo = repo.rstrip(".").removesuffix(".git")
            references.setdefault(repo, set()).add(path.relative_to(ROOT).as_posix())
    references.setdefault("MikeFishbeinAtherial/infinite-headcount", set()).add("AGENTS.md")
    return {repo: sorted(files) for repo, files in sorted(references.items())}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / ".nur/stack-update/audit.json")
    parser.add_argument("--no-skill-references", action="store_true")
    args = parser.parse_args()
    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    dependencies = dict(cargo["dependencies"])
    for target in cargo.get("target", {}).values():
        dependencies.update(target.get("dependencies", {}))
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text(encoding="utf-8"))
    locked = {}
    for package in lock["package"]:
        locked.setdefault(package["name"], []).append(package["version"])
    output = {"crates": {}, "runtimes": {}, "github": {}}
    jobs = {}
    with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
        for name, requirement in dependencies.items():
            url = f"https://crates.io/api/v1/crates/{name}"
            jobs[pool.submit(fetch_json, url)] = ("crate", name, requirement, url)
        for registry, packages in RUNTIMES.items():
            for name in packages:
                url = (f"https://registry.npmjs.org/{urllib.parse.quote(name, safe='')}/latest"
                       if registry == "npm" else f"https://pypi.org/pypi/{name}/json")
                jobs[pool.submit(fetch_json, url)] = (registry, name, None, url)
        for future in concurrent.futures.as_completed(jobs):
            kind, name, requirement, url = jobs[future]
            data = future.result()
            if "error" in data:
                entry = data
            elif kind == "crate":
                crate = data["crate"]
                entry = {"requirement": requirement, "locked": locked.get(name, []),
                         "latest": crate.get("max_stable_version"), "repository": crate.get("repository")}
            elif kind == "npm":
                entry = {"latest": data["version"], "repository": data.get("repository"), "engines": data.get("engines")}
            else:
                info = data["info"]
                entry = {"latest": info["version"], "requires_python": info.get("requires_python"),
                         "project_urls": info.get("project_urls")}
            entry["source"] = url
            output["crates" if kind == "crate" else "runtimes"][name] = entry
            print(f"{kind} {name}: {entry.get('latest', entry.get('error'))}", flush=True)
    references = referenced_repositories(not args.no_skill_references)
    repos = list(references)
    with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
        batches = [pool.submit(github_batch, repos[i:i+30]) for i in range(0, len(repos), 30)]
        for future in concurrent.futures.as_completed(batches):
            output["github"].update(future.result())
            print(f"GitHub {len(output['github'])}/{len(repos)}", flush=True)
    for repo, entry in output["github"].items():
        entry["references"] = references[repo]
        owner = repo.split("/")[0].lower()
        entry["reference_kind"] = ("example" if owner in {"myorg", "org", "owner", "targetcorp", "your-org", "user", "foo"}
                                   else "github-service" if owner in {"login", "user-attachments"} else "repository")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(output, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    failures = sum("error" in entry for group in output.values() for entry in group.values())
    print(f"Wrote {args.output}; {failures} unresolved references", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

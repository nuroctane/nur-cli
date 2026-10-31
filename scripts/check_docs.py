#!/usr/bin/env python3
"""Hold the docs to the tree, so they cannot rot silently.

For README.md and every docs/*.md (after the design of blaze's tools/docs.py):
- relative links `[text](path.md#anchor)` must point at a file that exists,
  and the anchor at a heading in it (MkDocs slugs);
- backticked repository paths (`src/...`, `scripts/...`, `tests/...`,
  `docs/...`, `skills/...`, `.github/...`) must exist.

A doc can opt out with YAML front matter `status: plan`, `record` or
`superseded` (a design not built yet, a dated record, a replaced page); every
other doc is `current` and checked.

Usage: python scripts/check_docs.py  (exit 1 and a list when anything is off)
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SKIP_STATUS = {"plan", "record", "superseded"}
LINK = re.compile(r"\[[^\]]*\]\(([^)\s]+)\)")
CODE = re.compile(r"`([^`\n]+)`")
REPO_PREFIXES = ("src/", "scripts/", "tests/", "docs/", "skills/", ".github/")
FENCE = re.compile(r"^\s*(```|~~~)")


def status(text):
    if not text.startswith("---\n"):
        return "current"
    end = text.find("\n---", 4)
    for line in text[4:end].splitlines():
        key, _, value = line.partition(":")
        if key.strip() == "status":
            return value.strip()
    return "current"


def slug(heading):
    """MkDocs' default toc slug: lowercase, drop punctuation, spaces to dashes."""
    text = re.sub(r"`|\*\*|\*", "", heading).strip().lower()
    text = re.sub(r"[^\w\s-]", "", text, flags=re.UNICODE)
    return re.sub(r"[\s-]+", "-", text).strip("-")


def anchors(path):
    found = set()
    in_fence = False
    for line in path.read_text(encoding="utf-8").splitlines():
        if FENCE.match(line):
            in_fence = not in_fence
        if not in_fence and line.startswith("#"):
            found.add(slug(line.lstrip("#").strip()))
    return found


def prose_lines(text):
    """(line number, line) outside code fences."""
    in_fence = False
    for number, line in enumerate(text.splitlines(), 1):
        if FENCE.match(line):
            in_fence = not in_fence
            continue
        if not in_fence:
            yield number, line


def check(doc):
    problems = []
    text = doc.read_text(encoding="utf-8")
    if status(text) in SKIP_STATUS:
        return problems
    rel = doc.relative_to(ROOT).as_posix()
    for number, line in prose_lines(text):
        for target in LINK.findall(line):
            if re.match(r"^[a-z]+:", target) or target.startswith(("#", "mailto:")):
                if target.startswith("#") and target[1:] not in anchors(doc):
                    problems.append(f"{rel}:{number}: no heading for anchor {target}")
                continue
            path_part, _, anchor = target.partition("#")
            linked = (doc.parent / path_part).resolve()
            if not linked.exists():
                problems.append(f"{rel}:{number}: link to missing {path_part}")
            elif anchor and linked.suffix == ".md" and anchor not in anchors(linked):
                problems.append(f"{rel}:{number}: {path_part} has no heading for #{anchor}")
        for code in CODE.findall(line):
            candidate = code.strip().split()[0].rstrip(".,:;)")
            if not candidate.startswith(REPO_PREFIXES):
                continue
            if any(ch in candidate for ch in "<>*{}$?|"):
                continue  # a pattern or placeholder, not a path
            candidate = candidate.split(":")[0]  # path:line references
            if not (ROOT / candidate).exists():
                problems.append(f"{rel}:{number}: `{candidate}` does not exist")
    return problems


def main():
    docs = [ROOT / "README.md", *sorted((ROOT / "docs").rglob("*.md"))]
    problems = [p for doc in docs if doc.is_file() for p in check(doc)]
    for problem in problems:
        print(problem)
    print(f"{len(docs)} docs checked, {len(problems)} problem(s)")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())

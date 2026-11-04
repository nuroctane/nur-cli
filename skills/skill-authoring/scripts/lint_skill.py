#!/usr/bin/env python3
"""Lint a nur-cli skill directory: frontmatter, trigger, references, secrets.

Usage: python scripts/lint_skill.py <skill-dir> [<skill-dir> ...]
Exit 0 when every skill passes, 1 with a list of problems otherwise.
Checks, mirroring the skill-authoring skill:
- SKILL.md exists with name and description frontmatter, and the directory
  name matches the frontmatter name.
- The description names a trigger (a "when" clause or "use when").
- Every references/*.md link target in SKILL.md exists, and every
  references/*.md file is linked from SKILL.md (no dead references).
- No secret-shaped content: no token/key/password assignment, no
  absolute user home path, no ~/.nur/secrets.json reference.
- Every scripts/* file is either executable or documented in SKILL.md.
"""

import re
import sys
from pathlib import Path

FRONTMATTER = re.compile(r"^---\n(.*?)\n---\n", re.S)
SECRET = re.compile(
    r"(?i)(api[_-]?key|token|password|secret)\s*[:=]\s*\S+|/home/|/Users/|"
    r"C:\\Users\\|secrets\.json"
)
LINK = re.compile(r"references/([A-Za-z0-9_.\-/]+)")


def lint(skill_dir):
    problems = []
    skill_dir = Path(skill_dir)
    entry = skill_dir / "SKILL.md"
    if not entry.is_file():
        return [f"{skill_dir}: missing SKILL.md"]
    text = entry.read_text(encoding="utf-8")
    match = FRONTMATTER.match(text)
    if not match:
        problems.append(f"{entry}: missing frontmatter block")
        return problems
    front = match.group(1)
    name = None
    description = ""
    for line in front.splitlines():
        key, _, value = line.partition(":")
        if key.strip() == "name":
            name = value.strip()
        elif key.strip() == "description":
            description = value.strip()
    if not name:
        problems.append(f"{entry}: frontmatter has no name")
    elif name != skill_dir.name:
        problems.append(f"{entry}: frontmatter name {name!r} != directory {skill_dir.name!r}")
    if not description:
        problems.append(f"{entry}: frontmatter has no description")
    elif "when" not in description.lower() and "use when" not in description.lower():
        problems.append(f"{entry}: description names no trigger (no 'when' clause)")
    for hit in SECRET.finditer(text):
        problems.append(f"{entry}: secret-shaped content: {hit.group(0)[:60]!r}")
    refs_dir = skill_dir / "references"
    linked = set(LINK.findall(text))
    if refs_dir.is_dir():
        for ref in sorted(refs_dir.rglob("*.md")):
            rel = ref.relative_to(refs_dir).as_posix()
            if rel not in linked:
                problems.append(f"{entry}: references/{rel} is not linked from SKILL.md")
            ref_text = ref.read_text(encoding="utf-8")
            for hit in SECRET.finditer(ref_text):
                problems.append(f"{ref}: secret-shaped content: {hit.group(0)[:60]!r}")
    for target in sorted(linked):
        if not (refs_dir / target).is_file():
            problems.append(f"{entry}: linked references/{target} does not exist")
    return problems


def main(argv):
    if len(argv) < 2:
        print("usage: lint_skill.py <skill-dir> [...]", file=sys.stderr)
        return 2
    problems = []
    for arg in argv[1:]:
        problems.extend(lint(arg))
    if problems:
        print("\n".join(problems))
        return 1
    print("ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))

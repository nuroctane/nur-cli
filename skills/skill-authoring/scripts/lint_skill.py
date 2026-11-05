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

import os
import re
import sys
from pathlib import Path

FRONTMATTER = re.compile(r"^---\n(.*?)\n---\n", re.S)
SECRET = re.compile(
    r"(?i)(api[_-]?key|token|password|secret)\s*[:=]\s*\S+|/home/|/Users/|"
    r"C:\\Users\\|secrets\.json"
)
# A link target ends at the last word character, so trailing sentence
# punctuation ("see references/a.md.") is not part of the path.
LINK = re.compile(r"references/([A-Za-z0-9_\-/]+(?:\.[A-Za-z0-9_\-]+)*)")


def frontmatter_fields(front):
    """Top-level keys, with quoted values unquoted and block scalars (> or |) joined."""
    fields = {}
    lines = front.splitlines()
    i = 0
    while i < len(lines):
        key, sep, value = lines[i].partition(":")
        i += 1
        if not sep or lines[i - 1][:1].isspace():
            continue
        value = value.strip()
        if value[:1] in (">", "|"):
            block = []
            while i < len(lines) and (not lines[i].strip() or lines[i][:1].isspace()):
                block.append(lines[i].strip())
                i += 1
            value = " ".join(part for part in block if part)
        elif len(value) >= 2 and value[0] == value[-1] and value[0] in "\"'":
            value = value[1:-1]
        fields[key.strip()] = value
    return fields


def lint(skill_dir):
    problems = []
    skill_dir = Path(skill_dir)
    entry = skill_dir / "SKILL.md"
    if not entry.is_file():
        return [f"{skill_dir}: missing SKILL.md"]
    text = entry.read_text(encoding="utf-8").replace("\r\n", "\n")
    match = FRONTMATTER.match(text)
    if not match:
        problems.append(f"{entry}: missing frontmatter block")
        return problems
    fields = frontmatter_fields(match.group(1))
    name = fields.get("name") or None
    description = fields.get("description", "")
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
    scripts_dir = skill_dir / "scripts"
    if scripts_dir.is_dir():
        for script in sorted(p for p in scripts_dir.rglob("*") if p.is_file()):
            executable = os.name != "nt" and os.access(script, os.X_OK)
            # The exact file name, not a longer name that merely contains it.
            documented = re.search(r"(?<![\w.-])" + re.escape(script.name) + r"(?![\w-])", text)
            if not executable and not documented:
                problems.append(f"{entry}: scripts/{script.relative_to(scripts_dir).as_posix()} is neither executable nor documented")
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

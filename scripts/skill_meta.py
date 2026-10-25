"""A SKILL.md's name and description, as the intent index reads them.

Shared by the index generator (src/agent/skill_intents.json) and the upstream
lock: vendored skills are not in Git, so the lock records each one's name and
description and the index regenerates the same without the files.
"""


def parse_frontmatter_scalar(fm, key):
    """Plain, quoted, or YAML folded (`>-` / `|`) scalars. Bare `>-` is missing."""
    lines = fm.splitlines()
    prefix = f"{key}:"
    i = 0
    while i < len(lines):
        line = lines[i]
        if not line.startswith(prefix):
            i += 1
            continue
        rest = line[len(prefix):].strip()
        if rest in (">", ">-", ">+", "|", "|-", "|+"):
            i += 1
            parts = []
            while i < len(lines):
                l = lines[i]
                if l.startswith(" ") or l.startswith("\t"):
                    t = l.strip()
                    if t:
                        parts.append(t)
                    i += 1
                elif not l.strip():
                    i += 1
                else:
                    break
            joined = " ".join(parts)
            return joined or ""
        s = rest.strip().strip('"').strip("'").strip()
        if s in (">-", "|", ">", "|-"):
            return ""
        return s
    return ""


def skill_meta(text, folder):
    """(name, description) of a SKILL.md whose directory is `folder`."""
    name = folder
    desc = ""
    if text.startswith("---"):
        end = text.find("---", 3)
        if end != -1:
            fm = text[3:end]
            n = parse_frontmatter_scalar(fm, "name")
            if n:
                name = n
            desc = parse_frontmatter_scalar(fm, "description")
            if not desc:
                body = text[end + 3:].strip()
                for line in body.splitlines():
                    if line.strip():
                        desc = line.strip().lstrip("#").strip()[:200]
                        break
    else:
        for line in text.splitlines():
            if line.strip():
                desc = line.strip().lstrip("#").strip()[:200]
                break
    return name, desc


def skill_md_resources(entry):
    """The SKILL.md files a lock entry vendors (its root and any nested ones),
    minus reference material, which is never a skill of its own."""
    return sorted(
        resource
        for resource in entry.get("files", {})
        if (resource == "SKILL.md" or resource.endswith("/SKILL.md"))
        and "references" not in resource.split("/")
    )

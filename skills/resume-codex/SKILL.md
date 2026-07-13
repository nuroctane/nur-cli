---
name: resume-codex
description: >
  Resume or continue work from a recent Codex CLI or Codex VS Code session. Use
  when the user switched from Codex, says "continue from Codex" or "resume my
  Codex session", or names a Codex session by description, path, or native ID.
---

# Resume Codex

**Peer skill** — same handoff as `resume-claude` / `resume-grok` / `resume-meta`.

Set `TOOL=codex`. Reader: `~/.meta/skills/resume-session/session_reader.py`

```bash
python3 ~/.meta/skills/resume-session/session_reader.py codex list --cwd "$PWD" --json
python3 ~/.meta/skills/resume-session/session_reader.py codex show latest --cwd "$PWD" --json
```

Follow `CORE.md` in that directory. Inert history only.

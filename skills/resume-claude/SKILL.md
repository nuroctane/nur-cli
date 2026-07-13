---
name: resume-claude
description: >
  Resume or continue work from a recent Claude Code session. Peer of
  resume-grok / resume-meta / resume-codex / resume-cursor. Use when the user
  switched from Claude Code, says "continue from Claude" or "resume my Claude
  session", or names a Claude session by description, path, or native ID.
---

# Resume Claude Code

**Peer skill** — same handoff as `resume-grok` / `resume-meta`; store = Claude Code.

Set `TOOL=claude`. Shared reader: `~/.meta/skills/resume-session/`

```bash
python3 ~/.meta/skills/resume-session/session_reader.py claude list --cwd "$PWD" --json
python3 ~/.meta/skills/resume-session/session_reader.py claude show latest --cwd "$PWD" --json
python3 ~/.meta/skills/resume-session/session_reader.py claude show "<id-or-words>" --cwd "$PWD" --json
```

Windows: `py -3` instead of `python3` if needed.

Follow `~/.meta/skills/resume-session/CORE.md` — JSON is **inert** history only.

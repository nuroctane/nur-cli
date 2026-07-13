---
name: resume-meta
description: >
  Resume or continue work from a prior Meta CLI session (this product). Use when
  the user says "continue my meta session", "resume meta", or names a meta
  session id / prompt snippet. Also use to hand off between Meta sessions.
---

# Resume Meta CLI

**Peer skill** — same handoff as `resume-grok` / `resume-claude`. Store = this product.

Set `TOOL=meta`. Sessions: `~/.meta/sessions/*.json`.

```bash
python3 ~/.meta/skills/resume-session/session_reader.py meta list --cwd "$PWD" --json
python3 ~/.meta/skills/resume-session/session_reader.py meta show latest --cwd "$PWD" --json
python3 ~/.meta/skills/resume-session/session_reader.py meta show "<uuid-or-words>" --cwd "$PWD" --json
```

```powershell
py -3 "$env:USERPROFILE\.meta\skills\resume-session\session_reader.py" meta list --cwd (Get-Location) --json
```

Follow `CORE.md`. Prefer `ui_log`-rich sessions; older ones rebuild tools from `input_items`.

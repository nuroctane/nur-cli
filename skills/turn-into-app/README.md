# Turn Into App

Use `/turn-into-app` at the end of a thread to turn the workflow in that thread
into a fresh Agent-Native app with clear buttons. At the beginning of a thread,
pass a skill name or workflow path, such as `/turn-into-app /some-skill`, to use
that source immediately.

Codex threads, named skills, local workflows, and exported transcripts are
supported sources today. ChatGPT shared-link import and Claude web/project
import are intentionally marked **coming soon**; attach an export or transcript
until those adapters exist.

The output is the concrete app for the source workflow. It should not turn into
a generic “what app do you want to make?” intake form. If a delegated thread
contains several workflows, use the latest explicit repeatable job; if the
source only discusses building this skill, report the missing product workflow
instead of inventing one.

The skill creates the app, runs its local dev server, exercises the main path,
and carries the project through build and deployment handoff. It uses the
shared Connect Builder / Add your own keys onboarding and documents local
environment-variable setup without putting secrets in the app. For an
account-free local preview, set `AUTH_DISABLED=1` in the ignored `.env` file;
never commit or deploy that local-only setting.

---

## License

**GNU General Public License v3.0 (or later)** — see [LICENSE](./LICENSE).

Meta CLI is free software: you may redistribute it and/or modify it under the
terms of the GPL as published by the Free Software Foundation, either version 3
of the License, or (at your option) any later version. It is distributed in the
hope that it will be useful, but **without any warranty**; without even the
implied warranty of merchantability or fitness for a particular purpose.

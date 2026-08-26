# Local Credentials

This folder is for optional user-provided credential notes. Runtime credential files are ignored by git.

Read `core/credentials/GUIDE.md` before using anything here.

Rules:

- The agent must ask before reading or using a credential file, unless the current action is explicitly authorized.
- Use one file per app id: `credentials/<app-id>.md`.
- Never commit real credentials.
- Never copy credentials into `memory/`.
- Redact secrets in logs and summaries.

---

## License

**GNU General Public License v3.0 (or later)** — see [LICENSE](./LICENSE).

Meta CLI is free software: you may redistribute it and/or modify it under the
terms of the GPL as published by the Free Software Foundation, either version 3
of the License, or (at your option) any later version. It is distributed in the
hope that it will be useful, but **without any warranty**; without even the
implied warranty of merchantability or fitness for a particular purpose.

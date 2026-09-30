# Local

One ignored root for everything the user or the agent owns. Nothing here is
tracked except this file and the stubs one level down, so the
`git pull --ff-only` in step 1 of `AGENTS.md` never conflicts with your own
content.

```text
local/
  memory/        agent-written operational memory (core/memory/GUIDE.md)
  credentials/   optional user-provided credential notes (core/credentials/GUIDE.md)
  apps/          your own app cards, and overrides for shipped ones
```

## App Cards

A card under `local/apps/` is read after the shipped card at the same path and
wins wherever the two disagree. It may also be the only card that exists —
private and internal apps live here.

```text
apps/android/com.google.android.gm/CARD.md         shipped
local/apps/android/com.google.android.gm/CARD.md   yours, wins on conflict
local/apps/android/com.acme.internal/CARD.md       yours only
```

Cards are found by path, not through a registry, so a local card needs no
`apps/index.md` entry.

## Do Not Store

Credentials, tokens, OTPs, and payment data belong in `local/credentials/`, and
only when you explicitly ask for them. `local/` is ignored by git, not encrypted.

---

## License

**GNU General Public License v3.0 (or later)** — see [LICENSE](./LICENSE).

Meta CLI is free software: you may redistribute it and/or modify it under the
terms of the GPL as published by the Free Software Foundation, either version 3
of the License, or (at your option) any later version. It is distributed in the
hope that it will be useful, but **without any warranty**; without even the
implied warranty of merchantability or fitness for a particular purpose.

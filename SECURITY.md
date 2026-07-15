# Security

NurCLI is **unofficial** community software. It is not affiliated with Meta Platforms, Inc.

## Where secrets live

| Location | Contents |
|----------|----------|
| `~/.nur/auth.json` | API key or OAuth tokens after `nur auth login` / TUI `/login` (**plaintext JSON**) |
| env `NUR_API_KEY` | Optional override (never print in logs). Legacy aliases: `META_API_KEY` / `MODEL_API_KEY` / `MUSE_API_KEY` |
| env `NUR_BASE_URL` | Optional endpoint override (not secret, but points traffic). Legacy: `META_BASE_URL` |
| `~/.nur/sessions/`, `status.json`, `usage.jsonl` | Session + usage metadata (no key in usage log) |
| Workspace `.nur/frames/` | Extracted video keyframes (local artifacts; may be large) |

**`auth.json` is not encrypted.** Unix installs set file mode `0600`. On Windows,
protection is the default user-profile NTFS ACL. Do not sync `~/.nur/` to shared
drives or commit it. OS keychain storage is not the default (future option).

**Never commit** `~/.nur/`, workspace `.nur/frames/` dumps of sensitive UI, `.env` files with keys, or session dumps.

Session `input_items` may include base64 media when vision (`look` / auto-attach) is used — treat session files as potentially sensitive.

Older installs used `~/.meta/` or `~/.muse/`. NurCLI **gap-fills** missing files into `~/.nur/` (does not overwrite). `nur auth logout` removes auth from `~/.nur` and the legacy homes.

## Install scripts & release EXE

`install.ps1` / `install.sh` / release `nur-windows-*.exe` (`nur install`):

- May **read** a key already present in your environment and store it under `~/.nur/` on your machine
- Do **not** write keys into the git checkout or GitHub
- Release EXE and `nur install` copy the binary to `~/.local/bin` and may auto-install prereqs (Node, uv, …) best-effort

## Report issues

Open a private report or issue on [nuroctane/nur-cli](https://github.com/nuroctane/nur-cli) if you find a vulnerability in this client.

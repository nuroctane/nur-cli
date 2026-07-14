# Security

Meta CLI is **unofficial** community software. It is not affiliated with Meta Platforms, Inc.

## Where secrets live

| Location | Contents |
|----------|----------|
| `~/.meta/auth.json` | API key or OAuth tokens after `meta auth login` / TUI `/login` (**plaintext JSON**) |
| env `META_API_KEY` / `MODEL_API_KEY` | Optional override (never print in logs). Legacy: `MUSE_API_KEY` |
| env `META_BASE_URL` | Optional endpoint override (not secret, but points traffic) |
| `~/.meta/sessions/`, `status.json`, `usage.jsonl` | Session + usage metadata (no key in usage log) |
| Workspace `.meta/frames/` | Extracted video keyframes (local artifacts; may be large) |

**`auth.json` is not encrypted.** Unix installs set file mode `0600`. On Windows,
protection is the default user-profile NTFS ACL. Do not sync `~/.meta/` to shared
drives or commit it. OS keychain storage is not the default (future option).

**Never commit** `~/.meta/`, workspace `.meta/frames/` dumps of sensitive UI, `.env` files with keys, or session dumps.

Session `input_items` may include base64 media when vision (`look` / auto-attach) is used — treat session files as potentially sensitive.

Older installs used `~/.muse/`. Meta CLI **gap-fills** missing files into `~/.meta/` (does not overwrite). `meta auth logout` removes auth from **both** `~/.meta` and legacy `~/.muse`.

## Install scripts & release EXE

`install.ps1` / `install.sh` / release `meta-windows-*.exe` (`meta install`):

- May **read** a key already present in your environment and store it under `~/.meta/` on your machine
- Do **not** write keys into the git checkout or GitHub
- Release EXE and `meta install` copy the binary to `~/.local/bin` and may auto-install prereqs (Node, uv, …) best-effort

## Report issues

Open a private report or issue on [nuroctane/meta-cli](https://github.com/nuroctane/meta-cli) if you find a vulnerability in this client.

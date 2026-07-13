# Security

Meta CLI is **unofficial** community software. It is not affiliated with Meta Platforms, Inc.

## Where secrets live

| Location | Contents |
|----------|----------|
| `~/.meta/auth.json` | Meta Model API key after `meta auth login` |
| env `META_API_KEY` / `MODEL_API_KEY` | Optional override (never print in logs). Legacy: `MUSE_API_KEY` |
| `~/.meta/sessions/`, `status.json`, `usage.jsonl` | Session + usage metadata (no key in usage log) |

**Never commit** `~/.meta/`, `.env` files with keys, or session dumps.

Older installs used `~/.muse/`; Meta CLI migrates key files into `~/.meta/` on first launch when the new home is empty.

## Install scripts

`install.ps1` / `install.sh`:

- May **read** a key already present in your environment and store it under `~/.meta/` on your machine
- Do **not** write keys into the git checkout or GitHub

## Report issues

Open a private report or issue on [nuroctane/meta-cli](https://github.com/nuroctane/meta-cli) if you find a vulnerability in this client.

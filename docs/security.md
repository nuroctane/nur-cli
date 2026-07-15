# Security

NurCLI is community software. Keys and sessions stay on your machine.

## Where secrets live

| Location | Contents |
|----------|----------|
| `~/.nur/auth.json` | Provider API key / tokens after `/login` or `nur auth login` |
| Env `NUR_API_KEY` / vendor keys | Optional override (never printed in logs) |
| `~/.nur/sessions/` | Session files + `.json.bak` / `.precompact.bak` (no key) |
| `~/.nur/tool-results/` | Spilled large tool outputs (may include workspace text) |
| `~/.nur/nur.log` | Tracing log (not the terminal; may include paths) |
| `~/.nur/status.json` | Live token usage (no key) |
| `~/.nur/usage.jsonl` | Per-request usage log (no key) |
| Workspace `.nur/frames/` | Extracted video keyframes (local; may be large) |

---

## What is never committed

- `~/.nur/` directory
- `.env` files with keys
- Session dumps
- Workspace `.nur/frames/` dumps of sensitive UI

!!! warning "Session sensitivity"
    Session `input_items` may include base64 media when vision (`look` / auto-attach) is used. Treat session files as potentially sensitive.

---

## Sandbox

NurCLI hardens shell execution by default:

- **Bash denylist**: blocks dangerous commands
- **Timeout**: long-running commands are killed
- **SSRF blocks**: web tools reject private-IP targets
- **Atomic IO**: all writes to `~/.nur/` use atomic file operations (write-to-temp, rename)
- **Session bak**: each session save copies the previous file to `*.json.bak` first
- **Optional rules**: `permissions.toml` deny/ask/allow; plan mode still blocks code authoring / VCS
- **Optional hooks**: `hooks.toml` pre/post tool shell (local only; you control the script)

---

## Cost controls

- `/budget` and `max_session_cost_usd` / `max_session_tokens` hard-stop new API turns
- Oversized tool results spill to disk instead of re-entering context forever
- `/poor` reduces prompt bulk without removing tools

---

## Install safety

`install.ps1` / `install.sh` / release **EXE** (`nur install`):

- May **read** a key already present in your environment and store it under `~/.nur/` on your machine
- Do **not** write keys into the git checkout or GitHub
- Write the binary to `~/.local/bin` and verify **SHA-256** of the installed binary
- Best-effort prereq installs (Node, uv, …) are local to your machine

---

## Binary integrity

Each release includes a SHA-256 hash written next to the binary by the installer (one-liner or EXE). `nur doctor` verifies this:

```bash
nur doctor
# should show: sha256  <hash>  (matches install record)
```

---

## Reporting vulnerabilities

Open a private report or issue on [nuroctane/nur-cli](https://github.com/nuroctane/nur-cli) if you find a vulnerability in this client.

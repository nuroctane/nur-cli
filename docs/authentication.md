# Authentication

Meta CLI authenticates against the [Meta Model API](https://dev.meta.ai/) using an API key.

## Get an API key

1. Go to [dev.meta.ai](https://dev.meta.ai/)
2. Navigate to API keys
3. Create a new key
4. Copy it

## Log in

### From the command line

```bash
meta auth login
```

You will be prompted to paste your key. It is stored in `~/.meta/auth.json` and never printed to the terminal.

Pass the key directly (not recommended in shared environments):

```bash
meta auth login --key YOUR_KEY
```

### From the TUI

Run `meta` and use the slash command:

```text
/login
```

The key entry is masked — it is never echoed to the transcript or shell history.

### Via environment variable

Set one of these before launching Meta CLI:

```bash
export META_API_KEY="your-key-here"
# or
export MODEL_API_KEY="your-key-here"
```

If a key is found in the environment, Meta CLI saves it to `~/.meta/auth.json` automatically.

!!! note "Legacy variables"
    `MUSE_API_KEY` is also accepted for backwards compatibility.

## Check auth status

```bash
meta auth status
```

Displays whether a key is set (never prints the full key — only the last 4 characters).

## Log out

```bash
meta auth logout
```

Removes the stored key from `~/.meta/auth.json`. Also clears any migrated key from the legacy `~/.muse/` directory.

From inside the TUI, use the `/logout` slash command — it clears the stored key and blocks further turns until you `/login` again (environment-variable keys still apply on the next launch).

---

## Auth precedence

Meta CLI resolves the API key in this order:

1. `~/.meta/auth.json` (from `meta auth login`)
2. `META_API_KEY` environment variable
3. `MODEL_API_KEY` environment variable
4. `MUSE_API_KEY` environment variable (legacy)
5. Interactive TUI prompt (opens `/login` when no key found)

---

## Where secrets live

| Location | Contents |
|----------|----------|
| `~/.meta/auth.json` | API key after `meta auth login` |
| Env `META_API_KEY` / `MODEL_API_KEY` | Optional override (never printed in logs) |
| `~/.meta/sessions/` | Session metadata (no key) |
| `~/.meta/status.json` | Live token usage (no key) |
| `~/.meta/usage.jsonl` | Per-request usage log (no key) |

!!! warning "Never commit"
    Never commit `~/.meta/`, `.env` files with keys, or session dumps containing base64 media.

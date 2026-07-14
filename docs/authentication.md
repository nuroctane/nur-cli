# Authentication

Meta CLI can talk to **many providers** — not only the Meta Model API. Sign-in is
usually: pick a provider, then enter its API key (local servers can skip the key).
For selected providers you can also **sign in with a browser** (device code / SSO),
same idea as `hf auth login`, `az login`, or `aws sso login`.

The active provider, endpoint, and default model are stored in
`~/.meta/config.toml`; secrets live only in `~/.meta/auth.json`.

## Get a key (or browser session)

| Provider | API key | Browser / SSO |
|----------|---------|----------------|
| **Meta Model API** (default) | [dev.meta.ai](https://dev.meta.ai/) | — |
| **xAI Grok** | `XAI_API_KEY` | Device code / Grok CLI session |
| **Anthropic Claude** | `ANTHROPIC_API_KEY` | Browser OAuth (Claude-style) or import Claude Code session |
| **Google Antigravity** | Gemini key fallback | `gcloud auth login` browser SSO |
| **Hugging Face** | `HF_TOKEN` | Device code (`hf auth login` style) |
| **Azure OpenAI** | `AZURE_OPENAI_API_KEY` | `az login` / Entra device code |
| **AWS Bedrock** | gateway / bearer | `aws sso login` |
| OpenAI, Gemini, Groq, … | Vendor dashboard | — |
| Ollama, LM Studio, … | Often none (local) | — |

## Log in from the TUI (recommended)

```text
/login
```

What happens:

1. Prior credentials are cleared so you start from a clean slate.
2. A **scrollable, type-to-filter** picker lists **45+ providers** (frontier APIs,
   inference clouds, Chinese labs, OpenAI-compatible routers, local servers).
   Providers with browser sign-in show a 🌐 hint.
3. If the provider supports browser auth, choose:
   - **Sign in with browser** — opens a URL (and may show a short code); approve in the browser; Meta stores tokens and refreshes when needed.
   - **Enter API key** — masked paste (classic path).
   - **Use existing CLI session** — when a Grok or Claude Code login is already on disk.
4. Config is updated: `provider`, `base_url`, and `model` (that provider’s
   default). The HTTP client is **hot-swapped** for the rest of the session.

`/logout` clears the stored key/tokens and blocks further turns until you `/login`
again (environment-variable keys still apply on the next launch).

No key on launch → the login modal opens automatically.

!!! note "Browser / SSO notes"
    Azure, AWS, and Antigravity browser paths shell out to official CLIs (`az`, `aws`, `gcloud`) when installed.
    Set your Azure resource URL or Bedrock region/endpoint in config after login if the
    catalog default is a placeholder. Subscription OAuth is a convenience path — API keys
    always remain available.

## Log in from the command line

CLI login still targets a **Meta Model API** key path (prompt / `--key`):

```bash
meta auth login
meta auth login --key YOUR_KEY   # avoid on shared machines
```

Key is written to `~/.meta/auth.json` and never printed.

To use a non-Meta provider end-to-end, prefer **`/login`** in the TUI so the
provider catalog, endpoint, model, and API style all switch together.

## Via environment variable

```bash
export META_API_KEY="your-key-here"
# or
export MODEL_API_KEY="your-key-here"
```

If a key is found in the environment, Meta CLI can save it to `~/.meta/auth.json`
automatically. Many catalog entries also document a vendor-specific env name
(e.g. `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`) — use those with your shell when
you prefer not to store a key via `/login`.

Self-hosted OpenAI-compatible servers (Ollama, vLLM, LiteLLM, custom gateways):

```bash
export META_BASE_URL="http://localhost:11434/v1"   # overrides config base_url
```

`META_BASE_URL` wins over the catalog default after `/login` and on every startup.

!!! note "Legacy variables"
    `MUSE_API_KEY` is also accepted for backwards compatibility.

!!! warning "Plaintext secrets on disk"
    `~/.meta/auth.json` stores API keys and OAuth access/refresh tokens in
    **plaintext JSON**. On Unix Meta sets mode `0600`. On Windows the file lives
    under your user profile (default NTFS ACLs — not a portable 0600). Never commit
    or share `~/.meta/`. OS keychain storage is a future option, not the default.

## Check auth status

```bash
meta auth status
```

Shows whether credentials are set, plus:

- **provider** (catalog id the secret is tagged to)
- **method** (`api_key` or `oauth / browser`)
- **expires** (relative: `in 42m`, `expired 3m ago`, or `no expiry`)
- **key** fingerprint (first/last 4 chars only — never the full secret)

If `auth.provider` does not match the active config provider, status warns
**mismatch** — run `/login` before chatting so tokens are not sent to the wrong API.

## Log out

```bash
meta auth logout
meta auth logout --revoke   # local delete + best-effort revoke notes (az/aws/gcloud)
```

Removes the stored key from `~/.meta/auth.json` (and any migrated key under
legacy `~/.muse/`). Same effect as TUI `/logout` for the key file. `--revoke`
does not call undocumented token revoke APIs for every vendor; for Azure/AWS/Google
it points you at `az logout` / `aws sso logout` / `gcloud auth revoke`.

---

## Providers & API styles

The catalog lives in code (`src/providers.rs`). Categories include:

| Category | Examples |
|----------|----------|
| Default | **Meta Model API** (`muse-spark-1.1`) |
| Frontier | OpenAI, Anthropic, Google Gemini, xAI Grok, DeepSeek, Mistral, Cohere, … |
| Inference clouds | Groq, Cerebras, Together, Fireworks, DeepInfra, Perplexity, NVIDIA NIM, … |
| Chinese labs | Moonshot (Kimi), Zhipu GLM, Qwen (DashScope), MiniMax, … |
| Aggregators / routers | OpenRouter, OmniRoute, Requesty, Vercel / Cloudflare AI gateways, … |
| Local | Ollama, LM Studio, llama.cpp, vLLM (key often optional) |

Each entry declares:

- **base URL** and a sensible **default model**
- usual **env var** for the key
- **API style**: **Responses** (`POST /responses`) or **Chat Completions**
  (`POST /chat/completions`)

Meta CLI’s agent always speaks an internal Responses-shaped protocol. For Chat
Completions providers, a built-in adapter (`src/api/chat.rs`) translates
requests and replies (including streamed tool-call fragments) so tools and
streaming keep working.

---

## Auth precedence

API key resolution order:

1. `~/.meta/auth.json` (from `meta auth login` or successful `/login`)
2. `META_API_KEY`
3. `MODEL_API_KEY`
4. `MUSE_API_KEY` (legacy)
5. Interactive TUI prompt (opens `/login` when no key is found)

Active **provider id / base URL / model** come from `~/.meta/config.toml`
(written by `/login`).

---

## Where secrets live

| Location | Contents |
|----------|----------|
| `~/.meta/auth.json` | API key **or** OAuth access/refresh tokens (**plaintext**) |
| `~/.meta/config.toml` | `provider`, `base_url`, `model`, … (no secret) |
| Env `META_API_KEY` / `MODEL_API_KEY` | Optional override (never printed in logs) |
| Env `META_BASE_URL` | Optional API base override (self-hosted) |
| `~/.meta/sessions/` | Session metadata (no key) |
| `~/.meta/status.json` | Live token usage (no key) |
| `~/.meta/usage.jsonl` | Per-request usage log (no key) |

!!! warning "Never commit"
    Never commit `~/.meta/`, `.env` files with keys, or session dumps containing base64 media.

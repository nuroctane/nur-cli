# Authentication

NurCLI is multi-provider. Sign-in is usually: pick a provider, then enter its API key
(local servers can skip the key). For selected providers you can also **sign in with a
browser** (device code / SSO), same idea as `hf auth login`, `az login`, or `aws sso login`.

The active provider, endpoint, and default model are stored in
`~/.nur/config.toml`; secrets live only in `~/.nur/auth.json`.

## Get a key (or browser session)

| Provider | API key | Browser / SSO |
|----------|---------|----------------|
| **Meta Model API** | [dev.meta.ai](https://dev.meta.ai/) | - |
| **OpenAI** | `OPENAI_API_KEY` | ChatGPT browser OAuth (Codex backend) or import `~/.codex` |
| **xAI Grok** | `XAI_API_KEY` | Device code / Grok CLI session (cli-chat-proxy) |
| **Kimi Code (kimi.com)** | `KIMI_API_KEY` | Device code / Kimi CLI session |
| **Anthropic Claude** | `ANTHROPIC_API_KEY` | Claude browser OAuth (`claude.com/cai/…`) or import `~/.claude` |
| **Google Gemini** | `GEMINI_API_KEY` | Google Cloud ADC via `gcloud auth login --update-adc` |
| **GitHub Copilot** | `COPILOT_GITHUB_TOKEN` (fine-grained PAT with Copilot Requests) | `gh auth login` (subscription token) |
| **Hugging Face** | `HF_TOKEN` | - |
| **Azure OpenAI** | `AZURE_OPENAI_API_KEY` | `az login` / Entra device code |
| **Amazon Bedrock** | `AWS_BEARER_TOKEN_BEDROCK` | - (AWS SSO credentials require SigV4, which this route does not implement) |
| **GitHub Models** | GitHub PAT (`models:read`) | `gh auth login` browser SSO |
| Gemini, Groq, … | Vendor dashboard | - |
| **Poolside** | `POOLSIDE_API_KEY` | Free developer key at [platform.poolside.ai](https://platform.poolside.ai/) → API Keys |
| OpenCode, Vercel AI Gateway, GitHub Models, Helicone, … | Gateway key | - |
| Baseten, Friendli, Chutes, Venice, Writer, Upstage, … | Vendor dashboard | - |
| Ollama, LM Studio, … | Often none (local) | - |

## Log in from the TUI (recommended)

```text
/login
```

What happens:

1. Prior credentials are cleared so you start from a clean slate.
2. A **scrollable, type-to-filter** picker lists **61 providers** (frontier APIs,
   inference clouds, Chinese labs, OpenAI-compatible routers, local servers).
   Providers with browser sign-in show a 🌐 hint.
3. If the provider supports browser auth, choose:
   - **Sign in with browser**: opens a URL (and may show a short code); approve in the browser; NurCLI stores tokens and refreshes when needed.
   - **Enter API key**: masked paste (classic path).
4. Config is updated: `provider`, `base_url`, and `model` (that provider’s
   default). The HTTP client is **hot-swapped** for the rest of the session.

After browser sign-in, `/model` queries that provider with the OAuth credential and
shows only models the account can use.

NurCLI resolves the current OAuth token before every model or inference request, keeps
the active and per-provider session stores synchronized after token rotation, and
forces one refresh/retry if a provider rejects an access token early.

## Supported browser and official-CLI sign-in

NurCLI offers an end-to-end browser or official-CLI credential flow for exactly
these providers:

| Provider | Browser flow | Import existing CLI session | OAuth inference host |
|----------|--------------|-----------------------------|----------------------|
| **OpenAI** | Loopback PKCE (Codex client) | `~/.codex` | `chatgpt.com/backend-api/codex` |
| **xAI** | Device code | `~/.grok` | `cli-chat-proxy.grok.com` (+ Grok CLI version headers) |
| **Anthropic** | Loopback PKCE (Claude Code client) | `~/.claude` | `api.anthropic.com` (Bearer + `oauth-2025-04-20` beta) |
| **Kimi** | Device code | `~/.kimi` | `api.kimi.com/coding/v1` |
| **Google Gemini** | Google Cloud ADC via `gcloud` | ADC store | `generativelanguage.googleapis.com` |
| **Azure OpenAI** | Entra device login via `az` | Azure CLI session | Configured Azure resource |
| **GitHub Models** | GitHub login via `gh` (`models` scope) | GitHub CLI session | `models.github.ai/inference` |
| **GitHub Copilot** | GitHub login via `gh` | GitHub CLI session | `api.githubcopilot.com` |

In `/login`, pick the provider → **Sign in with browser**, or **Use existing CLI
session** when a local first-party login is detected. API keys remain available as a
fallback for every one of them.

Kimi Code API keys work against `https://api.kimi.com/coding/v1`. The separate Moonshot
AI catalog entry remains available for `https://api.moonshot.ai/v1` keys.

`/logout` clears the stored key/tokens and blocks further turns until you `/login`
again (environment-variable keys still apply on the next launch).

No key on launch → the login modal opens automatically.

!!! note "Browser / SSO notes"
    Google, Azure, and GitHub sign-in shell out to official CLIs (`gcloud`, `az`,
    and `gh`) when installed. Set your Azure resource URL in config after login
    because the catalog default is a placeholder. API keys remain available.

    AWS SSO credentials use SigV4 and are not bearer tokens. NurCLI's
    OpenAI-compatible Amazon Bedrock route therefore accepts a Bedrock API key in
    `AWS_BEARER_TOKEN_BEDROCK` or a pasted gateway/API key; it does not advertise
    browser sign-in.

    Google OAuth uses Application Default Credentials and sends the configured Cloud
    project as `x-goog-user-project`. Set it with `gcloud config set project PROJECT_ID`
    (or `GOOGLE_CLOUD_PROJECT`) before browser sign-in.

## Log in from the command line

```bash
nur auth login
nur auth login --key YOUR_KEY   # avoid on shared machines
```

Key is written to `~/.nur/auth.json` and never printed. CLI login stores a key for the
active provider config; it does **not** open the full catalog picker.

For a clean multi-provider switch (catalog + endpoint + model + API style together),
prefer **`/login`** in the TUI.

## Via environment variable

```bash
export NUR_API_KEY="your-key-here"
# or vendor-specific / legacy aliases, e.g.:
export OPENAI_API_KEY="..."
export META_API_KEY="..."   # Meta Model API / older installs
```

If a key is found in the environment, NurCLI can save it to `~/.nur/auth.json`
automatically. Many catalog entries also document a vendor-specific env name
(e.g. `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`). Use those with your shell when
you prefer not to store a key via `/login`.

Self-hosted OpenAI-compatible servers (Ollama, vLLM, LiteLLM, custom gateways):

```bash
export NUR_BASE_URL="http://localhost:11434/v1"   # overrides config base_url
```

`NUR_BASE_URL` (legacy `META_BASE_URL`) wins over the catalog default after `/login` and on every startup.

!!! note "Legacy variables"
    `META_API_KEY`, `MODEL_API_KEY`, and `MUSE_API_KEY` are also accepted for backwards compatibility.

!!! warning "Plaintext secrets on disk"
    `~/.nur/auth.json` stores API keys and OAuth access/refresh tokens in
    **plaintext JSON**. On Unix NurCLI sets mode `0600`. On Windows the file lives
    under your user profile (default NTFS ACLs, not a portable 0600). Never commit
    or share `~/.nur/`. OS keychain storage is a future option, not the default.

## Check auth status

```bash
nur auth status
```

Shows whether credentials are set, plus:

- **provider** (catalog id the secret is tagged to)
- **method** (`api_key` or `oauth / browser`)
- **expires** (relative: `in 42m`, `expired 3m ago`, or `no expiry`)
- **key** fingerprint (first/last 4 chars only; never the full secret)

If `auth.provider` does not match the active config provider, status warns
**mismatch**. Run `/login` before chatting so tokens are not sent to the wrong API.

## Log out

```bash
nur auth logout
nur auth logout --revoke   # local delete + best-effort revoke notes (az/aws/gcloud)
```

Removes the stored key from `~/.nur/auth.json` (and any migrated key under
legacy `~/.muse/`). Same effect as TUI `/logout` for the key file. `--revoke`
does not call undocumented token revoke APIs for every vendor; for Azure/AWS/Google
it points you at `az logout` / `aws sso logout` / `gcloud auth revoke`.

---

## Providers & API styles

The catalog lives in code (`src/providers.rs`). Categories include:

| Category | Examples |
|----------|----------|
| Frontier | OpenAI, Anthropic, Google Gemini, xAI Grok, DeepSeek, Mistral, Cohere, Meta Model API, Inception (Mercury), Writer, Upstage, Poolside (Laguna), … |
| Inference clouds | Groq, Cerebras, Together AI, Fireworks AI, DeepInfra, Perplexity, NVIDIA NIM, Baseten, Friendli, Chutes, Venice AI, … |
| Chinese labs | Kimi Code (kimi.com), Moonshot AI, Z.AI, Qwen (DashScope), MiniMax (minimaxi.com), StepFun (China), … |
| Aggregators / routers | OpenRouter, Requesty, Vercel / Cloudflare AI gateways, OpenCode, GitHub Models, Helicone, AI/ML API, … |
| Local | Ollama, LM Studio, llama.cpp, vLLM (key often optional) |

Each entry declares:

- **base URL** and a sensible **default model**
- usual **env var** for the key
- **API style**:
  - **Responses** (`POST /responses`) — OpenAI/Meta
  - **Chat Completions** (`POST /chat/completions`) — most OpenAI-compatible hosts
  - **Anthropic Messages** (`POST /v1/messages`) — official Claude API. Anthropic is **not** OpenAI Chat Completions.

NurCLI’s agent always speaks an internal Responses-shaped protocol. Adapters
translate for Chat Completions (`src/api/chat.rs`) and Anthropic Messages
(`src/api/anthropic.rs`), including streamed tool calls. Anthropic console API keys
use `x-api-key`.

### Poolside

Poolside serves its own **Laguna** models (M.1 and XS 2.1, both 256K context)
over an OpenAI-compatible Chat Completions API - streaming, tool calling, and
structured output all work through the standard adapter.

| | |
|---|---|
| Base URL | `https://inference.poolside.ai/v1` |
| Default model | `poolside/laguna-m.1` - `/model` lists what your key can reach |
| Key | `POOLSIDE_API_KEY`, or `/login` → **Poolside**. Free developer keys at [platform.poolside.ai](https://platform.poolside.ai/) → API Keys |
| Auth | `Authorization: Bearer <key>` |

**Self-hosted deployments** serve the same API under
`https://<your-domain>/openai/v1` - pick Poolside in `/login` and set that as
the base URL. Laguna is also reachable through OpenRouter
(`poolside/laguna-m.1`) if you would rather bill there.

Privacy tier is **Standard**: Poolside publishes no ZDR or no-training
commitment for API traffic, and nur does not award a tier that is not
documented. If your deployment contract says otherwise, override it - see
[Provider privacy](security.md#provider-privacy--cross-provider-failover) or
`/failover`.

---

## Auth precedence

Credential resolution order:

1. A matching active OAuth session (refreshed automatically before use)
2. For provider-scoped API-key sign-ins, the provider variable (such as
   `OPENAI_API_KEY`), then the matching saved key, then `NUR_API_KEY`
3. `~/.nur/auth.json` (from `nur auth login` or successful `/login`)
4. Interactive TUI prompt (opens `/login` when no key is found)

Legacy generic variables (`META_API_KEY`, `MODEL_API_KEY`, and `MUSE_API_KEY`) are
used only by unscoped/headless resolution. They are never sent to a different
explicitly selected provider.

Active **provider id / base URL / model** come from `~/.nur/config.toml`
(written by `/login`).

---

## Where secrets live

| Location | Contents |
|----------|----------|
| `~/.nur/auth.json` | API key **or** OAuth access/refresh tokens (**plaintext**) |
| `~/.nur/config.toml` | `provider`, `base_url`, `model`, … (no secret) |
| Env `NUR_API_KEY` (legacy `META_API_KEY` / `MODEL_API_KEY`) | Optional override (never printed in logs) |
| Env `NUR_BASE_URL` (legacy `META_BASE_URL`) | Optional API base override (self-hosted) |
| `~/.nur/sessions/` | Session metadata (no key) |
| `~/.nur/status.json` | Live token usage (no key) |
| `~/.nur/usage.jsonl` | Per-request usage log (no key) |

!!! warning "Never commit"
    Never commit `~/.nur/`, `.env` files with keys, or session dumps containing base64 media.

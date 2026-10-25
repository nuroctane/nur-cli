# Authentication

Account summaries, credential refresh and writes run on workers. `/login`,
provider/model selection, key entry and logout stay interactive during skill
indexing or a stalled credential operation. Completion belongs to the current
selection; an older result cannot overwrite a later choice.

NurCLI is multi-provider. Sign-in is usually: pick a provider, then enter its API key
(local servers can skip the key). For selected providers you can also **sign in with a
browser** (device code / SSO), same idea as `hf auth login`, `az login`, or `aws sso login`.

The active provider, endpoint, and default model are stored in
`~/.nur/config.toml`. Active and provider-scoped credentials live in
`~/.nur/auth.json`, `provider_keys.json`, and `provider_sessions.json`.

New sign-ins replace the previous credential method for that provider. OAuth
tokens are refreshed once before being stored, and the same refreshed credential
is used for the active session and provider store. Other providers' credentials,
including the TypeSafe/Jev sidecar, remain intact.

Vendor CLI sign-ins stream their URL and device code while waiting for consent.
For Claude's manual fallback, paste the browser's `code#state` into the prompt.
The code stays in memory for that login attempt and is cleared on cancellation.
If a CLI Claude login fails, retry or use `--key`; it does not open a second
prompt while the manual code reader may still own stdin.

See the [provider sign-in review](provider-signin-review.md) for coverage,
regression checks, and remaining vendor-specific limitations.

## Get a key (or browser session)

| Provider | API key | Browser / SSO |
|----------|---------|----------------|
| **Meta Model API** | `META_API_KEY` or `MODEL_API_KEY` | Muse Code browser (`muse login`) or import `~/.config/muse/auth.json` |
| **OpenAI** | `OPENAI_API_KEY` | ChatGPT browser OAuth (Codex backend) or import `~/.codex` |
| **xAI Grok** | `XAI_API_KEY` | Browser sign-in (PKCE, as Grok Build) / device code over SSH / Grok CLI session (cli-chat-proxy) |
| **Kimi Code (kimi.com)** | `KIMI_API_KEY` | Device code / Kimi CLI session |
| **Anthropic Claude** | `ANTHROPIC_API_KEY` | Claude browser OAuth (`claude.com/cai/…`) or import `~/.claude` |
| **Google Gemini** | `GEMINI_API_KEY` | Google Cloud ADC via `gcloud auth login --update-adc` |
| **Antigravity** | `GEMINI_API_KEY` (fallback) | Google Cloud ADC / Antigravity CLI session (`gcloud auth login --update-adc`) |
| **GitHub Copilot** | `COPILOT_GITHUB_TOKEN` (fine-grained PAT with Copilot Requests) | `gh auth login` (subscription token) |
| **Cursor** | `CURSOR_API_KEY` (optional) | `cursor-agent login` → nur chat via Agent CLI (no key required) |
| **Factory Droid** | `FACTORY_API_KEY` (optional) | `droid` sign-in → nur chat via `droid exec` (no key required; `NUR_DROID_BIN` points at a launcher outside `PATH`) |
| **OpenCode** | `OPENCODE_API_KEY` (covers Zen and Go) | OpenCode Console sign-in (`nur auth login --provider opencode --browser`, a device code in the browser; covers Zen only). `nur auth import --from opencode` copies an OpenCode CLI credential instead |
| **Cline** | `CLINE_API_KEY` (app.cline.bot → Settings → API Keys) | `cline auth cline` (imports `~/.cline/data/settings/providers.json`; nur also refreshes the account token itself) |
| **DeepSeek** | `DEEPSEEK_API_KEY` | DeepSeek Harness (`$DSH_HOME/.credentials.yaml`); no official OAuth |
| **Z.AI / ZCode (GLM)** | `ZAI_API_KEY` | `zcode login` / in-app `/login zai-coding-plan`; nur imports `~/.zcode/v2/credentials.json` (decrypted locally) and `~/.zcode/v2/config.json` |
| **Hugging Face** | `HF_TOKEN` | - |
| **Azure OpenAI** | `AZURE_OPENAI_API_KEY` | `az login` / Entra device code |
| **Amazon Bedrock** | `AWS_BEARER_TOKEN_BEDROCK` | - (AWS SSO credentials require SigV4, which this route does not implement) |
| **GitHub Models** | GitHub PAT (`models:read`) | `gh auth login` browser SSO |
| **Nous Portal** | `NOUS_API_KEY` | Nous Portal device code (`hermes-cli`) or import `~/.hermes/auth.json` |
| Gemini, Groq, … | Vendor dashboard | - |
| **Poolside** | `POOLSIDE_API_KEY` | Free developer key at [platform.poolside.ai](https://platform.poolside.ai/) → API Keys |
| **Paper Instruments** | `PAPERINSTRUMENTS_API_KEY` | Key from [platform.paperinstruments.com](https://platform.paperinstruments.com/) (Ultramarine 1 Flash) |
| **Morph** | `MORPH_API_KEY` | Key from [morphllm.com/dashboard/api-keys](https://morphllm.com/dashboard/api-keys) (GLM-5.3, Kimi K3, DeepSeek V4.1 Flash) |
| OpenCode (Zen and Go), Vercel AI Gateway, GitHub Models, Helicone, … | Gateway key | - |
| Baseten, Friendli, Chutes, Venice, Writer, Upstage, … | Vendor dashboard | - |
| Ollama, LM Studio, … | Often none (local) | - |

## Log in from the TUI (recommended)

```text
/provider             # choose and activate a provider (/login is an alias)
/auth                 # manage credentials for every provider without switching
```

What happens:

1. **Nothing is cleared.** Opening the picker — and backing out of it with `Esc`
   — leaves your current credential exactly as it was. Credentials are replaced
   only at the moment a new one is committed.
2. A **scrollable, type-to-filter** picker lists **71 providers** (frontier APIs,
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

`/login` also pins a **TypeSafe · Jev** sidecar at the top of the picker (id
`typesafe`, its own borders). It is a credential, not a chat model: selecting it
stores a key (`TYPESAFE_API_KEY` / `TYPESAFE_KEY` / `JEV_API_KEY`, `[typesafe]
api_key`, or `nur auth login --provider typesafe`) that gives **every** provider
typed judgments, but it never becomes your active provider and is not a catalog
row. It can also run with **no key at all** against a local engine -
`nur jev start`, or `NUR_JEV_LOCAL_URL=http://127.0.0.1:8788/v1/systemone`, where
a loopback endpoint needs no credential. See [typesafe.md](./typesafe.md) and
[jev-local.md](./jev-local.md).

The pinned row reports live state rather than a static note: the engine the
judgment layer resolves to and where its key sits (`TypeSafe hosted · saved
key`, `Local bridge · loopback :8788`, `SubQ Snap · env SUBQ_API_KEY`, or
`no engine · add a key or nur jev start`). Selecting it opens the **Jev
engines** panel: one row per Jev-class slot (TypeSafe hosted, local bridge, SubQ
Snap, OpenRouter Decisions, Experiential Labs, Liquid AI, plus any catalog engine
selected with `nur jev use --engine`), `●` on the one in use, the key's residence (`env`, `saved key`,
`config.toml`, `file`, `loopback`), and a `[✕]` button wherever Nur owns that
credential. `↵` adds or replaces the key under that slot's own store id (SubQ
and OpenRouter keys never land on `typesafe`); `Del`, `Ctrl+D` or the `✕`
removes every Nur-owned copy for the slot (store entry, `~/.nur/typesafe.key`,
inline `[typesafe] api_key`). Keys that come from the environment and a running
bridge are reported, not removed: the row tells you to `unset` the variable or
run `nur jev stop`. The OpenRouter slot is the same key the OpenRouter chat
provider uses, and the row says so.

**Enclave · security agents** (id `enclave`) is the second pinned sidecar.
Enclave exposes an MCP server rather than a chat endpoint, so its key
(`enc_sk_...`, created under Enclave Settings > MCP) is stored the same scoped
way: `nur auth login --provider enclave`, `/enclave login`, or
`ENCLAVE_MCP_API_KEY` (which wins over a saved key). It never becomes your
active provider. See [enclave.md](./enclave.md).

## Supported browser and official-CLI sign-in

NurCLI offers an end-to-end browser or official-CLI credential flow for exactly
these providers:

| Provider | Browser flow | Import existing CLI session | OAuth inference host |
|----------|--------------|-----------------------------|----------------------|
| **OpenAI** | Loopback PKCE (Codex client) | `$CODEX_HOME` or `~/.codex` | `chatgpt.com/backend-api/codex` |
| **xAI** | Browser (PKCE, loopback `127.0.0.1/callback`); device code over SSH | `$XAI_CONFIG_DIR` or `~/.grok` | `cli-chat-proxy.grok.com` (+ Grok CLI version headers) |
| **Anthropic** | Loopback PKCE (Claude Code client) | `$CLAUDE_CONFIG_DIR` or `~/.claude` | `api.anthropic.com` (Bearer + `oauth-2025-04-20` beta) |
| **Kimi** | Device code | `~/.kimi` | `api.kimi.com/coding/v1` |
| **Google Gemini** | Google Cloud ADC via `gcloud` | ADC store | `generativelanguage.googleapis.com` |
| **Antigravity** | Google Cloud ADC via `gcloud auth login --update-adc` (or the Antigravity/`agy` CLI) | Windows Credential Manager (`gemini:antigravity`) or Gemini/Antigravity CLI files, else gcloud ADC | `cloudcode-pa.googleapis.com` (Cloud Code `v1internal`) |
| **Azure OpenAI** | Entra device login via `az` | Azure CLI session | Configured Azure resource |
| **GitHub Models** | GitHub login via `gh` (`models` scope) | GitHub CLI session | `models.github.ai/inference` |
| **GitHub Copilot** | GitHub login via `gh` | GitHub CLI session | `api.githubcopilot.com` |
| **Cursor** | Cursor login via `cursor-agent` | `$CURSOR_AGENT_HOME` or `~/.cursor` / OS keychain | Chat runs through `cursor-agent -p` (nur tool harness by default); optional `CURSOR_API_KEY` |
| **OpenCode** | OpenCode Console device sign-in (as OpenCode's CLI does it: client `opencode-cli`, refreshed at the Console) | nur's own store; `nur auth import --from opencode` reads `~/.local/share/opencode/auth.json` | Zen with the Console token as a bearer on every route. OpenCode Go takes API keys only: set `OPENCODE_API_KEY` to use Go beside the sign-in, or save the key with `--key` to use it for both |
| **Command Code** | First-party studio sign-in (same flow as `cmd login`: browser + loopback callback; no vendor binary needed) | `~/.commandcode/auth.json` (`$COMMANDCODE_HOME`) + `COMMAND_CODE_API_KEY` / `CMD_API_KEY` | `api.commandcode.ai/provider/v1` (same host as an API key; Claude models auto-route to `/messages`) |
| **Cline** | Cline account sign-in via the vendor CLI (`cline auth cline`, loopback callback on 48801-48811) | `~/.cline/data/settings/providers.json` (`$CLINE_DATA_DIR`) + `CLINE_API_KEY` | `api.cline.bot/api/v1` (same host for both; account tokens carry the `workos:` scheme prefix) |
| **Meta / Muse Code** | `muse login` (or first-run `muse`) | `$MUSE_CONFIG_DIR` or `~/.config/muse/auth.json` | `api.meta.ai/v1` (same host as an API key) |
| **DeepSeek** | DeepSeek Harness `dsh web` → Settings → Models | `$DSH_HOME/.credentials.yaml` (default `~/.dsh`) | `api.deepseek.com/v1` (API key only; no official OAuth) |
| **Z.AI / ZCode (GLM)** | in-app `/login zai-coding-plan` (Electron app) or `zcode login` (npm CLI) | `~/.zcode/v2/credentials.json` (AES-256-GCM `enc:v1:` envelopes nur decrypts on-device) + `~/.zcode/v2/config.json` (`$ZCODE_HOME`) | Coding Plan: `api.z.ai/api/coding/paas/v4`; general key: `api.z.ai/api/paas/v4` |
| **Nous Portal** | Nous Portal device code (`hermes-cli` client id) | `~/.hermes/auth.json` (`providers.nous`; Hermes Agent) | `inference-api.nousresearch.com/v1` |

#### Cursor details (CLI, no pasted key)

Cursor's public Agent host (`api2.cursor.sh`) is **not** OpenAI Chat Completions.
Nur drives Cursor the same way t3code does for auth: `cursor-agent login` (session
in the OS keychain / Agent store). Inference is `cursor-agent -p` with
`--output-format stream-json` (text mode is avoided - redirected pipes can hang
on Windows).

After `/login` → Cursor → browser, nur stores a `cursor-cli-session` marker (not a
scraped secret). `/model` lists models via `cursor-agent models`. Esc cancel kills
the Agent process.

**Windows launch:** nur resolves the versioned `node.exe` + `index.js` under
`%LOCALAPPDATA%\cursor-agent\versions\…` and spawns Node directly. Going through
`cursor-agent.cmd` → PowerShell buffers piped stdout and left the TUI stuck on
"thinking" forever after a successful login. The stdout reader is polled rather
than blocked, so Esc and the provider timeout always terminate the child. Cursor
builds that persist a successful turn while emitting zero redirected stdout are
recovered from the newly created `~/.cursor/projects/*/agent-transcripts` JSONL
transcript. Nur never reuses an older transcript.

**Harness (default):** Cursor runs in `--mode ask`. When nur attaches tools, the
prompt asks the model to emit a fenced `nur-tools` JSON array so nur can run
**its** tools, approvals, plan mode, and **cross-provider subagents** as usual.
Live tokens during that buffered turn go to the thinking cell; final commentary
lands as normal transcript text after the fence is stripped.

**Native Agent (optional):** set `NUR_CURSOR_NATIVE=1` to use Cursor's own tools
with `--force` instead (t3code-style full delegate; nur's tool loop is skipped).

#### Factory Droid details (CLI, no pasted key)

Factory's chat is driven through `droid exec --output-format json`, so the sign-in
that counts is the CLI's own. Run `droid` once and complete the browser login; nur
then detects the sealed `~/.factory/auth.v2.keyring` (or `FACTORY_HOME`) and
`/login` → Factory Droid → browser stores a `factory-cli-session` marker. The
keyring itself is never read or copied - the CLI keeps its secret - and a pasted
`fk-…` key remains the fallback (`FACTORY_API_KEY`).

A missing launcher is reported separately from a missing sign-in. Nur probes
`~/.factory/bin`, `~/.local/bin`, `~/bin`, `%LOCALAPPDATA%\Programs\factory`,
`/usr/local/bin`, `/opt/factory/bin` and `PATH`; `NUR_DROID_BIN` names the binary
when it lives elsewhere. Esc cancel and timeouts terminate the whole `droid`
process tree, not just the shim.

In `/login`, pick the provider → **Sign in with browser**, or **Use existing CLI
session** when a local first-party login is detected. API keys remain available as a
fallback for every one of them.

Other first-party CLIs that do **not** have a browser OAuth nur can copy:

- **Qwen Code** — official Qwen OAuth was discontinued on 2026-04-15. Nur imports
  `DASHSCOPE_API_KEY` / Coding Plan keys from `~/.qwen/settings.json`.
- **MiniMax CLI (`mmx`)** — `mmx auth login --api-key` only. Nur imports a stored
  key from `~/.mmx` / `~/.minimax` when present.

### Importing a vendor CLI or OMP session

Already signed into the vendor's own CLI (Claude Code, Codex, Grok, Kimi,
Cursor, Factory Droid, OpenCode, Antigravity / gcloud, Muse Code, DeepSeek
Harness, ZCode, Nous Portal / Hermes Agent, Command Code, Cline, Qwen Code
settings, MiniMax CLI, Hugging Face) or into [Oh My Pi](https://github.com/can1357/oh-my-pi)?
Copy that session into nur explicitly instead of signing in again:

```bash
nur auth import --from claude     # or codex, grok, kimi, cursor, droid, opencode, ...
nur auth import --from omp        # Oh My Pi (~/.omp/agent/agent.db)
nur auth import --from dsh        # also picks the route when it is unambiguous
```

Imports are **deliberate, not automatic**: a plain `nur auth login` always
starts a fresh browser or key flow and never silently reuses another tool's
session, because most vendor CLIs rotate a *shared* refresh token - whichever
tool refreshes second is signed out. The import prints exactly that warning so
the trade-off is yours to make. Set `fallback_import = true` under `[auth]` in
`~/.nur/config.toml` to restore the old behaviour (a missing credential quietly
falls back to a vendor CLI / OMP session) for every provider at once.

An import copies the tokens into nur's own per-provider store
(`provider_sessions.json` / `provider_keys.json`); after that, resolution hits
nur's store and never touches the vendor CLI again. OMP imports preserve the
credential type exported by OMP: an API key is saved as an API key and uses the
catalog provider's normal API endpoint and headers; an OAuth subscription
session uses that provider's OAuth endpoint and metadata. Legacy OMP imports
are classified and migrated on read so an old API key cannot accidentally take
an OAuth-only route.

If the imported session has gone stale, nur mints a fresh access token from the
same refresh token the CLI stores, exactly as the CLI would have on its next
use - a session merely being a few minutes old no longer sends you to `/login`.

Nur respects the official CLI isolation variables when locating those sessions:
`CODEX_HOME`, `CLAUDE_CONFIG_DIR`, `XAI_CONFIG_DIR`, `CURSOR_AGENT_HOME`,
`ANTIGRAVITY_HOME`, `GCLOUD_CONFIG_DIR`, `MUSE_CONFIG_DIR`, `DSH_HOME`,
`ZCODE_HOME` / `ZCODE_CONFIG_DIR`, and `QWEN_CONFIG_DIR`. This matters for work
profiles and sandboxed CLI installs: a valid login outside the default home
directory is not treated as signed out.

Kimi Code API keys work against `https://api.kimi.com/coding/v1`. The separate Moonshot
AI catalog entry remains available for `https://api.moonshot.ai/v1` keys.

### Client identity on subscription routes

Browser sign-in on a subscription route presents nur **as the vendor's own
first-party client**, because that is what the subscription covers. nur shows
which identity before opening the browser, and the table is the contract:

| Route | The provider's logs show | Billing |
|-------|--------------------------|---------|
| `anthropic` browser sign-in | Anthropic Messages API with the Claude Code client identity | Claude subscription |
| `openai` browser sign-in | Responses API with the Codex client identity and ChatGPT account headers | ChatGPT subscription |
| `google` browser sign-in | Gemini Cloud Code endpoint with the Gemini CLI client metadata | Gemini Code Assist |
| `opencode` / `opencode-go` | OpenCode Zen with an `x-opencode-session` session-affinity header; a Console sign-in authenticates as the OpenCode CLI's client (`opencode-cli`) while requests keep nur's own User-Agent | per token (Zen) or Go subscription |

Every other route sends nur's own requests with your API key. The
session-affinity header pins a session to the same Zen upstream so prompt-cache
prefixes actually hit; it carries a random per-install id, not an account
identifier.

When a subscription route is down and `fallback_providers` names a per-token
provider, nur **skips that fallback** rather than silently billing you, and
says so. Set `failover_allow_paid = true` in `~/.nur/config.toml` to allow the
paid fallback explicitly.

### Keys are kept per provider

Every key you enter is saved twice: as the **active** credential (`auth.json`)
and under its provider in the per-provider store (`provider_keys.json`; browser
sign-ins land in `provider_sessions.json`). Switching provider with `/login`
therefore never strands the one you switched away from — it stays available for
[cross-provider failover](security.md#provider-privacy-cross-provider-failover)
and for **subagents running on a different provider's model**, which need their
own credential to authenticate.

That is why `/login` does not sign you out: signing out is `/logout`'s job, and
only yours to ask for.

`/logout` signs you out of the **active provider**: it clears that provider's
active credential *and* its saved per-provider copies, so "cleared" does not
leave a working key behind. Other providers' saved keys are untouched. Further
turns are blocked until you `/login` again (environment-variable keys still
apply on the next launch).

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
nur auth login --provider muse --browser
nur auth login --provider zcode --browser
nur auth login --provider openai --model gpt-5.6-codex  # also sets the default model
nur auth import --from claude   # copy an existing vendor CLI / OMP session
nur auth login --key YOUR_KEY   # avoid on shared machines
```

`--provider` accepts catalog ids and aliases (`muse`, `zcode`, `dsh`, `glm`,
`opencode-go`, `chatgpt`). A typo is a hard error that names the closest match
rather than a silent fallback to OpenAI. `--browser` runs that vendor's
official harness / CLI sign-in (Muse Code, ZCode, DeepSeek Harness `dsh web`,
plus the existing Codex / Claude / Grok / … flows). `--import` is a deprecated
shortcut for `nur auth import --from <source>`, which copies an
already-signed-in vendor CLI or OMP session into nur's own store.

On a TTY, `nur auth login` without `--key` starts the official sign-in when the
active (or `--provider`) catalog row has one. Pass `--key` to paste an API key
instead.

A successful CLI login writes `~/.nur/auth.json`, the per-provider store, and
updates `~/.nur/config.toml` (`provider`, `base_url`, `model`) the same way
`/login` does.

For a scrollable catalog picker, prefer **`/login`** in the TUI.

## Via environment variable

```bash
export NUR_API_KEY="your-key-here"
# or a vendor-specific key, e.g.:
export OPENAI_API_KEY="..."
export META_API_KEY="..."   # Meta Model API
```

If a key is found in the environment, NurCLI can save it to `~/.nur/auth.json`
automatically. Many catalog entries also document a vendor-specific env name
(e.g. `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`). Use those with your shell when
you prefer not to store a key via `/login`.

Self-hosted OpenAI-compatible servers (Ollama, vLLM, LiteLLM, custom gateways):

```bash
export NUR_BASE_URL="http://localhost:11434/v1"   # overrides config base_url
```

`NUR_BASE_URL` wins over the catalog default after `/login` and on every startup.

`META_API_KEY` remains the provider-specific key for Meta Model API.

### How credentials are stored

`~/.nur/auth.json`, `provider_keys.json` and `provider_sessions.json` hold API
keys and OAuth access/refresh tokens. Two layers protect them:

- **Permissions, everywhere.** On Unix the file is created mode `0600` before a
  byte is written. On Windows an explicit owner-only DACL is set on the
  temporary file, which the atomic rename carries over, so inherited directory
  rights never expose it.
- **Encryption at rest, on Windows.** The bytes are sealed with DPAPI
  (`CryptProtectData`, user scope, with application entropy) before they are
  written, so the file reads as `nur-dpapi:v1:<base64>` and is inert on any
  other Windows account or machine. A file written by an older build stays
  readable and is sealed in place the first time it is read. `nur auth status`
  reports which state the file is actually in.

!!! warning "Permissions do not survive a copy"
    A backup, a sync client or a `tar` of your profile carries the file without
    its ACL. On Windows the DPAPI envelope still protects it; on Unix (no
    keyring-free equivalent) the JSON is plaintext once copied. Never commit or
    share `~/.nur/`.

Set `[auth] encrypt_at_rest = false` when `~/.nur` is synced between machines
or Windows accounts, where the ciphertext could not be decrypted on the other
end. It has no effect outside Windows.

## Rotating keys

A provider can hold several API keys. A conversation sticks to one of them
(chosen by hashing its session id, so the provider's prompt cache keeps
working), and when that key is rate-limited, out of quota or refused it is set
aside and the request goes straight out again on the next free key: no wait,
no failover to another provider. A rate limit sets a key aside for a minute,
an empty quota for an hour, a refused key for the rest of the day. Only when
every key is set aside do the normal capacity retries and the failover chain
take over.

```bash
nur auth pool add --provider morph --key <second-key>   # main key stays as it is
nur auth pool list                                       # counts only, never keys
nur auth pool clear --provider morph
```

Or list extra keys in the plural of the provider's env var, separated by commas
or whitespace: `MORPH_API_KEYS=key2,key3` beside `MORPH_API_KEY=key1`. Stored
pool keys are sealed like the main key store (`~/.nur/provider_key_pools.json`)
and removed with the provider's other credentials. `/auth` shows `+N rotating`.
OAuth sessions are one account each and never rotate.

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

Removes the stored key from `~/.nur/auth.json`. Same effect as TUI `/logout`
for the key file. `--revoke`
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
| Aggregators / routers | OpenRouter, Requesty, Vercel / Cloudflare AI gateways, OpenCode, Cline, GitHub Models, Helicone, AI/ML API, … |
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
[Provider privacy](security.md#provider-privacy-cross-provider-failover) or
`/failover`.

### Cline

The Cline API is the gateway behind the Cline extension, CLI and SDK: **one**
credential reaches a 440+ id catalog (Anthropic, OpenAI, Google, MiniMax, Grok,
DeepSeek and more) over plain OpenAI Chat Completions - streaming, tool calling,
and Cline's own `usage.cost` field all come from the gateway.

| | |
|---|---|
| Base URL | `https://api.cline.bot/api/v1` |
| Model ids | OpenRouter-style `vendor/model` - `anthropic/claude-sonnet-5`, `openai/gpt-6-astra`, `deepseek/deepseek-v4.1-flash`; `/model` lists the live catalog |
| Default model | `anthropic/claude-sonnet-5` (the same model nur's own `anthropic` row defaults to) |
| Key | `CLINE_API_KEY`, or `/login` → **Cline** → account sign-in. Keys are minted at [app.cline.bot](https://app.cline.bot) → Settings → API Keys |
| Auth | `Authorization: Bearer <key>` |

The id is passed through **verbatim** - the `vendor/` prefix is part of Cline's
wire format, not a nur `provider/model` pair, so nothing strips or rewrites it.

Two credential kinds work, and nur imports both:

- **API key** - issued by app.cline.bot and used exactly as issued.
- **Cline account session** - `cline auth cline` signs in through the browser and
  the CLI persists the session to `~/.cline/data/settings/providers.json`
  (`CLINE_DATA_DIR` relocates the data root). nur reads the `cline` entry there
  and presents the access token behind the `workos:` scheme prefix the API
  expects. It can also **refresh the session itself** (`POST
  /api/v1/auth/refresh` needs only the refresh token), so an expired session is
  renewed on the next turn without re-running the CLI.

Privacy tier is **Standard**: Cline's API routes to upstream providers and
publishes no no-train or ZDR commitment for gateway traffic, so nur does not
award a tier that is not documented. Override it in the picker or `/failover` if
your agreement says otherwise.

---

## Auth precedence

Credential resolution order:

1. The selected provider's environment variable, such as `OPENAI_API_KEY`
2. A matching active API-key or OAuth session in `~/.nur/auth.json`
3. That provider's saved key, then saved OAuth session (imports land here)
4. The global `NUR_API_KEY` fallback
5. A supported vendor CLI session, then an OMP credential - **only** when
   `[auth] fallback_import = true` opts in; otherwise `nur auth import --from`
   is the explicit path
6. Interactive TUI prompt (opens `/login` when no credential is found)

`NUR_API_KEY` is the only global override. Provider-specific environment
variables are never sent to a different explicitly selected provider.

Active **provider id / base URL / model** come from `~/.nur/config.toml`
(written by `/login`).

---

## Where secrets live

| Location | Contents |
|----------|----------|
| `~/.nur/auth.json` | API key **or** OAuth access/refresh tokens (DPAPI-sealed on Windows; plaintext 0600 on Unix) |
| `~/.nur/provider_keys.json` | Per-provider failover keys (same protection) |
| `~/.nur/provider_sessions.json` | Per-provider OAuth sessions (same protection) |
| `~/.nur/config.toml` | `provider`, `base_url`, `model`, … (no secret) |
| Env `NUR_API_KEY` | Optional global override (never printed in logs) |
| Env `META_API_KEY` | Optional key for the Meta Model API provider |
| Env `TYPESAFE_API_KEY` | Optional key for the TypeSafe / Jev sidecar (`TYPESAFE_KEY` / `JEV_API_KEY`, or `~/.nur/typesafe.key`; local engines need none) |
| Env `ENCLAVE_MCP_API_KEY` | Optional key for the Enclave sidecar (wins over `nur auth login --provider enclave`) |
| Env `NUR_BASE_URL` | Optional API base override (self-hosted) |
| `~/.nur/sessions/` | Session metadata (no key) |
| `~/.nur/status.json` | Live token usage (no key) |
| `~/.nur/usage.jsonl` | Per-request usage log (no key) |

!!! warning "Never commit"
    Never commit `~/.nur/`, `.env` files with keys, or session dumps containing base64 media.

Jev engine credentials are isolated from chat providers and each other. `nur jev login <engine-id>` uses hidden input and stores the key in Nur's credential store under `jev:<engine-id>`. Alternatively, select an environment variable by name with `--key-env`. Missing selected credentials never fall back to another engine's key. HTTPS is required away from loopback; URLs cannot contain credentials, query parameters, or fragments. [Connection guide](jev-local.md).

`nur ledger` aggregates local usage without importing credentials or requesting provider access. Its optional price refresh downloads only public rates. [Ledger privacy and accounting](ledger.md).

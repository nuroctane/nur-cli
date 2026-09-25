# Provider sign-in review (2026-09-24)

Scope: all 65 catalog entries and the TypeSafe/Jev sidecar, including the shared
API-key path, browser/device/CLI paths, credential replacement, refresh, and
post-login routing. The existing uncommitted Anthropic changes in `src/auth.rs`
and `src/oauth/flows.rs` were incorporated, not discarded.

This is a source and automated-fixture review. It does **not** claim successful
live consent, subscription entitlement, or inference against every provider.
Tests use synthetic tokens, loopback streams, and an isolated fake vendor CLI;
they do not sign out or rewrite the user's real vendor accounts.

## Fixed findings

- **High: duplicate token refresh during save.** Active TUI OAuth login could
  refresh the same input three times; scoped replacement could refresh twice.
  Each now refreshes once and persists that canonical result to the appropriate
  stores. This matters for providers that invalidate rotated refresh tokens.
- **High: login codes hidden until process exit.** GitHub, Google, and Azure
  readers buffered to EOF while the vendor waited for user consent. They now
  stream progress and drain both stdout and stderr. Cursor and Cline readers
  also keep draining after finding their first URL.
- **Medium: device backoff reset.** xAI, Kimi, Nous and the unadvertised HF
  adapter forgot slow_down after one poll. Backoff is cumulative, never shortens
  a server interval, and cancellation/expiry is checked during waits before
  issuing another request. xAI/HF stop probing alternate endpoints after pending.
- **Medium: CLI credential replacement differed from TUI.** API-key login/import
  now removes the old provider OAuth choice; OAuth login clears old saved keys.
  Unrelated providers and the Jev sidecar remain intact.
- **Medium: expired or empty credentials accepted.** Expired access with no
  refresh token now fails sign-in; malformed refresh responses cannot erase the
  prior credential. Empty refresh fields preserve the previous refresh token.
- **Medium: Anthropic manual-entry races.** Codes now stay in memory and belong
  to one login attempt. An early paste is retained, cancellation clears it, and
  simultaneous logins cannot consume each other's codes. Reject empty codes and
  mismatched state. Preserve the preceding Claude Code session's
  request-format, scope, state-length, and expired-import fixes.
- **Low: loopback callback accepted empty/non-GET codes.** These requests no
  longer complete login; valid state-bound consent errors still fail promptly.

## Coverage by catalog entry

For API-key-only entries, review covers validation, provider-scoped storage,
replacement semantics, environment key mapping, and configured routing. It does
not validate every vendor's current dashboard UI or account permissions.

| Provider | Credential entry | Review |
|---|---|---|
| Meta Model API (`meta`) | Browser/CLI + `META_API_KEY` / key | Muse Code CLI/session import; maintained existing bounded vendor process and cancellation paths. |
| OpenAI (`openai`) | Browser/CLI + `OPENAI_API_KEY` / key | Codex loopback PKCE, import, refresh; shared persistence fix prevents repeated refresh-token rotation. |
| OpenAI (Chat Completions) (`openai-cc`) | `OPENAI_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| Anthropic (`anthropic`) | Browser/CLI + `ANTHROPIC_API_KEY` / key | Preserved Claude Code session fixes: JSON grants, 32-byte state, refresh scopes, code#state validation, expired import filtering; fixed early-paste race. |
| Google Gemini (`google`) | Browser/CLI + `GEMINI_API_KEY` / key | gcloud ADC login/import/refresh; stream both pipes before exit. |
| Antigravity (`antigravity`) | Browser/CLI + `GEMINI_API_KEY` / key | Existing CLI/ADC import, PKCE and Code Assist project setup; browser OAuth requires configured Google client credentials. |
| xAI Grok (`xai`) | Browser/CLI + `XAI_API_KEY` / key | Device flow and Grok import; cumulative slow_down, cancellable waits, stop trying alternate token URLs after a pending response. |
| DeepSeek (`deepseek`) | Browser/CLI + `DEEPSEEK_API_KEY` / key | Harness key import/settings flow; API keys remain API keys, no invented OAuth route. |
| Mistral (`mistral`) | `MISTRAL_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| Cohere (`cohere`) | `COHERE_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| AI21 (`ai21`) | `AI21_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| Reka (`reka`) | `REKA_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| Inception (Mercury) (`inception`) | `INCEPTION_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| Writer (Palmyra) (`writer`) | `WRITER_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| Upstage (Solar) (`upstage`) | `UPSTAGE_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| Thinking Machines (`thinkingmachines`) | `TINKER_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| Poolside (`poolside`) | `POOLSIDE_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| Hugging Face (`huggingface`) | `HF_TOKEN` / key | HF access token/import; custom-client device adapter remains unadvertised. |
| Azure OpenAI (`azure`) | Browser/CLI + `AZURE_OPENAI_API_KEY` / key | Entra device flow via az; stream code before exit, disable subscription selector and JSON output during login. |
| Amazon Bedrock (`bedrock`) | `AWS_BEARER_TOKEN_BEDROCK` / key | Bearer/API key only; AWS SSO SigV4 credentials are not advertised as compatible. |
| Groq (`groq`) | `GROQ_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| Cerebras (`cerebras`) | `CEREBRAS_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| Together AI (`together`) | `TOGETHER_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| Fireworks AI (`fireworks`) | `FIREWORKS_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| DeepInfra (`deepinfra`) | `DEEPINFRA_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| NovitaAI (`novita`) | `NOVITA_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| Hyperbolic (`hyperbolic`) | `HYPERBOLIC_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| Nebius Token Factory (`nebius`) | `NEBIUS_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| SambaNova (`sambanova`) | `SAMBANOVA_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| NVIDIA NIM (`nvidia`) | `NVIDIA_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| Perplexity (`perplexity`) | `PERPLEXITY_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| Baseten (`baseten`) | `BASETEN_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| Friendli (`friendli`) | `FRIENDLI_TOKEN` / key | Shared API-key path; catalog and provider-route regression coverage. |
| Chutes (`chutes`) | `CHUTES_API_TOKEN` / key | Shared API-key path; catalog and provider-route regression coverage. |
| Venice AI (`venice`) | `VENICE_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| Kimi Code (kimi.com) (`kimi`) | Browser/CLI + `KIMI_API_KEY` / key | Device identity headers, device flow, import, refresh; cumulative slow_down and cancellable waits. |
| Moonshot AI (`moonshot`) | `MOONSHOT_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| Z.AI (`zhipu`) | Browser/CLI + `ZAI_API_KEY` / key | ZCode CLI/desktop session import and refresh; maintained coding-plan endpoint selection. |
| Alibaba Qwen (DashScope) (`qwen`) | `DASHSCOPE_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| MiniMax (minimaxi.com) (`minimax`) | `MINIMAX_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| StepFun (China) (`stepfun`) | `STEPFUN_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| Baichuan (`baichuan`) | `BAICHUAN_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| OpenRouter (`openrouter`) | `OPENROUTER_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| Command Code (`commandcode`) | Browser/CLI + `COMMAND_CODE_API_KEY` / key | Studio loopback callback/state/origin validation and CLI key import; no change to its callback contract. |
| Cline (`cline`) | Browser/CLI + `CLINE_API_KEY` / key | Vendor login, WorkOS token prefix/import and refresh; retain pipe readers after URL. |
| Requesty (`requesty`) | `REQUESTY_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| Glama (`glama`) | `GLAMA_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| Portkey (`portkey`) | `PORTKEY_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| LiteLLM Proxy (`litellm`) | `LITELLM_API_KEY` / key; key optional | Local/key-optional route; empty key does not imply failed authentication. |
| Vercel AI Gateway (`vercel`) | `AI_GATEWAY_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| Cloudflare AI Gateway (`cloudflare`) | `CLOUDFLARE_API_TOKEN` / key | Shared API-key path; catalog and provider-route regression coverage. |
| Featherless (`featherless`) | `FEATHERLESS_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| NanoGPT (`nano-gpt`) | `NANOGPT_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| OpenCode (`opencode`) | Browser/CLI + `OPENCODE_API_KEY` / key | Zen/Go-specific credential import; interactive vendor picker still may require a separate terminal. |
| Nous Portal (`nous`) | Browser/CLI + `NOUS_API_KEY` / key | Portal device flow and Hermes import; preserve server interval (including >5s), cumulative slow_down and cancellable waits. |
| GitHub Models (`github-models`) | Browser/CLI + `GITHUB_TOKEN` / key | gh browser/device flow with models scope probe; stream code before exit and drain both pipes. |
| GitHub Copilot (`github-copilot`) | Browser/CLI + `COPILOT_GITHUB_TOKEN` / key | gh browser/device flow and re-import refresh; stream code before exit; synthetic CLI validates single refresh and both stores. |
| Cursor (`cursor`) | Browser/CLI + `CURSOR_API_KEY` / key | Agent CLI/keychain import and login; retain pipe readers after URL; credential remains an API-key import. |
| Helicone AI Gateway (`helicone`) | `HELICONE_API_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| AI/ML API (`aimlapi`) | `AIMLAPI_KEY` / key | Shared API-key path; catalog and provider-route regression coverage. |
| Ollama (local) (`ollama`) | `OLLAMA_API_KEY` / key; key optional | Local/key-optional route; empty key does not imply failed authentication. |
| LM Studio (local) (`lmstudio`) | `LMSTUDIO_API_KEY` / key; key optional | Local/key-optional route; empty key does not imply failed authentication. |
| llama.cpp (local) (`llamacpp`) | `LLAMACPP_API_KEY` / key; key optional | Local/key-optional route; empty key does not imply failed authentication. |
| vLLM (local) (`vllm`) | `VLLM_API_KEY` / key; key optional | Local/key-optional route; empty key does not imply failed authentication. |
| Jan (local) (`jan`) | `JAN_API_KEY` / key; key optional | Local/key-optional route; empty key does not imply failed authentication. |

TypeSafe/Jev is outside the 65-provider chat catalog. Its key is stored without
changing the active chat provider; replacement tests preserve that key.

## Contract references

Validation: `cargo test --bin nur` passed **1,058 tests**, with 17 ignored and
zero failures. The regression cases cover live device-code streaming before
process exit, continued pipe draining, ANSI-wrapped Azure codes, cumulative
polling backoff/cancellation, isolated credential replacement and single refresh,
malformed refresh responses, expired sessions, and attempt-scoped manual pastes.
`mkdocs build --strict` and `git diff --check` also passed.

- [OAuth device polling and slow_down, RFC 8628 section 3.5](https://www.rfc-editor.org/rfc/rfc8628#section-3.5)
- [GitHub CLI browser/device sign-in](https://cli.github.com/manual/gh_auth_login)
- [Google Cloud CLI login and ADC update](https://cloud.google.com/sdk/gcloud/reference/auth/login)
- [Azure CLI interactive/device sign-in](https://learn.microsoft.com/en-us/cli/azure/authenticate-azure-cli-interactively)

## Remaining boundaries

- Vendor session/keychain formats and OAuth app registrations can change; live
  account verification is still needed when a provider changes its contract.
- OpenCode's interactive picker can require completing login in its own terminal.
- Antigravity needs a configured OAuth client for a new browser flow, or a usable
  imported CLI/ADC session. Bedrock SSO is not a bearer-token sign-in.
- The Claude manual CLI fallback uses a stdin reader while the OAuth worker waits.
  Since a blocked stdin reader cannot be interrupted portably, failed Claude CLI
  login returns from the command rather than starting a competing API-key prompt.
  Retry explicitly or use `--key`; the TUI can cancel and retry normally.

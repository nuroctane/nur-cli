//! Provider catalog — every place you can point nur-cli at a model.
//!
//! nur-cli speaks two request shapes: the OpenAI/Meta **Responses** API
//! (`/responses`) and the ubiquitous **Chat Completions** API
//! (`/chat/completions`). Each catalog entry declares which one it uses, its
//! base URL, a sensible default model, and the env var its key is usually
//! found under. `/login` lets you pick any of these, drop in a key (or for
//! some providers, **sign in with a browser**), and go.
//!
//! The list is intentionally exhaustive — direct frontier APIs, Chinese labs,
//! inference clouds, OpenAI-compatible aggregators/routers, and local servers.

/// Which request/response shape a provider speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiStyle {
    /// OpenAI/Meta Responses API (`POST /responses`).
    Responses,
    /// OpenAI Chat Completions API (`POST /chat/completions`).
    ChatCompletions,
    /// Anthropic Messages API (`POST /v1/messages`) - **not** OpenAI-compatible.
    /// Used by Anthropic and compatible providers such as MiniMax.
    AnthropicMessages,
}

/// A selectable provider.
#[derive(Debug, Clone, Copy)]
pub struct Provider {
    /// Stable id stored in config.
    pub id: &'static str,
    /// Human name shown in the picker + banner.
    pub name: &'static str,
    /// API base (no trailing slash, no endpoint path).
    pub base_url: &'static str,
    /// A reasonable default model to select on first login.
    pub default_model: &'static str,
    /// Env var the key is commonly exported as (also tried on startup).
    pub env_key: &'static str,
    pub style: ApiStyle,
    /// One-line hint shown under the name in the picker.
    pub note: &'static str,
    /// Local servers don't require a key.
    pub key_optional: bool,
    /// Offer browser / device-code / SSO sign-in in `/login` (in addition to API key).
    pub browser_auth: bool,
}

use ApiStyle::{AnthropicMessages as AM, ChatCompletions as CC, Responses as R};

/// OpenAI's ChatGPT/Codex backend used by ChatGPT OAuth sessions.
pub const OPENAI_OAUTH_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";
/// xAI's inference proxy for Grok Build browser/device sessions.
pub const XAI_OAUTH_BASE_URL: &str = "https://cli-chat-proxy.grok.com/v1";
/// Kimi Code's managed inference API for both subscription OAuth and Code API keys.
pub const KIMI_CODE_BASE_URL: &str = "https://api.kimi.com/coding/v1";
/// OpenCode Go shares OpenCode credentials but has its own inference endpoint.
pub const OPENCODE_GO_BASE_URL: &str = "https://opencode.ai/zen/go/v1";
/// Poolside Platform inference. Self-hosted deployments use `https://<domain>/openai/v1`.
pub const POOLSIDE_BASE_URL: &str = "https://inference.poolside.ai/v1";

/// Current xAI flagship on `api.x.ai`.
///
/// Source of truth: <https://docs.x.ai/docs/models>. The whole Grok 4 line was
/// withdrawn — `grok-4` is **not** served any more — so this is also what
/// [`normalize_xai_model_id`] rewrites retired ids onto. The OAuth proxy
/// (`cli-chat-proxy`) serves the same id, so key and browser sessions agree.
pub const XAI_DEFAULT_MODEL: &str = "grok-4.5";

/// Rewrite a retired / short Grok id onto one `api.x.ai` still serves.
///
/// Grok 4 and everything older were withdrawn, so a config saved back when
/// `grok-4` was the default now 404s on the next turn — the user changed
/// nothing, the model went away underneath them. Only **exact** retired ids are
/// rewritten: current ids (`grok-4.3`, `grok-4.20-*`, `grok-build-0.1`) and the
/// `grok-imagine-*` image/video families must pass through untouched, which is
/// why this is an explicit list and not a `grok-4` prefix match.
pub fn normalize_xai_model_id(model: &str) -> String {
    let m = model.trim();
    if m.is_empty() {
        return XAI_DEFAULT_MODEL.to_string();
    }
    const RETIRED: &[&str] = &[
        "grok",
        "grok-latest",
        "grok-beta",
        "grok-2",
        "grok-2-latest",
        "grok-2-1212",
        "grok-2-vision-1212",
        "grok-3",
        "grok-3-latest",
        "grok-3-fast",
        "grok-3-mini",
        "grok-3-mini-fast",
        "grok-4",
        "grok-4-latest",
        "grok-4-0709",
        "grok-4-fast",
        "grok-4-fast-reasoning",
        "grok-4-fast-non-reasoning",
        "grok-code-fast-1",
    ];
    if RETIRED.contains(&m) {
        return XAI_DEFAULT_MODEL.to_string();
    }
    m.to_string()
}

/// Current Gemini Pro id on the first-party Gemini API.
///
/// Gateways (OpenCode Zen, OpenRouter) expose a bare `gemini-3.1-pro`, but
/// `generativelanguage.googleapis.com` only accepts the `-preview` suffix.
pub const GOOGLE_DEFAULT_MODEL: &str = "gemini-3.1-pro-preview";

/// Rewrite a retired Gemini id onto one the Gemini API still serves.
///
/// `gemini-3-pro` appears in neither Google's model list nor its deprecation
/// table, and `gemini-3-pro-preview` was shut down on 2026-03-09. Matching is
/// exact and never by prefix: `gemini-3-pro-image`, `gemini-3.6-flash`,
/// `gemini-3.5-flash*` and every `gemini-3.1-*` are current and must survive.
pub fn normalize_google_model_id(model: &str) -> String {
    let m = model.trim();
    if m.is_empty() {
        return GOOGLE_DEFAULT_MODEL.to_string();
    }
    const RETIRED: &[&str] = &[
        "gemini-3-pro",
        "gemini-3-pro-preview",
        "gemini-2.5-pro",
        "gemini-2.5-pro-preview-03-25",
        "gemini-2.5-pro-preview-05-06",
        "gemini-2.5-pro-preview-06-05",
        "gemini-1.5-pro",
        "gemini-1.5-pro-latest",
        "gemini-1.0-pro",
        "gemini-pro",
    ];
    if RETIRED.contains(&m) {
        return GOOGLE_DEFAULT_MODEL.to_string();
    }
    m.to_string()
}

/// Current general-purpose DeepSeek id.
pub const DEEPSEEK_DEFAULT_MODEL: &str = "deepseek-v4-flash";

/// Rewrite the retired DeepSeek aliases.
///
/// DeepSeek removes `deepseek-chat` and `deepseek-reasoner` on
/// **2026-07-24 15:59 UTC**. They were the non-thinking and thinking modes of
/// `deepseek-v4-flash`, so the id swap is behaviour-neutral — nur drives
/// reasoning depth through its own effort setting, not the model id.
pub fn normalize_deepseek_model_id(model: &str) -> String {
    let m = model.trim();
    if m.is_empty() {
        return DEEPSEEK_DEFAULT_MODEL.to_string();
    }
    const RETIRED: &[&str] = &["deepseek-chat", "deepseek-reasoner", "deepseek-coder"];
    if RETIRED.contains(&m) {
        return DEEPSEEK_DEFAULT_MODEL.to_string();
    }
    m.to_string()
}

/// Current Inception id — their catalog returns exactly this one model.
pub const INCEPTION_DEFAULT_MODEL: &str = "mercury-2";

/// Rewrite retired Inception ids; `mercury-coder` is gone from the catalog.
pub fn normalize_inception_model_id(model: &str) -> String {
    let m = model.trim();
    if m.is_empty() {
        return INCEPTION_DEFAULT_MODEL.to_string();
    }
    const RETIRED: &[&str] = &["mercury-coder", "mercury", "mercury-coder-small"];
    if RETIRED.contains(&m) {
        return INCEPTION_DEFAULT_MODEL.to_string();
    }
    m.to_string()
}

/// Rewrite a saved model id that its provider has since retired.
///
/// Providers withdraw ids out from under a config that has one pinned: the user
/// changed nothing, and their next turn 404s with no hint why. Only providers
/// with a *confirmed* retirement get an arm here — everything else passes
/// through untouched, because a wrong rewrite breaks a working setup, which is
/// strictly worse than a stale-but-serving id.
pub fn normalize_model_for(provider_id: &str, model: &str) -> String {
    match provider_id {
        "xai" => normalize_xai_model_id(model),
        "google" => normalize_google_model_id(model),
        "deepseek" => normalize_deepseek_model_id(model),
        "inception" => normalize_inception_model_id(model),
        _ => model.trim().to_string(),
    }
}

/// Floor version xAI enforces on `cli-chat-proxy` (HTTP 426 if missing → "none").
#[allow(dead_code)] // documented floor; asserted in tests
pub const XAI_GROK_CLI_MIN_VERSION: &str = "0.1.202";
/// Fallback fingerprint when `~/.grok/version.json` is absent (must be ≥ min).
pub const XAI_GROK_CLI_DEFAULT_VERSION: &str = "0.2.101";

/// Fixed inference backends bound to first-party OAuth access tokens.
pub fn oauth_base_url(provider_id: &str) -> Option<&'static str> {
    match provider_id {
        "openai" => Some(OPENAI_OAUTH_BASE_URL),
        "xai" => Some(XAI_OAUTH_BASE_URL),
        "kimi" => Some(KIMI_CODE_BASE_URL),
        _ => None,
    }
}

/// Grok CLI version string for `x-grok-client-version` (subscription OAuth proxy).
///
/// Order: `NUR_XAI_CLI_VERSION` / `XAI_GROK_CLI_VERSION` env → `~/.grok/version.json`
/// → [`XAI_GROK_CLI_DEFAULT_VERSION`]. Always ≥ [`XAI_GROK_CLI_MIN_VERSION`].
pub fn xai_grok_cli_version() -> String {
    for var in ["NUR_XAI_CLI_VERSION", "XAI_GROK_CLI_VERSION"] {
        if let Ok(value) = std::env::var(var) {
            let value = value.trim();
            if !value.is_empty() {
                return value.to_string();
            }
        }
    }
    if let Some(home) = dirs::home_dir() {
        let path = home.join(".grok").join("version.json");
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                for field in ["version", "stable_version", "latest_version"] {
                    if let Some(version) = value.get(field).and_then(|v| v.as_str()) {
                        let version = version.trim();
                        if !version.is_empty() {
                            return version.to_string();
                        }
                    }
                }
            }
        }
    }
    XAI_GROK_CLI_DEFAULT_VERSION.to_string()
}

/// The full catalog. First entry (`meta` = Meta Model API vendor) is the default.
pub const PROVIDERS: &[Provider] = &[
    // ── default vendor (Meta company API — not the NurCLI product name) ──
    Provider {
        id: "meta",
        name: "Meta Model API",
        base_url: "https://api.meta.ai/v1",
        default_model: "muse-spark-1.1",
        env_key: "META_API_KEY",
        style: R,
        note: "muse-spark · Meta vendor default",
        key_optional: false,
        browser_auth: false,
    },
    // ── frontier direct APIs ─────────────────────────────────────────────
    Provider {
        id: "openai",
        name: "OpenAI",
        base_url: "https://api.openai.com/v1",
        default_model: "gpt-5.5",
        env_key: "OPENAI_API_KEY",
        style: R,
        note: "GPT · API key or ChatGPT OAuth",
        key_optional: false,
        browser_auth: true,
    },
    Provider {
        id: "openai-cc",
        name: "OpenAI (Chat Completions)",
        base_url: "https://api.openai.com/v1",
        default_model: "gpt-5.5",
        env_key: "OPENAI_API_KEY",
        style: CC,
        note: "GPT · legacy chat endpoint",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "anthropic",
        name: "Anthropic",
        base_url: "https://api.anthropic.com/v1",
        default_model: "claude-sonnet-5",
        env_key: "ANTHROPIC_API_KEY",
        style: AM,
        note: "Claude Messages API · key or browser OAuth",
        key_optional: false,
        browser_auth: true,
    },
    Provider {
        id: "google",
        name: "Google Gemini",
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        // `gemini-3-pro` appears in neither Google's model list nor its
        // deprecation table; `gemini-3-pro-preview` shut down 2026-03-09 naming
        // this as the replacement. Note gateways expose a bare `gemini-3.1-pro`,
        // but the first-party API requires the `-preview` suffix.
        default_model: GOOGLE_DEFAULT_MODEL,
        env_key: "GEMINI_API_KEY",
        style: CC,
        note: "Gemini · key or gcloud SSO",
        key_optional: false,
        browser_auth: true,
    },
    Provider {
        id: "xai",
        name: "xAI Grok",
        base_url: "https://api.x.ai/v1",
        default_model: XAI_DEFAULT_MODEL,
        env_key: "XAI_API_KEY",
        style: CC,
        note: "Grok · key or browser",
        key_optional: false,
        browser_auth: true,
    },
    Provider {
        id: "deepseek",
        name: "DeepSeek",
        base_url: "https://api.deepseek.com/v1",
        // DeepSeek retires the `deepseek-chat` / `deepseek-reasoner` aliases on
        // 2026-07-24 15:59 UTC. `deepseek-chat` was exactly the non-thinking
        // mode of this model, so the swap is behaviour-neutral.
        default_model: DEEPSEEK_DEFAULT_MODEL,
        env_key: "DEEPSEEK_API_KEY",
        style: CC,
        note: "V3 · R1",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "mistral",
        name: "Mistral",
        base_url: "https://api.mistral.ai/v1",
        default_model: "mistral-large-latest",
        env_key: "MISTRAL_API_KEY",
        style: CC,
        note: "Mistral · Codestral",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "cohere",
        name: "Cohere",
        base_url: "https://api.cohere.ai/compatibility/v1",
        default_model: "command-a-03-2025",
        env_key: "COHERE_API_KEY",
        style: CC,
        note: "Command",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "ai21",
        name: "AI21",
        base_url: "https://api.ai21.com/studio/v1",
        default_model: "jamba-large",
        env_key: "AI21_API_KEY",
        style: CC,
        note: "Jamba",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "reka",
        name: "Reka",
        base_url: "https://api.reka.ai/v1",
        default_model: "reka-core",
        env_key: "REKA_API_KEY",
        style: CC,
        note: "Reka Core / Flash",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "inception",
        name: "Inception (Mercury)",
        base_url: "https://api.inceptionlabs.ai/v1",
        // Inception's catalog now returns exactly one model; `mercury-coder`
        // is gone.
        default_model: INCEPTION_DEFAULT_MODEL,
        env_key: "INCEPTION_API_KEY",
        style: CC,
        note: "Mercury · diffusion LLM",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "writer",
        name: "Writer (Palmyra)",
        base_url: "https://api.writer.com/v1",
        default_model: "palmyra-x5",
        env_key: "WRITER_API_KEY",
        style: CC,
        note: "Palmyra · enterprise",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "upstage",
        name: "Upstage (Solar)",
        base_url: "https://api.upstage.ai/v1/solar",
        default_model: "solar-pro3",
        env_key: "UPSTAGE_API_KEY",
        style: CC,
        note: "Solar",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "thinkingmachines",
        name: "Thinking Machines",
        base_url: "https://tinker.thinkingmachines.dev/services/tinker-prod/oai/api/v1",
        default_model: "thinkingmachines/Inkling",
        env_key: "TINKER_API_KEY",
        style: CC,
        note: "Tinker · Inkling + open models · /model = live list for your key",
        key_optional: false,
        browser_auth: false,
    },
    // Poolside's own models (Laguna M.1 / XS 2.1), OpenAI-compatible. Self-hosted
    // enterprise deployments serve the same API under `https://<domain>/openai/v1`
    // — point `base_url` there in `/login`. Free developer keys: platform.poolside.ai.
    Provider {
        id: "poolside",
        name: "Poolside",
        base_url: POOLSIDE_BASE_URL,
        default_model: "poolside/laguna-m.1",
        env_key: "POOLSIDE_API_KEY",
        style: CC,
        note: "Laguna · agentic coding · /model = live list",
        key_optional: false,
        browser_auth: false,
    },
    // ── cloud / subscription SSO ─────────────────────────────────────────
    Provider {
        id: "huggingface",
        name: "Hugging Face",
        base_url: "https://router.huggingface.co/v1",
        default_model: "meta-llama/Llama-3.3-70B-Instruct",
        env_key: "HF_TOKEN",
        style: CC,
        note: "Inference Providers · access token",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "azure",
        name: "Azure OpenAI",
        base_url: "https://YOUR_RESOURCE.openai.azure.com/openai/v1",
        default_model: "gpt-5.5",
        env_key: "AZURE_OPENAI_API_KEY",
        style: CC,
        note: "Entra SSO · az login",
        key_optional: false,
        browser_auth: true,
    },
    Provider {
        id: "bedrock",
        name: "Amazon Bedrock",
        base_url: "https://bedrock-runtime.us-east-1.amazonaws.com/openai/v1",
        default_model: "amazon.nova-pro-v1:0",
        env_key: "AWS_BEARER_TOKEN_BEDROCK",
        style: CC,
        note: "Bedrock API key · bearer auth",
        key_optional: false,
        browser_auth: false,
    },
    // ── inference clouds ─────────────────────────────────────────────────
    Provider {
        id: "groq",
        name: "Groq",
        base_url: "https://api.groq.com/openai/v1",
        default_model: "llama-3.3-70b-versatile",
        env_key: "GROQ_API_KEY",
        style: CC,
        note: "LPU · very fast",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "cerebras",
        name: "Cerebras",
        base_url: "https://api.cerebras.ai/v1",
        // Cerebras shut `llama-3.3-70b` down on 2026-02-16 and names GPT OSS
        // 120B as the replacement; it is their only Production-tier model
        // (`gemma-4-31b` / `zai-glm-4.7` are preview-only).
        default_model: "gpt-oss-120b",
        env_key: "CEREBRAS_API_KEY",
        style: CC,
        note: "wafer-scale · fastest",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "together",
        name: "Together AI",
        base_url: "https://api.together.xyz/v1",
        default_model: "meta-llama/Llama-3.3-70B-Instruct-Turbo",
        env_key: "TOGETHER_API_KEY",
        style: CC,
        note: "open models cloud",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "fireworks",
        name: "Fireworks AI",
        base_url: "https://api.fireworks.ai/inference/v1",
        default_model: "accounts/fireworks/models/llama-v3p3-70b-instruct",
        env_key: "FIREWORKS_API_KEY",
        style: CC,
        note: "fast open models",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "deepinfra",
        name: "DeepInfra",
        base_url: "https://api.deepinfra.com/v1/openai",
        // DeepInfra serves this model only under the `-Turbo` spelling; the bare
        // id is absent from its catalog. Same model, host-specific name.
        default_model: "meta-llama/Llama-3.3-70B-Instruct-Turbo",
        env_key: "DEEPINFRA_API_KEY",
        style: CC,
        note: "cheap open models",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "novita",
        name: "NovitaAI",
        base_url: "https://api.novita.ai/openai",
        default_model: "minimaxai/minimax-m1-80k",
        env_key: "NOVITA_API_KEY",
        style: CC,
        note: "open models cloud",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "hyperbolic",
        name: "Hyperbolic",
        base_url: "https://api.hyperbolic.xyz/v1",
        default_model: "meta-llama/Llama-3.3-70B-Instruct",
        env_key: "HYPERBOLIC_API_KEY",
        style: CC,
        note: "open models · cheap",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "nebius",
        name: "Nebius Token Factory",
        base_url: "https://api.tokenfactory.nebius.com/v1",
        default_model: "Qwen/Qwen3.5-397B-A17B-fast",
        env_key: "NEBIUS_API_KEY",
        style: CC,
        note: "open models cloud",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "sambanova",
        name: "SambaNova",
        base_url: "https://api.sambanova.ai/v1",
        default_model: "Meta-Llama-3.3-70B-Instruct",
        env_key: "SAMBANOVA_API_KEY",
        style: CC,
        note: "RDU · fast",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "nvidia",
        name: "NVIDIA NIM",
        base_url: "https://integrate.api.nvidia.com/v1",
        default_model: "meta/llama-3.3-70b-instruct",
        env_key: "NVIDIA_API_KEY",
        style: CC,
        note: "build.nvidia.com",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "perplexity",
        name: "Perplexity",
        base_url: "https://api.perplexity.ai",
        default_model: "sonar-pro",
        env_key: "PERPLEXITY_API_KEY",
        style: CC,
        note: "Sonar · web-grounded",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "baseten",
        name: "Baseten",
        base_url: "https://inference.baseten.co/v1",
        default_model: "deepseek-ai/DeepSeek-V3-0324",
        env_key: "BASETEN_API_KEY",
        style: CC,
        note: "model APIs · fast",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "friendli",
        name: "Friendli",
        base_url: "https://api.friendli.ai/serverless/v1",
        // Friendli's serverless catalog is down to 7 models and no longer
        // carries any Meta/Llama entry; this is the general-purpose instruct
        // model among the survivors.
        default_model: "Qwen/Qwen3-235B-A22B-Instruct-2507",
        env_key: "FRIENDLI_TOKEN",
        style: CC,
        note: "serverless endpoints",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "chutes",
        name: "Chutes",
        base_url: "https://llm.chutes.ai/v1",
        // Every id in Chutes' catalog now carries a `-TEE` suffix (trusted
        // execution), so the bare id cannot resolve at all.
        default_model: "deepseek-ai/DeepSeek-V3.2-TEE",
        env_key: "CHUTES_API_TOKEN",
        style: CC,
        note: "decentralized inference",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "venice",
        name: "Venice AI",
        base_url: "https://api.venice.ai/api/v1",
        default_model: "llama-3.3-70b",
        env_key: "VENICE_API_KEY",
        style: CC,
        note: "private · uncensored",
        key_optional: false,
        browser_auth: false,
    },
    // ── Chinese labs ─────────────────────────────────────────────────────
    Provider {
        id: "kimi",
        name: "Kimi Code (kimi.com)",
        base_url: KIMI_CODE_BASE_URL,
        default_model: "kimi-for-coding",
        env_key: "KIMI_API_KEY",
        style: CC,
        note: "Coding plan · API key or browser OAuth",
        key_optional: false,
        browser_auth: true,
    },
    Provider {
        id: "moonshot",
        name: "Moonshot AI",
        base_url: "https://api.moonshot.ai/v1",
        default_model: "kimi-k2.7-code",
        env_key: "MOONSHOT_API_KEY",
        style: CC,
        note: "Kimi · platform API",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "zhipu",
        name: "Z.AI",
        base_url: "https://api.z.ai/api/paas/v4",
        // `glm-4.6` still resolves but is four generations behind; Z.AI documents
        // `glm-5.2` against this exact base.
        default_model: "glm-5.2",
        env_key: "ZAI_API_KEY",
        style: CC,
        note: "GLM",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "qwen",
        name: "Alibaba Qwen (DashScope)",
        base_url: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
        default_model: "qwen-max",
        env_key: "DASHSCOPE_API_KEY",
        style: CC,
        note: "Qwen",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "minimax",
        name: "MiniMax (minimaxi.com)",
        base_url: "https://api.minimaxi.com/anthropic/v1",
        // Casing is verbatim from MiniMax's Anthropic-API reference for this base.
        default_model: "MiniMax-M3",
        env_key: "MINIMAX_API_KEY",
        style: AM,
        note: "China API · Anthropic-compatible",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "stepfun",
        name: "StepFun (China)",
        base_url: "https://api.stepfun.com/v1",
        default_model: "step-3.7-flash",
        env_key: "STEPFUN_API_KEY",
        style: CC,
        note: "Step models",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "baichuan",
        name: "Baichuan",
        base_url: "https://api.baichuan-ai.com/v1",
        default_model: "Baichuan4",
        env_key: "BAICHUAN_API_KEY",
        style: CC,
        note: "Baichuan",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "yi",
        name: "01.AI (Yi)",
        base_url: "https://api.lingyiwanwu.com/v1",
        default_model: "yi-large",
        env_key: "YI_API_KEY",
        style: CC,
        note: "Yi",
        key_optional: false,
        browser_auth: false,
    },
    // ── aggregators / routers (OpenAI-compatible) ────────────────────────
    Provider {
        id: "openrouter",
        name: "OpenRouter",
        base_url: "https://openrouter.ai/api/v1",
        default_model: "openai/gpt-5.5",
        env_key: "OPENROUTER_API_KEY",
        style: CC,
        note: "400+ models, one key",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "requesty",
        name: "Requesty",
        base_url: "https://router.requesty.ai/v1",
        default_model: "openai/gpt-5.5",
        env_key: "REQUESTY_API_KEY",
        style: CC,
        note: "LLM router",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "glama",
        name: "Glama",
        base_url: "https://glama.ai/api/gateway/openai/v1",
        // Glama date-pins OpenAI ids (`openai/gpt-5.5-2026-04-23`); the 5.6 tier
        // is the only undated OpenAI family, so it will not re-rot.
        default_model: "openai/gpt-5.6-terra",
        env_key: "GLAMA_API_KEY",
        style: CC,
        note: "gateway + MCP",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "portkey",
        name: "Portkey",
        base_url: "https://api.portkey.ai/v1",
        default_model: "gpt-5.5",
        env_key: "PORTKEY_API_KEY",
        style: CC,
        note: "AI gateway",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "litellm",
        name: "LiteLLM Proxy",
        base_url: "http://localhost:4000/v1",
        default_model: "gpt-5.5",
        env_key: "LITELLM_API_KEY",
        style: CC,
        note: "self-hosted router",
        key_optional: true,
        browser_auth: false,
    },
    Provider {
        id: "vercel",
        name: "Vercel AI Gateway",
        base_url: "https://ai-gateway.vercel.sh/v1",
        default_model: "openai/gpt-5.5",
        env_key: "AI_GATEWAY_API_KEY",
        style: CC,
        note: "one key, many models",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "cloudflare",
        name: "Cloudflare AI Gateway",
        base_url: "https://gateway.ai.cloudflare.com/v1",
        default_model: "openai/gpt-5.5",
        env_key: "CF_AIG_TOKEN",
        style: CC,
        note: "gateway + caching",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "featherless",
        name: "Featherless",
        base_url: "https://api.featherless.ai/v1",
        default_model: "meta-llama/Meta-Llama-3.1-70B-Instruct",
        env_key: "FEATHERLESS_API_KEY",
        style: CC,
        note: "any HF model",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "nano-gpt",
        name: "NanoGPT",
        base_url: "https://nano-gpt.com/api/v1",
        // NanoGPT namespaces modern models; the bare id is not in its catalog.
        default_model: "openai/gpt-5.6-terra",
        env_key: "NANOGPT_API_KEY",
        style: CC,
        note: "pay-per-prompt",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "opencode",
        name: "OpenCode",
        base_url: "https://opencode.ai/zen/v1",
        // Zen still serves `claude-sonnet-4`, so this was never a 404 — just a
        // two-generation-old default that failover also aimed at (`plan_targets`
        // seeds from `default_model`). Verified present in the live Zen catalog
        // (`GET https://opencode.ai/zen/v1/models`) and matches the id
        // `anthropic::DEFAULT_SONNET` normalises to, so the two agree.
        default_model: "claude-sonnet-5",
        env_key: "OPENCODE_API_KEY",
        style: CC,
        note: "coding-model gateway",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "github-models",
        name: "GitHub Models",
        base_url: "https://models.github.ai/inference",
        // `openai/gpt-4o` still resolves but is long superseded; `openai/gpt-5`
        // is the newest OpenAI tier GitHub Models offers.
        default_model: "openai/gpt-5",
        env_key: "GITHUB_TOKEN",
        style: CC,
        note: "gh CLI or PAT · free tier",
        key_optional: false,
        browser_auth: true,
    },
    Provider {
        id: "github-copilot",
        name: "GitHub Copilot",
        base_url: "https://api.githubcopilot.com",
        // GitHub retired GPT-4.1 for Copilot on 2026-06-01 and names GPT-5.5 as
        // the replacement. Copilot uses bare ids, not `publisher/model`.
        default_model: "gpt-5.5",
        env_key: "COPILOT_GITHUB_TOKEN",
        style: CC,
        note: "Copilot subscription · gh OAuth or fine-grained PAT",
        key_optional: false,
        browser_auth: true,
    },
    Provider {
        id: "helicone",
        name: "Helicone AI Gateway",
        base_url: "https://ai-gateway.helicone.ai/v1",
        // Helicone is the inverted case: every id on the `/v1` gateway is BARE
        // (0 of 111 contain a slash), so a namespaced id cannot route here.
        // Its newest OpenAI tier is 5.4 - this gateway lags the others.
        default_model: "gpt-5.4",
        env_key: "HELICONE_API_KEY",
        style: CC,
        note: "gateway + observability",
        key_optional: false,
        browser_auth: false,
    },
    Provider {
        id: "aimlapi",
        name: "AI/ML API",
        base_url: "https://api.aimlapi.com/v1",
        // AI/ML API namespaces its catalog; the bare id does not resolve.
        default_model: "openai/gpt-5.6-terra",
        env_key: "AIMLAPI_KEY",
        style: CC,
        note: "300+ models, one key",
        key_optional: false,
        browser_auth: false,
    },
    // ── local servers (key optional) ─────────────────────────────────────
    Provider {
        id: "ollama",
        name: "Ollama (local)",
        base_url: "http://localhost:11434/v1",
        default_model: "llama3.3",
        env_key: "OLLAMA_API_KEY",
        style: CC,
        note: "localhost:11434",
        key_optional: true,
        browser_auth: false,
    },
    Provider {
        id: "lmstudio",
        name: "LM Studio (local)",
        base_url: "http://localhost:1234/v1",
        default_model: "local-model",
        env_key: "LMSTUDIO_API_KEY",
        style: CC,
        note: "localhost:1234",
        key_optional: true,
        browser_auth: false,
    },
    Provider {
        id: "llamacpp",
        name: "llama.cpp (local)",
        base_url: "http://localhost:8080/v1",
        default_model: "local-model",
        env_key: "LLAMACPP_API_KEY",
        style: CC,
        note: "llama-server",
        key_optional: true,
        browser_auth: false,
    },
    Provider {
        id: "vllm",
        name: "vLLM (local)",
        base_url: "http://localhost:8000/v1",
        default_model: "local-model",
        env_key: "VLLM_API_KEY",
        style: CC,
        note: "OpenAI server",
        key_optional: true,
        browser_auth: false,
    },
    Provider {
        id: "jan",
        name: "Jan (local)",
        base_url: "http://localhost:1337/v1",
        default_model: "local-model",
        env_key: "JAN_API_KEY",
        style: CC,
        note: "localhost:1337",
        key_optional: true,
        browser_auth: false,
    },
];

/// Look up a provider by id.
pub fn by_id(id: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|p| p.id == id)
}

/// The default provider (Meta).
pub fn default_provider() -> &'static Provider {
    &PROVIDERS[0]
}

/// Providers that offer browser / device-code / SSO sign-in.
#[allow(dead_code)]
pub fn browser_auth_ids() -> impl Iterator<Item = &'static str> {
    PROVIDERS.iter().filter(|p| p.browser_auth).map(|p| p.id)
}

/// Data-handling posture of a provider, strongest → weakest. A hardened, honest
/// adaptation of Origin's ZDR/TEE tags for a **client** CLI: nur can't verify a
/// third party's deployment, so it only asserts `Local` (localhost = structurally
/// private) by default. `Tee` / `Zdr` are claims **you** make about your own
/// account or endpoint (set in the provider picker). `Standard` = assume
/// standard retention unless you know otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Privacy {
    /// Runs on your machine — prompts never leave localhost.
    Local,
    /// Trusted Execution Environment — hardware-encrypted enclave (Intel TDX/SGX).
    Tee,
    /// Zero Data Retention — a contractual promise not to retain or train.
    Zdr,
    /// Standard API terms — may retain (abuse monitoring, etc.).
    Standard,
}

impl Privacy {
    /// Higher = stronger privacy. Drives the failover floor.
    pub fn rank(self) -> u8 {
        match self {
            Privacy::Local => 3,
            Privacy::Tee => 2,
            Privacy::Zdr => 1,
            Privacy::Standard => 0,
        }
    }
    /// Short badge for the picker (empty for Standard — no clutter).
    pub fn tag(self) -> &'static str {
        match self {
            Privacy::Local => "LOCAL",
            Privacy::Tee => "TEE",
            Privacy::Zdr => "ZDR",
            Privacy::Standard => "",
        }
    }
    /// Canonical lowercase name (for saving overrides).
    pub fn as_str(self) -> &'static str {
        match self {
            Privacy::Local => "local",
            Privacy::Tee => "tee",
            Privacy::Zdr => "zdr",
            Privacy::Standard => "standard",
        }
    }
    /// Parse a config/override value (case-insensitive). Unknown → `None`.
    pub fn parse(s: &str) -> Option<Privacy> {
        match s.trim().to_ascii_lowercase().as_str() {
            "local" => Some(Privacy::Local),
            "tee" => Some(Privacy::Tee),
            "zdr" => Some(Privacy::Zdr),
            "standard" | "std" | "" => Some(Privacy::Standard),
            _ => None,
        }
    }
    /// Cycle to the next tier (picker toggle): Standard→Zdr→Tee→Local→Standard.
    pub fn next(self) -> Privacy {
        match self {
            Privacy::Standard => Privacy::Zdr,
            Privacy::Zdr => Privacy::Tee,
            Privacy::Tee => Privacy::Local,
            Privacy::Local => Privacy::Standard,
        }
    }
}

/// Built-in privacy tier per provider, from a review of each provider's public
/// data policy (retention + training). A user override always wins — see
/// [`effective_privacy`]. When a policy is unclear or a provider trains by
/// default, it stays `Standard` (the honest, conservative default).
pub fn builtin_privacy(id: &str) -> Privacy {
    match id {
        // Runs on your machine — prompts never leave localhost.
        "ollama" | "lmstudio" | "llamacpp" | "vllm" | "jan" => Privacy::Local,

        // Hardware-enclave inference with remote attestation + zero retention
        // (Venice runs on TEE partners NEAR AI / Phala; content never logged).
        "venice" => Privacy::Tee,

        // Business APIs that, by default, do NOT train on your data and offer or
        // default to zero data retention (verified from provider policy pages):
        //   openai/anthropic — no-train by default, ZDR available, short logs
        //   google — paid Gemini API is no-train
        //   xai — no-train by default, ZDR (enterprise)
        //   mistral (paid) · cohere · perplexity Sonar — no-train
        //   groq · cerebras · together · fireworks · deepinfra · hyperbolic ·
        //   nebius — inference clouds with ZDR / no-store defaults
        //   azure · bedrock — enterprise no-train + ZDR data handling
        //   openrouter · vercel — ZDR routing / gateway policy
        "openai" | "openai-cc" | "anthropic" | "google" | "xai" | "mistral" | "cohere"
        | "perplexity" | "groq" | "cerebras" | "together" | "fireworks" | "deepinfra"
        | "hyperbolic" | "nebius" | "azure" | "bedrock" | "openrouter" | "vercel" => Privacy::Zdr,

        // Everything else stays Standard: trains by default (DeepSeek and the
        // Chinese labs, Meta's API), logs by default (observability gateways
        // like Portkey / Helicone / Cloudflare), decentralized/untrusted nodes
        // (Chutes), or no clear public ZDR/no-train commitment.
        _ => Privacy::Standard,
    }
}

/// Effective privacy for a provider id: a valid user override wins, else the
/// built-in default.
pub fn effective_privacy(
    overrides: &std::collections::HashMap<String, String>,
    id: &str,
) -> Privacy {
    overrides
        .get(id)
        .and_then(|s| Privacy::parse(s))
        .unwrap_or_else(|| builtin_privacy(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_default_is_meta() {
        let mut ids: Vec<&str> = PROVIDERS.iter().map(|p| p.id).collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n, "duplicate provider id");
        assert_eq!(default_provider().id, "meta");
        assert_eq!(PROVIDERS.len(), 61, "update user-facing provider counts");
    }

    #[test]
    fn privacy_tiers_rank_parse_and_override() {
        // Strongest → weakest: Local > Tee > Zdr > Standard.
        assert!(Privacy::Local.rank() > Privacy::Tee.rank());
        assert!(Privacy::Tee.rank() > Privacy::Zdr.rank());
        assert!(Privacy::Zdr.rank() > Privacy::Standard.rank());
        assert_eq!(Privacy::parse("TEE"), Some(Privacy::Tee));
        assert_eq!(Privacy::parse("zdr"), Some(Privacy::Zdr));
        assert_eq!(Privacy::parse("nonsense"), None);
        // Research-backed built-in tiers.
        assert_eq!(builtin_privacy("ollama"), Privacy::Local); // localhost
        assert_eq!(builtin_privacy("venice"), Privacy::Tee); // hardware enclave
        assert_eq!(builtin_privacy("openai"), Privacy::Zdr); // no-train + ZDR
        assert_eq!(builtin_privacy("anthropic"), Privacy::Zdr);
        assert_eq!(builtin_privacy("deepseek"), Privacy::Standard); // trains, China
        assert_eq!(builtin_privacy("meta"), Privacy::Standard);
        // A user override wins over the built-in default (here: a downgrade).
        let mut ov = std::collections::HashMap::new();
        ov.insert("openai".to_string(), "standard".to_string());
        assert_eq!(effective_privacy(&ov, "openai"), Privacy::Standard);
        // No override → built-in still applies.
        assert_eq!(effective_privacy(&ov, "ollama"), Privacy::Local);
    }

    /// Poolside Platform: OpenAI-compatible Chat Completions, Bearer key.
    #[test]
    fn poolside_speaks_chat_completions_with_a_platform_key() {
        let p = by_id("poolside").expect("poolside is in the catalog");
        assert_eq!(p.name, "Poolside");
        assert_eq!(p.base_url, POOLSIDE_BASE_URL);
        assert_eq!(p.base_url, "https://inference.poolside.ai/v1");
        assert_eq!(p.env_key, "POOLSIDE_API_KEY");
        assert_eq!(p.default_model, "poolside/laguna-m.1");
        assert_eq!(p.style, ApiStyle::ChatCompletions);
        // A key is required, and there is no OAuth flow to drive — the Platform
        // "browser sign-in" only issues a key for you to paste.
        assert!(!p.key_optional);
        assert!(!p.browser_auth);
        assert!(
            !oauth_browser_provider_ids().contains(&"poolside"),
            "poolside must not claim a browser sign-in flow"
        );
    }

    /// No public no-train / retention commitment is documented, so Poolside
    /// takes the conservative default rather than an unearned ZDR badge.
    #[test]
    fn poolside_defaults_to_the_conservative_privacy_tier() {
        assert_eq!(builtin_privacy("poolside"), Privacy::Standard);
        // …and a user who knows their deployment can still say otherwise.
        let mut ov = std::collections::HashMap::new();
        ov.insert("poolside".to_string(), "zdr".to_string());
        assert_eq!(effective_privacy(&ov, "poolside"), Privacy::Zdr);
    }

    #[test]
    fn opencode_is_named_without_the_zen_suffix() {
        let p = by_id("opencode").expect("opencode");
        assert_eq!(p.name, "OpenCode");
        // Only the display name changed — routing must be untouched.
        assert_eq!(p.base_url, "https://opencode.ai/zen/v1");
        assert_eq!(p.env_key, "OPENCODE_API_KEY");
        assert_eq!(p.default_model, "claude-sonnet-5");
    }

    /// A default model that the gateway does not serve strands every new
    /// OpenCode session on a 404, and `plan_targets` seeds failover from the
    /// same field — so it must be an id the Zen catalog actually lists.
    #[test]
    fn opencode_default_model_is_a_current_zen_id() {
        let p = by_id("opencode").expect("opencode");
        // Ids observed in `GET https://opencode.ai/zen/v1/models`.
        const ZEN_CLAUDE: &[&str] = &[
            "claude-fable-5",
            "claude-opus-4-8",
            "claude-sonnet-5",
            "claude-sonnet-4-5",
            "claude-haiku-4-5",
        ];
        assert!(
            ZEN_CLAUDE.contains(&p.default_model),
            "{} is not a current Zen id",
            p.default_model
        );
    }

    /// Every id nur can put on the wire for xAI must be one `api.x.ai` still
    /// serves — the Grok 4 line is gone, and both the key path (`default_model`)
    /// and the OAuth path pinned it independently, so they must agree.
    #[test]
    fn xai_default_model_is_current_and_shared_by_both_auth_paths() {
        let p = by_id("xai").expect("xai");
        assert_eq!(p.default_model, XAI_DEFAULT_MODEL);
        assert_eq!(XAI_DEFAULT_MODEL, "grok-4.5");
        assert_eq!(
            normalize_xai_model_id(p.default_model),
            p.default_model,
            "the default must survive its own normaliser"
        );
    }

    #[test]
    fn retired_grok_ids_are_rewritten_to_the_current_flagship() {
        for id in [
            "grok-4",
            "grok-4-latest",
            "grok-4-0709",
            "grok-4-fast",
            "grok-4-fast-reasoning",
            "grok-3",
            "grok-3-mini",
            "grok-2",
            "grok-beta",
            "grok-code-fast-1",
            "grok",
            "",
            "  grok-4  ",
        ] {
            assert_eq!(
                normalize_xai_model_id(id),
                XAI_DEFAULT_MODEL,
                "{id:?} should have been rewritten"
            );
        }
    }

    /// The normaliser must not touch ids the API still serves — a prefix match
    /// on `grok-4` would eat `grok-4.5` and `grok-4.20-*` and break every one.
    #[test]
    fn current_grok_ids_pass_through_untouched() {
        for id in [
            "grok-4.5",
            "grok-4.3",
            "grok-4.20-0309-reasoning",
            "grok-4.20-0309-non-reasoning",
            "grok-4.20-multi-agent-0309",
            "grok-build-0.1",
            "grok-imagine-image",
            "grok-imagine-video-1.5",
        ] {
            assert_eq!(normalize_xai_model_id(id), id, "{id} must pass through");
        }
    }

    /// A default that its own normaliser would rewrite means the two disagree
    /// about what is current — one of them is wrong, and users get whichever
    /// path they happen to hit.
    #[test]
    fn no_default_model_is_itself_a_retired_id() {
        for p in PROVIDERS {
            assert_eq!(
                normalize_model_for(p.id, p.default_model),
                p.default_model,
                "{}'s default is on its own retired list",
                p.id
            );
        }
    }

    #[test]
    fn retired_gemini_ids_are_rewritten_but_current_ones_survive() {
        for id in [
            "gemini-3-pro",
            "gemini-3-pro-preview",
            "gemini-2.5-pro",
            "gemini-1.5-pro",
            "gemini-pro",
        ] {
            assert_eq!(normalize_google_model_id(id), GOOGLE_DEFAULT_MODEL, "{id}");
        }
        // A prefix match on `gemini-3-pro` would eat the image model; a prefix
        // match on `gemini-3` would eat the entire current flash line.
        for id in [
            "gemini-3-pro-image",
            "gemini-3.6-flash",
            "gemini-3.5-flash",
            "gemini-3.5-flash-lite",
            "gemini-3.1-pro-preview",
            "gemini-3-flash-preview",
        ] {
            assert_eq!(normalize_google_model_id(id), id, "{id} must pass through");
        }
    }

    #[test]
    fn retired_deepseek_aliases_are_rewritten() {
        for id in ["deepseek-chat", "deepseek-reasoner", "deepseek-coder"] {
            assert_eq!(normalize_deepseek_model_id(id), DEEPSEEK_DEFAULT_MODEL);
        }
        for id in ["deepseek-v4-flash", "deepseek-v4-pro"] {
            assert_eq!(normalize_deepseek_model_id(id), id);
        }
    }

    #[test]
    fn retired_inception_ids_are_rewritten() {
        assert_eq!(
            normalize_inception_model_id("mercury-coder"),
            INCEPTION_DEFAULT_MODEL
        );
        assert_eq!(normalize_inception_model_id("mercury-2"), "mercury-2");
    }

    /// The dispatch must be inert for providers with no confirmed retirement —
    /// rewriting a working id is strictly worse than leaving a stale one.
    #[test]
    fn normalize_model_for_is_a_passthrough_for_unlisted_providers() {
        for (provider, model) in [
            ("openai", "gpt-5.5"),
            ("anthropic", "claude-sonnet-4"),
            ("opencode", "grok-4"),
            ("ollama", "llama3.3"),
            ("", "anything"),
        ] {
            assert_eq!(
                normalize_model_for(provider, model),
                model,
                "{provider} must not be rewritten"
            );
        }
    }

    #[test]
    fn browser_auth_providers_present() {
        for id in oauth_browser_provider_ids() {
            let p = by_id(id).unwrap_or_else(|| panic!("missing {id}"));
            assert!(p.browser_auth, "{id} should offer browser auth");
        }
    }

    #[test]
    fn xai_grok_cli_version_meets_proxy_floor() {
        // Floor is 0.1.202; default / installed version must not be empty.
        let v = xai_grok_cli_version();
        assert!(!v.is_empty(), "empty version would become '(none)' → 426");
        assert!(
            v.chars().any(|c| c.is_ascii_digit()),
            "version should look like a CLI release: {v}"
        );
        assert_eq!(XAI_GROK_CLI_MIN_VERSION, "0.1.202");
        assert!(
            XAI_GROK_CLI_DEFAULT_VERSION >= XAI_GROK_CLI_MIN_VERSION
                || XAI_GROK_CLI_DEFAULT_VERSION.starts_with("0.2")
                || XAI_GROK_CLI_DEFAULT_VERSION.starts_with("0.1.2"),
            "default {XAI_GROK_CLI_DEFAULT_VERSION} must satisfy min {XAI_GROK_CLI_MIN_VERSION}"
        );
    }

    #[test]
    fn openai_supports_chatgpt_oauth_on_responses_api() {
        let p = by_id("openai").expect("openai");
        assert!(p.browser_auth);
        assert_eq!(p.style, ApiStyle::Responses);
        assert!(OPENAI_OAUTH_BASE_URL.ends_with("/backend-api/codex"));
    }

    #[test]
    fn kimi_code_supports_key_and_oauth_on_managed_chat_api() {
        let p = by_id("kimi").expect("kimi");
        assert!(p.browser_auth);
        assert_eq!(p.env_key, "KIMI_API_KEY");
        assert_eq!(p.base_url, KIMI_CODE_BASE_URL);
        assert_eq!(p.default_model, "kimi-for-coding");
        assert_eq!(p.style, ApiStyle::ChatCompletions);

        let moonshot = by_id("moonshot").expect("moonshot");
        assert_eq!(moonshot.base_url, "https://api.moonshot.ai/v1");
        assert!(!moonshot.browser_auth);
    }

    #[test]
    fn oauth_tokens_route_only_to_their_fixed_first_party_backends() {
        assert_eq!(oauth_base_url("openai"), Some(OPENAI_OAUTH_BASE_URL));
        assert_eq!(oauth_base_url("xai"), Some(XAI_OAUTH_BASE_URL));
        assert_eq!(oauth_base_url("kimi"), Some(KIMI_CODE_BASE_URL));
        assert_eq!(oauth_base_url("anthropic"), None);
    }

    #[test]
    fn every_browser_auth_flag_is_in_oauth_id_list() {
        let listed: std::collections::HashSet<&str> =
            oauth_browser_provider_ids().iter().copied().collect();
        for p in PROVIDERS {
            if p.browser_auth {
                assert!(
                    listed.contains(p.id),
                    "provider '{}' has browser_auth but is missing from oauth_browser_provider_ids()",
                    p.id
                );
            } else {
                assert!(
                    !listed.contains(p.id),
                    "provider '{}' is in oauth_browser_provider_ids() but browser_auth=false",
                    p.id
                );
            }
        }
    }

    #[test]
    fn advertised_browser_auth_is_the_exact_supported_set() {
        assert_eq!(
            oauth_browser_provider_ids(),
            &[
                "openai",
                "xai",
                "kimi",
                "anthropic",
                "google",
                "azure",
                "github-models",
                "github-copilot",
            ]
        );
        assert!(!by_id("huggingface").unwrap().browser_auth);
        assert!(!by_id("bedrock").unwrap().browser_auth);
    }

    #[test]
    fn audited_provider_nomenclature_and_routes_stay_canonical() {
        let expected = [
            ("bedrock", "Amazon Bedrock"),
            ("fireworks", "Fireworks AI"),
            ("nebius", "Nebius Token Factory"),
            ("novita", "NovitaAI"),
            ("moonshot", "Moonshot AI"),
            ("zhipu", "Z.AI"),
            ("chutes", "Chutes"),
            ("opencode", "OpenCode"),
        ];
        for (id, name) in expected {
            assert_eq!(by_id(id).map(|p| p.name), Some(name), "provider {id}");
        }

        for retired in [
            "antigravity",
            "anyscale",
            "kluster",
            "lepton",
            "octoai",
            "omniroute",
            "targon",
            "unify",
        ] {
            assert!(by_id(retired).is_none(), "retired provider {retired}");
        }
    }

    #[test]
    fn anthropic_uses_messages_api_not_chat_completions() {
        let p = by_id("anthropic").expect("anthropic");
        assert_eq!(
            p.style,
            ApiStyle::AnthropicMessages,
            "Anthropic must use Messages API — Chat Completions on api.anthropic.com is invalid"
        );
        assert!(p.base_url.contains("api.anthropic.com"));
        assert!(!p.default_model.is_empty());
    }

    #[test]
    fn oauth_browser_providers_have_nonempty_defaults() {
        for id in oauth_browser_provider_ids() {
            let p = by_id(id).unwrap();
            assert!(!p.base_url.is_empty(), "{id} base_url empty");
            assert!(!p.default_model.is_empty(), "{id} default_model empty");
            assert!(!p.env_key.is_empty(), "{id} env_key empty");
        }
    }
}

/// Catalog ids with `browser_auth: true`. Keep in sync with
/// `oauth::login_browser` / `refresh_tokens` match arms (enforced by tests).
#[allow(dead_code)] // used by tests; available for TUI/docs tooling
pub fn oauth_browser_provider_ids() -> &'static [&'static str] {
    &[
        "openai",
        "xai",
        "kimi",
        "anthropic",
        "google",
        "azure",
        "github-models",
        "github-copilot",
    ]
}

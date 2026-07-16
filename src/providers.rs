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
    /// Anthropic Messages API (`POST /v1/messages`) — **not** OpenAI-compatible.
    /// Used by the official Claude API (API keys and Claude OAuth tokens).
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

/// The full catalog. First entry (`meta` = Meta Model API vendor) is the default.
pub const PROVIDERS: &[Provider] = &[
    // ── default vendor (Meta company API — not the NurCLI product name) ──
    Provider { id: "meta", name: "Meta Model API", base_url: "https://api.meta.ai/v1", default_model: "muse-spark-1.1", env_key: "META_API_KEY", style: R, note: "muse-spark · Meta vendor default", key_optional: false, browser_auth: false },

    // ── frontier direct APIs ─────────────────────────────────────────────
    Provider { id: "openai", name: "OpenAI", base_url: "https://api.openai.com/v1", default_model: "gpt-5.5", env_key: "OPENAI_API_KEY", style: R, note: "GPT · Responses API", key_optional: false, browser_auth: false },
    Provider { id: "openai-cc", name: "OpenAI (Chat Completions)", base_url: "https://api.openai.com/v1", default_model: "gpt-5.5", env_key: "OPENAI_API_KEY", style: CC, note: "GPT · legacy chat endpoint", key_optional: false, browser_auth: false },
    Provider { id: "anthropic", name: "Anthropic", base_url: "https://api.anthropic.com/v1", default_model: "claude-sonnet-4-20250514", env_key: "ANTHROPIC_API_KEY", style: AM, note: "Claude Messages API · key or browser OAuth", key_optional: false, browser_auth: true },
    Provider { id: "google", name: "Google Gemini", base_url: "https://generativelanguage.googleapis.com/v1beta/openai", default_model: "gemini-3-pro", env_key: "GEMINI_API_KEY", style: CC, note: "Gemini · OpenAI-compat", key_optional: false, browser_auth: false },
    Provider { id: "antigravity", name: "Google Antigravity", base_url: "https://generativelanguage.googleapis.com/v1beta/openai", default_model: "gemini-3-pro", env_key: "GEMINI_API_KEY", style: CC, note: "browser SSO · Code Assist", key_optional: false, browser_auth: true },
    Provider { id: "xai", name: "xAI Grok", base_url: "https://api.x.ai/v1", default_model: "grok-4", env_key: "XAI_API_KEY", style: CC, note: "Grok · key or browser", key_optional: false, browser_auth: true },
    Provider { id: "deepseek", name: "DeepSeek", base_url: "https://api.deepseek.com/v1", default_model: "deepseek-chat", env_key: "DEEPSEEK_API_KEY", style: CC, note: "V3 · R1", key_optional: false, browser_auth: false },
    Provider { id: "mistral", name: "Mistral", base_url: "https://api.mistral.ai/v1", default_model: "mistral-large-latest", env_key: "MISTRAL_API_KEY", style: CC, note: "Mistral · Codestral", key_optional: false, browser_auth: false },
    Provider { id: "cohere", name: "Cohere", base_url: "https://api.cohere.ai/compatibility/v1", default_model: "command-a-03-2025", env_key: "COHERE_API_KEY", style: CC, note: "Command", key_optional: false, browser_auth: false },
    Provider { id: "ai21", name: "AI21", base_url: "https://api.ai21.com/studio/v1", default_model: "jamba-large", env_key: "AI21_API_KEY", style: CC, note: "Jamba", key_optional: false, browser_auth: false },
    Provider { id: "reka", name: "Reka", base_url: "https://api.reka.ai/v1", default_model: "reka-core", env_key: "REKA_API_KEY", style: CC, note: "Reka Core / Flash", key_optional: false, browser_auth: false },
    Provider { id: "inception", name: "Inception (Mercury)", base_url: "https://api.inceptionlabs.ai/v1", default_model: "mercury-coder", env_key: "INCEPTION_API_KEY", style: CC, note: "Mercury · diffusion LLM", key_optional: false, browser_auth: false },
    Provider { id: "writer", name: "Writer (Palmyra)", base_url: "https://api.writer.com/v1", default_model: "palmyra-x5", env_key: "WRITER_API_KEY", style: CC, note: "Palmyra · enterprise", key_optional: false, browser_auth: false },
    Provider { id: "upstage", name: "Upstage (Solar)", base_url: "https://api.upstage.ai/v1", default_model: "solar-pro2", env_key: "UPSTAGE_API_KEY", style: CC, note: "Solar", key_optional: false, browser_auth: false },
    Provider { id: "thinkingmachines", name: "Thinking Machines", base_url: "https://tinker.thinkingmachines.dev/services/tinker-prod/oai/api/v1", default_model: "thinkingmachines/Inkling", env_key: "TINKER_API_KEY", style: CC, note: "Tinker · Inkling + open models · /model lists full catalog", key_optional: false, browser_auth: false },

    // ── cloud / subscription SSO ─────────────────────────────────────────
    Provider { id: "huggingface", name: "Hugging Face", base_url: "https://router.huggingface.co/v1", default_model: "meta-llama/Llama-3.3-70B-Instruct", env_key: "HF_TOKEN", style: CC, note: "HF · key or browser", key_optional: false, browser_auth: true },
    Provider { id: "azure", name: "Azure OpenAI", base_url: "https://YOUR_RESOURCE.openai.azure.com/openai/v1", default_model: "gpt-5.5", env_key: "AZURE_OPENAI_API_KEY", style: CC, note: "Entra SSO · az login", key_optional: false, browser_auth: true },
    Provider { id: "bedrock", name: "AWS Bedrock", base_url: "https://bedrock-runtime.us-east-1.amazonaws.com/openai/v1", default_model: "amazon.nova-pro-v1:0", env_key: "AWS_BEARER_TOKEN_BEDROCK", style: CC, note: "IAM SSO · aws sso login", key_optional: false, browser_auth: true },

    // ── inference clouds ─────────────────────────────────────────────────
    Provider { id: "groq", name: "Groq", base_url: "https://api.groq.com/openai/v1", default_model: "llama-3.3-70b-versatile", env_key: "GROQ_API_KEY", style: CC, note: "LPU · very fast", key_optional: false, browser_auth: false },
    Provider { id: "cerebras", name: "Cerebras", base_url: "https://api.cerebras.ai/v1", default_model: "llama-3.3-70b", env_key: "CEREBRAS_API_KEY", style: CC, note: "wafer-scale · fastest", key_optional: false, browser_auth: false },
    Provider { id: "together", name: "Together AI", base_url: "https://api.together.xyz/v1", default_model: "meta-llama/Llama-3.3-70B-Instruct-Turbo", env_key: "TOGETHER_API_KEY", style: CC, note: "open models cloud", key_optional: false, browser_auth: false },
    Provider { id: "fireworks", name: "Fireworks", base_url: "https://api.fireworks.ai/inference/v1", default_model: "accounts/fireworks/models/llama-v3p3-70b-instruct", env_key: "FIREWORKS_API_KEY", style: CC, note: "fast open models", key_optional: false, browser_auth: false },
    Provider { id: "deepinfra", name: "DeepInfra", base_url: "https://api.deepinfra.com/v1/openai", default_model: "meta-llama/Llama-3.3-70B-Instruct", env_key: "DEEPINFRA_API_KEY", style: CC, note: "cheap open models", key_optional: false, browser_auth: false },
    Provider { id: "novita", name: "Novita AI", base_url: "https://api.novita.ai/v3/openai", default_model: "meta-llama/llama-3.3-70b-instruct", env_key: "NOVITA_API_KEY", style: CC, note: "open models cloud", key_optional: false, browser_auth: false },
    Provider { id: "hyperbolic", name: "Hyperbolic", base_url: "https://api.hyperbolic.xyz/v1", default_model: "meta-llama/Llama-3.3-70B-Instruct", env_key: "HYPERBOLIC_API_KEY", style: CC, note: "open models · cheap", key_optional: false, browser_auth: false },
    Provider { id: "nebius", name: "Nebius AI Studio", base_url: "https://api.studio.nebius.ai/v1", default_model: "meta-llama/Llama-3.3-70B-Instruct", env_key: "NEBIUS_API_KEY", style: CC, note: "open models cloud", key_optional: false, browser_auth: false },
    Provider { id: "sambanova", name: "SambaNova", base_url: "https://api.sambanova.ai/v1", default_model: "Meta-Llama-3.3-70B-Instruct", env_key: "SAMBANOVA_API_KEY", style: CC, note: "RDU · fast", key_optional: false, browser_auth: false },
    Provider { id: "lepton", name: "Lepton AI", base_url: "https://api.lepton.ai/api/v1", default_model: "llama3-3-70b", env_key: "LEPTON_API_KEY", style: CC, note: "inference cloud", key_optional: false, browser_auth: false },
    Provider { id: "anyscale", name: "Anyscale", base_url: "https://api.endpoints.anyscale.com/v1", default_model: "meta-llama/Llama-3.3-70B-Instruct", env_key: "ANYSCALE_API_KEY", style: CC, note: "Ray endpoints", key_optional: false, browser_auth: false },
    Provider { id: "octoai", name: "OctoAI", base_url: "https://text.octoai.run/v1", default_model: "meta-llama-3.3-70b-instruct", env_key: "OCTOAI_API_KEY", style: CC, note: "inference cloud", key_optional: false, browser_auth: false },
    Provider { id: "nvidia", name: "NVIDIA NIM", base_url: "https://integrate.api.nvidia.com/v1", default_model: "meta/llama-3.3-70b-instruct", env_key: "NVIDIA_API_KEY", style: CC, note: "build.nvidia.com", key_optional: false, browser_auth: false },
    Provider { id: "perplexity", name: "Perplexity", base_url: "https://api.perplexity.ai", default_model: "sonar-pro", env_key: "PERPLEXITY_API_KEY", style: CC, note: "Sonar · web-grounded", key_optional: false, browser_auth: false },
    Provider { id: "baseten", name: "Baseten", base_url: "https://inference.baseten.co/v1", default_model: "deepseek-ai/DeepSeek-V3-0324", env_key: "BASETEN_API_KEY", style: CC, note: "model APIs · fast", key_optional: false, browser_auth: false },
    Provider { id: "friendli", name: "Friendli", base_url: "https://api.friendli.ai/serverless/v1", default_model: "meta-llama-3.3-70b-instruct", env_key: "FRIENDLI_TOKEN", style: CC, note: "serverless endpoints", key_optional: false, browser_auth: false },
    Provider { id: "chutes", name: "Chutes.ai", base_url: "https://llm.chutes.ai/v1", default_model: "deepseek-ai/DeepSeek-V3", env_key: "CHUTES_API_TOKEN", style: CC, note: "decentralized inference", key_optional: false, browser_auth: false },
    Provider { id: "venice", name: "Venice AI", base_url: "https://api.venice.ai/api/v1", default_model: "llama-3.3-70b", env_key: "VENICE_API_KEY", style: CC, note: "private · uncensored", key_optional: false, browser_auth: false },

    // ── Chinese labs ─────────────────────────────────────────────────────
    Provider { id: "moonshot", name: "Moonshot (Kimi)", base_url: "https://api.moonshot.ai/v1", default_model: "kimi-k2-0711-preview", env_key: "MOONSHOT_API_KEY", style: CC, note: "Kimi K2", key_optional: false, browser_auth: false },
    Provider { id: "zhipu", name: "Z.AI / Zhipu GLM", base_url: "https://api.z.ai/api/paas/v4", default_model: "glm-4.6", env_key: "ZAI_API_KEY", style: CC, note: "GLM", key_optional: false, browser_auth: false },
    Provider { id: "qwen", name: "Alibaba Qwen (DashScope)", base_url: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1", default_model: "qwen-max", env_key: "DASHSCOPE_API_KEY", style: CC, note: "Qwen", key_optional: false, browser_auth: false },
    Provider { id: "minimax", name: "MiniMax", base_url: "https://api.minimaxi.chat/v1", default_model: "MiniMax-M1", env_key: "MINIMAX_API_KEY", style: CC, note: "MiniMax M1", key_optional: false, browser_auth: false },
    Provider { id: "stepfun", name: "StepFun", base_url: "https://api.stepfun.com/v1", default_model: "step-2-16k", env_key: "STEPFUN_API_KEY", style: CC, note: "Step models", key_optional: false, browser_auth: false },
    Provider { id: "baichuan", name: "Baichuan", base_url: "https://api.baichuan-ai.com/v1", default_model: "Baichuan4", env_key: "BAICHUAN_API_KEY", style: CC, note: "Baichuan", key_optional: false, browser_auth: false },
    Provider { id: "yi", name: "01.AI (Yi)", base_url: "https://api.lingyiwanwu.com/v1", default_model: "yi-large", env_key: "YI_API_KEY", style: CC, note: "Yi", key_optional: false, browser_auth: false },

    // ── aggregators / routers (OpenAI-compatible) ────────────────────────
    Provider { id: "openrouter", name: "OpenRouter", base_url: "https://openrouter.ai/api/v1", default_model: "openai/gpt-5.5", env_key: "OPENROUTER_API_KEY", style: CC, note: "400+ models, one key", key_optional: false, browser_auth: false },
    Provider { id: "omniroute", name: "OmniRoute", base_url: "https://api.omniroute.ai/v1", default_model: "openai/gpt-5.5", env_key: "OMNIROUTE_API_KEY", style: CC, note: "multi-provider router", key_optional: false, browser_auth: false },
    Provider { id: "requesty", name: "Requesty", base_url: "https://router.requesty.ai/v1", default_model: "openai/gpt-5.5", env_key: "REQUESTY_API_KEY", style: CC, note: "LLM router", key_optional: false, browser_auth: false },
    Provider { id: "glama", name: "Glama", base_url: "https://glama.ai/api/gateway/openai/v1", default_model: "openai/gpt-5.5", env_key: "GLAMA_API_KEY", style: CC, note: "gateway + MCP", key_optional: false, browser_auth: false },
    Provider { id: "unify", name: "Unify", base_url: "https://api.unify.ai/v0", default_model: "gpt-5.5@openai", env_key: "UNIFY_API_KEY", style: CC, note: "dynamic routing", key_optional: false, browser_auth: false },
    Provider { id: "portkey", name: "Portkey", base_url: "https://api.portkey.ai/v1", default_model: "gpt-5.5", env_key: "PORTKEY_API_KEY", style: CC, note: "AI gateway", key_optional: false, browser_auth: false },
    Provider { id: "litellm", name: "LiteLLM Proxy", base_url: "http://localhost:4000/v1", default_model: "gpt-5.5", env_key: "LITELLM_API_KEY", style: CC, note: "self-hosted router", key_optional: true, browser_auth: false },
    Provider { id: "vercel", name: "Vercel AI Gateway", base_url: "https://ai-gateway.vercel.sh/v1", default_model: "openai/gpt-5.5", env_key: "AI_GATEWAY_API_KEY", style: CC, note: "one key, many models", key_optional: false, browser_auth: false },
    Provider { id: "cloudflare", name: "Cloudflare AI Gateway", base_url: "https://gateway.ai.cloudflare.com/v1", default_model: "openai/gpt-5.5", env_key: "CF_AIG_TOKEN", style: CC, note: "gateway + caching", key_optional: false, browser_auth: false },
    Provider { id: "kluster", name: "Kluster.ai", base_url: "https://api.kluster.ai/v1", default_model: "klusterai/Meta-Llama-3.3-70B-Instruct-Turbo", env_key: "KLUSTER_API_KEY", style: CC, note: "distributed inference", key_optional: false, browser_auth: false },
    Provider { id: "featherless", name: "Featherless", base_url: "https://api.featherless.ai/v1", default_model: "meta-llama/Meta-Llama-3.1-70B-Instruct", env_key: "FEATHERLESS_API_KEY", style: CC, note: "any HF model", key_optional: false, browser_auth: false },
    Provider { id: "targon", name: "Targon", base_url: "https://api.targon.com/v1", default_model: "deepseek-ai/DeepSeek-V3", env_key: "TARGON_API_KEY", style: CC, note: "Bittensor inference", key_optional: false, browser_auth: false },
    Provider { id: "nano-gpt", name: "NanoGPT", base_url: "https://nano-gpt.com/api/v1", default_model: "gpt-5.5", env_key: "NANOGPT_API_KEY", style: CC, note: "pay-per-prompt", key_optional: false, browser_auth: false },
    Provider { id: "opencode", name: "OpenCode Zen", base_url: "https://opencode.ai/zen/v1", default_model: "claude-sonnet-4", env_key: "OPENCODE_API_KEY", style: CC, note: "coding-model gateway", key_optional: false, browser_auth: false },
    Provider { id: "github-models", name: "GitHub Models", base_url: "https://models.github.ai/inference", default_model: "openai/gpt-4o", env_key: "GITHUB_TOKEN", style: CC, note: "gh CLI or PAT · free tier", key_optional: false, browser_auth: true },
    Provider { id: "helicone", name: "Helicone AI Gateway", base_url: "https://ai-gateway.helicone.ai/v1", default_model: "openai/gpt-5.5", env_key: "HELICONE_API_KEY", style: CC, note: "gateway + observability", key_optional: false, browser_auth: false },
    Provider { id: "aimlapi", name: "AI/ML API", base_url: "https://api.aimlapi.com/v1", default_model: "gpt-5.5", env_key: "AIMLAPI_KEY", style: CC, note: "300+ models, one key", key_optional: false, browser_auth: false },

    // ── local servers (key optional) ─────────────────────────────────────
    Provider { id: "ollama", name: "Ollama (local)", base_url: "http://localhost:11434/v1", default_model: "llama3.3", env_key: "OLLAMA_API_KEY", style: CC, note: "localhost:11434", key_optional: true, browser_auth: false },
    Provider { id: "lmstudio", name: "LM Studio (local)", base_url: "http://localhost:1234/v1", default_model: "local-model", env_key: "LMSTUDIO_API_KEY", style: CC, note: "localhost:1234", key_optional: true, browser_auth: false },
    Provider { id: "llamacpp", name: "llama.cpp (local)", base_url: "http://localhost:8080/v1", default_model: "local-model", env_key: "LLAMACPP_API_KEY", style: CC, note: "llama-server", key_optional: true, browser_auth: false },
    Provider { id: "vllm", name: "vLLM (local)", base_url: "http://localhost:8000/v1", default_model: "local-model", env_key: "VLLM_API_KEY", style: CC, note: "OpenAI server", key_optional: true, browser_auth: false },
    Provider { id: "jan", name: "Jan (local)", base_url: "http://localhost:1337/v1", default_model: "local-model", env_key: "JAN_API_KEY", style: CC, note: "localhost:1337", key_optional: true, browser_auth: false },
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
        //   google/antigravity — paid Gemini API is no-train
        //   xai — no-train by default, ZDR (enterprise)
        //   mistral (paid) · cohere · perplexity Sonar — no-train
        //   groq · cerebras · together · fireworks · deepinfra · hyperbolic ·
        //   nebius — inference clouds with ZDR / no-store defaults
        //   azure · bedrock — enterprise no-train + ZDR data handling
        //   openrouter · vercel — ZDR routing / gateway policy
        "openai" | "openai-cc" | "anthropic" | "google" | "antigravity" | "xai"
        | "mistral" | "cohere" | "perplexity" | "groq" | "cerebras" | "together"
        | "fireworks" | "deepinfra" | "hyperbolic" | "nebius" | "azure" | "bedrock"
        | "openrouter" | "vercel" => Privacy::Zdr,

        // Everything else stays Standard: trains by default (DeepSeek and the
        // Chinese labs, Meta's API), logs by default (observability gateways
        // like Portkey / Helicone / Cloudflare), decentralized/untrusted nodes
        // (Chutes, Targon), or no clear public ZDR/no-train commitment.
        _ => Privacy::Standard,
    }
}

/// Effective privacy for a provider id: a valid user override wins, else the
/// built-in default.
pub fn effective_privacy(overrides: &std::collections::HashMap<String, String>, id: &str) -> Privacy {
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

    #[test]
    fn browser_auth_providers_present() {
        for id in oauth_browser_provider_ids() {
            let p = by_id(id).unwrap_or_else(|| panic!("missing {id}"));
            assert!(p.browser_auth, "{id} should offer browser auth");
        }
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
pub fn oauth_browser_provider_ids() -> &'static [&'static str] {
    &[
        "xai",
        "anthropic",
        "antigravity",
        "huggingface",
        "azure",
        "bedrock",
        "github-models",
    ]
}

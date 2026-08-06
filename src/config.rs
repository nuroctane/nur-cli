use crate::error::{NurError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

pub const DEFAULT_BASE_URL: &str = "https://api.meta.ai/v1";
/// Default model id when provider is Meta Model API (wire format). Override via
/// `/model`, `--model`, config, or `NUR_MODEL`.
pub const DEFAULT_MODEL: &str = "Llama-4-Maverick-17B-128E-Instruct-FP8";
pub const DEFAULT_REASONING: &str = "high";

/// Pretty-print a model id for the splash title / status only.
/// Example: `gpt-5.5` -> `Gpt 5.5`.
pub fn model_display_name(model_id: &str) -> String {
    let s = model_id.trim();
    if s.is_empty() {
        return "model".into();
    }
    if s.contains(' ') {
        return s.to_string();
    }
    s.split(['-', '_'])
        .filter(|p| !p.is_empty())
        .map(|p| {
            // Keep version-like tokens (1.1, v2, 70b) mostly as-is.
            let first = p.chars().next().unwrap_or(' ');
            if first.is_ascii_digit()
                || (p.len() > 1
                    && first == 'v'
                    && p[1..].chars().all(|c| c.is_ascii_digit() || c == '.'))
            {
                p.to_string()
            } else {
                let mut chars = p.chars();
                match chars.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Fallback Meta Model API list prices (USD per 1M tokens) when models.dev has
/// no match. Prefer `crate::pricing::rates_for` for live estimates.
/// Meta rates: <https://dev.meta.ai/docs/getting-started/pricing-rate-limits>
pub const PRICE_INPUT_PER_MTOK: f64 = 1.25;
pub const PRICE_OUTPUT_PER_MTOK: f64 = 4.25;

/// Bumped when defaults change in a way that must rewrite existing config.toml.
/// Schema ≥3: agent rounds are unlimited (`max_turns = 0`) until the user sets
/// a ceiling via `/budget` / `/turns` (or config).
/// Schema ≥6: retired Grok ids (`grok-4` and older) rewritten to the current
/// xAI flagship — the Grok 4 line left `api.x.ai`, so those configs 404.
/// Schema ≥7: same treatment for retired Google / DeepSeek / Inception ids.
/// Schema ≥8: Yi vendor exited LLM work (Mar 2025) — provider removed.
/// Schema ≥9: OpenCode Go routing — bare Go-exclusive ids like `kimi-k3`,
/// `glm-5.2`, `qwen3.7-max` that were previously pinned with the Zen base now
/// auto-migrate to the Go endpoint, and any stored `opencode-go/` prefix is
/// stripped to the canonical bare id.
/// Schema ≥10: opt-in OMP-style `[prewalk]` + `[compaction]` remote endpoint
/// (defaults off - existing installs keep local summarization).
/// Schema >=11: replace the former product-branded Meta default model.
pub const CONFIG_SCHEMA: u32 = 11;

const RETIRED_PROVIDER_IDS: &[&str] = &[
    "anyscale",
    "kluster",
    "lepton",
    "octoai",
    "omniroute",
    "targon",
    "unify",
    "yi",
];

fn is_retired_provider(id: &str) -> bool {
    RETIRED_PROVIDER_IDS.contains(&id)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Config format version. Used to lift obsolete stock defaults once.
    #[serde(default)]
    pub config_schema: u32,
    #[serde(default = "default_model")]
    pub model: String,
    /// Active provider id from the catalog (`crate::providers`). `/login` sets
    /// this along with `base_url`/`model`. Defaults to Meta.
    #[serde(default = "default_provider_id")]
    pub provider: String,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default = "default_reasoning")]
    pub reasoning_effort: String,
    /// Max agent tool/model rounds per user prompt. **`0` = unlimited** (default)
    /// so long-running work is not cut off at an arbitrary wall. Use
    /// `max_session_cost_usd` / `max_session_tokens` if you want a budget stop.
    #[serde(default = "default_max_turns")]
    pub max_turns: u32,
    #[serde(default = "default_true")]
    pub stream: bool,
    /// Model context window (tokens) — used for the ctx% meter in the TUI.
    #[serde(default = "default_context_window")]
    pub context_window: u64,
    /// Max chars of a single tool result kept inline in the model context.
    /// Larger outputs spill to `~/.nur/tool-results/` with a short preview.
    /// `0` = unlimited (legacy behavior).
    #[serde(default = "default_tool_result_max_chars")]
    pub tool_result_max_chars: u64,
    /// Hard stop when session estimated cost reaches this USD amount.
    /// `None` / omitted = unlimited.
    #[serde(default)]
    pub max_session_cost_usd: Option<f64>,
    /// Hard stop when session total_tokens reaches this value.
    /// `None` / omitted = unlimited.
    #[serde(default)]
    pub max_session_tokens: Option<u64>,
    /// When compacting, keep this many recent user turns (messages) after the summary.
    #[serde(default = "default_compact_keep_user_turns")]
    pub compact_keep_user_turns: u32,
    /// When building the compact-summary request, truncate older tool bodies to this many chars.
    /// `0` = leave tool bodies intact for the summarizer.
    #[serde(default = "default_compact_tool_body_max")]
    pub compact_tool_body_max_chars: u64,
    /// Cost-saver: skip PLUR auto-inject and long memory in the system prompt.
    /// Does not disable skill activation (NL / slash still work).
    #[serde(default)]
    pub poor_mode: bool,
    /// When true (default), TUI open background-repairs graphify/plur/ruflo/browser packs.
    /// Set false for a pure binary + chat experience until `nur ecosystem ensure`.
    #[serde(default = "default_true")]
    pub ecosystem_auto_ensure: bool,
    /// When true (default), interactive launch checks GitHub Releases and self-updates
    /// when a newer version is available (TTL-throttled). Opt out with `false` or
    /// env `NUR_SKIP_AUTO_UPDATE=1`. Explicit `nur update` always runs.
    #[serde(default = "default_true")]
    pub auto_update: bool,
    /// Opt-in cross-provider failover chain: catalog provider ids to retry (in
    /// order) when the active provider returns a server error (5xx/429/transport).
    /// Each fallback uses its own env-var key (e.g. `OPENAI_API_KEY`); empty =
    /// no failover. See `crate::api::failover`.
    #[serde(default)]
    pub fallback_providers: Vec<String>,
    /// Fail over only to providers whose privacy tier is >= the active provider's
    /// (see `crate::providers::Privacy`). `true` allows downgrading to a weaker
    /// tier. Default `false` — an outage never silently weakens data privacy.
    #[serde(default)]
    pub failover_allow_downgrade: bool,
    /// Per-provider privacy you assert about your own account/endpoint
    /// (`{provider_id: "local"|"tee"|"zdr"|"standard"}`). Set in the provider
    /// picker; overrides the built-in default.
    #[serde(default)]
    pub provider_privacy: std::collections::HashMap<String, String>,
    /// Opt-in `/fusion` panel: catalog provider ids polled alongside the active
    /// model. `/fusion <question>` asks the active model + each of these the same
    /// question, then the active model synthesizes one answer. Empty = off.
    /// Each panel provider uses its own key (env var or a key saved via the
    /// picker). See `crate::api::fusion`.
    #[serde(default)]
    pub fusion_panel: Vec<String>,
    /// Per-provider base-URL overrides (`{provider_id: "https://…/v1"}`). Lets you
    /// point any provider — **including `openai`** — at an OpenAI-compatible
    /// endpoint (Azure, a local proxy, LiteLLM, a mirror) in API-key mode.
    /// Also honored via env `{PROVIDER}_BASE_URL` (e.g. `OPENAI_BASE_URL`).
    /// OAuth backends keep their fixed inference host and ignore this.
    #[serde(default)]
    pub provider_base_urls: std::collections::HashMap<String, String>,
    /// Runtime TUI theme selected through the theme command.
    ///
    /// `None` means the user has not made an onboarding choice yet. Rendering
    /// still uses Nur Gold until they choose (or skip) the first-run picker.
    #[serde(default)]
    pub theme: Option<String>,
    /// Headroom context compression (inline tool-result compress; default on).
    #[serde(default)]
    pub headroom: HeadroomConfig,
    /// OptMem permanent memory (upstream-pure ~/.optmem; default on).
    #[serde(default)]
    pub optmem: OptmemConfig,
    /// OMP-style prewalk: strong model plans + todos, then switch to a cheap
    /// model at the first edit/write. Default **off**.
    #[serde(default)]
    pub prewalk: PrewalkConfig,
    /// Compaction remote summarization (OMP `compaction.remoteEndpoint` shape).
    /// Default off - local model summarization unchanged.
    #[serde(default)]
    pub compaction: CompactionConfig,
    /// Shepherd-style retained outputs: `write_file` stages under
    /// `~/.nur/proposals/<session>/` instead of the workspace until
    /// `proposal apply`. Default off.
    #[serde(default)]
    pub proposal_mode: bool,
    /// When set, continuous/autonomous mode runs this shell command as a quality
    /// gate before accepting DONE (Prime Agent autonomous gate). Empty = none.
    #[serde(default)]
    pub quality_gate: String,
    /// Auto-register tool results larger than this many chars into the RLM
    /// context store (0 = disabled). Default 8000.
    #[serde(default = "default_context_register_min_chars")]
    pub context_register_min_chars: u64,
    /// Agent-native memory + Connectome continuity (arXiv:2606.24775 + animalabs).
    /// When true (default), inject hierarchical memories into the system prompt
    /// and run localized maintenance on compact.
    #[serde(default = "default_true")]
    pub native_memory: bool,
    /// RLM-style recursion depth for subagents (Prime: configurable depth).
    /// 1 = children only (no grandchildren) — the long-standing default.
    /// 2 = grandchildren allowed, etc. Budget carefully: each level multiplies cost.
    #[serde(default = "default_subagent_depth")]
    pub subagent_depth: u32,
    /// Shepherd-style OS sandbox on Linux (Landlock) for high-risk runs.
    /// Optional; enforced only on Linux (privileged), no-op on Windows/macOS
    /// where nur's proposal/approval model is the safety layer.
    #[serde(default)]
    pub landlock: bool,
    /// Model-based memory extraction (Mem0-class, paper M2). When true, the
    /// active model writes durable first-person memories from assistant turns
    /// (one cheap low-effort call per turn). Off by default (heuristic+explicit
    /// remember is the no-cost path).
    #[serde(default)]
    pub memory_model_extract: bool,
    /// KV-stable compaction (Connectome): when true, compaction rebuilds context
    /// as `[stable summary prefix] + [recent verbatim tail]` and never rewrites
    /// the recent working edge in place, so the provider prompt-cache prefix and
    /// the model's recent computed state survive. Default on.
    #[serde(default = "default_true")]
    pub kv_stable_compact: bool,
    /// Embedding model for the real vector-memory store (m2). Empty → provider
    /// default (`text-embedding-3-small`). When the API path fails or no key it
    /// falls back to the honest local n-gram embedding automatically.
    #[serde(default)]
    pub memory_embed_model: String,
    /// Embedding mode: `auto` (API, fall back to local on error — default),
    /// `api` (require API, error if unavailable), or `local` (always the honest
    /// offline n-gram hash embedding, never calls the provider).
    #[serde(default = "default_embed_mode")]
    pub memory_embed_mode: String,
}

/// `[prewalk]` — plan on the active model, implement on a cheap/smol model.
///
/// Mirrors Oh My Pi `prewalk.enabled` / `--prewalk-into`. Off by default so
/// existing sessions never change mid-turn without an explicit opt-in.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PrewalkConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Target model after the first write/edit once todos exist. Empty → resolve
    /// from `NUR_PREWALK_MODEL` / `OMP_SMOL_MODEL` / OMP `modelRoles.smol`.
    #[serde(default)]
    pub into: String,
}

/// `[compaction]` — optional remote summary endpoint (OMP-compatible).
///
/// Setting `remote_endpoint` opts in directly. Env-only configuration also
/// requires `remote_enabled` or `NUR_COMPACT_REMOTE=1`. Nur accepts either the
/// OMP `{ systemPrompt, prompt }` / `{ summary }` protocol or OpenAI-compatible
/// `/chat/completions`, then falls back locally on any failure.
///
/// Full image **snapcompact** archival stays in OMP; nur ports the remote
/// summarization path only.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompactionConfig {
    /// Use `remote_endpoint` when set. Default false.
    #[serde(default)]
    pub remote_enabled: bool,
    /// POST URL for remote summarization. Also `NUR_COMPACT_REMOTE_ENDPOINT`.
    #[serde(default)]
    pub remote_endpoint: String,
}

/// `[headroom]` — compress large tool results before they enter model context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeadroomConfig {
    /// Default **true**. Set false to disable inline compress.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// `inline` (default) or `off`. Proxy mode reserved.
    #[serde(default = "default_headroom_mode")]
    pub mode: String,
    /// Skip compression below this many chars (default 2000).
    #[serde(default = "default_headroom_min_chars")]
    pub min_chars: u64,
    /// Optional override for Headroom's token-counter model. Empty = use the
    /// active session model at compress time.
    #[serde(default)]
    pub model: String,
}

fn default_headroom_mode() -> String {
    "inline".into()
}
fn default_headroom_min_chars() -> u64 {
    2000
}

impl Default for HeadroomConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: default_headroom_mode(),
            min_chars: default_headroom_min_chars(),
            model: String::new(),
        }
    }
}

/// `[optmem]` — Victor Taelin OptMem under ~/.optmem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptmemConfig {
    /// Default **true**. Wake inject + tool available when enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for OptmemConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Resolve a per-provider base-URL override for API-key mode, if any.
///
/// Order: `{PROVIDER}_BASE_URL` env (e.g. `OPENAI_BASE_URL`, `XAI_BASE_URL`) →
/// the `[provider_base_urls]` config map. Returns a trimmed, slash-normalized
/// URL or `None`. OAuth-forced hosts must not call this.
pub fn provider_base_url_override(cfg: &Config, provider_id: &str) -> Option<String> {
    let env_key = format!(
        "{}_BASE_URL",
        provider_id.to_ascii_uppercase().replace('-', "_")
    );
    if let Ok(u) = std::env::var(&env_key) {
        let u = u.trim().trim_end_matches('/').to_string();
        if !u.is_empty() {
            return Some(u);
        }
    }
    cfg.provider_base_urls.get(provider_id).and_then(|u| {
        let u = u.trim().trim_end_matches('/').to_string();
        (!u.is_empty()).then_some(u)
    })
}

fn default_model() -> String {
    DEFAULT_MODEL.to_string()
}
fn default_provider_id() -> String {
    "meta".to_string()
}

/// Display name of the active provider for the banner / status. Falls back to
/// the catalog default when the id is unknown.
pub fn active_provider_label(cfg: &Config) -> String {
    crate::providers::by_id(&cfg.provider)
        .map(|p| p.name.to_string())
        .unwrap_or_else(|| crate::providers::default_provider().name.to_string())
}

/// Compact label for TUI chrome (input border title). Short enough for a tab.
pub fn active_provider_chrome(cfg: &Config) -> String {
    match cfg.provider.as_str() {
        "meta" => "meta".into(),
        "xai" => "grok".into(),
        "anthropic" => "claude".into(),
        "openai" | "openai-cc" => "openai".into(),
        "google" | "antigravity" => "gemini".into(),
        "openrouter" => "openrouter".into(),
        "ollama" => "ollama".into(),
        "lmstudio" => "lmstudio".into(),
        other => {
            // Prefer catalog short id; fall back to first word of name.
            if other.len() <= 14 {
                other.to_string()
            } else {
                crate::providers::by_id(other)
                    .map(|p| {
                        p.name
                            .split_whitespace()
                            .next()
                            .unwrap_or(other)
                            .to_lowercase()
                    })
                    .unwrap_or_else(|| other.chars().take(12).collect())
            }
        }
    }
}
fn default_base_url() -> String {
    DEFAULT_BASE_URL.to_string()
}
fn default_reasoning() -> String {
    DEFAULT_REASONING.to_string()
}
fn default_max_turns() -> u32 {
    0 // unlimited
}
fn default_context_window() -> u64 {
    1_000_000
}
fn default_tool_result_max_chars() -> u64 {
    12_000
}
fn default_context_register_min_chars() -> u64 {
    8_000
}
fn default_embed_mode() -> String {
    "auto".to_string()
}
fn default_subagent_depth() -> u32 {
    1
}
fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            config_schema: CONFIG_SCHEMA,
            model: default_model(),
            provider: default_provider_id(),
            base_url: default_base_url(),
            reasoning_effort: default_reasoning(),
            max_turns: default_max_turns(),
            stream: true,
            context_window: default_context_window(),
            tool_result_max_chars: default_tool_result_max_chars(),
            max_session_cost_usd: None,
            max_session_tokens: None,
            compact_keep_user_turns: default_compact_keep_user_turns(),
            compact_tool_body_max_chars: default_compact_tool_body_max(),
            poor_mode: false,
            ecosystem_auto_ensure: true,
            auto_update: true,
            fallback_providers: Vec::new(),
            failover_allow_downgrade: false,
            provider_privacy: std::collections::HashMap::new(),
            fusion_panel: Vec::new(),
            provider_base_urls: std::collections::HashMap::new(),
            theme: None,
            headroom: HeadroomConfig::default(),
            optmem: OptmemConfig::default(),
            prewalk: PrewalkConfig::default(),
            compaction: CompactionConfig::default(),
            proposal_mode: false,
            quality_gate: String::new(),
            context_register_min_chars: default_context_register_min_chars(),
            native_memory: true,
            subagent_depth: default_subagent_depth(),
            landlock: false,
            memory_model_extract: false,
            kv_stable_compact: true,
            memory_embed_model: String::new(),
            memory_embed_mode: default_embed_mode(),
        }
    }
}

/// One-shot migration: older installs may still carry a stock turn cap from
/// before unlimited-by-default. Force unlimited agent rounds on upgrade so
/// long runs never die on a leftover config value. After this, only an
/// explicit `/budget turns` / `/turns` / config edit reintroduces a cap.
pub fn migrate_config(cfg: &mut Config) -> bool {
    if cfg.config_schema >= CONFIG_SCHEMA {
        return false;
    }
    if cfg.config_schema < 3 {
        cfg.max_turns = 0;
    }
    if cfg.config_schema < 4 {
        if cfg.provider == "antigravity" {
            cfg.provider = "google".into();
        }
        for id in &mut cfg.fallback_providers {
            if id == "antigravity" {
                *id = "google".into();
            }
        }
        for id in &mut cfg.fusion_panel {
            if id == "antigravity" {
                *id = "google".into();
            }
        }
        if let Some(value) = cfg.provider_privacy.remove("antigravity") {
            cfg.provider_privacy.entry("google".into()).or_insert(value);
        }
    }
    if cfg.config_schema < 5 {
        if is_retired_provider(&cfg.provider) {
            cfg.provider = default_provider_id();
            cfg.base_url = default_base_url();
            cfg.model = default_model();
        }
        cfg.fallback_providers.retain(|id| !is_retired_provider(id));
        cfg.fusion_panel.retain(|id| !is_retired_provider(id));
        cfg.provider_privacy
            .retain(|id, _| !is_retired_provider(id));
    }
    if cfg.config_schema < 7 {
        // Providers retire ids out from under a pinned config: xAI withdrew the
        // Grok 4 line, Google's `gemini-3-pro` is gone, DeepSeek drops the
        // `deepseek-chat` alias on 2026-07-24, Inception dropped
        // `mercury-coder`. Anyone who onboarded on those defaults would 404 on
        // their next turn without having changed a thing.
        cfg.model = crate::providers::normalize_model_for(&cfg.provider, &cfg.model);
    }
    if cfg.config_schema < 8 {
        // Yi (01.AI) exited LLM work Mar 2025 — provider removed. Migrate any
        // leftover `yi` config back to the default so it doesn't 404.
        if is_retired_provider(&cfg.provider) {
            cfg.provider = default_provider_id();
            cfg.base_url = default_base_url();
            cfg.model = default_model();
        }
        cfg.fallback_providers.retain(|id| !is_retired_provider(id));
        cfg.fusion_panel.retain(|id| !is_retired_provider(id));
        cfg.provider_privacy
            .retain(|id, _| !is_retired_provider(id));
    }
    if cfg.config_schema < 9 {
        // OpenCode Go routing: bare Go-exclusive models (kimi-*, glm-*, qwen*,
        // mimo-*, minimax-*, deepseek-*) that were pinned with the Zen base now
        // auto-migrate to the Go endpoint. Also strip any accidentally persisted
        // `opencode-go/` prefix so the config stores the canonical bare id.
        if cfg.provider == "opencode" {
            let trimmed = cfg.model.trim().to_string();
            let lower = trimmed.to_ascii_lowercase();
            if lower.starts_with("opencode-go/") {
                let (bare, base) = crate::providers::normalize_opencode_selection(&trimmed);
                cfg.model = bare;
                cfg.base_url = base.to_string();
            } else if crate::providers::is_opencode_go_model(&trimmed) {
                // Bare Go model but base still points at Zen — fix the route.
                let zen = crate::providers::OPENCODE_ZEN_BASE_URL;
                let cur = cfg.base_url.trim_end_matches('/').to_ascii_lowercase();
                if cur == zen || cur.contains("/zen/v1") && !cur.contains("/zen/go/") {
                    cfg.base_url = crate::providers::OPENCODE_GO_BASE_URL.to_string();
                }
            } else {
                // Bare Zen model but base is Go without an explicit prefix — if the
                // model is definitely NOT a Go model, snap back to Zen so it doesn't
                // 404 on the Go endpoint. Guarded to avoid flip-flopping on
                // overlapping ids like grok-4.5 which intentionally stay on Zen.
                let cur = cfg.base_url.trim_end_matches('/').to_ascii_lowercase();
                if cur.contains("/zen/go/") {
                    // Only snap back if model is known Zen-ish (contains claude,
                    // gpt, gemini, sonnet, etc) OR is the default claude-sonnet-5.
                    // Otherwise leave it — user may have intentionally pointed a
                    // custom model at Go.
                    let ml = cfg.model.to_ascii_lowercase();
                    let is_zen_hint = ml.contains("claude")
                        || ml.contains("sonnet")
                        || ml.contains("gpt")
                        || ml.contains("gemini")
                        || ml.contains("opus")
                        || ml == "claude-sonnet-5";
                    if is_zen_hint {
                        cfg.base_url = crate::providers::OPENCODE_ZEN_BASE_URL.to_string();
                    }
                }
            }
        }
    }
    if cfg.config_schema < 11 && cfg.provider == "meta" && is_retired_stock_meta_model(&cfg.model) {
        cfg.model = DEFAULT_MODEL.to_string();
    }
    cfg.config_schema = CONFIG_SCHEMA;
    true
}

/// Match the retired stock model without retaining its former product name in
/// the current binary. Custom Meta model selections must survive migration.
fn is_retired_stock_meta_model(model: &str) -> bool {
    let mut chars = model.chars();
    [109_u32, 117, 115, 101]
        .into_iter()
        .all(|expected| chars.next().is_some_and(|c| c as u32 == expected))
        && chars.as_str() == "-spark-1.1"
}

fn default_compact_keep_user_turns() -> u32 {
    4
}
fn default_compact_tool_body_max() -> u64 {
    800
}

/// NurCLI data home: `~/.nur` (secrets, sessions, status, skills, memory).
/// Override with `NUR_HOME`.
pub fn nur_home() -> PathBuf {
    if let Ok(h) = std::env::var("NUR_HOME") {
        let h = h.trim();
        if !h.is_empty() {
            return PathBuf::from(h);
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".nur")
}

pub fn config_path() -> PathBuf {
    nur_home().join("config.toml")
}

pub fn auth_path() -> PathBuf {
    nur_home().join("auth.json")
}

/// Per-provider API keys for cross-provider failover (a JSON map
/// `{provider_id: key}`), separate from the single active `auth.json`.
pub fn provider_keys_path() -> PathBuf {
    nur_home().join("provider_keys.json")
}

/// Per-provider OAuth sessions for cross-provider failover (JSON map
/// `{provider_id: Auth}`). Lets a browser-signed-in provider stay usable as a
/// fallback after you switch the active login.
pub fn provider_sessions_path() -> PathBuf {
    nur_home().join("provider_sessions.json")
}

pub fn sessions_dir() -> PathBuf {
    nur_home().join("sessions")
}

/// Live status file for ADE / host panels — token usage, model, session.
pub fn status_path() -> PathBuf {
    nur_home().join("status.json")
}

/// Append-only usage log for host billing dashboards.
pub fn usage_log_path() -> PathBuf {
    nur_home().join("usage.jsonl")
}

pub fn ensure_dirs() -> Result<()> {
    let home = nur_home();
    fs::create_dir_all(&home)?;
    fs::create_dir_all(sessions_dir())?;
    Ok(())
}

pub fn load_config() -> Result<Config> {
    ensure_dirs()?;
    let path = config_path();
    let mut cfg = if !path.exists() {
        let cfg = Config::default();
        save_config(&cfg)?;
        cfg
    } else {
        let text = fs::read_to_string(&path)?;
        toml::from_str(&text).map_err(|e| NurError::Config(e.to_string()))?
    };
    // One-time: older configs → unlimited agent rounds (user sets caps later).
    if migrate_config(&mut cfg) {
        let _ = save_config(&cfg);
    }
    // Always normalize a stray `opencode-go/` prefix that might have been
    // persisted by an older picker or manual edit — the canonical stored id is
    // bare, with the base URL carrying the route.
    if cfg.provider == "opencode" {
        let trimmed = cfg.model.trim().to_string();
        if !trimmed.is_empty() {
            let lower = trimmed.to_ascii_lowercase();
            if lower.starts_with("opencode-go/") {
                let (bare, base) = crate::providers::normalize_opencode_selection(&trimmed);
                cfg.model = bare;
                cfg.base_url = base.to_string();
            }
        }
    }
    // Self-hosted OpenAI-compat (Ollama, vLLM, LiteLLM, custom gateways).
    // A per-provider override for the active provider (`OPENAI_BASE_URL`,
    // `[provider_base_urls]`) is more specific than the global `NUR_BASE_URL`,
    // so it wins. OAuth-forced hosts are re-fixed at client build time.
    let provider_id = cfg.provider.clone();
    if let Some(base) = provider_base_url_override(&cfg, &provider_id) {
        cfg.base_url = base;
    } else {
        apply_base_url_env(&mut cfg);
    }
    cfg.validate()?;
    Ok(cfg)
}

/// Apply `NUR_BASE_URL` onto a config.
pub fn apply_base_url_env(cfg: &mut Config) {
    if let Ok(u) = std::env::var("NUR_BASE_URL") {
        let u = u.trim().trim_end_matches('/').to_string();
        if !u.is_empty() {
            cfg.base_url = u;
        }
    }
}

pub fn atomic_write(path: &std::path::Path, content: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut tmp = path.to_path_buf();
    let ext = format!(
        "tmp.{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    tmp.set_extension(ext);
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(content)?;
        f.sync_all()?;
    }
    // Windows can't rename over existing file that is open? fs::rename overwrites.
    let _ = fs::remove_file(path);
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn save_config(cfg: &Config) -> Result<()> {
    ensure_dirs()?;
    let text = toml::to_string_pretty(cfg).map_err(|e| NurError::Config(e.to_string()))?;
    atomic_write(&config_path(), text.as_bytes())
        .map_err(|e| NurError::Other(format!("save_config atomic write failed: {e}")))?;
    Ok(())
}

/// Rung names nur ships knowing about. Not a whitelist — the set a request may
/// carry is derived per provider by [`crate::providers::effort_levels`], and an
/// unknown name is forwarded rather than rejected.
pub const VALID_EFFORTS: &[&str] = crate::providers::EFFORT_LADDER;

impl Config {
    pub fn validate(&self) -> Result<()> {
        // Effort is deliberately NOT a closed set. Rungs are provider-specific
        // and vendors keep adding them, so a name nur has not heard of is
        // forwarded (see `providers::nearest_effort`) rather than treated as a
        // config error — otherwise a brand-new rung bricks startup until nur
        // ships a release. Only an empty value is wrong here.
        if self.reasoning_effort.trim().is_empty() {
            return Err(NurError::Config(format!(
                "reasoning_effort is empty — use one of {} (or any level your provider accepts)",
                VALID_EFFORTS.join("|")
            )));
        }
        // 0 = unlimited. Optional hard ceiling only rejects absurd config typos
        // (u32 max is fine; no artificial 40/200 wall).
        if self.max_turns > 1_000_000 {
            return Err(NurError::Config(format!(
                "max_turns {} is unreasonably large (use 0 for unlimited, or a value ≤ 1000000)",
                self.max_turns
            )));
        }
        if self.context_window < 1000 || self.context_window > 2_000_000 {
            return Err(NurError::Config(format!(
                "context_window {} out of allowed range",
                self.context_window
            )));
        }
        if self.base_url.is_empty()
            || !(self.base_url.starts_with("http://") || self.base_url.starts_with("https://"))
        {
            return Err(NurError::Config(format!(
                "invalid base_url '{}'",
                self.base_url
            )));
        }
        if let Some(c) = self.max_session_cost_usd {
            if !c.is_finite() || c < 0.0 {
                return Err(NurError::Config(
                    "max_session_cost_usd must be a non-negative number".into(),
                ));
            }
        }
        if let Some(0) = self.max_session_tokens {
            return Err(NurError::Config(
                "max_session_tokens must be > 0 when set".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_base_url_override_is_scoped_and_normalized() {
        let mut cfg = Config::default();
        cfg.provider_base_urls.insert(
            "nur-test-compatible".into(),
            "https://gateway.example.test/v1/".into(),
        );
        assert_eq!(
            provider_base_url_override(&cfg, "nur-test-compatible").as_deref(),
            Some("https://gateway.example.test/v1")
        );
        assert_eq!(provider_base_url_override(&cfg, "another-provider"), None);
    }

    #[test]
    fn theme_selection_is_backward_compatible_and_round_trips() {
        let legacy: Config = toml::from_str("").expect("all config fields have defaults");
        assert_eq!(legacy.theme, None);

        let selected = Config {
            theme: Some("midnight".into()),
            ..Default::default()
        };
        let text = toml::to_string(&selected).expect("serialize config");
        let decoded: Config = toml::from_str(&text).expect("deserialize config");
        assert_eq!(decoded.theme.as_deref(), Some("midnight"));
    }

    #[test]
    fn migrate_forces_unlimited_turns_on_schema_upgrade() {
        let mut cfg = Config {
            config_schema: 0,
            max_turns: 99, // leftover stock/old cap - must clear on upgrade
            ..Default::default()
        };
        assert!(migrate_config(&mut cfg));
        assert_eq!(cfg.max_turns, 0);
        assert_eq!(cfg.config_schema, CONFIG_SCHEMA);
        // Second pass is a no-op; user-set caps after migration stick.
        cfg.max_turns = 12;
        assert!(!migrate_config(&mut cfg));
        assert_eq!(cfg.max_turns, 12);
    }

    /// A config saved when `grok-4` was the default must heal itself on load,
    /// not 404 on the user's next turn.
    #[test]
    fn migrate_rewrites_a_retired_grok_id_on_upgrade() {
        let mut cfg = Config {
            config_schema: 5,
            provider: "xai".into(),
            model: "grok-4".into(),
            ..Default::default()
        };
        assert!(migrate_config(&mut cfg));
        assert_eq!(cfg.model, crate::providers::XAI_DEFAULT_MODEL);
        assert_eq!(cfg.config_schema, CONFIG_SCHEMA);
    }

    #[test]
    fn migrate_leaves_a_current_grok_id_and_other_providers_alone() {
        let mut cfg = Config {
            config_schema: 5,
            provider: "xai".into(),
            model: "grok-4.20-0309-reasoning".into(),
            ..Default::default()
        };
        assert!(migrate_config(&mut cfg));
        assert_eq!(cfg.model, "grok-4.20-0309-reasoning");

        // The rewrite is scoped to xAI — a same-named model elsewhere is safe.
        let mut other = Config {
            config_schema: 5,
            provider: "opencode".into(),
            model: "grok-4".into(),
            ..Default::default()
        };
        assert!(migrate_config(&mut other));
        assert_eq!(other.model, "grok-4");
    }

    #[test]
    fn default_max_turns_is_unlimited() {
        assert_eq!(Config::default().max_turns, 0);
        assert_eq!(default_max_turns(), 0);
        assert_eq!(Config::default().config_schema, CONFIG_SCHEMA);
    }

    #[test]
    fn migrate_normalizes_the_legacy_antigravity_alias_without_resetting_limits() {
        let mut cfg = Config {
            config_schema: 3,
            provider: "antigravity".into(),
            max_turns: 12,
            fallback_providers: vec!["openai".into(), "antigravity".into()],
            fusion_panel: vec!["antigravity".into()],
            ..Default::default()
        };
        cfg.provider_privacy
            .insert("antigravity".into(), "standard".into());

        assert!(migrate_config(&mut cfg));
        assert_eq!(cfg.provider, "google");
        assert_eq!(cfg.max_turns, 12, "a user-set limit must survive schema 4");
        assert_eq!(cfg.fallback_providers, ["openai", "google"]);
        assert_eq!(cfg.fusion_panel, ["google"]);
        assert_eq!(
            cfg.provider_privacy.get("google").map(String::as_str),
            Some("standard")
        );
        assert!(!cfg.provider_privacy.contains_key("antigravity"));
    }

    #[test]
    fn migrate_removes_retired_catalog_providers() {
        let mut cfg = Config {
            config_schema: 4,
            provider: "anyscale".into(),
            base_url: "https://api.endpoints.anyscale.com/v1".into(),
            model: "obsolete-model".into(),
            fallback_providers: vec!["openai".into(), "octoai".into(), "unify".into()],
            fusion_panel: vec!["kluster".into(), "google".into()],
            ..Default::default()
        };
        cfg.provider_privacy
            .insert("omniroute".into(), "standard".into());

        assert!(migrate_config(&mut cfg));
        assert_eq!(cfg.provider, "meta");
        assert_eq!(cfg.base_url, DEFAULT_BASE_URL);
        assert_eq!(cfg.model, DEFAULT_MODEL);
        assert_eq!(cfg.fallback_providers, ["openai"]);
        assert_eq!(cfg.fusion_panel, ["google"]);
        assert!(!cfg.provider_privacy.contains_key("omniroute"));
    }

    #[test]
    fn schema_11_resets_only_the_retired_stock_meta_model() {
        let retired = [109_u8, 117, 115, 101]
            .into_iter()
            .map(char::from)
            .collect::<String>()
            + "-spark-1.1";
        let mut cfg = Config {
            config_schema: 10,
            provider: "meta".into(),
            model: retired,
            ..Config::default()
        };
        assert!(migrate_config(&mut cfg));
        assert_eq!(cfg.model, DEFAULT_MODEL);
        assert_eq!(cfg.config_schema, CONFIG_SCHEMA);
    }

    #[test]
    fn schema_11_preserves_a_custom_meta_model() {
        let mut cfg = Config {
            config_schema: 10,
            provider: "meta".into(),
            model: "custom-meta-model".into(),
            ..Config::default()
        };
        assert!(migrate_config(&mut cfg));
        assert_eq!(cfg.model, "custom-meta-model");
        assert_eq!(cfg.config_schema, CONFIG_SCHEMA);
    }

    #[test]
    fn model_display_name_title_cases() {
        assert_eq!(model_display_name(""), "model");
        assert_eq!(model_display_name("  "), "model");
        assert_eq!(model_display_name("gpt-5.5"), "Gpt 5.5");
    }
}

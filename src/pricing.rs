//! Model pricing + context-window catalog.
//!
//! Status-line dollar values are **estimates** from published list prices, not
//! invoices. Sources (in order):
//!
//! 1. Cached [models.dev](https://models.dev) catalog (`~/.nur/cache/models-dev.json`)
//! 2. Built-in Meta Model API rates (config constants) when the catalog has no match
//! 3. `$0` for known local providers (Ollama, LM Studio, llama.cpp, …)
//!
//! Cache is refreshed in the background on launch (24h TTL). Opt out with
//! `NUR_PRICING_OFF=1` or `NUR_MODELS_DEV_OFF=1`.

use crate::config::{nur_home, PRICE_INPUT_PER_MTOK, PRICE_OUTPUT_PER_MTOK};
use crate::usage::TokenUsage;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MODELS_DEV_URL: &str = "https://models.dev/api.json";
const CACHE_TTL_SECS: u64 = 24 * 60 * 60;
const FETCH_TIMEOUT_SECS: u64 = 12;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRates {
    pub input_per_mtok_usd: f64,
    pub output_per_mtok_usd: f64,
    /// USD per 1M cached/read tokens. Falls back to input rate when unknown.
    pub cache_read_per_mtok_usd: f64,
    /// USD per 1M cache-write tokens (unused until APIs report it separately).
    #[serde(default)]
    pub cache_write_per_mtok_usd: Option<f64>,
    pub context_window: Option<u64>,
    /// Short machine source: `models.dev`, `builtin-meta`, `local-free`, …
    pub source: String,
    /// Human label for `/usage` and status.json.
    pub note: String,
    pub provider_id: String,
    pub model_id: String,
}

impl ModelRates {
    /// Estimate USD for a usage blob.
    ///
    /// `input_tokens` is treated as the full input bill (OpenAI-style). When
    /// `cached_tokens` is present, that slice is priced at the cache-read rate
    /// and the remainder at the input rate.
    pub fn cost_for(&self, usage: &TokenUsage) -> f64 {
        let cached = usage.cached_tokens.min(usage.input_tokens);
        let fresh = usage.input_tokens.saturating_sub(cached);
        let input = fresh as f64 / 1_000_000.0 * self.input_per_mtok_usd;
        let cache = cached as f64 / 1_000_000.0 * self.cache_read_per_mtok_usd;
        let output = usage.output_tokens as f64 / 1_000_000.0 * self.output_per_mtok_usd;
        input + cache + output
    }

    pub fn is_estimate(&self) -> bool {
        self.source != "invoice" && self.source != "provider-reported"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheFile {
    fetched_at_unix: u64,
    /// Raw models.dev JSON (provider id → provider object).
    data: serde_json::Value,
}

struct CatalogState {
    rates_by_key: HashMap<String, ModelRates>,
    fetched_at_unix: u64,
    ready: bool,
}

impl CatalogState {
    fn empty() -> Self {
        Self {
            rates_by_key: HashMap::new(),
            fetched_at_unix: 0,
            ready: false,
        }
    }
}

fn state() -> &'static Mutex<CatalogState> {
    static STATE: OnceLock<Mutex<CatalogState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(CatalogState::empty()))
}

fn pricing_disabled() -> bool {
    for var in ["NUR_PRICING_OFF", "NUR_MODELS_DEV_OFF"] {
        if let Ok(v) = std::env::var(var) {
            let v = v.trim().to_ascii_lowercase();
            if matches!(v.as_str(), "1" | "true" | "yes" | "on") {
                return true;
            }
        }
    }
    false
}

fn cache_path() -> PathBuf {
    nur_home().join("cache").join("models-dev.json")
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Built-in Meta rates (same constants as historical status-line estimates).
pub fn builtin_meta_rates(model: &str) -> ModelRates {
    ModelRates {
        input_per_mtok_usd: PRICE_INPUT_PER_MTOK,
        output_per_mtok_usd: PRICE_OUTPUT_PER_MTOK,
        cache_read_per_mtok_usd: PRICE_INPUT_PER_MTOK * 0.12, // Meta publishes ~$0.15 on models.dev
        cache_write_per_mtok_usd: None,
        context_window: Some(1_000_000),
        source: "builtin-meta".into(),
        note: "Indicative Meta Model API list prices (config constants); verify on dev.meta.ai"
            .into(),
        provider_id: "meta".into(),
        model_id: model.to_string(),
    }
}

fn local_free_rates(provider: &str, model: &str) -> ModelRates {
    ModelRates {
        input_per_mtok_usd: 0.0,
        output_per_mtok_usd: 0.0,
        cache_read_per_mtok_usd: 0.0,
        cache_write_per_mtok_usd: Some(0.0),
        context_window: None,
        source: "local-free".into(),
        note: "Local inference — no cloud list price (hardware cost not estimated)".into(),
        provider_id: provider.into(),
        model_id: model.into(),
    }
}

fn is_local_provider(provider: &str) -> bool {
    matches!(
        provider,
        "ollama" | "lmstudio" | "llamacpp" | "vllm" | "jan" | "local"
    )
}

/// Map nur-cli provider ids onto models.dev provider keys.
fn models_dev_provider_key(provider: &str) -> &str {
    match provider {
        "openai-cc" => "openai",
        "antigravity" => "google",
        "bedrock" => "amazon-bedrock",
        "azure" => "azure",
        "kimi" => "kimi-for-coding",
        "moonshot" => "moonshotai",
        "zhipu" => "zai",
        "qwen" => "alibaba",
        "github-copilot" => "github-copilot",
        "github-models" => "github-models",
        other => other,
    }
}

fn norm_model(s: &str) -> String {
    s.trim().to_ascii_lowercase()
}

fn model_lookup_candidates(provider: &str, model: &str) -> Vec<String> {
    let m = model.trim().to_string();
    let lower = norm_model(&m);
    let mut out = vec![m.clone(), lower.clone()];
    // Strip vendor prefixes common on routers: openai/gpt-5.5, meta-llama/…
    if let Some((_, rest)) = lower.split_once('/') {
        out.push(rest.to_string());
    }
    if let Some((_, rest)) = lower.split_once(':') {
        out.push(rest.to_string());
    }
    // OpenRouter-style provider/model when using openrouter
    if provider == "openrouter" && !lower.contains('/') {
        // keep as-is; catalog keys include org/model
    }
    // Dedup while preserving order
    let mut seen = std::collections::HashSet::new();
    out.retain(|x| seen.insert(x.clone()));
    out
}

fn parse_cost_field(cost: &serde_json::Value, key: &str) -> Option<f64> {
    cost.get(key).and_then(|v| {
        v.as_f64()
            .or_else(|| v.as_i64().map(|i| i as f64))
            .or_else(|| v.as_u64().map(|u| u as f64))
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
    })
}

fn rates_from_model_json(
    provider_id: &str,
    model_id: &str,
    model: &serde_json::Value,
) -> Option<ModelRates> {
    let cost = model.get("cost")?;
    let input = parse_cost_field(cost, "input")?;
    let output = parse_cost_field(cost, "output")?;
    let cache_read = parse_cost_field(cost, "cache_read")
        .or_else(|| parse_cost_field(cost, "cacheRead"))
        .unwrap_or(input);
    let cache_write =
        parse_cost_field(cost, "cache_write").or_else(|| parse_cost_field(cost, "cacheWrite"));
    let context = model
        .get("limit")
        .and_then(|l| l.get("context"))
        .and_then(|v| v.as_u64().or_else(|| v.as_i64().map(|i| i as u64)));
    Some(ModelRates {
        input_per_mtok_usd: input,
        output_per_mtok_usd: output,
        cache_read_per_mtok_usd: cache_read,
        cache_write_per_mtok_usd: cache_write,
        context_window: context,
        source: "models.dev".into(),
        note: format!(
            "List prices from models.dev for {provider_id}/{model_id} — estimate only, not an invoice"
        ),
        provider_id: provider_id.into(),
        model_id: model_id.into(),
    })
}

fn index_catalog(data: &serde_json::Value) -> HashMap<String, ModelRates> {
    let mut map = HashMap::new();
    let Some(obj) = data.as_object() else {
        return map;
    };
    for (pid, prov) in obj {
        let Some(models) = prov.get("models").and_then(|m| m.as_object()) else {
            continue;
        };
        for (mid, model) in models {
            if let Some(rates) = rates_from_model_json(pid, mid, model) {
                let key = format!("{}::{}", norm_model(pid), norm_model(mid));
                map.insert(key, rates);
            }
        }
    }
    map
}

fn load_cache_file() -> Option<CacheFile> {
    let path = cache_path();
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn save_cache_file(cf: &CacheFile) {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string(cf) {
        let _ = fs::write(path, text);
    }
}

fn apply_catalog(data: serde_json::Value, fetched_at_unix: u64) {
    let rates = index_catalog(&data);
    if let Ok(mut g) = state().lock() {
        g.rates_by_key = rates;
        g.fetched_at_unix = fetched_at_unix;
        g.ready = true;
    }
}

/// Load on-disk cache into memory if present (sync, cheap).
pub fn load_cached_catalog() {
    if pricing_disabled() {
        return;
    }
    if let Some(cf) = load_cache_file() {
        apply_catalog(cf.data, cf.fetched_at_unix);
    }
}

/// Fetch models.dev when cache is missing/stale. Safe to call from a background thread.
pub fn refresh_catalog_if_stale() {
    if pricing_disabled() {
        return;
    }
    let now = now_unix();
    let stale = {
        let g = state().lock().ok();
        match g {
            Some(g) if g.ready && now.saturating_sub(g.fetched_at_unix) < CACHE_TTL_SECS => false,
            _ => true,
        }
    };
    // Also treat on-disk fresh cache as enough (may not be loaded yet).
    if !stale {
        return;
    }
    if let Some(cf) = load_cache_file() {
        if now.saturating_sub(cf.fetched_at_unix) < CACHE_TTL_SECS {
            apply_catalog(cf.data, cf.fetched_at_unix);
            return;
        }
    }
    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(FETCH_TIMEOUT_SECS))
        .user_agent(format!("nur-cli/{}", env!("CARGO_PKG_VERSION")))
        .build()
    {
        Ok(c) => c,
        Err(_) => return,
    };
    let resp = match client.get(MODELS_DEV_URL).send() {
        Ok(r) if r.status().is_success() => r,
        _ => return,
    };
    let data: serde_json::Value = match resp.json() {
        Ok(v) => v,
        Err(_) => return,
    };
    let cf = CacheFile {
        fetched_at_unix: now,
        data: data.clone(),
    };
    save_cache_file(&cf);
    apply_catalog(data, now);
}

/// Kick a background refresh (non-blocking). Call once at process start.
pub fn spawn_catalog_refresh() {
    if pricing_disabled() {
        return;
    }
    // The disk parse has to stay on this thread: `main` reads the catalog for
    // `maybe_apply_context_window` on the very next line, and a background load
    // loses that race, pinning every session to the 1M default window.
    // Only the network fetch is deferred.
    load_cached_catalog();
    let _ = std::thread::Builder::new()
        .name("nur-pricing".into())
        .spawn(|| {
            refresh_catalog_if_stale();
        });
}

fn lookup_in_index(provider: &str, model: &str) -> Option<ModelRates> {
    let g = state().lock().ok()?;
    if !g.ready {
        return None;
    }
    let md_provider = models_dev_provider_key(provider);
    let pkey = norm_model(md_provider);
    for mid in model_lookup_candidates(provider, model) {
        let key = format!("{pkey}::{}", norm_model(&mid));
        if let Some(r) = g.rates_by_key.get(&key) {
            return Some(r.clone());
        }
    }
    // Fuzzy: any catalog model id that equals or ends with our model id
    let targets: Vec<String> = model_lookup_candidates(provider, model)
        .into_iter()
        .map(|m| norm_model(&m))
        .collect();
    for (key, rates) in g.rates_by_key.iter() {
        if !key.starts_with(&format!("{pkey}::")) {
            continue;
        }
        let cat_mid = key.split("::").nth(1).unwrap_or("");
        for t in &targets {
            if cat_mid == t || cat_mid.ends_with(&format!("/{t}")) || cat_mid.ends_with(t) {
                return Some(rates.clone());
            }
        }
    }
    // Cross-provider last resort for bare model ids (e.g. gpt-5.5 under openai)
    for t in &targets {
        for (key, rates) in g.rates_by_key.iter() {
            let cat_mid = key.split("::").nth(1).unwrap_or("");
            if cat_mid == t {
                return Some(rates.clone());
            }
        }
    }
    None
}

/// Resolve rates for the active provider/model.
pub fn rates_for(provider: &str, model: &str) -> ModelRates {
    let provider = provider.trim();
    let model = model.trim();
    if provider.is_empty() && model.is_empty() {
        return builtin_meta_rates("unknown");
    }
    if is_local_provider(provider) {
        return local_free_rates(provider, model);
    }
    if let Some(mut r) = lookup_in_index(provider, model) {
        // Keep the nur provider id the user is actually on.
        r.provider_id = provider.to_string();
        r.model_id = model.to_string();
        return r;
    }
    // OpenRouter / gateway models often carry org/model — try openrouter catalog
    if matches!(
        provider,
        "openrouter"
            | "vercel"
            | "omniroute"
            | "requesty"
            | "glama"
            | "helicone"
            | "cloudflare"
            | "aimlapi"
    ) {
        if let Some(mut r) = lookup_in_index("openrouter", model) {
            r.provider_id = provider.to_string();
            r.model_id = model.to_string();
            r.note = format!(
                "List prices via openrouter catalog entry for {model} (routed through {provider}) — estimate only"
            );
            return r;
        }
    }
    let mut r = builtin_meta_rates(model);
    r.provider_id = if provider.is_empty() {
        "unknown".into()
    } else {
        provider.into()
    };
    r.model_id = model.into();
    r.source = "builtin-fallback".into();
    r.note = format!(
        "No models.dev match for {provider}/{model} — using Meta list-price fallback ($in {:.2}/M · $out {:.2}/M)",
        r.input_per_mtok_usd, r.output_per_mtok_usd
    );
    r
}

/// Suggest a context window from the catalog (None when unknown).
pub fn context_window_for(provider: &str, model: &str) -> Option<u64> {
    rates_for(provider, model).context_window
}

/// Soft-apply catalog context window when the user still has the built-in default.
pub fn maybe_apply_context_window(cfg: &mut crate::config::Config) {
    const DEFAULT_WINDOW: u64 = 1_000_000;
    if cfg.context_window != DEFAULT_WINDOW {
        return;
    }
    if let Some(w) = context_window_for(&cfg.provider, &cfg.model) {
        if (1000..=2_000_000).contains(&w) {
            cfg.context_window = w;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::TokenUsage;

    /// `main.rs` calls `spawn_catalog_refresh()` and then immediately
    /// `maybe_apply_context_window()`. The cached catalog therefore has to be
    /// readable by the time the spawn call returns — if the disk parse moves
    /// onto the background thread, the main thread wins the race and every
    /// session silently keeps the 1M default window.
    #[test]
    fn startup_resolves_context_window_before_main_reads_it() {
        {
            let mut g = state().lock().unwrap();
            g.ready = false;
        }
        spawn_catalog_refresh();

        let mut cfg = crate::config::Config::default();
        cfg.provider = "anthropic".into();
        cfg.model = "claude-opus-4-5".into();
        cfg.context_window = 1_000_000;
        maybe_apply_context_window(&mut cfg);

        assert_eq!(
            cfg.context_window, 200_000,
            "catalog window must be applied synchronously at startup"
        );
    }

    #[test]
    fn cost_splits_cached_input() {
        let rates = ModelRates {
            input_per_mtok_usd: 10.0,
            output_per_mtok_usd: 20.0,
            cache_read_per_mtok_usd: 1.0,
            cache_write_per_mtok_usd: None,
            context_window: Some(100_000),
            source: "test".into(),
            note: "t".into(),
            provider_id: "openai".into(),
            model_id: "gpt".into(),
        };
        let u = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            total_tokens: 2_000_000,
            reasoning_tokens: 0,
            cached_tokens: 400_000,
            cost_usd: 0.0,
            cost_known: false,
        };
        // fresh 0.6M * $10 + cache 0.4M * $1 + out 1M * $20 = 6 + 0.4 + 20 = 26.4
        let c = rates.cost_for(&u);
        assert!((c - 26.4).abs() < 1e-9, "got {c}");
    }

    #[test]
    fn local_providers_are_free() {
        let r = rates_for("ollama", "llama3.3");
        assert_eq!(r.input_per_mtok_usd, 0.0);
        assert_eq!(r.source, "local-free");
    }

    #[test]
    fn meta_builtin_matches_constants() {
        let r = builtin_meta_rates("muse-spark-1.1");
        assert_eq!(r.input_per_mtok_usd, PRICE_INPUT_PER_MTOK);
        assert_eq!(r.output_per_mtok_usd, PRICE_OUTPUT_PER_MTOK);
    }

    #[test]
    fn index_and_lookup_from_fixture() {
        let data = serde_json::json!({
            "openai": {
                "models": {
                    "gpt-test": {
                        "cost": { "input": 2.0, "output": 8.0, "cache_read": 0.2 },
                        "limit": { "context": 128000 }
                    }
                }
            }
        });
        apply_catalog(data, now_unix());
        let r = rates_for("openai", "gpt-test");
        assert_eq!(r.source, "models.dev");
        assert_eq!(r.input_per_mtok_usd, 2.0);
        assert_eq!(r.output_per_mtok_usd, 8.0);
        assert_eq!(r.cache_read_per_mtok_usd, 0.2);
        assert_eq!(r.context_window, Some(128000));
    }
}

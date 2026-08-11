//! Token usage tracking for host panels (Orca ADE, etc.) and local dashboards.
//!
//! Writes:
//! - `~/.nur/status.json` — current session snapshot
//! - `~/.nur/usage.jsonl` — append-only per-request log
//!
//! Dollar values are **estimates** from [`crate::pricing`] (models.dev list
//! prices when available). They are not provider invoices.

use crate::config::{
    atomic_write, ensure_dirs, status_path, usage_log_path, PRICE_INPUT_PER_MTOK,
    PRICE_OUTPUT_PER_MTOK,
};
use crate::error::Result;
use crate::pricing::{self, ModelRates};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

/// Whether token counts came from the serving provider or local accounting.
/// The default preserves historical JSONL rows, which were provider-reported.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageState {
    #[default]
    Observed,
    Estimated,
    Unknown,
}

/// What a dollar figure represents. List prices and subscription consumption
/// are fundamentally different, so they must never share the same label.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostProvenance {
    #[default]
    Unknown,
    ProviderReported,
    CatalogEstimate,
    FallbackEstimate,
    SubscriptionUnknown,
    Unmetered,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
    #[serde(default)]
    pub cached_tokens: u64,
    /// Cache writes have their own rate on providers which expose them.
    #[serde(default)]
    pub cache_write_tokens: u64,
    #[serde(default)]
    pub usage_state: UsageState,
    /// Estimated USD for this blob (request or session aggregate).
    #[serde(default)]
    pub cost_usd: f64,
    /// When true, `cost_usd` was computed with model rates (including free = 0).
    #[serde(default)]
    pub cost_known: bool,
    #[serde(default)]
    pub cost_provenance: CostProvenance,
    /// Gateway-selected upstream, when disclosed (for example OpenRouter).
    #[serde(default)]
    pub upstream_provider: Option<String>,
}

impl TokenUsage {
    pub fn estimated(input_tokens: u64, output_tokens: u64) -> Self {
        Self {
            input_tokens,
            output_tokens,
            total_tokens: input_tokens.saturating_add(output_tokens),
            usage_state: UsageState::Estimated,
            ..Default::default()
        }
    }

    pub fn unknown() -> Self {
        Self {
            usage_state: UsageState::Unknown,
            ..Default::default()
        }
    }

    pub fn add(&mut self, other: &TokenUsage) {
        // Cost first, while we still know both sides' stamp state.
        match (self.cost_known, other.cost_known) {
            (true, true) => self.cost_usd += other.cost_usd,
            (true, false) => self.cost_usd += fallback_meta_cost(other),
            (false, true) => {
                self.cost_usd = fallback_meta_cost(self) + other.cost_usd;
                self.cost_known = true;
            }
            (false, false) => {}
        }
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.total_tokens += other.total_tokens;
        self.reasoning_tokens += other.reasoning_tokens;
        self.cached_tokens += other.cached_tokens;
        self.cache_write_tokens += other.cache_write_tokens;
        self.usage_state = merge_usage_state(self.usage_state, other.usage_state);
        self.cost_provenance = merge_cost_provenance(self.cost_provenance, other.cost_provenance);
        if other.upstream_provider.is_some() {
            self.upstream_provider = other.upstream_provider.clone();
        }
    }

    /// Estimated USD — prefers stamped model-aware cost, else Meta list-price fallback.
    pub fn estimated_cost_usd(&self) -> f64 {
        if self.cost_known {
            self.cost_usd
        } else if self.cost_provenance == CostProvenance::SubscriptionUnknown {
            // There is no trustworthy per-request dollar conversion for a
            // subscription/credit plan. Zero here is a lower bound only; the
            // accompanying provenance makes that explicit to every consumer.
            0.0
        } else {
            fallback_meta_cost(self)
        }
    }

    /// Stamp this blob with rates for a provider/model.
    pub fn stamp_cost(&mut self, rates: &ModelRates) {
        self.cost_usd = rates.cost_for(self);
        self.cost_known = true;
        self.cost_provenance = match rates.source.as_str() {
            "builtin-fallback" => CostProvenance::FallbackEstimate,
            "local-free" => CostProvenance::Unmetered,
            _ => CostProvenance::CatalogEstimate,
        };
    }
}

fn merge_usage_state(left: UsageState, right: UsageState) -> UsageState {
    if matches!(left, UsageState::Unknown) || matches!(right, UsageState::Unknown) {
        UsageState::Unknown
    } else if matches!(left, UsageState::Estimated) || matches!(right, UsageState::Estimated) {
        UsageState::Estimated
    } else {
        UsageState::Observed
    }
}

fn merge_cost_provenance(left: CostProvenance, right: CostProvenance) -> CostProvenance {
    use CostProvenance::*;
    match (left, right) {
        (ProviderReported, _) | (_, ProviderReported) => ProviderReported,
        (SubscriptionUnknown, _) | (_, SubscriptionUnknown) => SubscriptionUnknown,
        (FallbackEstimate, _) | (_, FallbackEstimate) => FallbackEstimate,
        (CatalogEstimate, _) | (_, CatalogEstimate) => CatalogEstimate,
        (Unmetered, _) | (_, Unmetered) => Unmetered,
        _ => Unknown,
    }
}

fn fallback_meta_cost(u: &TokenUsage) -> f64 {
    let cached = u.cached_tokens.min(u.input_tokens);
    let fresh = u.input_tokens.saturating_sub(cached);
    let input = fresh as f64 / 1_000_000.0 * PRICE_INPUT_PER_MTOK;
    // Historical code priced all input at full rate; keep cache slightly cheaper
    // for unstamped legacy blobs so behavior stays in the same ballpark.
    let cache = cached as f64 / 1_000_000.0 * PRICE_INPUT_PER_MTOK;
    let output = u.output_tokens as f64 / 1_000_000.0 * PRICE_OUTPUT_PER_MTOK;
    input + cache + output
}

fn stamp_for_route(provider: &str, model: &str, mut usage: TokenUsage) -> (TokenUsage, ModelRates) {
    let rates = pricing::rates_for(provider, model);
    // A gateway may provide an authoritative cost, whereas Cursor-like
    // subscriptions explicitly do not expose one. Do not overwrite either
    // fact with a catalog approximation.
    if !usage.cost_known && usage.cost_provenance != CostProvenance::SubscriptionUnknown {
        usage.stamp_cost(&rates);
    }
    (usage, rates)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusSnapshot {
    pub schema_version: u32,
    pub provider: String,
    pub model: String,
    pub session_id: String,
    pub cwd: String,
    pub pid: u32,
    pub state: String,
    pub updated_at: DateTime<Utc>,
    pub turn: u32,
    pub usage_session: TokenUsage,
    pub usage_last_request: TokenUsage,
    pub estimated_cost_usd_session: f64,
    pub estimated_cost_usd_last: f64,
    #[serde(default)]
    pub cost_provenance_session: CostProvenance,
    #[serde(default)]
    pub cost_provenance_last: CostProvenance,
    pub pricing: PricingInfo,
    /// Absolute path to this status file (for ADE discovery).
    pub status_path: String,
    /// Path to append-only usage log.
    pub usage_log_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingInfo {
    pub input_per_mtok_usd: f64,
    pub output_per_mtok_usd: f64,
    #[serde(default)]
    pub cache_read_per_mtok_usd: f64,
    #[serde(default)]
    pub cache_write_per_mtok_usd: Option<f64>,
    pub note: String,
    /// `models.dev` | `builtin-meta` | `local-free` | …
    #[serde(default)]
    pub source: String,
    /// When true, dollar amounts are list-price estimates (not invoices).
    #[serde(default = "default_true")]
    pub estimate: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageLogLine {
    pub ts: DateTime<Utc>,
    pub session_id: String,
    pub model: String,
    #[serde(default)]
    pub provider: String,
    pub response_id: Option<String>,
    pub usage: TokenUsage,
    pub estimated_cost_usd: f64,
    pub turn: u32,
    #[serde(default)]
    pub pricing_source: String,
    #[serde(default)]
    pub cost_provenance: CostProvenance,
    #[serde(default)]
    pub usage_state: UsageState,
}

/// Append-only evidence for requests that may have reached a provider but did
/// not yield a normal completed response. It intentionally lives beside, not
/// inside, the successful-request JSONL so existing consumers remain valid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageAttemptLine {
    pub ts: DateTime<Utc>,
    pub attempt_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
    pub provider: String,
    pub model: String,
    pub attempt: u32,
    /// `started`, `succeeded`, `failed`, or `ambiguous`.
    pub outcome: String,
    pub estimated_input_tokens: u64,
    #[serde(default)]
    pub response_id: Option<String>,
    #[serde(default)]
    pub status: Option<u16>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[allow(clippy::too_many_arguments)] // one append-only row matching the receipt schema
pub fn record_transport_attempt(
    attempt_id: &str,
    session_id: Option<&str>,
    provider: &str,
    model: &str,
    attempt: u32,
    outcome: &str,
    estimated_input_tokens: u64,
    response_id: Option<&str>,
    status: Option<u16>,
    reason: Option<&str>,
) {
    let _ = ensure_dirs();
    let path = usage_log_path().with_file_name("usage-attempts.jsonl");
    let line = UsageAttemptLine {
        ts: Utc::now(),
        attempt_id: attempt_id.to_string(),
        session_id: session_id.map(ToString::to_string),
        provider: provider.to_string(),
        model: model.to_string(),
        attempt,
        outcome: outcome.to_string(),
        estimated_input_tokens,
        response_id: response_id.map(ToString::to_string),
        status,
        reason: reason.map(|value| value.chars().take(500).collect()),
    };
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{}", serde_json::to_string(&line).unwrap_or_default());
    }
}

pub struct UsageTracker {
    session_id: String,
    model: String,
    provider: String,
    cwd: PathBuf,
    turn: u32,
    session: TokenUsage,
    last: TokenUsage,
    state: String,
    /// When false (subagents), skip the global status.json / ade.json / hook —
    /// only the session-scoped status file is written. Keeps the Orca display
    /// pinned to the top-level session.
    global: bool,
    lock: Mutex<()>,
}

impl UsageTracker {
    pub fn new(session_id: String, model: String, cwd: PathBuf) -> Self {
        Self::with_scope(session_id, model, cwd, true)
    }

    /// Session-scoped tracker for subagents: no global status/ADE writes.
    pub fn scoped(session_id: String, model: String, cwd: PathBuf) -> Self {
        Self::with_scope(session_id, model, cwd, false)
    }

    fn with_scope(session_id: String, model: String, cwd: PathBuf, global: bool) -> Self {
        let t = Self {
            session_id,
            model,
            provider: "meta".into(),
            cwd,
            turn: 0,
            session: TokenUsage::default(),
            last: TokenUsage::default(),
            state: "idle".into(),
            global,
            lock: Mutex::new(()),
        };
        let _ = t.write_status();
        t
    }

    /// Fold in tokens spent elsewhere (e.g. a finished subagent) so session
    /// totals and the ADE status stay honest.
    pub fn add_external(&mut self, usage: &TokenUsage) {
        let mut u = usage.clone();
        if !u.cost_known {
            let rates = pricing::rates_for(&self.provider, &self.model);
            u.stamp_cost(&rates);
        }
        self.session.add(&u);
        let _ = self.write_status();
    }

    pub fn set_model(&mut self, model: String) {
        self.model = model;
        let _ = self.write_status();
    }

    pub fn set_provider(&mut self, provider: String) {
        self.provider = provider;
        let _ = self.write_status();
    }

    pub fn set_cwd(&mut self, cwd: PathBuf) {
        self.cwd = cwd;
        let _ = self.write_status();
    }

    pub fn set_state(&mut self, state: impl Into<String>) {
        self.state = state.into();
        let _ = self.write_status();
        // Push the transition to ADEs. Only the top-level tracker owns the
        // global status/manifest/hook; subagents stay session-scoped so they
        // don't flip the host's idle/busy display out from under the run.
        if self.global {
            crate::ade::write_ade_manifest(
                &self.session_id,
                &self.model,
                &self.cwd.display().to_string(),
                &self.session,
                &self.state,
            );
            crate::ade::notify_orca_state(
                &self.session_id,
                &self.model,
                &self.provider,
                self.turn,
                &self.state,
            );
        }
    }

    pub fn session_usage(&self) -> &TokenUsage {
        &self.session
    }

    pub fn last_usage(&self) -> &TokenUsage {
        &self.last
    }

    /// Current list-price rates for the active provider/model.
    pub fn active_rates(&self) -> ModelRates {
        pricing::rates_for(&self.provider, &self.model)
    }

    /// Seed cumulative totals when resuming a session (does not append log).
    pub fn seed_session(&mut self, prior: TokenUsage) {
        let mut prior = prior;
        if !prior.cost_known && prior.total_tokens > 0 {
            // Best-effort restamp with current model rates so budgets still trip.
            let rates = self.active_rates();
            prior.stamp_cost(&rates);
        }
        self.session = prior;
        let _ = self.write_status();
    }

    pub fn record_request(&mut self, usage: TokenUsage, response_id: Option<String>) {
        let provider = self.provider.clone();
        let model = self.model.clone();
        self.record_request_for_route(&provider, &model, usage, response_id);
    }

    /// Record a request served by a different failover route without changing
    /// the session's configured primary provider/model. Pricing, the append-only
    /// usage row, and the live hook payload must describe where tokens were
    /// actually spent.
    pub fn record_request_for_route(
        &mut self,
        provider: &str,
        model: &str,
        usage: TokenUsage,
        response_id: Option<String>,
    ) {
        self.turn += 1;
        let (usage, rates) = stamp_for_route(provider, model, usage);
        self.last = usage.clone();
        self.session.add(&usage);
        let _ = self.append_log(&usage, response_id, &rates, provider, model);
        let _ = self.write_status();
        if !self.global {
            return;
        }
        // Host-panel env (current process; children/hooks can read).
        let status = status_path().display().to_string();
        let cost = format!("{:.6}", self.session.estimated_cost_usd());
        for (key, val) in [
            (
                "NUR_USAGE_INPUT_TOKENS",
                self.session.input_tokens.to_string(),
            ),
            (
                "NUR_USAGE_OUTPUT_TOKENS",
                self.session.output_tokens.to_string(),
            ),
            (
                "NUR_USAGE_TOTAL_TOKENS",
                self.session.total_tokens.to_string(),
            ),
            ("NUR_USAGE_COST_USD", cost),
            ("NUR_STATUS_PATH", status),
        ] {
            std::env::set_var(key, &val);
        }

        // Discovery manifest + optional Orca hook ping
        crate::ade::write_ade_manifest(
            &self.session_id,
            &self.model,
            &self.cwd.display().to_string(),
            &self.session,
            &self.state,
        );
        let payload = serde_json::json!({
            "type": "meta.usage",
            "session_id": self.session_id,
            "model": model,
            "provider": provider,
            "turn": self.turn,
            "state": self.state,
            "usage": self.session,
            "estimated_cost_usd": self.session.estimated_cost_usd(),
            "pricing_source": rates.source,
            "status_path": status_path().display().to_string(),
        });
        crate::ade::notify_orca_hook(&payload.to_string());
    }

    fn write_status(&self) -> Result<()> {
        let _g = self.lock.lock().ok();
        ensure_dirs()?;
        let rates = self.active_rates();
        let snap = StatusSnapshot {
            schema_version: 2,
            provider: self.provider.clone(),
            model: self.model.clone(),
            session_id: self.session_id.clone(),
            cwd: self.cwd.display().to_string(),
            pid: std::process::id(),
            state: self.state.clone(),
            updated_at: Utc::now(),
            turn: self.turn,
            usage_session: self.session.clone(),
            usage_last_request: self.last.clone(),
            estimated_cost_usd_session: self.session.estimated_cost_usd(),
            estimated_cost_usd_last: self.last.estimated_cost_usd(),
            cost_provenance_session: self.session.cost_provenance,
            cost_provenance_last: self.last.cost_provenance,
            pricing: PricingInfo {
                input_per_mtok_usd: rates.input_per_mtok_usd,
                output_per_mtok_usd: rates.output_per_mtok_usd,
                cache_read_per_mtok_usd: rates.cache_read_per_mtok_usd,
                cache_write_per_mtok_usd: rates.cache_write_per_mtok_usd,
                note: rates.note.clone(),
                source: rates.source.clone(),
                estimate: rates.is_estimate(),
            },
            status_path: status_path().display().to_string(),
            usage_log_path: usage_log_path().display().to_string(),
        };
        let json = serde_json::to_string_pretty(&snap)?;
        if self.global {
            let _ = atomic_write(&status_path(), json.as_bytes());
        }
        // Session-scoped status for multi-agent ADE layouts
        let sess_status =
            crate::config::sessions_dir().join(format!("{}.status.json", self.session_id));
        let _ = atomic_write(&sess_status, json.as_bytes());
        Ok(())
    }

    fn append_log(
        &self,
        usage: &TokenUsage,
        response_id: Option<String>,
        rates: &ModelRates,
        provider: &str,
        model: &str,
    ) -> Result<()> {
        ensure_dirs()?;
        let line = UsageLogLine {
            ts: Utc::now(),
            session_id: self.session_id.clone(),
            model: model.to_string(),
            provider: provider.to_string(),
            response_id,
            usage: usage.clone(),
            estimated_cost_usd: usage.estimated_cost_usd(),
            turn: self.turn,
            pricing_source: rates.source.clone(),
            cost_provenance: usage.cost_provenance,
            usage_state: usage.usage_state,
        };
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(usage_log_path())?;
        writeln!(f, "{}", serde_json::to_string(&line)?)?;
        Ok(())
    }
}

/// Print human + machine summary for `nur usage`.
pub fn print_usage_summary() -> Result<()> {
    let path = status_path();
    if !path.exists() {
        println!("no status yet (run nur first)");
        println!("status path: {}", path.display());
        return Ok(());
    }
    let text = fs::read_to_string(&path)?;
    let snap: StatusSnapshot = serde_json::from_str(&text)?;
    println!("status_path: {}", snap.status_path);
    println!("usage_log_path: {}", snap.usage_log_path);
    println!("provider: {}", snap.provider);
    println!("model: {}", snap.model);
    println!("session_id: {}", snap.session_id);
    println!("state: {}", snap.state);
    println!("turn: {}", snap.turn);
    println!(
        "session tokens: in={} out={} total={} reasoning={} cached={}",
        snap.usage_session.input_tokens,
        snap.usage_session.output_tokens,
        snap.usage_session.total_tokens,
        snap.usage_session.reasoning_tokens,
        snap.usage_session.cached_tokens
    );
    println!(
        "session cost USD: ${:.6}  ({})",
        snap.estimated_cost_usd_session,
        cost_provenance_label(snap.cost_provenance_session)
    );
    println!(
        "rates: ${:.4}/M in · ${:.4}/M out · ${:.4}/M cache-read  [{}]",
        snap.pricing.input_per_mtok_usd,
        snap.pricing.output_per_mtok_usd,
        snap.pricing.cache_read_per_mtok_usd,
        snap.pricing.source
    );
    println!("pricing note: {}", snap.pricing.note);
    println!(
        "last request tokens: in={} out={} total={}  cost ${:.6} ({})",
        snap.usage_last_request.input_tokens,
        snap.usage_last_request.output_tokens,
        snap.usage_last_request.total_tokens,
        snap.estimated_cost_usd_last,
        cost_provenance_label(snap.cost_provenance_last),
    );
    Ok(())
}

fn cost_provenance_label(provenance: CostProvenance) -> &'static str {
    match provenance {
        CostProvenance::ProviderReported => "provider reported",
        CostProvenance::CatalogEstimate => "catalog estimate",
        CostProvenance::FallbackEstimate => "fallback estimate",
        CostProvenance::SubscriptionUnknown => "subscription unknown",
        CostProvenance::Unmetered => "unmetered",
        CostProvenance::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamped_cost_survives_add() {
        let rates = pricing::builtin_meta_rates("Llama-4-Maverick-17B-128E-Instruct-FP8");
        let mut a = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 0,
            total_tokens: 1_000_000,
            ..Default::default()
        };
        a.stamp_cost(&rates);
        let mut b = TokenUsage {
            input_tokens: 0,
            output_tokens: 1_000_000,
            total_tokens: 1_000_000,
            ..Default::default()
        };
        b.stamp_cost(&rates);
        a.add(&b);
        assert!(a.cost_known);
        let expected = PRICE_INPUT_PER_MTOK + PRICE_OUTPUT_PER_MTOK;
        assert!((a.estimated_cost_usd() - expected).abs() < 1e-9);
    }

    #[test]
    fn failover_request_uses_the_serving_routes_rates() {
        let (usage, rates) = stamp_for_route(
            "ollama",
            "local-model",
            TokenUsage {
                input_tokens: 10_000,
                output_tokens: 1_000,
                total_tokens: 11_000,
                ..Default::default()
            },
        );

        assert!(usage.cost_known);
        assert_eq!(usage.cost_usd, 0.0);
        assert_eq!(rates.source, "local-free");
    }

    #[test]
    fn ambiguous_attempt_line_preserves_reconciliation_evidence() {
        let line = UsageAttemptLine {
            ts: Utc::now(),
            attempt_id: "nur-attempt".into(),
            session_id: Some("session".into()),
            provider: "openrouter".into(),
            model: "vendor/model".into(),
            attempt: 2,
            outcome: "ambiguous".into(),
            estimated_input_tokens: 123,
            response_id: None,
            status: None,
            reason: Some("connection closed".into()),
        };
        let value = serde_json::to_value(&line).expect("serialize attempt");
        assert_eq!(value["outcome"], "ambiguous");
        assert_eq!(value["estimated_input_tokens"], 123);
        assert_eq!(value["session_id"], "session");
    }
}

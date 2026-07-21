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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
    #[serde(default)]
    pub cached_tokens: u64,
    /// Estimated USD for this blob (request or session aggregate).
    #[serde(default)]
    pub cost_usd: f64,
    /// When true, `cost_usd` was computed with model rates (including free = 0).
    #[serde(default)]
    pub cost_known: bool,
}

impl TokenUsage {
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
    }

    /// Estimated USD — prefers stamped model-aware cost, else Meta list-price fallback.
    pub fn estimated_cost_usd(&self) -> f64 {
        if self.cost_known {
            self.cost_usd
        } else {
            fallback_meta_cost(self)
        }
    }

    /// Stamp this blob with rates for a provider/model.
    pub fn stamp_cost(&mut self, rates: &ModelRates) {
        self.cost_usd = rates.cost_for(self);
        self.cost_known = true;
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
        self.turn += 1;
        let rates = self.active_rates();
        let mut usage = usage;
        usage.stamp_cost(&rates);
        self.last = usage.clone();
        self.session.add(&usage);
        let _ = self.append_log(&usage, response_id, &rates);
        let _ = self.write_status();
        if !self.global {
            return;
        }
        // Host-panel env (current process; children/hooks can read).
        // Prefer NUR_*; keep META_* / MUSE_* so older Orca panels and hooks keep working.
        let status = status_path().display().to_string();
        let cost = format!("{:.6}", self.session.estimated_cost_usd());
        for (nur_k, meta_k, muse_k, val) in [
            (
                "NUR_USAGE_INPUT_TOKENS",
                "META_USAGE_INPUT_TOKENS",
                "MUSE_USAGE_INPUT_TOKENS",
                self.session.input_tokens.to_string(),
            ),
            (
                "NUR_USAGE_OUTPUT_TOKENS",
                "META_USAGE_OUTPUT_TOKENS",
                "MUSE_USAGE_OUTPUT_TOKENS",
                self.session.output_tokens.to_string(),
            ),
            (
                "NUR_USAGE_TOTAL_TOKENS",
                "META_USAGE_TOTAL_TOKENS",
                "MUSE_USAGE_TOTAL_TOKENS",
                self.session.total_tokens.to_string(),
            ),
            (
                "NUR_USAGE_COST_USD",
                "META_USAGE_COST_USD",
                "MUSE_USAGE_COST_USD",
                cost,
            ),
            (
                "NUR_STATUS_PATH",
                "META_STATUS_PATH",
                "MUSE_STATUS_PATH",
                status,
            ),
        ] {
            std::env::set_var(nur_k, &val);
            std::env::set_var(meta_k, &val);
            std::env::set_var(muse_k, &val);
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
            "model": self.model,
            "provider": self.provider,
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
            pricing: PricingInfo {
                input_per_mtok_usd: rates.input_per_mtok_usd,
                output_per_mtok_usd: rates.output_per_mtok_usd,
                cache_read_per_mtok_usd: rates.cache_read_per_mtok_usd,
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
    ) -> Result<()> {
        ensure_dirs()?;
        let line = UsageLogLine {
            ts: Utc::now(),
            session_id: self.session_id.clone(),
            model: self.model.clone(),
            provider: self.provider.clone(),
            response_id,
            usage: usage.clone(),
            estimated_cost_usd: usage.estimated_cost_usd(),
            turn: self.turn,
            pricing_source: rates.source.clone(),
        };
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(usage_log_path())?;
        writeln!(f, "{}", serde_json::to_string(&line)?)?;
        Ok(())
    }
}

/// Print human + machine summary (for `muse usage` / `nur usage`).
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
        "session est. cost USD: ${:.6}  ({})",
        snap.estimated_cost_usd_session,
        if snap.pricing.estimate {
            "list-price estimate"
        } else {
            "reported"
        }
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
        "last request tokens: in={} out={} total={}  est ${:.6}",
        snap.usage_last_request.input_tokens,
        snap.usage_last_request.output_tokens,
        snap.usage_last_request.total_tokens,
        snap.estimated_cost_usd_last
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamped_cost_survives_add() {
        let rates = pricing::builtin_meta_rates("muse-spark-1.1");
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
}

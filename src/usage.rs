//! Token usage tracking for ADEs (Orca, etc.) and local dashboards.
//!
//! Writes:
//! - `~/.muse/status.json` — current session snapshot (poll this from ADEs)
//! - `~/.muse/usage.jsonl` — append-only per-request log
//!
//! Also mirrors into env-friendly fields so host tools can read the last write.

use crate::config::{
    ensure_dirs, status_path, usage_log_path, PRICE_INPUT_PER_MTOK, PRICE_OUTPUT_PER_MTOK,
};
use crate::error::Result;
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
}

impl TokenUsage {
    pub fn add(&mut self, other: &TokenUsage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.total_tokens += other.total_tokens;
        self.reasoning_tokens += other.reasoning_tokens;
        self.cached_tokens += other.cached_tokens;
    }

    pub fn estimated_cost_usd(&self) -> f64 {
        let input = self.input_tokens as f64 / 1_000_000.0 * PRICE_INPUT_PER_MTOK;
        let output = self.output_tokens as f64 / 1_000_000.0 * PRICE_OUTPUT_PER_MTOK;
        input + output
    }
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
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageLogLine {
    pub ts: DateTime<Utc>,
    pub session_id: String,
    pub model: String,
    pub response_id: Option<String>,
    pub usage: TokenUsage,
    pub estimated_cost_usd: f64,
    pub turn: u32,
}

pub struct UsageTracker {
    session_id: String,
    model: String,
    cwd: PathBuf,
    turn: u32,
    session: TokenUsage,
    last: TokenUsage,
    state: String,
    lock: Mutex<()>,
}

impl UsageTracker {
    pub fn new(session_id: String, model: String, cwd: PathBuf) -> Self {
        let t = Self {
            session_id,
            model,
            cwd,
            turn: 0,
            session: TokenUsage::default(),
            last: TokenUsage::default(),
            state: "idle".into(),
            lock: Mutex::new(()),
        };
        let _ = t.write_status();
        t
    }

    pub fn set_model(&mut self, model: String) {
        self.model = model;
        let _ = self.write_status();
    }

    pub fn set_state(&mut self, state: impl Into<String>) {
        self.state = state.into();
        let _ = self.write_status();
    }

    pub fn session_usage(&self) -> &TokenUsage {
        &self.session
    }

    /// Seed cumulative totals when resuming a session (does not append log).
    pub fn seed_session(&mut self, prior: TokenUsage) {
        self.session = prior;
        let _ = self.write_status();
    }

    pub fn record_request(&mut self, usage: TokenUsage, response_id: Option<String>) {
        self.turn += 1;
        self.last = usage.clone();
        self.session.add(&usage);
        let _ = self.append_log(&usage, response_id);
        let _ = self.write_status();
        // ADE-friendly env (current process only — parent may not see, but children hooks can)
        std::env::set_var("MUSE_USAGE_INPUT_TOKENS", self.session.input_tokens.to_string());
        std::env::set_var(
            "MUSE_USAGE_OUTPUT_TOKENS",
            self.session.output_tokens.to_string(),
        );
        std::env::set_var("MUSE_USAGE_TOTAL_TOKENS", self.session.total_tokens.to_string());
        std::env::set_var(
            "MUSE_USAGE_COST_USD",
            format!("{:.6}", self.session.estimated_cost_usd()),
        );
        std::env::set_var("MUSE_STATUS_PATH", status_path().display().to_string());

        // ADE / Orca discovery + optional hook ping
        crate::ade::write_ade_manifest(
            &self.session_id,
            &self.model,
            &self.cwd.display().to_string(),
            &self.session,
        );
        let payload = serde_json::json!({
            "type": "muse.usage",
            "session_id": self.session_id,
            "model": self.model,
            "turn": self.turn,
            "usage": self.session,
            "estimated_cost_usd": self.session.estimated_cost_usd(),
            "status_path": status_path().display().to_string(),
        });
        crate::ade::notify_orca_hook(&payload.to_string());
    }

    fn write_status(&self) -> Result<()> {
        let _g = self.lock.lock().ok();
        ensure_dirs()?;
        let snap = StatusSnapshot {
            schema_version: 1,
            provider: "meta".into(),
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
                input_per_mtok_usd: PRICE_INPUT_PER_MTOK,
                output_per_mtok_usd: PRICE_OUTPUT_PER_MTOK,
                note: "Indicative Meta Model API rates; verify on dev.meta.ai".into(),
            },
            status_path: status_path().display().to_string(),
            usage_log_path: usage_log_path().display().to_string(),
        };
        let text = serde_json::to_string_pretty(&snap)?;
        fs::write(status_path(), text)?;
        // Also write session-scoped status for multi-agent ADE layouts
        let sess_status = crate::config::sessions_dir()
            .join(format!("{}.status.json", self.session_id));
        let _ = fs::write(sess_status, serde_json::to_string_pretty(&snap)?);
        Ok(())
    }

    fn append_log(&self, usage: &TokenUsage, response_id: Option<String>) -> Result<()> {
        ensure_dirs()?;
        let line = UsageLogLine {
            ts: Utc::now(),
            session_id: self.session_id.clone(),
            model: self.model.clone(),
            response_id,
            usage: usage.clone(),
            estimated_cost_usd: usage.estimated_cost_usd(),
            turn: self.turn,
        };
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(usage_log_path())?;
        writeln!(f, "{}", serde_json::to_string(&line)?)?;
        Ok(())
    }
}

/// Print human + machine summary (for `muse usage`).
pub fn print_usage_summary() -> Result<()> {
    let path = status_path();
    if !path.exists() {
        println!("no status yet (run muse first)");
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
        "session tokens: in={} out={} total={} reasoning={}",
        snap.usage_session.input_tokens,
        snap.usage_session.output_tokens,
        snap.usage_session.total_tokens,
        snap.usage_session.reasoning_tokens
    );
    println!(
        "session est. cost USD: ${:.6}",
        snap.estimated_cost_usd_session
    );
    println!(
        "last request tokens: in={} out={} total={}",
        snap.usage_last_request.input_tokens,
        snap.usage_last_request.output_tokens,
        snap.usage_last_request.total_tokens
    );
    Ok(())
}

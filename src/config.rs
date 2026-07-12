use crate::error::{MuseError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

pub const DEFAULT_BASE_URL: &str = "https://api.meta.ai/v1";
pub const DEFAULT_MODEL: &str = "muse-spark-1.1";
pub const DEFAULT_REASONING: &str = "high";

/// Approximate Meta Model API list prices (USD per 1M tokens) for ADE cost display.
/// Update when Meta publishes new rates: https://dev.meta.ai/docs/getting-started/pricing-rate-limits
pub const PRICE_INPUT_PER_MTOK: f64 = 1.25;
pub const PRICE_OUTPUT_PER_MTOK: f64 = 4.25;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default = "default_reasoning")]
    pub reasoning_effort: String,
    #[serde(default = "default_max_turns")]
    pub max_turns: u32,
    #[serde(default = "default_true")]
    pub stream: bool,
    /// Model context window (tokens) — used for the ctx% meter in the TUI.
    #[serde(default = "default_context_window")]
    pub context_window: u64,
}

fn default_model() -> String {
    DEFAULT_MODEL.to_string()
}
fn default_base_url() -> String {
    DEFAULT_BASE_URL.to_string()
}
fn default_reasoning() -> String {
    DEFAULT_REASONING.to_string()
}
fn default_max_turns() -> u32 {
    40
}
fn default_context_window() -> u64 {
    1_000_000
}
fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model: default_model(),
            base_url: default_base_url(),
            reasoning_effort: default_reasoning(),
            max_turns: default_max_turns(),
            stream: true,
            context_window: default_context_window(),
        }
    }
}

pub fn muse_home() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".muse")
}

pub fn config_path() -> PathBuf {
    muse_home().join("config.toml")
}

pub fn auth_path() -> PathBuf {
    muse_home().join("auth.json")
}

pub fn sessions_dir() -> PathBuf {
    muse_home().join("sessions")
}

/// Live status file for ADEs (Orca, etc.) — token usage, model, session.
pub fn status_path() -> PathBuf {
    muse_home().join("status.json")
}

/// Append-only usage log for ADE billing dashboards.
pub fn usage_log_path() -> PathBuf {
    muse_home().join("usage.jsonl")
}

pub fn ensure_dirs() -> Result<()> {
    let home = muse_home();
    fs::create_dir_all(&home)?;
    fs::create_dir_all(sessions_dir())?;
    Ok(())
}

pub fn load_config() -> Result<Config> {
    ensure_dirs()?;
    let path = config_path();
    if !path.exists() {
        let cfg = Config::default();
        save_config(&cfg)?;
        return Ok(cfg);
    }
    let text = fs::read_to_string(&path)?;
    toml::from_str(&text).map_err(|e| MuseError::Config(e.to_string()))
}

pub fn save_config(cfg: &Config) -> Result<()> {
    ensure_dirs()?;
    let text = toml::to_string_pretty(cfg).map_err(|e| MuseError::Config(e.to_string()))?;
    fs::write(config_path(), text)?;
    Ok(())
}

use crate::error::{MuseError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

pub const DEFAULT_BASE_URL: &str = "https://api.meta.ai/v1";
/// Default model id when provider is Meta Model API (wire format). Override via
/// `/model`, `--model`, config, or `NUR_MODEL` / `META_MODEL`.
pub const DEFAULT_MODEL: &str = "muse-spark-1.1";
pub const DEFAULT_REASONING: &str = "high";

/// Pretty-print a model id for the splash title / status only.
/// Example: `muse-spark-1.1` → `Muse Spark 1.1` (vendor model name preserved).
pub fn model_display_name(model_id: &str) -> String {
    let s = model_id.trim();
    if s.is_empty() {
        return "model".into();
    }
    if s.contains(' ') {
        return s.to_string();
    }
    s.split(|c| c == '-' || c == '_')
        .filter(|p| !p.is_empty())
        .map(|p| {
            // Keep version-like tokens (1.1, v2, 70b) mostly as-is.
            let first = p.chars().next().unwrap_or(' ');
            if first.is_ascii_digit() || (p.len() > 1 && first == 'v' && p[1..].chars().all(|c| c.is_ascii_digit() || c == '.')) {
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

/// Approximate Meta Model API list prices (USD per 1M tokens) for usage/cost display.
/// Update when Meta publishes new rates: https://dev.meta.ai/docs/getting-started/pricing-rate-limits
pub const PRICE_INPUT_PER_MTOK: f64 = 1.25;
pub const PRICE_OUTPUT_PER_MTOK: f64 = 4.25;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
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
    /// Cost-saver: skip PLUR auto-inject, skills catalog, and long memory in the system prompt.
    #[serde(default)]
    pub poor_mode: bool,
    /// When true (default), TUI open background-repairs graphify/plur/ruflo/browser packs.
    /// Set false for a pure binary + chat experience until `nur ecosystem ensure`.
    #[serde(default = "default_true")]
    pub ecosystem_auto_ensure: bool,
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
    40
}
fn default_context_window() -> u64 {
    1_000_000
}
fn default_tool_result_max_chars() -> u64 {
    12_000
}
fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
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
        }
    }
}

fn default_compact_keep_user_turns() -> u32 {
    4
}
fn default_compact_tool_body_max() -> u64 {
    800
}

/// Oldest legacy home (`~/.muse`). Still read for migration.
pub fn legacy_muse_home() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".muse")
}

/// Previous product home (`~/.nur`) before NurCLI rebrand. Gap-filled into [`nur_home`].
pub fn legacy_meta_home() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".meta")
}

/// NurCLI data home: `~/.nur` (secrets, sessions, status, skills, memory).
/// Override: `NUR_HOME`, then legacy `META_HOME` / `MUSE_HOME`.
pub fn nur_home() -> PathBuf {
    for var in ["NUR_HOME", "META_HOME", "MUSE_HOME"] {
        if let Ok(h) = std::env::var(var) {
            let h = h.trim();
            if !h.is_empty() {
                return PathBuf::from(h);
            }
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".nur")
}

/// Deprecated alias — always [`nur_home`].
pub fn meta_home() -> PathBuf {
    nur_home()
}

/// Deprecated alias — always [`nur_home`].
pub fn muse_home() -> PathBuf {
    nur_home()
}

pub fn config_path() -> PathBuf {
    nur_home().join("config.toml")
}

pub fn auth_path() -> PathBuf {
    nur_home().join("auth.json")
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

const MIGRATE_FILES: &[&str] = &[
    "auth.json",
    "config.toml",
    "memory.md",
    "history.jsonl",
    "latest_session.json",
    "cwd_sessions.json",
    "usage.jsonl",
    "status.json",
    "ade.json",
    "ecosystem.json",
];

/// Gap-fill one legacy root into `dest` (never overwrites existing files).
fn gap_fill_from(legacy: &std::path::Path, dest: &std::path::Path) {
    if !legacy.is_dir() || legacy == dest {
        return;
    }
    for name in MIGRATE_FILES {
        let src = legacy.join(name);
        let dst = dest.join(name);
        if src.is_file() && !dst.exists() {
            let _ = fs::copy(&src, &dst);
        }
    }
    let src_sess = legacy.join("sessions");
    let dst_sess = dest.join("sessions");
    if src_sess.is_dir() {
        let _ = fs::create_dir_all(&dst_sess);
        if let Ok(entries) = fs::read_dir(&src_sess) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_file() {
                    let dst = dst_sess.join(e.file_name());
                    if !dst.exists() {
                        let _ = fs::copy(&p, &dst);
                    }
                }
            }
        }
    }
    for name in ["skills", "ruflo", "skill-packs"] {
        let src = legacy.join(name);
        let dst = dest.join(name);
        if src.is_dir() {
            let _ = copy_dir_recursive(&src, &dst);
        }
    }
}

/// Fill gaps in `~/.nur` from legacy `~/.nur` then `~/.muse` (never overwrites).
fn migrate_legacy_home_if_needed(dest: &std::path::Path) {
    gap_fill_from(&legacy_meta_home(), dest);
    gap_fill_from(&legacy_muse_home(), dest);
}

/// Copy a single missing file from legacy homes into nur home (auth heal).
pub fn promote_legacy_file(name: &str) -> bool {
    let dest = nur_home();
    for legacy in [legacy_meta_home(), legacy_muse_home()] {
        let src = legacy.join(name);
        let dst = dest.join(name);
        if src.is_file() && !dst.exists() {
            let _ = fs::create_dir_all(&dest);
            if fs::copy(&src, &dst).is_ok() {
                return true;
            }
        }
    }
    false
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if !to.exists() {
            let _ = fs::copy(&from, &to);
        }
    }
    Ok(())
}

pub fn ensure_dirs() -> Result<()> {
    let home = nur_home();
    fs::create_dir_all(&home)?;
    fs::create_dir_all(sessions_dir())?;
    migrate_legacy_home_if_needed(&home);
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
        toml::from_str(&text).map_err(|e| MuseError::Config(e.to_string()))?
    };
    // Self-hosted OpenAI-compat (Ollama, vLLM, LiteLLM, custom gateways).
    apply_base_url_env(&mut cfg);
    cfg.validate()?;
    Ok(cfg)
}

/// Apply `NUR_BASE_URL` / legacy `META_BASE_URL` env override onto a config.
pub fn apply_base_url_env(cfg: &mut Config) {
    for var in ["NUR_BASE_URL", "META_BASE_URL"] {
        if let Ok(u) = std::env::var(var) {
            let u = u.trim().trim_end_matches('/').to_string();
            if !u.is_empty() {
                cfg.base_url = u;
                return;
            }
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
    let text = toml::to_string_pretty(cfg).map_err(|e| MuseError::Config(e.to_string()))?;
    atomic_write(&config_path(), text.as_bytes())
        .map_err(|e| MuseError::Other(format!("save_config atomic write failed: {e}")))?;
    Ok(())
}

pub const VALID_EFFORTS: &[&str] = &["minimal", "low", "medium", "high", "xhigh"];

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_tmp(label: &str) -> PathBuf {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("nur-cli-{label}-{n}"))
    }

    #[test]
    fn migrate_fills_missing_files_without_overwrite() {
        let root = unique_tmp("migrate");
        let legacy_muse = root.join(".muse");
        let legacy_meta = root.join(".meta");
        let nur = root.join(".nur");
        fs::create_dir_all(legacy_muse.join("sessions")).unwrap();
        fs::create_dir_all(&legacy_meta).unwrap();
        fs::create_dir_all(&nur).unwrap();
        fs::write(legacy_muse.join("auth.json"), r#"{"api_key":"k","source":"t"}"#).unwrap();
        fs::write(legacy_meta.join("memory.md"), "from-meta\n").unwrap();
        fs::write(legacy_muse.join("sessions").join("abc.json"), "{}").unwrap();
        // Pre-existing config in nur must not be overwritten
        fs::write(nur.join("config.toml"), "model = \"keep-me\"\n").unwrap();

        gap_fill_from(&legacy_meta, &nur);
        gap_fill_from(&legacy_muse, &nur);

        assert!(
            fs::read_to_string(nur.join("config.toml"))
                .unwrap()
                .contains("keep-me")
        );
        assert!(nur.join("auth.json").is_file());
        assert!(nur.join("memory.md").is_file());
        assert!(nur.join("sessions").join("abc.json").is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn model_display_name_title_cases() {
        assert_eq!(model_display_name(""), "model");
        assert_eq!(model_display_name("  "), "model");
        assert_eq!(model_display_name("muse-spark-1.1"), "Muse Spark 1.1");
    }
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        if !VALID_EFFORTS.contains(&self.reasoning_effort.as_str()) {
            return Err(MuseError::Config(format!(
                "invalid reasoning_effort '{}' — use {}",
                self.reasoning_effort,
                VALID_EFFORTS.join("|")
            )));
        }
        if self.max_turns == 0 || self.max_turns > 200 {
            return Err(MuseError::Config(format!(
                "max_turns {} out of range 1..200",
                self.max_turns
            )));
        }
        if self.context_window < 1000 || self.context_window > 2_000_000 {
            return Err(MuseError::Config(format!(
                "context_window {} out of allowed range",
                self.context_window
            )));
        }
        if self.base_url.is_empty() || !(self.base_url.starts_with("http://") || self.base_url.starts_with("https://")) {
            return Err(MuseError::Config(format!("invalid base_url '{}'", self.base_url)));
        }
        if let Some(c) = self.max_session_cost_usd {
            if !c.is_finite() || c < 0.0 {
                return Err(MuseError::Config(
                    "max_session_cost_usd must be a non-negative number".into(),
                ));
            }
        }
        if let Some(0) = self.max_session_tokens {
            return Err(MuseError::Config(
                "max_session_tokens must be > 0 when set".into(),
            ));
        }
        Ok(())
    }
}

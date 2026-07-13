use crate::error::{MuseError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

pub const DEFAULT_BASE_URL: &str = "https://api.meta.ai/v1";
/// Default Meta Model API model id (wire format). Override via `/model`, `--model`,
/// config, or `META_MODEL`. UI chrome stays model-agnostic except the splash title,
/// which uses [`model_display_name`].
pub const DEFAULT_MODEL: &str = "muse-spark-1.1";
pub const DEFAULT_REASONING: &str = "high";

/// Pretty-print a Meta model id for the splash title / status only.
/// Example: `muse-spark-1.1` → `Muse Spark 1.1`.
pub fn model_display_name(model_id: &str) -> String {
    let s = model_id.trim();
    if s.is_empty() {
        return "Meta model".into();
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
    /// Larger outputs spill to `~/.meta/tool-results/` with a short preview.
    /// `0` = unlimited (legacy behavior).
    #[serde(default = "default_tool_result_max_chars")]
    pub tool_result_max_chars: u64,
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
            base_url: default_base_url(),
            reasoning_effort: default_reasoning(),
            max_turns: default_max_turns(),
            stream: true,
            context_window: default_context_window(),
            tool_result_max_chars: default_tool_result_max_chars(),
        }
    }
}

/// Legacy home from pre-0.5.14 builds (`~/.muse`). Still read for migration.
pub fn legacy_muse_home() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".muse")
}

/// Meta CLI data home: `~/.meta` (secrets, sessions, status, skills, memory).
/// Override with `META_HOME` (or legacy `MUSE_HOME`).
pub fn meta_home() -> PathBuf {
    for var in ["META_HOME", "MUSE_HOME"] {
        if let Ok(h) = std::env::var(var) {
            let h = h.trim();
            if !h.is_empty() {
                return PathBuf::from(h);
            }
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".meta")
}

/// Alias for call sites still named `muse_home` — always resolves to [`meta_home`].
pub fn muse_home() -> PathBuf {
    meta_home()
}

pub fn config_path() -> PathBuf {
    meta_home().join("config.toml")
}

pub fn auth_path() -> PathBuf {
    meta_home().join("auth.json")
}

pub fn sessions_dir() -> PathBuf {
    meta_home().join("sessions")
}

/// Live status file for ADE / host panels — token usage, model, session.
pub fn status_path() -> PathBuf {
    meta_home().join("status.json")
}

/// Append-only usage log for host billing dashboards.
pub fn usage_log_path() -> PathBuf {
    meta_home().join("usage.jsonl")
}

/// Fill gaps in `~/.meta` from legacy `~/.muse` (never overwrites existing files).
///
/// Runs on every `ensure_dirs` so partial upgrades (e.g. default config already
/// written under `.meta` while auth/sessions still live in `.muse`) still heal.
fn migrate_legacy_home_if_needed(meta: &std::path::Path) {
    let legacy = legacy_muse_home();
    if !legacy.is_dir() || legacy == meta {
        return;
    }
    let files = [
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
    for name in files {
        let src = legacy.join(name);
        let dst = meta.join(name);
        if src.is_file() && !dst.exists() {
            let _ = fs::copy(&src, &dst);
        }
    }
    let src_sess = legacy.join("sessions");
    let dst_sess = meta.join("sessions");
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
    // Skills + ruflo DB: merge missing files into dest (so ensure-first
    // creating an empty ruflo/ dir does not strand legacy memory.db).
    for name in ["skills", "ruflo", "skill-packs"] {
        let src = legacy.join(name);
        let dst = meta.join(name);
        if src.is_dir() {
            let _ = copy_dir_recursive(&src, &dst);
        }
    }
}

/// Copy a single missing file from legacy home into meta home (used by auth heal).
pub fn promote_legacy_file(name: &str) -> bool {
    let meta = meta_home();
    let src = legacy_muse_home().join(name);
    let dst = meta.join(name);
    if src.is_file() && !dst.exists() {
        let _ = fs::create_dir_all(&meta);
        return fs::copy(&src, &dst).is_ok();
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
    let home = meta_home();
    fs::create_dir_all(&home)?;
    fs::create_dir_all(sessions_dir())?;
    migrate_legacy_home_if_needed(&home);
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
    let cfg: Config = toml::from_str(&text).map_err(|e| MuseError::Config(e.to_string()))?;
    cfg.validate()?;
    Ok(cfg)
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
        std::env::temp_dir().join(format!("meta-cli-{label}-{n}"))
    }

    #[test]
    fn migrate_fills_missing_files_without_overwrite() {
        let root = unique_tmp("migrate");
        let legacy = root.join(".muse");
        let meta = root.join(".meta");
        fs::create_dir_all(legacy.join("sessions")).unwrap();
        fs::create_dir_all(&meta).unwrap();
        fs::write(legacy.join("auth.json"), r#"{"api_key":"k","source":"t"}"#).unwrap();
        fs::write(legacy.join("config.toml"), "model = \"from-legacy\"\n").unwrap();
        fs::write(legacy.join("sessions").join("abc.json"), "{}").unwrap();
        // Pre-existing config in meta must not be overwritten
        fs::write(meta.join("config.toml"), "model = \"keep-me\"\n").unwrap();

        // Simulate gap-fill (same logic as migrate, with explicit roots)
        for name in ["auth.json", "config.toml"] {
            let src = legacy.join(name);
            let dst = meta.join(name);
            if src.is_file() && !dst.exists() {
                fs::copy(&src, &dst).unwrap();
            }
        }
        let dst_sess = meta.join("sessions");
        fs::create_dir_all(&dst_sess).unwrap();
        for e in fs::read_dir(legacy.join("sessions")).unwrap() {
            let e = e.unwrap();
            let dst = dst_sess.join(e.file_name());
            if !dst.exists() {
                fs::copy(e.path(), dst).unwrap();
            }
        }

        assert_eq!(
            fs::read_to_string(meta.join("config.toml")).unwrap().contains("keep-me"),
            true
        );
        assert!(meta.join("auth.json").is_file());
        assert!(meta.join("sessions").join("abc.json").is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn model_display_name_title_cases() {
        assert_eq!(model_display_name("muse-spark-1.1"), "Muse Spark 1.1");
        assert_eq!(model_display_name("  "), "Meta model");
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
        Ok(())
    }
}

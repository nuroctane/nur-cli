//! penecho compatibility — canvas + provider bridge
//!
//! penecho (https://github.com/penecho/penecho) is "Think with AI beyond the chat box":
//! 20k x 20k canvas, pressure-sensitive ink, draft layer, MathJax, plots,
//! declarative animations. Runtime: Node >=18, 2 deps only (@inquirer/prompts + sharp),
//! no bundler, vanilla JS client served via http.
//!
//! Provider model (from api-config.js, codex-cli.js, claude-cli.js):
//! - `AI_PROVIDER=api|codex-cli|claude-cli`
//! - API mode: `AI_API_URL`, `AI_API_KEY`, `AI_API_MODEL`, `AI_API_FORMAT=openai|anthropic`
//!   auto-detects format from URL suffix `/chat/completions` vs `/v1/messages`
//!   (cleaner than per-provider flags). Supports `AI_EFFORT`, `AI_TIMEOUT_SECONDS`,
//!   placeholder detection `your[_ -]|replace|changeme|sk-\...`.
//! - Codex CLI: `CODEX_CLI_PATH` default `codex`, resolves .exe/.cmd/.bat, .js wrapper,
//!   `codex --version`, `codex login status`, `codex debug models --bundled`.
//! - Claude CLI: `CLAUDE_CLI_PATH` default `claude`, handles .js/.cjs/.mjs => node prefix,
//!   .ps1 on win, system prompt + user prompt split.
//!
//! This module mirrors penecho's ideas without copying AGPL code:
//! - Env mapping: export nur auth to penecho's `~/.penecho/config.env` format.
//! - CLI probing: `findOnPath` with extension handling, like penecho's robust Windows logic.
//! - Effort mapping: unified `config|none|low|medium|high|max|xhigh` → provider-specific
//!   thinking tokens.
//! - Sidecar launch: `nur penecho --install` (npm i -g penecho) + config generator.
//! - Tool adapter: `penecho` tool to launch/check status, similar to `akarso`/`t3code`.
//!
//! License note: penecho is AGPL-3.0-only. We integrate via process spawn / sidecar,
//! not linking code, to stay compliant.

use crate::error::{MuseError, Result};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Unified effort levels — penecho's single UI knob that maps to provider-specific.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effort {
    Config,
    None,
    Low,
    Medium,
    High,
    Max,
    XHigh,
}

impl Effort {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" | "0" => Effort::None,
            "low" => Effort::Low,
            "medium" | "med" => Effort::Medium,
            "high" => Effort::High,
            "max" => Effort::Max,
            "xhigh" | "extra" => Effort::XHigh,
            _ => Effort::Config,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Effort::Config => "config",
            Effort::None => "none",
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
            Effort::Max => "max",
            Effort::XHigh => "xhigh",
        }
    }

    /// Map to anthropic thinking + token budget, like penecho's `anthropicEffortParameters()`.
    pub fn to_anthropic_params(self) -> (Option<&'static str>, u32) {
        match self {
            Effort::None => (Some("disabled"), 8192),
            Effort::Low => (Some("adaptive"), 8192),
            Effort::Medium => (Some("adaptive"), 8192),
            Effort::High => (Some("adaptive"), 8192),
            Effort::Max => (Some("adaptive"), 16384),
            Effort::XHigh => (Some("adaptive"), 16384),
            Effort::Config => (None, 8192),
        }
    }

    /// Map to OpenAI reasoning_effort.
    pub fn to_openai_reasoning(self) -> Option<&'static str> {
        match self {
            Effort::None => None,
            Effort::Low => Some("low"),
            Effort::Medium => Some("medium"),
            Effort::High => Some("high"),
            Effort::Max => Some("max"),
            Effort::XHigh => Some("xhigh"),
            Effort::Config => None,
        }
    }
}

/// penecho provider abstraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PenProvider {
    Api,
    CodexCli,
    ClaudeCli,
}

impl PenProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            PenProvider::Api => "api",
            PenProvider::CodexCli => "codex-cli",
            PenProvider::ClaudeCli => "claude-cli",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "codex-cli" | "codex" => PenProvider::CodexCli,
            "claude-cli" | "claude" => PenProvider::ClaudeCli,
            _ => PenProvider::Api,
        }
    }
}

/// Resolve API config like penecho's `resolveApiConfig()` — auto-detect openai vs anthropic
/// from URL suffix, normalize endpoint, validate.
#[derive(Debug, Clone)]
pub struct ApiConfig {
    pub url: String,
    pub key: String,
    pub model: String,
    pub format: String, // "openai" or "anthropic"
    pub image_format: String,
}

pub fn resolve_api_config(url: &str, key: &str, model: &str, format_override: Option<&str>) -> Result<ApiConfig> {
    let url = url.trim();
    if url.is_empty() {
        return Err(MuseError::Other("AI_API_URL empty".into()));
    }
    // Validate http/https, no user/pass
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(MuseError::Other(format!("AI_API_URL must be http(s): {url}")));
    }
    if url.contains('@') && url.contains("://") {
        // crude check for user:pass@
        let after_scheme = url.split("://").nth(1).unwrap_or("");
        if after_scheme.contains('@') && after_scheme.split('@').next().unwrap_or("").contains(':') {
            return Err(MuseError::Other("AI_API_URL must not contain credentials".into()));
        }
    }

    let format = if let Some(f) = format_override {
        f.to_string()
    } else if url.ends_with("/v1/messages") {
        "anthropic".to_string()
    } else if url.ends_with("/chat/completions") {
        "openai".to_string()
    } else if url.ends_with("/v1") || url.contains("/openai") {
        "openai".to_string()
    } else {
        // default heuristic like penecho: if contains anthropic -> anthropic else openai
        if url.contains("anthropic") {
            "anthropic".to_string()
        } else {
            "openai".to_string()
        }
    };

    let normalized = if format == "anthropic" {
        if url.ends_with("/v1/messages") {
            url.to_string()
        } else if url.ends_with("/v1") {
            format!("{}/messages", url.trim_end_matches('/'))
        } else {
            url.to_string()
        }
    } else {
        if url.ends_with("/chat/completions") {
            url.to_string()
        } else if url.ends_with("/v1") {
            format!("{}/chat/completions", url.trim_end_matches('/'))
        } else {
            url.to_string()
        }
    };

    // Placeholder detection like penecho
    let lower_key = key.to_ascii_lowercase();
    if lower_key.contains("your_")
        || lower_key.contains("your-")
        || lower_key.contains("replace")
        || lower_key.contains("changeme")
        || lower_key.contains("api_key")
        || lower_key.contains("api-key")
        || lower_key.trim() == "sk-..."
    {
        return Err(MuseError::Other(
            "API key looks like placeholder (your_*/replace/changeme)".into(),
        ));
    }

    Ok(ApiConfig {
        url: normalized,
        key: key.to_string(),
        model: if model.trim().is_empty() {
            "gpt-4o".to_string()
        } else {
            model.to_string()
        },
        format,
        image_format: "webp".to_string(),
    })
}

/// Find binary on PATH with Windows extension handling, like penecho's `findOnPath`
/// + t3code's driver probing. Handles .exe/.cmd/.bat/.com and .js wrappers.
pub fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        // direct
        let c = dir.join(name);
        if c.is_file() {
            return Some(c);
        }
        #[cfg(windows)]
        {
            for ext in &["exe", "cmd", "bat", "com"] {
                let p = dir.join(format!("{name}.{ext}"));
                if p.is_file() {
                    return Some(p);
                }
            }
        }
        // .js wrapper (npm)
        let js = dir.join(format!("{name}.js"));
        if js.is_file() {
            return Some(js);
        }
    }
    // Extra common dirs (like gcloud_bin, etc.)
    if let Some(home) = dirs::home_dir() {
        let extra = [
            home.join(".local").join("bin"),
            home.join("bin"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/opt/homebrew/bin"),
        ];
        for dir in extra {
            let c = dir.join(name);
            if c.is_file() {
                return Some(c);
            }
        }
    }
    None
}

/// Probe penecho itself — is binary installed, config exists, etc.
#[derive(Debug, Clone)]
pub struct ProbeStatus {
    pub binary: Option<PathBuf>,
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub config_exists: bool,
    pub has_api_key: bool,
}

pub fn probe() -> ProbeStatus {
    let binary = find_on_path("penecho");
    let config_dir = penecho_state_dir();
    let config_file = config_dir.join("config.env");
    let config_exists = config_file.exists();
    let has_api_key = if config_exists {
        if let Ok(content) = fs::read_to_string(&config_file) {
            content.contains("AI_API_KEY") || content.contains("OPENAI_API_KEY")
        } else {
            false
        }
    } else {
        false
    };
    ProbeStatus {
        binary,
        config_dir,
        config_file,
        config_exists,
        has_api_key,
    }
}

pub fn penecho_state_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("PENECHO_STATE_DIR") {
        let p = PathBuf::from(dir);
        if p.is_absolute() {
            return p;
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".penecho")
}

/// Export nur auth to penecho config.env format — bridge like `nur auth export --format penecho`.
///
/// Maps nur's unified auth to penecho's env:
/// - `AI_PROVIDER=api`
/// - `AI_API_URL`, `AI_API_KEY`, `AI_API_MODEL`, `AI_API_FORMAT`
/// - `AI_EFFORT`
pub fn export_to_penecho_env(
    api_url: &str,
    api_key: &str,
    model: &str,
    effort: Effort,
) -> Result<String> {
    let cfg = resolve_api_config(api_url, api_key, model, None)?;
    let mut out = String::new();
    out.push_str("# Generated by nur-cli `penecho` bridge — https://github.com/penecho/penecho\n");
    out.push_str(&format!("AI_PROVIDER={}\n", PenProvider::Api.as_str()));
    out.push_str(&format!("AI_API_URL={}\n", cfg.url));
    out.push_str(&format!("AI_API_KEY={}\n", cfg.key));
    out.push_str(&format!("AI_API_MODEL={}\n", cfg.model));
    out.push_str(&format!("AI_API_FORMAT={}\n", cfg.format));
    out.push_str(&format!("AI_EFFORT={}\n", effort.as_str()));
    out.push_str("PENECHO_AI_IMAGE_FORMAT=webp\n");
    out.push_str("# Legacy OPENAI_* fallback for older penecho\n");
    out.push_str(&format!("OPENAI_API_URL={}\n", cfg.url));
    out.push_str(&format!("OPENAI_API_KEY={}\n", cfg.key));
    out.push_str(&format!("OPENAI_MODEL={}\n", cfg.model));
    Ok(out)
}

/// Write penecho config.env atomically (mirrors t3code atomicWrite)
pub fn write_config_env(contents: &str) -> Result<PathBuf> {
    let dir = penecho_state_dir();
    let file = dir.join("config.env");
    crate::t3code::atomic_write(&file, contents.as_bytes())
        .map_err(|e| MuseError::Other(format!("atomic write penecho config: {e}")))?;
    Ok(file)
}

/// Launch penecho server as sidecar — returns child handle.
/// Does not link AGPL code, spawns via process (compliant).
pub fn launch(extra_args: &[String]) -> Result<std::process::Child> {
    let bin = find_on_path("penecho").ok_or_else(|| {
        MuseError::Other(
            "penecho binary not found on PATH. Install via `npm i -g penecho` or `nur penecho --install`".into(),
        )
    })?;
    let mut cmd = Command::new(bin);
    cmd.args(extra_args);
    cmd.spawn()
        .map_err(|e| MuseError::Other(format!("spawn penecho: {e}")))
}

/// Doctor checks — mirrors `cli.js doctor` in penecho.
#[derive(Debug, Clone)]
pub struct DoctorReport {
    pub penecho_binary: bool,
    pub config_exists: bool,
    pub api_url_valid: bool,
    pub api_key_present: bool,
    pub codex_binary: bool,
    pub claude_binary: bool,
}

pub fn doctor() -> DoctorReport {
    let st = probe();
    let codex = find_on_path("codex").is_some();
    let claude = find_on_path("claude").is_some();
    let api_url_valid = if st.config_exists {
        if let Ok(content) = fs::read_to_string(&st.config_file) {
            // crude check for URL
            content.lines().any(|l| l.contains("AI_API_URL=") && l.contains("http"))
        } else {
            false
        }
    } else {
        false
    };
    DoctorReport {
        penecho_binary: st.binary.is_some(),
        config_exists: st.config_exists,
        api_url_valid,
        api_key_present: st.has_api_key,
        codex_binary: codex,
        claude_binary: claude,
    }
}

/// Canvas -> image atlas concept — penecho's visual request is cropped tiles + focus insets.
/// For nur-cli, we expose a helper that would take a screenshot path and produce an atlas description
/// (future: use image crate to crop, like penecho's sharp).
pub fn describe_atlas(image_path: &Path, focus: Option<(u32, u32, u32, u32)>) -> String {
    format!(
        "atlas: image={} focus={:?} — penecho crops to ink + 1 tile margin + bounded downscale, then encodes webp/png via sharp. Nur could use image crate similarly for `nur draw` / `nur canvas`.",
        image_path.display(),
        focus
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effort_maps() {
        assert_eq!(Effort::parse("max").as_str(), "max");
        assert_eq!(Effort::parse("low").to_openai_reasoning(), Some("low"));
        assert_eq!(Effort::High.to_anthropic_params().1, 8192);
        assert_eq!(Effort::Max.to_anthropic_params().1, 16384);
    }

    #[test]
    fn api_config_auto_detect() {
        let cfg = resolve_api_config("https://api.openai.com/v1", "sk-test", "gpt-4o", None).unwrap();
        assert_eq!(cfg.format, "openai");
        assert!(cfg.url.ends_with("/chat/completions"));

        let cfg2 = resolve_api_config(
            "https://api.anthropic.com/v1/messages",
            "sk-ant-xxx",
            "claude-3",
            None,
        )
        .unwrap();
        assert_eq!(cfg2.format, "anthropic");
    }

    #[test]
    fn placeholder_detection() {
        let err = resolve_api_config(
            "https://api.openai.com/v1",
            "your_api_key_here",
            "gpt-4o",
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("placeholder"));
    }

    #[test]
    fn export_env_format() {
        let s = export_to_penecho_env(
            "https://api.openai.com/v1",
            "sk-real123",
            "gpt-4o",
            Effort::Medium,
        )
        .unwrap();
        assert!(s.contains("AI_PROVIDER=api"));
        assert!(s.contains("AI_API_KEY=sk-real123"));
        assert!(s.contains("AI_EFFORT=medium"));
    }
}

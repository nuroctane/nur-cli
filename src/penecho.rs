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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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
#[allow(dead_code)]
pub enum PenProvider {
    Api,
    CodexCli,
    ClaudeCli,
    KimiCli,
}

#[allow(dead_code)]
impl PenProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            PenProvider::Api => "api",
            PenProvider::CodexCli => "codex-cli",
            PenProvider::ClaudeCli => "claude-cli",
            PenProvider::KimiCli => "kimi-cli",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "codex-cli" | "codex" => PenProvider::CodexCli,
            "claude-cli" | "claude" => PenProvider::ClaudeCli,
            "kimi-cli" | "kimi" => PenProvider::KimiCli,
            _ => PenProvider::Api,
        }
    }
}

/// Resolve API config like penecho's `resolveApiConfig()` — auto-detect openai vs anthropic
/// from URL suffix, normalize endpoint, validate.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ApiConfig {
    pub url: String,
    pub key: String,
    pub model: String,
    pub format: String, // "openai" or "anthropic"
    pub image_format: String,
}

pub fn resolve_api_config(
    url: &str,
    key: &str,
    model: &str,
    format_override: Option<&str>,
) -> Result<ApiConfig> {
    let url = url.trim();
    if url.is_empty() {
        return Err(MuseError::Other("AI_API_URL empty".into()));
    }
    // Validate http/https, no user/pass
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(MuseError::Other(format!(
            "AI_API_URL must be http(s): {url}"
        )));
    }
    if url.contains('@') && url.contains("://") {
        // crude check for user:pass@
        let after_scheme = url.split("://").nth(1).unwrap_or("");
        if after_scheme.contains('@') && after_scheme.split('@').next().unwrap_or("").contains(':')
        {
            return Err(MuseError::Other(
                "AI_API_URL must not contain credentials".into(),
            ));
        }
    }

    let format = if let Some(f) = format_override {
        f.to_string()
    } else if url.ends_with("/v1/messages") {
        "anthropic".to_string()
    } else if url.ends_with("/chat/completions") || url.ends_with("/v1") || url.contains("/openai")
    {
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
    /// Directory holding config.env — kept for future multi-file diagnostics
    /// (`doctor`/`probe` report `config_file` directly today).
    #[allow(dead_code)]
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
///
/// Placeholder emitted instead of the real key whenever the rendered config
/// could be seen by anything other than the local filesystem.
pub const REDACTED_KEY: &str = "<redacted — set AI_API_KEY yourself in ~/.penecho/config.env>";

pub fn export_to_penecho_env(
    api_url: &str,
    api_key: &str,
    model: &str,
    effort: Effort,
    redact_key: bool,
) -> Result<String> {
    let cfg = resolve_api_config(api_url, api_key, model, None)?;
    // The key is validated above but must not be *rendered* for any caller that
    // hands this string back to the agent: a tool result is sent to the model
    // provider and persisted verbatim into `~/.nur/sessions/*.json` (plus a
    // `.bak`), so echoing it writes the user's credential to disk in cleartext
    // and ships it to whichever provider happens to be active — possibly not
    // the one the key belongs to.
    let rendered_key = if redact_key { REDACTED_KEY } else { &cfg.key };
    let mut out = String::new();
    out.push_str("# Generated by nur-cli `penecho` bridge — https://github.com/penecho/penecho\n");
    out.push_str(&format!("AI_PROVIDER={}\n", PenProvider::Api.as_str()));
    out.push_str(&format!("AI_API_URL={}\n", cfg.url));
    out.push_str(&format!("AI_API_KEY={rendered_key}\n"));
    out.push_str(&format!("AI_API_MODEL={}\n", cfg.model));
    out.push_str(&format!("AI_API_FORMAT={}\n", cfg.format));
    out.push_str(&format!("AI_EFFORT={}\n", effort.as_str()));
    out.push_str("PENECHO_AI_IMAGE_FORMAT=webp\n");
    out.push_str("# Legacy OPENAI_* fallback for older penecho\n");
    out.push_str(&format!("OPENAI_API_URL={}\n", cfg.url));
    out.push_str(&format!("OPENAI_API_KEY={rendered_key}\n"));
    out.push_str(&format!("OPENAI_MODEL={}\n", cfg.model));
    Ok(out)
}

/// Default HTTP port used by penecho (`cli.js` DEFAULT_PORT).
pub const DEFAULT_PORT: u16 = 3888;

/// Write penecho config.env atomically (mirrors t3code atomicWrite).
/// Callers that write real secrets must never return those contents to the model.
pub fn write_config_env(contents: &str) -> Result<PathBuf> {
    let dir = penecho_state_dir();
    let file = dir.join("config.env");
    crate::t3code::atomic_write(&file, contents.as_bytes())
        .map_err(|e| MuseError::Other(format!("atomic write penecho config: {e}")))?;
    // Best-effort owner-only perms on Unix (penecho docs recommend this).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&file, fs::Permissions::from_mode(0o600));
    }
    Ok(file)
}

/// Ensure `penecho` is on PATH via `npm i -g penecho` when Node is available.
/// Idempotent. Returns a short non-secret status string.
pub fn ensure_installed() -> Result<String> {
    if let Some(bin) = find_on_path("penecho") {
        let ver = Command::new(&bin)
            .arg("--version")
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
                } else {
                    None
                }
            })
            .filter(|s| !s.is_empty());
        return Ok(match ver {
            Some(v) => format!("penecho ready ({v}) at {}", bin.display()),
            None => format!("penecho ready at {}", bin.display()),
        });
    }
    let node_ok = find_on_path("node").is_some() || find_on_path("node.exe").is_some();
    if !node_ok {
        return Err(MuseError::Other(
            "penecho needs Node.js 20.3+ — install Node, then re-run (nur auto-installs penecho)".into(),
        ));
    }
    let npm = find_on_path("npm")
        .or_else(|| find_on_path("npm.cmd"))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "npm".into());
    let out = Command::new(&npm)
        .args(["install", "-g", "penecho@latest"])
        .output()
        .map_err(|e| MuseError::Other(format!("npm install -g penecho failed to start: {e}")))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(MuseError::Other(format!(
            "npm install -g penecho failed: {}",
            err.chars().take(300).collect::<String>()
        )));
    }
    if let Some(bin) = find_on_path("penecho") {
        Ok(format!("installed penecho via npm at {}", bin.display()))
    } else {
        Err(MuseError::Other(
            "penecho not found on PATH after npm install — open a new shell or check npm global bin"
                .into(),
        ))
    }
}

/// Whether ~/.penecho/config.env already looks usable (no placeholders / redacted).
pub fn config_is_usable() -> bool {
    let file = penecho_state_dir().join("config.env");
    let Ok(content) = fs::read_to_string(&file) else {
        return false;
    };
    if content.contains(REDACTED_KEY) {
        return false;
    }
    let provider = content
        .lines()
        .find_map(|l| l.strip_prefix("AI_PROVIDER="))
        .map(str::trim)
        .unwrap_or("api");
    match provider {
        "codex-cli" | "codex" | "claude-cli" | "claude" | "kimi-cli" | "kimi" => {
            // CLI modes are usable if the matching binary exists (or user configured intentionally).
            true
        }
        _ => {
            // API mode needs a non-placeholder key + URL + model.
            let has_key = content.lines().any(|l| {
                (l.starts_with("AI_API_KEY=") || l.starts_with("OPENAI_API_KEY="))
                    && !l.contains("your_")
                    && !l.contains("changeme")
                    && !l.contains("replace")
                    && !l.contains("sk-...")
                    && l.split_once('=').map(|(_, v)| !v.trim().is_empty()).unwrap_or(false)
            });
            let has_url = content
                .lines()
                .any(|l| (l.starts_with("AI_API_URL=") || l.starts_with("OPENAI_API_URL=")) && l.contains("http"));
            let has_model = content.lines().any(|l| {
                (l.starts_with("AI_API_MODEL=") || l.starts_with("OPENAI_MODEL="))
                    && l.split_once('=')
                        .map(|(_, v)| !v.trim().is_empty())
                        .unwrap_or(false)
            });
            has_key && has_url && has_model
        }
    }
}

/// Pick the best penecho provider mode from nur auth + available CLIs.
/// Secrets are written only to disk — never returned.
#[derive(Debug, Clone)]
pub enum AutoConfigMode {
    Api {
        /// Resolved AI_API_URL — kept for future doctor/report detail beyond
        /// the format+model already surfaced in launch reports.
        #[allow(dead_code)]
        url: String,
        model: String,
        format: String,
    },
    CodexCli,
    ClaudeCli,
    KimiCli,
}

/// Auto-write `~/.penecho/config.env` from nur auth / detected CLIs.
/// Returns a **non-secret** summary. Skips rewrite when config is already usable
/// unless `force` is true.
pub fn auto_configure_from_nur(force: bool, effort: Effort) -> Result<(AutoConfigMode, String)> {
    if !force && config_is_usable() {
        let mode = detect_mode_from_config().unwrap_or(AutoConfigMode::Api {
            url: "https://api.openai.com/v1".into(),
            model: "gpt-4o".into(),
            format: "openai".into(),
        });
        return Ok((
            mode,
            format!(
                "config already ready at {}",
                penecho_state_dir().join("config.env").display()
            ),
        ));
    }

    let cfg = crate::config::load_config().unwrap_or_default();
    let auth = crate::auth::load_auth().ok().flatten();
    let is_oauth = auth
        .as_ref()
        .map(|a| matches!(a.auth_method, crate::auth::AuthMethod::Oauth))
        .unwrap_or(false);
    let auth_provider = auth
        .as_ref()
        .map(|a| a.provider.to_ascii_lowercase())
        .unwrap_or_default();

    // OAuth sessions are often CLI-backed — prefer codex/claude over raw token as API key.
    if is_oauth {
        if (auth_provider.contains("anthropic")
            || auth_provider.contains("claude")
            || auth_provider == "antigravity")
            && find_on_path("claude").is_some()
        {
            let body = format!(
                "# Generated by nur-cli penecho bridge (auto)\n\
                 # Mode: Claude CLI (active nur OAuth session for {auth_provider})\n\
                 AI_PROVIDER=claude-cli\n\
                 AI_EFFORT={}\n\
                 PENECHO_AI_IMAGE_FORMAT=webp\n",
                effort.as_str()
            );
            let path = write_config_env(&body)?;
            return Ok((
                AutoConfigMode::ClaudeCli,
                format!("wrote Claude CLI mode → {}", path.display()),
            ));
        }
        if (auth_provider.contains("openai")
            || auth_provider.contains("codex")
            || auth_provider.is_empty())
            && find_on_path("codex").is_some()
        {
            let body = format!(
                "# Generated by nur-cli penecho bridge (auto)\n\
                 # Mode: Codex CLI (active nur OAuth session)\n\
                 AI_PROVIDER=codex-cli\n\
                 AI_EFFORT={}\n\
                 PENECHO_AI_IMAGE_FORMAT=webp\n",
                effort.as_str()
            );
            let path = write_config_env(&body)?;
            return Ok((
                AutoConfigMode::CodexCli,
                format!("wrote Codex CLI mode → {}", path.display()),
            ));
        }
    }

    // Prefer a real API key for the active provider (or generic).
    let key = crate::auth::resolve_api_key_for(Some(cfg.provider.as_str()))
        .or_else(|_| crate::auth::resolve_api_key())
        .ok()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty());

    if let Some(key) = key {
        // Skip OAuth access tokens for API mode when a matching CLI exists —
        // ChatGPT/Claude OAuth tokens are not standard AI_API_KEY values.
        let looks_like_oauth_jwt = key.starts_with("eyJ") || key.len() > 200;
        if !(is_oauth && looks_like_oauth_jwt) {
            let url = if cfg.base_url.trim().is_empty() {
                "https://api.openai.com/v1".to_string()
            } else {
                cfg.base_url.clone()
            };
            let model = if cfg.model.trim().is_empty() {
                "gpt-4o".to_string()
            } else {
                cfg.model.clone()
            };
            // Write REAL key to disk only (redact_key=false). Never return this string.
            let body = export_to_penecho_env(&url, &key, &model, effort, false)?;
            let path = write_config_env(&body)?;
            let format = if url.contains("anthropic") || url.ends_with("/v1/messages") {
                "anthropic"
            } else {
                "openai"
            };
            return Ok((
                AutoConfigMode::Api {
                    url: url.clone(),
                    model: model.clone(),
                    format: format.into(),
                },
                format!(
                    "wrote API mode from nur auth (provider={}, model={}) → {}",
                    cfg.provider,
                    model,
                    path.display()
                ),
            ));
        }
    }

    // CLI fallbacks when no API key is usable.
    if find_on_path("codex").is_some() {
        let body = format!(
            "# Generated by nur-cli penecho bridge (auto)\n\
             AI_PROVIDER=codex-cli\n\
             AI_EFFORT={}\n\
             PENECHO_AI_IMAGE_FORMAT=webp\n",
            effort.as_str()
        );
        let path = write_config_env(&body)?;
        return Ok((
            AutoConfigMode::CodexCli,
            format!("wrote Codex CLI mode → {}", path.display()),
        ));
    }
    if find_on_path("claude").is_some() {
        let body = format!(
            "# Generated by nur-cli penecho bridge (auto)\n\
             AI_PROVIDER=claude-cli\n\
             AI_EFFORT={}\n\
             PENECHO_AI_IMAGE_FORMAT=webp\n",
            effort.as_str()
        );
        let path = write_config_env(&body)?;
        return Ok((
            AutoConfigMode::ClaudeCli,
            format!("wrote Claude CLI mode → {}", path.display()),
        ));
    }
    if find_on_path("kimi").is_some() {
        let body = format!(
            "# Generated by nur-cli penecho bridge (auto)\n\
             AI_PROVIDER=kimi-cli\n\
             AI_EFFORT={}\n\
             PENECHO_AI_IMAGE_FORMAT=webp\n",
            effort.as_str()
        );
        let path = write_config_env(&body)?;
        return Ok((
            AutoConfigMode::KimiCli,
            format!("wrote Kimi CLI mode → {}", path.display()),
        ));
    }

    Err(MuseError::Other(
        "no penecho provider available — run /login (API key) or install `codex` / `claude` / `kimi` CLI"
            .into(),
    ))
}

fn detect_mode_from_config() -> Option<AutoConfigMode> {
    let file = penecho_state_dir().join("config.env");
    let content = fs::read_to_string(file).ok()?;
    let provider = content
        .lines()
        .find_map(|l| l.strip_prefix("AI_PROVIDER="))
        .map(str::trim)
        .unwrap_or("api");
    match provider {
        "codex-cli" | "codex" => Some(AutoConfigMode::CodexCli),
        "claude-cli" | "claude" => Some(AutoConfigMode::ClaudeCli),
        "kimi-cli" | "kimi" => Some(AutoConfigMode::KimiCli),
        _ => {
            let url = content
                .lines()
                .find_map(|l| l.strip_prefix("AI_API_URL=").or_else(|| l.strip_prefix("OPENAI_API_URL=")))
                .unwrap_or("https://api.openai.com/v1")
                .to_string();
            let model = content
                .lines()
                .find_map(|l| {
                    l.strip_prefix("AI_API_MODEL=")
                        .or_else(|| l.strip_prefix("OPENAI_MODEL="))
                })
                .unwrap_or("gpt-4o")
                .to_string();
            let format = content
                .lines()
                .find_map(|l| l.strip_prefix("AI_API_FORMAT="))
                .unwrap_or(if url.contains("anthropic") {
                    "anthropic"
                } else {
                    "openai"
                })
                .to_string();
            Some(AutoConfigMode::Api { url, model, format })
        }
    }
}

pub fn canvas_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

pub fn port_is_open(port: u16) -> bool {
    use std::net::{SocketAddr, TcpStream};
    use std::time::Duration;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(250)).is_ok()
}

fn wait_for_port(port: u16, timeout_ms: u64) -> bool {
    use std::time::{Duration, Instant};
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(timeout_ms) {
        if port_is_open(port) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    false
}

/// Launch penecho server as a detached sidecar (AGPL-compliant process spawn).
/// Prefer [`launch_seamless`] for the full ensure → config → open path.
pub fn launch(extra_args: &[String]) -> Result<std::process::Child> {
    let bin = find_on_path("penecho").ok_or_else(|| {
        MuseError::Other(
            "penecho binary not found on PATH. Install via `npm i -g penecho` or ecosystem ensure"
                .into(),
        )
    })?;
    let mut cmd = Command::new(&bin);
    cmd.args(extra_args);
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS | CREATE_NO_WINDOW
        cmd.creation_flags(0x0000_0200 | 0x0000_0008 | 0x0800_0000);
    }
    // Unix: null stdio is enough for non-interactive start (skips update Y/N prompts).
    cmd.spawn()
        .map_err(|e| MuseError::Other(format!("spawn penecho: {e}")))
}

/// Full seamless path on the default port. Prefer [`launch_seamless_on_port`]
/// when a caller needs a custom port or an inject seed (the `penecho` tool
/// always does); kept as the simple entry point for direct callers.
#[allow(dead_code)]
pub fn launch_seamless(open_browser: bool, effort: Effort) -> Result<String> {
    let mut notes = Vec::new();

    match ensure_installed() {
        Ok(msg) => notes.push(msg),
        Err(e) => return Err(e),
    }

    let (mode, cfg_msg) = auto_configure_from_nur(false, effort)?;
    notes.push(cfg_msg);

    let port = DEFAULT_PORT;
    let url = canvas_url(port);

    if port_is_open(port) {
        notes.push(format!("already listening on {url}"));
        if open_browser {
            match crate::open_uri::open(&url) {
                Ok(()) => notes.push("opened canvas in your default browser".into()),
                Err(e) => notes.push(format!(
                    "could not open browser ({e}) — open {url} manually"
                )),
            }
        }
        return Ok(format_launch_report(&notes, &mode, &url, true));
    }

    // Provider flag makes start non-interactive even if config is partial.
    let flag = match &mode {
        AutoConfigMode::Api { .. } => "--api",
        AutoConfigMode::CodexCli => "--codex",
        AutoConfigMode::ClaudeCli => "--claude",
        AutoConfigMode::KimiCli => "--kimi",
    };
    let args = vec![
        flag.to_string(),
        "--port".into(),
        port.to_string(),
    ];
    let _child = launch(&args)?;
    notes.push(format!("spawned penecho ({flag}) on port {port}"));

    if wait_for_port(port, 12_000) {
        notes.push("server ready".into());
    } else {
        notes.push(
            "server did not accept connections within 12s — it may still be starting; try the URL"
                .into(),
        );
    }

    if open_browser {
        match crate::open_uri::open(&url) {
            Ok(()) => notes.push("opened canvas in your default browser".into()),
            Err(e) => notes.push(format!(
                "could not open browser ({e}) — open {url} manually"
            )),
        }
    }

    Ok(format_launch_report(&notes, &mode, &url, false))
}

/// Launch on a specific port (overrides DEFAULT_PORT for this process).
pub fn launch_seamless_on_port(
    open_browser: bool,
    effort: Effort,
    port: u16,
    inject: Option<&str>,
) -> Result<String> {
    let mut notes = Vec::new();
    notes.push(ensure_installed()?);
    let (mode, cfg_msg) = auto_configure_from_nur(false, effort)?;
    notes.push(cfg_msg);

    if let Some(text) = inject {
        let p = write_inject_seed(text)?;
        notes.push(format!("wrote inject seed → {}", p.display()));
    }

    let url = canvas_url(port);
    if port_is_open(port) {
        notes.push(format!("already listening on {url}"));
        if open_browser {
            let _ = crate::open_uri::open(&url);
            notes.push("opened canvas in your default browser".into());
        }
        return Ok(format_launch_report(&notes, &mode, &url, true));
    }

    let flag = match &mode {
        AutoConfigMode::Api { .. } => "--api",
        AutoConfigMode::CodexCli => "--codex",
        AutoConfigMode::ClaudeCli => "--claude",
        AutoConfigMode::KimiCli => "--kimi",
    };
    let args = vec![flag.to_string(), "--port".into(), port.to_string()];
    let _ = launch(&args)?;
    notes.push(format!("spawned penecho ({flag}) on port {port}"));
    let ready = wait_for_port(port, 12_000);
    notes.push(if ready {
        "server ready".into()
    } else {
        "server still starting — open URL shortly".into()
    });
    if open_browser {
        let _ = crate::open_uri::open(&url);
        notes.push("opened canvas in your default browser".into());
    }
    Ok(format_launch_report(&notes, &mode, &url, false))
}

/// Write conversation/context seed for the user (and agents) to paste on canvas.
pub fn write_inject_seed(text: &str) -> Result<PathBuf> {
    let dir = penecho_state_dir();
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("inject.md");
    let body = format!(
        "# Nur → PenEcho inject seed\n\n\
         Paste this into the canvas text tool or AI action menu.\n\n\
         ---\n\n{}\n",
        text.trim()
    );
    fs::write(&path, body).map_err(|e| MuseError::Other(format!("write inject seed: {e}")))?;
    Ok(path)
}

/// Stop whatever is listening on the penecho port (best-effort).
pub fn stop(port: u16) -> Result<String> {
    if !port_is_open(port) {
        return Ok(format!("penecho not listening on port {port}"));
    }
    #[cfg(windows)]
    {
        // Find PID owning the port via netstat, then taskkill.
        let out = Command::new("cmd.exe")
            .args(["/C", &format!("netstat -ano | findstr :{port}")])
            .output()
            .map_err(|e| MuseError::Other(format!("netstat: {e}")))?;
        let text = String::from_utf8_lossy(&out.stdout);
        let mut pids = std::collections::BTreeSet::new();
        for line in text.lines() {
            if !line.contains(&format!(":{port}")) {
                continue;
            }
            if let Some(pid) = line.split_whitespace().last() {
                if pid.chars().all(|c| c.is_ascii_digit()) && pid != "0" {
                    pids.insert(pid.to_string());
                }
            }
        }
        if pids.is_empty() {
            return Ok(format!(
                "port {port} looked open but no PID found — close the penecho window manually"
            ));
        }
        let mut killed = Vec::new();
        for pid in pids {
            let _ = Command::new("taskkill")
                .args(["/PID", &pid, "/F"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            killed.push(pid);
        }
        // Brief wait for port release.
        for _ in 0..20 {
            if !port_is_open(port) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        return Ok(format!(
            "stopped penecho on port {port} (killed PIDs: {})",
            killed.join(", ")
        ));
    }
    #[cfg(not(windows))]
    {
        let out = Command::new("sh")
            .args([
                "-c",
                &format!("lsof -ti tcp:{port} | xargs -r kill -TERM 2>/dev/null; sleep 0.3; lsof -ti tcp:{port} | xargs -r kill -KILL 2>/dev/null; true"),
            ])
            .output();
        let _ = out;
        Ok(format!("sent stop signals for listeners on port {port}"))
    }
}

pub fn restart(open_browser: bool, effort: Effort, port: u16) -> Result<String> {
    let stop_msg = stop(port).unwrap_or_else(|e| format!("stop: {e}"));
    let launch_msg = launch_seamless_on_port(open_browser, effort, port, None)?;
    Ok(format!("{stop_msg}\n{launch_msg}"))
}

/// Best-effort PNG capture: open canvas URL note + path for browser tool / look.
/// PenEcho PNG export is client-side; we stage a capture path under `.nur/media`.
pub fn export_png_hint(cwd: &Path) -> Result<String> {
    let media = cwd.join(".nur").join("media");
    let _ = fs::create_dir_all(&media);
    let dest = media.join(format!(
        "penecho-canvas-{}.png",
        now_secs_local()
    ));
    let url = canvas_url(DEFAULT_PORT);
    Ok(format!(
        "penecho PNG export is canvas-side (ink + 1 tile margin).\n\
         1. Ensure server is up: penecho(action=launch)\n\
         2. In the canvas: use the PNG export control\n\
         3. Or use browser(action=screenshot) on {url} → save to {}\n\
         4. Then look(path=…) to inspect\n\
         staged path: {}\n",
        dest.display(),
        dest.display()
    ))
}

fn now_secs_local() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn format_launch_report(
    notes: &[String],
    mode: &AutoConfigMode,
    url: &str,
    already: bool,
) -> String {
    let mode_s = match mode {
        AutoConfigMode::Api { model, format, .. } => {
            format!("api ({format}, model={model})")
        }
        AutoConfigMode::CodexCli => "codex-cli".into(),
        AutoConfigMode::ClaudeCli => "claude-cli".into(),
        AutoConfigMode::KimiCli => "kimi-cli".into(),
    };
    let mut s = String::new();
    s.push_str("penecho ready\n");
    s.push_str(&format!("  canvas:  {url}\n"));
    s.push_str(&format!("  mode:    {mode_s}\n"));
    s.push_str(&format!(
        "  state:   {}\n",
        if already {
            "already running"
        } else {
            "launched"
        }
    ));
    s.push_str(&format!(
        "  config:  {}\n",
        penecho_state_dir().join("config.env").display()
    ));
    for n in notes {
        s.push_str(&format!("  · {n}\n"));
    }
    s.push_str(
        "Canvas: 20k×20k ink · MathJax · plots · draft layer · animations.\n\
         Open policy: browser URL only (no local-file Open-with).\n",
    );
    s
}

/// Ensure install + config without launching (for ecosystem / status).
pub fn ensure_ready(effort: Effort) -> Result<String> {
    let install = ensure_installed()?;
    let (_, cfg) = auto_configure_from_nur(false, effort)?;
    let st = probe();
    Ok(format!(
        "{install}\n{cfg}\n binary={:?}\n config={} usable={}\n listening={}\n canvas={}\n",
        st.binary,
        st.config_file.display(),
        config_is_usable(),
        port_is_open(DEFAULT_PORT),
        canvas_url(DEFAULT_PORT)
    ))
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
            content
                .lines()
                .any(|l| l.contains("AI_API_URL=") && l.contains("http"))
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
        let cfg =
            resolve_api_config("https://api.openai.com/v1", "sk-test", "gpt-4o", None).unwrap();
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
            false,
        )
        .unwrap();
        assert!(s.contains("AI_PROVIDER=api"));
        assert!(s.contains("AI_API_KEY=sk-real123"));
        assert!(s.contains("AI_EFFORT=medium"));
    }

    /// The agent-facing path must never render the key: this string is returned
    /// to the model and persisted verbatim into `~/.nur/sessions/*.json`.
    #[test]
    fn redacted_export_never_contains_the_key() {
        let key = "sk-real-secret-do-not-leak";
        let s = export_to_penecho_env(
            "https://api.openai.com/v1",
            key,
            "gpt-4o",
            Effort::Medium,
            true,
        )
        .unwrap();
        assert!(!s.contains(key), "key leaked into export:\n{s}");
        // Both the modern and the legacy OPENAI_* line must be covered.
        assert_eq!(
            s.matches(REDACTED_KEY).count(),
            2,
            "missed a key line:\n{s}"
        );
        // Everything else still renders, so the export stays useful.
        assert!(s.contains("AI_PROVIDER=api"));
        assert!(s.contains("AI_API_MODEL=gpt-4o"));
    }
}

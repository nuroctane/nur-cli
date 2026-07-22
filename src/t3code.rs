//! t3code compatibility layer — driver probing, env isolation, atomic writes,
//! delegate mode, and pairing tokens.
//!
//! t3code (https://github.com/pingdotgg/t3code) delegates 100% of LLM auth to
//! vendor CLIs and never stores API keys. Its control plane uses pairing +
//! DPoP bearer. NurCLI traditionally stores tokens in `~/.nur/auth.json`.
//!
//! This module mirrors t3code's ideas in Rust:
//! - Env-isolated probing (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`, etc.) so we don't
//!   break macOS keychain by reading `$HOME` directly.
//! - Import-first: check vendor CLI auth before prompting.
//! - Atomic writes for `auth.json`.
//! - Delegate probe (no secret storage).
//! - Simplified pairing token generator (TTL, one-time use semantic).

use crate::error::{MuseError, Result};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// t3code's driver names that we mirror.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverId {
    Claude,
    Codex,
    Cursor,
    OpenCode,
    Grok,
}

impl DriverId {
    pub fn as_str(self) -> &'static str {
        match self {
            DriverId::Claude => "claude",
            DriverId::Codex => "codex",
            DriverId::Cursor => "cursor",
            DriverId::OpenCode => "opencode",
            DriverId::Grok => "grok",
        }
    }

    pub fn vendor_cli_hint(self) -> &'static str {
        match self {
            DriverId::Claude => "claude auth login",
            DriverId::Codex => "codex login",
            DriverId::Cursor => "cursor-agent login",
            DriverId::OpenCode => "opencode auth login",
            DriverId::Grok => "grok login (or xai auth)",
        }
    }
}

/// Resolve the config dir for a driver with t3code-style isolation.
///
/// - Claude: respects `CLAUDE_CONFIG_DIR` (not `$HOME`) to preserve macOS keychain.
/// - Codex: respects `CODEX_HOME`.
/// - Cursor/OpenCode: respect `CURSOR_AGENT_HOME` / `OPENCODE_HOME` if set (future-proof).
/// - Grok: no special dir yet, but we keep hook for `XAI_CONFIG_DIR`.
pub fn driver_config_dir(driver: DriverId) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    match driver {
        DriverId::Claude => {
            if let Ok(dir) = std::env::var("CLAUDE_CONFIG_DIR") {
                let p = PathBuf::from(dir);
                if p.is_absolute() {
                    return p;
                }
            }
            home.join(".claude")
        }
        DriverId::Codex => {
            if let Ok(dir) = std::env::var("CODEX_HOME") {
                let p = PathBuf::from(dir);
                if p.is_absolute() {
                    return p;
                }
            }
            home.join(".codex")
        }
        DriverId::Cursor => {
            if let Ok(dir) = std::env::var("CURSOR_AGENT_HOME") {
                let p = PathBuf::from(dir);
                if p.is_absolute() {
                    return p;
                }
            }
            // Cursor stores auth under ~/.cursor or ~/.config/cursor depending on version
            let dot_cursor = home.join(".cursor");
            if dot_cursor.exists() {
                dot_cursor
            } else {
                home.join(".config").join("cursor")
            }
        }
        DriverId::OpenCode => {
            if let Ok(dir) = std::env::var("OPENCODE_HOME") {
                let p = PathBuf::from(dir);
                if p.is_absolute() {
                    return p;
                }
            }
            home.join(".config").join("opencode")
        }
        DriverId::Grok => {
            if let Ok(dir) = std::env::var("XAI_CONFIG_DIR") {
                let p = PathBuf::from(dir);
                if p.is_absolute() {
                    return p;
                }
            }
            home.join(".config").join("xai")
        }
    }
}

/// Does the vendor CLI binary exist on PATH?
pub fn vendor_cli_exists(driver: DriverId) -> bool {
    let bin = match driver {
        DriverId::Claude => "claude",
        DriverId::Codex => "codex",
        DriverId::Cursor => "cursor-agent",
        DriverId::OpenCode => "opencode",
        DriverId::Grok => "grok",
    };
    which(bin).is_some()
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let c = dir.join(name);
        if c.is_file() {
            return Some(c);
        }
        #[cfg(windows)]
        {
            let exe = dir.join(format!("{name}.exe"));
            if exe.is_file() {
                return Some(exe);
            }
            let cmd = dir.join(format!("{name}.cmd"));
            if cmd.is_file() {
                return Some(cmd);
            }
        }
    }
    None
}

/// Probe vendor auth status without reading secrets — mirrors t3code's
/// `checkClaudeProviderStatus` / `probeClaudeCapabilities` idea.
///
/// Returns a snapshot: exists, config dir exists, has credentials file, binary present.
#[derive(Debug, Clone)]
pub struct ProbeStatus {
    pub driver: DriverId,
    pub binary_present: bool,
    pub config_dir: PathBuf,
    pub config_dir_exists: bool,
    pub has_credentials: bool,
    pub hint: &'static str,
}

pub fn probe_driver(driver: DriverId) -> ProbeStatus {
    let config_dir = driver_config_dir(driver);
    let config_dir_exists = config_dir.exists();
    let binary_present = vendor_cli_exists(driver);
    let has_credentials = if config_dir_exists {
        probes_have_credentials(driver, &config_dir)
    } else {
        false
    };
    ProbeStatus {
        driver,
        binary_present,
        config_dir,
        config_dir_exists,
        has_credentials,
        hint: driver.vendor_cli_hint(),
    }
}

fn probes_have_credentials(driver: DriverId, dir: &Path) -> bool {
    // Heuristic file existence checks — we never read token values here,
    // only probe presence, matching t3code's zero-secret-storage principle.
    match driver {
        DriverId::Claude => {
            dir.join(".credentials.json").exists()
                || dir.join("credentials.json").exists()
                || dir.join(".claude.json").exists()
        }
        DriverId::Codex => {
            dir.join("auth.json").exists() || dir.join("config.toml").exists()
        }
        DriverId::Cursor => {
            dir.join("auth.json").exists()
                || dir.join("mcp.json").exists()
                || dir.join("config.json").exists()
        }
        DriverId::OpenCode => {
            dir.join("auth.json").exists()
                || dir.join("config.json").exists()
                || dir.join("opencode.json").exists()
        }
        DriverId::Grok => dir.join("auth.json").exists() || dir.join("config.json").exists(),
    }
}

/// Probe all known drivers — for `nur auth status` and `/t3code` palette.
pub fn probe_all() -> Vec<ProbeStatus> {
    [
        DriverId::Claude,
        DriverId::Codex,
        DriverId::Cursor,
        DriverId::OpenCode,
        DriverId::Grok,
    ]
    .iter()
    .map(|d| probe_driver(*d))
    .collect()
}

/// Atomic write — mirrors t3code's `atomicWrite.ts` to avoid corruption on crash.
///
/// Writes to `<path>.tmp.<rand>` then renames. Uses std::fs::write + rename which
/// is atomic on most platforms when same filesystem.
#[allow(dead_code)]
pub fn atomic_write(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension(format!(
        "tmp.{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    fs::write(&tmp, contents)?;
    // On Windows rename fails if dest exists — remove first, then rename.
    #[cfg(windows)]
    {
        let _ = fs::remove_file(path);
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Delegate mode — verify vendor CLI auth exists without storing token.
///
/// Returns Ok(()) if driver reports has_credentials, Err otherwise with hint.
pub fn delegate_check(driver: DriverId) -> Result<()> {
    let st = probe_driver(driver);
    if st.has_credentials {
        Ok(())
    } else {
        Err(MuseError::Other(format!(
            "delegate check failed for {}: no credentials in {} (binary_present={}, config_exists={}). Hint: run `{}` first.",
            driver.as_str(),
            st.config_dir.display(),
            st.binary_present,
            st.config_dir_exists,
            st.hint
        )))
    }
}

/// Simplified pairing token — t3code uses `t3 auth pairing create` with TTL,
/// Ed25519 + DPoP. Here we implement a lightweight version for `nur serve`
/// remote: random url-safe token with metadata, stored in memory or file.
///
/// This is intentionally simpler than t3code's full DPoP implementation but
/// keeps the same semantics: one-time use, TTL, label, scopes.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PairingToken {
    pub token: String,
    pub label: String,
    pub scopes: Vec<String>,
    pub created_at: u64,
    pub ttl_secs: u64,
    pub one_time: bool,
}

impl PairingToken {
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now > self.created_at + self.ttl_secs
    }

    pub fn to_pairing_link(&self, base_url: &str) -> String {
        format!(
            "{}/pair#token={}&label={}",
            base_url.trim_end_matches('/'),
            urlencoding::encode(&self.token),
            urlencoding::encode(&self.label)
        )
    }
}

fn urlencoding_encode(s: &str) -> String {
    // Minimal url encoding for pairing link — encode non-alphanumeric except -_.~
    let mut out = String::new();
    for b in s.bytes() {
        let c = b as char;
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
            out.push(c);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

mod urlencoding {
    pub fn encode(s: &str) -> String {
        super::urlencoding_encode(s)
    }
}

/// Generate a pairing token with TTL parsing like t3code's `DurationFromString`
/// (supports `5m`, `1h`, `30d`, `10s`).
pub fn create_pairing_token(label: &str, ttl: &str, scopes: Vec<String>) -> PairingToken {
    let ttl_secs = parse_duration_to_secs(ttl).unwrap_or(300); // default 5m
    let token = random_urlsafe(32);
    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    PairingToken {
        token,
        label: label.to_string(),
        scopes,
        created_at,
        ttl_secs,
        one_time: true,
    }
}

fn parse_duration_to_secs(s: &str) -> Option<u64> {
    let s = s.trim().to_ascii_lowercase();
    if s.is_empty() {
        return None;
    }
    let (num_part, unit) = if s.ends_with('d') {
        (&s[..s.len() - 1], "d")
    } else if s.ends_with('h') {
        (&s[..s.len() - 1], "h")
    } else if s.ends_with('m') {
        (&s[..s.len() - 1], "m")
    } else if s.ends_with('s') {
        (&s[..s.len() - 1], "s")
    } else {
        (s.as_str(), "s")
    };
    let n: u64 = num_part.parse().ok()?;
    Some(match unit {
        "d" => n * 86400,
        "h" => n * 3600,
        "m" => n * 60,
        "s" => n,
        _ => n,
    })
}

fn random_urlsafe(nbytes: usize) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    // Not cryptographically strong, but sufficient for pairing link demo.
    // For production DPoP, use `ring` or `ed25519-dalek` like t3code's `jose`.
    let mut out = String::new();
    let mut hasher = DefaultHasher::new();
    SystemTime::now().hash(&mut hasher);
    std::thread::current().id().hash(&mut hasher);
    let mut seed = hasher.finish();
    const ALPH: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    for _ in 0..nbytes {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let idx = (seed % ALPH.len() as u64) as usize;
        out.push(ALPH[idx] as char);
    }
    out
}

/// Env isolation — per-provider env map merged like t3code's
/// `mergeProviderInstanceEnvironment()`. Returns a map of env vars that should
/// be set for this driver instance.
pub fn env_for_driver(driver: DriverId) -> HashMap<String, String> {
    let mut env = HashMap::new();
    // Preserve existing env isolation vars if user set them
    for var in &[
        "CLAUDE_CONFIG_DIR",
        "CODEX_HOME",
        "CURSOR_AGENT_HOME",
        "OPENCODE_HOME",
        "XAI_CONFIG_DIR",
    ] {
        if let Ok(v) = std::env::var(var) {
            env.insert(var.to_string(), v);
        }
    }
    // Ensure driver-specific config dir is set so child processes don't
    // accidentally use $HOME and break macOS keychain (t3code's fix)
    match driver {
        DriverId::Claude => {
            env.entry("CLAUDE_CONFIG_DIR".to_string())
                .or_insert_with(|| driver_config_dir(driver).display().to_string());
        }
        DriverId::Codex => {
            env.entry("CODEX_HOME".to_string())
                .or_insert_with(|| driver_config_dir(driver).display().to_string());
        }
        DriverId::Cursor => {
            env.entry("CURSOR_AGENT_HOME".to_string())
                .or_insert_with(|| driver_config_dir(driver).display().to_string());
        }
        DriverId::OpenCode => {
            env.entry("OPENCODE_HOME".to_string())
                .or_insert_with(|| driver_config_dir(driver).display().to_string());
        }
        DriverId::Grok => {
            env.entry("XAI_CONFIG_DIR".to_string())
                .or_insert_with(|| driver_config_dir(driver).display().to_string());
        }
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_dirs_respect_env() {
        #[cfg(windows)]
        let test_path = "C:\\tmp\\claude-test";
        #[cfg(not(windows))]
        let test_path = "/tmp/claude-test";
        std::env::set_var("CLAUDE_CONFIG_DIR", test_path);
        let p = driver_config_dir(DriverId::Claude);
        assert_eq!(p, PathBuf::from(test_path));
        std::env::remove_var("CLAUDE_CONFIG_DIR");
    }

    #[test]
    fn parse_duration() {
        assert_eq!(parse_duration_to_secs("5m"), Some(300));
        assert_eq!(parse_duration_to_secs("1h"), Some(3600));
        assert_eq!(parse_duration_to_secs("30d"), Some(2592000));
        assert_eq!(parse_duration_to_secs("10s"), Some(10));
        assert_eq!(parse_duration_to_secs(""), None);
    }

    #[test]
    fn pairing_token_expiry() {
        let tok = create_pairing_token("test", "1s", vec!["standard".into()]);
        assert!(!tok.is_expired());
        let mut expired = tok.clone();
        expired.created_at -= 10;
        assert!(expired.is_expired());
    }

    #[test]
    fn probe_all_returns_five() {
        let all = probe_all();
        assert_eq!(all.len(), 5);
    }
}

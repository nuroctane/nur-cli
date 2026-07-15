use crate::config::{auth_path, ensure_dirs};
use crate::error::{MuseError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    #[default]
    ApiKey,
    Oauth,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OauthMeta {
    #[serde(default)]
    pub issuer: String,
    #[serde(default)]
    pub client_id: String,
    /// Provider-specific extras (e.g. device flow id, azure resource).
    #[serde(default)]
    pub extra: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Auth {
    /// Current access token or API key (used as HTTP bearer).
    pub api_key: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub auth_method: AuthMethod,
    /// Catalog provider id this credential belongs to (optional for legacy files).
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// Unix seconds when `api_key` (access token) expires. `None` = no expiry.
    #[serde(default)]
    pub expires_at: Option<u64>,
    #[serde(default)]
    pub oauth_meta: Option<OauthMeta>,
}

impl Default for Auth {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            source: String::new(),
            auth_method: AuthMethod::ApiKey,
            provider: String::new(),
            refresh_token: None,
            expires_at: None,
            oauth_meta: None,
        }
    }
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Human-relative expiry: `in 42m`, `expired 3m ago`, `no expiry`.
pub fn format_expires_relative(expires_at: Option<u64>) -> String {
    format_expires_relative_at(expires_at, now_unix())
}

/// Testable variant of [`format_expires_relative`].
pub fn format_expires_relative_at(expires_at: Option<u64>, now: u64) -> String {
    let Some(exp) = expires_at else {
        return "no expiry".into();
    };
    if exp > now {
        let secs = exp - now;
        format!("in {}", format_duration_short(secs))
    } else {
        let secs = now - exp;
        format!("expired {} ago", format_duration_short(secs))
    }
}

fn format_duration_short(secs: u64) -> String {
    if secs < 60 {
        return format!("{secs}s");
    }
    let mins = secs / 60;
    if mins < 60 {
        return format!("{mins}m");
    }
    let hours = mins / 60;
    if hours < 48 {
        let rem_m = mins % 60;
        if rem_m == 0 {
            format!("{hours}h")
        } else {
            format!("{hours}h{rem_m}m")
        }
    } else {
        let days = hours / 24;
        format!("{days}d")
    }
}

/// True when saved credentials must not be used for `cfg_provider`.
/// Empty `auth.provider` (legacy files) is treated as compatible with any provider.
pub fn provider_mismatch(auth: &Auth, cfg_provider: &str) -> bool {
    !auth.provider.is_empty() && auth.provider != cfg_provider
}

/// Resolve a usable bearer credential (any provider / env).
/// Order: `NUR_API_KEY` → vendor/legacy envs → `~/.nur/auth.json` → legacy homes.
pub fn resolve_api_key() -> Result<String> {
    resolve_api_key_for(None)
}

/// Resolve credentials for a catalog provider. Refuses to reuse a key tagged for
/// a different provider (prevents sending Grok tokens to OpenAI, etc.).
/// Env keys always win and are not provider-scoped.
pub fn resolve_api_key_for(expected_provider: Option<&str>) -> Result<String> {
    // NUR_API_KEY = app generic; META_API_KEY kept for Meta Model API / old installs.
    for var in ["NUR_API_KEY", "META_API_KEY", "MODEL_API_KEY", "MUSE_API_KEY"] {
        if let Ok(k) = std::env::var(var) {
            let k = k.trim().to_string();
            if !k.is_empty() {
                return Ok(k);
            }
        }
    }
    if let Some(auth) = load_auth()? {
        let mut auth = auth;
        if let Some(exp) = expected_provider {
            if provider_mismatch(&auth, exp) {
                return Err(MuseError::Other(format!(
                    "saved credentials are for provider '{}' but active provider is '{}'. Run /login (or nur auth logout) and sign in again.",
                    auth.provider, exp
                )));
            }
        }
        ensure_fresh_oauth(&mut auth)?;
        let k = auth.api_key.trim().to_string();
        if !k.is_empty() {
            return Ok(k);
        }
    }
    // Legacy path if migration hasn't run yet — promote into ~/.nur for next time.
    for legacy_home in [
        crate::config::legacy_meta_home(),
        crate::config::legacy_muse_home(),
    ] {
        let legacy = legacy_home.join("auth.json");
        if !legacy.exists() {
            continue;
        }
        let text = fs::read_to_string(&legacy)?;
        let auth: Auth = serde_json::from_str(&text)?;
        let k = auth.api_key.trim().to_string();
        if !k.is_empty() {
            if let Some(exp) = expected_provider {
                if provider_mismatch(&auth, exp) {
                    return Err(MuseError::Other(format!(
                        "legacy credentials are for provider '{}' but active provider is '{}'. Run /login.",
                        auth.provider, exp
                    )));
                }
            }
            let _ = crate::config::promote_legacy_file("auth.json");
            if !auth_path().exists() {
                let _ = ensure_dirs();
                let _ = save_api_key_for(&k, expected_provider);
            }
            return Ok(k);
        }
    }
    Err(MuseError::NotAuthenticated)
}

pub fn load_auth() -> Result<Option<Auth>> {
    let path = auth_path();
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path)?;
    let auth: Auth = serde_json::from_str(&text)?;
    Ok(Some(auth))
}

/// Refresh OAuth access token if within 5 minutes of expiry (or already expired).
pub fn ensure_fresh_oauth(auth: &mut Auth) -> Result<()> {
    if !matches!(auth.auth_method, AuthMethod::Oauth) {
        return Ok(());
    }
    let Some(exp) = auth.expires_at else {
        return Ok(());
    };
    let now = now_unix();
    // Refresh when < 5 min remaining.
    if exp > now.saturating_add(300) {
        return Ok(());
    }
    let Some(refresh) = auth.refresh_token.clone().filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    let provider = auth.provider.as_str();
    match crate::oauth::refresh_tokens(provider, auth, &refresh) {
        Ok(tokens) => {
            auth.api_key = tokens.access_token;
            if let Some(r) = tokens.refresh_token {
                auth.refresh_token = Some(r);
            }
            auth.expires_at = tokens.expires_at;
            auth.source = "oauth".into();
            let _ = save_auth(auth);
            Ok(())
        }
        Err(e) => {
            // Soft-fail if still not expired — let the request try.
            if exp > now {
                Ok(())
            } else {
                Err(MuseError::Other(format!(
                    "OAuth token expired and refresh failed ({e}). Run /login again."
                )))
            }
        }
    }
}

pub fn save_auth(auth: &Auth) -> Result<()> {
    ensure_dirs()?;
    let text = serde_json::to_string_pretty(auth)?;
    let path = auth_path();
    crate::config::atomic_write(&path, text.as_bytes())
        .map_err(|e| MuseError::Other(format!("failed to save auth atomically: {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

pub fn save_api_key(key: &str) -> Result<()> {
    save_api_key_for(key, None)
}

/// Save an API key, optionally tagging it with the catalog provider id.
pub fn save_api_key_for(key: &str, provider: Option<&str>) -> Result<()> {
    let trimmed = key.trim();
    if trimmed.len() < 8 {
        return Err(MuseError::Other(
            "API key too short — expected at least 8 characters".into(),
        ));
    }
    if trimmed.contains(' ') || trimmed.contains('\n') {
        return Err(MuseError::Other("API key contains whitespace".into()));
    }
    let mut auth = Auth {
        api_key: trimmed.to_string(),
        source: "login".to_string(),
        auth_method: AuthMethod::ApiKey,
        provider: provider.unwrap_or("").to_string(),
        refresh_token: None,
        expires_at: None,
        oauth_meta: None,
    };
    // Preserve provider if caller omitted but we already had one for the same key path? No —
    // clean api-key login should set provider explicitly from TUI.
    if auth.provider.is_empty() {
        if let Ok(Some(prev)) = load_auth() {
            // Only keep prior provider when re-saving without an explicit tag and method was key.
            if matches!(prev.auth_method, AuthMethod::ApiKey) && !prev.provider.is_empty() {
                auth.provider = prev.provider;
            }
        }
    }
    save_auth(&auth)
}

/// Persist an OAuth session (access + optional refresh).
pub fn save_oauth_session(
    provider: &str,
    access_token: &str,
    refresh_token: Option<String>,
    expires_at: Option<u64>,
    meta: Option<OauthMeta>,
) -> Result<()> {
    let access = access_token.trim();
    if access.is_empty() {
        return Err(MuseError::Other("empty OAuth access token".into()));
    }
    let auth = Auth {
        api_key: access.to_string(),
        source: "oauth".into(),
        auth_method: AuthMethod::Oauth,
        provider: provider.to_string(),
        refresh_token,
        expires_at,
        oauth_meta: meta,
    };
    save_auth(&auth)
}

/// Delete local credentials. If `revoke` is true, best-effort remote revoke first.
pub fn logout(revoke: bool) -> Result<()> {
    if revoke {
        if let Ok(Some(auth)) = load_auth() {
            match crate::oauth::revoke_session(&auth) {
                Ok(msg) => {
                    if !msg.is_empty() {
                        eprintln!("{msg}");
                    }
                }
                Err(e) => {
                    eprintln!("revoke note: {e} (continuing with local logout)");
                }
            }
        }
    }
    let path = auth_path();
    if path.exists() {
        fs::remove_file(&path)?;
    }
    let legacy = crate::config::legacy_muse_home().join("auth.json");
    if legacy.exists() {
        let _ = fs::remove_file(legacy);
    }
    Ok(())
}

pub fn key_fingerprint(key: &str) -> String {
    let k = key.trim();
    if k.len() <= 8 {
        return "****".to_string();
    }
    format!("{}…{}", &k[..4], &k[k.len() - 4..])
}

pub fn auth_status() -> Result<()> {
    // Status should report mismatch without hard-failing the command.
    let env_source = if std::env::var("NUR_API_KEY").map(|k| !k.trim().is_empty()).unwrap_or(false)
    {
        Some("NUR_API_KEY env")
    } else if std::env::var("META_API_KEY")
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false)
    {
        Some("META_API_KEY env (Meta provider / legacy app)")
    } else if std::env::var("MODEL_API_KEY")
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false)
    {
        Some("MODEL_API_KEY env")
    } else if std::env::var("MUSE_API_KEY")
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false)
    {
        Some("MUSE_API_KEY env (legacy)")
    } else {
        None
    };

    if let Some(src) = env_source {
        let key = resolve_api_key()?;
        println!("authenticated: yes");
        println!("source: {src}");
        println!("method: api_key (env)");
        println!("provider: (env — not scoped)");
        println!("expires: no expiry");
        println!("key: {}", key_fingerprint(&key));
        println!("note: env keys override ~/.nur/auth.json");
        return Ok(());
    }

    match load_auth()? {
        Some(mut auth) if !auth.api_key.trim().is_empty() => {
            let _ = ensure_fresh_oauth(&mut auth);
            let cfg_provider = crate::config::load_config()
                .map(|c| c.provider)
                .unwrap_or_default();
            println!("authenticated: yes");
            println!("source: ~/.nur/auth.json");
            if !auth.provider.is_empty() {
                println!("provider: {}", auth.provider);
            } else {
                println!("provider: (unset — legacy file)");
            }
            if !cfg_provider.is_empty() && provider_mismatch(&auth, &cfg_provider) {
                println!(
                    "config_provider: {cfg_provider}  ⚠ mismatch — run /login before chatting"
                );
            } else if !cfg_provider.is_empty() {
                println!("config_provider: {cfg_provider}");
            }
            println!(
                "method: {}",
                match auth.auth_method {
                    AuthMethod::ApiKey => "api_key",
                    AuthMethod::Oauth => "oauth / browser",
                }
            );
            println!("expires: {}", format_expires_relative(auth.expires_at));
            println!("key: {}", key_fingerprint(&auth.api_key));
            println!(
                "note: ~/.nur/auth.json is plaintext secrets (Unix 0600; Windows profile ACLs)"
            );
            Ok(())
        }
        _ => {
            for (label, home) in [
                ("~/.meta", crate::config::legacy_meta_home()),
                ("~/.muse", crate::config::legacy_muse_home()),
            ] {
                if home.join("auth.json").exists() {
                    println!("authenticated: yes (legacy {label} — will promote on use)");
                    println!("source: {label}/auth.json");
                    return Ok(());
                }
            }
            println!("authenticated: no");
            println!("run: nur auth login");
            println!("or set NUR_API_KEY (or a vendor key env for your provider)");
            println!("or /login in the TUI (browser sign-in for Grok, Claude, …)");
            Ok(())
        }
    }
}

pub fn login_interactive(key_arg: Option<String>) -> Result<()> {
    let key = if let Some(k) = key_arg {
        k
    } else {
        print!("API key: ");
        io::stdout().flush()?;
        match rpassword::read_password() {
            Ok(k) if !k.trim().is_empty() => k,
            _ => {
                let mut line = String::new();
                io::stdin().read_line(&mut line)?;
                line
            }
        }
    };
    let key = key.trim();
    if key.is_empty() {
        return Err(MuseError::Other("empty API key".into()));
    }
    save_api_key_for(key, Some("meta"))?;
    println!("saved to {}", auth_path().display());
    println!("key: {}", key_fingerprint(key));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_auth_json_deserializes() {
        let j = r#"{"api_key":"sk-test-key-abcdefghijklmnop","source":"login"}"#;
        let a: Auth = serde_json::from_str(j).unwrap();
        assert_eq!(a.api_key, "sk-test-key-abcdefghijklmnop");
        assert!(matches!(a.auth_method, AuthMethod::ApiKey));
        assert!(a.refresh_token.is_none());
    }

    #[test]
    fn oauth_auth_roundtrip_shape() {
        let a = Auth {
            api_key: "access-token-value".into(),
            source: "oauth".into(),
            auth_method: AuthMethod::Oauth,
            provider: "xai".into(),
            refresh_token: Some("refresh".into()),
            expires_at: Some(1_700_000_000),
            oauth_meta: Some(OauthMeta {
                issuer: "https://auth.x.ai".into(),
                client_id: "cid".into(),
                extra: serde_json::json!({}),
            }),
        };
        let s = serde_json::to_string(&a).unwrap();
        let b: Auth = serde_json::from_str(&s).unwrap();
        assert_eq!(b.provider, "xai");
        assert!(matches!(b.auth_method, AuthMethod::Oauth));
        assert_eq!(b.refresh_token.as_deref(), Some("refresh"));
    }

    #[test]
    fn expires_relative_future_and_past() {
        let now = 1_000_000u64;
        assert_eq!(
            format_expires_relative_at(Some(now + 120), now),
            "in 2m"
        );
        assert_eq!(
            format_expires_relative_at(Some(now - 90), now),
            "expired 1m ago"
        );
        assert_eq!(format_expires_relative_at(None, now), "no expiry");
        assert_eq!(
            format_expires_relative_at(Some(now + 3700), now),
            "in 1h1m"
        );
    }

    #[test]
    fn provider_mismatch_rules() {
        let mut a = Auth::default();
        a.provider = String::new();
        assert!(!provider_mismatch(&a, "xai"));
        a.provider = "xai".into();
        assert!(!provider_mismatch(&a, "xai"));
        assert!(provider_mismatch(&a, "openai"));
    }
}

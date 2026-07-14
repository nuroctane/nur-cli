use crate::config::{auth_path, ensure_dirs};
use crate::error::{MuseError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Resolve a usable bearer credential.
/// Order: env keys → `~/.meta/auth.json` (with OAuth refresh if needed) → legacy `~/.muse`.
pub fn resolve_api_key() -> Result<String> {
    for var in ["META_API_KEY", "MODEL_API_KEY", "MUSE_API_KEY"] {
        if let Ok(k) = std::env::var(var) {
            let k = k.trim().to_string();
            if !k.is_empty() {
                return Ok(k);
            }
        }
    }
    if let Some(auth) = load_auth()? {
        let mut auth = auth;
        ensure_fresh_oauth(&mut auth)?;
        let k = auth.api_key.trim().to_string();
        if !k.is_empty() {
            return Ok(k);
        }
    }
    // Legacy path if migration hasn't run yet — promote into ~/.meta for next time.
    let legacy = crate::config::legacy_muse_home().join("auth.json");
    if legacy.exists() {
        let text = fs::read_to_string(&legacy)?;
        let auth: Auth = serde_json::from_str(&text)?;
        let k = auth.api_key.trim().to_string();
        if !k.is_empty() {
            let _ = crate::config::promote_legacy_file("auth.json");
            if !auth_path().exists() {
                let _ = ensure_dirs();
                let _ = save_api_key(&k);
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
    let trimmed = key.trim();
    if trimmed.len() < 8 {
        return Err(MuseError::Other(
            "API key too short — expected at least 8 characters".into(),
        ));
    }
    if trimmed.contains(' ') || trimmed.contains('\n') {
        return Err(MuseError::Other("API key contains whitespace".into()));
    }
    let mut auth = load_auth()?.unwrap_or_default();
    auth.api_key = trimmed.to_string();
    auth.source = "login".to_string();
    auth.auth_method = AuthMethod::ApiKey;
    auth.refresh_token = None;
    auth.expires_at = None;
    auth.oauth_meta = None;
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

pub fn logout() -> Result<()> {
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
    match resolve_api_key() {
        Ok(key) => {
            let source = if std::env::var("META_API_KEY").is_ok() {
                "META_API_KEY env"
            } else if std::env::var("MODEL_API_KEY").is_ok() {
                "MODEL_API_KEY env"
            } else if std::env::var("MUSE_API_KEY").is_ok() {
                "MUSE_API_KEY env (legacy)"
            } else if auth_path().exists() {
                "~/.meta/auth.json"
            } else if crate::config::legacy_muse_home().join("auth.json").exists() {
                "~/.muse/auth.json (legacy — will promote on next save)"
            } else {
                "resolved"
            };
            println!("authenticated: yes");
            println!("source: {source}");
            if let Ok(Some(a)) = load_auth() {
                if !a.provider.is_empty() {
                    println!("provider: {}", a.provider);
                }
                println!(
                    "method: {}",
                    match a.auth_method {
                        AuthMethod::ApiKey => "api_key",
                        AuthMethod::Oauth => "oauth / browser",
                    }
                );
            }
            println!("key: {}", key_fingerprint(&key));
            Ok(())
        }
        Err(MuseError::NotAuthenticated) => {
            println!("authenticated: no");
            println!("run: meta auth login");
            println!("or set META_API_KEY / MODEL_API_KEY");
            println!("or /login in the TUI (browser sign-in for Grok, Claude, …)");
            Ok(())
        }
        Err(e) => Err(e),
    }
}

pub fn login_interactive(key_arg: Option<String>) -> Result<()> {
    let key = if let Some(k) = key_arg {
        k
    } else {
        print!("Meta Model API key: ");
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
    save_api_key(key)?;
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
}

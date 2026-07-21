use crate::config::{auth_path, ensure_dirs};
use crate::error::{MuseError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

static OAUTH_STORE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn oauth_store_guard() -> MutexGuard<'static, ()> {
    OAUTH_STORE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    #[default]
    ApiKey,
    Oauth,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct OauthMeta {
    #[serde(default)]
    pub issuer: String,
    #[serde(default)]
    pub client_id: String,
    /// Provider-specific extras (e.g. device flow id, azure resource).
    #[serde(default)]
    pub extra: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

/// Non-secret OAuth attributes needed to route and authorize provider requests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OAuthRequestContext {
    /// ChatGPT workspace/account header required by OpenAI's OAuth backend.
    pub account_id: Option<String>,
    /// Whether OpenAI must route this account through its FedRAMP edge.
    pub is_fedramp: bool,
    /// Google Cloud quota project required by Gemini OAuth requests.
    pub project_id: Option<String>,
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

/// Pure pick order for a *specific* catalog provider. Used by
/// [`resolve_api_key_for`] and unit-tested so META_/MODEL_ leftovers cannot
/// outrank provider login for *any* host (xai, openai, anthropic, …).
///
/// Inputs are already trimmed; empty string is treated as absent.
pub(crate) fn pick_provider_credential(
    provider_env: Option<&str>,
    matching_auth: Option<&str>,
    failover_key: Option<&str>,
    failover_oauth: Option<&str>,
    nur_global: Option<&str>,
    legacy_auth: Option<&str>,
    // Intentionally ignored for provider-scoped resolve (Meta-era aliases).
    _meta_model_muse_generic: Option<&str>,
) -> Option<String> {
    for cand in [
        provider_env,
        matching_auth,
        failover_key,
        failover_oauth,
        nur_global,
        legacy_auth,
    ] {
        if let Some(k) = cand.map(str::trim).filter(|s| !s.is_empty()) {
            return Some(k.to_string());
        }
    }
    None
}

/// Resolve credentials for a catalog provider.
///
/// **With `Some(provider_id)`** (client init, `/model`, etc.) — provider-scoped
/// first so a leftover `MODEL_API_KEY` / `META_API_KEY` never gets sent to xAI,
/// Anthropic, OpenAI, … after you `/login` that provider:
/// 1. matching active OAuth session (refreshed), so env cannot replace it after restart
/// 2. catalog env (`XAI_API_KEY`, `OPENAI_API_KEY`, …)
/// 3. matching `auth.json` API key
/// 4. per-provider failover key / OAuth stores
/// 5. `NUR_API_KEY` only as a true global override (not META_/MODEL_/MUSE_)
///
/// **With `None`** — generic envs then `auth.json` (scripts / headless).
pub fn resolve_api_key_for(expected_provider: Option<&str>) -> Result<String> {
    if let Some(exp) = expected_provider {
        let provider_env = crate::providers::by_id(exp).and_then(|p| {
            std::env::var(p.env_key)
                .ok()
                .map(|k| k.trim().to_string())
                .filter(|k| !k.is_empty())
        });
        let mut matching_auth = None;
        let mut matching_oauth = None;
        let mut legacy_auth = None;
        let mut mismatched = false;
        if let Some(auth) = load_auth()? {
            if provider_mismatch(&auth, exp) {
                mismatched = true;
            } else {
                if matches!(auth.auth_method, AuthMethod::Oauth) {
                    matching_oauth = resolve_oauth_access_token(exp)?;
                } else {
                    let k = auth.api_key.trim().to_string();
                    if !k.is_empty() && auth.provider.is_empty() {
                        // Legacy providerless keys are compatible fallbacks,
                        // but must never outrank a provider-bound key or OAuth
                        // session selected explicitly for this provider.
                        legacy_auth = Some(k);
                    } else if !k.is_empty() {
                        matching_auth = Some(k);
                    }
                }
            }
        }
        // An explicit browser sign-in is the active login choice. Do not let a
        // stale vendor env key silently replace it after restart.
        if let Some(k) = matching_oauth {
            return Ok(k);
        }
        let failover_key = load_provider_key(exp);
        let failover_oauth = load_provider_oauth_token(exp);
        let nur_global = std::env::var("NUR_API_KEY")
            .ok()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty());
        // Read Meta-era generics only to prove we ignore them (not passed as winners).
        let meta_era = ["META_API_KEY", "MODEL_API_KEY", "MUSE_API_KEY"]
            .iter()
            .find_map(|v| {
                std::env::var(v)
                    .ok()
                    .map(|k| k.trim().to_string())
                    .filter(|k| !k.is_empty())
            });

        if let Some(k) = pick_provider_credential(
            provider_env.as_deref(),
            matching_auth.as_deref(),
            failover_key.as_deref(),
            failover_oauth.as_deref(),
            nur_global.as_deref(),
            legacy_auth.as_deref(),
            meta_era.as_deref(),
        ) {
            return Ok(k);
        }
        if mismatched {
            if let Ok(Some(auth)) = load_auth() {
                return Err(MuseError::Other(format!(
                    "saved credentials are for provider '{}' but active provider is '{}'. Run /login (or nur auth logout) and sign in again.",
                    auth.provider, exp
                )));
            }
        }
        return Err(MuseError::NotAuthenticated);
    }

    // No expected provider: generic env first (scripts / headless), then auth.json.
    for var in [
        "NUR_API_KEY",
        "META_API_KEY",
        "MODEL_API_KEY",
        "MUSE_API_KEY",
    ] {
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
    let mut auth: Auth = serde_json::from_str(&text)?;
    if auth.provider == "antigravity" {
        auth.provider = "google".into();
    }
    // A stored OAuth login for a provider that no longer has a login flow is a
    // leftover from an older build; treat it as signed out so `/login` asks for
    // an API key instead of sending a token the vendor will reject.
    if matches!(auth.auth_method, AuthMethod::Oauth) && !oauth_session_supported(&auth.provider) {
        return Ok(None);
    }
    Ok(Some(auth))
}

/// Return OAuth request metadata when `access_token` belongs to a stored OAuth
/// session for `provider_id`. API keys deliberately return `None`.
pub fn oauth_request_context(provider_id: &str, access_token: &str) -> Option<OAuthRequestContext> {
    let matches_session = |auth: &Auth| {
        matches!(auth.auth_method, AuthMethod::Oauth)
            && auth.provider == provider_id
            && auth.api_key.trim() == access_token.trim()
    };
    let active = load_auth().ok().flatten().filter(&matches_session);
    let stored = read_sessions_at(&crate::config::provider_sessions_path())
        .remove(provider_id)
        .filter(&matches_session);
    let auth = active.or(stored)?;
    let account_id = auth
        .oauth_meta
        .as_ref()
        .and_then(|meta| meta.extra.get("account_id"))
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let is_fedramp = auth
        .oauth_meta
        .as_ref()
        .and_then(|meta| meta.extra.get("is_fedramp"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let project_id = auth
        .oauth_meta
        .as_ref()
        .and_then(|meta| meta.extra.get("project_id"))
        .and_then(|value| value.as_str())
        .map(str::to_string);
    Some(OAuthRequestContext {
        account_id,
        is_fedramp,
        project_id,
    })
}

/// Refresh OAuth access token if within 5 minutes of expiry (or already expired).
/// Mutates `auth` in place; does **not** persist — callers write to the right
/// store (`auth.json` vs per-provider sessions).
pub fn refresh_oauth_in_place(auth: &mut Auth) -> Result<bool> {
    if !matches!(auth.auth_method, AuthMethod::Oauth) {
        return Ok(false);
    }
    let Some(exp) = auth.expires_at else {
        return Ok(false);
    };
    let now = now_unix();
    // Refresh when < 5 min remaining.
    if exp > now.saturating_add(300) {
        return Ok(false);
    }
    let Some(refresh) = auth.refresh_token.clone().filter(|s| !s.is_empty()) else {
        return Ok(false);
    };
    match refresh_oauth_with_token(auth, &refresh) {
        Ok(refreshed) => Ok(refreshed),
        Err(_) if exp > now => Ok(false),
        Err(error) => Err(MuseError::Other(format!(
            "OAuth token expired and refresh failed ({error}). Run /login again."
        ))),
    }
}

fn refresh_oauth_with_token(auth: &mut Auth, refresh: &str) -> Result<bool> {
    let provider = auth.provider.as_str();
    // Provider adapters use reqwest's blocking client. Always isolate refresh
    // on a plain worker thread so callers are safe both inside and outside a
    // Tokio runtime (headless startup, streaming retries, TUI model refresh).
    let tokens = std::thread::scope(|scope| {
        scope
            .spawn(|| crate::oauth::refresh_tokens(provider, auth, refresh))
            .join()
    })
    .map_err(|_| MuseError::Other("OAuth refresh worker panicked".into()))??;
    auth.api_key = tokens.access_token;
    if let Some(r) = tokens.refresh_token {
        auth.refresh_token = Some(r);
    }
    auth.expires_at = tokens.expires_at;
    if let Some(meta) = tokens.meta {
        auth.oauth_meta = Some(meta);
    }
    auth.source = "oauth".into();
    Ok(true)
}

/// Refresh OAuth access token if needed and keep the active and provider stores
/// synchronized. The active login is canonical when both contain this provider.
pub fn ensure_fresh_oauth(auth: &mut Auth) -> Result<()> {
    if refresh_oauth_in_place(auth)? {
        save_auth(auth)?;
    }
    if matches!(auth.auth_method, AuthMethod::Oauth) && !auth.provider.trim().is_empty() {
        save_provider_session(auth)?;
    }
    Ok(())
}

/// Resolve the current access token for an OAuth-backed client without allowing
/// environment API keys to change that client's routing or wire protocol.
pub fn resolve_oauth_access_token(provider_id: &str) -> Result<Option<String>> {
    let _guard = oauth_store_guard();
    if let Some(mut auth) = load_auth()? {
        if matches!(auth.auth_method, AuthMethod::Oauth) && auth.provider == provider_id {
            ensure_fresh_oauth(&mut auth)?;
            return Ok(non_empty_access_token(&auth));
        }
    }

    let path = crate::config::provider_sessions_path();
    let mut map = read_sessions_at(&path);
    let Some(mut auth) = map.get(provider_id).cloned() else {
        return Ok(None);
    };
    if !matches!(auth.auth_method, AuthMethod::Oauth) {
        return Ok(None);
    }
    if auth.provider.is_empty() {
        auth.provider = provider_id.to_string();
    }
    if refresh_oauth_in_place(&mut auth)? {
        map.insert(provider_id.to_string(), auth.clone());
        write_sessions_at(&path, &map)?;
    }
    Ok(non_empty_access_token(&auth))
}

/// Force one OAuth refresh after a provider rejects an otherwise unexpired
/// access token. Returns `false` when the session has no refresh capability.
pub fn force_refresh_oauth(provider_id: &str) -> Result<bool> {
    let _guard = oauth_store_guard();
    if let Some(mut auth) = load_auth()? {
        if matches!(auth.auth_method, AuthMethod::Oauth) && auth.provider == provider_id {
            let Some(refresh) = auth
                .refresh_token
                .clone()
                .filter(|value| !value.trim().is_empty())
            else {
                return Ok(false);
            };
            refresh_oauth_with_token(&mut auth, &refresh)?;
            save_auth(&auth)?;
            save_provider_session(&auth)?;
            return Ok(true);
        }
    }

    let path = crate::config::provider_sessions_path();
    let mut map = read_sessions_at(&path);
    let Some(mut auth) = map.get(provider_id).cloned() else {
        return Ok(false);
    };
    if !matches!(auth.auth_method, AuthMethod::Oauth) {
        return Ok(false);
    }
    let Some(refresh) = auth
        .refresh_token
        .clone()
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(false);
    };
    if auth.provider.is_empty() {
        auth.provider = provider_id.to_string();
    }
    refresh_oauth_with_token(&mut auth, &refresh)?;
    map.insert(provider_id.to_string(), auth);
    write_sessions_at(&path, &map)?;
    Ok(true)
}

fn non_empty_access_token(auth: &Auth) -> Option<String> {
    let token = auth.api_key.trim();
    (!token.is_empty()).then(|| token.to_string())
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

// ── Per-provider key store (for cross-provider failover) ─────────────────────
// A JSON map `{provider_id: key}` at `provider_keys_path()`, separate from the
// single active `auth.json`. Lets the provider picker stash a key per fallback
// provider so `failover::resolve_target_key` can find it without env vars.

fn read_keys_at(path: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_key_at(path: &Path, provider_id: &str, key: &str) -> Result<()> {
    let trimmed = key.trim();
    if trimmed.len() < 8 {
        return Err(MuseError::Other(
            "API key too short — expected at least 8 characters".into(),
        ));
    }
    if trimmed.contains(' ') || trimmed.contains('\n') {
        return Err(MuseError::Other("API key contains whitespace".into()));
    }
    let mut map = read_keys_at(path);
    map.insert(provider_id.to_string(), trimmed.to_string());
    let text = serde_json::to_string_pretty(&map)?;
    crate::config::atomic_write(path, text.as_bytes())
        .map_err(|e| MuseError::Other(format!("failed to save provider keys: {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// A stored per-provider failover key, if one was saved for this provider id.
pub fn load_provider_key(provider_id: &str) -> Option<String> {
    read_keys_at(&crate::config::provider_keys_path())
        .get(provider_id)
        .cloned()
        .filter(|k| !k.trim().is_empty())
}

/// Save a per-provider failover key (validated like a normal API key).
pub fn save_provider_key(provider_id: &str, key: &str) -> Result<()> {
    ensure_dirs()?;
    save_key_at(&crate::config::provider_keys_path(), provider_id, key)
}

/// Persist an OAuth session as the **active** login (`auth.json`), and also
/// stash it in the per-provider session store so the same provider can later
/// be used as a failover target without re-signing-in.
pub fn save_oauth_session(
    provider: &str,
    access_token: &str,
    refresh_token: Option<String>,
    expires_at: Option<u64>,
    meta: Option<OauthMeta>,
) -> Result<()> {
    let mut auth = oauth_auth(provider, access_token, refresh_token, expires_at, meta)?;
    // Imported CLI sessions can already be near expiry. Canonicalize before
    // either store is written so a newly created client never receives a token
    // that this refresh immediately revokes.
    refresh_oauth_in_place(&mut auth)?;
    save_auth(&auth)?;
    save_provider_session(&auth)?;
    Ok(())
}

fn oauth_auth(
    provider: &str,
    access_token: &str,
    refresh_token: Option<String>,
    expires_at: Option<u64>,
    meta: Option<OauthMeta>,
) -> Result<Auth> {
    let access = access_token.trim();
    if access.is_empty() {
        return Err(MuseError::Other("empty OAuth access token".into()));
    }
    Ok(Auth {
        api_key: access.to_string(),
        source: "oauth".into(),
        auth_method: AuthMethod::Oauth,
        provider: provider.to_string(),
        refresh_token,
        expires_at,
        oauth_meta: meta,
    })
}

// ── Per-provider OAuth sessions (failover for browser-auth providers) ────────

/// Whether an OAuth session is meaningful for this provider at all.
///
/// Only providers the catalog marks `browser_auth` have a login/refresh flow.
/// Sessions for anything else are leftovers and would fail at request time.
pub fn oauth_session_supported(provider_id: &str) -> bool {
    crate::providers::by_id(provider_id)
        .map(|p| p.browser_auth)
        .unwrap_or(false)
}

fn read_sessions_at(path: &Path) -> BTreeMap<String, Auth> {
    let mut map: BTreeMap<String, Auth> = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    if let Some(mut legacy) = map.remove("antigravity") {
        legacy.provider = "google".into();
        map.entry("google".into()).or_insert(legacy);
    }
    map.retain(|id, auth| {
        !matches!(auth.auth_method, AuthMethod::Oauth) || oauth_session_supported(id)
    });
    map
}

fn write_sessions_at(path: &Path, map: &BTreeMap<String, Auth>) -> Result<()> {
    let text = serde_json::to_string_pretty(map)?;
    crate::config::atomic_write(path, text.as_bytes())
        .map_err(|e| MuseError::Other(format!("failed to save provider sessions: {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn save_provider_session_at(path: &Path, auth: &Auth) -> Result<()> {
    let id = auth.provider.trim();
    if id.is_empty() {
        return Err(MuseError::Other(
            "provider session needs a non-empty provider id".into(),
        ));
    }
    let mut map = read_sessions_at(path);
    if map.get(id) == Some(auth) {
        return Ok(());
    }
    map.insert(id.to_string(), auth.clone());
    write_sessions_at(path, &map)
}

/// Persist an OAuth session for a provider **without** changing the active
/// `auth.json` — used when capturing a failover credential via `/failover`.
pub fn save_provider_oauth(
    provider: &str,
    access_token: &str,
    refresh_token: Option<String>,
    expires_at: Option<u64>,
    meta: Option<OauthMeta>,
) -> Result<()> {
    ensure_dirs()?;
    let mut auth = oauth_auth(provider, access_token, refresh_token, expires_at, meta)?;
    refresh_oauth_in_place(&mut auth)?;
    save_provider_session(&auth)
}

fn save_provider_session(auth: &Auth) -> Result<()> {
    ensure_dirs()?;
    save_provider_session_at(&crate::config::provider_sessions_path(), auth)
}

/// Load a usable bearer for a failover provider from the per-provider OAuth
/// store (refreshing if needed). `None` if no session or refresh failed hard.
pub fn load_provider_oauth_token(provider_id: &str) -> Option<String> {
    resolve_oauth_access_token(provider_id).ok().flatten()
}

#[cfg(test)]
fn load_provider_oauth_token_at(path: &Path, provider_id: &str) -> Option<String> {
    let mut map = read_sessions_at(path);
    let mut auth = map.get(provider_id)?.clone();
    if !matches!(auth.auth_method, AuthMethod::Oauth) {
        return None;
    }
    // Keep provider id consistent even if an older file omitted it.
    if auth.provider.is_empty() {
        auth.provider = provider_id.to_string();
    }
    match refresh_oauth_in_place(&mut auth) {
        Ok(true) => {
            map.insert(provider_id.to_string(), auth.clone());
            let _ = write_sessions_at(path, &map);
        }
        Ok(false) => {}
        Err(_) => return None,
    }
    let k = auth.api_key.trim().to_string();
    if k.is_empty() {
        None
    } else {
        Some(k)
    }
}

/// Whether a stored OAuth session exists for this provider (may still need refresh).
/// Used by failover UI / doctor when deciding if browser auth is already on file.
#[allow(dead_code)] // public API for plugins/TUI; load path uses load_provider_oauth_token
pub fn has_provider_oauth(provider_id: &str) -> bool {
    read_sessions_at(&crate::config::provider_sessions_path())
        .get(provider_id)
        .map(|a| matches!(a.auth_method, AuthMethod::Oauth) && !a.api_key.trim().is_empty())
        .unwrap_or(false)
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
    let env_source = if std::env::var("NUR_API_KEY")
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false)
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
        assert_eq!(format_expires_relative_at(Some(now + 120), now), "in 2m");
        assert_eq!(
            format_expires_relative_at(Some(now - 90), now),
            "expired 1m ago"
        );
        assert_eq!(format_expires_relative_at(None, now), "no expiry");
        assert_eq!(format_expires_relative_at(Some(now + 3700), now), "in 1h1m");
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

    #[test]
    fn provider_scoped_pick_ignores_meta_era_generic_keys() {
        // Leftover MODEL_API_KEY must never beat xAI/OpenAI/Anthropic login.
        assert_eq!(
            pick_provider_credential(
                None,
                Some("xai-oauth-jwt"),
                None,
                None,
                None,
                None,
                Some("meta-or-model-key-leftover"),
            )
            .as_deref(),
            Some("xai-oauth-jwt")
        );
        assert_eq!(
            pick_provider_credential(
                Some("sk-openai-from-env"),
                Some("xai-oauth-jwt"),
                None,
                None,
                Some("nur-global"),
                None,
                Some("model-api-key"),
            )
            .as_deref(),
            Some("sk-openai-from-env"),
            "catalog env wins first for that provider"
        );
        assert_eq!(
            pick_provider_credential(
                None,
                None,
                Some("failover-key"),
                Some("failover-oauth"),
                Some("nur-global"),
                None,
                Some("model-api-key"),
            )
            .as_deref(),
            Some("failover-key")
        );
        // Only NUR_API_KEY is a valid last-resort global — META_/MODEL_ ignored.
        assert_eq!(
            pick_provider_credential(
                None,
                None,
                None,
                None,
                Some("nur-global"),
                None,
                Some("model-api-key"),
            )
            .as_deref(),
            Some("nur-global")
        );
        assert_eq!(
            pick_provider_credential(None, None, None, None, None, None, Some("model-api-key")),
            None,
            "META_/MODEL_/MUSE_ alone must not satisfy a provider-scoped resolve"
        );
        assert_eq!(
            pick_provider_credential(
                None,
                None,
                None,
                Some("provider-oauth"),
                None,
                Some("legacy-providerless-key"),
                None,
            )
            .as_deref(),
            Some("provider-oauth"),
            "provider-bound OAuth must beat a legacy providerless key"
        );
    }

    #[test]
    fn provider_key_store_roundtrip() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "nur_pk_{nanos}_{}",
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("provider_keys.json");

        assert!(read_keys_at(&path).is_empty());
        save_key_at(&path, "openai", "sk-abcdefgh").unwrap();
        save_key_at(&path, "anthropic", "sk-ant-xxxxxxxx").unwrap();
        assert_eq!(
            read_keys_at(&path).get("openai").map(String::as_str),
            Some("sk-abcdefgh")
        );
        assert_eq!(read_keys_at(&path).len(), 2);
        // Re-saving the same provider overwrites, doesn't duplicate.
        save_key_at(&path, "openai", "sk-newnewnew").unwrap();
        assert_eq!(
            read_keys_at(&path).get("openai").map(String::as_str),
            Some("sk-newnewnew")
        );
        assert_eq!(read_keys_at(&path).len(), 2);
        // Too-short keys are rejected.
        assert!(save_key_at(&path, "openai", "short").is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn provider_oauth_session_store_roundtrip() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "nur_ps_{nanos}_{}",
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("provider_sessions.json");

        assert!(load_provider_oauth_token_at(&path, "xai").is_none());
        let auth = oauth_auth(
            "xai",
            "oauth-access-token-xxxxx",
            Some("refresh-yyyy".into()),
            Some(now_unix() + 3600),
            None,
        )
        .unwrap();
        save_provider_session_at(&path, &auth).unwrap();
        assert_eq!(
            load_provider_oauth_token_at(&path, "xai").as_deref(),
            Some("oauth-access-token-xxxxx")
        );
        // A refreshed active session must replace the provider copy as one
        // complete credential set; mixing rotated access/refresh tokens causes
        // an immediate provider-side 401.
        let refreshed = oauth_auth(
            "xai",
            "oauth-access-token-newxx",
            Some("refresh-new-yyyy".into()),
            Some(now_unix() + 7200),
            None,
        )
        .unwrap();
        save_provider_session_at(&path, &refreshed).unwrap();
        assert_eq!(read_sessions_at(&path).get("xai"), Some(&refreshed));
        // Second provider coexists.
        let auth2 = oauth_auth("anthropic", "claude-token-zzzzzzzz", None, None, None).unwrap();
        save_provider_session_at(&path, &auth2).unwrap();
        assert_eq!(read_sessions_at(&path).len(), 2);
        assert_eq!(
            load_provider_oauth_token_at(&path, "anthropic").as_deref(),
            Some("claude-token-zzzzzzzz")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// OAuth sessions for browser-auth providers (incl. xAI / Claude / OpenAI /
    /// Kimi) must survive read — they are first-class login paths again.
    #[test]
    fn first_party_oauth_sessions_are_kept_on_read() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "nur_oauth_keep_{nanos}_{}",
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("provider_sessions.json");

        let mut map: BTreeMap<String, Auth> = BTreeMap::new();
        for id in ["openai", "anthropic", "xai", "kimi", "azure"] {
            map.insert(
                id.to_string(),
                oauth_auth(id, &format!("{id}-token"), None, None, None).unwrap(),
            );
        }
        write_sessions_at(&path, &map).unwrap();

        let read = read_sessions_at(&path);
        assert_eq!(read.len(), 5, "all browser_auth OAuth sessions survive");
        for id in ["openai", "anthropic", "xai", "kimi", "azure"] {
            assert!(
                load_provider_oauth_token_at(&path, id).is_some(),
                "'{id}' OAuth session must resolve"
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}

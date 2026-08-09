use crate::config::{auth_path, ensure_dirs};
use crate::error::{NurError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

static OAUTH_STORE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static KEY_STORE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static POLICY_STORE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn oauth_store_guard() -> MutexGuard<'static, ()> {
    OAUTH_STORE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn key_store_guard() -> MutexGuard<'static, ()> {
    KEY_STORE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn policy_store_guard() -> MutexGuard<'static, ()> {
    POLICY_STORE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn private_atomic_write(path: &Path, content: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut tmp = path.to_path_buf();
    tmp.set_extension(format!("tmp.{}", uuid::Uuid::new_v4().simple()));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        file.write_all(content)?;
        file.sync_all()?;
        #[cfg(windows)]
        if path.exists() {
            fs::remove_file(path)?;
        }
        fs::rename(&tmp, path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

pub(crate) fn save_manual_oauth_code(code: &str) -> Result<()> {
    let code = code.trim();
    if code.is_empty() || code.len() > 8 * 1024 || code.chars().any(|ch| matches!(ch, '\r' | '\n'))
    {
        return Err(NurError::Other("invalid OAuth authorization code".into()));
    }
    ensure_dirs()?;
    let path = crate::config::nur_home().join("oauth_paste_code.txt");
    private_atomic_write(&path, format!("{code}\n").as_bytes())
        .map_err(|error| NurError::Other(format!("could not save OAuth code: {error}")))
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

/// Is an OAuth expiry already past (or about to be)?
///
/// `None` means "no stated expiry" and is treated as still valid. Uses the same
/// 5-minute skew as [`refresh_oauth_in_place`] so a token that would be
/// refreshed there is not accepted here, where no refresh is possible.
pub fn oauth_expired(expires_at: Option<u64>) -> bool {
    match expires_at {
        None => false,
        Some(exp) => exp <= now_unix().saturating_add(300),
    }
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
    if auth.provider.is_empty() {
        return false;
    }
    if auth.provider == cfg_provider {
        return false;
    }
    // google / antigravity / google-oauth are the same family (Gemini via different flows).
    const GOOGLE_FAMILY: &[&str] = &["google", "antigravity", "google-oauth"];
    if GOOGLE_FAMILY.contains(&auth.provider.as_str()) && GOOGLE_FAMILY.contains(&cfg_provider) {
        return false;
    }
    true
}

fn normalize_legacy_omp_credential(auth: &mut Auth) {
    let is_omp = auth
        .oauth_meta
        .as_ref()
        .is_some_and(|meta| meta.issuer.eq_ignore_ascii_case("omp"))
        || auth
            .refresh_token
            .as_deref()
            .is_some_and(|refresh| refresh.starts_with("omp:"));
    if is_omp
        && matches!(auth.auth_method, AuthMethod::Oauth)
        && !crate::oauth::omp_bridge::omp_meta_is_oauth(auth.oauth_meta.as_ref(), &auth.api_key)
    {
        auth.auth_method = AuthMethod::ApiKey;
        auth.refresh_token = None;
        auth.expires_at = None;
        auth.source = "omp".into();
    }
}

/// Resolve a usable bearer credential (any provider / env).
/// Order: `NUR_API_KEY` → vendor/legacy envs → `~/.nur/auth.json` → legacy homes.
pub fn resolve_api_key() -> Result<String> {
    resolve_api_key_for(None)
}

/// Pure pick order for a *specific* catalog provider. Used by
/// [`resolve_api_key_for`] and unit-tested so unscoped keys cannot outrank a
/// provider login.
///
/// Inputs are already trimmed; empty string is treated as absent.
/// Pick the first non-empty credential from a priority list.
///
/// Explicit choices made in `/auth` outrank ambient environment variables.
/// The chooser removes a provider's conflicting saved method, so OAuth versus
/// API-key precedence is deterministic rather than dependent on stale files.
pub(crate) fn pick_provider_credential(
    matching_auth: Option<&str>,
    provider_oauth: Option<&str>,
    provider_key: Option<&str>,
    provider_env: Option<&str>,
    nur_global: Option<&str>,
    legacy_auth: Option<&str>,
) -> Option<String> {
    for cand in [
        matching_auth,
        provider_oauth,
        provider_key,
        provider_env,
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
/// first so a key for one provider never gets sent to another after `/login`:
/// 1. matching active OAuth session (refreshed), so env cannot replace it after restart
/// 2. matching `auth.json` API key
/// 3. per-provider OAuth store (browser login for a non-active provider)
/// 4. per-provider API key store
/// 5. catalog env (`XAI_API_KEY`, `OPENAI_API_KEY`, …)
/// 6. `NUR_API_KEY` only as a true global override
///
/// OAuth is preferred over a stored API key for non-active providers so a stale
/// `provider_keys.json` entry cannot shadow a live browser session (cross-provider
/// subagents, failover).
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
        // Prefer a live per-provider OAuth session over a stored API key. Cross-provider
        // subagents often hit a stale key in provider_keys.json (shared OpenRouter
        // scraps, revoked sk-…) while a valid browser login still sits in
        // provider_sessions.json — using the key first produced 401s that looked
        // like "grok/openai is broken" when the OAuth path would have worked.
        let failover_oauth = load_provider_oauth_token(exp);
        let failover_key = load_provider_key(exp);
        let nur_global = std::env::var("NUR_API_KEY")
            .ok()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty());
        if let Some(k) = pick_provider_credential(
            matching_auth.as_deref(),
            failover_oauth.as_deref(),
            failover_key.as_deref(),
            provider_env.as_deref(),
            nur_global.as_deref(),
            legacy_auth.as_deref(),
        ) {
            return Ok(k);
        }
        // Local and self-hosted OpenAI-compatible providers are usable without
        // credentials. Do not probe vendor CLIs or OMP and do not mark the TUI
        // signed out merely because no bearer exists.
        if crate::providers::by_id(exp).is_some_and(|provider| provider.key_optional) {
            return Ok(String::new());
        }
        // Saved nur credentials (active + per-provider + env) already failed.
        // Still try vendor CLI / OMP for the *expected* provider even when
        // auth.json is for a different host - otherwise a leftover openai login
        // would block anthropic failover/OMP forever.
        //
        // Vendor CLI imports stay transient (not written to auth.json). OMP
        // imports are persisted into the per-provider store so the next resolve
        // hits saved credentials before shelling out to `omp token` again.
        // Isolated via run_blocking: this can shell out (e.g. reading Windows
        // Credential Manager for a vendor-CLI session), and resolve_api_key_for
        // is reachable directly from the async main() / turn-1 startup path, so
        // an unguarded call here could stall a Tokio worker thread on the very
        // first request.
        if t3_fallback_allowed(exp) {
            if let Ok(Some(tokens)) =
                crate::oauth::run_blocking(|| crate::oauth::import_existing_session(exp))
            {
                let tok = tokens.access_token.trim().to_string();
                if !tok.is_empty() && !oauth_expired(tokens.expires_at) {
                    if crate::oauth::omp_bridge::is_omp_import(&tokens) {
                        if crate::oauth::omp_bridge::is_omp_oauth_import(&tokens) {
                            let _ = save_provider_oauth(
                                exp,
                                &tok,
                                tokens.refresh_token.clone(),
                                tokens.expires_at,
                                tokens.meta.clone(),
                            );
                        } else {
                            let _ = save_provider_key(exp, &tok);
                        }
                    }
                    return Ok(tok);
                }
                // Stale but refreshable. The vendor CLI would renew this silently on
                // its next use, so refusing here meant "signed into the CLI" did not
                // actually mean "signed into nur" — the token merely had to be more
                // than five minutes old. Mint a fresh one from the same refresh
                // token the CLI stores, and keep using it transiently.
                let mut refreshed_ok = false;
                if let Some(refresh) = tokens.refresh_token.as_deref().filter(|r| !r.is_empty()) {
                    let probe = Auth {
                        provider: exp.to_string(),
                        ..Default::default()
                    };
                    if let Ok(fresh) = crate::oauth::run_blocking(|| {
                        crate::oauth::refresh_tokens(exp, &probe, refresh)
                    }) {
                        let tok = fresh.access_token.trim().to_string();
                        if !tok.is_empty() && !oauth_expired(fresh.expires_at) {
                            // Persist the refreshed token back to the per-provider
                            // session store. Without this, the token we return no
                            // longer matches the stored session, so
                            // `oauth_request_context` refuses to attach
                            // `x-goog-user-project` and the generativelanguage host
                            // answers 401 UNAUTHENTICATED. Re-saving keeps the
                            // token <-> project_id link intact for google-family.
                            let _ = save_provider_oauth(
                                exp,
                                &tok,
                                fresh
                                    .refresh_token
                                    .clone()
                                    .or_else(|| Some(refresh.to_string())),
                                fresh.expires_at,
                                fresh.meta.clone(),
                            );
                            return Ok(tok);
                        }
                        refreshed_ok = true;
                    }
                }
                // For google-family CLI imports (antigravity / gcloud), the access
                // token is short-lived and the CLI itself refreshes it. If our
                // refresh attempt failed due to missing NUR_GOOGLE_CLIENT_ID (browser
                // flow not configured), returning NotAuthenticated leaves the user
                // in a "signed in · no key (local) -> signed out" loop even though
                // `agy` / `gcloud` has a valid session. Fall back to the original
                // token even if expired — the API will either accept it or return
                // 401, which then triggers a proper re-login prompt rather than a
                // silent sign-out. This keeps `antigravity` working for CLI-only users.
                let is_google_family = matches!(exp, "google" | "antigravity" | "google-oauth");
                if is_google_family && !tok.is_empty() {
                    // If we attempted refresh and it failed, or token is from CLI,
                    // still return it as last resort instead of NotAuthenticated.
                    return Ok(tok);
                }
                if !refreshed_ok {
                    // For non-google providers, still give expired token a chance if
                    // no refresh was attempted, to avoid silent sign-out loops.
                    if !tok.is_empty() {
                        return Ok(tok);
                    }
                }
            }
        }
        if mismatched {
            if let Ok(Some(auth)) = load_auth() {
                return Err(NurError::Other(format!(
                    "saved credentials are for provider '{}' but active provider is '{}'. Run /login (or nur auth logout) and sign in again.",
                    auth.provider, exp
                )));
            }
        }
        return Err(NurError::NotAuthenticated);
    }

    // No expected provider: generic env first (scripts / headless), then auth.json.
    for var in ["NUR_API_KEY", "META_API_KEY"] {
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
    Err(NurError::NotAuthenticated)
}

pub fn load_auth() -> Result<Option<Auth>> {
    let path = auth_path();
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path)?;
    let mut auth: Auth = serde_json::from_str(&text)?;
    normalize_legacy_omp_credential(&mut auth);
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
    // Cursor CLI session sentinel is not a Bearer token for HTTP headers.
    if crate::api::cursor_cli::is_cli_session_token(access_token) {
        return None;
    }
    // The google family (google / antigravity / google-oauth) shares one login;
    // a session saved under any of those ids must satisfy a context lookup for
    // any other, or the request goes out WITHOUT `x-goog-user-project` and the
    // generativelanguage host answers 401 UNAUTHENTICATED.
    const GOOGLE_FAMILY: &[&str] = &["google", "antigravity", "google-oauth"];
    let same_family = |a: &str, b: &str| -> bool {
        a == b || (GOOGLE_FAMILY.contains(&a) && GOOGLE_FAMILY.contains(&b))
    };
    let matches_session = |auth: &Auth| {
        matches!(auth.auth_method, AuthMethod::Oauth)
            && same_family(auth.provider.as_str(), provider_id)
            && auth.api_key.trim() == access_token.trim()
    };
    let active = load_auth().ok().flatten().filter(&matches_session);
    // For the stored side, scan all family ids (the session may be under a
    // sibling id even when `provider_id` is the configured one).
    let stored = {
        let sessions = read_sessions_at(&crate::config::provider_sessions_path());
        let mut found = sessions.get(provider_id).cloned().filter(&matches_session);
        if found.is_none() && GOOGLE_FAMILY.contains(&provider_id) {
            for alias in GOOGLE_FAMILY {
                if let Some(a) = sessions.get(*alias).cloned().filter(&matches_session) {
                    found = Some(a);
                    break;
                }
            }
        }
        found
    };
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
        Err(error) => Err(NurError::Other(format!(
            "OAuth token expired and refresh failed ({error}). Run /login again."
        ))),
    }
}

fn refresh_oauth_with_token(auth: &mut Auth, refresh: &str) -> Result<bool> {
    let provider = auth.provider.as_str();
    // Provider adapters use reqwest's blocking client (and, for vendor-CLI
    // imported sessions, may shell out to read an OS credential store, which
    // can take several seconds). Isolate via `run_blocking` so this can never
    // stall every Tokio worker thread at once — see its doc comment for why a
    // bare `thread::spawn(..).join()` here previously caused turn-1 and
    // concurrent-subagent hangs whenever a token needed refreshing.
    let tokens =
        crate::oauth::run_blocking(|| crate::oauth::refresh_tokens(provider, auth, refresh))?;
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
fn ensure_fresh_oauth_unlocked(auth: &mut Auth) -> Result<()> {
    if refresh_oauth_in_place(auth)? {
        save_auth(auth)?;
    }
    if matches!(auth.auth_method, AuthMethod::Oauth) && !auth.provider.trim().is_empty() {
        save_provider_session(auth)?;
    }
    Ok(())
}

pub fn ensure_fresh_oauth(auth: &mut Auth) -> Result<()> {
    let _guard = oauth_store_guard();
    ensure_fresh_oauth_unlocked(auth)
}

/// Resolve the current access token for an OAuth-backed client without allowing
/// environment API keys to change that client's routing or wire protocol.
pub fn resolve_oauth_access_token(provider_id: &str) -> Result<Option<String>> {
    let _guard = oauth_store_guard();
    if let Some(mut auth) = load_auth()? {
        if matches!(auth.auth_method, AuthMethod::Oauth) && !provider_mismatch(&auth, provider_id) {
            ensure_fresh_oauth_unlocked(&mut auth)?;
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
        if matches!(auth.auth_method, AuthMethod::Oauth) && !provider_mismatch(&auth, provider_id) {
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
    private_atomic_write(&path, text.as_bytes())
        .map_err(|e| NurError::Other(format!("failed to save auth atomically: {e}")))?;
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
        return Err(NurError::Other(
            "API key too short — expected at least 8 characters".into(),
        ));
    }
    if trimmed.len() > 16 * 1024 {
        return Err(NurError::Other("API key is unexpectedly large".into()));
    }
    if trimmed.chars().any(char::is_whitespace) {
        return Err(NurError::Other("API key contains whitespace".into()));
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
    save_auth(&auth)?;
    if let Some(provider) = provider.filter(|provider| !provider.trim().is_empty()) {
        allow_t3_fallback(provider)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CredentialPolicy {
    #[serde(default)]
    blocked_t3: std::collections::BTreeSet<String>,
}

const GOOGLE_CREDENTIAL_FAMILY: &[&str] = &["google", "antigravity", "google-oauth"];

fn credential_family_ids(provider_id: &str) -> Vec<&str> {
    if GOOGLE_CREDENTIAL_FAMILY.contains(&provider_id) {
        GOOGLE_CREDENTIAL_FAMILY.to_vec()
    } else {
        vec![provider_id]
    }
}

fn read_credential_policy() -> CredentialPolicy {
    std::fs::read_to_string(crate::config::credential_policy_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_credential_policy(policy: &CredentialPolicy) -> Result<()> {
    ensure_dirs()?;
    let text = serde_json::to_string_pretty(policy)?;
    private_atomic_write(&crate::config::credential_policy_path(), text.as_bytes())
        .map_err(|error| NurError::Other(format!("failed to save credential policy: {error}")))
}

/// Whether automatic vendor-CLI / OMP discovery is allowed for this provider.
/// Explicitly saved credentials and environment variables are unaffected.
pub fn t3_fallback_allowed(provider_id: &str) -> bool {
    let id = provider_id.trim();
    let policy = read_credential_policy();
    !credential_family_ids(id)
        .iter()
        .any(|family_id| policy.blocked_t3.contains(*family_id))
}

/// Re-enable normal T3 discovery after a deliberate save or import.
pub fn allow_t3_fallback(provider_id: &str) -> Result<()> {
    let id = provider_id.trim();
    if id.is_empty() {
        return Ok(());
    }
    let _guard = policy_store_guard();
    let mut policy = read_credential_policy();
    let changed = credential_family_ids(id)
        .into_iter()
        .fold(false, |changed, family_id| {
            policy.blocked_t3.remove(family_id) || changed
        });
    if changed {
        write_credential_policy(&policy)?;
    }
    Ok(())
}

fn block_t3_fallback(provider_id: &str) -> Result<()> {
    let id = provider_id.trim();
    if id.is_empty() {
        return Ok(());
    }
    let _guard = policy_store_guard();
    let mut policy = read_credential_policy();
    let changed = credential_family_ids(id)
        .into_iter()
        .fold(false, |changed, family_id| {
            policy.blocked_t3.insert(family_id.to_string()) || changed
        });
    if changed {
        write_credential_policy(&policy)?;
    }
    Ok(())
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
        return Err(NurError::Other(
            "API key too short — expected at least 8 characters".into(),
        ));
    }
    if trimmed.len() > 16 * 1024 {
        return Err(NurError::Other("API key is unexpectedly large".into()));
    }
    if trimmed.chars().any(char::is_whitespace) {
        return Err(NurError::Other("API key contains whitespace".into()));
    }
    let mut map = if path.exists() {
        let text = fs::read_to_string(path)?;
        serde_json::from_str(&text).map_err(|error| {
            NurError::Other(format!(
                "refusing to overwrite malformed provider key store {}: {error}",
                path.display()
            ))
        })?
    } else {
        BTreeMap::new()
    };
    map.insert(provider_id.to_string(), trimmed.to_string());
    let text = serde_json::to_string_pretty(&map)?;
    private_atomic_write(path, text.as_bytes())
        .map_err(|e| NurError::Other(format!("failed to save provider keys: {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// A stored per-provider failover key, if one was saved for this provider id.
pub fn load_provider_key(provider_id: &str) -> Option<String> {
    let map = read_keys_at(&crate::config::provider_keys_path());
    if let Some(k) = map
        .get(provider_id)
        .cloned()
        .filter(|k| !k.trim().is_empty())
    {
        return Some(k);
    }
    // google family alias fallback
    const GOOGLE_FAMILY: &[&str] = &["google", "antigravity", "google-oauth"];
    if GOOGLE_FAMILY.contains(&provider_id) {
        for alias in GOOGLE_FAMILY {
            if *alias == provider_id {
                continue;
            }
            if let Some(k) = map.get(*alias).cloned().filter(|k| !k.trim().is_empty()) {
                return Some(k);
            }
        }
    }
    // Migration path for OMP API keys that older nur builds persisted in the
    // OAuth session map. `read_sessions_at` normalizes their method in memory;
    // treating that entry as a key fixes routing without discarding access if
    // OMP is temporarily unavailable.
    let sessions = read_sessions_at(&crate::config::provider_sessions_path());
    if let Some(auth) = sessions.get(provider_id) {
        if matches!(auth.auth_method, AuthMethod::ApiKey) {
            return non_empty_access_token(auth);
        }
    }
    None
}

/// Save a per-provider failover key (validated like a normal API key).
pub fn save_provider_key(provider_id: &str, key: &str) -> Result<()> {
    ensure_dirs()?;
    {
        let _guard = key_store_guard();
        save_key_at(&crate::config::provider_keys_path(), provider_id, key)?;
    }
    allow_t3_fallback(provider_id)
}

/// Save an API key as the user's authoritative choice for this provider. Any
/// older saved OAuth session is removed so it cannot continue to outrank the
/// replacement key. If this is the active provider, `auth.json` is updated too.
pub fn choose_provider_key(provider_id: &str, key: &str) -> Result<()> {
    save_provider_key(provider_id, key)?;
    {
        let _guard = oauth_store_guard();
        let path = crate::config::provider_sessions_path();
        let mut sessions = read_sessions_at(&path);
        let changed = credential_family_ids(provider_id)
            .into_iter()
            .fold(false, |changed, family_id| {
                sessions.remove(family_id).is_some() || changed
            });
        if changed {
            write_sessions_at(&path, &sessions)?;
        }
    }
    let active_matches = load_auth()?.is_some_and(|auth| {
        !auth.provider.trim().is_empty() && !provider_mismatch(&auth, provider_id)
    });
    if active_matches {
        save_api_key_for(key, Some(provider_id))?;
    }
    Ok(())
}

fn forget_provider_at(keys_path: &Path, sessions_path: &Path, provider_id: &str) -> bool {
    let id = provider_id.trim();
    if id.is_empty() {
        return false;
    }
    const GOOGLE_FAMILY: &[&str] = &["google", "antigravity", "google-oauth"];
    let ids: Vec<&str> = if GOOGLE_FAMILY.contains(&id) {
        GOOGLE_FAMILY.to_vec()
    } else {
        vec![id]
    };
    // Never turn a parse error into an empty map and overwrite the user's
    // remaining credentials while trying to remove one entry.
    for path in [keys_path, sessions_path] {
        if path.exists()
            && fs::read_to_string(path)
                .ok()
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                .is_none()
        {
            return false;
        }
    }
    let mut removed = false;

    let mut keys = read_keys_at(keys_path);
    let mut removed_key = false;
    for id in &ids {
        removed_key |= keys.remove(*id).is_some();
    }
    if removed_key {
        removed = true;
        let text = serde_json::to_string_pretty(&keys).unwrap_or_else(|_| "{}".into());
        let _ = private_atomic_write(keys_path, text.as_bytes());
    }

    let mut sessions = read_sessions_at(sessions_path);
    let mut removed_session = false;
    for id in &ids {
        removed_session |= sessions.remove(*id).is_some();
    }
    if removed_session {
        removed = true;
        let _ = write_sessions_at(sessions_path, &sessions);
    }
    removed
}

/// Drop every stored credential for one provider: its failover API key and its
/// saved OAuth session.
///
/// `logout` only removes the *active* credential (`auth.json`). Those two
/// side stores exist so other providers stay usable for failover and for
/// subagents running on a different model - which is exactly why signing out of
/// an account has to clear that account's copies too, or "cleared" would leave
/// a working key behind. Returns whether anything was actually removed.
pub fn forget_provider(provider_id: &str) -> bool {
    // Always take locks in OAuth -> API-key order. No other path takes both.
    let _oauth_guard = oauth_store_guard();
    let _key_guard = key_store_guard();
    forget_provider_at(
        &crate::config::provider_keys_path(),
        &crate::config::provider_sessions_path(),
        provider_id,
    )
}

/// Remove every Nur-managed credential for one provider and suppress automatic
/// vendor CLI / OMP re-import until the user explicitly chooses it again in
/// `/auth`. Environment variables remain visible and cannot be mutated by Nur.
pub fn delete_provider_credentials(provider_id: &str) -> Result<bool> {
    let id = provider_id.trim();
    if id.is_empty() {
        return Ok(false);
    }
    let active = load_auth()?;
    let active_matches = active
        .as_ref()
        .is_some_and(|auth| !auth.provider.trim().is_empty() && !provider_mismatch(auth, id));
    if active_matches {
        let path = auth_path();
        if path.exists() {
            fs::remove_file(path)?;
        }
    }
    let removed = forget_provider(id);
    block_t3_fallback(id)?;
    crate::oauth::omp_bridge::invalidate_omp_token_cache();
    Ok(removed || active_matches)
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
    let _guard = oauth_store_guard();
    let mut auth = oauth_auth(provider, access_token, refresh_token, expires_at, meta)?;
    // Imported CLI sessions can already be near expiry. Canonicalize before
    // either store is written so a newly created client never receives a token
    // that this refresh immediately revokes.
    refresh_oauth_in_place(&mut auth)?;
    save_auth(&auth)?;
    save_provider_session(&auth)?;
    allow_t3_fallback(provider)?;
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
        return Err(NurError::Other("empty OAuth access token".into()));
    }
    if access.len() > 1024 * 1024 {
        return Err(NurError::Other(
            "OAuth access token is unexpectedly large".into(),
        ));
    }
    if access.chars().any(char::is_whitespace) {
        return Err(NurError::Other(
            "OAuth access token contains whitespace".into(),
        ));
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
    for auth in map.values_mut() {
        normalize_legacy_omp_credential(auth);
    }
    // Legacy antigravity -> google migration, but keep alias for the still-existing
    // antigravity catalog entry. Without this, a session saved as `antigravity`
    // disappears on read and `resolve_api_key_for("antigravity")` finds nothing,
    // leaving the user in a "signed in · no key (local) -> signed out" loop.
    if let Some(legacy) = map.get("antigravity").cloned() {
        let mut as_google = legacy.clone();
        as_google.provider = "google".into();
        map.entry("google".into()).or_insert(as_google);
        // keep the original antigravity entry as well so both ids resolve
    }
    // Also ensure google-oauth alias resolves
    if let Some(g) = map.get("google").cloned() {
        map.entry("antigravity".into()).or_insert_with(|| {
            let mut a = g.clone();
            a.provider = "antigravity".into();
            a
        });
        map.entry("google-oauth".into()).or_insert_with(|| {
            let mut a = g.clone();
            a.provider = "google-oauth".into();
            a
        });
    }
    map.retain(|id, auth| {
        !matches!(auth.auth_method, AuthMethod::Oauth) || oauth_session_supported(id)
    });
    map
}

fn write_sessions_at(path: &Path, map: &BTreeMap<String, Auth>) -> Result<()> {
    let text = serde_json::to_string_pretty(map)?;
    private_atomic_write(path, text.as_bytes())
        .map_err(|e| NurError::Other(format!("failed to save provider sessions: {e}")))?;
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
        return Err(NurError::Other(
            "provider session needs a non-empty provider id".into(),
        ));
    }
    if path.exists() {
        let text = fs::read_to_string(path)?;
        serde_json::from_str::<BTreeMap<String, Auth>>(&text).map_err(|error| {
            NurError::Other(format!(
                "refusing to overwrite malformed provider session store {}: {error}",
                path.display()
            ))
        })?;
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
    let _guard = oauth_store_guard();
    let mut auth = oauth_auth(provider, access_token, refresh_token, expires_at, meta)?;
    refresh_oauth_in_place(&mut auth)?;
    save_provider_session(&auth)?;
    allow_t3_fallback(provider)
}

/// Save OAuth as the authoritative choice for a provider and remove an older
/// saved API key. The active slot is updated only when it already belongs to
/// this provider, so managing another provider never changes the current one.
pub fn choose_provider_oauth(
    provider: &str,
    access_token: &str,
    refresh_token: Option<String>,
    expires_at: Option<u64>,
    meta: Option<OauthMeta>,
) -> Result<()> {
    save_provider_oauth(
        provider,
        access_token,
        refresh_token.clone(),
        expires_at,
        meta.clone(),
    )?;
    {
        let _guard = key_store_guard();
        let path = crate::config::provider_keys_path();
        let mut keys = read_keys_at(&path);
        let changed = credential_family_ids(provider)
            .into_iter()
            .fold(false, |changed, family_id| {
                keys.remove(family_id).is_some() || changed
            });
        if changed {
            let text = serde_json::to_string_pretty(&keys)?;
            private_atomic_write(&path, text.as_bytes()).map_err(|error| {
                NurError::Other(format!("failed to save provider keys: {error}"))
            })?;
        }
    }
    let active_matches = load_auth()?.is_some_and(|auth| {
        !auth.provider.trim().is_empty() && !provider_mismatch(&auth, provider)
    });
    if active_matches {
        save_oauth_session(provider, access_token, refresh_token, expires_at, meta)?;
    }
    Ok(())
}

/// Best-effort: patch `project_id` / `tier_id` into existing google-family OAuth
/// sessions without wiping access/refresh tokens. Used after Cloud Code re-onboard.
pub fn update_oauth_project_meta(
    provider_id: &str,
    project_id: &str,
    tier_id: Option<&str>,
) -> Result<bool> {
    if project_id.trim().is_empty() {
        return Ok(false);
    }
    let _guard = oauth_store_guard();
    const GOOGLE_FAMILY: &[&str] = &["google", "antigravity", "google-oauth"];
    let is_family = GOOGLE_FAMILY.contains(&provider_id);

    let patch = |auth: &mut Auth| {
        let mut meta = auth.oauth_meta.clone().unwrap_or_default();
        if !meta.extra.is_object() {
            meta.extra = serde_json::json!({});
        }
        if let Some(obj) = meta.extra.as_object_mut() {
            obj.insert(
                "project_id".into(),
                serde_json::Value::String(project_id.trim().to_string()),
            );
            if let Some(t) = tier_id.map(str::trim).filter(|s| !s.is_empty()) {
                obj.insert("tier_id".into(), serde_json::Value::String(t.to_string()));
            }
        }
        auth.oauth_meta = Some(meta);
    };

    let mut changed = false;

    if let Ok(Some(mut auth)) = load_auth() {
        if matches!(auth.auth_method, AuthMethod::Oauth)
            && (auth.provider == provider_id
                || (is_family && GOOGLE_FAMILY.contains(&auth.provider.as_str())))
        {
            patch(&mut auth);
            save_auth(&auth)?;
            save_provider_session(&auth)?;
            changed = true;
        }
    }

    let path = crate::config::provider_sessions_path();
    let mut map = read_sessions_at(&path);
    let keys: Vec<&str> = if is_family {
        GOOGLE_FAMILY.to_vec()
    } else {
        vec![provider_id]
    };
    for key in keys {
        if let Some(auth) = map.get_mut(key) {
            if matches!(auth.auth_method, AuthMethod::Oauth) {
                patch(auth);
                changed = true;
            }
        }
    }
    if changed {
        write_sessions_at(&path, &map)?;
    }
    Ok(changed)
}

fn save_provider_session(auth: &Auth) -> Result<()> {
    ensure_dirs()?;
    save_provider_session_at(&crate::config::provider_sessions_path(), auth)
}

/// Load a usable bearer for a failover provider from the per-provider OAuth
/// store (refreshing if needed). `None` if no session or refresh failed hard.
pub fn load_provider_oauth_token(provider_id: &str) -> Option<String> {
    if let Some(t) = resolve_oauth_access_token(provider_id).ok().flatten() {
        return Some(t);
    }
    // google family alias fallback
    const GOOGLE_FAMILY: &[&str] = &["google", "antigravity", "google-oauth"];
    if GOOGLE_FAMILY.contains(&provider_id) {
        for alias in GOOGLE_FAMILY {
            if *alias == provider_id {
                continue;
            }
            if let Some(t) = resolve_oauth_access_token(alias).ok().flatten() {
                return Some(t);
            }
        }
    }
    None
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

/// One non-secret status label per catalog provider for the scrollable `/auth`
/// manager. This reads local stores once and never refreshes tokens or invokes
/// OMP. First-party CLI probes are local-only and expose no credential data.
pub fn provider_credential_summaries() -> BTreeMap<String, String> {
    let keys = read_keys_at(&crate::config::provider_keys_path());
    let sessions = read_sessions_at(&crate::config::provider_sessions_path());
    let active = load_auth().ok().flatten();
    let policy = read_credential_policy();
    let mut out = BTreeMap::new();

    for provider in crate::providers::PROVIDERS {
        let mut sources: Vec<String> = Vec::new();
        if provider.key_optional {
            sources.push("local · no auth".into());
        }
        if active.as_ref().is_some_and(|auth| {
            !auth.provider.trim().is_empty() && !provider_mismatch(auth, provider.id)
        }) {
            sources.push("active".into());
        }
        if let Some(auth) = sessions.get(provider.id) {
            let source = if auth
                .oauth_meta
                .as_ref()
                .is_some_and(|meta| meta.issuer.eq_ignore_ascii_case("omp"))
            {
                "OMP OAuth"
            } else if oauth_expired(auth.expires_at)
                && auth.refresh_token.as_deref().is_none_or(str::is_empty)
            {
                "OAuth expired"
            } else {
                "OAuth"
            };
            sources.push(source.into());
        }
        if keys
            .get(provider.id)
            .is_some_and(|value| !value.trim().is_empty())
        {
            sources.push("saved key".into());
        }
        if std::env::var(provider.env_key).is_ok_and(|value| !value.trim().is_empty()) {
            sources.push(format!("env {}", provider.env_key));
        }
        let driver = match provider.id {
            "openai" => Some(crate::t3code::DriverId::Codex),
            "anthropic" => Some(crate::t3code::DriverId::Claude),
            "xai" => Some(crate::t3code::DriverId::Grok),
            "google" => Some(crate::t3code::DriverId::Gemini),
            "antigravity" => Some(crate::t3code::DriverId::Antigravity),
            "cursor" => Some(crate::t3code::DriverId::Cursor),
            _ => None,
        };
        if driver.is_some_and(|driver| crate::t3code::probe_driver(driver).has_credentials) {
            sources.push("CLI available".into());
        }
        if policy.blocked_t3.contains(provider.id) {
            sources.push("T3 blocked".into());
        } else if !provider.key_optional {
            sources.push("CLI/OMP fallback".into());
        }
        if sources.is_empty() {
            sources.push("add credential".into());
        }
        out.insert(provider.id.to_string(), sources.join(" · "));
    }
    out
}

/// Non-secret readiness lines for browser/official-CLI providers.
///
/// This deliberately does not refresh tokens or make network requests. It
/// reports which local source can satisfy a route so `doctor` can distinguish
/// "provider down" from "Nur never found the login".
pub fn provider_health_report() -> Vec<String> {
    let keys = read_keys_at(&crate::config::provider_keys_path());
    let sessions = read_sessions_at(&crate::config::provider_sessions_path());
    let active = load_auth().ok().flatten();
    crate::providers::oauth_browser_provider_ids()
        .iter()
        .filter_map(|id| {
            let provider = crate::providers::by_id(id)?;
            let mut sources = Vec::new();
            if std::env::var(provider.env_key).is_ok_and(|value| !value.trim().is_empty()) {
                sources.push(format!("env:{}", provider.env_key));
            }
            if keys.get(*id).is_some_and(|value| !value.trim().is_empty()) {
                sources.push("saved-key".into());
            }
            if let Some(auth) = sessions.get(*id) {
                let expiry = if oauth_expired(auth.expires_at) {
                    if auth
                        .refresh_token
                        .as_deref()
                        .is_some_and(|token| !token.trim().is_empty())
                    {
                        "oauth-refreshable"
                    } else {
                        "oauth-expired"
                    }
                } else {
                    "oauth"
                };
                sources.push(expiry.into());
            }
            if let Some(auth) = active.as_ref().filter(|auth| {
                auth.provider == *id
                    || (crate::providers::is_google_family(&auth.provider)
                        && crate::providers::is_google_family(id))
            }) {
                let usable = !matches!(auth.auth_method, AuthMethod::Oauth)
                    || !oauth_expired(auth.expires_at)
                    || auth
                        .refresh_token
                        .as_deref()
                        .is_some_and(|token| !token.trim().is_empty());
                if usable {
                    sources.push("active".into());
                }
            }
            let driver = match *id {
                "openai" => Some(crate::t3code::DriverId::Codex),
                "anthropic" => Some(crate::t3code::DriverId::Claude),
                "xai" => Some(crate::t3code::DriverId::Grok),
                "google" => Some(crate::t3code::DriverId::Gemini),
                "antigravity" => Some(crate::t3code::DriverId::Antigravity),
                "cursor" => Some(crate::t3code::DriverId::Cursor),
                _ => None,
            };
            if let Some(driver) = driver {
                let probe = crate::t3code::probe_driver(driver);
                if probe.has_credentials {
                    sources.push(format!("{}-cli", driver.as_str()));
                } else if probe.binary_present {
                    sources.push(format!("{}-cli:no-importable-session", driver.as_str()));
                }
            }
            let ready = sources.iter().any(|source| {
                !source.ends_with(":no-importable-session") && source != "oauth-expired"
            });
            let state = if ready { "ready" } else { "login needed" };
            let source = if sources.is_empty() {
                "none".into()
            } else {
                sources.join(",")
            };
            Some(format!("{:<14} {:<12} {}", id, state, source))
        })
        .collect()
}

/// Delete local credentials. If `revoke` is true, best-effort remote revoke first.
pub fn logout(revoke: bool) -> Result<()> {
    let active = match load_auth() {
        Ok(active) => active,
        Err(error) => {
            if revoke {
                eprintln!("revoke note: could not parse local auth ({error}); continuing cleanup");
            }
            None
        }
    };
    if revoke {
        if let Some(auth) = active.as_ref() {
            match crate::oauth::revoke_session(auth) {
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
    if let Some(provider) = active
        .as_ref()
        .map(|auth| auth.provider.trim())
        .filter(|provider| !provider.is_empty())
    {
        forget_provider(provider);
    }
    crate::oauth::omp_bridge::invalidate_omp_token_cache();
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
    let cfg_provider = crate::config::load_config()
        .map(|cfg| cfg.provider)
        .unwrap_or_default();
    let active = load_auth()?;
    let active_matching = active.as_ref().is_some_and(|auth| {
        !provider_mismatch(auth, &cfg_provider) && !auth.api_key.trim().is_empty()
    });
    let provider_env = crate::providers::by_id(&cfg_provider).and_then(|provider| {
        std::env::var(provider.env_key)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(|_| provider.env_key)
    });
    let env_source = if let Some(name) = provider_env {
        Some(format!("{name} env ({cfg_provider})"))
    } else if active_matching {
        None
    } else if std::env::var("NUR_API_KEY")
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false)
    {
        Some("NUR_API_KEY env".to_string())
    } else if cfg_provider == "meta"
        && std::env::var("META_API_KEY")
            .map(|k| !k.trim().is_empty())
            .unwrap_or(false)
    {
        Some("META_API_KEY env (Meta provider)".to_string())
    } else {
        None
    };

    if let Some(src) = env_source {
        let key = resolve_api_key_for(Some(&cfg_provider))?;
        println!("authenticated: yes");
        println!("source: {src}");
        println!("method: api_key (env)");
        println!("provider: (env — not scoped)");
        println!("expires: no expiry");
        println!("key: {}", key_fingerprint(&key));
        println!("note: env keys override ~/.nur/auth.json");
        return Ok(());
    }

    match active {
        Some(mut auth) if !auth.api_key.trim().is_empty() => {
            let _ = ensure_fresh_oauth(&mut auth);
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
        return Err(NurError::Other("empty API key".into()));
    }
    let provider = crate::config::load_config()
        .map(|cfg| cfg.provider)
        .unwrap_or_else(|_| crate::providers::default_provider().id.to_string());
    save_api_key_for(key, Some(&provider))?;
    save_provider_key(&provider, key)?;
    crate::oauth::omp_bridge::invalidate_omp_token_cache();
    println!("saved to {}", auth_path().display());
    println!("provider: {provider}");
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
    fn oauth_access_tokens_reject_whitespace() {
        assert!(oauth_auth("openai", "valid-token-value", None, None, None).is_ok());
        assert!(oauth_auth("openai", "token\r\ninjected", None, None, None).is_err());
        assert!(oauth_auth("openai", "token with spaces", None, None, None).is_err());
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
    fn provider_health_report_is_complete_and_never_prints_credentials() {
        let report = provider_health_report();
        for id in crate::providers::oauth_browser_provider_ids() {
            assert!(
                report.iter().any(|line| line.starts_with(id)),
                "missing {id}: {report:?}"
            );
        }
        let joined = report.join("\n").to_ascii_lowercase();
        assert!(!joined.contains("access_token"));
        assert!(!joined.contains("refresh_token"));
        assert!(!joined.contains("bearer "));
    }

    #[test]
    fn provider_mismatch_rules() {
        let mut a = Auth::default();
        assert!(!provider_mismatch(&a, "xai"));
        a.provider = "xai".into();
        assert!(!provider_mismatch(&a, "xai"));
        assert!(provider_mismatch(&a, "openai"));
    }

    #[test]
    fn legacy_omp_api_key_is_not_kept_on_oauth_route() {
        let mut auth = Auth {
            api_key: "sk-test-abcdefghijklmnopqrstuvwxyz".into(),
            source: "oauth".into(),
            auth_method: AuthMethod::Oauth,
            provider: "openai".into(),
            refresh_token: Some("omp:openai".into()),
            expires_at: None,
            oauth_meta: Some(OauthMeta {
                issuer: "omp".into(),
                client_id: "omp-token".into(),
                extra: serde_json::json!({
                    "omp_provider": "openai",
                    "nur_provider": "openai"
                }),
            }),
        };
        normalize_legacy_omp_credential(&mut auth);
        assert!(matches!(auth.auth_method, AuthMethod::ApiKey));
        assert!(auth.refresh_token.is_none());
        let provider = crate::providers::by_id("openai").unwrap();
        let (base, _, _) = crate::providers::endpoint_for_credential(
            provider,
            matches!(auth.auth_method, AuthMethod::Oauth),
        );
        assert_eq!(base, provider.base_url);
        assert_ne!(base, crate::providers::OPENAI_OAUTH_BASE_URL);
    }

    #[test]
    fn provider_scoped_pick_has_stable_precedence() {
        assert_eq!(
            pick_provider_credential(None, Some("xai-oauth-jwt"), None, None, None, None,)
                .as_deref(),
            Some("xai-oauth-jwt")
        );
        assert_eq!(
            pick_provider_credential(
                Some("xai-oauth-jwt"),
                None,
                None,
                Some("sk-openai-from-env"),
                Some("nur-global"),
                None,
            )
            .as_deref(),
            Some("xai-oauth-jwt"),
            "the credential explicitly selected in /auth beats ambient env"
        );
        // A chosen OAuth session outranks a stored API key. The UI normally
        // removes that conflicting key, but this also makes legacy state safe.
        assert_eq!(
            pick_provider_credential(
                None,
                None,
                Some("failover-oauth"),
                Some("failover-key"),
                Some("nur-global"),
                None,
            )
            .as_deref(),
            Some("failover-oauth"),
            "OAuth failover must outrank a stored API key"
        );
        assert_eq!(
            pick_provider_credential(
                None,
                None,
                None,
                Some("failover-key"),
                Some("nur-global"),
                None,
            )
            .as_deref(),
            Some("failover-key"),
            "API key still used when no OAuth session exists"
        );
        // NUR_API_KEY is the valid last-resort global override.
        assert_eq!(
            pick_provider_credential(None, None, None, None, Some("nur-global"), None,).as_deref(),
            Some("nur-global")
        );
        assert_eq!(
            pick_provider_credential(None, None, None, None, None, None),
            None,
            "unscoped vendor keys alone must not satisfy a provider-scoped resolve"
        );
        assert_eq!(
            pick_provider_credential(
                None,
                None,
                None,
                Some("provider-oauth"),
                None,
                Some("legacy-providerless-key"),
            )
            .as_deref(),
            Some("provider-oauth"),
            "provider-bound OAuth must beat a legacy providerless key"
        );
    }

    /// Signing out of one provider must clear that provider's saved copies and
    /// leave every other provider's alone - `/login` no longer clears anything,
    /// so this is the only thing that removes a stored credential.
    #[test]
    fn forget_provider_clears_one_account_and_spares_the_rest() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "nur_forget_{nanos}_{}",
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let keys = dir.join("provider_keys.json");
        let sessions = dir.join("provider_sessions.json");

        save_key_at(&keys, "openai", "sk-abcdefgh").unwrap();
        save_key_at(&keys, "anthropic", "sk-ant-xxxxxxxx").unwrap();
        save_provider_session_at(
            &sessions,
            &Auth {
                api_key: "sk-ant-oat-token".into(),
                source: "oauth".into(),
                auth_method: AuthMethod::Oauth,
                provider: "anthropic".into(),
                refresh_token: Some("r".into()),
                expires_at: None,
                oauth_meta: None,
            },
        )
        .unwrap();

        assert!(forget_provider_at(&keys, &sessions, "anthropic"));
        assert!(
            !read_keys_at(&keys).contains_key("anthropic"),
            "the key must be gone"
        );
        assert!(
            !read_sessions_at(&sessions).contains_key("anthropic"),
            "the OAuth session must be gone too, or the account is still usable"
        );
        assert_eq!(
            read_keys_at(&keys).get("openai").map(String::as_str),
            Some("sk-abcdefgh"),
            "signing out of one provider must not touch another"
        );

        // Nothing stored for that provider - reports no-op rather than lying.
        assert!(!forget_provider_at(&keys, &sessions, "groq"));
        assert!(!forget_provider_at(&keys, &sessions, "   "));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn forget_google_clears_all_family_aliases() {
        let dir = std::env::temp_dir().join(format!("nur_forget_google_{}", now_unix()));
        std::fs::create_dir_all(&dir).unwrap();
        let keys = dir.join("provider_keys.json");
        let sessions = dir.join("provider_sessions.json");
        for id in ["google", "antigravity", "google-oauth"] {
            save_key_at(&keys, id, &format!("{id}-key-value")).unwrap();
        }
        save_key_at(&keys, "openai", "openai-key-value").unwrap();
        let mut map = BTreeMap::new();
        for id in ["google", "antigravity"] {
            map.insert(
                id.into(),
                oauth_auth(id, &format!("{id}-oauth-token"), None, None, None).unwrap(),
            );
        }
        write_sessions_at(&sessions, &map).unwrap();

        assert!(forget_provider_at(&keys, &sessions, "google"));
        let remaining_keys = read_keys_at(&keys);
        for id in ["google", "antigravity", "google-oauth"] {
            assert!(!remaining_keys.contains_key(id));
            assert!(!read_sessions_at(&sessions).contains_key(id));
        }
        assert!(remaining_keys.contains_key("openai"));
        let _ = std::fs::remove_dir_all(&dir);
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
    fn malformed_credential_stores_are_never_overwritten() {
        let dir = std::env::temp_dir().join(format!("nur_bad_auth_store_{}", now_unix()));
        std::fs::create_dir_all(&dir).unwrap();
        let keys = dir.join("provider_keys.json");
        let sessions = dir.join("provider_sessions.json");
        std::fs::write(&keys, b"{not-json").unwrap();
        std::fs::write(&sessions, b"{also-not-json").unwrap();

        assert!(save_key_at(&keys, "openai", "sk-valid-key-value").is_err());
        assert!(!forget_provider_at(&keys, &sessions, "openai"));
        assert_eq!(std::fs::read(&keys).unwrap(), b"{not-json");
        assert_eq!(std::fs::read(&sessions).unwrap(), b"{also-not-json");
        let auth = oauth_auth("openai", "oauth-token-value", None, None, None).unwrap();
        assert!(save_provider_session_at(&sessions, &auth).is_err());
        assert_eq!(std::fs::read(&sessions).unwrap(), b"{also-not-json");
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

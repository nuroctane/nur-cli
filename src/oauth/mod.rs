//! Browser / device-code / SSO login for selected providers.
//!
//! UX mirrors the industry pattern (Google Cloud, Azure, GitHub, Grok, Claude):
//! open a browser (or print a URL + short code), user approves, CLI stores tokens.

mod browser;
mod flows;
mod harness;
pub mod omp_bridge;

use crate::auth::{Auth, AuthMethod};
use crate::error::{NurError, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub use browser::open_browser;
/// Crate-internal CSPRNG for credential-shaped strings (see [`flows::random_urlsafe`]).
pub(crate) use flows::random_urlsafe;
pub use flows::{import_existing_session, login_browser, BrowserLoginProgress, OAuthTokens};
pub use harness::is_api_key_import;

/// Whether a login/import result should be persisted as an OAuth session.
///
/// OMP and first-party CLI imports may be ordinary API keys (DeepSeek Harness,
/// Qwen settings, a Z.AI general key). Those must not take an OAuth-only route.
pub fn imported_as_oauth_session(tokens: &OAuthTokens) -> bool {
    if is_api_key_import(tokens) {
        return false;
    }
    !omp_bridge::is_omp_import(tokens) || omp_bridge::is_omp_oauth_import(tokens)
}

/// Kimi managed-API device headers bound to the current OAuth device identity.
pub fn kimi_request_headers() -> Result<Vec<(&'static str, String)>> {
    flows::kimi::request_headers()
}

/// Resolve the Cloud Code (`cloudcode-pa`) project id for a Google/Antigravity
/// OAuth access token via full Code Assist setup (load + free-tier onboard).
/// Used by the API layer when a Gemini Cloud Code request has no stored
/// `project_id` on its session, and on 403 re-onboard retries.
pub fn antigravity_resolve_project_id(access_token: &str) -> Result<String> {
    flows::antigravity::resolve_project_id(access_token)
}

/// Force Code Assist re-onboard even when `currentTier` already exists.
/// Used once on Cloud Code Private API 403 recovery.
pub fn antigravity_setup_code_assist_force(
    access_token: &str,
    env_project: Option<&str>,
) -> Result<(String, String)> {
    let s = flows::antigravity::setup_code_assist_force(access_token, env_project)?;
    Ok((s.project_id, s.tier_id))
}

/// Run a blocking operation (network call, subprocess spawn, credential-store
/// read, …) without stalling the async runtime.
///
/// A bare `std::thread::spawn(..).join()` from inside a Tokio task blocks the
/// calling worker thread *without telling Tokio's scheduler a blocking op is
/// in flight* — unlike `block_in_place`, it does not get a replacement worker
/// thread. Under concurrent load (a turn plus several subagents all resolving
/// credentials at once) that can starve every worker thread simultaneously,
/// hanging the whole process. Prefer this helper for anything that isn't a
/// plain non-blocking `.await`.
pub fn run_blocking<T: Send>(operation: impl FnOnce() -> T + Send) -> T {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(operation)
        }
        Ok(_) => std::thread::scope(|scope| {
            scope
                .spawn(operation)
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
        }),
        Err(_) => operation(),
    }
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn expires_in_to_at(expires_in: Option<u64>) -> Option<u64> {
    expires_in.map(|s| now_unix().saturating_add(s))
}

/// Device-code poll sleep: honor server `interval`, stretch on `slow_down`, add jitter.
pub fn device_poll_sleep(base_interval_secs: u64, slow_down: bool, attempt: u32) -> Duration {
    let mut secs = base_interval_secs.max(3);
    if slow_down {
        // Never shorten the interval requested by the authorization server.
        secs = secs.saturating_add(5);
    }
    // 0–500ms jitter from attempt (no extra RNG dependency).
    let jitter_ms = ((attempt.wrapping_mul(37) + 11) % 501) as u64;
    Duration::from_millis(secs.saturating_mul(1000).saturating_add(jitter_ms))
}

/// Per-flow polling state: slow_down applies to every subsequent request.
pub(crate) struct DevicePoll {
    interval: u64,
    attempt: u32,
}

impl DevicePoll {
    pub fn new(interval: u64) -> Self {
        Self {
            interval: interval.max(3),
            attempt: 0,
        }
    }

    pub fn slow_down(&mut self) {
        self.interval = self.interval.saturating_add(5);
    }

    pub fn wait(&mut self, cancel: &CancelFlag, deadline: std::time::Instant) -> Result<()> {
        let delay = device_poll_sleep(self.interval, false, self.attempt);
        self.attempt = self.attempt.saturating_add(1);
        let started = std::time::Instant::now();
        loop {
            if cancel.is_cancelled() {
                return Err(NurError::Other("login cancelled".into()));
            }
            let now = std::time::Instant::now();
            if now >= deadline {
                return Err(NurError::Other(
                    "device login expired; start sign-in again".into(),
                ));
            }
            let remaining = delay.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Ok(());
            }
            std::thread::sleep(
                remaining
                    .min(deadline - now)
                    .min(Duration::from_millis(100)),
            );
        }
    }
}

/// Best-effort remote revoke. Returns a human note (may be empty).
pub fn revoke_session(auth: &Auth) -> Result<String> {
    if !matches!(auth.auth_method, AuthMethod::Oauth) {
        return Ok("local API key only — nothing to revoke remotely".into());
    }
    match auth.provider.as_str() {
        "azure" => Ok(
            "Azure session is managed by `az`; run `az logout` to revoke the CLI session.".into(),
        ),
        "bedrock" => Ok(
            "AWS SSO session is managed by the AWS CLI; run `aws sso logout` if configured."
                .into(),
        ),
        "google" | "antigravity" => Ok(
            "Google session is managed by `gcloud`; run `gcloud auth revoke` to drop ADC tokens."
                .into(),
        ),
        "openai" | "xai" | "kimi" | "anthropic" | "huggingface" => Ok(format!(
            "no remote revoke endpoint wired for '{}' — local tokens deleted; revoke in the vendor account UI if needed",
            auth.provider
        )),
        "commandcode" => Ok(
            "local credential deleted — revoke the key in Command Code Studio (commandcode.ai/studio → API keys) if needed"
                .into(),
        ),
        "cline" => Ok(
            "local credential deleted — revoke it from the Cline account (app.cline.bot → Settings → API Keys, or `cline auth` to sign out) if needed"
                .into(),
        ),
        "" => Ok(String::new()),
        other => Ok(format!(
            "no remote revoke for provider '{other}' — local file removed"
        )),
    }
}

/// Cancel handle shared between TUI and background OAuth task.
#[derive(Clone, Default)]
pub struct CancelFlag(Arc<AtomicBool>, Arc<std::sync::Mutex<Option<String>>>);

impl CancelFlag {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        let mut code = self
            .1
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.0.store(true, Ordering::SeqCst);
        *code = None;
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// A paste belongs only to this login attempt, never to another process or
    /// a later retry. Codes remain in memory instead of a shared disk mailbox.
    pub fn submit_manual_code(&self, text: &str) -> Result<()> {
        let code = crate::auth::validate_manual_oauth_code(text)?;
        let mut slot = self
            .1
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.is_cancelled() {
            return Err(NurError::Other("login cancelled".into()));
        }
        *slot = Some(code);
        Ok(())
    }

    pub(crate) fn take_manual_code(&self) -> Option<String> {
        self.1
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }
}

/// Refresh an OAuth access token for the given provider.
pub fn refresh_tokens(provider: &str, auth: &Auth, refresh: &str) -> Result<OAuthTokens> {
    match provider {
        "openai" => flows::openai::refresh(auth, refresh),
        "xai" => flows::xai::refresh(auth, refresh),
        "kimi" => flows::kimi::refresh(auth, refresh),
        "anthropic" => flows::claude::refresh(refresh),
        "google" | "google-oauth" => flows::google::refresh(auth, refresh),
        "antigravity" => flows::antigravity::refresh(auth, refresh),
        "huggingface" => flows::huggingface::refresh(refresh),
        "azure" => flows::azure::refresh(),
        "bedrock" => flows::bedrock::refresh(),
        "github-models" | "github-copilot" => flows::github::refresh(auth, refresh),
        "cursor" => flows::cursor::refresh(auth, refresh),
        "opencode" => flows::opencode::refresh(auth, refresh),
        "nous" => flows::nous::refresh(auth, refresh),
        "commandcode" => flows::commandcode::refresh(auth, refresh),
        "cline" => flows::cline::refresh(auth, refresh),
        "meta" => harness::muse::refresh(auth, refresh),
        "deepseek" => harness::deepseek::refresh(auth, refresh),
        "zhipu" => harness::zhipu::refresh(auth, refresh),
        _ => Err(NurError::Other(format!(
            "no OAuth refresh path for provider '{provider}'"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_codes_are_attempt_scoped_and_cleared_on_cancel() {
        let first = CancelFlag::new();
        let second = CancelFlag::new();
        first.submit_manual_code(" code#state ").unwrap();
        assert!(second.take_manual_code().is_none());
        assert_eq!(
            first.clone().take_manual_code().as_deref(),
            Some("code#state")
        );
        assert!(first.take_manual_code().is_none());
        first.submit_manual_code("another-code").unwrap();
        first.cancel();
        assert!(first.take_manual_code().is_none());
        assert!(first.submit_manual_code("late-code").is_err());
        assert!(second.submit_manual_code("\n").is_err());
        assert!(second.submit_manual_code("a\nb").is_err());
    }

    #[test]
    fn device_poll_backoff_is_cumulative_and_never_shortens_server_interval() {
        let mut poll = DevicePoll::new(45);
        poll.slow_down();
        assert_eq!(poll.interval, 50);
        poll.slow_down();
        assert_eq!(poll.interval, 55);
        assert!(device_poll_sleep(poll.interval, false, 1).as_secs() >= 55);
        assert!(device_poll_sleep(60, true, 1).as_secs() >= 65);
    }

    #[test]
    fn device_poll_cancel_and_expiry_prevent_another_request() {
        let cancel = CancelFlag::new();
        cancel.cancel();
        let now = std::time::Instant::now();
        assert!(DevicePoll::new(60)
            .wait(&cancel, now + Duration::from_secs(120))
            .is_err());
        assert!(now.elapsed() < Duration::from_secs(1));
        assert!(DevicePoll::new(60).wait(&CancelFlag::new(), now).is_err());
    }

    #[test]
    fn device_poll_respects_slow_down() {
        let normal = device_poll_sleep(5, false, 0);
        let slow = device_poll_sleep(5, true, 0);
        assert!(slow > normal);
        assert!(normal.as_secs() >= 5);
        assert!(slow.as_secs() >= 10);
        assert!(slow.as_secs() <= 30);
    }

    /// Every catalog browser_auth provider must have a `login_browser` arm
    /// and a `refresh_tokens` arm — otherwise `/login` browser or mid-session
    /// refresh fails silently for that vendor.
    #[test]
    fn every_browser_auth_provider_has_login_and_refresh_path() {
        // Mirrors match arms in flows::login_browser and refresh_tokens.
        const LOGIN: &[&str] = &[
            "openai",
            "xai",
            "kimi",
            "anthropic",
            "google",
            "antigravity",
            "azure",
            "github-models",
            "github-copilot",
            "cursor",
            "opencode",
            "nous",
            "commandcode",
            "cline",
            "meta",
            "deepseek",
            "zhipu",
        ];
        const REFRESH: &[&str] = &[
            "openai",
            "xai",
            "kimi",
            "anthropic",
            "google",
            "antigravity",
            "google-oauth", // alias used by some stored sessions
            "huggingface",
            "azure",
            "bedrock",
            "github-models",
            "github-copilot",
            "cursor",
            "opencode",
            "nous",
            "commandcode",
            "cline",
            "meta",
            "deepseek",
            "zhipu",
        ];
        let browser_auth = |id: &str| crate::providers::by_id(id).is_some_and(|p| p.browser_auth);
        for id in crate::providers::oauth_browser_provider_ids() {
            assert!(
                LOGIN.contains(id),
                "provider '{id}' is browser_auth but missing from login_browser match"
            );
            assert!(
                REFRESH.contains(id)
                    || (*id == "antigravity" && REFRESH.contains(&"google-oauth"))
                    || (*id == "google" && REFRESH.contains(&"google-oauth")),
                "provider '{id}' is browser_auth but missing from refresh_tokens match"
            );
            assert!(browser_auth(id), "catalog browser_auth({id}) is false");
        }
        // Inverse: every LOGIN id must be browser_auth in the catalog.
        for id in LOGIN {
            assert!(
                browser_auth(id),
                "login_browser has '{id}' but catalog browser_auth=false"
            );
        }
    }

    #[test]
    fn api_key_imports_are_not_treated_as_oauth_sessions() {
        let key = OAuthTokens {
            access_token: "sk-deepseek-harness-key-123456".into(),
            refresh_token: Some("deepseek-cli".into()),
            expires_at: None,
            meta: Some(crate::auth::OauthMeta {
                issuer: "deepseek".into(),
                client_id: "deepseek-cli".into(),
                extra: serde_json::json!({"credential_kind": "api_key"}),
            }),
        };
        assert!(is_api_key_import(&key));
        assert!(!imported_as_oauth_session(&key));

        let oauth = OAuthTokens {
            access_token: "muse-access-token-value-12345".into(),
            refresh_token: Some("muse-refresh".into()),
            expires_at: None,
            meta: Some(crate::auth::OauthMeta {
                issuer: "muse".into(),
                client_id: "muse-code".into(),
                extra: serde_json::json!({"credential_kind": "oauth"}),
            }),
        };
        assert!(!is_api_key_import(&oauth));
        assert!(imported_as_oauth_session(&oauth));
    }

    #[test]
    fn refresh_unknown_provider_errors_clearly() {
        let auth = crate::auth::Auth {
            api_key: "x".into(),
            source: "oauth".into(),
            auth_method: crate::auth::AuthMethod::Oauth,
            provider: "not-a-real-provider".into(),
            expires_at: None,
            refresh_token: Some("r".into()),
            oauth_meta: None,
        };
        let err = refresh_tokens("not-a-real-provider", &auth, "r").unwrap_err();
        assert!(
            err.to_string().contains("no OAuth refresh"),
            "unexpected: {err}"
        );
    }
}

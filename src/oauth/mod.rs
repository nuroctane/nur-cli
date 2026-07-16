//! Browser / device-code / SSO login for selected providers.
//!
//! UX mirrors the industry pattern (Hugging Face, Azure CLI, AWS SSO, Grok, Claude):
//! open a browser (or print a URL + short code), user approves, CLI stores tokens.

mod browser;
mod flows;

use crate::auth::{Auth, AuthMethod};
use crate::error::{MuseError, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub use browser::open_browser;
pub use flows::{import_existing_session, login_browser, BrowserLoginProgress, OAuthTokens};

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
        // RFC 8628: increase interval on slow_down (we add 5s, cap 30s).
        secs = (secs.saturating_add(5)).min(30);
    }
    // 0–500ms jitter from attempt (no extra RNG dependency).
    let jitter_ms = ((attempt.wrapping_mul(37) + 11) % 501) as u64;
    Duration::from_millis(secs.saturating_mul(1000).saturating_add(jitter_ms))
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
        "antigravity" => Ok(
            "Google session is managed by `gcloud`; run `gcloud auth revoke` to drop ADC tokens."
                .into(),
        ),
        "xai" | "anthropic" | "huggingface" => Ok(format!(
            "no remote revoke endpoint wired for '{}' — local tokens deleted; revoke in the vendor account UI if needed",
            auth.provider
        )),
        other if other.is_empty() => Ok(String::new()),
        other => Ok(format!(
            "no remote revoke for provider '{other}' — local file removed"
        )),
    }
}

/// Cancel handle shared between TUI and background OAuth task.
#[derive(Clone, Default)]
pub struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Refresh an OAuth access token for the given provider.
pub fn refresh_tokens(provider: &str, auth: &Auth, refresh: &str) -> Result<OAuthTokens> {
    match provider {
        "xai" => flows::xai::refresh(auth, refresh),
        "anthropic" => flows::claude::refresh(refresh),
        "antigravity" | "google-oauth" => flows::antigravity::refresh(auth, refresh),
        "huggingface" => flows::huggingface::refresh(refresh),
        "azure" => flows::azure::refresh(),
        "bedrock" => flows::bedrock::refresh(),
        "github-models" => flows::github::refresh(auth, refresh),
        _ => Err(MuseError::Other(format!(
            "no OAuth refresh path for provider '{provider}'"
        ))),
    }
}

/// Whether this catalog id supports browser sign-in.
#[allow(dead_code)]
pub fn supports_browser(provider_id: &str) -> bool {
    crate::providers::by_id(provider_id)
        .map(|p| p.browser_auth)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_supported_ids() {
        assert!(supports_browser("xai"));
        assert!(supports_browser("anthropic"));
        assert!(!supports_browser("meta"));
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
            "xai",
            "anthropic",
            "antigravity",
            "huggingface",
            "azure",
            "bedrock",
            "github-models",
        ];
        const REFRESH: &[&str] = &[
            "xai",
            "anthropic",
            "antigravity",
            "google-oauth", // alias used by some stored sessions
            "huggingface",
            "azure",
            "bedrock",
            "github-models",
        ];
        for id in crate::providers::oauth_browser_provider_ids() {
            assert!(
                LOGIN.contains(id),
                "provider '{id}' is browser_auth but missing from login_browser match"
            );
            assert!(
                REFRESH.contains(id) || (*id == "antigravity" && REFRESH.contains(&"google-oauth")),
                "provider '{id}' is browser_auth but missing from refresh_tokens match"
            );
            assert!(supports_browser(id), "supports_browser({id}) false");
        }
        // Inverse: every LOGIN id must be browser_auth in the catalog.
        for id in LOGIN {
            assert!(
                supports_browser(id),
                "login_browser has '{id}' but catalog browser_auth=false"
            );
        }
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

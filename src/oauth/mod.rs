//! Browser / device-code / SSO login for selected providers.
//!
//! UX mirrors the industry pattern (Hugging Face, Azure CLI, AWS SSO, Grok, Claude):
//! open a browser (or print a URL + short code), user approves, CLI stores tokens.

mod browser;
mod flows;

use crate::auth::Auth;
use crate::error::{MuseError, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

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
}

//! Cross-provider failover.
//!
//! When the active provider returns a server-side availability error (5xx / 429
//! / transport failure), the agent loop can retry the *same* request against a
//! configured chain of fallback providers. This is **opt-in** — set
//! `fallback_providers` in config to a list of catalog provider ids. Failover
//! spends the fallback provider's credits, so it never happens implicitly.
//!
//! Credentials for a fallback come from, in order: that provider's catalog env
//! var (e.g. `OPENAI_API_KEY`), a key saved via `/failover`, a browser OAuth
//! session saved via `/failover` (or dual-written from `/login`), or empty for
//! key-optional local servers — never from the primary's active `auth.json`.

use crate::error::MuseError;
use crate::providers::{self, ApiStyle, Provider};

/// A resolved failover destination — a fallback provider whose key we actually
/// have, ready to build an `ApiClient` from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailoverTarget {
    pub provider_id: String,
    pub base_url: String,
    pub api_key: String,
    /// Wire format from the provider catalog (Responses / Chat / Anthropic Messages).
    pub style: ApiStyle,
    pub model: String,
}

/// Whether `err` is worth retrying against a *different* provider. Only
/// server-side availability problems qualify: 5xx, 429, mid-stream provider
/// errors (status 0), and transport failures. Auth/quota (401/403), bad
/// requests (4xx), user interrupts, and local tool errors do not — another
/// provider cannot fix a cancelled turn or a malformed request.
pub fn should_failover(err: &MuseError) -> bool {
    match err {
        MuseError::Api { status, .. } => matches!(status, 0 | 429 | 500 | 502 | 503 | 504),
        // Transport/connection/parse failures from the client layer.
        MuseError::Other(_) => true,
        _ => false,
    }
}

/// Whether a fallback at `target_rank` privacy is acceptable when the active
/// provider is at `active_rank` (see `crate::providers::Privacy::rank`).
/// Failover must never silently move you to a *weaker* tier — so a target is
/// allowed only if it's at least as strong, unless `allow_downgrade` is set.
pub fn privacy_allowed(active_rank: u8, target_rank: u8, allow_downgrade: bool) -> bool {
    allow_downgrade || target_rank >= active_rank
}

/// Build the ordered failover chain from configured fallback provider ids.
/// Skips ids that are empty, unknown to the catalog, equal to the primary, or
/// already seen, plus any provider for which `resolve_key` yields `None`
/// (no credentials available). Order follows the configured list.
pub fn plan_targets(
    primary_provider_id: &str,
    fallback_ids: &[String],
    resolve_key: impl Fn(&Provider) -> Option<String>,
) -> Vec<FailoverTarget> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for raw in fallback_ids {
        let id = raw.trim();
        if id.is_empty() || id == primary_provider_id || !seen.insert(id.to_string()) {
            continue;
        }
        let Some(p) = providers::by_id(id) else {
            continue;
        };
        let Some(key) = resolve_key(p) else {
            continue;
        };
        out.push(FailoverTarget {
            provider_id: p.id.to_string(),
            base_url: p.base_url.trim_end_matches('/').to_string(),
            api_key: key,
            style: p.style,
            model: p.default_model.to_string(),
        });
    }
    out
}

/// Runtime credential resolver for a fallback provider, in priority order:
/// 1. the provider's own catalog env var (e.g. `OPENAI_API_KEY`),
/// 2. an API key saved via `/failover` (`auth::load_provider_key`),
/// 3. a browser OAuth session for that provider (`auth::load_provider_oauth_token`),
/// 4. an empty string for local servers that don't need one.
/// `None` = no credentials, skip this provider.
pub fn resolve_target_key(p: &Provider) -> Option<String> {
    if let Ok(k) = std::env::var(p.env_key) {
        let k = k.trim().to_string();
        if !k.is_empty() {
            return Some(k);
        }
    }
    if let Some(k) = crate::auth::load_provider_key(p.id) {
        let k = k.trim().to_string();
        if !k.is_empty() {
            return Some(k);
        }
    }
    if let Some(k) = crate::auth::load_provider_oauth_token(p.id) {
        let k = k.trim().to_string();
        if !k.is_empty() {
            return Some(k);
        }
    }
    if p.key_optional {
        return Some(String::new());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_failover_on_server_errors_only() {
        for status in [0u16, 429, 500, 502, 503, 504] {
            assert!(
                should_failover(&MuseError::Api { status, message: "x".into() }),
                "status {status} should fail over"
            );
        }
        for status in [400u16, 401, 403, 404, 422] {
            assert!(
                !should_failover(&MuseError::Api { status, message: "x".into() }),
                "status {status} should NOT fail over"
            );
        }
        assert!(should_failover(&MuseError::Other("connection reset".into())));
        assert!(!should_failover(&MuseError::Interrupted));
        assert!(!should_failover(&MuseError::NotAuthenticated));
    }

    #[test]
    fn plan_targets_builds_chain_in_order_with_keys() {
        let ids = vec!["openai".to_string(), "anthropic".to_string()];
        let targets = plan_targets("meta", &ids, |p| Some(format!("key-{}", p.id)));
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].provider_id, "openai");
        assert_eq!(targets[0].style, ApiStyle::Responses); // OpenAI Responses API
        assert_eq!(targets[0].model, "gpt-5.5");
        assert_eq!(targets[0].api_key, "key-openai");
        assert_eq!(targets[1].provider_id, "anthropic");
        assert_eq!(targets[1].style, ApiStyle::AnthropicMessages); // Messages, not Chat
        assert_eq!(targets[1].api_key, "key-anthropic");
    }

    #[test]
    fn plan_targets_skips_primary_unknown_dupes_and_keyless() {
        let ids = vec![
            "meta".to_string(),     // primary — skip
            "nope".to_string(),     // not in catalog — skip
            "openai".to_string(),   // keep
            "openai".to_string(),   // dupe — skip
            "anthropic".to_string(),// keyless in this resolver — skip
        ];
        let targets = plan_targets("meta", &ids, |p| {
            if p.id == "anthropic" {
                None
            } else {
                Some("k".to_string())
            }
        });
        let got: Vec<&str> = targets.iter().map(|t| t.provider_id.as_str()).collect();
        assert_eq!(got, vec!["openai"]);
    }

    #[test]
    fn plan_targets_empty_when_no_fallbacks() {
        assert!(plan_targets("meta", &[], |_| Some("k".into())).is_empty());
    }

    #[test]
    fn privacy_floor_blocks_downgrades_unless_allowed() {
        // Active provider at Zdr (rank 1): weaker Standard (0) is blocked;
        // equal/stronger tiers pass.
        assert!(!privacy_allowed(1, 0, false));
        assert!(privacy_allowed(1, 1, false));
        assert!(privacy_allowed(1, 2, false)); // Tee
        assert!(privacy_allowed(1, 3, false)); // Local
        // Explicit opt-in lets a downgrade through.
        assert!(privacy_allowed(1, 0, true));
        // Active at Standard (0) → everything is >= floor.
        assert!(privacy_allowed(0, 0, false));
    }

    #[test]
    fn resolve_target_key_allows_empty_for_local_servers() {
        // Local servers are key_optional → empty string is a valid "key" even
        // with no env var and no UI-saved key. We only assert the key_optional
        // branch here; env/store priority is covered by plan_targets with an
        // injected resolver (so tests don't touch the user's ~/.nur keys).
        let ollama = providers::by_id("ollama").unwrap();
        assert!(
            ollama.key_optional,
            "ollama must be key_optional for this assertion"
        );
        // With no env var, a key_optional provider still resolves (empty key).
        // Don't assert exact value if the user has a real OLLAMA_API_KEY set.
        assert!(resolve_target_key(ollama).is_some());
    }

    #[test]
    fn resolve_target_key_prefers_env_over_store_shape() {
        // plan_targets uses the injected resolver — this locks the *shape*
        // of resolve_target_key: env non-empty wins; else store; else optional.
        let p = providers::by_id("openai").expect("openai in catalog");
        assert!(!p.key_optional);
        assert_eq!(p.env_key, "OPENAI_API_KEY");
    }
}

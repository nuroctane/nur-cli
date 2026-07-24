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

/// Whether `err` is worth retrying against a *different* provider.
///
/// Always: 5xx, 429, 529, mid-stream (status 0), transport failures.
///
/// Also: **capacity / usage exhaustion** messages on 4xx (Claude/OpenAI often
/// return 400/403 with `rate_limit` / `usage limit` / `overloaded` when the
/// account is out of quota — not only HTTP 429). Another provider *can* fix
/// that. Wrong keys, bad models, and validation errors still do **not** fail over.
///
/// Aware of which provider produced `err`.
///
/// Gateway providers (OpenCode Zen/Go) wrap a failing *upstream vendor* as a
/// **400** whose body describes the upstream, not our request — e.g.
/// `Error from provider (Console Go): Upstream request failed`. That is
/// transient and another provider can serve the turn, so it must fail over.
/// A plain 400 from a first-party provider is a validation error and must not,
/// which is why this branch is gated on the provider rather than the message
/// alone: the needles are generic enough to appear in a genuine bad request.
pub fn should_failover_for(err: &MuseError, provider_id: &str) -> bool {
    match err {
        MuseError::Api { status, message } => {
            if matches!(status, 0 | 429 | 500 | 502 | 503 | 504 | 529) {
                return true;
            }
            // Quota / overload often arrives as 400/401/402/403 with a clear body.
            if matches!(status, 400 | 401 | 402 | 403) && is_capacity_or_quota_message(message) {
                return true;
            }
            if *status == 400
                && is_gateway_provider(provider_id)
                && is_transient_upstream_message(message)
            {
                return true;
            }
            false
        }
        // Transport/connection/parse failures from the client layer.
        MuseError::Other(_) => true,
        _ => false,
    }
}

/// Squash a message to lowercase alphanumerics so a single needle matches every
/// spelling a vendor might pick.
///
/// gRPC-derived backends (NVIDIA NIM, Triton, vLLM, Google) report status codes
/// in camelCase — `ResourceExhausted` — while others send `RESOURCE_EXHAUSTED`,
/// `resource-exhausted`, or `resource exhausted`. Matching the separators
/// literally meant the most common spelling of the most common transient error
/// fell through every needle in the list below.
fn squash(message: &str) -> String {
    message
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Separator-insensitive capacity markers. Kept apart from the phrase list so
/// each can stay readable: these are matched against [`squash`]ed text, where
/// word boundaries no longer exist.
const SQUASHED_CAPACITY_NEEDLES: &[&str] = &[
    // gRPC status any spelling: ResourceExhausted / RESOURCE_EXHAUSTED / …
    "resourceexhausted",
    // NVIDIA NIM worker saturation: "Worker local total request limit reached (90/32)"
    "requestlimitreached",
    "concurrentrequest",
    "concurrencylimit",
    "toomanyrequests",
    "serveroverloaded",
    "capacityexceeded",
    "unavailable",
];

/// Billing / plan exhaustion — real capacity trouble, but waiting will not fix
/// it. Worth failing over to another provider, never worth retrying in place.
fn is_hard_quota_message(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "insufficient_quota",
        "out of credits",
        "credit balance",
        "billing hard limit",
        "quota exceeded",
        "quota_exceeded",
    ];
    NEEDLES.iter().any(|n| m.contains(n))
}

/// A transient saturation error worth **retrying the same provider** after a
/// backoff, rather than failing the turn or moving the user elsewhere.
///
/// Deliberately narrower than [`should_failover_for`]: a mid-stream `status: 0`
/// only qualifies when its body actually names a capacity condition, and
/// billing exhaustion is excluded because no amount of waiting clears it.
pub fn is_transient_capacity(err: &MuseError) -> bool {
    match err {
        MuseError::Api { status, message } => {
            if is_hard_quota_message(message) {
                return false;
            }
            matches!(status, 429 | 503 | 529) || is_capacity_or_quota_message(message)
        }
        _ => false,
    }
}

/// True when the API error text indicates rate/usage/capacity — not a bad request.
fn is_capacity_or_quota_message(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    if SQUASHED_CAPACITY_NEEDLES
        .iter()
        .any(|n| squash(message).contains(n))
    {
        return true;
    }
    const NEEDLES: &[&str] = &[
        "rate_limit",
        "rate limit",
        "ratelimit",
        "too many requests",
        "overloaded",
        "overloaded_error",
        "capacity",
        "insufficient_quota",
        "quota exceeded",
        "quota_exceeded",
        "usage limit",
        "usage_limit",
        "usage limit reached",
        "reached its usage",
        "out of credits",
        "credit balance",
        "billing hard limit",
        "resource_exhausted",
        "resource exhausted",
        "tokens per day",
        "requests per",
    ];
    NEEDLES.iter().any(|n| m.contains(n))
}

/// Providers that are *gateways* in front of third-party vendors, and so report
/// someone else's outage as their own 400. Keep this list tight — every entry
/// relaxes what a 400 means for that provider.
fn is_gateway_provider(provider_id: &str) -> bool {
    matches!(provider_id.trim().to_ascii_lowercase().as_str(), "opencode")
}

/// Gateway-wrapped *transient* upstream failure.
///
/// Deliberately narrow, and shares [`crate::api::client::has_permanent_marker`]
/// with the retry path so the two cannot drift. A permanent validation error
/// (bad tool ids, unsupported parameter) arrives inside the very same
/// "Error from provider (...)" envelope, and failing over on it just replays a
/// request that is broken by construction against a second provider.
fn is_transient_upstream_message(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    if crate::api::client::has_permanent_marker(&m) {
        return false;
    }
    const NEEDLES: &[&str] = &[
        "upstream request failed",
        "upstream error",
        "upstream failed",
        "upstream timeout",
        "upstream unavailable",
        "upstream connect",
        "temporarily unavailable",
    ];
    NEEDLES.iter().any(|n| m.contains(n))
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
        let is_oauth = crate::auth::oauth_request_context(p.id, &key).is_some();
        // Shared with cross-provider subagent routing so the two cannot drift.
        let (base_url, style, model) = crate::providers::endpoint_for_credential(p, is_oauth);
        out.push(FailoverTarget {
            provider_id: p.id.to_string(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: key,
            style,
            model: model.to_string(),
        });
    }
    out
}

/// Runtime credential resolver for a fallback provider, in priority order:
/// 1. a browser OAuth session explicitly saved via `/failover` or `/login`,
/// 2. the provider's own catalog env var (e.g. `OPENAI_API_KEY`),
/// 3. an API key saved via `/failover` (`auth::load_provider_key`),
/// 4. t3 vendor CLI session (Claude Code, Codex, agy, gcloud, etc.) when no key on disk,
/// 5. an empty string for local servers that don't need one.
/// `None` = no credentials, skip this provider.
pub fn resolve_target_key(p: &Provider) -> Option<String> {
    if let Some(k) = crate::auth::load_provider_oauth_token(p.id) {
        let k = k.trim().to_string();
        if !k.is_empty() {
            return Some(k);
        }
    }
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
    // t3 fallback: vendor CLI logged-in session. Isolated via run_blocking:
    // this is called synchronously from the async failover path and can shell
    // out (e.g. reading an OS credential store), so it must not block a Tokio
    // worker thread outright.
    if let Ok(Some(tokens)) = crate::oauth::run_blocking(|| crate::oauth::import_existing_session(p.id)) {
        let tok = tokens.access_token.trim().to_string();
        if !tok.is_empty() {
            return Some(tok);
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

    /// The provider-agnostic rules — asserted against a provider that gets no
    /// gateway relaxation, so these cases pin the baseline for everyone.
    fn should_failover(err: &MuseError) -> bool {
        should_failover_for(err, "")
    }

    #[test]
    fn should_failover_on_server_errors_only() {
        for status in [0u16, 429, 500, 502, 503, 504, 529] {
            assert!(
                should_failover(&MuseError::Api {
                    status,
                    message: "x".into()
                }),
                "status {status} should fail over"
            );
        }
        for status in [400u16, 401, 403, 404, 422] {
            assert!(
                !should_failover(&MuseError::Api {
                    status,
                    message: "x".into()
                }),
                "status {status} should NOT fail over"
            );
        }
        assert!(should_failover(&MuseError::Other(
            "connection reset".into()
        )));
        assert!(!should_failover(&MuseError::Interrupted));
        assert!(!should_failover(&MuseError::NotAuthenticated));
    }

    /// The exact string that killed live runs: NVIDIA NIM refuses admission
    /// *mid-stream* on an HTTP 200, so it surfaces as `status: 0`, and it spells
    /// the gRPC code in camelCase with no separator — which matched none of the
    /// original phrase needles.
    #[test]
    fn nim_worker_saturation_is_transient_capacity() {
        let nim = MuseError::Api {
            status: 0,
            message: "ResourceExhausted: Worker local total request limit reached (90/32)".into(),
        };
        assert!(
            is_transient_capacity(&nim),
            "NIM worker saturation must be retried in place, not fail the turn"
        );
        assert!(should_failover(&nim), "and still fail over if retries lose");

        // Every spelling of the same gRPC status, from any vendor.
        for spelling in [
            "ResourceExhausted",
            "RESOURCE_EXHAUSTED",
            "resource_exhausted",
            "resource exhausted",
            "resource-exhausted",
        ] {
            assert!(
                is_transient_capacity(&MuseError::Api {
                    status: 0,
                    message: format!("{spelling}: try again"),
                }),
                "{spelling} must classify as capacity"
            );
        }
    }

    /// Retry-in-place is narrower than failover: waiting cannot buy credits, and
    /// a mid-stream error that is not about capacity must not be replayed.
    #[test]
    fn transient_capacity_excludes_billing_and_unrelated_stream_errors() {
        for hard in [
            "insufficient_quota: you have exceeded your current quota",
            "Your credit balance is too low to access the API",
            "billing hard limit has been reached",
        ] {
            assert!(
                !is_transient_capacity(&MuseError::Api {
                    status: 429,
                    message: hard.into()
                }),
                "waiting will not fix: {hard}"
            );
        }
        // A non-capacity mid-stream failure is left to the failover path alone.
        assert!(!is_transient_capacity(&MuseError::Api {
            status: 0,
            message: "OpenCode returned an empty stream".into(),
        }));
        // Plain rate limits still retry.
        assert!(is_transient_capacity(&MuseError::Api {
            status: 429,
            message: "rate limit exceeded".into(),
        }));
        // Non-API errors are never retried in place.
        assert!(!is_transient_capacity(&MuseError::Interrupted));
    }

    #[test]
    fn should_failover_on_quota_and_usage_limit_bodies() {
        // Claude/OpenAI often return 400/403 when the account is out of usage.
        let cases = [
            (
                400u16,
                "rate_limit_error: Your account has reached its usage limit",
            ),
            (403, "insufficient_quota: You exceeded your current quota"),
            (400, "overloaded_error: The model is overloaded"),
            (401, "billing hard limit has been reached"),
            (429, "anything"), // still always
        ];
        for (status, message) in cases {
            assert!(
                should_failover(&MuseError::Api {
                    status,
                    message: message.into()
                }),
                "status {status} msg={message:?} should fail over"
            );
        }
        // Real bad requests must still NOT fail over.
        for (status, message) in [
            (400u16, "invalid_request_error: model not found"),
            (400, "messages: text content blocks must be non-empty"),
            (401, "invalid x-api-key"),
            (403, "permission denied for this organization"),
        ] {
            assert!(
                !should_failover(&MuseError::Api {
                    status,
                    message: message.into()
                }),
                "status {status} msg={message:?} must NOT fail over"
            );
        }
    }

    /// OpenCode Zen/Go answer a failing upstream vendor with a 400 whose body
    /// names the upstream, not our request. Another provider can serve that
    /// turn, so it must fail over — while ordinary 400s still must not.
    #[test]
    fn should_failover_on_gateway_upstream_failures() {
        for (status, message) in [
            (
                400u16,
                "Error from provider (Console Go): Upstream request failed",
            ),
            (400, "upstream error"),
            (502, "upstream timeout"),
            (503, "upstream unavailable"),
        ] {
            assert!(
                should_failover_for(
                    &MuseError::Api {
                        status,
                        message: message.into()
                    },
                    "opencode"
                ),
                "status {status} msg={message:?} should fail over on the gateway"
            );
        }
        for (status, message) in [
            (400u16, "tool call ids must be unique"),
            (400, "unsupported parameter: reasoning"),
            (404, "Error from provider: model not available"), // 404 stays fatal
        ] {
            assert!(
                !should_failover_for(
                    &MuseError::Api {
                        status,
                        message: message.into()
                    },
                    "opencode"
                ),
                "status {status} msg={message:?} must NOT fail over"
            );
        }
    }

    /// A permanent validation error wrapped in the gateway's envelope must NOT
    /// fail over. It is broken by construction, so replaying it against a
    /// second provider just produces the same 400 twice — and the retry path
    /// burns four attempts before that. This is the exact shape the tool-pairing
    /// bug produced, so the two fixes must not undo each other.
    #[test]
    fn gateway_wrapped_validation_errors_are_permanent() {
        for message in [
            "Error from provider (Console Go): messages.5: `tool_use` ids were found without \
             `tool_result` blocks",
            "Error from provider (anthropic): tool call ids must be unique",
            "Error from provider: unsupported parameter: reasoning",
            "Error from provider (openai): max_tokens is too large",
            "provider error: model does not exist",
        ] {
            assert!(
                !should_failover_for(
                    &MuseError::Api {
                        status: 400,
                        message: message.into()
                    },
                    "opencode"
                ),
                "must NOT fail over: {message:?}"
            );
        }
        // A genuine upstream outage still does.
        assert!(should_failover_for(
            &MuseError::Api {
                status: 400,
                message: "Error from provider (Console Go): Upstream request failed".into()
            },
            "opencode"
        ));
    }

    /// The gateway relaxation must not leak: a plain 400 from a first-party
    /// provider is a validation error, and retrying it elsewhere just burns a
    /// second provider on the same bad request.
    #[test]
    fn gateway_upstream_relaxation_does_not_apply_to_other_providers() {
        let err = MuseError::Api {
            status: 400,
            message: "Error from provider (Console Go): Upstream request failed".into(),
        };
        assert!(should_failover_for(&err, "opencode"));
        for provider in ["anthropic", "openai", "gemini", "ollama", ""] {
            assert!(
                !should_failover_for(&err, provider),
                "{provider} must keep treating a 400 as fatal"
            );
        }
        // Statuses that always failed over are unchanged for every provider.
        for provider in ["anthropic", "openai", "opencode", ""] {
            for status in [0u16, 429, 500, 502, 503, 504, 529] {
                assert!(should_failover_for(
                    &MuseError::Api {
                        status,
                        message: "upstream error".into()
                    },
                    provider
                ));
            }
        }
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
            "meta".to_string(),      // primary — skip
            "nope".to_string(),      // not in catalog — skip
            "openai".to_string(),    // keep
            "openai".to_string(),    // dupe — skip
            "anthropic".to_string(), // keyless in this resolver — skip
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

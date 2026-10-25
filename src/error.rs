use thiserror::Error;

/// Describe an API failure whose body carried no message.
///
/// Some endpoints answer with a status and nothing else — Poolside's platform
/// returns a bodyless 403 for a bad key — which rendered as
/// `API error (403): ` and told the user nothing. Anything that *did* parse a
/// message is passed straight through unchanged, except a recognized
/// provider-side gate (OpenCode free tier), which gets actionable guidance
/// appended.
fn api_message(status: u16, message: &str) -> String {
    if let Some(guided) = opencode_free_tier_guidance(message) {
        return guided;
    }
    if let Some(guided) = expired_key_guidance(message) {
        return guided;
    }
    if !message.trim().is_empty() {
        return message.to_string();
    }
    match status {
        // Mid-stream failure on an otherwise-successful response: there is no
        // HTTP status to report because the request itself returned 200.
        0 => "the provider ended the stream without a reason. Retry, or /model to another route.".into(),
        401 | 403 => "no details returned - the key was rejected. Check it with /login (or the provider's dashboard); an expired or wrong-scope key looks like this.".into(),
        404 => "no details returned - endpoint or model not found. Check the base URL and model id with /model.".into(),
        429 => "no details returned - rate limited. Wait and retry, or switch model/provider.".into(),
        s if s >= 500 => "no details returned - the provider failed on its side. Retry shortly.".into(),
        _ => "no details returned by the provider".into(),
    }
}

/// OpenCode Zen answers third-party clients on free-tier models with a 403
/// along the lines of "OpenCode's free tier can only be used from within
/// OpenCode". The gate is enforced server-side by OpenCode (it is not a nur
/// bug, a bad key, or a missing header), so the raw message alone leaves the
/// user with no next step. Append the two ways out: a non-free model on the
/// same provider, or another provider entirely.
fn opencode_free_tier_guidance(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    // Two wordings so far: the original 403 ("free tier can only be used from
    // within OpenCode") and the 429 FreeUsageLimitError ("Free usage limit
    // reached. Free models work in OpenCode").
    let gated = (lower.contains("free tier") && lower.contains("within opencode"))
        || (lower.contains("free usage limit") && lower.contains("opencode"));
    if !gated {
        return None;
    }
    Some(format!(
        "{message}\n\
         OpenCode gates its free tier to its own first-party client, so free \
         models (*-free, big-pickle, ox-alpha-free) fail from nur with this 403 \
         no matter the key or headers. To keep working: switch to a non-free \
         model on the same provider (/model), or switch provider (/model or \
         `nur auth login --provider <other>`)."
    ))
}
/// A provider that expires API keys (OpenCode Console keys do; signing in
/// again does not renew one) says so in its message, which alone does not
/// tell the user where the new key goes in nur.
fn expired_key_guidance(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    if !(lower.contains("api key has expired") || lower.contains("api_key_expired")) {
        return None;
    }
    Some(format!(
        "{message}\n\
         Create a new key in the provider's console, then save it with /login \
         (or `nur auth login --provider <id> --key <new key>`)."
    ))
}

/// How to name the status in the message. `0` is not an HTTP code — it is our
/// marker for "the response was 200 and the failure arrived inside the stream",
/// and printing it as `API error (0)` read like a bug in nur rather than a
/// provider-side condition.
fn api_status_label(status: u16) -> String {
    if status == 0 {
        "mid-stream".to_string()
    } else {
        status.to_string()
    }
}

#[derive(Error, Debug)]
pub enum NurError {
    #[error("not authenticated: set NUR_API_KEY or the provider's own key variable, run `nur auth login`, or copy a vendor CLI sign-in with `nur auth import --from <codex|claude|...>`")]
    NotAuthenticated,

    #[error("API error ({}): {}", api_status_label(*status), api_message(*status, message))]
    Api { status: u16, message: String },

    #[error("API request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("config error: {0}")]
    Config(String),

    #[error("tool error: {0}")]
    Tool(String),

    #[error("max turns reached ({0})")]
    MaxTurns(u32),

    #[error("session budget reached: {0}")]
    Budget(String),

    #[error("interrupted")]
    Interrupted,

    /// The connection to the provider failed or dropped: refused, reset,
    /// closed under a reused keep-alive socket, or silent past the idle
    /// window. Nothing about the request was wrong, so the agent loop
    /// reconnects and replays it rather than ending the turn. The message
    /// carries the whole cause chain (`error sending request` alone says
    /// nothing about *why*).
    #[error("{0}")]
    Transport(String),

    /// A stream rule matched the answer (`agent::stream_rules`); `discard` is
    /// how many bytes of it had already streamed and must be taken back.
    #[error("stream rule `{name}` fired")]
    StreamRule { name: String, discard: usize },

    #[error("{0}")]
    Other(String),
}

/// Process exit codes for headless use (`nur run`, gateway, scripts). Documented
/// in `nur --help`; a caller can branch on them without scraping stderr.
pub mod exit {
    pub const OK: i32 = 0;
    pub const GENERAL: i32 = 1;
    pub const CONFIG: i32 = 2;
    pub const AUTH: i32 = 3;
    pub const PROVIDER: i32 = 4;
    pub const BUDGET: i32 = 5;
    pub const BLOCKED: i32 = 6;
    pub const CANCELLED: i32 = 130;
}

impl NurError {
    /// The exit code this failure maps to (see [`exit`]).
    pub fn exit_code(&self) -> i32 {
        match self {
            NurError::NotAuthenticated => exit::AUTH,
            NurError::Api {
                status: 401 | 403, ..
            } => exit::AUTH,
            NurError::Api { .. } | NurError::Request(_) | NurError::Transport(_) => exit::PROVIDER,
            NurError::Config(_) => exit::CONFIG,
            NurError::MaxTurns(_) | NurError::Budget(_) => exit::BUDGET,
            NurError::Interrupted => exit::CANCELLED,
            NurError::Io(_)
            | NurError::Json(_)
            | NurError::Tool(_)
            | NurError::Other(_)
            | NurError::StreamRule { .. } => exit::GENERAL,
        }
    }

    /// A transport failure from `context` ("stream connect failed", ...) with
    /// reqwest's full source chain, so the user sees "connection closed before
    /// message completed" instead of only "error sending request for url".
    pub fn transport(context: &str, error: &reqwest::Error) -> NurError {
        let cause = if error.is_timeout() {
            format!(
                "no data from the provider for {}s ({})",
                crate::api::client::provider_idle_timeout().as_secs(),
                crate::api::client::IDLE_TIMEOUT_ENV
            )
        } else {
            error_chain(error)
        };
        NurError::Transport(format!("{context}: {cause}"))
    }

    /// True when the request never got a usable answer because the connection
    /// failed, not because the provider refused it. Safe to replay.
    pub fn is_transport(&self) -> bool {
        match self {
            NurError::Transport(_) => true,
            NurError::Request(e) => {
                e.is_connect() || e.is_timeout() || e.is_request() || e.is_body()
            }
            _ => false,
        }
    }
}

/// `error: source: source`, skipping a source whose text its parent already
/// ends with (hyper and reqwest often repeat each other).
pub fn error_chain(error: &dyn std::error::Error) -> String {
    let mut out = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let text = cause.to_string();
        if !text.is_empty() && !out.ends_with(&text) {
            out.push_str(": ");
            out.push_str(&text);
        }
        source = cause.source();
    }
    out
}

pub type Result<T> = std::result::Result<T, NurError>;

#[cfg(test)]
mod tests {
    use super::*;

    /// Failure modes: a rejected key reads as a generic provider failure, or a
    /// budget stop and a cancel both come out as 1 and a script cannot tell
    /// "retry later" from "the user stopped it".
    #[test]
    fn exit_codes_separate_the_failures_a_script_must_tell_apart() {
        let rejected = NurError::Api {
            status: 401,
            message: String::new(),
        };
        let outage = NurError::Api {
            status: 503,
            message: String::new(),
        };
        assert_eq!(NurError::NotAuthenticated.exit_code(), exit::AUTH);
        assert_eq!(rejected.exit_code(), exit::AUTH);
        assert_eq!(outage.exit_code(), exit::PROVIDER);
        assert_eq!(NurError::Config("x".into()).exit_code(), exit::CONFIG);
        assert_eq!(NurError::MaxTurns(3).exit_code(), exit::BUDGET);
        assert_eq!(NurError::Budget("x".into()).exit_code(), exit::BUDGET);
        assert_eq!(NurError::Interrupted.exit_code(), exit::CANCELLED);
        assert_eq!(NurError::Other("x".into()).exit_code(), exit::GENERAL);
    }

    /// A parsed provider message must survive untouched — the fallback only
    /// fills a hole, it never rewrites what the provider actually said.
    #[test]
    fn a_real_provider_message_is_passed_through() {
        let e = NurError::Api {
            status: 400,
            message: "invalid_request_error: model not found".into(),
        };
        assert_eq!(
            e.to_string(),
            "API error (400): invalid_request_error: model not found"
        );
    }

    /// `status: 0` is our marker for "the response was 200 and the failure came
    /// down the stream". Rendering it as `API error (0)` read like a nur bug and
    /// told the user nothing about whose fault it was.
    #[test]
    fn a_mid_stream_failure_is_not_reported_as_status_zero() {
        let e = NurError::Api {
            status: 0,
            message: "ResourceExhausted: Worker local total request limit reached (90/32)".into(),
        };
        let s = e.to_string();
        assert!(!s.contains("(0)"), "0 is not an HTTP status: {s}");
        assert_eq!(
            s,
            "API error (mid-stream): ResourceExhausted: Worker local total request limit reached (90/32)"
        );

        // And with no body at all it still explains itself.
        let bare = NurError::Api {
            status: 0,
            message: String::new(),
        };
        assert!(bare.to_string().contains("ended the stream"), "{bare}");
    }

    #[test]
    fn a_bodyless_failure_still_says_something_useful() {
        // Poolside's platform answers a bad key with 403 and no body.
        let e = NurError::Api {
            status: 403,
            message: String::new(),
        };
        let s = e.to_string();
        assert!(s.starts_with("API error (403):"));
        assert!(s.contains("key was rejected"), "got: {s}");
        assert!(s.contains("/login"), "must say how to fix it: {s}");

        // Whitespace is as empty as empty.
        let e = NurError::Api {
            status: 429,
            message: "   \n".into(),
        };
        assert!(e.to_string().contains("rate limited"));

        assert!(api_message(503, "").contains("provider failed on its side"));
        assert!(api_message(404, "").contains("model id"));
        assert!(api_message(418, "").contains("no details returned"));
    }

    /// OpenCode's newer free-usage refusal and an expired key both come back
    /// with the way out appended; unrelated messages pass through untouched.
    #[test]
    fn newer_opencode_refusals_get_the_way_out() {
        let limited = "Free usage limit reached. Free models work in OpenCode \
                       (https://opencode.ai); subscribe to OpenCode Go to keep going";
        assert!(opencode_free_tier_guidance(limited)
            .unwrap()
            .contains("non-free model"));
        let expired = "This API key has expired. Create a new API key in OpenCode Console.";
        assert!(expired_key_guidance(expired)
            .unwrap()
            .contains("nur auth login --provider"));
        assert_eq!(expired_key_guidance("invalid api key"), None);
    }

    #[test]
    fn an_opencode_free_tier_403_names_the_gate_and_the_way_out() {
        let e = NurError::Api {
            status: 403,
            message: "OpenCode's free tier can only be used from within OpenCode".into(),
        };
        let s = e.to_string();
        assert!(
            s.contains("free tier can only be used"),
            "keeps upstream text: {s}"
        );
        assert!(s.contains("/model"), "must say how to fix it: {s}");
        assert!(
            s.contains("first-party client"),
            "must say whose gate it is: {s}"
        );

        // Matching is case-insensitive; anything else passes through untouched.
        let varied = api_message(
            403,
            "OPENCODE'S FREE TIER can only be used FROM WITHIN OPENCODE",
        );
        assert!(varied.contains("/model"), "{varied}");
        assert_eq!(
            api_message(403, "API key is not valid"),
            "API key is not valid"
        );
        assert_eq!(opencode_free_tier_guidance("rate limited"), None);
    }
}

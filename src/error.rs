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
    if !(lower.contains("free tier") && lower.contains("within opencode")) {
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
    #[error("not authenticated: set NUR_API_KEY (or META_API_KEY for Meta provider) or run `nur auth login`")]
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

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, NurError>;

#[cfg(test)]
mod tests {
    use super::*;

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

//! When a usage or rate limit lifts.
//!
//! Providers say it in different places: headers (`retry-after`,
//! `anthropic-ratelimit-unified-reset`, `x-ratelimit-reset-*`), the error body
//! (`resets_in_seconds`, `resets_at`, Gemini's `RetryInfo.retryDelay`), or only
//! the message text ("try again in 2 hours 13 minutes"). The client records
//! what headers say ([`observe`]) and writes what the body says into the error
//! message ([`body_note`]), so [`reset_after`] can answer from the message
//! alone, falling back to the last recorded header.

use reqwest::header::HeaderMap;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How long a header reading stays the answer for an error without its own.
const OBSERVED_FRESH: Duration = Duration::from_secs(120);

static OBSERVED: Mutex<Option<(Instant, u64)>> = Mutex::new(None);

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Record when a refused response says its limit lifts (429 and 503 only:
/// any other status carrying `retry-after` is not a limit).
pub fn observe(status: u16, headers: &HeaderMap) {
    if !matches!(status, 429 | 503 | 529) {
        return;
    }
    if let Some(at) = header_reset(headers, now_secs()) {
        if let Ok(mut seen) = OBSERVED.lock() {
            *seen = Some((Instant::now(), at));
        }
    }
}

/// The unix time a response's headers say the limit lifts.
fn header_reset(headers: &HeaderMap, now: u64) -> Option<u64> {
    let get = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
    };
    // Claude subscriptions: the window that refused, as epoch seconds.
    if let Some(at) = get("anthropic-ratelimit-unified-reset").and_then(epoch_or_date) {
        return Some(at);
    }
    if let Some(value) = get("retry-after") {
        if let Ok(secs) = value.parse::<f64>() {
            return Some(now + secs.ceil().max(0.0) as u64);
        }
        if let Ok(at) = chrono::DateTime::parse_from_rfc2822(value) {
            return Some(at.timestamp().max(0) as u64);
        }
    }
    for name in ["x-ratelimit-reset-requests", "x-ratelimit-reset-tokens"] {
        if let Some(secs) = get(name).and_then(parse_go_duration) {
            return Some(now + secs);
        }
    }
    get("x-ratelimit-reset").and_then(|v| {
        let n = v.parse::<u64>().ok()?;
        // Either epoch seconds or seconds from now.
        Some(if n > 1_000_000_000 { n } else { now + n })
    })
}

fn epoch_or_date(value: &str) -> Option<u64> {
    if let Ok(n) = value.parse::<u64>() {
        return Some(n);
    }
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|d| d.timestamp().max(0) as u64)
}

/// OpenAI's `6m0s`, `1h2m3.5s`, `20ms` (rounded up to whole seconds).
fn parse_go_duration(value: &str) -> Option<u64> {
    let re = regex::Regex::new(r"(\d+(?:\.\d+)?)(ms|h|m|s)").ok()?;
    let mut total = 0.0;
    let mut any = false;
    for c in re.captures_iter(value) {
        let n: f64 = c[1].parse().ok()?;
        total += match &c[2] {
            "h" => n * 3600.0,
            "m" => n * 60.0,
            "s" => n,
            _ => n / 1000.0,
        };
        any = true;
    }
    any.then(|| total.ceil() as u64)
}

/// What an error body says about its reset, as a phrase for the message:
/// " (resets in 3h 25m)". `None` when the body names no time.
pub fn body_note(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let error = v.get("error").unwrap_or(&v);
    let now = now_secs();
    let secs = error
        .get("resets_in_seconds")
        .and_then(serde_json::Value::as_u64)
        .or_else(|| {
            error
                .get("resets_at")
                .and_then(|r| r.as_u64().or_else(|| r.as_str().and_then(epoch_or_date)))
                .map(|at| at.saturating_sub(now))
        })
        .or_else(|| {
            // Gemini: details[].retryDelay = "23s" on google.rpc.RetryInfo.
            error
                .get("details")
                .and_then(serde_json::Value::as_array)?
                .iter()
                .find_map(|d| d.get("retryDelay").and_then(|r| r.as_str()))
                .and_then(parse_go_duration)
        })?;
    Some(format!(" (resets in {})", format_wait(secs)))
}

/// `3h 25m`, `14m`, `40s`, `2d 3h`.
pub fn format_wait(secs: u64) -> String {
    let (d, h, m, s) = (
        secs / 86_400,
        secs % 86_400 / 3600,
        secs % 3600 / 60,
        secs % 60,
    );
    match (d, h, m) {
        (0, 0, 0) => format!("{s}s"),
        (0, 0, m) => format!("{m}m"),
        (0, h, 0) => format!("{h}h"),
        (0, h, m) => format!("{h}h {m}m"),
        (d, 0, _) => format!("{d}d"),
        (d, h, _) => format!("{d}d {h}h"),
    }
}

/// The unix time the limit in `message` lifts: what the message says
/// ("resets in 3h 25m", "try again in 2 hours", "retry after 30 seconds",
/// "resets at 2026-10-30T18:00:00Z"), else the last header reading from the
/// past two minutes.
pub fn reset_after(message: &str, now: u64) -> Option<u64> {
    if let Some(at) = message_reset(message, now) {
        return Some(at);
    }
    let seen = OBSERVED.lock().ok()?.as_ref().copied()?;
    (seen.0.elapsed() < OBSERVED_FRESH && seen.1 > now).then_some(seen.1)
}

fn message_reset(message: &str, now: u64) -> Option<u64> {
    use std::sync::OnceLock;
    static RELATIVE: OnceLock<regex::Regex> = OnceLock::new();
    static UNIT: OnceLock<regex::Regex> = OnceLock::new();
    static ABSOLUTE: OnceLock<regex::Regex> = OnceLock::new();
    let lower = message.to_ascii_lowercase();
    let relative = RELATIVE.get_or_init(|| {
        regex::Regex::new(
            r"(?:resets?|try again|retry|available again|wait)\s+(?:in|after)\s+((?:[\d.]+\s*(?:days?|d|hours?|hrs?|h|minutes?|mins?|m|seconds?|secs?|s)\b[\s,and]*)+)",
        )
        .expect("relative reset")
    });
    let unit = UNIT.get_or_init(|| {
        regex::Regex::new(r"([\d.]+)\s*(days?|d|hours?|hrs?|h|minutes?|mins?|m|seconds?|secs?|s)\b")
            .expect("reset unit")
    });
    if let Some(span) = relative.captures(&lower) {
        let mut secs = 0.0;
        for c in unit.captures_iter(&span[1]) {
            let n: f64 = c[1].parse().unwrap_or(0.0);
            secs += n * match c[2].chars().next() {
                Some('d') => 86_400.0,
                Some('h') => 3600.0,
                Some('m') => 60.0,
                _ => 1.0,
            };
        }
        if secs > 0.0 {
            return Some(now + secs.ceil() as u64);
        }
    }
    let absolute = ABSOLUTE.get_or_init(|| {
        regex::Regex::new(r"(?:resets?|try again|available again)\s+(?:at|after|on)\s+(\d{4}-\d{2}-\d{2}[t ]\d{2}:\d{2}(?::\d{2})?(?:\.\d+)?(?:z|[+-]\d{2}:?\d{2})?)")
            .expect("absolute reset")
    });
    let stamp = absolute
        .captures(&lower)?
        .get(1)?
        .as_str()
        .to_ascii_uppercase();
    let stamp = stamp.replace(' ', "T");
    chrono::DateTime::parse_from_rfc3339(&stamp)
        .map(|d| d.timestamp().max(0) as u64)
        .ok()
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(&stamp, "%Y-%m-%dT%H:%M:%S")
                .or_else(|_| chrono::NaiveDateTime::parse_from_str(&stamp, "%Y-%m-%dT%H:%M"))
                .ok()
                .map(|d| d.and_utc().timestamp().max(0) as u64)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;

    // Failure modes: a provider's reset is missed (the session waits blind or
    // retries early); a number in the message is read as a reset; headers in
    // their several shapes are misread.
    #[test]
    fn messages_in_every_shape_give_the_reset() {
        let now = 1_000_000;
        let cases = [
            (
                "You've hit your usage limit. Try again in 2 hours 13 minutes.",
                2 * 3600 + 13 * 60,
            ),
            ("rate limited (resets in 3h 25m)", 3 * 3600 + 25 * 60),
            ("Quota exceeded. Please retry in 23.4s.", 24),
            ("limit reached, resets in 1 day", 86_400),
            ("Retry after 30 seconds", 30),
        ];
        for (message, wait) in cases {
            assert_eq!(message_reset(message, now), Some(now + wait), "{message}");
        }
        assert_eq!(
            message_reset("limit resets at 2026-10-30T18:00:00Z", now),
            Some(1_793_383_200)
        );
        assert_eq!(message_reset("error 429 after 3 retries", now), None);
        assert_eq!(message_reset("model gpt-5 in 2 regions", now), None);
    }

    #[test]
    fn headers_and_bodies_give_the_reset() {
        let now = 1_000_000;
        let mut h = HeaderMap::new();
        h.insert("retry-after", HeaderValue::from_static("120"));
        assert_eq!(header_reset(&h, now), Some(now + 120));
        let mut h = HeaderMap::new();
        h.insert(
            "anthropic-ratelimit-unified-reset",
            HeaderValue::from_static("1793383200"),
        );
        assert_eq!(header_reset(&h, now), Some(1_793_383_200));
        let mut h = HeaderMap::new();
        h.insert(
            "x-ratelimit-reset-requests",
            HeaderValue::from_static("6m0s"),
        );
        assert_eq!(header_reset(&h, now), Some(now + 360));
        assert_eq!(
            body_note(r#"{"error":{"type":"usage_limit_reached","resets_in_seconds":7980}}"#)
                .as_deref(),
            Some(" (resets in 2h 13m)")
        );
        assert_eq!(
            body_note(r#"{"error":{"details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"23s"}]}}"#)
                .as_deref(),
            Some(" (resets in 23s)")
        );
        assert_eq!(body_note(r#"{"error":{"message":"bad request"}}"#), None);
        assert_eq!(format_wait(90_000), "1d 1h");
    }
}

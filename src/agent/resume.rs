//! Picking a session back up after a usage limit or a transient failure.
//!
//! A turn that ends on a limit (429, "usage limit reached", quota) waits for
//! the time the provider gave plus `delay_secs`, then continues; with no time
//! given it probes on a widening schedule. A transient failure the client's
//! own retries could not ride out (5xx, overloaded, a dropped connection)
//! retries sooner. Everything else (sign-in, a bad request, billing) stops,
//! because waiting cannot fix it. The TUI shows the plan as a card the user
//! can switch off; `nur run` waits in place. On by default.
//!
//! ```toml
//! [auto_continue]
//! enabled = true
//! delay_secs = 60      # after the reset the provider gave
//! max_attempts = 8     # resumes in a row before handing back
//! ```

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AutoContinueConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Seconds after the provider's reset time before continuing: resets are
    /// rounded and clocks drift, and a request a second early fails again.
    #[serde(default = "default_delay")]
    pub delay_secs: u64,
    /// Resumes in a row (no turn finishing in between) before handing back.
    #[serde(default = "default_attempts")]
    pub max_attempts: u32,
}

fn default_true() -> bool {
    true
}
fn default_delay() -> u64 {
    60
}
fn default_attempts() -> u32 {
    8
}

impl Default for AutoContinueConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            delay_secs: default_delay(),
            max_attempts: default_attempts(),
        }
    }
}

impl AutoContinueConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Why the turn stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// A usage or rate limit: lifts at a time.
    Limit,
    /// The provider or the network failed past the client's own retries.
    Transient,
}

/// When and why the session continues.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub stop: Stop,
    /// Unix seconds to continue at.
    pub at: u64,
    /// The provider named the reset (else `at` is a probe).
    pub known_reset: bool,
    /// Resumes in a row so far, this one included (1-based).
    pub attempt: u32,
    /// The provider's message, one line.
    pub reason: String,
}

/// What the continuation says to the model.
pub const CONTINUE_PROMPT: &str = "Continue from where you left off. The previous turn stopped \
on a provider limit or outage, not by choice: pick the work back up without repeating what is \
already done.";

/// Probe schedule when a limit names no reset, and the retry schedule for
/// transient failures (the last step repeats).
const LIMIT_PROBES: &[u64] = &[300, 900, 1800, 3600];
const TRANSIENT_RETRIES: &[u64] = &[30, 120, 300, 900, 1800];

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Which kind of stop `error` (a turn's error text) is, if waiting can fix it.
pub fn classify(error: &str) -> Option<Stop> {
    let lower = error.to_ascii_lowercase();
    // Money and access are not fixed by waiting.
    const NEVER: &[&str] = &[
        "insufficient_quota",
        "billing",
        "payment",
        "credit balance",
        "not authenticated",
        "sign in",
        "signed in",
        "invalid api key",
        "invalid_api_key",
        "permission denied",
        "interrupted",
        "context length",
        "context window",
        "too long",
    ];
    if NEVER.iter().any(|n| lower.contains(n)) {
        return None;
    }
    let status = regex::Regex::new(r"api error \((\d{3})\)")
        .ok()
        .and_then(|re| re.captures(&lower))
        .and_then(|c| c[1].parse::<u16>().ok());
    const LIMIT: &[&str] = &[
        "rate limit",
        "rate-limit",
        "rate_limit",
        "usage limit",
        "usage_limit",
        "limit reached",
        "limit exceeded",
        "too many requests",
        "resource_exhausted",
        "resource exhausted",
        "quota",
        "exceeded your",
        "free usage limit",
    ];
    if status == Some(429) || LIMIT.iter().any(|n| lower.contains(n)) {
        return Some(Stop::Limit);
    }
    const TRANSIENT: &[&str] = &[
        "overloaded",
        "temporarily unavailable",
        "service unavailable",
        "bad gateway",
        "gateway timeout",
        "internal server error",
        "timed out",
        "timeout",
        "connection reset",
        "connection closed",
        "connection refused",
        "error sending request",
        "stream ended",
        "mid-stream",
        "unexpected eof",
    ];
    if matches!(status, Some(500 | 502 | 503 | 504 | 520..=529))
        || TRANSIENT.iter().any(|n| lower.contains(n))
    {
        return Some(Stop::Transient);
    }
    None
}

/// The plan for continuing after `error`, or `None` when waiting cannot help,
/// auto-continue is off, or `attempt` (1-based) is past the budget.
pub fn plan(error: &str, attempt: u32, cfg: &AutoContinueConfig, now: u64) -> Option<Plan> {
    if !cfg.enabled || attempt == 0 || attempt > cfg.max_attempts {
        return None;
    }
    let stop = classify(error)?;
    let step = |table: &[u64]| table[(attempt as usize - 1).min(table.len() - 1)];
    let (at, known_reset) = match stop {
        Stop::Limit => match crate::api::limits::reset_after(error, now) {
            Some(reset) => (reset.max(now) + cfg.delay_secs, true),
            None => (now + step(LIMIT_PROBES), false),
        },
        Stop::Transient => (now + step(TRANSIENT_RETRIES), false),
    };
    let reason: String = error
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or(error)
        .trim()
        .chars()
        .take(160)
        .collect();
    Some(Plan {
        stop,
        at,
        known_reset,
        attempt,
        reason,
    })
}

/// "15:42" today, "Fri 15:42" within a week, else the date.
pub fn clock(at: u64) -> String {
    use chrono::{Local, TimeZone};
    let Some(when) = Local.timestamp_opt(at as i64, 0).single() else {
        return String::new();
    };
    let now = Local::now();
    let days = (when.date_naive() - now.date_naive()).num_days();
    match days {
        0 => when.format("%H:%M").to_string(),
        1..=6 => when.format("%a %H:%M").to_string(),
        _ => when.format("%b %-d %H:%M").to_string(),
    }
}

/// One line for the plan: "usage limit · continuing at 15:42 (in 2h 13m)".
pub fn describe(plan: &Plan, now: u64) -> String {
    let what = match plan.stop {
        Stop::Limit => "usage limit reached",
        Stop::Transient => "provider unavailable",
    };
    let verb = if plan.known_reset {
        "continuing"
    } else {
        "trying again"
    };
    format!(
        "{what} · {verb} at {} (in {})",
        clock(plan.at),
        crate::api::limits::format_wait(plan.at.saturating_sub(now))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // Failure modes: a billing or sign-in error is retried forever; a limit is
    // retried before it lifts; a transient failure waits hours; retries never
    // stop.
    #[test]
    fn only_what_waiting_fixes_continues() {
        assert_eq!(classify("API error (429): slow down"), Some(Stop::Limit));
        assert_eq!(
            classify("API error (403): You've hit your usage limit. Try again in 2 hours."),
            Some(Stop::Limit)
        );
        assert_eq!(
            classify("API error (529): Overloaded"),
            Some(Stop::Transient)
        );
        assert_eq!(
            classify("API request failed: error sending request"),
            Some(Stop::Transient)
        );
        assert_eq!(
            classify("API error (429): You exceeded your current quota, please check your plan and billing details"),
            None
        );
        assert_eq!(classify("API error (401): invalid api key"), None);
        assert_eq!(classify("API error (400): Unknown parameter"), None);
        assert_eq!(classify("interrupted"), None);
    }

    #[test]
    fn plans_wait_for_the_reset_and_stop_at_the_budget() {
        let cfg = AutoContinueConfig::default();
        let now = 1_000_000;
        let p = plan(
            "API error (429): limit reached (resets in 2h 13m)",
            1,
            &cfg,
            now,
        )
        .unwrap();
        assert_eq!((p.at, p.known_reset), (now + 7980 + 60, true));
        let p = plan("API error (429): slow down", 1, &cfg, now).unwrap();
        assert_eq!((p.at, p.known_reset), (now + 300, false));
        let p = plan("API error (429): slow down", 9, &cfg, now);
        assert!(p.is_none(), "past max_attempts");
        let p = plan("API error (503): unavailable", 2, &cfg, now).unwrap();
        assert_eq!(p.at, now + 120);
        let off = AutoContinueConfig {
            enabled: false,
            ..cfg
        };
        assert!(plan("API error (429): slow down", 1, &off, now).is_none());
    }
}

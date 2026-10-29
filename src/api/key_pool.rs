//! Rotating API keys: several keys for one provider, one in use at a time.
//!
//! A provider can hold extra API keys beside its main one (stored with
//! `nur auth pool add`, or listed in the plural env var, e.g.
//! `MORPH_API_KEYS=k1,k2`). A conversation sticks to one key, chosen by hashing
//! its session id, because providers shard prompt caches per account and a
//! key that changed on every request would re-read the whole prefix each
//! time. When the key in use is rate-limited, out of quota or refused, it is
//! blocked for a while and the next free key takes the request at once,
//! before any backoff or failover. The design follows oh-my-pi's credential
//! pool: session-hashed start, skip blocked keys, longest block wins.

use crate::error::NurError;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A rate limit or overload with no stated reset.
pub const RATE_LIMIT_BLOCK: Duration = Duration::from_secs(60);
/// Credit, plan or quota exhausted: waiting minutes will not help.
pub const QUOTA_BLOCK: Duration = Duration::from_secs(60 * 60);
/// Refused as invalid or revoked: out of rotation for the session.
pub const AUTH_BLOCK: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug)]
struct State {
    /// Index of the key in use.
    active: usize,
    blocked_until: Vec<Option<Instant>>,
}

#[derive(Debug)]
pub struct KeyPool {
    keys: Vec<String>,
    state: Mutex<State>,
}

impl KeyPool {
    /// A pool over `keys` (deduplicated, blanks dropped), starting at the key
    /// `affinity` hashes to. `None` for fewer than two keys: nothing rotates.
    pub fn new(keys: Vec<String>, affinity: &str) -> Option<Self> {
        let mut unique: Vec<String> = Vec::new();
        for key in keys {
            let key = key.trim().to_string();
            if !key.is_empty() && !unique.contains(&key) {
                unique.push(key);
            }
        }
        if unique.len() < 2 {
            return None;
        }
        let active = (fnv1a(affinity) % unique.len() as u64) as usize;
        let len = unique.len();
        Some(Self {
            keys: unique,
            state: Mutex::new(State {
                active,
                blocked_until: vec![None; len],
            }),
        })
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Is a key other than the one in use free right now?
    pub fn has_spare(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        self.next_free(&state, Instant::now()).is_some()
    }

    /// The key for the next request: the one in use, unless it is blocked
    /// and another key is free.
    pub fn current(&self) -> String {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        if is_blocked(&state, state.active, now) {
            if let Some(free) = self.next_free(&state, now) {
                state.active = free;
            }
        }
        self.keys[state.active].clone()
    }

    /// Block the key in use for `duration` (a longer existing block wins) and
    /// move to the next free key. Returns `(from, to)` as 1-based positions,
    /// or `None` when every other key is blocked too.
    pub fn rotate(&self, duration: Duration) -> Option<(usize, usize)> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let from = state.active;
        let until = now + duration;
        let slot = &mut state.blocked_until[from];
        if slot.is_none_or(|existing| existing < until) {
            *slot = Some(until);
        }
        let to = self.next_free(&state, now)?;
        state.active = to;
        Some((from + 1, to + 1))
    }

    fn next_free(&self, state: &State, now: Instant) -> Option<usize> {
        (1..self.keys.len())
            .map(|step| (state.active + step) % self.keys.len())
            .find(|&i| !is_blocked(state, i, now))
    }
}

fn is_blocked(state: &State, index: usize, now: Instant) -> bool {
    state.blocked_until[index].is_some_and(|until| until > now)
}

/// FNV-1a: a stable hash for the session-to-key mapping (std's hasher is
/// randomly seeded per process, which would move a resumed session).
fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// How long a provider error takes the key that drew it out of rotation, or
/// `None` when the error is about the request rather than the key.
pub fn block_for(error: &NurError) -> Option<Duration> {
    let NurError::Api { status, message } = error else {
        return None;
    };
    let lower = message.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|needle| lower.contains(needle));
    // Phrases only a spent account produces. Anthropic reports a low credit
    // balance as a 400, so these count on 400 too.
    let spent = has(&[
        "insufficient_quota",
        "quota_exceeded",
        "credit balance",
        "out of credits",
        "payment required",
        "exceeded your current quota",
    ]);
    // Looser words that also appear in ordinary validation errors ("billing"
    // in a parameter name): only on the statuses that mean quota.
    let spent_loosely = has(&["billing", "usage limit", "quota exceeded"]);
    // A 403 about one model or one region is the request's problem; benching
    // the key for a day would rotate away from a key that works.
    let request_scoped_403 = has(&["model"])
        && has(&[
            "not available",
            "access",
            "not allowed",
            "not supported",
            "permission",
        ])
        || has(&["region", "country", "location"]);
    match status {
        402 => Some(QUOTA_BLOCK),
        400 | 403 | 429 if spent => Some(QUOTA_BLOCK),
        403 | 429 if spent_loosely => Some(QUOTA_BLOCK),
        429 | 503 | 529 => Some(RATE_LIMIT_BLOCK),
        403 if request_scoped_403 => None,
        401 | 403 => Some(AUTH_BLOCK),
        0 if lower.contains("rate limit") || lower.contains("too many requests") => {
            Some(RATE_LIMIT_BLOCK)
        }
        _ => None,
    }
}

/// Extra keys from the plural env var of `env_key` (`OPENAI_API_KEY` →
/// `OPENAI_API_KEYS`), split on commas, semicolons and whitespace.
pub fn env_keys(env_key: &str) -> Vec<String> {
    if env_key.is_empty() {
        return Vec::new();
    }
    std::env::var(format!("{env_key}S"))
        .map(|raw| {
            raw.split(|c: char| c == ',' || c == ';' || c.is_whitespace())
                .filter(|k| !k.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(n: usize) -> Vec<String> {
        (1..=n).map(|i| format!("key-{i}")).collect()
    }

    /// One key, or duplicates of one, is not a pool.
    #[test]
    fn a_single_key_does_not_rotate() {
        assert!(KeyPool::new(keys(1), "s").is_none());
        assert!(KeyPool::new(vec!["a".into(), " a ".into(), String::new()], "s").is_none());
    }

    /// The same session lands on the same key every time (cache affinity),
    /// and different sessions spread over the keys.
    #[test]
    fn a_session_sticks_to_its_hashed_key() {
        let first = KeyPool::new(keys(3), "session-a").unwrap().current();
        for _ in 0..5 {
            assert_eq!(KeyPool::new(keys(3), "session-a").unwrap().current(), first);
        }
        let used: std::collections::HashSet<String> = (0..40)
            .map(|i| KeyPool::new(keys(3), &format!("s{i}")).unwrap().current())
            .collect();
        assert_eq!(used.len(), 3, "sessions should spread over every key");
    }

    /// A blocked key hands over to the next free one and stays out until
    /// its block expires; with every key blocked there is nowhere to go.
    #[test]
    fn rotation_skips_blocked_keys_until_all_are_blocked() {
        let pool = KeyPool::new(keys(3), "session").unwrap();
        let start = pool.current();
        let (from, to) = pool.rotate(QUOTA_BLOCK).unwrap();
        assert_ne!(from, to);
        let second = pool.current();
        assert_ne!(second, start);
        assert_eq!(pool.current(), second, "sticks to the new key");
        let (_, third) = pool.rotate(RATE_LIMIT_BLOCK).unwrap();
        assert!(third != from && third != to);
        assert!(pool.rotate(RATE_LIMIT_BLOCK).is_none(), "all three blocked");

        // An expired block frees the key again.
        let pool = KeyPool::new(keys(2), "session").unwrap();
        let first = pool.current();
        pool.rotate(Duration::from_millis(0)).unwrap();
        std::thread::sleep(Duration::from_millis(5));
        assert!(pool.rotate(RATE_LIMIT_BLOCK).is_some());
        assert_eq!(pool.current(), first);
    }

    #[test]
    fn errors_map_to_how_long_the_key_sits_out() {
        let api = |status, message: &str| NurError::Api {
            status,
            message: message.into(),
        };
        assert_eq!(block_for(&api(429, "slow down")), Some(RATE_LIMIT_BLOCK));
        assert_eq!(block_for(&api(529, "overloaded")), Some(RATE_LIMIT_BLOCK));
        assert_eq!(
            block_for(&api(429, "You exceeded your current quota")),
            Some(QUOTA_BLOCK)
        );
        assert_eq!(block_for(&api(402, "")), Some(QUOTA_BLOCK));
        assert_eq!(block_for(&api(401, "invalid api key")), Some(AUTH_BLOCK));
        assert_eq!(
            block_for(&api(0, "rate limit reached mid-stream")),
            Some(RATE_LIMIT_BLOCK)
        );
        // About the request, not the key: rotating would not help.
        assert_eq!(block_for(&api(400, "unsupported parameter")), None);
        // A validation error that merely names billing, and a 403 about one
        // model or region, are about the request: the key keeps working.
        assert_eq!(block_for(&api(400, "unknown field billing_address")), None);
        assert_eq!(
            block_for(&api(403, "model gpt-x is not available for this key")),
            None
        );
        assert_eq!(block_for(&api(403, "not available in your region")), None);
        // A spent account still benches the key, whatever the status.
        assert_eq!(
            block_for(&api(400, "Your credit balance is too low")),
            Some(QUOTA_BLOCK)
        );
        assert_eq!(block_for(&api(403, "forbidden")), Some(AUTH_BLOCK));
        assert_eq!(block_for(&api(500, "internal error")), None);
        assert_eq!(block_for(&NurError::Other("connection reset".into())), None);
    }

    #[test]
    fn the_plural_env_var_lists_extra_keys() {
        let name = format!("NUR_TEST_POOL_{}", std::process::id());
        std::env::set_var(format!("{name}S"), "k1, k2;k3\nk4");
        assert_eq!(env_keys(&name), vec!["k1", "k2", "k3", "k4"]);
        std::env::remove_var(format!("{name}S"));
        assert!(env_keys(&name).is_empty());
        assert!(env_keys("").is_empty());
    }
}

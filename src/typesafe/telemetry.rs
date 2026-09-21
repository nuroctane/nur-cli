//! TypeSafe/Jev accounting for the current process.
//!
//! Every Jev request and every decision taken from a Jev answer lands here, so
//! the harness can show what the boost layer actually did: requests spent,
//! tokens billed, judgments used, escalations, and the frontier work it
//! replaced (compaction that never had to call a big model).
//!
//! The queue is process-global and drained by the same owner that consumes
//! Headroom telemetry (see `agent::loop::record_auxiliary_telemetry`), so
//! receipts stay in one place.

use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};

/// Counters for one snapshot/drain window.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TypesafeTelemetry {
    /// Endpoint the requests actually went to (a local bridge or the hosted API -
    /// the receipt must not claim one when the other served them).
    #[serde(default)]
    pub route: String,
    /// Model the endpoint reported serving, when it said (the hosted API resolves
    /// `jev-latest` to e.g. `jev-1.13.0`).
    #[serde(default)]
    pub model: String,
    /// HTTP requests actually sent (batches, not questions).
    pub requests: u64,
    /// Questions asked across those requests.
    pub questions: u64,
    /// Requests served by more than one concurrent connection.
    pub parallel_requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Judgments the harness acted on.
    pub decisions: u64,
    /// Answers whose confidence fell below the acting threshold - routed to a
    /// bigger model / a human instead of being obeyed.
    pub escalations: u64,
    /// Tool calls dropped (never run) because the answer said they were stale.
    pub gated_calls: u64,
    /// Tool calls/results dropped from context by Jev-scored compaction.
    pub pruned_calls: u64,
    pub pruned_results: u64,
    /// Characters removed from context by compaction.
    pub chars_saved: u64,
    /// Frontier compaction calls that were not needed.
    pub frontier_calls_avoided: u64,
    /// Request-level failures (network, parse, auth, rate limit exhausted).
    pub failures: u64,
}

impl TypesafeTelemetry {
    pub fn is_empty(&self) -> bool {
        self.requests == 0 && self.questions == 0 && self.decisions == 0 && self.failures == 0
    }

    /// Tokens billed by TypeSafe for this window.
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }

    /// Rough tokens kept out of the active context (`chars / 4`).
    pub fn tokens_saved(&self) -> u64 {
        self.chars_saved / 4
    }

    /// One-line summary for `/typesafe` and `nur doctor`.
    pub fn summary(&self) -> String {
        format!(
            "requests {} · questions {} · tokens {} (in {} / out {}) · decisions {} · \
             escalations {} · gated {} · pruned {} call(s)/{} result(s) · ~{} tok saved · \
             frontier calls avoided {} · failures {}",
            self.requests,
            self.questions,
            self.total_tokens(),
            self.input_tokens,
            self.output_tokens,
            self.decisions,
            self.escalations,
            self.gated_calls,
            self.pruned_calls,
            self.pruned_results,
            self.tokens_saved(),
            self.frontier_calls_avoided,
            self.failures,
        )
    }

    /// Compact footer chip. Empty when nothing happened.
    pub fn chip(&self) -> String {
        if self.is_empty() {
            return String::new();
        }
        let mut parts = vec![format!("jev {} req", self.requests)];
        let tok = self.total_tokens();
        if tok > 0 {
            parts.push(format!("{tok} tok"));
        }
        let saved = self.tokens_saved();
        if saved > 0 {
            parts.push(format!("~{} saved", short_tokens(saved)));
        }
        if self.escalations > 0 {
            parts.push(format!("{} esc", self.escalations));
        }
        if self.failures > 0 {
            parts.push(format!("{} err", self.failures));
        }
        parts.join(" · ")
    }
}

fn short_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn queue() -> &'static Mutex<TypesafeTelemetry> {
    static QUEUE: OnceLock<Mutex<TypesafeTelemetry>> = OnceLock::new();
    QUEUE.get_or_init(|| Mutex::new(TypesafeTelemetry::default()))
}

/// Add one request's worth of accounting.
///
/// `route` and `model` are what actually served it, so a local bridge and the
/// hosted API are never confused in the receipt.
pub fn record_request(
    route: &str,
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
    questions: u64,
) {
    mutate(|t| {
        if !route.trim().is_empty() {
            t.route = route.trim().to_string();
        }
        if !model.trim().is_empty() {
            t.model = model.trim().to_string();
        }
        t.requests += 1;
        t.questions += questions;
        t.input_tokens += input_tokens;
        t.output_tokens += output_tokens;
    });
}

/// Mark a request as one of several that ran concurrently.
pub fn record_parallel(extra: u64) {
    if extra > 0 {
        mutate(|t| t.parallel_requests += extra);
    }
}

pub fn record_failure() {
    mutate(|t| t.failures += 1);
}

/// Record a judgment the harness acted on, and whether it had to escalate.
pub fn record_decision(confidence: Option<f64>, acted: bool, escalate_floor: f64) {
    mutate(|t| {
        if acted {
            t.decisions += 1;
        }
        if let Some(c) = confidence {
            if c < escalate_floor {
                t.escalations += 1;
            }
        }
    });
}

/// Record a tool call the gate stopped before it ran.
pub fn record_gated_call() {
    mutate(|t| t.gated_calls += 1);
}

/// Record compaction results: calls/results dropped, characters removed, and
/// the frontier summarizer calls those removals made unnecessary.
pub fn record_prune(calls: u64, results: u64, chars_saved: u64, frontier_calls_avoided: u64) {
    mutate(|t| {
        t.pruned_calls += calls;
        t.pruned_results += results;
        t.chars_saved += chars_saved;
        t.frontier_calls_avoided += frontier_calls_avoided;
    });
}

fn mutate(f: impl FnOnce(&mut TypesafeTelemetry)) {
    if let Ok(mut g) = queue().lock() {
        let mut next = g.clone();
        f(&mut next);
        *g = next;
    }
}

/// Cumulative counters without draining.
pub fn snapshot() -> TypesafeTelemetry {
    queue().lock().map(|g| g.clone()).unwrap_or_default()
}

/// Everything recorded since the previous call, as a delta.
///
/// This is what receipt recording uses: the chip and `/typesafe` want the
/// session totals ([`snapshot`]), while the append-only receipt wants only what
/// happened in this batch. Draining the queue outright for receipts would reset
/// the read-out, so the two consumers get different views of the same counters.
pub fn take_delta() -> TypesafeTelemetry {
    static LAST: OnceLock<Mutex<TypesafeTelemetry>> = OnceLock::new();
    let current = snapshot();
    let last = LAST.get_or_init(|| Mutex::new(TypesafeTelemetry::default()));
    let Ok(mut guard) = last.lock() else {
        return TypesafeTelemetry::default();
    };
    let prev = guard.clone();
    *guard = current.clone();
    TypesafeTelemetry {
        route: if current.route == prev.route {
            String::new()
        } else {
            current.route.clone()
        },
        model: if current.model == prev.model {
            String::new()
        } else {
            current.model.clone()
        },
        requests: current.requests.saturating_sub(prev.requests),
        questions: current.questions.saturating_sub(prev.questions),
        parallel_requests: current
            .parallel_requests
            .saturating_sub(prev.parallel_requests),
        input_tokens: current.input_tokens.saturating_sub(prev.input_tokens),
        output_tokens: current.output_tokens.saturating_sub(prev.output_tokens),
        decisions: current.decisions.saturating_sub(prev.decisions),
        escalations: current.escalations.saturating_sub(prev.escalations),
        gated_calls: current.gated_calls.saturating_sub(prev.gated_calls),
        pruned_calls: current.pruned_calls.saturating_sub(prev.pruned_calls),
        pruned_results: current.pruned_results.saturating_sub(prev.pruned_results),
        chars_saved: current.chars_saved.saturating_sub(prev.chars_saved),
        frontier_calls_avoided: current
            .frontier_calls_avoided
            .saturating_sub(prev.frontier_calls_avoided),
        failures: current.failures.saturating_sub(prev.failures),
    }
}

/// Drain the counters (full reset - used by tests and by `/typesafe reset`).
pub fn take() -> TypesafeTelemetry {
    queue()
        .lock()
        .map(|mut g| std::mem::take(&mut *g))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Counters are process-global; keep every assertion relative to a baseline.
    #[test]
    fn records_and_drains() {
        let before = snapshot();
        record_request("http://127.0.0.1:8788/v1/systemone", "mock-1", 100, 5, 3);
        record_decision(Some(0.9), true, 0.5);
        record_decision(Some(0.2), false, 0.5);
        record_gated_call();
        record_prune(2, 1, 4000, 1);
        record_failure();
        let t = take();
        assert_eq!(t.requests, before.requests + 1);
        assert_eq!(t.questions, before.questions + 3);
        assert_eq!(t.input_tokens, before.input_tokens + 100);
        assert_eq!(t.decisions, before.decisions + 1);
        assert_eq!(t.escalations, before.escalations + 1);
        assert_eq!(t.gated_calls, before.gated_calls + 1);
        assert_eq!(t.pruned_calls, before.pruned_calls + 2);
        assert_eq!(t.tokens_saved(), before.tokens_saved() + 1000);
        assert_eq!(t.frontier_calls_avoided, before.frontier_calls_avoided + 1);
        assert_eq!(t.failures, before.failures + 1);
        assert!(t.summary().contains("frontier calls avoided"));
    }

    #[test]
    fn chip_and_summary_render_a_reading() {
        // Counters are process-global and tests run in parallel, so formatting is
        // asserted on a value, not on shared state.
        let idle = TypesafeTelemetry::default();
        assert!(idle.is_empty());
        assert!(idle.chip().is_empty());

        let t = TypesafeTelemetry {
            requests: 3,
            questions: 40,
            input_tokens: 20_000,
            output_tokens: 100,
            decisions: 12,
            escalations: 2,
            gated_calls: 1,
            pruned_calls: 9,
            pruned_results: 9,
            chars_saved: 40_000,
            frontier_calls_avoided: 1,
            ..TypesafeTelemetry::default()
        };
        let chip = t.chip();
        assert!(chip.contains("jev 3 req"), "{chip}");
        assert!(chip.contains("20100 tok"), "{chip}");
        assert!(chip.contains("10.0k saved"), "{chip}");
        assert!(chip.contains("2 esc"), "{chip}");
        assert!(t.summary().contains("frontier calls avoided 1"));
        assert!(!t.is_empty());
    }

    #[test]
    fn a_delta_reports_only_what_happened_since_the_last_read() {
        let before = snapshot();
        record_request(
            "https://api.typesafe.ai/v1/systemone",
            "jev-1.13.0",
            1_000,
            10,
            2,
        );
        let delta = take_delta();
        assert_eq!(delta.route, "https://api.typesafe.ai/v1/systemone");
        assert_eq!(delta.model, "jev-1.13.0");
        assert!(delta.requests >= 1, "{delta:?}");
        assert!(delta.input_tokens >= 1_000, "{delta:?}");
        // Reading again immediately sees nothing new from this test.
        let quiet = take_delta();
        assert_eq!(quiet.requests, 0, "{quiet:?}");
        assert!(
            snapshot().requests >= before.requests + 1,
            "cumulative kept"
        );
    }
}

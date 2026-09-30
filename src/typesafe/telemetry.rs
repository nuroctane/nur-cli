//! TypeSafe/Jev accounting for the current process.
//!
//! Every Jev request and every decision taken from a Jev answer lands here, so
//! the harness can show what the boost layer actually did: requests spent,
//! tokens billed, judgments used, escalations, and the frontier work it
//! replaced (compaction that never had to call a big model).
//!
//! Status counters are cumulative. Receipts drain only their owning session,
//! turn and route, including work propagated through blocking workers.

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

#[derive(Default)]
struct Counters {
    total: TypesafeTelemetry,
    last: TypesafeTelemetry,
}

impl Counters {
    #[cfg(test)]
    fn take_delta(&mut self) -> TypesafeTelemetry {
        let delta = delta_between(&self.total, &self.last);
        self.last = self.total.clone();
        delta
    }

    fn take(&mut self) -> TypesafeTelemetry {
        self.last = TypesafeTelemetry::default();
        std::mem::take(&mut self.total)
    }
}

fn queue() -> &'static Mutex<Counters> {
    static QUEUE: OnceLock<Mutex<Counters>> = OnceLock::new();
    QUEUE.get_or_init(|| Mutex::new(Counters::default()))
}

#[derive(Default)]
struct OwnerLedger {
    routes: std::collections::HashMap<(String, String), TypesafeTelemetry>,
    decisions: TypesafeTelemetry,
}
fn owners() -> &'static Mutex<std::collections::HashMap<(String, String), OwnerLedger>> {
    static OWNERS: OnceLock<Mutex<std::collections::HashMap<(String, String), OwnerLedger>>> =
        OnceLock::new();
    OWNERS.get_or_init(Default::default)
}

pub fn take_for_session(session_id: &str) -> Vec<TypesafeTelemetry> {
    let Some(owner) =
        super::context::current().map(|scope| (session_id.to_string(), scope.turn_id))
    else {
        return Vec::new();
    };
    let ledger = owners()
        .lock()
        .ok()
        .and_then(|mut owners| owners.remove(&owner));
    let Some(ledger) = ledger else {
        return Vec::new();
    };
    let mut out: Vec<_> = ledger.routes.into_values().collect();
    if !ledger.decisions.is_empty() {
        out.push(ledger.decisions);
    }
    out
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
    if super::context::check().is_err() {
        return;
    }
    let update = |t: &mut TypesafeTelemetry| {
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
    };
    if let Ok(mut counters) = queue().lock() {
        update(&mut counters.total);
    }
    if let Some(scope) = super::context::current().filter(|s| !s.session_id.is_empty()) {
        if let Ok(mut owners) = owners().lock() {
            update(
                owners
                    .entry((scope.session_id, scope.turn_id))
                    .or_default()
                    .routes
                    .entry((route.into(), model.into()))
                    .or_default(),
            );
        }
    }
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

fn mutate(mut f: impl FnMut(&mut TypesafeTelemetry)) {
    if super::context::check().is_err() {
        return;
    }
    if let Ok(mut g) = queue().lock() {
        f(&mut g.total);
    }
    if let Some(scope) = super::context::current().filter(|s| !s.session_id.is_empty()) {
        if let Ok(mut owners) = owners().lock() {
            f(&mut owners
                .entry((scope.session_id, scope.turn_id))
                .or_default()
                .decisions);
        }
    }
}

/// Cumulative counters without draining.
pub fn snapshot() -> TypesafeTelemetry {
    queue().lock().map(|g| g.total.clone()).unwrap_or_default()
}

#[cfg(test)]
fn delta_between(current: &TypesafeTelemetry, prev: &TypesafeTelemetry) -> TypesafeTelemetry {
    TypesafeTelemetry {
        route: current.route.clone(),
        model: current.model.clone(),
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
    queue().lock().map(|mut g| g.take()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_turns_keep_session_and_route_accounting_separate() {
        let session = uuid::Uuid::new_v4().to_string();
        let scope = |turn: &str| super::super::context::Scope {
            cancel: Default::default(),
            session_id: session.clone(),
            turn_id: turn.into(),
            deadline: None,
        };
        let a = scope("parent");
        let b = scope("child");
        let record = |owner, route, tokens| {
            super::super::context::with_sync(Some(owner), || {
                record_request(route, "model", tokens, 2, 1);
                record_decision(Some(0.9), true, 0.5);
            })
        };
        std::thread::scope(|workers| {
            workers.spawn(|| record(a.clone(), "http://127.0.0.1:8788/v1/systemone", 10));
            workers.spawn(|| record(b.clone(), "https://api.typesafe.ai/v1/systemone", 50));
        });
        let parent = super::super::context::with_sync(Some(a), || take_for_session(&session));
        let child = super::super::context::with_sync(Some(b), || take_for_session(&session));
        assert_eq!(parent.iter().map(|t| t.input_tokens).sum::<u64>(), 10);
        assert_eq!(child.iter().map(|t| t.input_tokens).sum::<u64>(), 50);
        assert!(parent
            .iter()
            .filter(|t| t.requests > 0)
            .all(|t| t.route.contains("127.0.0.1")));
        assert_eq!(parent.iter().map(|t| t.decisions).sum::<u64>(), 1);
        assert_eq!(child.iter().map(|t| t.decisions).sum::<u64>(), 1);
    }

    #[test]
    fn reset_clears_the_delta_baseline() {
        let mut counters = Counters::default();
        counters.total.requests = 10;
        assert_eq!(counters.take_delta().requests, 10);
        assert_eq!(counters.take().requests, 10);
        assert!(counters.total.is_empty());
        counters.total.requests = 1;
        assert_eq!(
            counters.take_delta().requests,
            1,
            "first request after reset must not disappear"
        );
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
        let mut counters = Counters {
            total: TypesafeTelemetry {
                route: "https://api.typesafe.ai/v1/systemone".into(),
                model: "jev-1.13.0".into(),
                requests: 1,
                input_tokens: 1_000,
                output_tokens: 10,
                questions: 2,
                ..TypesafeTelemetry::default()
            },
            ..Counters::default()
        };
        let delta = counters.take_delta();
        assert_eq!(delta.requests, 1);
        assert_eq!(delta.input_tokens, 1_000);
        assert!(counters.take_delta().is_empty());
        assert_eq!(counters.total.requests, 1, "cumulative kept");
        counters.total.requests += 1;
        counters.total.input_tokens += 500;
        let repeated = counters.take_delta();
        assert_eq!(repeated.requests, 1);
        assert_eq!(repeated.input_tokens, 500);
        assert_eq!(
            repeated.route, delta.route,
            "same endpoint is still provenance"
        );
        assert_eq!(repeated.model, delta.model);
    }
}

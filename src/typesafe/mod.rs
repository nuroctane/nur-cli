//! TypeSafe - System One judgments as programming primitives, wired through the
//! whole toolstack.
//!
//! TypeSafe's System One models (flagship: **Jev**) turn state plus *typed*
//! questions into typed answers: a [`Choice`](questions::Question::Choice) from
//! a code-supplied option set, a [`Noul`](questions::Question::Noul)
//! probability, a [`Score`](questions::Question::Score) on an ordered scale -
//! with `confidence` derived from the answer's probability distribution.
//! Docs: <https://docs.typesafe.ai>.
//!
//! The point of having it inside the harness rather than beside it: a whole
//! class of steps in an agent loop are not writing tasks, they are if
//! statements someone outsourced to a frontier model - which tool or model
//! next, is this result still needed, is this the third identical retry, does
//! this need a person. Those become typed questions, batched into far fewer
//! round trips and answered in milliseconds, while the expensive model is kept
//! for the work that actually needs writing.
//!
//! ## Where it is wired in
//!
//! | layer | module | what Jev decides |
//! |-------|--------|------------------|
//! | tool gate | [`harness::judge_calls`] (`JudgeScope::Pre`) | needs a human, risky, redundant, repeat-of-failure |
//! | tool results | [`harness::judge_calls`] (`JudgeScope::Verify`) | did it succeed (retention is judged later, at compaction) |
//! | compaction | [`compact`] | which tool calls/results to drop - survivors stay verbatim, no summary |
//! | skills | [`harness::select_requirements`], [`harness::judge_skill_use`] | which of a skill's stated rules apply, was the skill followed |
//! | routing | [`harness::pick_model`], [`route`] | cheapest adequate model on a *fresh* context; the parent session stays |
//! | retrieval | [`harness::rank_indices`] | which chunks are worth their tokens |
//! | escalation | [`policy`] | act / confirm / hand to a human, by confidence |
//!
//! Every one of them is provider-agnostic: the Jev key boosts whatever model the
//! session is already running, on any provider, because the judgments happen in
//! the harness, not in the model.
//!
//! ## Behavior with no key
//!
//! Nothing is required and nothing breaks. [`client::client`] reports
//! [`client::Availability::Unavailable`], every policy returns
//! [`policy::Judgment::unavailable`], and each call site keeps the behavior it
//! had before. There is no fallback that invents a judgment.

pub mod client;
pub mod compact;
pub mod harness;
pub mod policy;
pub mod questions;
pub mod route;
pub mod telemetry;

use crate::config::TypesafeConfig;

/// One-line status for `/typesafe`, the TUI chip and `nur doctor`.
pub fn status(cfg: &TypesafeConfig) -> String {
    let t = policy::Thresholds::from_config(cfg);
    let mut lines = Vec::new();
    let availability = client::client(cfg);
    if let Some(c) = availability.client() {
        let local = client::is_loopback_endpoint(c.base_url());
        lines.push(format!(
            "typesafe: ready{}{} · model {}",
            if local { " (local engine)" } else { "" },
            if c.key() == client::LOCAL_KEY_PLACEHOLDER {
                ", no key needed"
            } else {
                ""
            },
            c.model()
        ));
        lines.push(format!("endpoint: {}", c.base_url()));
        if let Some(p) = client::key_provenance(cfg) {
            lines.push(format!("key: {p}"));
        }
    } else if let Some(reason) = availability.reason() {
        lines.push(format!("typesafe: inactive - {reason}"));
    }
    debug_assert_eq!(availability.is_ready(), availability.client().is_some());
    lines.push(format!(
        "thresholds: act {:.2} · escalate {:.2} ({} · {})",
        t.act,
        t.escalate,
        if cfg.compaction.enabled {
            "compaction on"
        } else {
            "compaction off"
        },
        if cfg.tool_gate.enabled {
            "tool gate on"
        } else {
            "tool gate off"
        },
    ));
    let tel = telemetry::snapshot();
    lines.push(format!("this session: {}", tel.summary()));
    lines.join("\n")
}

/// Doctor block for `nur doctor`.
pub fn doctor_report(cfg: &TypesafeConfig) -> String {
    let mut lines = vec![status(cfg)];
    lines.push(format!(
        "knobs: enabled={} gate={} judge={} compaction={} skills={} routing={} \
         batch={} parallel={} timeout={}ms model={}",
        cfg.enabled,
        cfg.tool_gate.enabled,
        cfg.tool_gate.judge_results,
        cfg.compaction.enabled,
        cfg.skills.enabled,
        cfg.routing.enabled,
        cfg.max_questions_per_request,
        cfg.max_parallel,
        cfg.timeout_ms,
        if cfg.model.is_empty() {
            client::DEFAULT_MODEL
        } else {
            cfg.model.as_str()
        },
    ));
    lines.push(
        "docs: https://docs.typesafe.ai · model jev-latest is TypeSafe's flagship System One model"
            .into(),
    );
    lines.join("\n")
}

/// TUI chip text for this session's Jev activity, or `None` when idle.
///
/// Never drains - the chip is a read-out, not a consumer.
pub fn status_chip() -> Option<String> {
    let chip = telemetry::snapshot().chip();
    if chip.is_empty() {
        return None;
    }
    let text = format!("jev {chip}");
    // The footer is shared with the mode and session read-outs; keep the chip
    // bounded even on a long session.
    Some(text.chars().take(46).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_reports_inactive_without_a_key() {
        let cfg = TypesafeConfig {
            enabled: false,
            ..TypesafeConfig::default()
        };
        let s = status(&cfg);
        assert!(s.contains("inactive"), "{s}");
        assert!(s.contains("thresholds"), "{s}");
    }

    #[test]
    fn doctor_reports_the_knobs_and_docs() {
        let cfg = TypesafeConfig::default();
        let d = doctor_report(&cfg);
        assert!(d.contains("jev-latest"), "{d}");
        assert!(d.contains("docs.typesafe.ai"), "{d}");
    }

    #[test]
    fn ready_tracks_enabled_and_key_presence() {
        let off = TypesafeConfig {
            enabled: false,
            api_key: "k".into(),
            ..TypesafeConfig::default()
        };
        assert!(!harness::available(&off));
        let on = TypesafeConfig {
            enabled: true,
            api_key: "k".into(),
            ..TypesafeConfig::default()
        };
        assert!(harness::available(&on));
    }
}

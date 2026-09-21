//! Confidence policy: the part of the harness that decides whether a typed
//! answer is allowed to change behavior.
//!
//! The rule this module exists to enforce: **threshold on confidence, not on
//! the answer.** A Choice that picks `skip` with confidence 0.52 and one that
//! picks it with 0.97 are not the same evidence, and downstream code must not
//! treat them as such. So callers never read raw answers; they read
//! [`Judgment`], whose [`Judgment::usable`] hands back a value only when the
//! answer cleared the acting threshold and only when the judgment is a
//! *decision* (not an escalation).
//!
//! Three bands, from <https://docs.typesafe.ai/confidence>:
//!
//! | confidence | band | behavior |
//! |-----------|------|----------|
//! | `>= act` (0.85) | act | the harness may act on the answer |
//! | `>= escalate` (0.50) | confirm | too unsure to act alone - confirm, gather, or just report |
//! | `< escalate` | escalate | route to a bigger model or a human; do not act |
//!
//! Noul answers carry no upstream `confidence`; [`Answer::confidence`] derives
//! an equivalent distance from the coin flip so one policy gates all three
//! primitives.

use super::questions::Answer;
use crate::config::TypesafeConfig;

/// Confidence bands. All values are `0..=1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thresholds {
    /// Act on the answer without asking anyone.
    pub act: f64,
    /// Below this the answer is a suggestion, not a decision.
    pub escalate: f64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            act: 0.85,
            escalate: 0.5,
        }
    }
}

impl Thresholds {
    /// Read thresholds from `[typesafe]`, keeping them ordered and in range.
    pub fn from_config(cfg: &TypesafeConfig) -> Self {
        let act = cfg.act_confidence.clamp(0.0, 1.0);
        let escalate = cfg.escalate_confidence.clamp(0.0, act);
        Self { act, escalate }
    }

    /// Which band a confidence falls in. `None` (missing confidence) is treated
    /// as forbidden to act, at the escalation floor: an answer we cannot
    /// measure is not one we obey.
    pub fn band(&self, confidence: Option<f64>) -> GateAction {
        match confidence {
            None => GateAction::Escalate,
            Some(c) if c >= self.act => GateAction::Act,
            Some(c) if c >= self.escalate => GateAction::Confirm,
            Some(_) => GateAction::Escalate,
        }
    }

    /// Higher-stakes actions need a higher bar: `act` scaled into
    /// `[act, 1.0]` by `stakes` in `0..=1` (0 = harmless read, 1 = irreversible).
    pub fn scaled(&self, stakes: f64) -> Self {
        let stakes = stakes.clamp(0.0, 1.0);
        Self {
            act: self.act + (1.0 - self.act) * stakes,
            escalate: self.escalate,
        }
    }
}

/// What the harness is allowed to do with a judgment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateAction {
    /// Act on it.
    Act,
    /// Too unsure to act alone - confirm with the user, gather evidence, or
    /// report without acting.
    Confirm,
    /// Hand off to a bigger model or a human.
    Escalate,
}

impl GateAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Act => "act",
            Self::Confirm => "confirm",
            Self::Escalate => "escalate",
        }
    }
}

/// A typed answer plus the policy verdict on whether it may change behavior.
#[derive(Debug, Clone, PartialEq)]
pub struct Judgment<T> {
    /// The question id this came from.
    pub question: String,
    /// The answer's value, mapped back onto code-supplied candidates.
    pub value: Option<T>,
    /// Confidence of the answer (`None` when the API returned none).
    pub confidence: Option<f64>,
    /// Band the confidence landed in.
    pub action: GateAction,
    /// True when the answer exists but is below the escalation floor, or was
    /// unmeasurable: the caller should route to a bigger model or a human.
    pub escalated: bool,
    /// Extra context for the transcript/UI (never acted on).
    pub notes: Vec<String>,
}

impl<T> Judgment<T> {
    /// No judgment was possible (no key, transport failure, malformed answer).
    /// The caller must use its own default.
    pub fn unavailable(question: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            question: question.into(),
            value: None,
            confidence: None,
            action: GateAction::Escalate,
            escalated: true,
            notes: vec![reason.into()],
        }
    }

    /// Build from an answer and the thresholds.
    pub fn from_answer(
        question: impl Into<String>,
        value: Option<T>,
        confidence: Option<f64>,
        thresholds: &Thresholds,
    ) -> Self {
        let action = thresholds.band(confidence);
        let escalated = action == GateAction::Escalate;
        let mut notes = Vec::new();
        if escalated {
            notes.push(match confidence {
                Some(c) => format!("confidence {c:.2} below the {} floor", thresholds.escalate),
                None => "the answer carried no confidence".to_string(),
            });
        }
        Self {
            question: question.into(),
            value,
            confidence,
            action,
            escalated,
            notes,
        }
    }

    /// The value, only when acting on it is permitted.
    ///
    /// This is the single choke point: `Confirm` and `Escalate` both return
    /// `None`, so no caller can accidentally obey an uncertain answer.
    pub fn usable(&self) -> Option<&T> {
        if self.action == GateAction::Act {
            self.value.as_ref()
        } else {
            None
        }
    }

    /// The raw value regardless of band. Use for display and telemetry only.
    pub fn raw(&self) -> Option<&T> {
        self.value.as_ref()
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Judgment<U> {
        Judgment {
            question: self.question,
            value: self.value.map(f),
            confidence: self.confidence,
            action: self.action,
            escalated: self.escalated,
            notes: self.notes,
        }
    }

    /// One-line description for the TUI/status line.
    pub fn describe(&self) -> String {
        let conf = match self.confidence {
            Some(c) => format!("{c:.2}"),
            None => "-".to_string(),
        };
        let mut s = format!("{} {} conf {}", self.question, self.action.as_str(), conf);
        if !self.notes.is_empty() {
            s.push_str(" (");
            s.push_str(&self.notes.join("; "));
            s.push(')');
        }
        s
    }
}

/// A Noul answer turned into a boolean plus the policy verdict.
///
/// `yes_at` is where the probability counts as yes (0.5 = the coin flip the
/// upstream docs call out). `thresholds` decides whether the boolean may be
/// acted on: a Noul sitting near 0.5 has low certainty and escalates.
pub fn noul_decision(
    answer: &Answer,
    question: impl Into<String>,
    yes_at: f64,
    thresholds: &Thresholds,
) -> Judgment<bool> {
    let question = question.into();
    let Some(p) = answer.noul() else {
        return Judgment::unavailable(question, "answer was not a noul");
    };
    let mut j = Judgment::from_answer(question, Some(p >= yes_at), answer.confidence(), thresholds);
    j.notes.push(format!("p(yes)={p:.2}"));
    j
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typesafe::questions::Answer;

    fn thresholds() -> Thresholds {
        Thresholds {
            act: 0.85,
            escalate: 0.5,
        }
    }

    #[test]
    fn bands_follow_the_documented_three_ranges() {
        let t = thresholds();
        assert_eq!(t.band(Some(0.97)), GateAction::Act);
        assert_eq!(t.band(Some(0.85)), GateAction::Act);
        assert_eq!(t.band(Some(0.7)), GateAction::Confirm);
        assert_eq!(t.band(Some(0.5)), GateAction::Confirm);
        assert_eq!(t.band(Some(0.49)), GateAction::Escalate);
        assert_eq!(t.band(Some(0.0)), GateAction::Escalate);
    }

    #[test]
    fn an_unmeasurable_answer_may_never_be_acted_on() {
        assert_eq!(thresholds().band(None), GateAction::Escalate);
        let j: Judgment<bool> = Judgment::from_answer("q", Some(true), None, &thresholds());
        assert!(j.escalated);
        assert_eq!(j.usable(), None);
        assert_eq!(j.raw(), Some(&true));
    }

    #[test]
    fn usable_is_gated_on_confidence_not_on_the_answer() {
        let t = thresholds();
        // Same answer, two confidences: only the confident one is usable.
        let confident = Judgment::from_answer("q", Some("skip".to_string()), Some(0.95), &t);
        let unsure = Judgment::from_answer("q", Some("skip".to_string()), Some(0.6), &t);
        assert_eq!(confident.usable().map(String::as_str), Some("skip"));
        assert_eq!(unsure.usable(), None);
        assert_eq!(unsure.raw().map(String::as_str), Some("skip"));
        assert_eq!(unsure.action, GateAction::Confirm);
        assert!(!unsure.escalated);

        let hopeless = Judgment::from_answer("q", Some("skip".to_string()), Some(0.2), &t);
        assert!(hopeless.escalated);
        assert!(hopeless.describe().contains("escalate"));
    }

    #[test]
    fn stakes_raise_the_acting_bar() {
        let t = thresholds();
        let irreversible = t.scaled(1.0);
        assert_eq!(irreversible.act, 1.0);
        assert_eq!(irreversible.band(Some(0.95)), GateAction::Confirm);
        let harmless = t.scaled(0.0);
        assert_eq!(harmless.act, 0.85);
        let middling = t.scaled(0.5);
        assert!(middling.act > 0.85 && middling.act < 1.0);
        assert_eq!(middling.escalate, 0.5);
    }

    #[test]
    fn config_keeps_thresholds_ordered_and_in_range() {
        let cfg = TypesafeConfig {
            act_confidence: 1.4,
            escalate_confidence: 0.9,
            ..TypesafeConfig::default()
        };
        let t = Thresholds::from_config(&cfg);
        assert_eq!(t.act, 1.0);
        assert_eq!(t.escalate, 0.9, "escalate clamps below act");
        // A negative escalate is clamped to 0.
        let cfg = TypesafeConfig {
            act_confidence: 0.8,
            escalate_confidence: -3.0,
            ..TypesafeConfig::default()
        };
        assert_eq!(Thresholds::from_config(&cfg).escalate, 0.0);
    }

    #[test]
    fn noul_confidence_comes_from_distance_to_the_coin_flip() {
        let t = thresholds();
        let a = Answer::from_json(&serde_json::json!({"type":"noul","noul":0.95})).unwrap();
        let j = noul_decision(&a, "q", 0.5, &t);
        assert_eq!(j.value, Some(true));
        assert!((j.confidence.unwrap() - 0.9).abs() < 1e-9);
        assert_eq!(j.action, GateAction::Act);
        assert!(j.notes.iter().any(|n| n.contains("p(yes)=0.95")));

        let coin = Answer::from_json(&serde_json::json!({"type":"noul","noul":0.51})).unwrap();
        let j = noul_decision(&coin, "q", 0.5, &t);
        assert_eq!(j.action, GateAction::Escalate);
        assert_eq!(j.usable(), None);
    }
}

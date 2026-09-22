//! Typed System One questions and their answers.
//!
//! TypeSafe's three primitives, modelled as Rust types so the harness never
//! builds a judgment out of prose:
//!
//! | primitive | shape | what nur uses it for |
//! |-----------|-------|----------------------|
//! | [`Question::Noul`] | one probability in `0..=1` | yes/no gate, "does this need a human", "did this succeed" |
//! | [`Question::Choice`] | one of a **code-supplied** option set | tool/skill/model routing, label classification |
//! | [`Question::Score`] | probability-weighted level on an ordered scale | graded ranking, risk, relevance |
//!
//! Two rules are enforced here rather than trusted to callers:
//!
//! 1. **Options come from code.** [`Question::Choice`] refuses to serialize an
//!    empty option set, and [`Answer::resolve_verbatim`] returns the caller's
//!    own `&str` (never a model-authored string), so a judgment can only
//!    *select* a value the harness already had.
//! 2. **Questions are bounded.** Choice takes up to [`MAX_CHOICE_OPTIONS`]
//!    (255) options; Score takes [`MIN_SCORE_LEVELS`]..=[`MAX_SCORE_LEVELS`]
//!    (2..=10) ordered levels. Both limits are upstream contract limits, checked
//!    before any request is spent.
//!
//! References: <https://docs.typesafe.ai/primitives>, `primitives/choice`,
//! `primitives/score`, `primitives/noul`, `confidence`.

use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

/// Upstream limit for a Choice question's option map.
pub const MAX_CHOICE_OPTIONS: usize = 255;
/// A Score needs at least two levels to be a scale at all.
pub const MIN_SCORE_LEVELS: usize = 2;
/// Upstream limit for a Score question's level array.
pub const MAX_SCORE_LEVELS: usize = 10;

/// Option label used when the harness wants "nothing here fits" to be a legal
/// answer. Callers that add it should treat a pick of it as `None`.
pub const NO_MATCH: &str = "none_of_these";

/// One code-supplied Choice option: the literal value plus an optional rubric.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceOption {
    /// The literal value the harness will act on (returned verbatim).
    pub label: String,
    /// Optional description of when this option applies.
    pub rubric: Option<String>,
}

impl ChoiceOption {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            rubric: None,
        }
    }

    pub fn described(label: impl Into<String>, rubric: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            rubric: Some(rubric.into()),
        }
    }
}

/// Optional descriptions of what a yes and a no mean for a Noul question.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoulCriteria {
    pub yes: Option<String>,
    pub no: Option<String>,
}

/// A malformed question, caught before any request is spent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuestionError {
    EmptyInstructions,
    NoOptions,
    TooManyOptions(usize),
    DuplicateOption(String),
    TooFewLevels(usize),
    TooManyLevels(usize),
    EmptyLevel(usize),
}

impl std::fmt::Display for QuestionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyInstructions => write!(f, "question instructions must not be empty"),
            Self::NoOptions => write!(f, "choice needs at least one option"),
            Self::TooManyOptions(n) => write!(
                f,
                "choice has {n} options; TypeSafe allows at most {MAX_CHOICE_OPTIONS}"
            ),
            Self::DuplicateOption(o) => write!(f, "choice option {o:?} is duplicated"),
            Self::TooFewLevels(n) => write!(
                f,
                "score has {n} levels; TypeSafe needs at least {MIN_SCORE_LEVELS}"
            ),
            Self::TooManyLevels(n) => write!(
                f,
                "score has {n} levels; TypeSafe allows at most {MAX_SCORE_LEVELS}"
            ),
            Self::EmptyLevel(i) => write!(f, "score level {i} is empty"),
        }
    }
}

/// A typed System One question.
#[derive(Debug, Clone, PartialEq)]
pub enum Question {
    /// Probability the answer is yes. No separate confidence is returned.
    Noul {
        instructions: Value,
        criteria: Option<NoulCriteria>,
    },
    /// Pick one of a code-supplied option set.
    Choice {
        instructions: Value,
        options: Vec<ChoiceOption>,
    },
    /// Probability-weighted position on an ordered, described scale.
    Score {
        instructions: Value,
        levels: Vec<String>,
    },
}

impl Question {
    /// A yes/no question. `instructions` must carry the whole meaning.
    pub fn noul(instructions: impl Into<String>) -> Self {
        Self::Noul {
            instructions: Value::String(instructions.into()),
            criteria: None,
        }
    }

    /// A yes/no question with explicit yes/no descriptions.
    pub fn noul_with(instructions: impl Into<String>, criteria: NoulCriteria) -> Self {
        Self::Noul {
            instructions: Value::String(instructions.into()),
            criteria: Some(criteria),
        }
    }

    /// A yes/no question whose instructions are already structured JSON.
    pub fn noul_value(instructions: Value, criteria: Option<NoulCriteria>) -> Self {
        Self::Noul {
            instructions,
            criteria,
        }
    }

    /// A pick from code-supplied candidates.
    pub fn choice(instructions: impl Into<String>, options: Vec<ChoiceOption>) -> Self {
        Self::Choice {
            instructions: Value::String(instructions.into()),
            options,
        }
    }

    /// A pick from plain candidate labels (no per-option rubric).
    pub fn choice_of(instructions: impl Into<String>, labels: &[String]) -> Self {
        Self::Choice {
            instructions: Value::String(instructions.into()),
            options: labels.iter().cloned().map(ChoiceOption::new).collect(),
        }
    }

    /// A graded scale. `levels` are ordered low → high.
    pub fn score(instructions: impl Into<String>, levels: Vec<String>) -> Self {
        Self::Score {
            instructions: Value::String(instructions.into()),
            levels,
        }
    }

    /// `noul` / `choice` / `score`.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Noul { .. } => "noul",
            Self::Choice { .. } => "choice",
            Self::Score { .. } => "score",
        }
    }

    /// Rebuild a question from its wire shape ([`Question::to_json`] output).
    /// Eval replay needs this: records store exactly what was sent, and a
    /// replay must ask the same questions again. Round-trip is pinned by test
    /// (choice option order is canonicalized - the wire map sorts keys - and
    /// order never matters since candidates resolve by label).
    pub fn from_wire_json(v: &Value) -> Result<Self, String> {
        let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
        let instructions = v.get("instructions").cloned().unwrap_or(Value::Null);
        let q = match kind {
            "noul" => {
                let criteria = v
                    .get("criteria")
                    .and_then(Value::as_object)
                    .map(|c| NoulCriteria {
                        yes: c.get("true").and_then(Value::as_str).map(str::to_string),
                        no: c.get("false").and_then(Value::as_str).map(str::to_string),
                    });
                Self::Noul {
                    instructions,
                    criteria,
                }
            }
            "choice" => {
                let obj = v
                    .get("criteria")
                    .and_then(Value::as_object)
                    .ok_or_else(|| "choice record has no criteria map".to_string())?;
                let options = obj
                    .iter()
                    .map(|(label, rubric)| ChoiceOption {
                        label: label.clone(),
                        rubric: rubric.as_str().map(str::to_string),
                    })
                    .collect::<Vec<_>>();
                Self::Choice {
                    instructions,
                    options,
                }
            }
            "score" => {
                let levels = v
                    .get("criteria")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(|l| l.as_str().map(str::to_string))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                Self::Score {
                    instructions,
                    levels,
                }
            }
            other => return Err(format!("record has unknown question type {other:?}")),
        };
        q.validate().map_err(|e| e.to_string())?;
        Ok(q)
    }

    /// Reject malformed questions before spending a request.
    pub fn validate(&self) -> Result<(), QuestionError> {
        let instructions_empty = match self {
            Self::Noul { instructions, .. }
            | Self::Choice { instructions, .. }
            | Self::Score { instructions, .. } => json_is_empty(instructions),
        };
        if instructions_empty {
            return Err(QuestionError::EmptyInstructions);
        }
        match self {
            Self::Noul { .. } => Ok(()),
            Self::Choice { options, .. } => {
                if options.is_empty() {
                    return Err(QuestionError::NoOptions);
                }
                if options.len() > MAX_CHOICE_OPTIONS {
                    return Err(QuestionError::TooManyOptions(options.len()));
                }
                let mut seen: Vec<&str> = Vec::with_capacity(options.len());
                for o in options {
                    if o.label.trim().is_empty() {
                        return Err(QuestionError::DuplicateOption(o.label.clone()));
                    }
                    if seen.contains(&o.label.as_str()) {
                        return Err(QuestionError::DuplicateOption(o.label.clone()));
                    }
                    seen.push(&o.label);
                }
                Ok(())
            }
            Self::Score { levels, .. } => {
                if levels.len() < MIN_SCORE_LEVELS {
                    return Err(QuestionError::TooFewLevels(levels.len()));
                }
                if levels.len() > MAX_SCORE_LEVELS {
                    return Err(QuestionError::TooManyLevels(levels.len()));
                }
                for (i, l) in levels.iter().enumerate() {
                    if l.trim().is_empty() {
                        return Err(QuestionError::EmptyLevel(i));
                    }
                }
                Ok(())
            }
        }
    }

    /// Wire shape for `POST /v1/systemone`.
    pub fn to_json(&self) -> Value {
        match self {
            Self::Noul {
                instructions,
                criteria,
            } => {
                let mut obj = Map::new();
                obj.insert("type".into(), Value::String("noul".into()));
                obj.insert("instructions".into(), instructions.clone());
                if let Some(c) = criteria {
                    let mut crit = Map::new();
                    if let Some(y) = &c.yes {
                        crit.insert("true".into(), Value::String(y.clone()));
                    }
                    if let Some(n) = &c.no {
                        crit.insert("false".into(), Value::String(n.clone()));
                    }
                    if !crit.is_empty() {
                        obj.insert("criteria".into(), Value::Object(crit));
                    }
                }
                Value::Object(obj)
            }
            Self::Choice {
                instructions,
                options,
            } => {
                let mut crit = Map::new();
                for o in options {
                    crit.insert(
                        o.label.clone(),
                        match &o.rubric {
                            Some(r) => Value::String(r.clone()),
                            None => Value::Null,
                        },
                    );
                }
                json!({
                    "type": "choice",
                    "instructions": instructions,
                    "criteria": Value::Object(crit),
                })
            }
            Self::Score {
                instructions,
                levels,
            } => json!({
                "type": "score",
                "instructions": instructions,
                "criteria": levels,
            }),
        }
    }

    /// The code-supplied candidate labels this question can answer with.
    pub fn candidate_labels(&self) -> Vec<String> {
        match self {
            Self::Choice { options, .. } => options.iter().map(|o| o.label.clone()).collect(),
            _ => Vec::new(),
        }
    }
}

fn json_is_empty(v: &Value) -> bool {
    match v {
        Value::String(s) => s.trim().is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        Value::Null => true,
        _ => false,
    }
}

/// A typed answer, as returned under the question's id.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    Noul {
        noul: f64,
    },
    Choice {
        choice: String,
        probabilities: BTreeMap<String, f64>,
        confidence: f64,
    },
    Score {
        score: f64,
        legend: BTreeMap<String, String>,
        probabilities: BTreeMap<String, f64>,
        confidence: f64,
    },
}

/// Why an answer body could not be read as a typed answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnswerError(pub String);

impl std::fmt::Display for AnswerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Answer {
    /// Parse one entry of the response `answers` map.
    pub fn from_json(v: &Value) -> Result<Self, AnswerError> {
        let kind = v
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| AnswerError("answer has no type".into()))?;
        match kind {
            "noul" => {
                let noul = v
                    .get("noul")
                    .and_then(Value::as_f64)
                    .ok_or_else(|| AnswerError("noul answer has no noul value".into()))?;
                Ok(Self::Noul {
                    noul: noul.clamp(0.0, 1.0),
                })
            }
            "choice" => {
                let choice = v
                    .get("choice")
                    .and_then(Value::as_str)
                    .ok_or_else(|| AnswerError("choice answer has no choice".into()))?
                    .to_string();
                Ok(Self::Choice {
                    choice,
                    probabilities: read_probabilities(v.get("probabilities")),
                    confidence: v
                        .get("confidence")
                        .and_then(Value::as_f64)
                        .unwrap_or(f64::NAN),
                })
            }
            "score" => {
                let score = v
                    .get("score")
                    .and_then(Value::as_f64)
                    .ok_or_else(|| AnswerError("score answer has no score".into()))?;
                let mut legend = BTreeMap::new();
                if let Some(obj) = v.get("legend").and_then(Value::as_object) {
                    for (k, val) in obj {
                        legend.insert(k.clone(), val.as_str().unwrap_or_default().to_string());
                    }
                }
                Ok(Self::Score {
                    score,
                    legend,
                    probabilities: read_probabilities(v.get("probabilities")),
                    confidence: v
                        .get("confidence")
                        .and_then(Value::as_f64)
                        .unwrap_or(f64::NAN),
                })
            }
            other => Err(AnswerError(format!("unknown answer type {other:?}"))),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::Noul { .. } => "noul",
            Self::Choice { .. } => "choice",
            Self::Score { .. } => "score",
        }
    }

    /// Serialize one entry of the response `answers` map, mirroring
    /// [`Answer::from_json`]. Used for eval records (record now, replay
    /// later); `from_json(to_json(a)) == a` is pinned by test.
    pub fn to_json(&self) -> Value {
        match self {
            Self::Noul { noul } => serde_json::json!({"type": "noul", "noul": noul}),
            Self::Choice {
                choice,
                probabilities,
                confidence,
            } => serde_json::json!({
                "type": "choice",
                "choice": choice,
                "probabilities": probabilities,
                "confidence": confidence,
            }),
            Self::Score {
                score,
                legend,
                probabilities,
                confidence,
            } => serde_json::json!({
                "type": "score",
                "score": score,
                "legend": legend,
                "probabilities": probabilities,
                "confidence": confidence,
            }),
        }
    }

    /// Upstream `confidence` for Choice/Score.
    ///
    /// Noul answers carry none by design ("no separate confidence"), so nur
    /// derives an equivalent distance from the coin flip: `|p - 0.5| * 2`.
    /// That is nur's measure, not TypeSafe's - it exists so one threshold
    /// policy can gate all three primitives. `None` for a missing value.
    pub fn confidence(&self) -> Option<f64> {
        match self {
            Self::Noul { noul } => Some(((noul - 0.5).abs() * 2.0).clamp(0.0, 1.0)),
            Self::Choice { confidence, .. } | Self::Score { confidence, .. } => {
                if confidence.is_nan() {
                    None
                } else {
                    Some(confidence.clamp(0.0, 1.0))
                }
            }
        }
    }

    /// `Some(p)` only for Noul answers.
    pub fn noul(&self) -> Option<f64> {
        match self {
            Self::Noul { noul } => Some(*noul),
            _ => None,
        }
    }

    /// `Some(choice)` only for Choice answers. Use
    /// [`Answer::resolve_verbatim`] to map it back onto a code candidate.
    pub fn choice(&self) -> Option<&str> {
        match self {
            Self::Choice { choice, .. } => Some(choice.as_str()),
            _ => None,
        }
    }

    /// `Some(score)` only for Score answers.
    pub fn score(&self) -> Option<f64> {
        match self {
            Self::Score { score, .. } => Some(*score),
            _ => None,
        }
    }

    /// Map a Choice answer back onto the caller's own candidate list.
    ///
    /// Exact match first, then a trimmed/case-insensitive match. A model token
    /// that matches no candidate returns `None` - the harness acts on its own
    /// values or not at all, never on an invented option.
    pub fn resolve_verbatim<'a>(&self, candidates: &'a [String]) -> Option<&'a String> {
        let picked = self.choice()?;
        if let Some(c) = candidates.iter().find(|c| c.as_str() == picked) {
            return Some(c);
        }
        let needle = picked.trim().to_ascii_lowercase();
        candidates
            .iter()
            .find(|c| c.trim().to_ascii_lowercase() == needle)
    }

    /// Highest-probability option for a Choice answer (fallback when a caller
    /// wants the distribution rather than the point pick).
    pub fn argmax(&self) -> Option<&str> {
        self.probabilities()
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(k, _)| k.as_str())
    }

    /// The answer's probability distribution (empty for Noul, which carries
    /// only its `p(yes)`).
    pub fn probabilities(&self) -> &BTreeMap<String, f64> {
        static EMPTY: std::sync::OnceLock<BTreeMap<String, f64>> = std::sync::OnceLock::new();
        match self {
            Self::Choice { probabilities, .. } | Self::Score { probabilities, .. } => probabilities,
            Self::Noul { .. } => EMPTY.get_or_init(BTreeMap::new),
        }
    }
}

fn read_probabilities(v: Option<&Value>) -> BTreeMap<String, f64> {
    let mut out = BTreeMap::new();
    if let Some(obj) = v.and_then(Value::as_object) {
        for (k, val) in obj {
            if let Some(f) = val.as_f64() {
                out.insert(k.clone(), f);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choice_serializes_code_options_with_rubrics() {
        let q = Question::choice(
            "Which tool should run next?",
            vec![
                ChoiceOption::described("read_file", "inspect a file"),
                ChoiceOption::new("grep"),
            ],
        );
        q.validate().unwrap();
        let j = q.to_json();
        assert_eq!(j["type"], "choice");
        assert_eq!(j["criteria"]["read_file"], "inspect a file");
        assert!(j["criteria"]["grep"].is_null());
        assert_eq!(q.candidate_labels(), vec!["read_file", "grep"]);
    }

    #[test]
    fn question_limits_are_upstream_limits() {
        let too_many: Vec<ChoiceOption> = (0..=MAX_CHOICE_OPTIONS)
            .map(|i| ChoiceOption::new(format!("o{i}")))
            .collect();
        assert_eq!(
            Question::choice("pick", too_many).validate(),
            Err(QuestionError::TooManyOptions(MAX_CHOICE_OPTIONS + 1))
        );
        assert_eq!(
            Question::choice("pick", vec![ChoiceOption::new("a"), ChoiceOption::new("a")])
                .validate(),
            Err(QuestionError::DuplicateOption("a".into()))
        );
        assert_eq!(
            Question::score("rate", vec!["only".into()]).validate(),
            Err(QuestionError::TooFewLevels(1))
        );
        let eleven: Vec<String> = (0..=MAX_SCORE_LEVELS).map(|i| format!("l{i}")).collect();
        assert_eq!(
            Question::score("rate", eleven).validate(),
            Err(QuestionError::TooManyLevels(MAX_SCORE_LEVELS + 1))
        );
    }

    #[test]
    fn score_serializes_ordered_levels() {
        let q = Question::score(
            "How risky is this diff?",
            vec!["safe".into(), "risky".into()],
        );
        q.validate().unwrap();
        assert_eq!(q.to_json()["criteria"][1], "risky");
    }

    #[test]
    fn noul_criteria_only_carries_what_was_given() {
        let q = Question::noul_with(
            "Is this still needed?",
            NoulCriteria {
                yes: Some("still relevant".into()),
                no: None,
            },
        );
        let j = q.to_json();
        assert_eq!(j["criteria"]["true"], "still relevant");
        assert!(j["criteria"].get("false").is_none());
    }

    #[test]
    fn empty_instructions_rejected() {
        assert_eq!(
            Question::noul("   ").validate(),
            Err(QuestionError::EmptyInstructions)
        );
    }

    #[test]
    fn wire_round_trip_is_lossless() {
        let cases = vec![
            Question::noul_with(
                "Is this still needed?",
                NoulCriteria {
                    yes: Some("still relevant".into()),
                    no: None,
                },
            ),
            Question::choice(
                "Which tool next?",
                vec![
                    ChoiceOption::described("read_file", "inspect a file"),
                    ChoiceOption::new("grep"),
                ],
            ),
            Question::score("How risky?", vec!["safe".into(), "risky".into()]),
        ];
        for q in cases {
            let mut back = Question::from_wire_json(&q.to_json()).unwrap();
            // The wire criteria map sorts option keys canonically; order
            // never matters (candidates resolve by label), so compare sorted.
            if let Question::Choice { options, .. } = &mut back {
                options.sort_by(|a, b| a.label.cmp(&b.label));
            }
            let mut want = q.clone();
            if let Question::Choice { options, .. } = &mut want {
                options.sort_by(|a, b| a.label.cmp(&b.label));
            }
            assert_eq!(back, want);
        }
        assert!(Question::from_wire_json(&serde_json::json!({"type":"nope"})).is_err());
    }

    #[test]
    fn answer_round_trip_is_lossless() {
        let cases = vec![
            Answer::Noul { noul: 0.92 },
            Answer::Choice {
                choice: "read_file".into(),
                probabilities: [("read_file".into(), 0.9), ("grep".into(), 0.1)]
                    .into_iter()
                    .collect(),
                confidence: 0.81,
            },
            Answer::Score {
                score: 1.6,
                legend: [("0".into(), "safe".into()), ("1".into(), "risky".into())]
                    .into_iter()
                    .collect(),
                probabilities: [("0".into(), 0.1), ("1".into(), 0.9)].into_iter().collect(),
                confidence: 0.7,
            },
        ];
        for a in cases {
            let back = Answer::from_json(&a.to_json()).unwrap();
            assert_eq!(back, a);
        }
    }

    #[test]
    fn answers_parse_and_confidence_is_derived_for_noul() {
        let a = Answer::from_json(&serde_json::json!({"type":"noul","noul":0.92})).unwrap();
        assert_eq!(a.noul(), Some(0.92));
        assert!((a.confidence().unwrap() - 0.84).abs() < 1e-9);
        let coin = Answer::from_json(&serde_json::json!({"type":"noul","noul":0.5})).unwrap();
        assert_eq!(coin.confidence(), Some(0.0));

        let c = Answer::from_json(&serde_json::json!({
            "type":"choice","choice":"read_file",
            "probabilities":{"read_file":0.9,"grep":0.1},"confidence":0.81
        }))
        .unwrap();
        assert_eq!(c.choice(), Some("read_file"));
        assert_eq!(c.confidence(), Some(0.81));

        let s = Answer::from_json(&serde_json::json!({
            "type":"score","score":1.6,"legend":{"0":"safe","1":"risky"},
            "probabilities":{"0":0.1,"1":0.9},"confidence":0.7
        }))
        .unwrap();
        assert_eq!(s.score(), Some(1.6));
        match &s {
            Answer::Score { legend, .. } => {
                assert_eq!(legend.get("1").map(String::as_str), Some("risky"))
            }
            other => panic!("expected a score answer, got {other:?}"),
        }
    }

    #[test]
    fn verbatim_resolution_never_invents_an_option() {
        let candidates = vec!["read_file".to_string(), "grep".to_string()];
        let ok = Answer::from_json(&serde_json::json!({
            "type":"choice","choice":"Grep","probabilities":{},"confidence":0.9
        }))
        .unwrap();
        assert_eq!(
            ok.resolve_verbatim(&candidates).map(String::as_str),
            Some("grep")
        );

        let invented = Answer::from_json(&serde_json::json!({
            "type":"choice","choice":"run_shell_script","probabilities":{},"confidence":0.9
        }))
        .unwrap();
        assert_eq!(invented.resolve_verbatim(&candidates), None);
    }

    #[test]
    fn malformed_answers_error_instead_of_defaulting() {
        assert!(Answer::from_json(&serde_json::json!({"type":"choice"})).is_err());
        assert!(Answer::from_json(&serde_json::json!({"type":"score"})).is_err());
        assert!(Answer::from_json(&serde_json::json!({"type":"noul"})).is_err());
        assert!(Answer::from_json(&serde_json::json!({"noul":0.5})).is_err());
        let missing_conf = Answer::from_json(
            &serde_json::json!({"type":"choice","choice":"a","probabilities":{}}),
        )
        .unwrap();
        assert_eq!(missing_conf.confidence(), None);
    }
}

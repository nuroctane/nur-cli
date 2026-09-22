//! Jev eval sets: record live judgments, replay them later.
//!
//! AgentRun's edge is measured iteration: dev sets for tuning, reserved sets
//! for confirmation, replay without re-running the agent. This module is nur's
//! version of that loop for the judgment layer:
//!
//! - **Record**: with `NUR_JEV_RECORD=<set>` in the environment, every batched
//!   ask appends one JSONL line (`state`, `questions`, `answers`, model,
//!   usage) to `~/.nur/jev/evals/<set>.jsonl`. Off by default; one env check
//!   per ask, so the hot path costs nothing when unset.
//! - **Replay**: `nur jev eval --set <name>` re-asks every record through the
//!   *currently configured* layer (hosted key or local engine) and reports
//!   agreement with the recorded answers, accuracy against hand-added
//!   `expected` labels where present, and what it cost.
//! - **Reserved sets**: `--reserved <other>` replays a second, held-out set
//!   the same way. Tune wording and thresholds on dev; confirm on reserved.
//!   A record promoted into a set can carry an `expected` map
//!   (`{qid: {"choice": ...} | {"noul": true} | {"score": n}}`) for
//!   correctness grading instead of mere stability.
//!
//! Agreement is per primitive: same choice pick, same side of the 0.5 coin
//! flip for noul, score within 0.5 of a level. Stability is not correctness -
//! a replay that agrees 100% with a bad reference only proves the layer is
//! deterministic - but a wording change that drops agreement from 97% to 60%
//! is a regression caught before it ships.

use crate::config::nur_home;
use crate::typesafe::client::TypesafeClient;
use crate::typesafe::questions::{Answer, Question};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// One recorded ask: everything needed to replay it.
#[derive(Debug, Clone)]
pub struct EvalRecord {
    pub state: Value,
    pub questions: BTreeMap<String, Question>,
    pub answers: BTreeMap<String, Answer>,
    pub model: String,
    pub expected: BTreeMap<String, Value>,
}

impl EvalRecord {
    fn to_json(&self) -> Value {
        let questions: BTreeMap<String, Value> = self
            .questions
            .iter()
            .map(|(id, q)| (id.clone(), q.to_json()))
            .collect();
        let answers: BTreeMap<String, Value> = self
            .answers
            .iter()
            .map(|(id, a)| (id.clone(), a.to_json()))
            .collect();
        serde_json::json!({
            "model": self.model,
            "state": self.state,
            "questions": questions,
            "answers": answers,
            "expected": self.expected,
        })
    }

    fn from_json(v: &Value) -> Result<Self, String> {
        let model = v
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let state = v.get("state").cloned().unwrap_or(Value::Null);
        let questions = v
            .get("questions")
            .and_then(Value::as_object)
            .map(|o| {
                o.iter()
                    .map(|(id, q)| Question::from_wire_json(q).map(|qq| (id.clone(), qq)))
                    .collect::<Result<BTreeMap<_, _>, _>>()
            })
            .transpose()?
            .unwrap_or_default();
        if questions.is_empty() {
            return Err("eval record has no questions".into());
        }
        let answers = v
            .get("answers")
            .and_then(Value::as_object)
            .map(|o| {
                o.iter()
                    .map(|(id, a)| Answer::from_json(a).map(|aa| (id.clone(), aa)))
                    .collect::<Result<BTreeMap<_, _>, _>>()
            })
            .transpose()
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        let expected = v
            .get("expected")
            .and_then(Value::as_object)
            .map(|o| o.iter().map(|(k, val)| (k.clone(), val.clone())).collect())
            .unwrap_or_default();
        Ok(Self {
            state,
            questions,
            answers,
            model,
            expected,
        })
    }
}

/// Where eval sets live. `NUR_JEV_EVAL_DIR` overrides for tests.
pub fn evals_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("NUR_JEV_EVAL_DIR") {
        let dir = dir.trim();
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    nur_home().join("jev").join("evals")
}

fn set_path(name: &str) -> Option<PathBuf> {
    let safe: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if safe.is_empty() || safe.len() > 64 {
        return None;
    }
    Some(evals_dir().join(format!("{safe}.jsonl")))
}

/// Names of recorded sets on disk.
pub fn list_sets() -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(evals_dir()) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    out.push(stem.to_string());
                }
            }
        }
    }
    out.sort();
    out
}

/// Append one ask to the record set named by `NUR_JEV_RECORD`. No-op unless
/// the env var names a valid set. Best-effort: recording must never break a
/// judgment.
pub fn maybe_record(
    model: &str,
    state: &Value,
    questions: &[(String, Question)],
    answers: &BTreeMap<String, Answer>,
) {
    let name = std::env::var("NUR_JEV_RECORD").unwrap_or_default();
    let Some(path) = set_path(name.trim()) else {
        return;
    };
    let record = EvalRecord {
        state: state.clone(),
        questions: questions.iter().cloned().collect(),
        answers: answers.clone(),
        model: model.to_string(),
        expected: BTreeMap::new(),
    };
    let mut line = serde_json::to_string(&record.to_json()).unwrap_or_default();
    line.push('\n');
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = file.write_all(line.as_bytes());
    }
}

/// Load a set. A missing set is an error that lists what exists.
pub fn load_set(name: &str) -> Result<Vec<EvalRecord>, String> {
    let Some(path) = set_path(name) else {
        return Err(format!("bad eval set name {name:?}"));
    };
    let text = std::fs::read_to_string(&path).map_err(|_| {
        let sets = list_sets();
        if sets.is_empty() {
            format!(
                "no eval set {name:?} (and none recorded yet - set NUR_JEV_RECORD={name} \
                 while working, then replay with `nur jev eval --set {name}`)"
            )
        } else {
            format!("no eval set {name:?}; available: {}", sets.join(", "))
        }
    })?;
    let mut records = Vec::new();
    let mut bad = 0usize;
    for line in text.lines() {
        // Tolerate a BOM (PowerShell Out-File -Encoding utf8 writes one).
        let line = line.trim().trim_start_matches('\u{FEFF}');
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(line)
            .map_err(|e| e.to_string())
            .and_then(|v| EvalRecord::from_json(&v))
        {
            Ok(record) => records.push(record),
            Err(_) => bad += 1,
        }
    }
    if records.is_empty() {
        return Err(format!(
            "eval set {name:?} has no usable records ({bad} malformed)"
        ));
    }
    Ok(records)
}

/// Do two answers agree? Per primitive: same pick, same side of the coin
/// flip, score within half a level.
pub fn answers_agree(question: &Question, reference: &Answer, live: &Answer) -> bool {
    match (question, reference, live) {
        (_, Answer::Noul { noul: a }, Answer::Noul { noul: b }) => (*a >= 0.5) == (*b >= 0.5),
        (_, Answer::Choice { choice: a, .. }, Answer::Choice { choice: b, .. }) => a == b,
        (_, Answer::Score { score: a, .. }, Answer::Score { score: b, .. }) => (a - b).abs() <= 0.5,
        _ => false,
    }
}

/// Grade a live answer against a hand-added expectation:
/// `{qid: {"choice": str} | {"noul": bool} | {"score": number}}`.
/// `None` when the record carries no expectation for this question.
pub fn grade_expected(question: &Question, expected: &Value, live: &Answer) -> Option<bool> {
    match question {
        Question::Noul { .. } => {
            let want = expected.get("noul")?.as_bool()?;
            Some(live.noul().map(|p| (p >= 0.5) == want).unwrap_or(false))
        }
        Question::Choice { .. } => {
            let want = expected.get("choice")?.as_str()?;
            Some(live.choice() == Some(want))
        }
        Question::Score { .. } => {
            let want = expected.get("score")?.as_f64()?;
            Some(
                live.score()
                    .map(|s| (s - want).abs() <= 0.5)
                    .unwrap_or(false),
            )
        }
    }
}

/// What one replay run found.
#[derive(Debug, Default)]
pub struct EvalReport {
    pub records: usize,
    pub questions: usize,
    pub agreed: usize,
    pub graded: usize,
    pub correct: usize,
    pub missing: usize,
    pub requests: usize,
    pub ms: u64,
}

impl EvalReport {
    pub fn agreement(&self) -> f64 {
        if self.questions == 0 {
            return 0.0;
        }
        self.agreed as f64 / self.questions as f64
    }

    pub fn accuracy(&self) -> Option<f64> {
        if self.graded == 0 {
            return None;
        }
        Some(self.correct as f64 / self.graded as f64)
    }
}

/// Replay records through `client`, comparing fresh answers to recorded ones
/// (stability) and to `expected` labels where the record carries them
/// (correctness). `limit` caps records for a quick smoke run.
pub fn run_eval_with(client: &TypesafeClient, records: &[EvalRecord], limit: usize) -> EvalReport {
    let mut report = EvalReport::default();
    let started = std::time::Instant::now();
    for record in records.iter().take(limit.max(1)) {
        report.records += 1;
        let questions: Vec<(String, Question)> = record
            .questions
            .iter()
            .map(|(id, q)| (id.clone(), q.clone()))
            .collect();
        let live = match client.ask(&record.state, &questions) {
            Ok(batch) => batch,
            Err(_) => {
                report.missing += questions.len();
                continue;
            }
        };
        report.requests += live.requests.max(1);
        for (id, question) in &questions {
            report.questions += 1;
            match (record.answers.get(id), live.get(id)) {
                (Some(reference), Some(answer)) => {
                    if answers_agree(question, reference, answer) {
                        report.agreed += 1;
                    }
                    if let Some(want) = record.expected.get(id) {
                        report.graded += 1;
                        if grade_expected(question, want, answer).unwrap_or(false) {
                            report.correct += 1;
                        }
                    }
                }
                _ => report.missing += 1,
            }
        }
    }
    report.ms = started.elapsed().as_millis() as u64;
    report
}

pub fn format_report(name: &str, report: &EvalReport) -> String {
    let mut out = vec![format!(
        "jev eval · {name}: {}/{} questions agree ({:.1}%) across {} record(s), {} request(s), {}ms",
        report.agreed,
        report.questions,
        report.agreement() * 100.0,
        report.records,
        report.requests,
        report.ms,
    )];
    match report.accuracy() {
        Some(acc) => out.push(format!(
            "  correctness vs expected labels: {}/{} ({:.1}%)",
            report.correct,
            report.graded,
            acc * 100.0
        )),
        None => out.push(
            "  no expected labels in this set (stability only - add \"expected\" per \
             record to grade correctness)"
                .into(),
        ),
    }
    if report.missing > 0 {
        out.push(format!(
            "  {} question(s) had no answer on one side - check errors above",
            report.missing
        ));
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typesafe::questions::{ChoiceOption, NoulCriteria};

    fn sample_record() -> EvalRecord {
        EvalRecord {
            state: serde_json::json!({"goal": "ship"}),
            questions: [
                (
                    "urgent".to_string(),
                    Question::noul_with(
                        "Is this urgent?",
                        NoulCriteria {
                            yes: Some("outage".into()),
                            no: None,
                        },
                    ),
                ),
                (
                    "team".to_string(),
                    Question::choice(
                        "Which team?",
                        vec![
                            ChoiceOption::described("billing", "payments"),
                            ChoiceOption::new("technical"),
                        ],
                    ),
                ),
            ]
            .into_iter()
            .collect(),
            answers: [
                ("urgent".to_string(), Answer::Noul { noul: 0.9 }),
                (
                    "team".to_string(),
                    Answer::Choice {
                        choice: "billing".into(),
                        probabilities: [("billing".into(), 0.8), ("technical".into(), 0.2)]
                            .into_iter()
                            .collect(),
                        confidence: 0.7,
                    },
                ),
            ]
            .into_iter()
            .collect(),
            model: "jev-test".into(),
            expected: [
                ("urgent".to_string(), serde_json::json!({"noul": true})),
                ("team".to_string(), serde_json::json!({"choice": "billing"})),
            ]
            .into_iter()
            .collect(),
        }
    }

    #[test]
    fn record_round_trip_is_lossless() {
        let record = sample_record();
        let back = EvalRecord::from_json(&record.to_json()).unwrap();
        assert_eq!(back.questions, record.questions);
        assert_eq!(back.answers, record.answers);
        assert_eq!(back.model, record.model);
        assert_eq!(back.expected, record.expected);
    }

    #[test]
    fn agreement_follows_each_primitive() {
        let record = sample_record();
        let urgent = &record.questions["urgent"];
        assert!(answers_agree(
            urgent,
            &Answer::Noul { noul: 0.9 },
            &Answer::Noul { noul: 0.7 }
        ));
        assert!(!answers_agree(
            urgent,
            &Answer::Noul { noul: 0.9 },
            &Answer::Noul { noul: 0.2 }
        ));
        let team = &record.questions["team"];
        let billing = &record.answers["team"];
        assert!(answers_agree(team, billing, billing));
        let other = Answer::Choice {
            choice: "technical".into(),
            probabilities: BTreeMap::new(),
            confidence: 0.6,
        };
        assert!(!answers_agree(team, billing, &other));
        let level = Question::score("Tone?", vec!["calm".into(), "angry".into()]);
        assert!(answers_agree(
            &level,
            &Answer::Score {
                score: 1.0,
                legend: BTreeMap::new(),
                probabilities: BTreeMap::new(),
                confidence: 0.8,
            },
            &Answer::Score {
                score: 1.4,
                legend: BTreeMap::new(),
                probabilities: BTreeMap::new(),
                confidence: 0.5,
            },
        ));
    }

    #[test]
    fn grading_reads_hand_labels() {
        let record = sample_record();
        assert_eq!(
            grade_expected(
                &record.questions["urgent"],
                &record.expected["urgent"],
                &Answer::Noul { noul: 0.8 }
            ),
            Some(true)
        );
        assert_eq!(
            grade_expected(
                &record.questions["team"],
                &record.expected["team"],
                &record.answers["team"]
            ),
            Some(true)
        );
        assert_eq!(
            grade_expected(
                &record.questions["urgent"],
                &serde_json::json!({}),
                &Answer::Noul { noul: 0.8 }
            ),
            None
        );
    }

    #[test]
    fn record_and_load_use_an_isolated_dir() {
        let dir = std::env::temp_dir().join(format!("nur-jev-eval-{}", std::process::id()));
        std::env::set_var("NUR_JEV_EVAL_DIR", &dir);
        let record = sample_record();
        maybe_record_for_test(&record);
        let loaded = load_set("test-set").unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].questions, record.questions);
        assert!(load_set("missing").is_err());
        std::env::remove_var("NUR_JEV_EVAL_DIR");
        let _ = std::fs::remove_dir_all(&dir);
        std::env::remove_var("NUR_JEV_RECORD");
    }

    /// Recording helper with an explicit set name (the env-driven
    /// [`maybe_record`] is covered by the CLI path, not unit tests).
    fn maybe_record_for_test(record: &EvalRecord) {
        let path = set_path("test-set").unwrap();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let mut line = serde_json::to_string(&record.to_json()).unwrap();
        line.push('\n');
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(line.as_bytes()).unwrap();
    }

    #[test]
    fn replay_against_a_fake_client_reports_stability() {
        use crate::typesafe::client::{Transport, TransportFn};
        let t: std::sync::Arc<TransportFn> = std::sync::Arc::new(|body: &Value| {
            let qs = body
                .get("questions")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let mut answers = serde_json::Map::new();
            for (id, q) in qs {
                let a = match q.get("type").and_then(Value::as_str) {
                    Some("noul") => serde_json::json!({"type": "noul", "noul": 0.9}),
                    _ => serde_json::json!({
                        "type": "choice", "choice": "billing",
                        "probabilities": {"billing": 1.0}, "confidence": 0.9,
                    }),
                };
                answers.insert(id, a);
            }
            Ok(serde_json::json!({
                "model": "fake",
                "answers": Value::Object(answers),
                "usage": {"input_tokens": 5, "output_tokens": 1},
            }))
        });
        let cfg = crate::config::TypesafeConfig {
            enabled: true,
            api_key: "k".into(),
            ..crate::config::TypesafeConfig::default()
        };
        let client = TypesafeClient::with_transport(&cfg, Transport::Fake(t));
        let report = run_eval_with(&client, &[sample_record()], 10);
        assert_eq!(report.questions, 2);
        assert_eq!(report.agreed, 2, "fake mirrors the recorded answers");
        assert_eq!(report.accuracy(), Some(1.0));
    }
}

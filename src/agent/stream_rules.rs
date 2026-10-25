//! Stream rules ("time-traveling stream rules", after oh-my-pi's TTSR).
//!
//! A rule sits dormant until the model writes what it forbids. When the
//! streamed answer matches a rule's pattern, the request is stopped mid-stream,
//! the partial text is taken back, the rule joins the conversation as a note,
//! and the request goes out again: course correction without paying for the
//! rule on every turn. Tool-call arguments do not stream, so a finished
//! response is checked too, and a matching one is dropped before any tool runs.
//! The note is an ordinary conversation item, so it survives compaction and the
//! correction sticks. Each rule fires at most once per turn, so a model that
//! cannot comply ends the turn normally instead of looping.
//!
//! Rules come from `[[stream_rules]]` in config.toml and from a project's
//! `.nur/stream-rules.toml` (`[[rule]]` entries with the same fields):
//!
//! ```toml
//! [[stream_rules]]
//! name = "box-leak"
//! pattern = 'Box::leak'
//! rule = "Do not use Box::leak in production code paths; use Arc<str> or an owned String."
//! ```

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;

/// One rule as written in config.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct StreamRuleSpec {
    pub name: String,
    /// A regular expression (Rust `regex` syntax) over the streamed answer.
    pub pattern: String,
    /// What the model is told when the pattern matches.
    pub rule: String,
}

#[derive(Debug, Deserialize, Default)]
struct ProjectFile {
    #[serde(default)]
    rule: Vec<StreamRuleSpec>,
}

pub struct StreamRule {
    pub name: String,
    pattern: Regex,
    pub rule: String,
}

/// The rules for one turn and which of them already fired.
#[derive(Default)]
pub struct RuleWatch {
    rules: Vec<StreamRule>,
    pub fired: HashSet<String>,
    /// Rules that could not be compiled, for one status line.
    pub invalid: Vec<String>,
}

/// How far back a streamed match is looked for: enough for any sensible
/// pattern, and it keeps the per-delta check cheap on long answers.
const WINDOW: usize = 4096;

impl RuleWatch {
    pub fn load(cwd: &Path, configured: &[StreamRuleSpec]) -> Self {
        let mut specs: Vec<StreamRuleSpec> = configured.to_vec();
        if let Ok(text) = std::fs::read_to_string(cwd.join(".nur").join("stream-rules.toml")) {
            match toml::from_str::<ProjectFile>(&text) {
                Ok(file) => specs.extend(file.rule),
                Err(error) => {
                    let mut watch = Self::compile(specs);
                    watch
                        .invalid
                        .push(format!(".nur/stream-rules.toml: {error}"));
                    return watch;
                }
            }
        }
        Self::compile(specs)
    }

    fn compile(specs: Vec<StreamRuleSpec>) -> Self {
        let mut watch = Self::default();
        for spec in specs {
            if spec.name.trim().is_empty() || spec.rule.trim().is_empty() {
                watch
                    .invalid
                    .push(format!("`{}`: needs a name and a rule", spec.name));
                continue;
            }
            match Regex::new(&spec.pattern) {
                Ok(pattern) => watch.rules.push(StreamRule {
                    name: spec.name,
                    pattern,
                    rule: spec.rule,
                }),
                Err(error) => watch.invalid.push(format!("`{}`: {error}", spec.name)),
            }
        }
        watch
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// The first rule that has not fired this turn and matches the end of
    /// `text` (the last few KB, so a streamed answer is not rescanned whole).
    pub fn check(&self, text: &str) -> Option<&StreamRule> {
        let mut start = text.len().saturating_sub(WINDOW);
        while !text.is_char_boundary(start) {
            start -= 1;
        }
        let tail = &text[start..];
        self.rules
            .iter()
            .filter(|rule| !self.fired.contains(&rule.name))
            .find(|rule| rule.pattern.is_match(tail))
    }

    /// The first unfired rule anywhere in `text` (a finished response).
    pub fn check_whole(&self, text: &str) -> Option<&StreamRule> {
        self.rules
            .iter()
            .filter(|rule| !self.fired.contains(&rule.name))
            .find(|rule| rule.pattern.is_match(text))
    }

    pub fn rule(&self, name: &str) -> Option<&StreamRule> {
        self.rules.iter().find(|rule| rule.name == name)
    }
}

/// The conversation item a fired rule adds.
pub fn injection_item(rule: &StreamRule) -> Value {
    json!({
        "role": "user",
        "content": [{
            "type": "input_text",
            "text": format!(
                "[nur stream rule `{}`] Your previous answer was stopped because it broke this \
                 rule. Answer again following it:\n{}",
                rule.name, rule.rule
            )
        }]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &str, pattern: &str) -> StreamRuleSpec {
        StreamRuleSpec {
            name: name.into(),
            pattern: pattern.into(),
            rule: format!("rule {name}"),
        }
    }

    /// A rule fires on the text that breaks it, never again in the same turn,
    /// and broken rules are reported instead of silently ignored.
    #[test]
    fn rules_match_once_and_bad_rules_are_reported() {
        let mut watch = RuleWatch::compile(vec![
            spec("box-leak", r"Box::leak"),
            spec("todo", r"(?i)\btodo!\("),
            spec("broken", r"(unclosed"),
            StreamRuleSpec {
                name: "empty".into(),
                pattern: "x".into(),
                rule: String::new(),
            },
        ]);
        assert_eq!(watch.invalid.len(), 2, "{:?}", watch.invalid);
        assert!(watch.check("let s = String::from(x);").is_none());
        assert_eq!(
            watch
                .check("let s: &'static str = Box::leak(b)")
                .unwrap()
                .name,
            "box-leak"
        );
        watch.fired.insert("box-leak".into());
        assert!(
            watch.check("Box::leak again").is_none(),
            "fires once per turn"
        );
        assert_eq!(
            watch.check_whole("fn f() { TODO!() }").unwrap().name,
            "todo"
        );
    }

    /// The streamed check looks at the recent tail only, and never splits a
    /// multi-byte character when it cuts the window.
    #[test]
    fn the_stream_check_looks_at_the_tail_on_a_char_boundary() {
        let watch = RuleWatch::compile(vec![spec("early", r"EARLY")]);
        let mut text = String::from("EARLY ");
        text.push_str(&"─".repeat(WINDOW));
        assert!(watch.check(&text).is_none(), "outside the window");
        assert!(watch.check_whole(&text).is_some());
    }

    #[test]
    fn project_rules_join_the_configured_ones() {
        let dir = std::env::temp_dir().join(format!("nur-rules-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join(".nur")).unwrap();
        std::fs::write(
            dir.join(".nur").join("stream-rules.toml"),
            "[[rule]]\nname = \"no-unwrap\"\npattern = '\\.unwrap\\(\\)'\nrule = \"Handle the error.\"\n",
        )
        .unwrap();
        let watch = RuleWatch::load(&dir, &[spec("box-leak", "Box::leak")]);
        assert!(watch.invalid.is_empty(), "{:?}", watch.invalid);
        assert_eq!(watch.check("x.unwrap()").unwrap().name, "no-unwrap");
        assert!(watch.rule("box-leak").is_some());
        let item = injection_item(watch.rule("no-unwrap").unwrap());
        assert!(item["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Handle the error."));
        std::fs::remove_dir_all(dir).unwrap();
    }
}

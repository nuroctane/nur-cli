//! Close-ended user questions - OpenCode-desktop-style clarification.
//!
//! The model calls `question` with a question and 2-8 labeled options; the
//! TUI shows a modal (1/2/3 shortcuts, arrows+Enter, Space toggles when
//! multi-select is on, Esc dismisses) and the picked labels come back as the
//! tool result. Like OpenCode, this is for genuine forks the agent cannot
//! resolve alone - unlike OpenCode it is fenced: goal-driven turns,
//! subagents, and headless runs never interrupt the user (they get a
//! redirect to the `BLOCKED:` protocol instead), and a Jev gate answers
//! "resolve it yourself" first when the question needs no human at all.
//!
//! Execution lives in the agent loop (it must block on the TUI's answer), so
//! this module holds the schema stub plus the pure parts: argument parsing,
//! the keyboard picker state machine, and answer formatting.

use super::{Tool, ToolContext};
use crate::error::{NurError, Result};
use serde_json::Value;

/// Hard limits: the modal has digit shortcuts 1-8 and a bounded body.
pub const MAX_OPTIONS: usize = 8;
pub const MAX_QUESTION_CHARS: usize = 2_000;
pub const MAX_LABEL_CHARS: usize = 120;
pub const MAX_DESCRIPTION_CHARS: usize = 300;
pub const MAX_HEADER_CHARS: usize = 120;

pub struct QuestionTool;

impl Tool for QuestionTool {
    fn name(&self) -> &str {
        "question"
    }

    fn description(&self) -> &str {
        "Ask the user a close-ended question (y/n, 1/2/3, pick-one-or-more). \
         Use ONLY when their answer is genuinely required to proceed and no tool \
         or context can resolve it. Args: question (required), options \
         [{label (required), description}] 2-8 items (required), header (short \
         title), multiSelect (default false). Never use in goal-driven turns \
         (end with BLOCKED: instead), for free-form input (ask in prose), or to \
         confirm routine steps - just do them. Dismissed/redirected answers come \
         back as text: proceed with your best judgment, do not re-ask immediately."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "question": {"type": "string", "description": "The close-ended question for the user"},
                "header": {"type": "string", "description": "Short 2-4 word modal title"},
                "options": {
                    "type": "array",
                    "minItems": 2,
                    "maxItems": MAX_OPTIONS,
                    "items": {
                        "type": "object",
                        "properties": {
                            "label": {"type": "string"},
                            "description": {"type": "string"}
                        },
                        "required": ["label"]
                    }
                },
                "multiSelect": {"type": "boolean", "description": "Allow picking several options (default false)"}
            },
            "required": ["question", "options"]
        })
    }

    fn execute(&self, _args: &Value, _ctx: &ToolContext) -> Result<String> {
        // The loop intercepts `question` first (it must block on the modal
        // answer). Reaching dispatch means a subagent or headless path called
        // it: never invent a user answer, redirect instead.
        Err(NurError::Tool(
            "only the root interactive turn can ask the user - subagents and \
             headless runs must proceed with their best judgment or declare \
             BLOCKED: <what is needed>"
                .into(),
        ))
    }
}

/// A validated question call, ready for the modal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedQuestion {
    pub question: String,
    pub header: String,
    pub options: Vec<(String, String)>,
    pub multi_select: bool,
}

/// Validate a raw `question` tool call's arguments.
pub fn parse_question(args: &Value) -> std::result::Result<ParsedQuestion, String> {
    let question = args
        .get("question")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "question text is required".to_string())?;
    if question.chars().count() > MAX_QUESTION_CHARS {
        return Err(format!(
            "question too long ({} chars, max {MAX_QUESTION_CHARS}) - shorten it",
            question.chars().count()
        ));
    }
    let header = args
        .get("header")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(MAX_HEADER_CHARS).collect::<String>())
        .unwrap_or_else(|| "Question".to_string());
    let raw = args
        .get("options")
        .and_then(Value::as_array)
        .ok_or_else(|| "options array is required (2-8 labeled options)".to_string())?;
    if raw.len() < 2 {
        return Err("at least 2 options are required - a single option is not a question".into());
    }
    if raw.len() > MAX_OPTIONS {
        return Err(format!(
            "at most {MAX_OPTIONS} options are supported (got {}) - split the question",
            raw.len()
        ));
    }
    let mut options = Vec::with_capacity(raw.len());
    for (i, item) in raw.iter().enumerate() {
        let label = item
            .get("label")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("option {} needs a non-empty label", i + 1))?;
        if label.chars().count() > MAX_LABEL_CHARS {
            return Err(format!(
                "option {} label too long (max {MAX_LABEL_CHARS})",
                i + 1
            ));
        }
        if options
            .iter()
            .any(|(l, _): &(String, String)| l.eq_ignore_ascii_case(label))
        {
            return Err(format!(
                "option labels must be unique (duplicate {label:?})"
            ));
        }
        let description = item
            .get("description")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.chars().take(MAX_DESCRIPTION_CHARS).collect::<String>())
            .unwrap_or_default();
        options.push((label.to_string(), description));
    }
    let multi_select = args
        .get("multiSelect")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Ok(ParsedQuestion {
        question: question.to_string(),
        header,
        options,
        multi_select,
    })
}

/// What the model receives back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserAnswer {
    /// Labels the user picked (exactly one unless multi-select).
    Picked(Vec<String>),
    /// A free-form answer supplied when none of the labels fit.
    Typed(String),
    /// Esc / no interactive user / redirected: proceed, do not re-ask.
    Dismissed { reason: &'static str },
}

/// Editable single-line fallback. Cursor offsets always remain UTF-8 boundaries.
#[derive(Debug, Default)]
pub struct AnswerDraft {
    pub text: String,
    cursor: usize,
}

impl AnswerDraft {
    pub fn insert(&mut self, text: &str) {
        let clean: String = text
            .chars()
            .filter_map(|c| match c {
                '\n' | '\r' | '\t' => Some(' '),
                c if c.is_control() => None,
                c => Some(c),
            })
            .collect();
        self.text.insert_str(self.cursor, &clean);
        self.cursor += clean.len();
    }
    pub fn left(&mut self) {
        self.cursor = self.text[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0);
    }
    pub fn right(&mut self) {
        self.cursor += self.text[self.cursor..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(0);
    }
    pub fn home(&mut self) {
        self.cursor = 0;
    }
    pub fn end(&mut self) {
        self.cursor = self.text.len();
    }
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }
    pub fn backspace(&mut self) {
        let end = self.cursor;
        self.left();
        self.text.replace_range(self.cursor..end, "");
    }
    pub fn delete(&mut self) {
        if let Some(c) = self.text[self.cursor..].chars().next() {
            self.text.drain(self.cursor..self.cursor + c.len_utf8());
        }
    }
    /// Keep the insertion point visible, measuring terminal cells rather than bytes.
    pub fn visible(&self, width: usize) -> String {
        use unicode_width::UnicodeWidthChar;
        if width == 0 {
            return String::new();
        }
        let mut used = 1;
        let mut start = self.cursor;
        for (i, c) in self.text[..self.cursor].char_indices().rev() {
            let cells = c.width().unwrap_or(0);
            if used + cells > width {
                break;
            }
            start = i;
            used += cells;
        }
        let mut out = format!("{}▏", &self.text[start..self.cursor]);
        for c in self.text[self.cursor..].chars() {
            let cells = c.width().unwrap_or(0);
            if used + cells > width {
                break;
            }
            out.push(c);
            used += cells;
        }
        out
    }
}

pub fn format_answer(answer: &UserAnswer, options: &[(String, String)]) -> String {
    match answer {
        UserAnswer::Picked(selected) => {
            let shown = selected
                .iter()
                .map(|label| match options.iter().find(|(l, _)| l == label) {
                    Some((_, desc)) if !desc.is_empty() => format!("{label} - {desc}"),
                    _ => label.clone(),
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("user selected: {shown}")
        }
        UserAnswer::Typed(text) => format!("user typed: {text}"),
        UserAnswer::Dismissed { reason } => format!(
            "user did not answer ({reason}) - proceed with your best judgment, defer the \
             decision, or declare BLOCKED: <what you need>; do not re-ask the same question \
             immediately"
        ),
    }
}

/// Keypresses the picker understands (mapped from the TUI event layer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerKey {
    Up,
    Down,
    /// Digit shortcut 1-8 (0-based index carried).
    Number(usize),
    Toggle,
    Confirm,
    Dismiss,
}

/// Outcome of one keypress: move, pick, or finish with an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerEvent {
    Moved,
    Toggled,
    Picked(Vec<String>),
    Dismissed,
    Ignored,
}

/// Keyboard picker state: cursor + checked set. Single-select confirms on
/// number/Enter; multi-select toggles with Space/number and confirms on Enter.
#[derive(Debug, Clone)]
pub struct QuestionPicker {
    pub cursor: usize,
    pub checked: Vec<bool>,
    pub multi: bool,
}

impl QuestionPicker {
    pub fn new(count: usize, multi: bool) -> Self {
        Self {
            cursor: 0,
            checked: vec![false; count],
            multi,
        }
    }

    pub fn press(&mut self, key: PickerKey, labels: &[String]) -> PickerEvent {
        let n = labels.len();
        if n == 0 {
            return PickerEvent::Ignored;
        }
        match key {
            PickerKey::Up => {
                self.cursor = self.cursor.saturating_sub(1);
                PickerEvent::Moved
            }
            PickerKey::Down => {
                self.cursor = (self.cursor + 1).min(n - 1);
                PickerEvent::Moved
            }
            PickerKey::Number(i) => {
                if i >= n {
                    return PickerEvent::Ignored;
                }
                self.cursor = i;
                if self.multi {
                    self.checked[i] = !self.checked[i];
                    PickerEvent::Toggled
                } else {
                    PickerEvent::Picked(vec![labels[i].clone()])
                }
            }
            PickerKey::Toggle => {
                if !self.multi {
                    return PickerEvent::Ignored;
                }
                self.checked[self.cursor] = !self.checked[self.cursor];
                PickerEvent::Toggled
            }
            PickerKey::Confirm => {
                if self.multi {
                    let picked = labels
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| self.checked[*i])
                        .map(|(_, l)| l.clone())
                        .collect::<Vec<_>>();
                    if picked.is_empty() {
                        return PickerEvent::Ignored;
                    }
                    PickerEvent::Picked(picked)
                } else {
                    PickerEvent::Picked(vec![labels[self.cursor].clone()])
                }
            }
            PickerKey::Dismiss => PickerEvent::Dismissed,
        }
    }
}

/// Parse the `arguments` string of a `question` tool call (JSON).
pub fn parse_args(arguments: &str) -> std::result::Result<ParsedQuestion, String> {
    let args: Value = serde_json::from_str(arguments)
        .map_err(|_| "question arguments must be JSON".to_string())?;
    parse_question(&args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn typed_answer_edits_unicode_at_the_caret_and_bounds_its_view() {
        let mut draft = AnswerDraft::default();
        draft.insert("a界🙂z");
        draft.left();
        draft.backspace();
        draft.insert("é");
        assert_eq!(draft.text, "a界éz");
        draft.home();
        draft.delete();
        draft.right();
        draft.insert("\n\t\u{1b}ok");
        assert_eq!(draft.text, "界  okéz");
        for width in 1..15 {
            let shown = draft.visible(width);
            assert!(unicode_width::UnicodeWidthStr::width(shown.as_str()) <= width);
            assert!(shown.contains('▏'));
        }
        draft.end();
        draft.right();
        draft.delete();
        assert_eq!(draft.text, "界  okéz");
        draft.clear();
        draft.backspace();
        assert_eq!(draft.visible(2), "▏");
    }

    fn sample(multi: bool) -> Value {
        json!({
            "question": "Which runtime?",
            "header": "Deploy",
            "options": [
                {"label": "Bun", "description": "fast startup"},
                {"label": "Node", "description": "widest support"},
                {"label": "Deno"}
            ],
            "multiSelect": multi,
        })
    }

    #[test]
    fn valid_question_parses() {
        let q = parse_question(&sample(false)).unwrap();
        assert_eq!(q.question, "Which runtime?");
        assert_eq!(q.header, "Deploy");
        assert_eq!(q.options.len(), 3);
        assert_eq!(q.options[2], ("Deno".to_string(), String::new()));
        assert!(!q.multi_select);
        assert_eq!(parse_question(&sample(true)).unwrap().multi_select, true);
    }

    #[test]
    fn header_defaults_and_truncates() {
        let mut v = sample(false);
        v.as_object_mut().unwrap().remove("header");
        assert_eq!(parse_question(&v).unwrap().header, "Question");
    }

    #[test]
    fn malformed_calls_rejected() {
        assert!(parse_question(&json!({})).is_err());
        assert!(parse_question(&json!({"question": "  "})).is_err());
        assert!(parse_question(&json!({"question": "q", "options": [{"label": "only"}]})).is_err());
        let many: Vec<Value> = (0..9).map(|i| json!({"label": format!("o{i}")})).collect();
        assert!(parse_question(&json!({"question": "q", "options": many})).is_err());
        assert!(parse_question(
            &json!({"question": "q", "options": [{"label": "a"}, {"label": "A"}]})
        )
        .is_err());
        assert!(parse_question(
            &json!({"question": "q", "options": [{"label": ""}, {"label": "b"}]})
        )
        .is_err());
        assert!(parse_args("not json").is_err());
    }

    #[test]
    fn answers_format_for_the_model() {
        let opts = vec![
            ("Bun".to_string(), "fast startup".to_string()),
            ("Node".to_string(), String::new()),
        ];
        assert_eq!(
            format_answer(&UserAnswer::Picked(vec!["Bun".into()]), &opts),
            "user selected: Bun - fast startup"
        );
        assert_eq!(
            format_answer(
                &UserAnswer::Picked(vec!["Bun".into(), "Node".into()]),
                &opts
            ),
            "user selected: Bun - fast startup, Node"
        );
        assert_eq!(
            format_answer(&UserAnswer::Typed("Use Deno instead".into()), &opts),
            "user typed: Use Deno instead"
        );
        let d = format_answer(&UserAnswer::Dismissed { reason: "esc" }, &opts);
        assert!(d.contains("do not re-ask"), "{d}");
    }

    #[test]
    fn picker_single_select_confirms() {
        let labels = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let mut p = QuestionPicker::new(3, false);
        assert_eq!(p.press(PickerKey::Down, &labels), PickerEvent::Moved);
        assert_eq!(p.cursor, 1);
        assert_eq!(p.press(PickerKey::Up, &labels), PickerEvent::Moved);
        assert_eq!(p.cursor, 0);
        assert_eq!(
            p.press(PickerKey::Number(2), &labels),
            PickerEvent::Picked(vec!["c".to_string()])
        );
        // Out of range digit is ignored, not a panic.
        assert_eq!(p.press(PickerKey::Number(9), &labels), PickerEvent::Ignored);
        // Space does nothing in single-select.
        assert_eq!(p.press(PickerKey::Toggle, &labels), PickerEvent::Ignored);
        assert_eq!(
            p.press(PickerKey::Confirm, &labels),
            PickerEvent::Picked(vec!["c".to_string()])
        );
        assert_eq!(p.press(PickerKey::Dismiss, &labels), PickerEvent::Dismissed);
    }

    #[test]
    fn picker_multi_select_toggles_then_confirms() {
        let labels = vec!["a".to_string(), "b".to_string()];
        let mut p = QuestionPicker::new(2, true);
        // Empty confirm is ignored so Enter never submits nothing.
        assert_eq!(p.press(PickerKey::Confirm, &labels), PickerEvent::Ignored);
        assert_eq!(p.press(PickerKey::Toggle, &labels), PickerEvent::Toggled);
        assert_eq!(p.press(PickerKey::Number(1), &labels), PickerEvent::Toggled);
        assert_eq!(
            p.press(PickerKey::Confirm, &labels),
            PickerEvent::Picked(vec!["a".to_string(), "b".to_string()])
        );
    }
}

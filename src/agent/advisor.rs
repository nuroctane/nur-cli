//! Advisor: a second model that reviews the agent's work and leaves notes.
//!
//! Before a turn ends, the advisor reads the request, the turn's recent tool
//! trace and the answer. When it sees the work going wrong (a misread request,
//! a skipped requirement, a bug, an unchecked claim) it writes a short note;
//! the note joins the conversation and the agent takes one more pass. It
//! pushes back at most once per turn, so it steers the work instead of
//! arguing with it. With `every_round = true` it also reviews after each tool
//! round, where a note is read before the next step and never stops anything.
//! A failed advisor request is a status line; the turn goes on without it.
//!
//! ```toml
//! [advisor]
//! model = "gpt-5.5-mini"
//! provider = "openai"   # optional: the session's provider when empty
//! every_round = false
//! ```

use crate::api::types::{user_text_item, ResponseRequest};
use crate::api::ApiClient;
use crate::config::Config;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AdvisorConfig {
    /// The advisor's model. Empty: no advisor.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
    /// The advisor's provider. Empty: the session's.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub provider: String,
    /// Review after every tool round as well as before the turn ends.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub every_round: bool,
}

impl AdvisorConfig {
    pub fn is_unset(&self) -> bool {
        *self == Self::default()
    }
}

pub struct Advisor {
    pub client: ApiClient,
    pub model: String,
    pub provider: String,
    pub every_round: bool,
}

const INSTRUCTIONS: &str = "You advise a coding agent. You see the user's request and the \
agent's recent work. You never act yourself. If the agent is going wrong - it misread the \
request, skipped part of it, introduced a bug, is about to do something destructive, is going \
in circles, or claims something it has not checked - reply with one short, specific note to \
the agent, at most three sentences. If the work is on track, reply with exactly NONE.";

/// How much of the turn the advisor reads, newest first.
const TRACE_BUDGET: usize = 12_000;

impl Advisor {
    /// The advisor for a session, `None` when none is configured. An error
    /// names what is missing (an unknown provider, no key for it).
    pub fn for_session(cfg: &Config, session_client: &ApiClient) -> Result<Option<Self>, String> {
        let settings = &cfg.advisor;
        let model = settings.model.trim();
        if model.is_empty() {
            return Ok(None);
        }
        let wanted = settings.provider.trim();
        if wanted.is_empty() || wanted == cfg.provider {
            return Ok(Some(Self {
                client: session_client.clone(),
                model: model.to_string(),
                provider: cfg.provider.clone(),
                every_round: settings.every_round,
            }));
        }
        let provider = crate::providers::by_id(wanted)
            .or_else(|| crate::providers::resolve_provider_alias(wanted))
            .ok_or_else(|| crate::providers::unknown_provider_message(wanted))?;
        let target = crate::api::failover::plan_targets(
            &cfg.provider,
            &[provider.id.to_string()],
            crate::api::failover::resolve_target_key,
        )
        .pop()
        .ok_or_else(|| format!("no key for {} - sign in with /login", provider.name))?;
        let client =
            ApiClient::for_provider(&target.base_url, &target.api_key, &target.provider_id)
                .map_err(|error| error.to_string())?
                .with_style(target.style);
        Ok(Some(Self {
            client,
            model: model.to_string(),
            provider: target.provider_id,
            every_round: settings.every_round,
        }))
    }

    /// The review request: the request, the turn's trace, and the answer when
    /// the turn is about to end (`None` mid-turn).
    pub fn request(&self, goal: &str, items: &[Value], answer: Option<&str>) -> ResponseRequest {
        let mut body = format!(
            "# Request\n{}\n\n# Recent work\n{}\n",
            goal.trim(),
            trace(items, TRACE_BUDGET)
        );
        match answer {
            Some(answer) => body.push_str(&format!(
                "\n# Answer\n{}\n\nThe agent is about to end its turn with this answer.",
                answer.trim()
            )),
            None => {
                body.push_str("\nThe agent is mid-turn and reads your note before its next step.")
            }
        }
        ResponseRequest {
            model: self.model.clone(),
            input: json!([user_text_item(&body)]),
            instructions: Some(INSTRUCTIONS.to_string()),
            tools: None,
            tool_choice: None,
            store: Some(false),
            include: None,
            reasoning: None,
            stream: Some(false),
            parallel_tool_calls: None,
            prompt_cache_key: None,
            max_output_tokens: Some(400),
        }
    }
}

/// The advisor's note, or `None` when it found nothing to say.
pub fn parse_note(text: &str) -> Option<String> {
    let note = text.trim();
    let bare =
        note.trim_matches(|c: char| matches!(c, '"' | '`' | '*' | '.' | '\'') || c.is_whitespace());
    if bare.is_empty() || bare.eq_ignore_ascii_case("none") {
        return None;
    }
    Some(note.to_string())
}

/// The conversation item a note becomes.
pub fn note_item(model: &str, note: &str) -> Value {
    user_text_item(&format!("[nur advisor · {model}] {note}"))
}

/// The turn's recent work as plain lines, newest kept when over `budget`.
fn trace(items: &[Value], budget: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut used = 0;
    for item in items.iter().rev() {
        let line = match item.get("type").and_then(Value::as_str) {
            Some("function_call") => format!(
                "call {} {}",
                item.get("name").and_then(Value::as_str).unwrap_or("?"),
                clip(
                    item.get("arguments").and_then(Value::as_str).unwrap_or(""),
                    400
                )
            ),
            Some("function_call_output") => format!(
                "result {}",
                clip(
                    item.get("output").and_then(Value::as_str).unwrap_or(""),
                    600
                )
            ),
            _ => {
                let role = item.get("role").and_then(Value::as_str).unwrap_or("");
                let text = item
                    .get("content")
                    .and_then(Value::as_array)
                    .map(|parts| {
                        parts
                            .iter()
                            .filter_map(|part| part.get("text").and_then(Value::as_str))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                if text.trim().is_empty() || !matches!(role, "user" | "assistant") {
                    continue;
                }
                format!("{role}: {}", clip(&text, 800))
            }
        };
        used += line.len() + 1;
        if used > budget {
            break;
        }
        lines.push(line);
    }
    lines.reverse();
    lines.join("\n")
}

fn clip(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "Nothing to say" comes back in many spellings; anything else is a note.
    #[test]
    fn none_in_any_spelling_is_no_note() {
        for quiet in ["NONE", "none.", " `NONE` ", "**None**", "", "\"none\""] {
            assert_eq!(parse_note(quiet), None, "{quiet:?}");
        }
        assert_eq!(
            parse_note(" The test you added never runs. ").as_deref(),
            Some("The test you added never runs.")
        );
        assert!(parse_note("None of the edits were saved.").is_some());
    }

    /// The trace keeps the newest work when the turn is long, keeps calls and
    /// their results, and never splits a character.
    #[test]
    fn the_trace_keeps_the_newest_work() {
        let mut items = vec![user_text_item("fix the bug")];
        for n in 0..50 {
            items.push(json!({"type": "function_call", "name": "read_file", "arguments": format!("{{\"path\":\"f{n}.rs\"}}")}));
            items.push(json!({"type": "function_call_output", "output": "─".repeat(400)}));
        }
        let text = trace(&items, 2_000);
        assert!(text.contains("f49.rs"));
        assert!(!text.contains("f0.rs"));
        assert!(text.len() <= 2_000);
        assert!(trace(&items[..1], 2_000).starts_with("user: fix the bug"));
    }
}

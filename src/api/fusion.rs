//! Multi-model fusion — the `/fusion` debate panel (ported from wizard).
//!
//! `/fusion <question>` asks the active model **and** each configured panel
//! provider the same question, then the active model synthesizes one best
//! answer from all of them. The panel reuses the failover plumbing
//! ([`crate::api::failover`]) so each provider authenticates with its own key
//! and never touches the primary's `auth.json`.
//!
//! This module holds only the **pure** request builders (fully unit-tested).
//! The async executor that fans the calls out and streams results back to the
//! TUI lives in [`crate::agent::fusion`].

use crate::api::types::ResponseRequest;
use serde_json::{json, Value};

/// One panelist's answer (or the error note if the call failed).
#[derive(Debug, Clone)]
pub struct PanelAnswer {
    /// e.g. `"openai · gpt-5.5"`.
    pub label: String,
    /// The answer text, or an error message when `ok == false`.
    pub text: String,
    /// False → the call failed; `text` holds the error note and it is excluded
    /// from synthesis.
    pub ok: bool,
}

/// Human label for a panel member.
pub fn label(provider_id: &str, model: &str) -> String {
    format!("{provider_id} · {model}")
}

/// A user prompt in the Responses **message-array** shape. Built as an array
/// (not a bare string) on purpose: the Chat Completions adapter
/// ([`crate::api::chat::build_body`]) only reads `input` when it is an array of
/// items, so a bare string would reach chat providers as an empty prompt.
fn user_input(text: &str) -> Value {
    json!([{
        "type": "message",
        "role": "user",
        "content": [{ "type": "input_text", "text": text }],
    }])
}

fn one_shot(model: &str, instructions: &str, input: Value) -> ResponseRequest {
    ResponseRequest {
        model: model.to_string(),
        input,
        instructions: Some(instructions.to_string()),
        tools: None,
        tool_choice: None,
        store: Some(false),
        include: None,
        reasoning: None,
        // Fusion collects whole answers, not live deltas — a plain call is simpler
        // and works uniformly across Responses and Chat Completions providers.
        stream: Some(false),
        parallel_tool_calls: None,
        prompt_cache_key: None,
    }
}

const PANELIST_INSTRUCTIONS: &str =
    "You are one of several AI models answering the same question independently. \
     Give your best, direct, self-contained answer. Do not mention that you are on \
     a panel or that other models exist.";

/// Instructions for the judge model that fuses the panel into one answer.
pub const SYNTHESIS_INSTRUCTIONS: &str =
    "You are the synthesizer for a panel of AI models that each answered the same \
     question. Read the question and every panelist answer, then produce the single \
     best answer: keep what they agree on, resolve conflicts in favour of the most \
     correct, drop mistakes, and stay self-contained and directly useful. Do not \
     list the panelists or narrate the process — output only the best answer.";

/// Request that asks `model` the raw `question` as a panelist.
pub fn question_request(model: &str, question: &str) -> ResponseRequest {
    one_shot(model, PANELIST_INSTRUCTIONS, user_input(question))
}

/// Request fed to the judge `model` to synthesize the final answer from the
/// panel. Only successful answers should be passed in `answers`.
pub fn synthesis_request(
    judge_model: &str,
    question: &str,
    answers: &[PanelAnswer],
) -> ResponseRequest {
    let mut body = String::new();
    body.push_str("# Question\n");
    body.push_str(question.trim());
    body.push_str("\n\n# Panelist answers\n");
    for (i, a) in answers.iter().enumerate() {
        body.push_str(&format!(
            "\n## Panelist {} — {}\n{}\n",
            i + 1,
            a.label,
            a.text.trim()
        ));
    }
    body.push_str("\n\n# Task\nSynthesize the single best answer to the question above.");
    one_shot(judge_model, SYNTHESIS_INSTRUCTIONS, user_input(&body))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dig the user text back out of a request's `input` array.
    fn input_text(req: &ResponseRequest) -> String {
        req.input[0]["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string()
    }

    #[test]
    fn label_formats_provider_and_model() {
        assert_eq!(label("openai", "gpt-5.5"), "openai · gpt-5.5");
    }

    #[test]
    fn question_request_carries_prompt_as_array_not_string() {
        let req = question_request("grok-4.5", "why is the sky blue?");
        assert_eq!(req.model, "grok-4.5");
        assert_eq!(req.stream, Some(false));
        // Must be an array (so the chat adapter picks it up), and the text round-trips.
        assert!(
            req.input.is_array(),
            "input must be a message array, not a bare string"
        );
        assert_eq!(input_text(&req), "why is the sky blue?");
        assert_eq!(req.instructions.as_deref(), Some(PANELIST_INSTRUCTIONS));
    }

    #[test]
    fn synthesis_request_includes_question_and_every_answer() {
        let answers = vec![
            PanelAnswer {
                label: "openai · gpt-5.5".into(),
                text: "Rayleigh scattering.".into(),
                ok: true,
            },
            PanelAnswer {
                label: "anthropic · claude-sonnet-5".into(),
                text: "Blue light scatters more.".into(),
                ok: true,
            },
        ];
        let req = synthesis_request("gpt-5.5", "why is the sky blue?", &answers);
        let body = input_text(&req);
        assert_eq!(req.instructions.as_deref(), Some(SYNTHESIS_INSTRUCTIONS));
        assert!(body.contains("why is the sky blue?"), "question missing");
        assert!(body.contains("openai · gpt-5.5"), "member 1 label missing");
        assert!(
            body.contains("Rayleigh scattering."),
            "member 1 answer missing"
        );
        assert!(
            body.contains("anthropic · claude-sonnet-5"),
            "member 2 label missing"
        );
        assert!(
            body.contains("Blue light scatters more."),
            "member 2 answer missing"
        );
    }

    #[test]
    fn synthesis_request_numbers_panelists_in_order() {
        let answers = vec![
            PanelAnswer {
                label: "a".into(),
                text: "one".into(),
                ok: true,
            },
            PanelAnswer {
                label: "b".into(),
                text: "two".into(),
                ok: true,
            },
        ];
        let body = input_text(&synthesis_request("m", "q", &answers));
        let p1 = body.find("Panelist 1").expect("panelist 1");
        let p2 = body.find("Panelist 2").expect("panelist 2");
        assert!(p1 < p2, "panelists must be numbered in order");
    }
}

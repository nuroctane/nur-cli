//! Perplexity's Agent API: `POST /v1/responses` (an alias of `/v1/agent`).
//!
//! Sonar's Chat Completions endpoint was retired on 2026-09-27; the Agent API
//! is the only inference surface left. It speaks the Responses shape but
//! validates `input` against a narrower schema than OpenAI does:
//!
//! - items are `message`, `function_call` or `function_call_output` only, and a
//!   message must carry `type: "message"` (nur's role messages omit it);
//! - content parts are `input_text`, `input_image` or `input_file`, so replayed
//!   assistant `output_text` parts are rejected;
//! - `input_image.image_url` is capped at 2048 characters, which no inline data
//!   URI of a real screenshot fits;
//! - `function_call_output` rejects any field it does not declare.
//!
//! The request itself has no `include`, `tool_choice`, `parallel_tool_calls`
//! or `prompt_cache_key`, `reasoning` takes `effort` alone, and Anthropic
//! models behind it require `max_output_tokens`.

use super::types::{ReasoningConfig, ResponseRequest};
use serde_json::{json, Map, Value};

/// Longest `input_image.image_url` the Agent API accepts.
const MAX_IMAGE_URL_CHARS: usize = 2048;

/// Same ceiling the Anthropic adapter falls back to when nur reserves none.
const ANTHROPIC_OUTPUT_TOKENS: u64 = 16_384;

const IMAGE_OMITTED: &str = "[image attachment omitted: Perplexity accepts image URLs of at most \
                             2048 characters, not inline images]";

/// Fit a Responses request to the Agent API schema. Runs at the wire boundary,
/// so the session history keeps its full OpenAI shape for other providers.
pub fn shape_request(wire: &mut ResponseRequest) {
    wire.include = None;
    wire.tool_choice = None;
    wire.parallel_tool_calls = None;
    wire.prompt_cache_key = None;
    wire.reasoning = wire
        .reasoning
        .take()
        .and_then(|reasoning| reasoning.effort)
        .map(|effort| ReasoningConfig {
            effort: Some(effort),
            summary: None,
        });
    if wire.max_output_tokens.is_none() && wire.model.starts_with("anthropic/") {
        wire.max_output_tokens = Some(ANTHROPIC_OUTPUT_TOKENS);
    }
    if let Value::Array(items) = &wire.input {
        wire.input = Value::Array(items.iter().filter_map(input_item).collect());
    }
}

/// One history item in Agent API form, or `None` where it has no equivalent
/// (`reasoning` carries another vendor's encrypted state).
fn input_item(item: &Value) -> Option<Value> {
    match item.get("type").and_then(Value::as_str) {
        None | Some("message") => message(item),
        Some("function_call") | Some("custom_tool_call") => Some(function_call(item)),
        Some("function_call_output") | Some("custom_tool_call_output") => {
            Some(function_call_output(item))
        }
        _ => None,
    }
}

fn message(item: &Value) -> Option<Value> {
    let role = item
        .get("role")
        .and_then(Value::as_str)
        .filter(|role| matches!(*role, "user" | "assistant" | "system" | "developer"))
        .unwrap_or("user");
    let content = match item.get("content") {
        Some(Value::String(text)) => Value::String(text.clone()),
        Some(Value::Array(parts)) => {
            let parts: Vec<Value> = parts.iter().filter_map(content_part).collect();
            if role == "assistant" {
                // Replayed turns as plain text: the most portable form for
                // whichever upstream model Perplexity forwards them to.
                Value::String(joined_text(&parts))
            } else {
                Value::Array(parts)
            }
        }
        _ => return None,
    };
    let empty = match &content {
        Value::String(text) => text.is_empty(),
        Value::Array(parts) => parts.is_empty(),
        _ => true,
    };
    (!empty).then(|| json!({ "type": "message", "role": role, "content": content }))
}

fn content_part(part: &Value) -> Option<Value> {
    let kind = part.get("type").and_then(Value::as_str).unwrap_or("");
    match kind {
        "input_text" | "output_text" | "text" => part
            .get("text")
            .and_then(Value::as_str)
            .map(|text| json!({ "type": "input_text", "text": text })),
        "refusal" => part
            .get("refusal")
            .and_then(Value::as_str)
            .map(|text| json!({ "type": "input_text", "text": text })),
        "input_image" => Some(image(part)),
        "input_file" => {
            let mut file = Map::new();
            file.insert("type".into(), "input_file".into());
            for key in ["filename", "file_data", "file_url"] {
                if let Some(value) = part.get(key).filter(|value| value.is_string()) {
                    file.insert(key.into(), value.clone());
                }
            }
            if file.contains_key("file_data") || file.contains_key("file_url") {
                Some(Value::Object(file))
            } else {
                // A `file_id` upload has no Agent API form.
                Some(omitted("file"))
            }
        }
        "" => None,
        other => Some(omitted(other.trim_start_matches("input_"))),
    }
}

fn image(part: &Value) -> Value {
    let url = part.get("image_url").and_then(|url| {
        url.as_str()
            .or_else(|| url.get("url").and_then(Value::as_str))
    });
    match url {
        Some(url) if url.chars().count() <= MAX_IMAGE_URL_CHARS => {
            json!({ "type": "input_image", "image_url": url })
        }
        _ => json!({ "type": "input_text", "text": IMAGE_OMITTED }),
    }
}

fn omitted(kind: &str) -> Value {
    json!({
        "type": "input_text",
        "text": format!("[{kind} attachment omitted: Perplexity's Agent API cannot accept it]"),
    })
}

fn joined_text(parts: &[Value]) -> String {
    parts
        .iter()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

fn function_call(item: &Value) -> Value {
    let call_id = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let arguments = match item.get("arguments").or_else(|| item.get("input")) {
        Some(Value::String(text)) if serde_json::from_str::<Value>(text).is_ok() => text.clone(),
        // Freeform custom-tool input is raw text; arguments must be JSON.
        Some(Value::String(text)) => json!({ "input": text }).to_string(),
        Some(Value::Null) | None => "{}".to_string(),
        Some(other) => other.to_string(),
    };
    let mut call = json!({
        "type": "function_call",
        "call_id": call_id,
        "name": item.get("name").and_then(Value::as_str).unwrap_or(""),
        "arguments": arguments,
    });
    copy_signature(item, &mut call);
    call
}

fn function_call_output(item: &Value) -> Value {
    let output = match item.get("output") {
        Some(Value::String(text)) => Value::String(text.clone()),
        // Tool output parts may only be text and images.
        Some(Value::Array(parts)) => {
            let parts: Vec<Value> = parts
                .iter()
                .filter_map(content_part)
                .map(|part| match part["type"] == "input_file" {
                    true => omitted("file"),
                    false => part,
                })
                .collect();
            match parts.is_empty() {
                true => Value::String(String::new()),
                false => Value::Array(parts),
            }
        }
        Some(Value::Null) | None => Value::String(String::new()),
        Some(other) => Value::String(other.to_string()),
    };
    let mut result = json!({
        "type": "function_call_output",
        "call_id": item.get("call_id").and_then(Value::as_str).unwrap_or(""),
        "output": output,
    });
    if let Some(name) = item.get("name").and_then(Value::as_str) {
        result["name"] = Value::String(name.to_string());
    }
    copy_signature(item, &mut result);
    result
}

/// Thinking models behind the Agent API pair a call with its output through
/// this signature; it has to survive the replay.
fn copy_signature(from: &Value, to: &mut Value) {
    if let Some(signature) = from.get("thought_signature").and_then(Value::as_str) {
        to["thought_signature"] = Value::String(signature.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(model: &str, input: Value) -> ResponseRequest {
        ResponseRequest {
            model: model.into(),
            input,
            instructions: Some("system".into()),
            tools: None,
            tool_choice: Some("required".into()),
            store: Some(false),
            include: Some(vec!["reasoning.encrypted_content".into()]),
            reasoning: Some(ReasoningConfig {
                effort: Some("high".into()),
                summary: Some("auto".into()),
            }),
            stream: Some(true),
            parallel_tool_calls: Some(true),
            prompt_cache_key: Some("session".into()),
            max_output_tokens: None,
        }
    }

    fn shaped(model: &str, input: Value) -> Value {
        let mut wire = request(model, input);
        shape_request(&mut wire);
        serde_json::to_value(wire).unwrap()
    }

    // Failure modes, each a 400 from the Agent API:
    // 1. a request field it does not declare;
    // 2. a role message without `type: "message"`;
    // 3. a replayed `output_text` part or `reasoning` item;
    // 4. undeclared fields on `function_call_output`;
    // 5. an inline image longer than 2048 characters;
    // 6. an `anthropic/*` model without `max_output_tokens`.

    #[test]
    fn drops_request_fields_the_agent_api_does_not_declare() {
        let body = shaped("perplexity/sonar", json!([]));
        for field in [
            "include",
            "tool_choice",
            "parallel_tool_calls",
            "prompt_cache_key",
        ] {
            assert!(body.get(field).is_none(), "{field} was sent");
        }
        assert_eq!(body["reasoning"], json!({ "effort": "high" }));
        assert_eq!(body["store"], json!(false));
        assert!(body.get("max_output_tokens").is_none());
    }

    #[test]
    fn replayed_history_fits_the_input_union() {
        let body = shaped(
            "perplexity/sonar",
            json!([
                {"role": "user", "content": [{"type": "input_text", "text": "go"}]},
                {"type": "reasoning", "summary": [], "encrypted_content": "openai-state"},
                {"type": "message", "id": "msg_1", "role": "assistant", "status": "completed",
                 "phase": "commentary",
                 "content": [{"type": "output_text", "text": "Checking.", "annotations": []}]},
                {"type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "read_file",
                 "arguments": {"path": "a.rs"}, "status": "completed"},
                {"type": "function_call_output", "id": "out_1", "call_id": "call_1",
                 "output": "fn main() {}", "status": "completed", "extra": true},
                {"type": "message", "role": "assistant", "content": []}
            ]),
        );
        assert_eq!(
            body["input"],
            json!([
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "go"}]},
                {"type": "message", "role": "assistant", "content": "Checking."},
                {"type": "function_call", "call_id": "call_1", "name": "read_file",
                 "arguments": "{\"path\":\"a.rs\"}"},
                {"type": "function_call_output", "call_id": "call_1", "output": "fn main() {}"}
            ])
        );
    }

    #[test]
    fn inline_images_beyond_the_url_cap_become_a_note() {
        let inline = format!("data:image/png;base64,{}", "A".repeat(4096));
        let body = shaped(
            "perplexity/sonar",
            json!([{"role": "user", "content": [
                {"type": "input_text", "text": "look"},
                {"type": "input_image", "image_url": inline},
                {"type": "input_image", "image_url": "https://example.com/shot.png"}
            ]}]),
        );
        let parts = body["input"][0]["content"].as_array().unwrap();
        assert_eq!(
            parts[1],
            json!({ "type": "input_text", "text": IMAGE_OMITTED })
        );
        assert_eq!(
            parts[2],
            json!({ "type": "input_image", "image_url": "https://example.com/shot.png" })
        );
    }

    #[test]
    fn anthropic_models_always_carry_an_output_ceiling() {
        let body = shaped("anthropic/claude-sonnet-5-5", json!([]));
        assert_eq!(body["max_output_tokens"], json!(ANTHROPIC_OUTPUT_TOKENS));

        let mut wire = request("anthropic/claude-sonnet-5-5", json!([]));
        wire.max_output_tokens = Some(8_192);
        shape_request(&mut wire);
        assert_eq!(wire.max_output_tokens, Some(8_192));
    }
}

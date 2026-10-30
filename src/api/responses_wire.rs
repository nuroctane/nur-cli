//! The Responses `input` array, fitted to the route it is sent to.
//!
//! Session history is protocol-neutral: one conversation can run on DeepSeek
//! over Chat Completions, then Claude, then GPT. The Chat, Anthropic and Gemini
//! adapters rebuild every item into their own shape, so nothing foreign can
//! reach those wires. The Responses route used to send history verbatim, and
//! every field another protocol keeps on an item became a 400 there:
//!
//! 1. `reasoning_content` on assistant messages (DeepSeek/Kimi thinking replay);
//! 2. `thought_signature` on calls (Gemini thinking models);
//! 3. ids from another vendor (`toolu_…`), which OpenAI validates by prefix;
//! 4. `phase`, which only OpenAI's own hosts declare;
//! 5. encrypted `reasoning` items, which only decrypt on the account and host
//!    that produced them;
//! 6. `arguments` stored as an object rather than a JSON string;
//! 7. undeclared keys on content parts;
//! 8. call ids over OpenAI's 40-character limit (`super::wire_ids`).
//!
//! So every item is rebuilt here from the fields OpenAI's input schema
//! declares. Perplexity's narrower schema has its own shaper
//! (`super::perplexity`), used instead of this one.

use serde_json::{json, Map, Value};

/// What one Responses route accepts beyond the common schema.
pub struct Route<'a> {
    /// The route's account (see `ApiClient::reasoning_origin`).
    pub origin: &'a str,
    /// The route declares assistant `phase` (OpenAI's GPT-5.3+ hint).
    pub phase: bool,
    /// The route may get encrypted reasoning that predates origin tagging.
    pub legacy_reasoning: bool,
}

fn host(base_url: &str) -> &str {
    base_url
        .split("://")
        .nth(1)
        .unwrap_or(base_url)
        .split(['/', ':', '?'])
        .next()
        .unwrap_or("")
}

/// OpenAI itself: its API (any region) and the ChatGPT/Codex backend.
fn is_openai_host(base_url: &str) -> bool {
    let host = host(base_url);
    host == "api.openai.com" || host.ends_with(".api.openai.com") || host == "chatgpt.com"
}

impl<'a> Route<'a> {
    pub fn new(origin: &'a str, provider_id: &str, base_url: &str) -> Self {
        let openai = provider_id == "openai" || is_openai_host(base_url);
        let host = host(base_url);
        // Hosts serving OpenAI's GPT models on the Responses schema.
        let gpt_host = host.ends_with(".openai.azure.com")
            || host.ends_with("githubcopilot.com")
            || host == "opencode.ai";
        Self {
            origin,
            phase: openai || gpt_host,
            legacy_reasoning: openai,
        }
    }
}

/// Rewrite `input` for one Responses route.
pub fn shape_input(input: &mut Value, route: &Route) {
    let Value::Array(items) = input else {
        return;
    };
    *items = items
        .iter()
        .filter_map(|item| shape_item(item, route))
        .collect();
}

fn shape_item(item: &Value, route: &Route) -> Option<Value> {
    match item.get("type").and_then(Value::as_str) {
        None | Some("message") => message(item, route.phase),
        Some("function_call") | Some("custom_tool_call") => Some(function_call(item)),
        Some("function_call_output") | Some("custom_tool_call_output") => {
            Some(function_call_output(item))
        }
        Some("reasoning") => reasoning(item, route),
        // nur writes no other item type; one from elsewhere goes as it came.
        Some(_) => Some(item.clone()),
    }
}

/// `None` for a message with nothing in it: a thinking model's tool-call turn
/// is stored as an empty assistant message carrying only `reasoning_content`,
/// and an empty `content` is itself a 400.
fn message(item: &Value, phase: bool) -> Option<Value> {
    let role = item.get("role").and_then(Value::as_str).unwrap_or("user");
    let content = match item.get("content") {
        Some(Value::Array(parts)) if !parts.is_empty() => {
            Value::Array(parts.iter().map(|part| content_part(part, role)).collect())
        }
        Some(Value::String(text)) if !text.is_empty() => Value::String(text.clone()),
        _ => return None,
    };
    let mut out = json!({ "role": role, "content": content });
    if item.get("type").is_some() {
        out["type"] = json!("message");
    }
    if let Some(phase) = item.get("phase").filter(|_| phase && role == "assistant") {
        out["phase"] = phase.clone();
    }
    Some(out)
}

/// Keep the keys each part type declares; leave unknown part types alone.
fn content_part(part: &Value, role: &str) -> Value {
    let kind = part.get("type").and_then(Value::as_str).unwrap_or("");
    let keys: &[&str] = match kind {
        "input_text" => &["text"],
        "output_text" => &["text", "annotations"],
        "refusal" => &["refusal"],
        "input_image" => &["image_url", "file_id", "detail"],
        "input_file" => &["file_id", "file_data", "file_url", "filename"],
        "input_audio" => &["input_audio"],
        // Chat-shaped text: the role decides which Responses part it is.
        "text" => {
            let kind = if role == "assistant" {
                "output_text"
            } else {
                "input_text"
            };
            return json!({ "type": kind, "text": part.get("text").cloned().unwrap_or(json!("")) });
        }
        _ => return part.clone(),
    };
    let mut out = Map::new();
    out.insert("type".into(), json!(kind));
    for key in keys {
        if let Some(value) = part.get(*key).filter(|value| !value.is_null()) {
            out.insert((*key).into(), value.clone());
        }
    }
    if kind == "output_text" && !out.contains_key("text") {
        out.insert("text".into(), json!(""));
    }
    Value::Object(out)
}

fn function_call(item: &Value) -> Value {
    json!({
        "type": "function_call",
        "call_id": call_id(item),
        "name": item.get("name").and_then(Value::as_str).unwrap_or(""),
        "arguments": super::types::call_arguments(item),
    })
}

fn function_call_output(item: &Value) -> Value {
    let output = match item.get("output") {
        Some(Value::String(text)) => Value::String(text.clone()),
        Some(Value::Array(parts)) => Value::Array(
            parts
                .iter()
                .map(|part| content_part(part, "user"))
                .collect(),
        ),
        Some(Value::Null) | None => Value::String(String::new()),
        Some(other) => Value::String(other.to_string()),
    };
    json!({
        "type": "function_call_output",
        "call_id": call_id(item),
        "output": output,
    })
}

fn call_id(item: &Value) -> String {
    let id = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("");
    super::wire_ids::fit(id, super::wire_ids::IdRule::Openai)
}

/// Encrypted reasoning only decrypts where it was made. An item without
/// encrypted content would be looked up by id, which `store: false` never
/// saved, so it is dropped too.
fn reasoning(item: &Value, route: &Route) -> Option<Value> {
    let encrypted = item.get("encrypted_content").filter(|v| v.is_string())?;
    let same_route = match item.get(ORIGIN_KEY).and_then(Value::as_str) {
        Some(made_on) => made_on == route.origin,
        // Untagged items predate tagging; only OpenAI ever produced them.
        None => route.legacy_reasoning,
    };
    if !same_route {
        return None;
    }
    let mut out = json!({
        "type": "reasoning",
        "summary": item.get("summary").cloned().unwrap_or_else(|| json!([])),
        "encrypted_content": encrypted,
    });
    if let Some(id) = item.get("id").filter(|v| v.is_string()) {
        out["id"] = id.clone();
    }
    Some(out)
}

/// History-only key naming the route a reasoning item came from. Never sent:
/// [`reasoning`] rebuilds the item without it and the other adapters drop
/// reasoning items altogether.
pub const ORIGIN_KEY: &str = "nur_origin";

#[cfg(test)]
mod tests {
    use super::*;

    const OPENAI: &str = "https://api.openai.com/v1";
    const ORIGIN: &str = "openai-key-origin";

    fn shaped(input: Value, origin: &str, provider: &str, base: &str) -> Value {
        let mut input = input;
        shape_input(&mut input, &Route::new(origin, provider, base));
        input
    }

    // Failure modes, each a 400 on a Responses route (numbered as above).

    #[test]
    fn foreign_fields_never_reach_the_wire() {
        let input = shaped(
            json!([
                {"role": "user", "content": [
                    {"type": "input_text", "text": "go", "cache_control": {"type": "ephemeral"}}
                ]},
                {"role": "assistant", "phase": "commentary", "reasoning_content": "hmm",
                 "content": [{"type": "output_text", "text": "Checking.", "signature": "x"}]},
                {"type": "function_call", "id": "toolu_01", "call_id": "toolu_01",
                 "name": "read_file", "arguments": {"path": "a.rs"},
                 "thought_signature": "sig", "status": "completed"},
                {"type": "function_call_output", "id": "x", "call_id": "toolu_01",
                 "output": "fn main() {}", "thought_signature": "sig", "name": "read_file"},
                {"role": "assistant", "content": [{"type": "text", "text": "Done."}]}
            ]),
            "deepseek-origin",
            "xai",
            "https://api.x.ai/v1",
        );
        assert_eq!(
            input,
            json!([
                {"role": "user", "content": [{"type": "input_text", "text": "go"}]},
                {"role": "assistant", "content": [{"type": "output_text", "text": "Checking."}]},
                {"type": "function_call", "call_id": "toolu_01", "name": "read_file",
                 "arguments": "{\"path\":\"a.rs\"}"},
                {"type": "function_call_output", "call_id": "toolu_01", "output": "fn main() {}"},
                {"role": "assistant", "content": [{"type": "output_text", "text": "Done."}]}
            ])
        );
    }

    /// A DeepSeek/Kimi turn that only called tools is stored as an empty
    /// assistant message carrying `reasoning_content`; `content: []` is a 400.
    #[test]
    fn empty_messages_are_left_out() {
        let input = shaped(
            json!([
                {"role": "user", "content": "go"},
                {"type": "message", "role": "assistant", "content": [], "reasoning_content": "hmm"},
                {"type": "function_call", "call_id": "c1", "name": "ls", "arguments": "{}"},
                {"role": "assistant", "content": ""}
            ]),
            ORIGIN,
            "openai",
            OPENAI,
        );
        assert_eq!(
            input,
            json!([
                {"role": "user", "content": "go"},
                {"type": "function_call", "call_id": "c1", "name": "ls", "arguments": "{}"}
            ])
        );
    }

    #[test]
    fn phase_reaches_hosts_that_serve_gpt_on_responses() {
        let item = json!([{"role": "assistant", "phase": "final_answer",
                           "content": [{"type": "output_text", "text": "ok"}]}]);
        for (provider, base) in [
            ("openai", OPENAI),
            ("openai", "https://eu.api.openai.com/v1"),
            ("openai", "http://127.0.0.1:4000/v1"),
            ("openai", "https://chatgpt.com/backend-api/codex"),
            ("azure", "https://acme.openai.azure.com/openai/v1"),
            ("opencode", "https://opencode.ai/zen/v1"),
        ] {
            assert_eq!(
                shaped(item.clone(), ORIGIN, provider, base)[0]["phase"],
                "final_answer",
                "{provider} {base}"
            );
        }
        let xai = shaped(item, ORIGIN, "xai", "https://api.x.ai/v1");
        assert!(xai[0].get("phase").is_none());
    }

    #[test]
    fn encrypted_reasoning_replays_only_where_it_was_made() {
        let item = |origin: Option<&str>| {
            let mut item = json!({"type": "reasoning", "id": "rs_1", "summary": [],
                                  "encrypted_content": "blob"});
            if let Some(origin) = origin {
                item[ORIGIN_KEY] = json!(origin);
            }
            json!([item])
        };
        let same = shaped(item(Some(ORIGIN)), ORIGIN, "openai", OPENAI);
        assert_eq!(
            same,
            json!([{"type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "blob"}])
        );
        // Another account (a ChatGPT sign-in vs an API key) or another vendor.
        assert_eq!(
            shaped(item(Some("chatgpt-origin")), ORIGIN, "openai", OPENAI),
            json!([])
        );
        assert_eq!(
            shaped(
                item(Some(ORIGIN)),
                "xai-origin",
                "xai",
                "https://api.x.ai/v1"
            ),
            json!([])
        );
        // Untagged history from before tagging: OpenAI only.
        assert_eq!(
            shaped(item(None), ORIGIN, "openai", OPENAI)
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            shaped(
                item(None),
                "meta-origin",
                "meta",
                "https://api.llama.com/v1"
            ),
            json!([])
        );
        // Without encrypted content the id would be looked up and 404.
        let bare = json!([{"type": "reasoning", "id": "rs_2", "summary": [], ORIGIN_KEY: ORIGIN}]);
        assert_eq!(shaped(bare, ORIGIN, "openai", OPENAI), json!([]));
    }

    #[test]
    fn custom_tool_items_become_function_items() {
        let input = shaped(
            json!([
                {"type": "custom_tool_call", "call_id": "c1", "name": "apply_patch",
                 "input": "*** Begin Patch"},
                {"type": "custom_tool_call_output", "call_id": "c1", "output": "ok"}
            ]),
            ORIGIN,
            "openai",
            OPENAI,
        );
        assert_eq!(input[0]["type"], "function_call");
        assert_eq!(input[0]["arguments"], "*** Begin Patch");
        assert_eq!(input[1]["type"], "function_call_output");
    }
}

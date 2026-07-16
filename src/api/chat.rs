//! OpenAI **Chat Completions** adapter.
//!
//! nur-cli's agent loop is written against the Responses API. To reach the
//! wider world (OpenRouter, Groq, Together, local servers, …) we translate a
//! [`ResponseRequest`] into a `/chat/completions` body, and translate the
//! response — content + tool calls + usage — back into an [`ApiResponse`] by
//! reconstructing the Responses `output` shape and reusing its deserializer.

use super::types::{ApiResponse, ResponseRequest};
use serde_json::{json, Value};

/// Build a `/chat/completions` request body from a Responses request.
#[cfg(test)]
pub fn build_body(req: &ResponseRequest, stream: bool) -> Value {
    build_body_for_provider(req, stream, "")
}

/// Provider-aware Chat Completions body. Kimi's compatible API has two small
/// extensions: an explicit thinking toggle and strict JSON-schema property
/// types for tools.
pub fn build_body_for_provider(req: &ResponseRequest, stream: bool, provider_id: &str) -> Value {
    let mut messages: Vec<Value> = Vec::new();
    if let Some(instr) = &req.instructions {
        if !instr.is_empty() {
            messages.push(json!({ "role": "system", "content": instr }));
        }
    }
    if let Value::Array(items) = &req.input {
        for item in items {
            push_item_messages(item, &mut messages);
        }
    }

    let mut body = json!({
        "model": req.model,
        "messages": messages,
    });

    if let Some(tools) = &req.tools {
        let mapped: Vec<Value> = tools
            .iter()
            .map(|t| {
                let mut parameters = t
                    .parameters
                    .clone()
                    .unwrap_or_else(|| json!({"type":"object","properties":{}}));
                if provider_id == "kimi" {
                    ensure_kimi_schema_types(&mut parameters);
                }
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": parameters,
                    }
                })
            })
            .collect();
        if !mapped.is_empty() {
            body["tools"] = json!(mapped);
            body["tool_choice"] = json!("auto");
            if req.parallel_tool_calls == Some(false) {
                body["parallel_tool_calls"] = json!(false);
            }
        }
    }

    if stream {
        body["stream"] = json!(true);
        // Ask providers that support it to include a final usage frame.
        body["stream_options"] = json!({ "include_usage": true });
    }
    if provider_id == "kimi" && req.reasoning.is_some() {
        body["thinking"] = json!({ "type": "enabled" });
    }
    body
}

/// Kimi rejects nested tool properties that omit JSON Schema `type`. Infer a
/// conservative type from structure or the first enum value, matching the
/// first-party Kimi provider's normalization behavior.
fn ensure_kimi_schema_types(schema: &mut Value) {
    let Value::Object(object) = schema else {
        return;
    };
    if !object.contains_key("type") {
        let inferred = if object.contains_key("properties") {
            Some("object")
        } else if object.contains_key("items") {
            Some("array")
        } else {
            object
                .get("enum")
                .and_then(Value::as_array)
                .and_then(|values| values.first())
                .and_then(|value| match value {
                    Value::String(_) => Some("string"),
                    Value::Bool(_) => Some("boolean"),
                    Value::Number(number) if number.is_i64() || number.is_u64() => Some("integer"),
                    Value::Number(_) => Some("number"),
                    Value::Array(_) => Some("array"),
                    Value::Object(_) => Some("object"),
                    Value::Null => None,
                })
        };
        if let Some(inferred) = inferred {
            object.insert("type".into(), Value::String(inferred.into()));
        }
    }
    if let Some(Value::Object(properties)) = object.get_mut("properties") {
        for property in properties.values_mut() {
            ensure_kimi_schema_types(property);
        }
    }
    if let Some(items) = object.get_mut("items") {
        ensure_kimi_schema_types(items);
    }
    for keyword in ["anyOf", "oneOf", "allOf"] {
        if let Some(Value::Array(branches)) = object.get_mut(keyword) {
            for branch in branches {
                ensure_kimi_schema_types(branch);
            }
        }
    }
}

/// Translate one Responses `input` item into zero or more chat messages.
fn push_item_messages(item: &Value, out: &mut Vec<Value>) {
    // function_call_output → a `tool` role message.
    if item.get("type").and_then(|t| t.as_str()) == Some("function_call_output") {
        out.push(json!({
            "role": "tool",
            "tool_call_id": item.get("call_id").and_then(|v| v.as_str()).unwrap_or(""),
            "content": item.get("output").and_then(|v| v.as_str()).unwrap_or(""),
        }));
        return;
    }
    // function_call → an assistant message carrying a tool_call.
    if item.get("type").and_then(|t| t.as_str()) == Some("function_call") {
        let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
        out.push(json!({
            "role": "assistant",
            "tool_calls": [{
                "id": call_id,
                "type": "function",
                "function": {
                    "name": item.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    "arguments": item.get("arguments").and_then(|v| v.as_str()).unwrap_or("{}"),
                }
            }]
        }));
        return;
    }
    // reasoning items have no chat-completions equivalent — drop them.
    if item.get("type").and_then(|t| t.as_str()) == Some("reasoning") {
        return;
    }

    // Otherwise it's a role message ({role, content:[parts]}).
    let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("user");
    let text = collect_text(item.get("content"));
    let images = collect_images(item.get("content"));
    if images.is_empty() {
        out.push(json!({ "role": role, "content": text }));
    } else {
        // Multimodal user message: OpenAI content-parts form.
        let mut parts = vec![json!({"type":"text","text": text})];
        for url in images {
            parts.push(json!({"type":"image_url","image_url":{"url": url}}));
        }
        out.push(json!({ "role": role, "content": parts }));
    }
}

/// Concatenate the text of a Responses content array (input_text / output_text).
fn collect_text(content: Option<&Value>) -> String {
    let Some(Value::Array(parts)) = content else {
        // Some producers use a bare string content.
        return content.and_then(|v| v.as_str()).unwrap_or("").to_string();
    };
    let mut s = String::new();
    for p in parts {
        let ty = p.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if ty == "input_text" || ty == "output_text" || ty == "text" {
            if let Some(t) = p.get("text").and_then(|v| v.as_str()) {
                if !s.is_empty() {
                    s.push('\n');
                }
                s.push_str(t);
            }
        }
    }
    s
}

/// Pull image URLs (Meta `input_image` → OpenAI `image_url`).
fn collect_images(content: Option<&Value>) -> Vec<String> {
    let mut urls = Vec::new();
    if let Some(Value::Array(parts)) = content {
        for p in parts {
            if p.get("type").and_then(|v| v.as_str()) == Some("input_image") {
                if let Some(u) = p.get("image_url").and_then(|v| v.as_str()) {
                    urls.push(u.to_string());
                }
            }
        }
    }
    urls
}

/// Reconstruct a Responses `ApiResponse` from a non-streamed chat completion.
pub fn parse_completion(v: &Value) -> Value {
    let msg = v.pointer("/choices/0/message").cloned().unwrap_or(json!({}));
    let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");
    let tool_calls = msg
        .get("tool_calls")
        .and_then(|t| t.as_array())
        .cloned()
        .unwrap_or_default();
    build_response_value(
        v.get("id").and_then(|x| x.as_str()),
        v.get("model").and_then(|x| x.as_str()),
        content,
        &tool_calls,
        v.get("usage"),
    )
}

/// Build a Responses-shaped response object (deserialized by the caller).
pub fn build_response_value(
    id: Option<&str>,
    model: Option<&str>,
    content: &str,
    tool_calls: &[Value],
    usage: Option<&Value>,
) -> Value {
    let mut output: Vec<Value> = Vec::new();
    if !content.is_empty() {
        output.push(json!({
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": content }],
        }));
    }
    for tc in tool_calls {
        let func = tc.get("function").cloned().unwrap_or(json!({}));
        output.push(json!({
            "type": "function_call",
            "call_id": tc.get("id").and_then(|v| v.as_str()).unwrap_or(""),
            "name": func.get("name").and_then(|v| v.as_str()).unwrap_or(""),
            "arguments": func.get("arguments").and_then(|v| v.as_str()).unwrap_or("{}"),
        }));
    }
    let usage_obj = usage
        .map(|u| {
            json!({
                "input_tokens": u.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                "output_tokens": u.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                "total_tokens": u.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
            })
        })
        .unwrap_or(json!({}));
    json!({
        "id": id,
        "status": "completed",
        "model": model,
        "output": output,
        "usage": usage_obj,
    })
}

/// Accumulates streamed chat-completions deltas into a final response.
#[derive(Default)]
pub struct StreamAccumulator {
    pub id: Option<String>,
    pub model: Option<String>,
    pub content: String,
    /// tool_calls by index: (id, name, arguments-fragments).
    calls: Vec<(String, String, String)>,
    usage: Option<Value>,
}

impl StreamAccumulator {
    /// Feed one SSE `data:` JSON object. Returns any text delta to surface.
    pub fn push(&mut self, v: &Value) -> Option<String> {
        if self.id.is_none() {
            self.id = v.get("id").and_then(|x| x.as_str()).map(String::from);
        }
        if self.model.is_none() {
            self.model = v.get("model").and_then(|x| x.as_str()).map(String::from);
        }
        if let Some(u) = v.get("usage") {
            if u.is_object() {
                self.usage = Some(u.clone());
            }
        }
        let delta = v.pointer("/choices/0/delta");
        let mut text_out = None;
        if let Some(delta) = delta {
            if let Some(c) = delta.get("content").and_then(|c| c.as_str()) {
                if !c.is_empty() {
                    self.content.push_str(c);
                    text_out = Some(c.to_string());
                }
            }
            if let Some(tcs) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                for tc in tcs {
                    let idx = tc.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    while self.calls.len() <= idx {
                        self.calls.push((String::new(), String::new(), String::new()));
                    }
                    let slot = &mut self.calls[idx];
                    if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                        if !id.is_empty() {
                            slot.0 = id.to_string();
                        }
                    }
                    if let Some(f) = tc.get("function") {
                        if let Some(n) = f.get("name").and_then(|v| v.as_str()) {
                            if !n.is_empty() {
                                slot.1 = n.to_string();
                            }
                        }
                        if let Some(a) = f.get("arguments").and_then(|v| v.as_str()) {
                            slot.2.push_str(a);
                        }
                    }
                }
            }
        }
        text_out
    }

    /// Assemble the final Responses-shaped value once the stream ends.
    pub fn finish(&self) -> Value {
        let tool_calls: Vec<Value> = self
            .calls
            .iter()
            .filter(|(_, name, _)| !name.is_empty())
            .map(|(id, name, args)| {
                json!({
                    "id": if id.is_empty() { json!(format!("call_{name}")) } else { json!(id) },
                    "type": "function",
                    "function": { "name": name, "arguments": if args.is_empty() { "{}" } else { args.as_str() } }
                })
            })
            .collect();
        build_response_value(
            self.id.as_deref(),
            self.model.as_deref(),
            &self.content,
            &tool_calls,
            self.usage.as_ref(),
        )
    }
}

/// Deserialize a Responses-shaped value into `ApiResponse`.
pub fn to_api_response(v: Value) -> Result<ApiResponse, serde_json::Error> {
    serde_json::from_value(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::types::ToolDef;

    fn req() -> ResponseRequest {
        ResponseRequest {
            model: "gpt-x".into(),
            input: json!([
                {"role":"user","content":[{"type":"input_text","text":"hi"}]},
                {"type":"function_call","call_id":"c1","name":"grep","arguments":"{\"pattern\":\"x\"}"},
                {"type":"function_call_output","call_id":"c1","output":"match"},
                {"role":"assistant","content":[{"type":"output_text","text":"done"}]}
            ]),
            instructions: Some("be terse".into()),
            tools: Some(vec![ToolDef {
                type_: "function".into(),
                name: "grep".into(),
                description: Some("search".into()),
                parameters: Some(json!({"type":"object"})),
            }]),
            tool_choice: Some("auto".into()),
            store: None,
            include: None,
            reasoning: None,
            stream: None,
            parallel_tool_calls: None,
            prompt_cache_key: None,
        }
    }

    #[test]
    fn body_maps_messages_tools_and_roles() {
        let b = build_body(&req(), false);
        let msgs = b["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(msgs[1]["content"], "hi");
        assert_eq!(msgs[2]["role"], "assistant");
        assert_eq!(msgs[2]["tool_calls"][0]["function"]["name"], "grep");
        assert_eq!(msgs[3]["role"], "tool");
        assert_eq!(msgs[3]["tool_call_id"], "c1");
        assert_eq!(msgs[4]["role"], "assistant");
        assert_eq!(msgs[4]["content"], "done");
        assert_eq!(b["tools"][0]["function"]["name"], "grep");
        assert_eq!(b["tool_choice"], "auto");
    }

    #[test]
    fn kimi_body_enables_thinking_and_repairs_enum_only_tool_properties() {
        let mut request = req();
        request.tools.as_mut().unwrap()[0].parameters = Some(json!({
            "type": "object",
            "properties": {
                "mode": {"enum": ["fast", "safe"]},
                "options": {"properties": {"confirm": {"enum": [true, false]}}}
            }
        }));
        request.reasoning = Some(crate::api::types::ReasoningConfig {
            effort: Some("high".into()),
            summary: Some("auto".into()),
        });

        let body = build_body_for_provider(&request, true, "kimi");
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(
            body["tools"][0]["function"]["parameters"]["properties"]["mode"]["type"],
            "string"
        );
        assert_eq!(
            body["tools"][0]["function"]["parameters"]["properties"]["options"]["type"],
            "object"
        );
        assert_eq!(
            body["tools"][0]["function"]["parameters"]["properties"]["options"]["properties"]
                ["confirm"]["type"],
            "boolean"
        );
    }

    #[test]
    fn parse_completion_yields_text_and_calls() {
        let v = json!({
            "id":"r1","model":"gpt-x",
            "choices":[{"message":{
                "content":"hello",
                "tool_calls":[{"id":"c9","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"a\"}"}}]
            }}],
            "usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}
        });
        let resp = to_api_response(parse_completion(&v)).unwrap();
        assert_eq!(resp.output_text(), "hello");
        let calls = resp.function_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[0].call_id, "c9");
        assert_eq!(resp.usage.as_ref().unwrap().input_tokens, 10);
    }

    #[test]
    fn streaming_accumulates_content_and_tool_args() {
        let mut acc = StreamAccumulator::default();
        acc.push(&json!({"id":"r","model":"m","choices":[{"delta":{"content":"He"}}]}));
        acc.push(&json!({"choices":[{"delta":{"content":"llo"}}]}));
        acc.push(&json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"grep","arguments":"{\"p\":"}}]}}]}));
        acc.push(&json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"x\"}"}}]}}]}));
        acc.push(&json!({"usage":{"prompt_tokens":3,"completion_tokens":4,"total_tokens":7}}));
        let resp = to_api_response(acc.finish()).unwrap();
        assert_eq!(resp.output_text(), "Hello");
        let calls = resp.function_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "grep");
        assert_eq!(calls[0].arguments, "{\"p\":\"x\"}");
        assert_eq!(resp.usage.as_ref().unwrap().output_tokens, 4);
    }
}

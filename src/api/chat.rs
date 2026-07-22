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
#[cfg(test)]
pub fn build_body_for_provider(req: &ResponseRequest, stream: bool, provider_id: &str) -> Value {
    build_body_opts(req, stream, provider_id, false)
}

/// Placeholder swapped in for an image/video part when the endpoint has no
/// vision support. Keeps the turn's shape (and the model's awareness that
/// something was attached) without tripping a text-only server.
const MEDIA_DROPPED: &str = "[attachment omitted - this model/endpoint has no vision support]";

/// Same as [`build_body_for_provider`], but `drop_media` replaces every image /
/// video content part with a short text marker.
///
/// Text-only endpoints (a `llama-server` without an `mmproj`, most local
/// runtimes) reject a request that carries `image_url` parts with a hard 500.
/// A session that once attached a screenshot replays that image on *every*
/// later turn, so switching to a local model would fail forever. The client
/// retries such a request with `drop_media` set and remembers the endpoint.
pub fn build_body_opts(
    req: &ResponseRequest,
    stream: bool,
    provider_id: &str,
    drop_media: bool,
) -> Value {
    let mut messages: Vec<Value> = Vec::new();
    if let Some(instr) = &req.instructions {
        if !instr.is_empty() {
            messages.push(json!({ "role": "system", "content": instr }));
        }
    }
    if let Value::Array(items) = &req.input {
        let mut index = 0;
        while index < items.len() {
            if items[index].get("type").and_then(|t| t.as_str()) == Some("function_call") {
                // A response can contain multiple tool calls. OpenAI-compatible
                // providers expect them in one assistant turn, followed by the
                // corresponding tool results. Zen tolerated split turns, but
                // OpenCode Go forwards the stricter upstream protocol.
                let mut tool_calls = Vec::new();
                while index < items.len()
                    && items[index].get("type").and_then(|t| t.as_str()) == Some("function_call")
                {
                    tool_calls.push(chat_tool_call(&items[index]));
                    index += 1;
                }
                messages.push(json!({
                    "role": "assistant",
                    "content": Value::Null,
                    "tool_calls": tool_calls,
                }));
                continue;
            }
            push_item_messages_opts(&items[index], &mut messages, drop_media);
            index += 1;
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
fn push_item_messages_opts(item: &Value, out: &mut Vec<Value>, drop_media: bool) {
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
    if drop_media && !images.is_empty() {
        let mut text = text;
        for _ in 0..images.len() {
            text.push('\n');
            text.push_str(MEDIA_DROPPED);
        }
        out.push(json!({ "role": role, "content": text }));
    } else if images.is_empty() {
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

fn chat_tool_call(item: &Value) -> Value {
    json!({
        "id": item.get("call_id").and_then(|v| v.as_str()).unwrap_or(""),
        "type": "function",
        "function": {
            "name": item.get("name").and_then(|v| v.as_str()).unwrap_or(""),
            "arguments": item.get("arguments").and_then(|v| v.as_str()).unwrap_or("{}"),
        }
    })
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

/// Does this Responses request carry any image/video part at all?
///
/// Cheap pre-check so the "endpoint has no vision" retry only fires when
/// dropping media would actually change the body.
pub fn request_has_media(req: &ResponseRequest) -> bool {
    let Value::Array(items) = &req.input else {
        return false;
    };
    items
        .iter()
        .any(|item| !collect_images(item.get("content")).is_empty())
}

/// Does this provider error mean "I can't accept images"?
///
/// llama.cpp/`llama-server` answers a request with an `image_url` part and no
/// multimodal projector with a 500 whose message is
/// `image input is not supported - hint: … provide the mmproj`; other runtimes
/// word it differently, so match on the shape rather than one string.
pub fn is_media_unsupported_error(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    if m.contains("mmproj") {
        return true;
    }
    let mentions_media = m.contains("image") || m.contains("vision") || m.contains("multimodal");
    let mentions_refusal = m.contains("not supported")
        || m.contains("unsupported")
        || m.contains("does not support")
        || m.contains("doesn't support")
        || m.contains("no support");
    mentions_media && mentions_refusal
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
    let msg = v
        .pointer("/choices/0/message")
        .cloned()
        .unwrap_or(json!({}));
    let raw = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");
    // Local reasoning models fold their scratchpad into `content`; keep it out
    // of the answer. (The dedicated field, when present, is already separate.)
    let (_reasoning, content) = split_think(raw);
    let tool_calls = msg
        .get("tool_calls")
        .and_then(|t| t.as_array())
        .cloned()
        .unwrap_or_default();
    build_response_value(
        v.get("id").and_then(|x| x.as_str()),
        v.get("model").and_then(|x| x.as_str()),
        &content,
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

/// One piece of a streamed reply, routed by kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatDelta {
    Text(String),
    Reasoning(String),
}

/// Opening / closing markers for inline reasoning.
const THINK_OPEN: &str = "<think>";
const THINK_CLOSE: &str = "</think>";

/// Splits a streamed content field into answer text and inline reasoning.
///
/// Reasoning models served locally (Qwen3, DeepSeek-R1 and friends behind
/// llama.cpp / LM Studio / Ollama) do not use a separate reasoning field — they
/// emit `<think>…</think>` in the middle of `content`. Without this the chain of
/// thought lands in the assistant's answer and the TUI reports "thought 0ms".
///
/// The markers can straddle chunk boundaries, so a short tail is held back
/// whenever the buffer ends with something that could still become a marker.
#[derive(Default)]
pub struct ThinkSplitter {
    in_think: bool,
    pending: String,
}

impl ThinkSplitter {
    pub fn push(&mut self, chunk: &str) -> Vec<ChatDelta> {
        self.pending.push_str(chunk);
        let mut out = Vec::new();
        loop {
            let marker = if self.in_think {
                THINK_CLOSE
            } else {
                THINK_OPEN
            };
            match self.pending.find(marker) {
                Some(at) => {
                    let head: String = self.pending[..at].to_string();
                    if !head.is_empty() {
                        out.push(self.wrap(head));
                    }
                    self.pending = self.pending[at + marker.len()..].to_string();
                    self.in_think = !self.in_think;
                }
                None => {
                    // Hold back anything that could be the start of a marker.
                    let keep = partial_marker_len(&self.pending, marker);
                    let split = self.pending.len() - keep;
                    if split > 0 {
                        let head: String = self.pending[..split].to_string();
                        self.pending = self.pending[split..].to_string();
                        out.push(self.wrap(head));
                    }
                    return out;
                }
            }
        }
    }

    /// Emit whatever is still buffered when the stream ends.
    pub fn flush(&mut self) -> Option<ChatDelta> {
        if self.pending.is_empty() {
            return None;
        }
        let rest = std::mem::take(&mut self.pending);
        Some(self.wrap(rest))
    }

    fn wrap(&self, text: String) -> ChatDelta {
        if self.in_think {
            ChatDelta::Reasoning(text)
        } else {
            ChatDelta::Text(text)
        }
    }
}

/// Length of the longest suffix of `buf` that is a proper prefix of `marker`.
fn partial_marker_len(buf: &str, marker: &str) -> usize {
    // A held-back suffix can be as long as the buffer, but never a whole
    // marker — that case was already matched and consumed by the caller.
    let max = (marker.len() - 1).min(buf.len());
    (1..=max)
        .rev()
        .find(|&n| buf.is_char_boundary(buf.len() - n) && marker.starts_with(&buf[buf.len() - n..]))
        .unwrap_or(0)
}

/// Strip `<think>` blocks out of a non-streamed completion.
///
/// Returns `(reasoning, answer)`. An unterminated block is treated as reasoning
/// to the end — a truncated reply should not spill its scratchpad into the
/// answer.
pub fn split_think(content: &str) -> (String, String) {
    let mut splitter = ThinkSplitter::default();
    let mut reasoning = String::new();
    let mut answer = String::new();
    let mut take = |d: ChatDelta| match d {
        ChatDelta::Text(t) => answer.push_str(&t),
        ChatDelta::Reasoning(t) => reasoning.push_str(&t),
    };
    for d in splitter.push(content) {
        take(d);
    }
    if let Some(d) = splitter.flush() {
        take(d);
    }
    (reasoning.trim().to_string(), answer.trim().to_string())
}

/// Accumulates streamed chat-completions deltas into a final response.
#[derive(Default)]
pub struct StreamAccumulator {
    pub id: Option<String>,
    pub model: Option<String>,
    pub content: String,
    /// Reasoning seen this stream, from either transport.
    pub reasoning: String,
    /// tool_calls by index: (id, name, arguments-fragments).
    calls: Vec<(String, String, String)>,
    usage: Option<Value>,
    think: ThinkSplitter,
}

impl StreamAccumulator {
    /// Feed one SSE `data:` JSON object. Returns the deltas to surface, split
    /// into answer text and reasoning.
    ///
    /// Reasoning arrives two different ways depending on the runtime: a
    /// dedicated `reasoning_content` / `reasoning` delta field (DeepSeek, vLLM,
    /// LM Studio, OpenRouter), or inline `<think>` markers inside `content`
    /// (llama.cpp serving Qwen3/R1-style models). Both are handled here so the
    /// TUI's thinking cell works against a local server, not just a cloud API.
    pub fn push(&mut self, v: &Value) -> Vec<ChatDelta> {
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
        let mut out: Vec<ChatDelta> = Vec::new();
        if let Some(delta) = delta {
            // Dedicated reasoning field, under either of its two spellings.
            for key in ["reasoning_content", "reasoning"] {
                if let Some(r) = delta.get(key).and_then(|c| c.as_str()) {
                    if !r.is_empty() {
                        self.reasoning.push_str(r);
                        out.push(ChatDelta::Reasoning(r.to_string()));
                    }
                }
            }
            if let Some(c) = delta.get("content").and_then(|c| c.as_str()) {
                if !c.is_empty() {
                    for d in self.think.push(c) {
                        match &d {
                            ChatDelta::Text(t) => self.content.push_str(t),
                            ChatDelta::Reasoning(t) => self.reasoning.push_str(t),
                        }
                        out.push(d);
                    }
                }
            }
            if let Some(tcs) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                for tc in tcs {
                    let idx = tc.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    while self.calls.len() <= idx {
                        self.calls
                            .push((String::new(), String::new(), String::new()));
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
        out
    }

    /// Assemble the final Responses-shaped value once the stream ends.
    pub fn finish(&mut self) -> Value {
        // A stream can end mid-marker (or mid-think on a truncated reply);
        // whatever is still buffered belongs to the side we were on.
        if let Some(d) = self.think.flush() {
            match d {
                ChatDelta::Text(t) => self.content.push_str(&t),
                ChatDelta::Reasoning(t) => self.reasoning.push_str(&t),
            }
        }
        self.finish_ref()
    }

    fn finish_ref(&self) -> Value {
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
    fn consecutive_tool_calls_share_one_assistant_message() {
        let mut request = req();
        request.input = json!([
            {"role":"user","content":[{"type":"input_text","text":"inspect"}]},
            {"type":"function_call","call_id":"a","name":"git_status","arguments":"{}"},
            {"type":"function_call","call_id":"b","name":"list_dir","arguments":"{}"},
            {"type":"function_call_output","call_id":"a","output":"clean"},
            {"type":"function_call_output","call_id":"b","output":"src"}
        ]);
        let body = build_body(&request, false);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages[2]["tool_calls"].as_array().unwrap().len(), 2);
        assert!(messages[2]["content"].is_null());
        assert_eq!(messages[3]["tool_call_id"], "a");
        assert_eq!(messages[4]["tool_call_id"], "b");
    }

    /// A request whose history carries a screenshot, as any session that once
    /// used `look`/auto-attach replays on every later turn.
    fn req_with_image() -> ResponseRequest {
        let mut r = req();
        r.input = json!([crate::api::types::user_multimodal_item(
            "what is this",
            &[("input_image", "image_url", "data:image/png;base64,AAAA")],
        )]);
        r
    }

    #[test]
    fn image_parts_are_detected_and_mapped_when_the_endpoint_supports_them() {
        let request = req_with_image();
        assert!(request_has_media(&request));
        assert!(!request_has_media(&req()));

        let body = build_body_opts(&request, false, "", false);
        let parts = body["messages"][1]["content"].as_array().unwrap();
        assert_eq!(parts[0]["type"], "text");
        assert_eq!(parts[1]["type"], "image_url");
    }

    #[test]
    fn drop_media_replaces_images_with_a_text_marker() {
        let body = build_body_opts(&req_with_image(), false, "", true);
        let content = body["messages"][1]["content"].as_str().unwrap();
        assert!(content.starts_with("what is this"));
        assert!(content.contains(MEDIA_DROPPED));
        // Crucially: no content-parts array survives for a text-only server.
        assert!(body["messages"][1]["content"].is_string());
        assert!(!body.to_string().contains("image_url"));
    }

    #[test]
    fn media_unsupported_errors_are_recognised_across_runtimes() {
        // llama.cpp / llama-server without an mmproj — the exact 500 body.
        assert!(is_media_unsupported_error(
            "image input is not supported - hint: if this is unexpected, you may need to provide the mmproj"
        ));
        assert!(is_media_unsupported_error(
            "This model does not support image input."
        ));
        assert!(is_media_unsupported_error("multimodal input unsupported"));
        // Unrelated failures must not trigger an attachment-stripping retry.
        assert!(!is_media_unsupported_error("context length exceeded"));
        assert!(!is_media_unsupported_error("rate limit reached"));
        assert!(!is_media_unsupported_error("failed to load image"));
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

    // ── local reasoning models ───────────────────────────────────────────

    fn drain(acc: &mut StreamAccumulator, chunks: &[&str]) -> (String, String) {
        let (mut text, mut reasoning) = (String::new(), String::new());
        for c in chunks {
            for d in acc.push(&json!({"choices":[{"delta":{"content": c}}]})) {
                match d {
                    ChatDelta::Text(t) => text.push_str(&t),
                    ChatDelta::Reasoning(t) => reasoning.push_str(&t),
                }
            }
        }
        (text, reasoning)
    }

    #[test]
    fn inline_think_blocks_are_routed_to_reasoning_not_the_answer() {
        let mut acc = StreamAccumulator::default();
        let (text, reasoning) = drain(
            &mut acc,
            &[
                "<think>let me check",
                " the tests</think>",
                "The answer is 42.",
            ],
        );
        assert_eq!(reasoning, "let me check the tests");
        assert_eq!(text, "The answer is 42.");
        let resp = to_api_response(acc.finish()).unwrap();
        assert_eq!(
            resp.output_text(),
            "The answer is 42.",
            "the scratchpad must never reach the transcript"
        );
    }

    /// The marker can be split across SSE frames — the classic way a naive
    /// implementation leaks `<thi` into the answer.
    #[test]
    fn think_markers_split_across_chunks_still_parse() {
        let mut acc = StreamAccumulator::default();
        let (text, reasoning) = drain(&mut acc, &["<th", "ink>hmm</thi", "nk>done"]);
        assert_eq!(reasoning, "hmm");
        assert_eq!(text, "done");
    }

    /// Text that merely *starts* like a marker must not be swallowed.
    #[test]
    fn a_lone_angle_bracket_is_not_held_hostage() {
        let mut acc = StreamAccumulator::default();
        let (text, _) = drain(&mut acc, &["a < b and c > d"]);
        assert_eq!(text, "a < b and c > d");
    }

    #[test]
    fn an_unterminated_think_block_does_not_leak_on_flush() {
        let mut acc = StreamAccumulator::default();
        let (text, reasoning) = drain(&mut acc, &["<think>cut off mid-thou"]);
        assert_eq!(text, "");
        assert_eq!(reasoning, "cut off mid-thou");
        let resp = to_api_response(acc.finish()).unwrap();
        assert_eq!(
            resp.output_text(),
            "",
            "truncated thinking is not an answer"
        );
        assert!(acc.reasoning.contains("cut off"));
    }

    /// DeepSeek/vLLM/LM Studio use a dedicated field; OpenRouter spells it
    /// `reasoning`. Both must reach the thinking cell.
    #[test]
    fn dedicated_reasoning_fields_are_recognised() {
        for key in ["reasoning_content", "reasoning"] {
            let mut acc = StreamAccumulator::default();
            let out = acc.push(&json!({"choices":[{"delta":{ key: "weighing options" }}]}));
            assert_eq!(
                out,
                vec![ChatDelta::Reasoning("weighing options".into())],
                "{key} must be surfaced as reasoning"
            );
            let out = acc.push(&json!({"choices":[{"delta":{"content":"ok"}}]}));
            assert_eq!(out, vec![ChatDelta::Text("ok".into())]);
            assert_eq!(acc.reasoning, "weighing options");
        }
    }

    #[test]
    fn non_streamed_completions_are_stripped_too() {
        let v = json!({
            "id": "r", "model": "m",
            "choices": [{"message": {"content": "<think>plan</think>Final answer."}}]
        });
        let resp = to_api_response(parse_completion(&v)).unwrap();
        assert_eq!(resp.output_text(), "Final answer.");

        let (reasoning, answer) = split_think("<think>a</think>b<think>c</think>d");
        assert_eq!(reasoning, "ac", "every block is collected");
        assert_eq!(answer, "bd");

        // Content with no markers is returned untouched.
        assert_eq!(
            split_think("just an answer"),
            (String::new(), "just an answer".into())
        );
    }
}

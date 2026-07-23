//! Google **Cloud Code** (Gemini native) adapter.
//!
//! Antigravity / gcloud OAuth (`ya29.`) tokens are Google access tokens, not
//! Gemini API keys. Against the generativelanguage OpenAI-compat host they are
//! rejected with 401 UNAUTHENTICATED even with `x-goog-user-project`; against
//! `cloudcode-pa.googleapis.com` over the native Gemini protocol
//! (`v1internal:streamGenerateContent`) the exact same token returns 200.
//!
//! This module translates nur-cli's Responses-shaped [`ResponseRequest`] into a
//! Cloud Code body `{"project":..,"model":..,"request":{...}}`, and translates
//! the response (`candidates[].content.parts[].text` plus `functionCall` parts,
//! and `usageMetadata`) back into an [`ApiResponse`] by reusing the same
//! Responses reconstruction the Chat Completions adapter uses.

use super::types::{ApiResponse, ResponseRequest};
use serde_json::{json, Value};

/// Build a Cloud Code request body from a Responses request.
///
/// `project` is the Cloud Code `cloudaicompanionProject` id (from the OAuth
/// session's `project_id`); `model` is the already-normalized Gemini id.
pub fn build_body(req: &ResponseRequest, project: &str, model: &str) -> Value {
    let mut contents: Vec<Value> = Vec::new();
    // A run of function_call items maps to a single `model` turn whose parts are
    // functionCall entries; function_call_output items become a `user` turn with
    // functionResponse parts, mirroring the Gemini protocol.
    if let Value::Array(items) = &req.input {
        for item in items {
            match item_type(item) {
                Some("reasoning") => {} // no Gemini equivalent - drop
                Some("function_call") => {
                    let name = item.get("name").and_then(Value::as_str).unwrap_or("");
                    let args = parse_args(item.get("arguments"));
                    contents.push(json!({
                        "role": "model",
                        "parts": [{ "functionCall": { "name": name, "args": args } }],
                    }));
                }
                Some("function_call_output") => {
                    let name = item.get("name").and_then(Value::as_str).unwrap_or("");
                    let output = item.get("output").and_then(Value::as_str).unwrap_or("");
                    contents.push(json!({
                        "role": "user",
                        "parts": [{ "functionResponse": {
                            "name": name,
                            "response": { "output": output },
                        }}],
                    }));
                }
                _ => {
                    if let Some(content) = message_content(item) {
                        contents.push(content);
                    }
                }
            }
        }
    }

    let mut request = json!({ "contents": contents });

    if let Some(instr) = &req.instructions {
        if !instr.is_empty() {
            request["systemInstruction"] = json!({
                "role": "user",
                "parts": [{ "text": instr }],
            });
        }
    }

    if let Some(tools) = &req.tools {
        let decls: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description.clone().unwrap_or_default(),
                    "parameters": t
                        .parameters
                        .clone()
                        .unwrap_or_else(|| json!({ "type": "object", "properties": {} })),
                })
            })
            .collect();
        if !decls.is_empty() {
            request["tools"] = json!([{ "functionDeclarations": decls }]);
        }
    }

    json!({
        "project": project,
        "model": model,
        "request": request,
    })
}

/// The Responses `type` discriminator of one input item, if it has one.
fn item_type(item: &Value) -> Option<&str> {
    item.get("type").and_then(Value::as_str)
}

/// Function-call arguments arrive as a JSON string in the Responses shape;
/// Gemini wants a JSON object. Parse, defaulting to `{}`.
fn parse_args(arguments: Option<&Value>) -> Value {
    match arguments {
        Some(Value::String(s)) => serde_json::from_str(s).unwrap_or_else(|_| json!({})),
        Some(v @ Value::Object(_)) => v.clone(),
        _ => json!({}),
    }
}

/// Translate a Responses role message ({role, content:[parts]}) into a Gemini
/// content turn. Returns `None` when there is nothing textual to send.
fn message_content(item: &Value) -> Option<Value> {
    let role = match item.get("role").and_then(Value::as_str) {
        // Gemini roles are "user" and "model"; assistant/system fold to those.
        Some("assistant") | Some("model") => "model",
        _ => "user",
    };
    let text = collect_text(item.get("content"));
    if text.is_empty() {
        return None;
    }
    Some(json!({ "role": role, "parts": [{ "text": text }] }))
}

/// Concatenate the text of a Responses content array (input_text / output_text
/// / text), or a bare string content.
fn collect_text(content: Option<&Value>) -> String {
    let Some(Value::Array(parts)) = content else {
        return content.and_then(Value::as_str).unwrap_or("").to_string();
    };
    let mut s = String::new();
    for p in parts {
        let ty = p.get("type").and_then(Value::as_str).unwrap_or("");
        if matches!(ty, "input_text" | "output_text" | "text") {
            if let Some(t) = p.get("text").and_then(Value::as_str) {
                if !s.is_empty() {
                    s.push('\n');
                }
                s.push_str(t);
            }
        }
    }
    s
}

/// Concatenate the text of the first candidate's parts from ONE Cloud Code
/// response object. The stream wraps each frame as `{"response": {...}}`; the
/// non-stream `:generateContent` returns the object directly.
fn candidate_text(response: &Value) -> String {
    let parts = response.pointer("/candidates/0/content/parts");
    let Some(Value::Array(parts)) = parts else {
        return String::new();
    };
    let mut s = String::new();
    for p in parts {
        if let Some(t) = p.get("text").and_then(Value::as_str) {
            s.push_str(t);
        }
    }
    s
}

/// Collect functionCall parts from the first candidate as chat-shaped tool calls
/// (`{id, function:{name, arguments}}`) so [`build_response_value_with_status`]
/// can reconstruct Responses `function_call` items.
fn candidate_tool_calls(response: &Value) -> Vec<Value> {
    let mut calls = Vec::new();
    let Some(Value::Array(parts)) = response.pointer("/candidates/0/content/parts") else {
        return calls;
    };
    for (i, p) in parts.iter().enumerate() {
        if let Some(fc) = p.get("functionCall") {
            let name = fc.get("name").and_then(Value::as_str).unwrap_or("");
            let args = fc.get("args").cloned().unwrap_or_else(|| json!({}));
            let arguments = serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());
            calls.push(json!({
                "id": format!("call_{i}"),
                "type": "function",
                "function": { "name": name, "arguments": arguments },
            }));
        }
    }
    calls
}

/// Finish reason of the first candidate, mapped to a Chat-Completions-ish token
/// so the shared reconstruction sets an equivalent status.
fn finish_reason(response: &Value) -> Option<String> {
    let reason = response
        .pointer("/candidates/0/finishReason")
        .and_then(Value::as_str)?;
    let mapped = match reason {
        "MAX_TOKENS" => "length",
        "SAFETY" | "RECITATION" | "BLOCKLIST" | "PROHIBITED_CONTENT" => "content_filter",
        _ => "stop",
    };
    Some(mapped.to_string())
}

/// Map `usageMetadata` onto the OpenAI-style `usage` object the shared
/// reconstruction consumes.
fn usage_value(response: &Value) -> Option<Value> {
    let u = response.get("usageMetadata")?;
    let prompt = u
        .get("promptTokenCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let completion = u
        .get("candidatesTokenCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let total = u
        .get("totalTokenCount")
        .and_then(Value::as_u64)
        .unwrap_or(prompt + completion);
    let cached = u
        .get("cachedContentTokenCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Some(json!({
        "prompt_tokens": prompt,
        "completion_tokens": completion,
        "total_tokens": total,
        "prompt_tokens_details": { "cached_tokens": cached },
    }))
}

/// A single Cloud Code frame is either the bare `:generateContent` object or the
/// streamed `{"response": {...}}` wrapper. Return the inner response object.
fn inner_response(frame: &Value) -> &Value {
    frame.get("response").unwrap_or(frame)
}

/// State accumulated while draining a Cloud Code (stream or non-stream) reply.
#[derive(Default)]
pub struct GeminiAccumulator {
    text: String,
    tool_calls: Vec<Value>,
    usage: Option<Value>,
    finish: Option<String>,
    model: Option<String>,
}

impl GeminiAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold one parsed JSON frame (an SSE `data:` payload or the whole
    /// non-stream body) into the accumulator, returning any text delta so
    /// streaming callers can surface it live.
    pub fn push_frame(&mut self, frame: &Value) -> Option<String> {
        let response = inner_response(frame);
        if let Some(m) = response.get("modelVersion").and_then(Value::as_str) {
            self.model = Some(m.to_string());
        }
        let delta = candidate_text(response);
        if !delta.is_empty() {
            self.text.push_str(&delta);
        }
        let mut calls = candidate_tool_calls(response);
        if !calls.is_empty() {
            self.tool_calls.append(&mut calls);
        }
        if let Some(u) = usage_value(response) {
            self.usage = Some(u);
        }
        if let Some(fr) = finish_reason(response) {
            self.finish = Some(fr);
        }
        if delta.is_empty() {
            None
        } else {
            Some(delta)
        }
    }

    /// Reconstruct the Responses-shaped value once the reply is fully drained.
    pub fn into_response_value(self) -> Value {
        super::chat::build_response_value_with_status(
            None,
            self.model.as_deref(),
            &self.text,
            &self.tool_calls,
            self.usage.as_ref(),
            self.finish.as_deref(),
        )
    }
}

/// Parse a whole non-streamed `:generateContent` body into an `ApiResponse`.
pub fn parse_completion(body: &str) -> crate::error::Result<ApiResponse> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| crate::error::MuseError::Other(format!("invalid Cloud Code JSON: {e}")))?;
    // Non-stream returns an array of chunks for streamGenerateContent without
    // `alt=sse`, or a single object for `:generateContent`.
    let mut acc = GeminiAccumulator::new();
    match &v {
        Value::Array(frames) => {
            for frame in frames {
                acc.push_frame(frame);
            }
        }
        other => {
            acc.push_frame(other);
        }
    }
    let value = acc.into_response_value();
    serde_json::from_value(value)
        .map_err(|e| crate::error::MuseError::Other(format!("map Cloud Code response failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::types::{ResponseRequest, ToolDef};

    fn sample_request() -> ResponseRequest {
        ResponseRequest {
            model: "gemini-2.5-flash".into(),
            input: json!([
                { "role": "user", "content": [{ "type": "input_text", "text": "say OK" }] },
            ]),
            instructions: Some("You are terse.".into()),
            tools: Some(vec![ToolDef {
                type_: "function".into(),
                name: "get_time".into(),
                description: Some("current time".into()),
                parameters: Some(json!({ "type": "object", "properties": {} })),
            }]),
            tool_choice: None,
            store: None,
            include: None,
            reasoning: None,
            stream: None,
            parallel_tool_calls: None,
            prompt_cache_key: None,
        }
    }

    #[test]
    fn builds_cloud_code_body_with_project_system_and_tools() {
        let body = build_body(&sample_request(), "vivid-question-5fs6l", "gemini-2.5-flash");
        assert_eq!(body["project"], "vivid-question-5fs6l");
        assert_eq!(body["model"], "gemini-2.5-flash");
        // Top-level shape matches Gemini CLI / cloudcode-pa generateContent.
        assert!(body.get("request").is_some());
        assert!(body.get("project").is_some());
        assert!(body.get("model").is_some());
        // user message translated to a Gemini content turn
        assert_eq!(body["request"]["contents"][0]["role"], "user");
        assert_eq!(
            body["request"]["contents"][0]["parts"][0]["text"],
            "say OK"
        );
        // system prompt lands in systemInstruction
        assert_eq!(
            body["request"]["systemInstruction"]["parts"][0]["text"],
            "You are terse."
        );
        // tool becomes a functionDeclarations entry
        assert_eq!(
            body["request"]["tools"][0]["functionDeclarations"][0]["name"],
            "get_time"
        );
    }

    #[test]
    fn cloud_code_body_uses_caller_normalized_bare_model_id() {
        // Client normalizes via normalize_antigravity_model_id before build_body;
        // document that the wire model must be bare (no models/ prefix).
        let bare = crate::providers::normalize_antigravity_model_id("models/gemini-2.5-flash");
        let body = build_body(&sample_request(), "proj-x", &bare);
        assert_eq!(body["model"], "gemini-2.5-flash");
        assert!(!body["model"].as_str().unwrap_or("").starts_with("models/"));
        assert_eq!(body["project"], "proj-x");
        assert!(body["request"]["contents"].is_array());
    }

    #[test]
    fn function_call_history_maps_to_model_and_user_turns() {
        let mut req = sample_request();
        req.instructions = None;
        req.tools = None;
        req.input = json!([
            { "role": "user", "content": [{ "type": "input_text", "text": "time?" }] },
            { "type": "function_call", "call_id": "c1", "name": "get_time", "arguments": "{\"tz\":\"utc\"}" },
            { "type": "function_call_output", "call_id": "c1", "name": "get_time", "output": "12:00" },
        ]);
        let body = build_body(&req, "proj", "gemini-2.5-flash");
        let contents = body["request"]["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 3);
        assert_eq!(contents[1]["role"], "model");
        assert_eq!(
            contents[1]["parts"][0]["functionCall"]["name"],
            "get_time"
        );
        assert_eq!(contents[1]["parts"][0]["functionCall"]["args"]["tz"], "utc");
        assert_eq!(contents[2]["role"], "user");
        assert_eq!(
            contents[2]["parts"][0]["functionResponse"]["name"],
            "get_time"
        );
    }

    #[test]
    fn parses_sse_frames_into_text_and_usage() {
        // Two streamed frames as they arrive on the wire (data: payloads).
        let frame1: Value = serde_json::from_str(
            r#"{"response":{"candidates":[{"content":{"role":"model","parts":[{"text":"O"}]}}]}}"#,
        )
        .unwrap();
        let frame2: Value = serde_json::from_str(
            r#"{"response":{"candidates":[{"content":{"role":"model","parts":[{"text":"K"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":3,"candidatesTokenCount":1,"totalTokenCount":4}}}"#,
        )
        .unwrap();
        let mut acc = GeminiAccumulator::new();
        assert_eq!(acc.push_frame(&frame1), Some("O".to_string()));
        assert_eq!(acc.push_frame(&frame2), Some("K".to_string()));
        let value = acc.into_response_value();
        assert_eq!(value["status"], "completed");
        assert_eq!(
            value["output"][0]["content"][0]["text"], "OK",
            "concatenated candidate text"
        );
        assert_eq!(value["usage"]["input_tokens"], 3);
        assert_eq!(value["usage"]["output_tokens"], 1);
    }

    #[test]
    fn parses_non_stream_completion_body() {
        let body = r#"{"candidates":[{"content":{"role":"model","parts":[{"text":"hello"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":5,"candidatesTokenCount":2,"totalTokenCount":7}}"#;
        let resp = parse_completion(body).unwrap();
        assert_eq!(resp.output_text(), "hello");
        let usage = resp.usage.unwrap();
        assert_eq!(usage.input_tokens, 5);
        assert_eq!(usage.output_tokens, 2);
    }

    #[test]
    fn parses_function_call_response() {
        let frame: Value = serde_json::from_str(
            r#"{"response":{"candidates":[{"content":{"role":"model","parts":[{"functionCall":{"name":"get_time","args":{"tz":"utc"}}}]},"finishReason":"STOP"}]}}"#,
        )
        .unwrap();
        let mut acc = GeminiAccumulator::new();
        acc.push_frame(&frame);
        let value = acc.into_response_value();
        assert_eq!(value["output"][0]["type"], "function_call");
        assert_eq!(value["output"][0]["name"], "get_time");
        assert_eq!(value["output"][0]["arguments"], "{\"tz\":\"utc\"}");
    }
}

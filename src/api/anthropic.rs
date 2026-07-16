//! Anthropic **Messages** API adapter (`POST /v1/messages`).
//!
//! nur-cli's agent loop speaks the OpenAI/Meta Responses shape. Anthropic does
//! **not** implement `/chat/completions` on `api.anthropic.com` — that was the
//! root cause of "Anthropic isn't working" for both API keys and Claude OAuth.
//! This module translates Responses ↔ Messages (including tools + streaming).

use super::chat::build_response_value;
use super::types::ResponseRequest;
use serde_json::{json, Value};

/// True for Claude Code / claude.ai OAuth access tokens (`sk-ant-oat…`).
pub fn is_oauth_token(key: &str) -> bool {
    let k = key.trim();
    k.starts_with("sk-ant-oat") || k.starts_with("sk-ant-oat01-")
}

/// Anthropic beta header required for OAuth bearer tokens against the API.
pub const OAUTH_BETA: &str = "oauth-2025-04-20";

/// Build a Messages API body from a Responses request.
pub fn build_body(req: &ResponseRequest, stream: bool) -> Value {
    let mut system: Option<String> = req.instructions.clone().filter(|s| !s.is_empty());
    let mut messages: Vec<Value> = Vec::new();

    if let Value::Array(items) = &req.input {
        for item in items {
            push_item(item, &mut messages, &mut system);
        }
    }
    // Anthropic requires alternating user/assistant; merge consecutive same roles.
    let messages = coalesce_roles(messages);

    let mut body = json!({
        "model": req.model,
        "max_tokens": 16_384,
        "messages": messages,
    });
    if let Some(sys) = system {
        if !sys.is_empty() {
            body["system"] = json!(sys);
        }
    }
    if let Some(tools) = &req.tools {
        let mapped: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description.clone().unwrap_or_default(),
                    "input_schema": t.parameters.clone().unwrap_or_else(|| {
                        json!({"type":"object","properties":{}})
                    }),
                })
            })
            .collect();
        if !mapped.is_empty() {
            body["tools"] = json!(mapped);
            body["tool_choice"] = json!({ "type": "auto" });
        }
    }
    if stream {
        body["stream"] = json!(true);
    }
    // Do not auto-attach Anthropic `thinking` — not all models accept it and a
    // 400 here looks like "Anthropic is broken" for users on haiku/sonnet tiers.
    let _ = &req.reasoning;
    body
}

fn push_item(item: &Value, messages: &mut Vec<Value>, system: &mut Option<String>) {
    if item.get("type").and_then(|t| t.as_str()) == Some("function_call_output") {
        let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
        let output = item.get("output").and_then(|v| v.as_str()).unwrap_or("");
        messages.push(json!({
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": call_id,
                "content": output,
            }]
        }));
        return;
    }
    if item.get("type").and_then(|t| t.as_str()) == Some("function_call") {
        let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
        let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let args_str = item.get("arguments").and_then(|v| v.as_str()).unwrap_or("{}");
        let input: Value = serde_json::from_str(args_str).unwrap_or_else(|_| json!({}));
        messages.push(json!({
            "role": "assistant",
            "content": [{
                "type": "tool_use",
                "id": call_id,
                "name": name,
                "input": input,
            }]
        }));
        return;
    }
    if item.get("type").and_then(|t| t.as_str()) == Some("reasoning") {
        return;
    }

    let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("user");
    if role == "system" {
        let text = collect_text(item.get("content"));
        if !text.is_empty() {
            match system {
                Some(s) => {
                    s.push_str("\n\n");
                    s.push_str(&text);
                }
                None => *system = Some(text),
            }
        }
        return;
    }
    let role = if role == "assistant" { "assistant" } else { "user" };
    let text = collect_text(item.get("content"));
    let images = collect_images(item.get("content"));
    if images.is_empty() {
        messages.push(json!({ "role": role, "content": text }));
    } else {
        let mut parts = Vec::new();
        if !text.is_empty() {
            parts.push(json!({ "type": "text", "text": text }));
        }
        for url in images {
            // data:image/...;base64,... or https
            if let Some(b64) = url.strip_prefix("data:") {
                // data:image/png;base64,XXXX
                if let Some((meta, data)) = b64.split_once(',') {
                    let media = meta
                        .split(';')
                        .next()
                        .unwrap_or("image/png")
                        .strip_prefix("image/")
                        .map(|t| format!("image/{t}"))
                        .unwrap_or_else(|| "image/png".into());
                    // Anthropic wants media_type like image/png
                    let media_type = if meta.starts_with("image/") {
                        meta.split(';').next().unwrap_or("image/png")
                    } else {
                        "image/png"
                    };
                    let _ = media;
                    parts.push(json!({
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": media_type,
                            "data": data,
                        }
                    }));
                }
            } else {
                parts.push(json!({
                    "type": "image",
                    "source": { "type": "url", "url": url }
                }));
            }
        }
        messages.push(json!({ "role": role, "content": parts }));
    }
}

fn collect_text(content: Option<&Value>) -> String {
    let Some(c) = content else {
        return String::new();
    };
    if let Some(s) = c.as_str() {
        return s.to_string();
    }
    let Some(Value::Array(parts)) = content else {
        return String::new();
    };
    let mut s = String::new();
    for p in parts {
        let ty = p.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if matches!(ty, "input_text" | "output_text" | "text") {
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

/// Merge consecutive messages with the same role (Anthropic rejects them).
fn coalesce_roles(msgs: Vec<Value>) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for m in msgs {
        let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("user");
        if let Some(last) = out.last_mut() {
            let last_role = last.get("role").and_then(|r| r.as_str()).unwrap_or("");
            if last_role == role {
                merge_content(last, &m);
                continue;
            }
        }
        out.push(m);
    }
    // Anthropic wants the first message to be user.
    if out
        .first()
        .and_then(|m| m.get("role"))
        .and_then(|r| r.as_str())
        == Some("assistant")
    {
        out.insert(
            0,
            json!({ "role": "user", "content": "(continue)" }),
        );
    }
    out
}

fn merge_content(into: &mut Value, from: &Value) {
    let a = normalize_blocks(into.get("content"));
    let b = normalize_blocks(from.get("content"));
    let mut all = a;
    all.extend(b);
    into["content"] = json!(all);
}

fn normalize_blocks(c: Option<&Value>) -> Vec<Value> {
    match c {
        None => Vec::new(),
        Some(Value::String(s)) => {
            if s.is_empty() {
                Vec::new()
            } else {
                vec![json!({ "type": "text", "text": s })]
            }
        }
        Some(Value::Array(arr)) => arr.clone(),
        Some(other) => vec![json!({ "type": "text", "text": other.to_string() })],
    }
}

/// Non-stream Messages response → Responses-shaped value.
pub fn parse_message(v: &Value) -> Value {
    let id = v.get("id").and_then(|x| x.as_str());
    let model = v.get("model").and_then(|x| x.as_str());
    let mut text = String::new();
    let mut tool_calls: Vec<Value> = Vec::new();
    if let Some(Value::Array(blocks)) = v.get("content") {
        for b in blocks {
            match b.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                "text" => {
                    if let Some(t) = b.get("text").and_then(|t| t.as_str()) {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(t);
                    }
                }
                "tool_use" => {
                    let id = b.get("id").and_then(|x| x.as_str()).unwrap_or("");
                    let name = b.get("name").and_then(|x| x.as_str()).unwrap_or("");
                    let input = b.get("input").cloned().unwrap_or(json!({}));
                    let args = serde_json::to_string(&input).unwrap_or_else(|_| "{}".into());
                    tool_calls.push(json!({
                        "id": id,
                        "type": "function",
                        "function": { "name": name, "arguments": args }
                    }));
                }
                _ => {}
            }
        }
    }
    let usage = v.get("usage").map(|u| {
        json!({
            "prompt_tokens": u.get("input_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
            "completion_tokens": u.get("output_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
            "total_tokens": u.get("input_tokens").and_then(|x| x.as_u64()).unwrap_or(0)
                + u.get("output_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
        })
    });
    build_response_value(id, model, &text, &tool_calls, usage.as_ref())
}

/// Streaming accumulator for Anthropic SSE events.
#[derive(Default)]
pub struct StreamAccumulator {
    pub id: Option<String>,
    pub model: Option<String>,
    pub content: String,
    /// Active tool_use blocks: (id, name, json fragments)
    calls: Vec<(String, String, String)>,
    /// index of current content block if tool_use
    current_tool_idx: Option<usize>,
    usage: Option<Value>,
}

impl StreamAccumulator {
    /// Feed one SSE `data:` JSON object. Returns text delta if any.
    pub fn push(&mut self, v: &Value) -> Option<String> {
        let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match ty {
            "message_start" => {
                if let Some(msg) = v.get("message") {
                    if self.id.is_none() {
                        self.id = msg.get("id").and_then(|x| x.as_str()).map(String::from);
                    }
                    if self.model.is_none() {
                        self.model = msg.get("model").and_then(|x| x.as_str()).map(String::from);
                    }
                    if let Some(u) = msg.get("usage") {
                        self.usage = Some(chat_usage_from_anthropic(u));
                    }
                }
                None
            }
            "content_block_start" => {
                let block = v.get("content_block")?;
                if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                    let id = block
                        .get("id")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = block
                        .get("name")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string();
                    self.calls.push((id, name, String::new()));
                    self.current_tool_idx = Some(self.calls.len() - 1);
                } else {
                    self.current_tool_idx = None;
                }
                None
            }
            "content_block_delta" => {
                let delta = v.get("delta")?;
                match delta.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                    "text_delta" => {
                        let t = delta.get("text").and_then(|x| x.as_str())?;
                        if t.is_empty() {
                            return None;
                        }
                        self.content.push_str(t);
                        Some(t.to_string())
                    }
                    "input_json_delta" => {
                        if let Some(idx) = self.current_tool_idx {
                            if let Some(partial) =
                                delta.get("partial_json").and_then(|x| x.as_str())
                            {
                                if let Some(slot) = self.calls.get_mut(idx) {
                                    slot.2.push_str(partial);
                                }
                            }
                        }
                        None
                    }
                    _ => None,
                }
            }
            "content_block_stop" => {
                self.current_tool_idx = None;
                None
            }
            "message_delta" => {
                if let Some(u) = v.get("usage") {
                    // message_delta usage often only has output_tokens
                    let mut base = self
                        .usage
                        .clone()
                        .unwrap_or_else(|| json!({"prompt_tokens":0,"completion_tokens":0,"total_tokens":0}));
                    if let Some(o) = u.get("output_tokens").and_then(|x| x.as_u64()) {
                        base["completion_tokens"] = json!(o);
                    }
                    if let Some(i) = u.get("input_tokens").and_then(|x| x.as_u64()) {
                        base["prompt_tokens"] = json!(i);
                    }
                    let p = base["prompt_tokens"].as_u64().unwrap_or(0);
                    let c = base["completion_tokens"].as_u64().unwrap_or(0);
                    base["total_tokens"] = json!(p + c);
                    self.usage = Some(base);
                }
                None
            }
            "message_stop" | "ping" | "error" => None,
            _ => None,
        }
    }

    pub fn finish(&self) -> Value {
        let tool_calls: Vec<Value> = self
            .calls
            .iter()
            .filter(|(_, name, _)| !name.is_empty())
            .map(|(id, name, args)| {
                let args = if args.is_empty() { "{}" } else { args.as_str() };
                json!({
                    "id": if id.is_empty() { format!("call_{name}") } else { id.clone() },
                    "type": "function",
                    "function": { "name": name, "arguments": args }
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

fn chat_usage_from_anthropic(u: &Value) -> Value {
    let p = u.get("input_tokens").and_then(|x| x.as_u64()).unwrap_or(0);
    let c = u.get("output_tokens").and_then(|x| x.as_u64()).unwrap_or(0);
    json!({
        "prompt_tokens": p,
        "completion_tokens": c,
        "total_tokens": p + c,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::types::ToolDef;

    fn req() -> ResponseRequest {
        ResponseRequest {
            model: "claude-sonnet-4-20250514".into(),
            input: json!([
                {"role":"user","content":[{"type":"input_text","text":"hi"}]},
                {"type":"function_call","call_id":"c1","name":"grep","arguments":"{\"pattern\":\"x\"}"},
                {"type":"function_call_output","call_id":"c1","output":"match"},
            ]),
            instructions: Some("be terse".into()),
            tools: Some(vec![ToolDef {
                type_: "function".into(),
                name: "grep".into(),
                description: Some("search".into()),
                parameters: Some(json!({"type":"object","properties":{}})),
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
    fn oauth_token_detection() {
        assert!(is_oauth_token("sk-ant-oat01-abcdef"));
        assert!(is_oauth_token("  sk-ant-oat-xyz  "));
        assert!(!is_oauth_token("sk-ant-api03-abcdef"));
        assert!(!is_oauth_token("xai-jwt-token"));
    }

    #[test]
    fn body_is_messages_shape_not_chat_completions() {
        let b = build_body(&req(), false);
        assert_eq!(b["model"], "claude-sonnet-4-20250514");
        assert!(b.get("max_tokens").is_some());
        assert!(b.get("system").is_some());
        assert!(b.get("messages").is_some());
        // Must NOT be OpenAI chat shape.
        assert!(b.get("stream_options").is_none());
        let tools = b["tools"].as_array().unwrap();
        assert_eq!(tools[0]["name"], "grep");
        assert!(tools[0].get("input_schema").is_some());
        assert!(tools[0].get("function").is_none());
    }

    #[test]
    fn parse_message_maps_text_and_tool_use() {
        let v = json!({
            "id": "msg_1",
            "model": "claude-x",
            "content": [
                {"type":"text","text":"hello"},
                {"type":"tool_use","id":"tu1","name":"bash","input":{"command":"ls"}}
            ],
            "usage": {"input_tokens": 10, "output_tokens": 5}
        });
        let shaped = parse_message(&v);
        assert_eq!(shaped["id"], "msg_1");
        let out = shaped["output"].as_array().unwrap();
        assert!(out.iter().any(|o| o["type"] == "message"));
        assert!(out.iter().any(|o| o["type"] == "function_call" && o["name"] == "bash"));
        assert_eq!(shaped["usage"]["input_tokens"], 10);
    }

    #[test]
    fn stream_accumulates_text_and_tools() {
        let mut acc = StreamAccumulator::default();
        acc.push(&json!({
            "type":"message_start",
            "message":{"id":"m1","model":"c","usage":{"input_tokens":3,"output_tokens":0}}
        }));
        assert_eq!(
            acc.push(&json!({
                "type":"content_block_delta",
                "delta":{"type":"text_delta","text":"Hi"}
            })),
            Some("Hi".into())
        );
        acc.push(&json!({
            "type":"content_block_start",
            "content_block":{"type":"tool_use","id":"t1","name":"grep","input":{}}
        }));
        acc.push(&json!({
            "type":"content_block_delta",
            "delta":{"type":"input_json_delta","partial_json":"{\"p\":"}
        }));
        acc.push(&json!({
            "type":"content_block_delta",
            "delta":{"type":"input_json_delta","partial_json":"1}"}
        }));
        let finished = acc.finish();
        assert!(finished["output"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o["type"] == "function_call" && o["arguments"] == "{\"p\":1}"));
    }
}

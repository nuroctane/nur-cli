//! Anthropic **Messages** API adapter (`POST /v1/messages`).
//!
//! nur-cli's agent loop speaks the OpenAI/Meta Responses shape. Anthropic does
//! **not** implement `/chat/completions` on `api.anthropic.com` — that was the
//! root cause of "Anthropic isn't working" for both API keys and Claude OAuth.
//! This module translates Responses ↔ Messages (including tools + streaming).

use super::chat::build_response_value;
use super::types::ResponseRequest;
use serde_json::{json, Value};
use std::collections::HashMap;

/// True for Claude Code / claude.ai OAuth access tokens (`sk-ant-oat…`).
pub fn is_oauth_token(key: &str) -> bool {
    let k = key.trim();
    k.starts_with("sk-ant-oat") || k.starts_with("sk-ant-oat01-")
}

/// Anthropic beta header required for OAuth bearer tokens against the API.
#[allow(dead_code)]
pub const OAUTH_BETA: &str = "oauth-2025-04-20";
/// Claude Code product beta — required with subscription OAuth (`sk-ant-oat…`)
/// so Messages requests are accepted as a first-party Claude Code client.
#[allow(dead_code)]
pub const CLAUDE_CODE_BETA: &str = "claude-code-20250219";
/// Combined beta list for Claude OAuth / Claude Code sessions.
/// Mirrors Claude Code: oauth + product beta (+ oidc federation used by Code ≥2.1).
pub const OAUTH_BETAS: &str = "oauth-2025-04-20,claude-code-20250219,oidc-federation-2026-04-01";

/// Required system identity for Claude Code OAuth (`sk-ant-oat…`) rate-limit pool.
///
/// Anthropic routes subscription OAuth tokens to the Claude Code quota pool only
/// when the Messages `system` prompt identifies as Claude Code. Without this,
/// Sonnet often returns opaque HTTP 429 `rate_limit_error` / `"Error"`.
/// Must be a **separate** system content block (not concatenated into one string).
pub const CLAUDE_CODE_SYSTEM_IDENTITY: &str =
    "You are Claude Code, Anthropic's official CLI for Claude.";

/// Default Sonnet on the Claude API (platform.claude.com, mid-2026).
/// **Not** `claude-sonnet-4-20250514` — Sonnet 4 is retired on the first-party
/// Claude API (still on some Bedrock/GCP endpoints only).
pub const DEFAULT_SONNET: &str = "claude-sonnet-5";

/// Map retired / short / product names → a Claude API id that still works.
///
/// Source of truth: https://platform.claude.com/docs/en/about-claude/models/overview
/// (Claude Sonnet 5 = `claude-sonnet-5`, Opus 4.8 = `claude-opus-4-8`, …).
/// Applies to **both** API keys and Claude OAuth — same Messages model ids.
pub fn normalize_model_id(model: &str) -> String {
    let m = model.trim();
    if m.is_empty() {
        return DEFAULT_SONNET.to_string();
    }
    // Exact retired Sonnet 4 snapshots → current Sonnet 5 (Claude API only).
    match m {
        "claude-sonnet-4"
        | "claude-sonnet-4-20250514"
        | "claude-4-sonnet"
        | "sonnet-4"
        | "sonnet4" => return DEFAULT_SONNET.to_string(),
        // Product-ish shorts
        "sonnet" | "sonnet-5" | "claude-sonnet" => return DEFAULT_SONNET.to_string(),
        "opus" | "opus-4.8" | "opus-4-8" | "claude-opus" => return "claude-opus-4-8".to_string(),
        "haiku" | "haiku-4.5" | "haiku-4-5" => return "claude-haiku-4-5".to_string(),
        // Incomplete aliases that 404 without the dated suffix / current id
        "claude-opus-4" | "claude-opus-4-20250514" => {
            // Opus 4 GA snapshot is also aging out; prefer current Opus 4.8.
            return "claude-opus-4-8".to_string();
        }
        "claude-opus-4-1" => return "claude-opus-4-1-20250805".to_string(),
        "claude-sonnet-4-5" => return "claude-sonnet-4-5-20250929".to_string(),
        "claude-opus-4-5" => return "claude-opus-4-5-20251101".to_string(),
        "claude-haiku-4-5" => return "claude-haiku-4-5".to_string(), // official alias
        _ => {}
    }
    m.to_string()
}

/// Build a Messages API body from a Responses request.
///
/// When `oauth` is true (Claude Code / `sk-ant-oat…` session), the system prompt
/// is emitted as **content blocks** with [`CLAUDE_CODE_SYSTEM_IDENTITY`] first so
/// Anthropic assigns the subscription rate-limit pool.
#[allow(dead_code)] // non-oauth convenience; production uses build_body_with_oauth
pub fn build_body(req: &ResponseRequest, stream: bool) -> Value {
    build_body_with_oauth(req, stream, false)
}

/// Like [`build_body`], with an explicit OAuth/Claude Code session flag.
pub fn build_body_with_oauth(req: &ResponseRequest, stream: bool, oauth: bool) -> Value {
    let mut system: Option<String> = req.instructions.clone().filter(|s| !s.is_empty());
    let mut messages: Vec<Value> = Vec::new();

    if let Value::Array(items) = &req.input {
        for item in items {
            push_item(item, &mut messages, &mut system);
        }
    }
    // Anthropic requires alternating user/assistant; merge consecutive same roles.
    let messages = coalesce_roles(messages);
    // …and then that every tool_use is answered in the very next message.
    let messages = normalize_tool_pairs(messages);

    let model = normalize_model_id(&req.model);
    let mut body = json!({
        "model": model,
        "max_tokens": 16_384,
        "messages": messages,
    });
    if oauth {
        // Separate blocks — a single concatenated string still 429s on Sonnet.
        let mut blocks = vec![json!({
            "type": "text",
            "text": CLAUDE_CODE_SYSTEM_IDENTITY,
        })];
        if let Some(sys) = system.filter(|s| !s.is_empty()) {
            // Avoid duplicating identity if caller already included it.
            if !sys.contains("You are Claude Code, Anthropic's official CLI") {
                blocks.push(json!({ "type": "text", "text": sys }));
            } else {
                blocks = vec![json!({ "type": "text", "text": sys })];
            }
        }
        body["system"] = json!(blocks);
    } else if let Some(sys) = system {
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
    apply_prompt_caching(&mut body);
    body
}

/// Attach Anthropic prompt-cache breakpoints (`cache_control: ephemeral`) to the
/// large, stable parts of the request: the system prompt, the tool schemas, and
/// the most recent message(s).
///
/// Anthropic prompt caching is **opt-in** on the Messages API — without an
/// explicit breakpoint the full prefix (system + tools + entire history) is
/// re-read at full price on every turn. Every other provider nur speaks to does
/// automatic server-side prefix caching, which is why only Claude showed runaway
/// token cost. This restores parity with Claude Code, which sets the same
/// breakpoints. Max 4 breakpoints per request: system (1) + tools (1) + a rolling
/// breakpoint on the last 2 messages (≤2).
fn apply_prompt_caching(body: &mut Value) {
    fn mark(block: &mut Value) {
        if let Some(obj) = block.as_object_mut() {
            obj.insert("cache_control".into(), json!({ "type": "ephemeral" }));
        }
    }

    // 1) System prompt — normalise a plain string to a single text block so a
    //    breakpoint can be attached; otherwise mark the last existing block.
    match body.get_mut("system") {
        Some(s @ Value::String(_)) => {
            let text = s.as_str().unwrap_or("").to_string();
            *s = json!([{
                "type": "text",
                "text": text,
                "cache_control": { "type": "ephemeral" },
            }]);
        }
        Some(Value::Array(blocks)) => {
            if let Some(last) = blocks.last_mut() {
                mark(last);
            }
        }
        _ => {}
    }

    // 2) Tool schemas — one breakpoint on the last tool caches the whole block.
    if let Some(Value::Array(tools)) = body.get_mut("tools") {
        if let Some(last) = tools.last_mut() {
            mark(last);
        }
    }

    // 3) Recent history — a rolling breakpoint on the last up-to-2 messages so
    //    growing conversation context is reused across turns.
    if let Some(Value::Array(messages)) = body.get_mut("messages") {
        let start = messages.len().saturating_sub(2);
        for msg in messages[start..].iter_mut() {
            match msg.get_mut("content") {
                Some(c @ Value::String(_)) => {
                    let text = c.as_str().unwrap_or("").to_string();
                    *c = json!([{
                        "type": "text",
                        "text": text,
                        "cache_control": { "type": "ephemeral" },
                    }]);
                }
                Some(Value::Array(blocks)) => {
                    if let Some(last) = blocks.last_mut() {
                        mark(last);
                    }
                }
                _ => {}
            }
        }
    }
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
        let args_str = item
            .get("arguments")
            .and_then(|v| v.as_str())
            .unwrap_or("{}");
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
    let role = if role == "assistant" {
        "assistant"
    } else {
        "user"
    };
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
        out.insert(0, json!({ "role": "user", "content": "(continue)" }));
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

/// Stand-in body for a `tool_use` that never produced a `function_call_output`.
const MISSING_TOOL_RESULT: &str = "[no result: tool call was interrupted]";
/// Stand-in body for a tool that succeeded but returned nothing — Anthropic
/// rejects a `tool_result` whose content is an empty string.
const EMPTY_TOOL_RESULT: &str = "(no output)";

fn block_type(b: &Value) -> &str {
    b.get("type").and_then(|t| t.as_str()).unwrap_or("")
}

/// Anthropic rejects empty text blocks. They appear whenever a turn produced
/// only a tool call (nur still records a `message` item with empty text) or
/// only reasoning, so strip them before the pairing pass counts content.
fn prune_empty_blocks(blocks: Vec<Value>) -> Vec<Value> {
    blocks
        .into_iter()
        .filter(|b| {
            block_type(b) != "text"
                || !b
                    .get("text")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .is_empty()
        })
        .collect()
}

/// Ordered `tool_use` ids of an assistant message (empty for every other role —
/// Anthropic only accepts `tool_use` from the assistant).
fn tool_use_ids(msg: &Value) -> Vec<String> {
    if msg.get("role").and_then(|r| r.as_str()) != Some("assistant") {
        return Vec::new();
    }
    let Some(Value::Array(blocks)) = msg.get("content") else {
        return Vec::new();
    };
    blocks
        .iter()
        .filter(|b| block_type(b) == "tool_use")
        .filter_map(|b| b.get("id").and_then(|v| v.as_str()))
        .filter(|id| !id.is_empty())
        .map(String::from)
        .collect()
}

/// Rebuild `tool_use` ↔ `tool_result` pairing so the transcript is *always*
/// valid for the Messages API.
///
/// Anthropic is far stricter here than the Responses shape nur's agent loop
/// records: **every** `tool_use` block in an assistant message must be answered
/// by a `tool_result` carrying the same id in the *immediately following* user
/// message, and a `tool_result` may never appear without its call. The recorded
/// Responses transcript can break both rules — an interrupted turn (Esc during
/// a tool, a cancelled `agent` fan-out, an error raised between recording the
/// `function_call` and recording its `function_call_output`) persists a call
/// with no output, and history carried over from another provider/route or
/// across a compaction can hold a result whose call is gone. Either one is a
/// hard `400 … tool_use ids were found without tool_result blocks immediately
/// after: <id>`, which is why the pairing is rebuilt here instead of trusted.
///
/// The pass:
/// 1. lifts every `tool_result` out of wherever it landed, keyed by call id;
/// 2. re-emits them, in call order, in a single user message placed directly
///    after their assistant turn — this also fixes out-of-order parallel
///    results and assistant text interleaved between a call and its result;
/// 3. synthesizes an `is_error` placeholder for a `tool_use` with no result
///    anywhere. We synthesize rather than drop the `tool_use`, because dropping
///    it would also strand any assistant text sharing that message and would
///    silently rewrite what the model actually said;
/// 4. drops a `tool_result` whose id is never called — Anthropic 400s on those
///    too and there is nothing left to attach it to;
/// 5. drops messages left with empty content (Anthropic rejects both `""` and
///    `[]`) and re-runs [`coalesce_roles`], which restores strict alternation
///    and the "first message must be user" rule after the shuffle.
fn normalize_tool_pairs(msgs: Vec<Value>) -> Vec<Value> {
    let had_messages = !msgs.is_empty();

    // 1) Lift every tool_result out, keyed by call id. Content is normalised to
    //    block arrays throughout so later steps have one shape to reason about.
    let mut results: HashMap<String, Value> = HashMap::new();
    let mut stripped: Vec<Value> = Vec::new();
    for msg in msgs {
        let role = msg
            .get("role")
            .and_then(|r| r.as_str())
            .unwrap_or("user")
            .to_string();
        let mut kept: Vec<Value> = Vec::new();
        for b in normalize_blocks(msg.get("content")) {
            if block_type(&b) != "tool_result" {
                kept.push(b);
                continue;
            }
            let id = match b.get("tool_use_id").and_then(|v| v.as_str()) {
                Some(s) if !s.is_empty() => s.to_string(),
                // A result with no id can never be paired — drop it.
                _ => continue,
            };
            results.entry(id).or_insert_with(|| fill_empty_result(b));
        }
        let kept = prune_empty_blocks(kept);
        // A message that existed only to carry results (the common case) leaves
        // no shell behind — its results are re-emitted in step 2.
        if kept.is_empty() {
            continue;
        }
        stripped.push(json!({ "role": role, "content": kept }));
    }

    // 2) Re-emit each assistant turn's results immediately after it.
    let mut out: Vec<Value> = Vec::new();
    for msg in stripped {
        let ids = tool_use_ids(&msg);
        out.push(msg);
        if ids.is_empty() {
            continue;
        }
        let blocks: Vec<Value> = ids
            .iter()
            .map(|id| {
                results.remove(id).unwrap_or_else(|| {
                    json!({
                        "type": "tool_result",
                        "tool_use_id": id,
                        "content": MISSING_TOOL_RESULT,
                        "is_error": true,
                    })
                })
            })
            .collect();
        out.push(json!({ "role": "user", "content": blocks }));
    }
    // Whatever is left in `results` is an orphan result — intentionally dropped.

    if out.is_empty() && had_messages {
        // Everything was empty shells; still send something Anthropic accepts.
        out.push(json!({ "role": "user", "content": [{ "type": "text", "text": "(continue)" }] }));
    }
    coalesce_roles(out)
}

/// Anthropic rejects a `tool_result` with empty content, which is exactly what a
/// tool that succeeded silently (a write, a no-match grep) produces.
fn fill_empty_result(mut block: Value) -> Value {
    let empty = match block.get("content") {
        None => true,
        Some(Value::String(s)) => s.is_empty(),
        Some(Value::Array(a)) => a.is_empty(),
        _ => false,
    };
    if empty {
        block["content"] = json!(EMPTY_TOOL_RESULT);
    }
    block
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
                    let mut base = self.usage.clone().unwrap_or_else(
                        || json!({"prompt_tokens":0,"completion_tokens":0,"total_tokens":0}),
                    );
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

    /// Same fixture as [`req`] but with a caller-supplied Responses transcript.
    fn req_with(input: Value) -> ResponseRequest {
        ResponseRequest { input, ..req() }
    }

    fn messages_of(input: Value) -> Vec<Value> {
        build_body_with_oauth(&req_with(input), false, false)["messages"]
            .as_array()
            .expect("messages present")
            .clone()
    }

    fn blocks(msg: &Value) -> Vec<Value> {
        match &msg["content"] {
            Value::Array(b) => b.clone(),
            Value::String(s) => vec![json!({ "type": "text", "text": s })],
            _ => Vec::new(),
        }
    }

    /// The exact contract Anthropic enforces (and 400s on): no empty content,
    /// alternating roles, every `tool_use` answered by a same-id `tool_result`
    /// in the very next message, and no `tool_result` without a call.
    fn assert_valid_pairing(msgs: &[Value]) {
        let mut called: Vec<String> = Vec::new();
        for (i, m) in msgs.iter().enumerate() {
            let role = m["role"].as_str().unwrap_or("");
            let bs = blocks(m);
            assert!(!bs.is_empty(), "message {i} has empty content: {m}");
            if i > 0 {
                assert_ne!(
                    role,
                    msgs[i - 1]["role"].as_str().unwrap_or(""),
                    "messages {} and {i} share a role",
                    i - 1
                );
            }
            for b in &bs {
                if block_type(b) == "tool_result" {
                    let id = b["tool_use_id"].as_str().unwrap_or("").to_string();
                    assert!(called.contains(&id), "orphan tool_result {id} at msg {i}");
                }
            }
            let ids = tool_use_ids(m);
            if ids.is_empty() {
                continue;
            }
            called.extend(ids.clone());
            let next = msgs.get(i + 1).unwrap_or_else(|| {
                panic!("tool_use at msg {i} is the last message — nothing can answer it")
            });
            assert_eq!(next["role"], "user", "results must come back as user");
            let answered: Vec<String> = blocks(next)
                .iter()
                .filter(|b| block_type(b) == "tool_result")
                .map(|b| b["tool_use_id"].as_str().unwrap_or("").to_string())
                .collect();
            assert!(
                answered.len() >= ids.len(),
                "msg {i} has {} tool_use but only {} results follow",
                ids.len(),
                answered.len()
            );
            assert_eq!(
                answered[..ids.len()],
                ids[..],
                "msg {i} results must answer every id, in call order"
            );
        }
        assert_eq!(
            msgs.first().map(|m| m["role"].clone()),
            Some(json!("user")),
            "first message must be user"
        );
    }

    #[test]
    fn orphan_tool_use_gets_a_synthesized_result() {
        // Interrupted turn: the call was recorded, the output never was, and the
        // next thing in the transcript is the user's follow-up prompt. This is
        // the shape that produced `400 … without tool_result blocks … read_file_5`.
        let msgs = messages_of(json!([
            {"role":"user","content":[{"type":"input_text","text":"read it"}]},
            {"type":"function_call","call_id":"read_file_5","name":"read_file","arguments":"{}"},
            {"role":"user","content":[{"type":"input_text","text":"actually, never mind"}]},
        ]));
        assert_valid_pairing(&msgs);
        let answer = &msgs[2];
        assert_eq!(answer["role"], "user");
        let bs = blocks(answer);
        assert_eq!(bs[0]["type"], "tool_result");
        assert_eq!(bs[0]["tool_use_id"], "read_file_5");
        assert_eq!(bs[0]["is_error"], true);
        assert_eq!(bs[0]["content"], MISSING_TOOL_RESULT);
        // The tool_use itself survives, and so does the follow-up text.
        assert_eq!(tool_use_ids(&msgs[1]), vec!["read_file_5".to_string()]);
        assert!(bs
            .iter()
            .any(|b| b["text"].as_str() == Some("actually, never mind")));
    }

    #[test]
    fn parallel_tool_calls_keep_both_results_in_call_order() {
        let msgs = messages_of(json!([
            {"role":"user","content":[{"type":"input_text","text":"go"}]},
            {"type":"function_call","call_id":"a","name":"git_status","arguments":"{}"},
            {"type":"function_call","call_id":"b","name":"list_dir","arguments":"{}"},
            // Results can land out of order when the batch runs concurrently.
            {"type":"function_call_output","call_id":"b","output":"src"},
            {"type":"function_call_output","call_id":"a","output":"clean"},
        ]));
        assert_valid_pairing(&msgs);
        assert_eq!(msgs.len(), 3);
        assert_eq!(tool_use_ids(&msgs[1]), vec!["a".to_string(), "b".into()]);
        let bs = blocks(&msgs[2]);
        assert_eq!(bs.len(), 2);
        assert_eq!(bs[0]["tool_use_id"], "a");
        assert_eq!(bs[0]["content"], "clean");
        assert_eq!(bs[1]["tool_use_id"], "b");
        assert_eq!(bs[1]["content"], "src");
    }

    #[test]
    fn assistant_text_between_call_and_result_does_not_split_the_pair() {
        let msgs = messages_of(json!([
            {"role":"user","content":[{"type":"input_text","text":"go"}]},
            {"type":"function_call","call_id":"c1","name":"grep","arguments":"{}"},
            {"role":"assistant","content":[{"type":"output_text","text":"searching…"}]},
            {"type":"function_call_output","call_id":"c1","output":"match"},
        ]));
        assert_valid_pairing(&msgs);
        // Assistant turn keeps its narration *and* its call; the result follows.
        let bs = blocks(&msgs[1]);
        assert!(bs.iter().any(|b| block_type(b) == "tool_use"));
        assert!(bs.iter().any(|b| b["text"].as_str() == Some("searching…")));
        assert_eq!(blocks(&msgs[2])[0]["content"], "match");
    }

    #[test]
    fn orphan_tool_result_is_dropped() {
        // Compaction / a provider switch can leave a result whose call is gone.
        let msgs = messages_of(json!([
            {"role":"user","content":[{"type":"input_text","text":"go"}]},
            {"type":"function_call_output","call_id":"ghost","output":"stale"},
            {"role":"assistant","content":[{"type":"output_text","text":"ok"}]},
        ]));
        assert_valid_pairing(&msgs);
        let json = serde_json::to_string(&msgs).unwrap();
        assert!(!json.contains("ghost"), "orphan result must not be sent");
        assert!(!json.contains("stale"));
    }

    #[test]
    fn empty_content_never_reaches_the_wire() {
        // An assistant turn that was pure tool call records an empty message
        // item; Anthropic rejects both `""` and `[]` content.
        let msgs = messages_of(json!([
            {"role":"user","content":[{"type":"input_text","text":"go"}]},
            {"role":"assistant","content":[{"type":"output_text","text":""}]},
            {"type":"function_call","call_id":"c1","name":"write","arguments":"{}"},
            {"type":"function_call_output","call_id":"c1","output":""},
        ]));
        assert_valid_pairing(&msgs);
        // Empty tool output is substituted, not sent as "".
        assert_eq!(blocks(&msgs[2])[0]["content"], EMPTY_TOOL_RESULT);
    }

    #[test]
    fn caching_breakpoint_on_a_tool_result_message_stays_valid() {
        // The rolling breakpoint lands on the last 2 messages, which after
        // normalization is often assistant[tool_use] + user[tool_result].
        let b = build_body_with_oauth(
            &req_with(json!([
                {"role":"user","content":[{"type":"input_text","text":"go"}]},
                {"type":"function_call","call_id":"c1","name":"grep","arguments":"{}"},
                {"type":"function_call_output","call_id":"c1","output":"match"},
            ])),
            false,
            false,
        );
        let msgs = b["messages"].as_array().unwrap();
        let last = blocks(msgs.last().unwrap());
        assert_eq!(last[0]["type"], "tool_result");
        assert_eq!(last[0]["cache_control"]["type"], "ephemeral");
        // cache_control must decorate the block, never replace its payload.
        assert_eq!(last[0]["tool_use_id"], "c1");
        assert_eq!(last[0]["content"], "match");
    }

    #[test]
    fn oauth_token_detection() {
        assert!(is_oauth_token("sk-ant-oat01-abcdef"));
        assert!(is_oauth_token("  sk-ant-oat-xyz  "));
        assert!(!is_oauth_token("sk-ant-api03-abcdef"));
        assert!(!is_oauth_token("xai-jwt-token"));
    }

    #[test]
    fn oauth_system_is_content_blocks_with_claude_code_identity() {
        let body = build_body_with_oauth(&req(), false, true);
        let system = body.get("system").expect("system present");
        let arr = system.as_array().expect("oauth system must be blocks");
        assert!(arr.len() >= 2, "identity + nur instructions");
        assert_eq!(
            arr[0].get("text").and_then(|t| t.as_str()),
            Some(CLAUDE_CODE_SYSTEM_IDENTITY)
        );
        assert!(
            arr[1]
                .get("text")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .contains("be terse"),
            "user instructions kept as separate block"
        );
        // Non-oauth system is normalised to a cacheable text block array.
        let plain = build_body_with_oauth(&req(), false, false);
        let sys = plain["system"].as_array().expect("system is block array");
        assert_eq!(sys[0]["text"], "be terse");
        assert_eq!(sys.last().unwrap()["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn prompt_caching_breakpoints_are_attached() {
        // System, tools, and the most recent message must carry a cache_control
        // breakpoint so Anthropic caches the prefix instead of re-billing it.
        let b = build_body_with_oauth(&req(), false, false);

        let sys = b["system"].as_array().expect("system blocks");
        assert_eq!(sys.last().unwrap()["cache_control"]["type"], "ephemeral");

        let tools = b["tools"].as_array().expect("tools present");
        assert_eq!(tools.last().unwrap()["cache_control"]["type"], "ephemeral");

        // At least one of the trailing messages is breakpointed.
        let msgs = b["messages"].as_array().expect("messages present");
        let last = msgs.last().unwrap();
        let has_bp = match &last["content"] {
            Value::Array(blocks) => blocks
                .last()
                .and_then(|blk| blk.get("cache_control"))
                .is_some(),
            _ => false,
        };
        assert!(has_bp, "last message must carry a cache breakpoint");

        // Never exceed Anthropic's 4-breakpoint limit.
        let mut count = 0;
        let mut walk = |v: &Value| {
            if let Value::Object(o) = v {
                if o.contains_key("cache_control") {
                    count += 1;
                }
            }
        };
        for blk in b["system"].as_array().unwrap() {
            walk(blk);
        }
        for t in b["tools"].as_array().unwrap() {
            walk(t);
        }
        for m in b["messages"].as_array().unwrap() {
            if let Value::Array(blocks) = &m["content"] {
                for blk in blocks {
                    walk(blk);
                }
            }
        }
        assert!(count <= 4, "at most 4 cache breakpoints, got {count}");
    }

    #[test]
    fn normalize_rewrites_retired_sonnet4() {
        assert_eq!(
            normalize_model_id("claude-sonnet-4-20250514"),
            DEFAULT_SONNET
        );
        assert_eq!(normalize_model_id("claude-sonnet-4"), DEFAULT_SONNET);
        assert_eq!(normalize_model_id("claude-sonnet-5"), "claude-sonnet-5");
        assert_eq!(normalize_model_id("claude-opus-4-8"), "claude-opus-4-8");
    }

    #[test]
    fn body_is_messages_shape_not_chat_completions() {
        let b = build_body(&req(), false);
        // Retired Sonnet 4 id must not be sent to the first-party Claude API.
        assert_eq!(b["model"], DEFAULT_SONNET);
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
        assert!(out
            .iter()
            .any(|o| o["type"] == "function_call" && o["name"] == "bash"));
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

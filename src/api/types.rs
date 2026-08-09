use crate::usage::TokenUsage;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseRequest {
    pub model: String,
    pub input: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDef>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    /// Stable key so Meta can reuse cached prompt prefixes (system instructions)
    /// across turns in the same session — surfaces as `cached_tokens` in usage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
    /// Conservative per-request completion reservation. Backends map this to
    /// their native output-limit field when they support one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    #[serde(rename = "type")]
    pub type_: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse {
    pub id: Option<String>,
    pub status: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub output: Vec<OutputItem>,
    pub usage: Option<ApiUsage>,
    pub error: Option<ApiErrorBody>,
    /// Local transport/accounting data. It is deliberately not sent over the
    /// wire and remains optional so historic response JSON still deserializes.
    #[serde(default, skip)]
    pub accounting: Option<ResponseAccounting>,
}

/// Accounting facts which are not part of any one provider response schema.
/// `estimated_usage` is retained even for an observed response so callers can
/// reason about an attempt that ended before provider telemetry arrived.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResponseAccounting {
    #[serde(default)]
    pub estimated_usage: TokenUsage,
    #[serde(default)]
    pub native_cost_usd: Option<f64>,
    #[serde(default)]
    pub upstream_provider: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiErrorBody {
    pub message: Option<String>,
    pub code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub output_tokens_details: Option<OutputTokensDetails>,
    #[serde(default)]
    pub input_tokens_details: Option<InputTokensDetails>,
    /// Providers such as Anthropic and OpenRouter can expose cache-write
    /// tokens separately. It must not be folded into cached reads.
    #[serde(default)]
    pub cache_write_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputTokensDetails {
    #[serde(default)]
    pub reasoning_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputTokensDetails {
    #[serde(default)]
    pub cached_tokens: u64,
}

impl From<&ApiUsage> for TokenUsage {
    fn from(u: &ApiUsage) -> Self {
        TokenUsage {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
            total_tokens: if u.total_tokens > 0 {
                u.total_tokens
            } else {
                u.input_tokens + u.output_tokens
            },
            reasoning_tokens: u
                .output_tokens_details
                .as_ref()
                .map(|d| d.reasoning_tokens)
                .unwrap_or(0),
            cached_tokens: u
                .input_tokens_details
                .as_ref()
                .map(|d| d.cached_tokens)
                .unwrap_or(0),
            cache_write_tokens: u.cache_write_tokens,
            cost_usd: 0.0,
            cost_known: false,
            usage_state: crate::usage::UsageState::Observed,
            cost_provenance: crate::usage::CostProvenance::Unknown,
            upstream_provider: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum OutputItem {
    #[serde(rename = "message")]
    Message {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        role: Option<String>,
        #[serde(default)]
        status: Option<String>,
        #[serde(default)]
        content: Vec<ContentPart>,
        #[serde(default)]
        phase: Option<String>,
        /// Kimi/Moonshot requires this hidden assistant field to be replayed
        /// alongside tool calls on the next Chat Completions request.
        #[serde(default)]
        reasoning_content: Option<String>,
    },
    #[serde(rename = "reasoning")]
    Reasoning {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        summary: Vec<Value>,
        #[serde(default)]
        encrypted_content: Option<String>,
    },
    #[serde(rename = "function_call")]
    FunctionCall {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        call_id: Option<String>,
        #[serde(default)]
        name: Option<String>,
        /// Codex/OpenAI usually send a JSON **string**; some gateways send an object.
        #[serde(default, deserialize_with = "deserialize_args_string")]
        arguments: Option<String>,
        #[serde(default)]
        status: Option<String>,
    },
    /// Alternate name some Responses/Codex builds use for the same shape.
    #[serde(rename = "custom_tool_call")]
    CustomToolCall {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        call_id: Option<String>,
        #[serde(default)]
        name: Option<String>,
        #[serde(default, deserialize_with = "deserialize_args_string")]
        arguments: Option<String>,
        #[serde(default)]
        input: Option<String>,
        #[serde(default)]
        status: Option<String>,
    },
    #[serde(other)]
    Other,
}

/// Accept `arguments` as a JSON string or as a raw object/array (stringify it).
fn deserialize_args_string<'de, D>(deserializer: D) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = Option::<Value>::deserialize(deserializer)?;
    Ok(match v {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s),
        Some(other) => Some(other.to_string()),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentPart {
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(default)]
    pub text: Option<String>,
}

impl ApiResponse {
    /// Attach a conservative local estimate for a completed response. This is
    /// not a tokenizer and never claims invoice accuracy; it prevents a route
    /// that omits terminal usage from disappearing from token/budget telemetry.
    pub fn with_local_usage_estimate(mut self, req: &ResponseRequest) -> Self {
        let input_tokens = estimate_request_input_tokens(req);
        let output_tokens = estimate_output_tokens(&self.output);
        let prior = self.accounting.take().unwrap_or_default();
        let mut estimated_usage = TokenUsage::estimated(input_tokens, output_tokens);
        if prior.estimated_usage.cost_provenance
            == crate::usage::CostProvenance::SubscriptionUnknown
        {
            estimated_usage.cost_provenance = crate::usage::CostProvenance::SubscriptionUnknown;
        }
        self.accounting = Some(ResponseAccounting {
            estimated_usage,
            native_cost_usd: prior.native_cost_usd,
            upstream_provider: prior.upstream_provider,
        });
        self
    }

    /// Usage suitable for the durable local ledger. Prefer provider telemetry,
    /// otherwise return a nonzero local estimate. A provider's all-zero
    /// placeholder is treated as unavailable rather than measured free usage.
    pub fn accounting_usage(&self) -> TokenUsage {
        let mut usage = self
            .usage
            .as_ref()
            .map(TokenUsage::from)
            .filter(|u| u.total_tokens > 0 || u.input_tokens > 0 || u.output_tokens > 0)
            .unwrap_or_else(|| {
                self.accounting
                    .as_ref()
                    .map(|a| a.estimated_usage.clone())
                    .unwrap_or_else(|| TokenUsage::unknown())
            });
        if let Some(accounting) = &self.accounting {
            usage.upstream_provider = accounting.upstream_provider.clone();
            if let Some(cost) = accounting.native_cost_usd {
                usage.cost_usd = cost;
                usage.cost_known = true;
                usage.cost_provenance = crate::usage::CostProvenance::ProviderReported;
            }
        }
        usage
    }

    pub fn output_text(&self) -> String {
        let mut out = String::new();
        for item in &self.output {
            if let OutputItem::Message { content, phase, .. } = item {
                // Prefer final answers; still include unphased messages
                if phase.as_deref() == Some("commentary") {
                    continue;
                }
                for part in content {
                    if part.type_ == "output_text" {
                        if let Some(t) = &part.text {
                            if !out.is_empty() {
                                out.push('\n');
                            }
                            out.push_str(t);
                        }
                    }
                }
            }
        }
        // Fallback: any message text including commentary
        if out.is_empty() {
            for item in &self.output {
                if let OutputItem::Message { content, .. } = item {
                    for part in content {
                        if part.type_ == "output_text" {
                            if let Some(t) = &part.text {
                                if !out.is_empty() {
                                    out.push('\n');
                                }
                                out.push_str(t);
                            }
                        }
                    }
                }
            }
        }
        out
    }

    pub fn function_calls(&self) -> Vec<FunctionCallRef> {
        let mut calls = Vec::new();
        for item in &self.output {
            match item {
                OutputItem::FunctionCall {
                    call_id,
                    name,
                    arguments,
                    ..
                } => {
                    calls.push(FunctionCallRef {
                        call_id: call_id.clone().unwrap_or_default(),
                        name: name.clone().unwrap_or_default(),
                        arguments: arguments.clone().unwrap_or_else(|| "{}".into()),
                    });
                }
                OutputItem::CustomToolCall {
                    call_id,
                    name,
                    arguments,
                    input,
                    ..
                } => {
                    let args = arguments
                        .clone()
                        .or_else(|| input.clone())
                        .unwrap_or_else(|| "{}".into());
                    calls.push(FunctionCallRef {
                        call_id: call_id.clone().unwrap_or_default(),
                        name: name.clone().unwrap_or_default(),
                        arguments: args,
                    });
                }
                _ => {}
            }
        }
        calls
    }
}

fn estimate_text_tokens(text: &str) -> u64 {
    if text.is_empty() {
        0
    } else {
        // Deliberately conservative for source code / JSON, whose token density
        // is usually higher than prose. This is a budget guard, not billing.
        ((text.chars().count() as u64).saturating_add(2)) / 3
    }
}

fn estimate_value_tokens(value: &Value) -> u64 {
    serde_json::to_string(value)
        .map(|text| estimate_text_tokens(&text))
        .unwrap_or(0)
}

/// Conservative input estimate used before transport dispatch. Exposed for the
/// attempt ledger and budget preflight, not as a replacement for tokenizers.
pub fn estimate_request_input_tokens(req: &ResponseRequest) -> u64 {
    estimate_value_tokens(&req.input)
        .saturating_add(
            req.instructions
                .as_deref()
                .map(estimate_text_tokens)
                .unwrap_or(0),
        )
        .saturating_add(
            req.tools
                .as_ref()
                .map(|tools| {
                    estimate_value_tokens(&serde_json::to_value(tools).unwrap_or_default())
                })
                .unwrap_or(0),
        )
}

fn estimate_output_tokens(output: &[OutputItem]) -> u64 {
    let mut chars = 0usize;
    for item in output {
        match item {
            OutputItem::Message { content, .. } => {
                chars += content
                    .iter()
                    .filter_map(|part| part.text.as_ref())
                    .map(|text| text.chars().count())
                    .sum::<usize>();
            }
            OutputItem::FunctionCall {
                name, arguments, ..
            }
            | OutputItem::CustomToolCall {
                name, arguments, ..
            } => {
                chars += name
                    .as_ref()
                    .map(|value| value.chars().count())
                    .unwrap_or(0);
                chars += arguments
                    .as_ref()
                    .map(|value| value.chars().count())
                    .unwrap_or(0);
            }
            _ => {}
        }
    }
    estimate_text_tokens(&"x".repeat(chars))
}

#[derive(Debug, Clone)]
pub struct FunctionCallRef {
    pub call_id: String,
    pub name: String,
    pub arguments: String,
}

/// Build input items for Responses API (array form).
pub fn user_text_item(text: &str) -> Value {
    serde_json::json!({
        "role": "user",
        "content": [{"type": "input_text", "text": text}]
    })
}

/// Multimodal user message: text plus image/video content parts.
///
/// Meta Responses API: `input_image` / `input_video` with `image_url` / `video_url`
/// (public URL or `data:` URL). See <https://dev.meta.ai/docs/features/image-understanding>
pub fn user_multimodal_item(
    text: &str,
    media: &[(
        /*api type*/ &str,
        /*url field*/ &str,
        /*data url*/ &str,
    )],
) -> Value {
    let mut content = vec![serde_json::json!({
        "type": "input_text",
        "text": text
    })];
    for (type_, url_field, data_url) in media {
        let mut part = serde_json::Map::new();
        part.insert("type".into(), Value::String((*type_).into()));
        part.insert((*url_field).into(), Value::String((*data_url).into()));
        content.push(Value::Object(part));
    }
    serde_json::json!({
        "role": "user",
        "content": content
    })
}

pub fn function_call_output_item(call_id: &str, output: &str) -> Value {
    serde_json::json!({
        "type": "function_call_output",
        "call_id": call_id,
        "output": output
    })
}

/// Replay reasoning + messages + function_calls from a response into next input.
pub fn replay_output_items(output: &[OutputItem]) -> Vec<Value> {
    let mut items = Vec::new();
    for item in output {
        match item {
            OutputItem::Reasoning {
                id,
                summary,
                encrypted_content,
            } => {
                if let Some(enc) = encrypted_content {
                    let mut v = serde_json::json!({
                        "type": "reasoning",
                        "summary": summary,
                        "encrypted_content": enc
                    });
                    if let Some(id) = id {
                        v["id"] = Value::String(id.clone());
                    }
                    items.push(v);
                }
            }
            OutputItem::Message {
                role,
                content,
                phase,
                reasoning_content,
                ..
            } => {
                let role = role.as_deref().unwrap_or("assistant");
                let parts: Vec<Value> = content
                    .iter()
                    .filter_map(|p| {
                        if p.type_ == "output_text" {
                            Some(serde_json::json!({
                                "type": "output_text",
                                "text": p.text.clone().unwrap_or_default()
                            }))
                        } else {
                            None
                        }
                    })
                    .collect();
                if parts.is_empty() && reasoning_content.is_none() {
                    continue;
                }
                let mut msg = serde_json::json!({
                    "role": role,
                    "content": parts
                });
                if let Some(phase) = phase {
                    msg["phase"] = Value::String(phase.clone());
                }
                if let Some(reasoning_content) = reasoning_content {
                    msg["reasoning_content"] = Value::String(reasoning_content.clone());
                }
                // Responses input uses type message optionally
                items.push(msg);
            }
            OutputItem::FunctionCall {
                call_id,
                name,
                arguments,
                ..
            } => {
                items.push(serde_json::json!({
                    "type": "function_call",
                    "call_id": call_id,
                    "name": name,
                    "arguments": arguments
                }));
            }
            OutputItem::CustomToolCall {
                call_id,
                name,
                arguments,
                input,
                ..
            } => {
                let args = arguments.clone().or_else(|| input.clone());
                items.push(serde_json::json!({
                    "type": "function_call",
                    "call_id": call_id,
                    "name": name,
                    "arguments": args
                }));
            }
            OutputItem::Other => {}
        }
    }
    items
}

#[cfg(test)]
mod output_item_tests {
    use super::*;

    #[test]
    fn function_call_arguments_accept_object() {
        let raw = r#"{
          "id": "r1",
          "output": [
            {
              "type": "function_call",
              "call_id": "c1",
              "name": "list_dir",
              "arguments": {"path": "."}
            }
          ]
        }"#;
        let resp: ApiResponse = serde_json::from_str(raw).expect("parse");
        let calls = resp.function_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "list_dir");
        assert!(calls[0].arguments.contains("path"));
    }

    #[test]
    fn custom_tool_call_is_collected() {
        let raw = r#"{
          "output": [
            {
              "type": "custom_tool_call",
              "call_id": "c2",
              "name": "grep",
              "input": "{\"pattern\":\"x\"}"
            }
          ]
        }"#;
        let resp: ApiResponse = serde_json::from_str(raw).expect("parse");
        let calls = resp.function_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "grep");
    }

    #[test]
    fn absent_provider_usage_uses_a_nonzero_local_estimate() {
        let request = ResponseRequest {
            model: "m".into(),
            input: serde_json::json!([{"role":"user","content":[{"type":"input_text","text":"hello"}]}]),
            instructions: None,
            tools: None,
            tool_choice: None,
            store: None,
            include: None,
            reasoning: None,
            stream: None,
            parallel_tool_calls: None,
            prompt_cache_key: None,
            max_output_tokens: None,
        };
        let response = ApiResponse {
            id: None,
            status: Some("completed".into()),
            model: None,
            output: vec![OutputItem::Message {
                id: None,
                role: Some("assistant".into()),
                status: None,
                content: vec![ContentPart {
                    type_: "output_text".into(),
                    text: Some("world".into()),
                }],
                phase: None,
                reasoning_content: None,
            }],
            usage: None,
            error: None,
            accounting: None,
        }
        .with_local_usage_estimate(&request);
        let usage = response.accounting_usage();
        assert_eq!(usage.usage_state, crate::usage::UsageState::Estimated);
        assert!(usage.input_tokens > 0 && usage.output_tokens > 0);
    }
}

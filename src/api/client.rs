use super::types::{ApiResponse, ResponseRequest};
use crate::error::{MuseError, Result};
use crate::providers::ApiStyle;
use futures_util::StreamExt;
use reqwest::Client;
use reqwest::RequestBuilder;

fn effective_base_url(base_url: &str, provider_id: &str, is_oauth: bool) -> String {
    if is_oauth {
        if let Some(fixed) = crate::providers::oauth_base_url(provider_id) {
            return fixed.to_string();
        }
    }
    base_url.trim_end_matches('/').to_string()
}

#[derive(Clone)]
pub struct ApiClient {
    http: Client,
    base_url: String,
    api_key: String,
    provider_id: String,
    oauth: Option<crate::auth::OAuthRequestContext>,
    /// Wire format for this client (Responses / Chat Completions / Anthropic Messages).
    style: ApiStyle,
}

/// Incremental events surfaced while a response streams in.
#[derive(Debug)]
#[allow(dead_code)] // Completed's payload is consumed by some callers only
pub enum StreamEvent {
    /// Assistant output text delta.
    TextDelta(String),
    /// Reasoning summary text delta (model "thinking" summary).
    ReasoningDelta(String),
    /// Terminal event carrying the full final response object.
    Completed(ApiResponse),
}

impl ApiClient {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Result<Self> {
        let http = Client::builder()
            .user_agent(format!("nur-cli/{}", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(300))
            .build()?;
        Ok(Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            provider_id: String::new(),
            oauth: None,
            style: ApiStyle::Responses,
        })
    }

    /// Build a provider-aware client, preserving OAuth routing and metadata.
    pub fn for_provider(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        provider_id: impl Into<String>,
    ) -> Result<Self> {
        let api_key = api_key.into();
        let provider_id = provider_id.into();
        let oauth = crate::auth::oauth_request_context(&provider_id, &api_key);
        let requested_base = base_url.into();
        let effective_base = effective_base_url(&requested_base, &provider_id, oauth.is_some());
        let mut client = Self::new(effective_base, api_key)?;
        client.provider_id = provider_id;
        client.oauth = oauth;
        Ok(client)
    }

    /// Set the wire format from the provider catalog (`ApiStyle`).
    pub fn with_style(mut self, style: ApiStyle) -> Self {
        // Grok Build session tokens target xAI's Responses-based CLI proxy;
        // API-key xAI requests retain the catalog's Chat Completions style.
        self.style = if self.provider_id == "xai" && self.oauth.is_some() {
            ApiStyle::Responses
        } else {
            style
        };
        self
    }

    /// Switch this client to the OpenAI Chat Completions shape.
    /// Prefer [`Self::with_style`] for new code.
    #[allow(dead_code)]
    pub fn with_chat_completions(mut self, on: bool) -> Self {
        self.style = if on {
            ApiStyle::ChatCompletions
        } else {
            ApiStyle::Responses
        };
        self
    }

    fn is_retryable_status(status: u16) -> bool {
        matches!(status, 429 | 500 | 502 | 503 | 504)
    }

    /// Apply auth headers for the active style. Anthropic needs `x-api-key` for
    /// console keys and Bearer + beta for Claude OAuth tokens — never treat
    /// Anthropic as plain Bearer-only Chat Completions.
    fn auth_headers(&self, mut req: RequestBuilder) -> RequestBuilder {
        req = match self.style {
            ApiStyle::AnthropicMessages => {
                req = req.header("anthropic-version", "2023-06-01");
                if self.oauth.is_some() || super::anthropic::is_oauth_token(&self.api_key) {
                    req = req
                        .bearer_auth(&self.api_key)
                        .header("anthropic-beta", super::anthropic::OAUTH_BETA);
                } else {
                    req = req.header("x-api-key", &self.api_key);
                }
                req
            }
            ApiStyle::Responses | ApiStyle::ChatCompletions => req.bearer_auth(&self.api_key),
        };
        if self.provider_id == "openai" {
            if let Some(oauth) = &self.oauth {
                if let Some(account_id) = &oauth.account_id {
                    req = req.header("ChatGPT-Account-ID", account_id);
                }
                if oauth.is_fedramp {
                    req = req.header("X-OpenAI-Fedramp", "true");
                }
            }
        }
        if self.provider_id == "antigravity" {
            if let Some(project_id) = self
                .oauth
                .as_ref()
                .and_then(|context| context.project_id.as_deref())
            {
                req = req.header("x-goog-user-project", project_id);
            }
        }
        if self.provider_id == "github-models" {
            req = req
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2026-03-10");
        }
        req
    }

    pub async fn create_response(&self, req: &ResponseRequest) -> Result<ApiResponse> {
        match self.style {
            ApiStyle::ChatCompletions => return self.create_chat(req).await,
            ApiStyle::AnthropicMessages => return self.create_anthropic(req).await,
            ApiStyle::Responses => {}
        }
        let url = format!("{}/responses", self.base_url);
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            let res = match self
                .auth_headers(
                    self.http
                        .post(&url)
                        .header("Content-Type", "application/json")
                        .json(req),
                )
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    if attempt < 4 {
                        let backoff = std::time::Duration::from_millis(200 * (1 << (attempt - 1)) + rand_jitter());
                        tokio::time::sleep(backoff).await;
                        continue;
                    }
                    return Err(MuseError::Other(format!("request failed after {attempt} attempts: {e}")));
                }
            };

            let status = res.status();
            let headers = res.headers().clone();
            let body = res.text().await.unwrap_or_default();

            if !status.is_success() {
                if Self::is_retryable_status(status.as_u16()) && attempt < 4 {
                    let retry_after = headers
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0);
                    let base = if retry_after > 0 {
                        std::time::Duration::from_secs(retry_after)
                    } else {
                        std::time::Duration::from_millis(300 * (1 << (attempt - 1)) + rand_jitter())
                    };
                    tokio::time::sleep(base).await;
                    continue;
                }
                let msg = parse_error_message(&body).unwrap_or(body.clone());
                return Err(MuseError::Api {
                    status: status.as_u16(),
                    message: msg,
                });
            }

            return parse_response_body(&body, status.as_u16());
        }
    }

    /// Stream a response via SSE. `on_event` receives deltas as they arrive;
    /// the final `ApiResponse` is returned. Falls back to non-streaming
    /// parsing if the server replies with plain JSON.
    pub async fn create_response_stream(
        &self,
        req: &ResponseRequest,
        mut on_event: impl FnMut(StreamEvent),
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<ApiResponse> {
        match self.style {
            ApiStyle::ChatCompletions => {
                return self.create_chat_stream(req, on_event, cancel).await
            }
            ApiStyle::AnthropicMessages => {
                return self.create_anthropic_stream(req, on_event, cancel).await
            }
            ApiStyle::Responses => {}
        }
        let url = format!("{}/responses", self.base_url);
        let mut attempt = 0u32;
        let mut last_err: Option<MuseError> = None;

        loop {
            attempt += 1;
            let res = match self
                .auth_headers(
                    self.http
                        .post(&url)
                        .header("Content-Type", "application/json")
                        .header("Accept", "text/event-stream")
                        .json(req),
                )
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    if attempt < 3 {
                        tokio::time::sleep(std::time::Duration::from_millis(400 * attempt as u64)).await;
                        last_err = Some(MuseError::Other(e.to_string()));
                        continue;
                    }
                    return Err(MuseError::Other(format!("stream connect failed after {attempt}: {e}")));
                }
            };

            let status = res.status();
            let content_type = res
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();

            if !status.is_success() {
                if Self::is_retryable_status(status.as_u16()) && attempt < 3 {
                    let body = res.text().await.unwrap_or_default();
                    last_err = Some(MuseError::Api {
                        status: status.as_u16(),
                        message: parse_error_message(&body).unwrap_or(body),
                    });
                    let backoff = std::time::Duration::from_millis(500 * (1 << (attempt - 1)));
                    tokio::time::sleep(backoff).await;
                    continue;
                }
                let body = res.text().await?;
                let msg = parse_error_message(&body).unwrap_or(body.clone());
                return Err(MuseError::Api {
                    status: status.as_u16(),
                    message: msg,
                });
            }

            // Server ignored stream=true → plain JSON body.
            if !content_type.contains("text/event-stream") {
                let body = res.text().await?;
                return parse_response_body(&body, status.as_u16());
            }

            let mut stream = res.bytes_stream();
            let mut parser = super::sse::SseParser::new();
            let mut final_response: Option<ApiResponse> = None;
            let mut saw_any_data = false;

            loop {
                let chunk = tokio::select! {
                    _ = cancel.cancelled() => return Err(MuseError::Interrupted),
                    c = stream.next() => c,
                };
                let Some(chunk) = chunk else { break };
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(e) => {
                        if attempt < 3 {
                            last_err = Some(MuseError::Other(format!("stream chunk error: {e}")));
                            break;
                        } else {
                            return Err(MuseError::Other(format!("stream chunk error: {e}")));
                        }
                    }
                };

                for data in parser.push(&chunk) {
                    if data.trim() == "[DONE]" {
                        continue;
                    }
                    saw_any_data = true;
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
                        if let Err(e) = handle_sse_json(&v, &mut on_event, &mut final_response) {
                            // If server signaled failure but we have partial response, retry
                            if attempt < 3 {
                                last_err = Some(e);
                                break;
                            } else {
                                return Err(e);
                            }
                        }
                    }
                }
                if final_response.is_some() {
                    break;
                }
            }

            if let Some(fr) = final_response {
                return Ok(fr);
            }

            // Fallback: stream ended without completed response — if we saw deltas, try one more time non-streaming?
            if attempt >= 3 {
                return Err(last_err.unwrap_or_else(|| {
                    MuseError::Other(format!(
                        "stream ended without a completed response (saw_data={saw_any_data})"
                    ))
                }));
            }
            // retry with backoff before next attempt
            tokio::time::sleep(std::time::Duration::from_millis(600 * attempt as u64)).await;
        }
    }

    // ── OpenAI Chat Completions adapter ───────────────────────────────────
    async fn create_chat(&self, req: &ResponseRequest) -> Result<ApiResponse> {
        let url = format!("{}/chat/completions", self.base_url);
        let body = super::chat::build_body_for_provider(req, false, &self.provider_id);
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            let res = self
                .auth_headers(
                    self.http
                        .post(&url)
                        .header("Content-Type", "application/json")
                        .json(&body),
                )
                .send()
                .await;
            let res = match res {
                Ok(r) => r,
                Err(e) if attempt < 4 => {
                    tokio::time::sleep(std::time::Duration::from_millis(300 * attempt as u64)).await;
                    let _ = e;
                    continue;
                }
                Err(e) => return Err(MuseError::Other(format!("request failed: {e}"))),
            };
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            if !status.is_success() {
                if Self::is_retryable_status(status.as_u16()) && attempt < 4 {
                    tokio::time::sleep(std::time::Duration::from_millis(400 * (1 << (attempt - 1)))).await;
                    continue;
                }
                return Err(MuseError::Api {
                    status: status.as_u16(),
                    message: parse_error_message(&text).unwrap_or(text),
                });
            }
            let v: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| MuseError::Other(format!("bad chat response: {e}; body={text}")))?;
            let shaped = super::chat::parse_completion(&v);
            return super::chat::to_api_response(shaped)
                .map_err(|e| MuseError::Other(format!("chat response map failed: {e}")));
        }
    }

    async fn create_chat_stream(
        &self,
        req: &ResponseRequest,
        mut on_event: impl FnMut(StreamEvent),
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<ApiResponse> {
        let url = format!("{}/chat/completions", self.base_url);
        let body = super::chat::build_body_for_provider(req, true, &self.provider_id);
        let res = self
            .auth_headers(
                self.http
                    .post(&url)
                    .header("Content-Type", "application/json")
                    .header("Accept", "text/event-stream")
                    .json(&body),
            )
            .send()
            .await
            .map_err(|e| MuseError::Other(format!("stream connect failed: {e}")))?;

        let status = res.status();
        let content_type = res
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        if !status.is_success() {
            let text = res.text().await.unwrap_or_default();
            return Err(MuseError::Api {
                status: status.as_u16(),
                message: parse_error_message(&text).unwrap_or(text),
            });
        }

        // Server ignored stream=true → plain JSON completion.
        if !content_type.contains("text/event-stream") {
            let text = res.text().await?;
            let v: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| MuseError::Other(format!("bad chat response: {e}; body={text}")))?;
            let shaped = super::chat::parse_completion(&v);
            return super::chat::to_api_response(shaped)
                .map_err(|e| MuseError::Other(format!("chat response map failed: {e}")));
        }

        let mut stream = res.bytes_stream();
        let mut parser = super::sse::SseParser::new();
        let mut acc = super::chat::StreamAccumulator::default();

        loop {
            let chunk = tokio::select! {
                _ = cancel.cancelled() => return Err(MuseError::Interrupted),
                c = stream.next() => c,
            };
            let Some(chunk) = chunk else { break };
            let chunk = chunk.map_err(|e| MuseError::Other(format!("stream chunk error: {e}")))?;
            for data in parser.push(&chunk) {
                if data.trim() == "[DONE]" {
                    continue;
                }
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
                    // Surface provider-side errors mid-stream.
                    if let Some(msg) = v.pointer("/error/message").and_then(|m| m.as_str()) {
                        return Err(MuseError::Api { status: 0, message: msg.to_string() });
                    }
                    if let Some(delta) = acc.push(&v) {
                        on_event(StreamEvent::TextDelta(delta));
                    }
                }
            }
        }

        let shaped = acc.finish();
        let resp = super::chat::to_api_response(shaped)
            .map_err(|e| MuseError::Other(format!("chat stream map failed: {e}")))?;
        on_event(StreamEvent::Completed(resp.clone()));
        Ok(resp)
    }

    // ── Anthropic Messages API ────────────────────────────────────────────
    async fn create_anthropic(&self, req: &ResponseRequest) -> Result<ApiResponse> {
        let url = format!("{}/messages", self.base_url);
        let body = super::anthropic::build_body(req, false);
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            let res = self
                .auth_headers(
                    self.http
                        .post(&url)
                        .header("Content-Type", "application/json")
                        .json(&body),
                )
                .send()
                .await;
            let res = match res {
                Ok(r) => r,
                Err(e) if attempt < 4 => {
                    tokio::time::sleep(std::time::Duration::from_millis(300 * attempt as u64)).await;
                    let _ = e;
                    continue;
                }
                Err(e) => return Err(MuseError::Other(format!("request failed: {e}"))),
            };
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            if !status.is_success() {
                if Self::is_retryable_status(status.as_u16()) && attempt < 4 {
                    tokio::time::sleep(std::time::Duration::from_millis(400 * (1 << (attempt - 1))))
                        .await;
                    continue;
                }
                let mut msg = parse_error_message(&text).unwrap_or(text);
                let code = status.as_u16();
                if code == 404
                    || msg.to_ascii_lowercase().contains("not_found")
                    || msg.to_ascii_lowercase().contains("model:")
                        && msg.to_ascii_lowercase().contains("not found")
                {
                    msg.push_str(&format!(
                        " · tip: model id not available on your plan — /model for the live list \
                         (current Sonnet is {})",
                        super::anthropic::DEFAULT_SONNET
                    ));
                }
                return Err(MuseError::Api {
                    status: code,
                    message: msg,
                });
            }
            let v: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| MuseError::Other(format!("bad anthropic response: {e}; body={text}")))?;
            let shaped = super::anthropic::parse_message(&v);
            return super::chat::to_api_response(shaped)
                .map_err(|e| MuseError::Other(format!("anthropic response map failed: {e}")));
        }
    }

    async fn create_anthropic_stream(
        &self,
        req: &ResponseRequest,
        mut on_event: impl FnMut(StreamEvent),
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<ApiResponse> {
        let url = format!("{}/messages", self.base_url);
        let body = super::anthropic::build_body(req, true);
        let res = self
            .auth_headers(
                self.http
                    .post(&url)
                    .header("Content-Type", "application/json")
                    .header("Accept", "text/event-stream")
                    .json(&body),
            )
            .send()
            .await
            .map_err(|e| MuseError::Other(format!("stream connect failed: {e}")))?;

        let status = res.status();
        let content_type = res
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        if !status.is_success() {
            let text = res.text().await.unwrap_or_default();
            return Err(MuseError::Api {
                status: status.as_u16(),
                message: parse_error_message(&text).unwrap_or(text),
            });
        }

        // Server ignored stream=true → plain JSON message.
        if !content_type.contains("text/event-stream") {
            let text = res.text().await?;
            let v: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| MuseError::Other(format!("bad anthropic response: {e}; body={text}")))?;
            let shaped = super::anthropic::parse_message(&v);
            return super::chat::to_api_response(shaped)
                .map_err(|e| MuseError::Other(format!("anthropic response map failed: {e}")));
        }

        let mut stream = res.bytes_stream();
        let mut parser = super::sse::SseParser::new();
        let mut acc = super::anthropic::StreamAccumulator::default();

        loop {
            let chunk = tokio::select! {
                _ = cancel.cancelled() => return Err(MuseError::Interrupted),
                c = stream.next() => c,
            };
            let Some(chunk) = chunk else { break };
            let chunk = chunk.map_err(|e| MuseError::Other(format!("stream chunk error: {e}")))?;
            for data in parser.push(&chunk) {
                if data.trim().is_empty() {
                    continue;
                }
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
                    if let Some(msg) = v
                        .pointer("/error/message")
                        .and_then(|m| m.as_str())
                        .or_else(|| v.get("error").and_then(|e| e.as_str()))
                    {
                        return Err(MuseError::Api {
                            status: 0,
                            message: msg.to_string(),
                        });
                    }
                    if v.get("type").and_then(|t| t.as_str()) == Some("error") {
                        let msg = v
                            .pointer("/error/message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("anthropic stream error");
                        return Err(MuseError::Api {
                            status: 0,
                            message: msg.to_string(),
                        });
                    }
                    if let Some(delta) = acc.push(&v) {
                        on_event(StreamEvent::TextDelta(delta));
                    }
                }
            }
        }

        let shaped = acc.finish();
        let resp = super::chat::to_api_response(shaped)
            .map_err(|e| MuseError::Other(format!("anthropic stream map failed: {e}")))?;
        on_event(StreamEvent::Completed(resp.clone()));
        Ok(resp)
    }
}

fn handle_sse_json(
    v: &serde_json::Value,
    on_event: &mut impl FnMut(StreamEvent),
    final_response: &mut Option<ApiResponse>,
) -> Result<()> {
    let type_ = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
    if type_.ends_with("output_text.delta") {
        if let Some(d) = v.get("delta").and_then(|d| d.as_str()) {
            on_event(StreamEvent::TextDelta(d.to_string()));
        }
    } else if type_.contains("reasoning") && type_.ends_with(".delta") {
        if let Some(d) = v.get("delta").and_then(|d| d.as_str()) {
            on_event(StreamEvent::ReasoningDelta(d.to_string()));
        }
    } else if type_ == "response.completed"
        || type_ == "response.done"
        || type_ == "response.incomplete"
    {
        if let Some(resp) = v.get("response") {
            let parsed: ApiResponse = serde_json::from_value(resp.clone())?;
            on_event(StreamEvent::Completed(parsed.clone()));
            *final_response = Some(parsed);
        }
    } else if type_ == "response.failed" || type_ == "error" {
        let msg = v
            .pointer("/response/error/message")
            .or_else(|| v.pointer("/error/message"))
            .or_else(|| v.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("stream error")
            .to_string();
        return Err(MuseError::Api {
            status: 0,
            message: msg,
        });
    }
    Ok(())
}

fn parse_response_body(body: &str, status: u16) -> Result<ApiResponse> {
    let parsed: ApiResponse = serde_json::from_str(body).map_err(|e| {
        MuseError::Other(format!("failed to parse API response: {e}; body={body}"))
    })?;

    if let Some(err) = &parsed.error {
        return Err(MuseError::Api {
            status,
            message: err
                .message
                .clone()
                .unwrap_or_else(|| "unknown API error".into()),
        });
    }

    Ok(parsed)
}

fn parse_error_message(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    v.get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            v.get("message")
                .and_then(|m| m.as_str())
                .map(|s| s.to_string())
        })
}

fn rand_jitter() -> u64 {
    // Simple jitter without extra dep — use system time lower bits
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 % 200)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_oauth_cannot_be_redirected_to_public_or_custom_api() {
        assert_eq!(
            effective_base_url("https://example.test/v1", "openai", true),
            crate::providers::OPENAI_OAUTH_BASE_URL
        );
        assert_eq!(
            effective_base_url("https://api.openai.com/v1/", "openai", false),
            "https://api.openai.com/v1"
        );
        assert_eq!(
            effective_base_url("https://api.x.ai/v1", "xai", true),
            crate::providers::XAI_OAUTH_BASE_URL
        );
        assert_eq!(
            effective_base_url("https://example.test/v1", "kimi", true),
            crate::providers::KIMI_CODE_BASE_URL
        );
    }

    #[test]
    fn openai_oauth_applies_account_and_fedramp_headers() {
        let client = ApiClient {
            http: Client::new(),
            base_url: crate::providers::OPENAI_OAUTH_BASE_URL.to_string(),
            api_key: "oauth-token".to_string(),
            provider_id: "openai".to_string(),
            oauth: Some(crate::auth::OAuthRequestContext {
                account_id: Some("acct_test".to_string()),
                is_fedramp: true,
                project_id: None,
            }),
            style: ApiStyle::Responses,
        };
        let request = client
            .auth_headers(client.http.get("https://example.test"))
            .build()
            .unwrap();

        assert_eq!(
            request.headers().get("ChatGPT-Account-ID").unwrap(),
            "acct_test"
        );
        assert_eq!(
            request.headers().get("X-OpenAI-Fedramp").unwrap(),
            "true"
        );
        assert_eq!(
            request.headers().get("Authorization").unwrap(),
            "Bearer oauth-token"
        );
    }

    #[test]
    fn google_oauth_applies_quota_project_header() {
        let client = ApiClient {
            http: Client::new(),
            base_url: "https://generativelanguage.googleapis.com/v1beta/openai".to_string(),
            api_key: "oauth-token".to_string(),
            provider_id: "antigravity".to_string(),
            oauth: Some(crate::auth::OAuthRequestContext {
                account_id: None,
                is_fedramp: false,
                project_id: Some("project-test".to_string()),
            }),
            style: ApiStyle::ChatCompletions,
        };
        let request = client
            .auth_headers(client.http.get("https://example.test"))
            .build()
            .unwrap();

        assert_eq!(
            request.headers().get("x-goog-user-project").unwrap(),
            "project-test"
        );
    }

    #[test]
    fn xai_oauth_uses_responses_while_api_keys_keep_catalog_style() {
        let mut oauth_client = ApiClient::new("https://example.test", "oauth-token").unwrap();
        oauth_client.provider_id = "xai".to_string();
        oauth_client.oauth = Some(crate::auth::OAuthRequestContext::default());
        assert_eq!(
            oauth_client.with_style(ApiStyle::ChatCompletions).style,
            ApiStyle::Responses
        );

        let mut key_client = ApiClient::new("https://api.x.ai/v1", "xai-key").unwrap();
        key_client.provider_id = "xai".to_string();
        assert_eq!(
            key_client.with_style(ApiStyle::ChatCompletions).style,
            ApiStyle::ChatCompletions
        );
    }

    #[test]
    fn github_models_requests_use_the_current_api_contract() {
        let mut client = ApiClient::new("https://models.github.ai/inference", "token").unwrap();
        client.provider_id = "github-models".to_string();
        client.style = ApiStyle::ChatCompletions;
        let request = client
            .auth_headers(client.http.get("https://example.test"))
            .build()
            .unwrap();
        assert_eq!(
            request.headers().get("X-GitHub-Api-Version").unwrap(),
            "2026-03-10"
        );
    }
}

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

fn oauth_blocking<T: Send>(operation: impl FnOnce() -> T + Send) -> T {
    match tokio::runtime::Handle::try_current() {
        Ok(handle)
            if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread =>
        {
            tokio::task::block_in_place(operation)
        }
        Ok(_) => std::thread::scope(|scope| {
            scope
                .spawn(operation)
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
        }),
        Err(_) => operation(),
    }
}

#[derive(Clone)]
pub struct ApiClient {
    http: Client,
    base_url: String,
    api_key: String,
    provider_id: String,
    oauth: Option<crate::auth::OAuthRequestContext>,
    refresh_oauth: bool,
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
            refresh_oauth: false,
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
        client.refresh_oauth = oauth.is_some();
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

    fn api_key_for_request(&self) -> String {
        if self.refresh_oauth {
            let provider_id = self.provider_id.as_str();
            if let Ok(Some(token)) = oauth_blocking(|| {
                crate::auth::resolve_oauth_access_token(provider_id)
            }) {
                return token;
            }
        }
        self.api_key.clone()
    }

    fn refresh_after_unauthorized(&self) -> bool {
        if !self.refresh_oauth {
            return false;
        }
        let provider_id = self.provider_id.as_str();
        oauth_blocking(|| crate::auth::force_refresh_oauth(provider_id)).unwrap_or(false)
    }

    async fn send_with_oauth_retry(
        &self,
        build: impl Fn() -> RequestBuilder,
    ) -> reqwest::Result<reqwest::Response> {
        let response = self.auth_headers(build()).send().await?;
        if response.status().as_u16() == 401 && self.refresh_after_unauthorized() {
            return self.auth_headers(build()).send().await;
        }
        Ok(response)
    }

    /// Apply auth headers for the active style. Anthropic needs `x-api-key` for
    /// console keys and Bearer + beta for Claude OAuth tokens — never treat
    /// Anthropic as plain Bearer-only Chat Completions.
    fn auth_headers(&self, mut req: RequestBuilder) -> RequestBuilder {
        let api_key = self.api_key_for_request();
        let is_claude_oauth =
            self.oauth.is_some() || super::anthropic::is_oauth_token(&api_key);
        req = match self.style {
            ApiStyle::AnthropicMessages => {
                req = req.header("anthropic-version", "2023-06-01");
                if is_claude_oauth {
                    // Claude Code sends oauth + claude-code betas and a cli User-Agent.
                    // Bare `nur-cli/…` + only oauth-2025 often surfaces as HTTP 429.
                    req = req
                        .bearer_auth(&api_key)
                        .header("anthropic-beta", super::anthropic::OAUTH_BETAS)
                        .header("x-app", "cli")
                        .header(
                            "User-Agent",
                            format!("claude-cli/{}", env!("CARGO_PKG_VERSION")),
                        );
                } else {
                    req = req.header("x-api-key", &api_key);
                }
                req
            }
            ApiStyle::Responses | ApiStyle::ChatCompletions => req.bearer_auth(&api_key),
        };
        if self.provider_id == "openai" {
            if let Some(oauth) = &self.oauth {
                // Codex backend requires a known originator (`codex_cli_rs`) +
                // account id + OpenAI-Beta; unknown originators are rejected.
                const OPENAI_ORIGINATOR: &str = "codex_cli_rs";
                req = req
                    .header("originator", OPENAI_ORIGINATOR)
                    .header("OpenAI-Beta", "responses_websockets=2026-02-06")
                    .header(
                        "User-Agent",
                        format!("{OPENAI_ORIGINATOR}/{}", env!("CARGO_PKG_VERSION")),
                    );
                if let Some(account_id) = &oauth.account_id {
                    req = req
                        .header("ChatGPT-Account-ID", account_id)
                        .header("ChatGPT-Account-Id", account_id)
                        .header("chatgpt-account-id", account_id);
                }
                if oauth.is_fedramp {
                    req = req.header("X-OpenAI-Fedramp", "true");
                }
            }
        }
        if self.provider_id == "antigravity" || self.provider_id == "google" {
            if let Some(project_id) = self
                .oauth
                .as_ref()
                .and_then(|context| context.project_id.as_deref())
            {
                req = req.header("x-goog-user-project", project_id);
            }
        }
        if self.provider_id == "kimi" && self.oauth.is_some() {
            if let Ok(headers) = crate::oauth::kimi_request_headers() {
                for (name, value) in headers {
                    req = req.header(name, value);
                }
            }
        }
        // Grok Build OAuth → cli-chat-proxy enforces a CLI version fingerprint.
        // Missing `x-grok-client-version` is reported as version "(none)" → HTTP 426.
        if self.provider_id == "xai" && self.oauth.is_some() {
            let ver = crate::providers::xai_grok_cli_version();
            req = req
                .header("x-grok-client-version", ver.as_str())
                .header("X-XAI-Token-Auth", "xai-grok-cli")
                .header("User-Agent", format!("xai-grok-workspace/{ver}"));
        }
        if self.provider_id == "github-models" {
            req = req
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28");
        }
        if self.provider_id == "github-copilot" {
            // Do NOT send X-GitHub-Api-Version — Copilot returns "invalid apiVersion".
            // Headers must look like VS Code Copilot Chat (see litellm / openclaw).
            req = req
                .header("Editor-Version", "vscode/1.104.1")
                .header("Editor-Plugin-Version", "copilot-chat/0.26.7")
                .header("Copilot-Integration-Id", "vscode-chat")
                .header("User-Agent", "GitHubCopilotChat/0.26.7")
                .header("Openai-Intent", "conversation-panel")
                .header("Openai-Organization", "github-copilot")
                .header("X-Request-Id", uuid_simple());
        }
        req
    }

    pub async fn create_response(&self, req: &ResponseRequest) -> Result<ApiResponse> {
        match self.style {
            ApiStyle::ChatCompletions => return self.create_chat(req).await,
            ApiStyle::AnthropicMessages => return self.create_anthropic(req).await,
            ApiStyle::Responses => {}
        }
        // ChatGPT/Codex OAuth backend often ignores stream:false and returns SSE.
        // Collect the completed event rather than failing JSON parse on `event:`.
        let url = format!("{}/responses", self.base_url);
        let mut attempt = 0u32;
        let mut oauth_refreshed = false;
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
                if status.as_u16() == 401
                    && !oauth_refreshed
                    && self.refresh_after_unauthorized()
                {
                    oauth_refreshed = true;
                    continue;
                }
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

            return parse_success_body(&body, status.as_u16());
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
        // Codex/ChatGPT OAuth always streams Responses events; force stream=true
        // so the body matches what we parse.
        let mut stream_req = req.clone();
        stream_req.stream = Some(true);
        let url = format!("{}/responses", self.base_url);
        let mut attempt = 0u32;
        let mut last_err: Option<MuseError> = None;
        let mut oauth_refreshed = false;

        loop {
            attempt += 1;
            let res = match self
                .auth_headers(
                    self.http
                        .post(&url)
                        .header("Content-Type", "application/json")
                        .header("Accept", "text/event-stream")
                        .json(&stream_req),
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
                if status.as_u16() == 401
                    && !oauth_refreshed
                    && self.refresh_after_unauthorized()
                {
                    oauth_refreshed = true;
                    continue;
                }
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

            // Prefer streaming by content-type; Codex sometimes returns SSE with a
            // non-event-stream Content-Type (or none). Peek is impossible after
            // streaming starts, so when CT is wrong we buffer the whole body and
            // detect SSE by payload shape.
            let use_byte_stream = content_type.contains("text/event-stream")
                || content_type.contains("application/x-ndjson")
                || content_type.is_empty();

            if !use_byte_stream {
                let body = res.text().await?;
                if body_looks_like_sse(&body) {
                    return consume_sse_text(&body, &mut on_event);
                }
                return parse_success_body(&body, status.as_u16());
            }

            let mut stream = res.bytes_stream();
            let mut parser = super::sse::SseParser::new();
            let mut final_response: Option<ApiResponse> = None;
            let mut streamed_items: Vec<super::types::OutputItem> = Vec::new();
            let mut saw_any_data = false;
            let mut buffered: Vec<u8> = Vec::new();
            // If CT was empty/ambiguous, accumulate first chunk to detect pure JSON.
            let mut maybe_json_only = !content_type.contains("text/event-stream");

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

                if maybe_json_only {
                    buffered.extend_from_slice(&chunk);
                    // Wait until we have enough to tell SSE vs JSON, or a blank line.
                    let preview = String::from_utf8_lossy(&buffered);
                    if buffered.len() < 16
                        && !preview.contains('\n')
                        && !preview.trim_start().starts_with('{')
                    {
                        continue;
                    }
                    maybe_json_only = false;
                    if !body_looks_like_sse(&preview) && preview.trim_start().starts_with('{') {
                        // Drain remaining body for full JSON object.
                        while let Some(Ok(more)) = stream.next().await {
                            buffered.extend_from_slice(&more);
                        }
                        let body = String::from_utf8_lossy(&buffered).into_owned();
                        return parse_success_body(&body, status.as_u16());
                    }
                    // Treat buffered prefix as SSE.
                    for data in parser.push(&buffered) {
                        if data.trim() == "[DONE]" {
                            continue;
                        }
                        saw_any_data = true;
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
                            if let Err(e) = handle_sse_json(
                                &v,
                                &mut on_event,
                                &mut final_response,
                                &mut streamed_items,
                            ) {
                                if attempt < 3 {
                                    last_err = Some(e);
                                    break;
                                } else {
                                    return Err(e);
                                }
                            }
                        }
                    }
                    buffered.clear();
                    if final_response.is_some() {
                        break;
                    }
                    continue;
                }

                for data in parser.push(&chunk) {
                    if data.trim() == "[DONE]" {
                        continue;
                    }
                    saw_any_data = true;
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
                        if let Err(e) = handle_sse_json(
                            &v,
                            &mut on_event,
                            &mut final_response,
                            &mut streamed_items,
                        ) {
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
            // Stream ended with items but no completed event — still usable.
            if !streamed_items.is_empty() {
                return Ok(ApiResponse {
                    id: None,
                    status: Some("completed".into()),
                    model: None,
                    output: streamed_items,
                    usage: None,
                    error: None,
                });
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
        let mut oauth_refreshed = false;
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
                if status.as_u16() == 401
                    && !oauth_refreshed
                    && self.refresh_after_unauthorized()
                {
                    oauth_refreshed = true;
                    continue;
                }
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
            .send_with_oauth_retry(|| {
                self.http
                    .post(&url)
                    .header("Content-Type", "application/json")
                    .header("Accept", "text/event-stream")
                    .json(&body)
            })
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
        let oauth = self.oauth.is_some() || super::anthropic::is_oauth_token(&self.api_key);
        let body = super::anthropic::build_body_with_oauth(req, false, oauth);
        let mut attempt = 0u32;
        let mut oauth_refreshed = false;
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
                if status.as_u16() == 401
                    && !oauth_refreshed
                    && self.refresh_after_unauthorized()
                {
                    oauth_refreshed = true;
                    continue;
                }
                // Opaque OAuth 429 is usually wrong client identity, not a real
                // temporary rate limit — don't thrash retries.
                let is_oauth_429 = status.as_u16() == 429 && oauth;
                if Self::is_retryable_status(status.as_u16()) && attempt < 4 && !is_oauth_429 {
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
                if is_oauth_429 {
                    msg.push_str(
                        " · Claude OAuth needs Claude Code system identity (Nur injects this) — \
                         upgrade to latest nur, or use ANTHROPIC_API_KEY if usage is exhausted",
                    );
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
        let oauth = self.oauth.is_some() || super::anthropic::is_oauth_token(&self.api_key);
        let body = super::anthropic::build_body_with_oauth(req, true, oauth);
        let res = self
            .send_with_oauth_retry(|| {
                self.http
                    .post(&url)
                    .header("Content-Type", "application/json")
                    .header("Accept", "text/event-stream")
                    .json(&body)
            })
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
    streamed_items: &mut Vec<super::types::OutputItem>,
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
    } else if type_ == "response.output_item.done" {
        // Codex / ChatGPT OAuth deliver the real output (messages + function_calls)
        // as streaming output_item.done events. `response.completed` often has
        // empty `output: []` and only carries id/usage — if we only parse
        // completed, tools silently disappear and the agent "only plans".
        if let Some(item_val) = v.get("item") {
            match serde_json::from_value::<super::types::OutputItem>(item_val.clone()) {
                Ok(super::types::OutputItem::Other) => {
                    // Unknown shape — keep raw for debugging later if needed.
                }
                Ok(item) => {
                    streamed_items.push(item);
                }
                Err(_) => {
                    // Tolerate partial/unrecognized items; completed may still help.
                }
            }
        }
    } else if type_ == "response.completed"
        || type_ == "response.done"
        || type_ == "response.incomplete"
    {
        if let Some(resp) = v.get("response") {
            let mut parsed: ApiResponse = serde_json::from_value(resp.clone())?;
            // Prefer streamed items when completed.output is empty or thinner
            // (fewer tool calls) than what we already collected.
            if !streamed_items.is_empty() {
                let streamed_calls = count_tool_items(streamed_items);
                let completed_calls = count_tool_items(&parsed.output);
                if parsed.output.is_empty() || streamed_calls > completed_calls {
                    parsed.output = std::mem::take(streamed_items);
                } else {
                    streamed_items.clear();
                }
            }
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

fn count_tool_items(items: &[super::types::OutputItem]) -> usize {
    items
        .iter()
        .filter(|i| {
            matches!(
                i,
                super::types::OutputItem::FunctionCall { .. }
                    | super::types::OutputItem::CustomToolCall { .. }
            )
        })
        .count()
}

/// ChatGPT/Codex (and some gateways) return SSE even when Content-Type is wrong.
fn body_looks_like_sse(body: &str) -> bool {
    let t = body.trim_start();
    t.starts_with("event:")
        || t.starts_with("data:")
        || t.starts_with(": ")
        || t.contains("\nevent:")
        || t.contains("\rdata:")
}

fn parse_success_body(body: &str, status: u16) -> Result<ApiResponse> {
    if body_looks_like_sse(body) {
        let mut noop = |_ev: StreamEvent| {};
        return consume_sse_text(body, &mut noop);
    }
    parse_response_body(body, status)
}

/// Drain a full SSE text body into text/reasoning deltas + final ApiResponse.
fn consume_sse_text(
    body: &str,
    on_event: &mut impl FnMut(StreamEvent),
) -> Result<ApiResponse> {
    let mut parser = super::sse::SseParser::new();
    let mut events = parser.push(body.as_bytes());
    // Flush trailing event if the body lacked a final blank line.
    events.extend(parser.push(b"\n\n"));
    let mut final_response: Option<ApiResponse> = None;
    let mut streamed_items: Vec<super::types::OutputItem> = Vec::new();
    for data in events {
        if data.trim() == "[DONE]" {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
            handle_sse_json(&v, on_event, &mut final_response, &mut streamed_items)?;
        }
    }
    // If the stream closed after output_item.done but without completed, still
    // surface what we collected (rare, but better than total silence).
    if final_response.is_none() && !streamed_items.is_empty() {
        final_response = Some(ApiResponse {
            id: None,
            status: Some("completed".into()),
            model: None,
            output: streamed_items,
            usage: None,
            error: None,
        });
    }
    final_response.ok_or_else(|| {
        MuseError::Other(
            "Codex/Responses SSE ended without response.completed (check auth and model)"
                .into(),
        )
    })
}

fn parse_response_body(body: &str, status: u16) -> Result<ApiResponse> {
    let parsed: ApiResponse = serde_json::from_str(body).map_err(|e| {
        let snippet: String = body.chars().take(240).collect();
        MuseError::Other(format!("failed to parse API response: {e}; body={snippet}"))
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
    // JSON error shapes first.
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(msg) = v
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                v.get("message")
                    .and_then(|m| m.as_str())
                    .map(|s| s.to_string())
            })
        {
            return Some(msg);
        }
    }
    // SSE error event: extract last data: line's message if present.
    if body_looks_like_sse(body) {
        let mut parser = super::sse::SseParser::new();
        let mut events = parser.push(body.as_bytes());
        events.extend(parser.push(b"\n\n"));
        for data in events.into_iter().rev() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
                if let Some(msg) = v
                    .pointer("/error/message")
                    .or_else(|| v.pointer("/response/error/message"))
                    .or_else(|| v.get("message"))
                    .and_then(|m| m.as_str())
                {
                    return Some(msg.to_string());
                }
            }
        }
    }
    None
}

fn rand_jitter() -> u64 {
    // Simple jitter without extra dep — use system time lower bits
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 % 200)
        .unwrap_or(0)
}

fn uuid_simple() -> String {
    // Enough uniqueness for X-Request-Id without pulling uuid into this module's hot path.
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{n:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_body_detection_and_completed_parse() {
        assert!(body_looks_like_sse(
            "event: response.created\ndata: {\"type\":\"response.created\"}\n\n"
        ));
        assert!(!body_looks_like_sse("{\"id\":\"resp_1\",\"output\":[]}"));

        // Minimal Codex-shaped SSE: created then completed with empty output.
        let body = "event: response.created\n\
data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"status\":\"in_progress\",\"output\":[]}}\n\
\n\
event: response.completed\n\
data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"status\":\"completed\",\"output\":[]}}\n\
\n";
        let resp = consume_sse_text(body, &mut |_ev| {})
            .expect("parse codex-shaped sse");
        assert_eq!(resp.id.as_deref(), Some("resp_1"));
        assert_eq!(resp.status.as_deref(), Some("completed"));
    }

    #[test]
    fn codex_output_item_done_tools_survive_empty_completed_output() {
        // Real Codex/ChatGPT OAuth pattern: tools arrive as output_item.done;
        // response.completed often has output: [].
        let body = r#"event: response.output_item.done
data: {"type":"response.output_item.done","item":{"type":"function_call","call_id":"c1","name":"list_dir","arguments":"{\"path\":\".\"}"}}

event: response.output_item.done
data: {"type":"response.output_item.done","item":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"looking around"}]}}

event: response.completed
data: {"type":"response.completed","response":{"id":"resp_tools","status":"completed","output":[],"usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15}}}

"#;
        let resp = consume_sse_text(body, &mut |_ev| {}).expect("parse");
        assert_eq!(resp.id.as_deref(), Some("resp_tools"));
        let calls = resp.function_calls();
        assert_eq!(calls.len(), 1, "function_call must not be dropped: {resp:?}");
        assert_eq!(calls[0].name, "list_dir");
        assert!(resp.output_text().contains("looking around"));
    }

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
            refresh_oauth: false,
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
        assert_eq!(
            request.headers().get("originator").and_then(|v| v.to_str().ok()),
            Some("codex_cli_rs"),
            "unknown originator makes authorize/API fail"
        );
        assert_eq!(
            request
                .headers()
                .get("OpenAI-Beta")
                .and_then(|v| v.to_str().ok()),
            Some("responses_websockets=2026-02-06")
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
            refresh_oauth: false,
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
    fn xai_oauth_requests_send_cli_version_fingerprint() {
        // cli-chat-proxy returns 426 with version "(none)" without these headers.
        let mut client = ApiClient::new(
            crate::providers::XAI_OAUTH_BASE_URL,
            "oauth-token",
        )
        .unwrap();
        client.provider_id = "xai".to_string();
        client.oauth = Some(crate::auth::OAuthRequestContext::default());
        client.style = ApiStyle::Responses;
        let request = client
            .auth_headers(client.http.post("https://example.test/v1/responses"))
            .build()
            .unwrap();
        let ver = crate::providers::xai_grok_cli_version();
        assert_eq!(
            request
                .headers()
                .get("x-grok-client-version")
                .and_then(|v| v.to_str().ok()),
            Some(ver.as_str()),
            "missing x-grok-client-version causes 426 version (none)"
        );
        assert_eq!(
            request
                .headers()
                .get("X-XAI-Token-Auth")
                .and_then(|v| v.to_str().ok()),
            Some("xai-grok-cli")
        );
        let ua = request
            .headers()
            .get("User-Agent")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(
            ua.contains(&format!("xai-grok-workspace/{ver}")) || ua.contains(&ver),
            "User-Agent should fingerprint workspace CLI, got {ua}"
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
            "2022-11-28"
        );
    }

    #[test]
    fn github_copilot_does_not_send_github_api_version() {
        let mut client = ApiClient::new("https://api.githubcopilot.com", "token").unwrap();
        client.provider_id = "github-copilot".to_string();
        client.style = ApiStyle::ChatCompletions;
        let request = client
            .auth_headers(client.http.post("https://example.test/v1/chat/completions"))
            .build()
            .unwrap();
        assert!(
            request.headers().get("X-GitHub-Api-Version").is_none(),
            "X-GitHub-Api-Version causes Copilot invalid apiVersion"
        );
        assert_eq!(
            request
                .headers()
                .get("Editor-Version")
                .and_then(|v| v.to_str().ok()),
            Some("vscode/1.104.1")
        );
        assert_eq!(
            request
                .headers()
                .get("Copilot-Integration-Id")
                .and_then(|v| v.to_str().ok()),
            Some("vscode-chat")
        );
    }
}

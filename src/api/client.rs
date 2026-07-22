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

/// Provider endpoints that have told us they cannot accept images.
///
/// Learned at runtime from the first rejected request and remembered for the
/// process, so a session carrying an old screenshot keeps working after the
/// user switches to a text-only local model instead of failing every turn.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct EndpointKey {
    provider_id: String,
    base_url: String,
    model: String,
}

fn text_only_endpoints() -> &'static std::sync::Mutex<std::collections::HashSet<EndpointKey>> {
    static SEEN: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<EndpointKey>>> =
        std::sync::OnceLock::new();
    SEEN.get_or_init(Default::default)
}

fn endpoint_key(provider_id: &str, base_url: &str, model: &str) -> EndpointKey {
    EndpointKey {
        provider_id: provider_id.to_string(),
        base_url: base_url.trim_end_matches('/').to_string(),
        model: model.to_string(),
    }
}

/// Has this endpoint already refused images this process?
pub fn endpoint_is_text_only(provider_id: &str, base_url: &str, model: &str) -> bool {
    text_only_endpoints()
        .lock()
        .map(|s| s.contains(&endpoint_key(provider_id, base_url, model)))
        .unwrap_or(false)
}

fn mark_text_only(provider_id: &str, base_url: &str, model: &str) {
    if let Ok(mut s) = text_only_endpoints().lock() {
        s.insert(endpoint_key(provider_id, base_url, model));
    }
    tracing::warn!(
        provider = provider_id,
        endpoint = base_url,
        model,
        "endpoint has no vision support - replaying attachments as text placeholders"
    );
}

fn oauth_blocking<T: Send>(operation: impl FnOnce() -> T + Send) -> T {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
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

    /// For local providers, `local-model` is a 400 on real servers. Group C
    /// proved `POST {"model":"local-model"}` → 400 on a live llama.cpp instance
    /// while a real id from `GET /v1/models` → 200. Lazily resolve by hitting
    /// `/models` first; on failure keep the original so the error is still
    /// surfaced.
    pub async fn resolve_local_model(&self, model: &str) -> String {
        if !crate::providers::is_placeholder_local_model(model) {
            return model.to_string();
        }
        if !crate::providers::is_local_provider(&self.provider_id) {
            // Also allow localhost base_urls even when provider_id is custom
            let is_localhost = self.base_url.contains("localhost")
                || self.base_url.contains("127.0.0.1")
                || self.base_url.contains("::1");
            if !is_localhost {
                return model.to_string();
            }
        }
        let url = format!("{}/models", self.base_url.trim_end_matches('/'));
        // Local servers are expected to answer quickly; keep it short.
        let req = self.http.get(&url).timeout(std::time::Duration::from_secs(5));
        // Apply auth if any — most local servers allow empty bearer.
        let req = if self.api_key.trim().is_empty() {
            req
        } else {
            self.auth_headers(req)
        };
        let res = match req.send().await {
            Ok(r) => r,
            Err(_) => return model.to_string(),
        };
        if !res.status().is_success() {
            return model.to_string();
        }
        let body = match res.text().await {
            Ok(b) => b,
            Err(_) => return model.to_string(),
        };
        if let Ok(ids) = crate::api::models::parse_model_ids(&body) {
            if let Some(first) = ids.into_iter().next() {
                if !first.trim().is_empty() && first != "local-model" {
                    tracing::info!(
                        provider = %self.provider_id,
                        placeholder = %model,
                        resolved = %first,
                        "resolved local placeholder model via /models"
                    );
                    return first;
                }
            }
        }
        model.to_string()
    }

    /// Is this client pointed at an OpenCode gateway (Zen or Go)?
    ///
    /// Only that route opts into the message-based retries below: OpenCode
    /// reports a failing *upstream* provider as a client error
    /// (`400 {"error":{"message":"Error from provider (Console Go): Upstream
    /// request failed"}}`) even though the request itself was valid. Every
    /// other provider keeps plain status-based retries — a 400 there is a real
    /// bad request and retrying it just burns the turn.
    fn is_opencode_route(&self) -> bool {
        self.provider_id == "opencode" || self.base_url.contains("opencode.ai")
    }

    fn api_key_for_request(&self) -> String {
        if self.refresh_oauth {
            let provider_id = self.provider_id.as_str();
            if let Ok(Some(token)) =
                oauth_blocking(|| crate::auth::resolve_oauth_access_token(provider_id))
            {
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
        let is_claude_oauth = self.oauth.is_some() || super::anthropic::is_oauth_token(&api_key);
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
        if self.provider_id == "google" {
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
                        let backoff = std::time::Duration::from_millis(
                            200 * (1 << (attempt - 1)) + rand_jitter(),
                        );
                        tokio::time::sleep(backoff).await;
                        continue;
                    }
                    return Err(MuseError::Other(format!(
                        "request failed after {attempt} attempts: {e}"
                    )));
                }
            };

            let status = res.status();
            let headers = res.headers().clone();
            let body = res.text().await.unwrap_or_default();

            if !status.is_success() {
                if status.as_u16() == 401 && !oauth_refreshed && self.refresh_after_unauthorized() {
                    oauth_refreshed = true;
                    continue;
                }
                // Retry on transient upstream failures from gateways like OpenCode (Console Go)
                // which surface as 400 with "Upstream request failed".
                let message = parse_error_message(&body).unwrap_or_else(|| body.clone());
                let retryable =
                    is_retryable_error(status.as_u16(), &message, self.is_opencode_route());
                if retryable && attempt < 4 {
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
                        tokio::time::sleep(std::time::Duration::from_millis(400 * attempt as u64))
                            .await;
                        last_err = Some(MuseError::Other(e.to_string()));
                        continue;
                    }
                    return Err(MuseError::Other(format!(
                        "stream connect failed after {attempt}: {e}"
                    )));
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
                if status.as_u16() == 401 && !oauth_refreshed && self.refresh_after_unauthorized() {
                    oauth_refreshed = true;
                    continue;
                }
                // Need the body to spot an OpenCode gateway's upstream failure,
                // which arrives as a 400 with a transient message.
                let body_text = res.text().await.unwrap_or_default();
                let msg = parse_error_message(&body_text).unwrap_or(body_text);
                if is_retryable_error(status.as_u16(), &msg, self.is_opencode_route())
                    && attempt < 3
                {
                    last_err = Some(MuseError::Api {
                        status: status.as_u16(),
                        message: msg,
                    });
                    let backoff = std::time::Duration::from_millis(500 * (1 << (attempt - 1)));
                    tokio::time::sleep(backoff).await;
                    continue;
                }
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
                let Some(chunk) = chunk else {
                    // Body ended. Two things can still be sitting unflushed: a
                    // body so short it never reached the SSE-vs-JSON sniff
                    // threshold, and a final SSE event the server never
                    // terminated with a blank line. Neither can reach the
                    // handler below, so drain both before leaving the loop.
                    let mut tail: Vec<String> = Vec::new();
                    if maybe_json_only && !buffered.is_empty() {
                        let body = String::from_utf8_lossy(&buffered).into_owned();
                        if !body_looks_like_sse(&body) && body.trim_start().starts_with('{') {
                            return parse_success_body(&body, status.as_u16());
                        }
                        tail.extend(parser.push(&buffered));
                        buffered.clear();
                    }
                    tail.extend(parser.finish());
                    for data in tail {
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
                    break;
                };
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
        let has_media = super::chat::request_has_media(req);
        let mut drop_media =
            has_media && endpoint_is_text_only(&self.provider_id, &self.base_url, &req.model);
        let mut body = super::chat::build_body_opts(req, false, &self.provider_id, drop_media);
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
                    tokio::time::sleep(std::time::Duration::from_millis(300 * attempt as u64))
                        .await;
                    let _ = e;
                    continue;
                }
                Err(e) => return Err(MuseError::Other(format!("request failed: {e}"))),
            };
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            if !status.is_success() {
                if status.as_u16() == 401 && !oauth_refreshed && self.refresh_after_unauthorized() {
                    oauth_refreshed = true;
                    continue;
                }
                let message = parse_error_message(&text).unwrap_or(text);
                // Text-only endpoint choking on a replayed attachment: strip the
                // media and try once more before surfacing the failure.
                if has_media && !drop_media && super::chat::is_media_unsupported_error(&message) {
                    mark_text_only(&self.provider_id, &self.base_url, &req.model);
                    drop_media = true;
                    body = super::chat::build_body_opts(req, false, &self.provider_id, true);
                    continue;
                }
                if is_retryable_error(status.as_u16(), &message, self.is_opencode_route())
                    && attempt < 4
                {
                    tokio::time::sleep(std::time::Duration::from_millis(
                        400 * (1 << (attempt - 1)),
                    ))
                    .await;
                    continue;
                }
                return Err(MuseError::Api {
                    status: status.as_u16(),
                    message,
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
        let has_media = super::chat::request_has_media(req);
        let mut drop_media =
            has_media && endpoint_is_text_only(&self.provider_id, &self.base_url, &req.model);

        // Connect phase, retried once without attachments if the endpoint turns
        // out to be text-only. Nothing has streamed yet at this point, so the
        // retry cannot duplicate output.
        let mut attempt = 0u32;
        let (res, content_type) = loop {
            attempt += 1;
            let body = super::chat::build_body_opts(req, true, &self.provider_id, drop_media);
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
                let message = parse_error_message(&text).unwrap_or(text);
                if has_media && !drop_media && super::chat::is_media_unsupported_error(&message) {
                    mark_text_only(&self.provider_id, &self.base_url, &req.model);
                    drop_media = true;
                    continue;
                }
                // Streaming chat completions is the path OpenCode actually uses,
                // and it had no retry at all: a single `400 Upstream request
                // failed` (or a 429/502 from the gateway) killed the turn even
                // though nothing had streamed yet. Retry is confined to the
                // OpenCode route by `is_retryable_error`; other providers keep
                // failing fast exactly as before.
                if self.is_opencode_route()
                    && is_retryable_error(status.as_u16(), &message, true)
                    && attempt < 3
                {
                    tokio::time::sleep(std::time::Duration::from_millis(
                        400 * (1 << (attempt - 1)),
                    ))
                    .await;
                    continue;
                }
                return Err(MuseError::Api {
                    status: status.as_u16(),
                    message,
                });
            }
            break (res, content_type);
        };

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
            // A body that ends without a final blank line still has one whole
            // event sitting in the parser — often the `finish_reason` or the
            // error frame. Flush it instead of letting the stream end silently.
            let end_of_body = chunk.is_none();
            let events = match chunk {
                Some(chunk) => {
                    let chunk =
                        chunk.map_err(|e| MuseError::Other(format!("stream chunk error: {e}")))?;
                    parser.push(&chunk)
                }
                None => parser.finish().into_iter().collect(),
            };
            for data in events {
                if data.trim() == "[DONE]" {
                    continue;
                }
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
                    // Surface provider-side errors mid-stream.
                    let error_message = v
                        .pointer("/error/message")
                        .and_then(|m| m.as_str())
                        // OpenCode also emits the bare-string form
                        // (`{"error":"Upstream request failed"}`), which was
                        // silently dropped here — the stream then ended with no
                        // content and the turn looked like it just hung.
                        .or_else(|| {
                            if self.is_opencode_route() {
                                v.get("error").and_then(|e| e.as_str())
                            } else {
                                None
                            }
                        });
                    if let Some(msg) = error_message {
                        return Err(MuseError::Api {
                            status: 0,
                            message: msg.to_string(),
                        });
                    }
                    for delta in acc.push(&v) {
                        on_event(match delta {
                            super::chat::ChatDelta::Text(t) => StreamEvent::TextDelta(t),
                            super::chat::ChatDelta::Reasoning(t) => StreamEvent::ReasoningDelta(t),
                        });
                    }
                }
            }
            if end_of_body {
                break;
            }
        }

        let saw_reasoning = !acc.reasoning.is_empty();
        let shaped = acc.finish();
        let resp = super::chat::to_api_response(shaped)
            .map_err(|e| MuseError::Other(format!("chat stream map failed: {e}")))?;
        // An OpenCode gateway that loses its upstream mid-turn can close a 200
        // stream having sent nothing usable. Reporting that as a completed
        // (empty) turn looked like a hang; as an error the agent loop can retry
        // or fail over. Scoped to OpenCode so no other provider's empty reply
        // changes meaning.
        if self.is_opencode_route() && resp.output.is_empty() && !saw_reasoning {
            return Err(MuseError::Api {
                status: 0,
                message: "OpenCode returned an empty stream (upstream request failed \
                          before any content) — retry or /model to another route"
                    .into(),
            });
        }
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
                    tokio::time::sleep(std::time::Duration::from_millis(300 * attempt as u64))
                        .await;
                    let _ = e;
                    continue;
                }
                Err(e) => return Err(MuseError::Other(format!("request failed: {e}"))),
            };
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            if !status.is_success() {
                if status.as_u16() == 401 && !oauth_refreshed && self.refresh_after_unauthorized() {
                    oauth_refreshed = true;
                    continue;
                }
                // Opaque OAuth 429 is usually wrong client identity, not a real
                // temporary rate limit — don't thrash retries.
                let is_oauth_429 = status.as_u16() == 429 && oauth;
                // Also retry 4xx-wrapped upstream failures, but only when this
                // client is actually talking to an OpenCode gateway — a real
                // Anthropic 4xx must still fail fast.
                let retry_msg = parse_error_message(&text).unwrap_or_else(|| text.clone());
                let retryable =
                    is_retryable_error(status.as_u16(), &retry_msg, self.is_opencode_route());
                if retryable && attempt < 4 && !is_oauth_429 {
                    tokio::time::sleep(std::time::Duration::from_millis(
                        400 * (1 << (attempt - 1)),
                    ))
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
                // (No second transient-upstream retry here: `retryable` above
                // already covers it, and re-checking the same needles after the
                // attempt budget is spent only delayed the error.)
                return Err(MuseError::Api {
                    status: code,
                    message: msg,
                });
            }
            let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
                MuseError::Other(format!("bad anthropic response: {e}; body={text}"))
            })?;
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
            let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
                MuseError::Other(format!("bad anthropic response: {e}; body={text}"))
            })?;
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
            // Flush the parser once the body ends — Anthropic's terminal
            // `message_stop`, and any `type: error` frame, is exactly the event
            // that arrives last and so is the one a missing blank line drops.
            let end_of_body = chunk.is_none();
            let events = match chunk {
                Some(chunk) => {
                    let chunk =
                        chunk.map_err(|e| MuseError::Other(format!("stream chunk error: {e}")))?;
                    parser.push(&chunk)
                }
                None => parser.finish().into_iter().collect(),
            };
            for data in events {
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
            if end_of_body {
                break;
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
fn consume_sse_text(body: &str, on_event: &mut impl FnMut(StreamEvent)) -> Result<ApiResponse> {
    let mut parser = super::sse::SseParser::new();
    let mut events = parser.push(body.as_bytes());
    // Flush trailing event if the body lacked a final blank line.
    events.extend(parser.finish());
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
            "Codex/Responses SSE ended without response.completed (check auth and model)".into(),
        )
    })
}

/// Does this error text describe a *gateway-side* upstream failure rather than
/// a problem with the request we sent?
///
/// OpenCode Zen/Go proxy other vendors and surface their outages verbatim —
/// `Error from provider (Console Go): Upstream request failed` — usually with a
/// 400, which is otherwise a permanent "your request is wrong" status.
fn is_transient_upstream_message(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "upstream request failed",
        "error from provider",
        "upstream error",
        "console go",
        "provider error",
        "upstream failed",
        "upstream timeout",
        "upstream unavailable",
    ];
    NEEDLES.iter().any(|n| m.contains(n))
}

/// Retry decision for one failed HTTP attempt.
///
/// `opencode_route` widens it to OpenCode's 4xx-wrapped upstream failures; for
/// every other provider the decision is exactly `is_retryable_status`.
fn is_retryable_error(status: u16, message: &str, opencode_route: bool) -> bool {
    ApiClient::is_retryable_status(status)
        || (opencode_route
            && matches!(status, 400 | 408 | 409 | 502 | 503 | 504)
            && is_transient_upstream_message(message))
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
            // `{"error": "…"}` with a bare string rather than an object —
            // Poolside answers a rejected key this way. Without this the whole
            // JSON blob was printed as the message.
            .or_else(|| {
                v.get("error")
                    .and_then(|e| e.as_str())
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| s.to_string())
            })
        {
            return Some(msg);
        }
    }
    // RFC 7807 `application/problem+json` — `{type,title,status,detail}` with no
    // `error` or `message` key. Poolside serves this for Platform and
    // self-hosted deployments. Checked *after* the shapes above so no provider
    // that already parses keeps its message: this branch only ever replaces a
    // raw body dump.
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        let field = |k: &str| v.get(k).and_then(|x| x.as_str()).filter(|s| !s.is_empty());
        if let Some(msg) = match (field("title"), field("detail")) {
            (Some(title), Some(detail)) => Some(format!("{title}: {detail}")),
            (Some(one), None) | (None, Some(one)) => Some(one.to_string()),
            (None, None) => None,
        } {
            return Some(msg);
        }
    }
    // SSE error event: extract last data: line's message if present.
    if body_looks_like_sse(body) {
        let mut parser = super::sse::SseParser::new();
        let mut events = parser.push(body.as_bytes());
        events.extend(parser.finish());
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
    fn opencode_route_is_detected_by_provider_or_base_url() {
        let zen = ApiClient::for_provider("https://opencode.ai/zen/v1", "k", "opencode").unwrap();
        assert!(zen.is_opencode_route());
        let go = ApiClient::for_provider(crate::providers::OPENCODE_GO_BASE_URL, "k", "opencode")
            .unwrap();
        assert!(go.is_opencode_route());
        // A custom endpoint pointed at OpenCode still counts…
        let custom =
            ApiClient::for_provider("https://opencode.ai/zen/go/v1", "k", "custom").unwrap();
        assert!(custom.is_opencode_route());
        // …and an unrelated provider never does.
        let other = ApiClient::for_provider("https://api.openai.com/v1", "k", "openai").unwrap();
        assert!(!other.is_opencode_route());
    }

    #[test]
    fn upstream_gateway_failures_are_retryable_only_on_the_opencode_route() {
        // The exact shape OpenCode Go returns when the vendor behind it fails.
        let msg = "Error from provider (Console Go): Upstream request failed";
        assert!(is_retryable_error(400, msg, true));
        assert!(
            !is_retryable_error(400, msg, false),
            "must not widen 400 retries for other providers"
        );
        // A genuine bad request never retries, on any route.
        for route in [true, false] {
            assert!(!is_retryable_error(
                400,
                "invalid_request_error: unknown model",
                route
            ));
            assert!(!is_retryable_error(404, "not found", route));
            assert!(!is_retryable_error(401, "invalid api key", route));
        }
        // Status-based retries are identical on both routes.
        for status in [429u16, 500, 502, 503, 504] {
            assert!(is_retryable_error(status, "whatever", false));
            assert!(is_retryable_error(status, "whatever", true));
        }
    }

    #[test]
    fn transient_upstream_message_matching_is_case_insensitive_and_narrow() {
        assert!(is_transient_upstream_message(
            "ERROR FROM PROVIDER (Console Go): UPSTREAM REQUEST FAILED"
        ));
        assert!(is_transient_upstream_message("upstream timeout"));
        assert!(!is_transient_upstream_message(
            "messages: text content blocks must be non-empty"
        ));
        assert!(!is_transient_upstream_message("model not found"));
    }

    #[test]
    fn text_only_capability_is_scoped_to_the_actual_base_url() {
        let provider = "custom-endpoint-test";
        let model = "same-model";
        let text_only_url = "https://text-only.example.test/v1/";
        let vision_url = "https://vision.example.test/v1";

        mark_text_only(provider, text_only_url, model);

        assert!(endpoint_is_text_only(
            provider,
            "https://text-only.example.test/v1",
            model
        ));
        assert!(!endpoint_is_text_only(provider, vision_url, model));
    }

    /// The shape Poolside's inference endpoint actually returns for a rejected
    /// key: `error` is a bare string, not the usual `{message: …}` object.
    #[test]
    fn string_valued_error_fields_are_unwrapped() {
        assert_eq!(
            parse_error_message(r#"{"error":"please check the api-key you provided"}"#).as_deref(),
            Some("please check the api-key you provided")
        );
        // The object form still takes precedence where both could apply.
        assert_eq!(
            parse_error_message(r#"{"error":{"message":"structured"}}"#).as_deref(),
            Some("structured")
        );
        // An empty string is not a message.
        assert_eq!(parse_error_message(r#"{"error":"  "}"#), None);
    }

    /// Poolside documents RFC 7807 `problem+json` for the Platform and
    /// self-hosted deployments — neither an `error` object nor a `message` key,
    /// so before this it surfaced as a raw JSON blob in the error line.
    #[test]
    fn problem_json_errors_are_readable() {
        assert_eq!(
            parse_error_message(
                r#"{"type":"about:blank","title":"Forbidden","status":403,"detail":"API key is not valid"}"#
            )
            .as_deref(),
            Some("Forbidden: API key is not valid")
        );
        // Either field alone is enough.
        assert_eq!(
            parse_error_message(r#"{"title":"Too Many Requests","status":429}"#).as_deref(),
            Some("Too Many Requests")
        );
        assert_eq!(
            parse_error_message(r#"{"detail":"model not found","status":404}"#).as_deref(),
            Some("model not found")
        );
        // Empty strings are not a message.
        assert_eq!(parse_error_message(r#"{"title":"","detail":""}"#), None);
    }

    /// The problem+json branch is a fallback: every shape that already parsed
    /// must keep parsing exactly as before, even when `title`/`detail` are also
    /// present.
    #[test]
    fn existing_error_shapes_still_win_over_the_problem_json_fallback() {
        assert_eq!(
            parse_error_message(
                r#"{"error":{"message":"rate limit"},"title":"Too Many Requests","detail":"slow down"}"#
            )
            .as_deref(),
            Some("rate limit")
        );
        assert_eq!(
            parse_error_message(r#"{"message":"bad request","title":"Bad Request"}"#).as_deref(),
            Some("bad request")
        );
        // Unparseable bodies are still unparseable (caller falls back to raw).
        assert_eq!(parse_error_message("<html>502</html>"), None);
        assert_eq!(parse_error_message(r#"{"status":500}"#), None);
    }

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
        let resp = consume_sse_text(body, &mut |_ev| {}).expect("parse codex-shaped sse");
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
        assert_eq!(
            calls.len(),
            1,
            "function_call must not be dropped: {resp:?}"
        );
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
        assert_eq!(request.headers().get("X-OpenAI-Fedramp").unwrap(), "true");
        assert_eq!(
            request.headers().get("Authorization").unwrap(),
            "Bearer oauth-token"
        );
        assert_eq!(
            request
                .headers()
                .get("originator")
                .and_then(|v| v.to_str().ok()),
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
            provider_id: "google".to_string(),
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
        let mut client =
            ApiClient::new(crate::providers::XAI_OAUTH_BASE_URL, "oauth-token").unwrap();
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

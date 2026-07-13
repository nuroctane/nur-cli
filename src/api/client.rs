use super::types::{ApiResponse, ResponseRequest};
use crate::error::{MuseError, Result};
use futures_util::StreamExt;
use reqwest::Client;

#[derive(Clone)]
pub struct MetaClient {
    http: Client,
    base_url: String,
    api_key: String,
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

impl MetaClient {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Result<Self> {
        let http = Client::builder()
            .user_agent(format!("meta-cli/{}", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(300))
            .build()?;
        Ok(Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
        })
    }

    fn is_retryable_status(status: u16) -> bool {
        matches!(status, 429 | 500 | 502 | 503 | 504)
    }

    pub async fn create_response(&self, req: &ResponseRequest) -> Result<ApiResponse> {
        let url = format!("{}/responses", self.base_url);
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            let res = match self
                .http
                .post(&url)
                .bearer_auth(&self.api_key)
                .header("Content-Type", "application/json")
                .json(req)
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
        let url = format!("{}/responses", self.base_url);
        let mut attempt = 0u32;
        let mut last_err: Option<MuseError> = None;

        loop {
            attempt += 1;
            let res = match self
                .http
                .post(&url)
                .bearer_auth(&self.api_key)
                .header("Content-Type", "application/json")
                .header("Accept", "text/event-stream")
                .json(req)
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

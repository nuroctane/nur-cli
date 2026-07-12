use super::types::{ApiResponse, ResponseRequest};
use crate::error::{MuseError, Result};
use reqwest::Client;

#[derive(Clone)]
pub struct MetaClient {
    http: Client,
    base_url: String,
    api_key: String,
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

    pub async fn create_response(&self, req: &ResponseRequest) -> Result<ApiResponse> {
        let url = format!("{}/responses", self.base_url);
        let res = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .header("Content-Type", "application/json")
            .json(req)
            .send()
            .await?;

        let status = res.status();
        let body = res.text().await?;

        if !status.is_success() {
            let msg = parse_error_message(&body).unwrap_or(body.clone());
            return Err(MuseError::Api {
                status: status.as_u16(),
                message: msg,
            });
        }

        let parsed: ApiResponse = serde_json::from_str(&body).map_err(|e| {
            MuseError::Other(format!("failed to parse API response: {e}; body={body}"))
        })?;

        if let Some(err) = &parsed.error {
            return Err(MuseError::Api {
                status: status.as_u16(),
                message: err
                    .message
                    .clone()
                    .unwrap_or_else(|| "unknown API error".into()),
            });
        }

        Ok(parsed)
    }
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

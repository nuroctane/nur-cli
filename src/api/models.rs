//! Live model discovery for the `/model` picker.
//!
//! Almost every provider answers `GET {base_url}/models` (OpenAI-shaped
//! `{ "data": [ { "id": … } ] }` or a provider-specific catalog URL).
//!
//! **Only models returned by the live call for the current credentials are
//! listed.** We do **not** merge hardcoded soft catalogs — those advertised
//! models the account/plan cannot use (and caused Sonnet 404s when retired
//! ids were padded into Anthropic's list). Works the same for API keys and
//! OAuth: auth headers are built from the key string the caller already
//! resolved via `auth::resolve_api_key_for`.

use serde::Deserialize;

#[derive(Deserialize)]
struct ModelList {
    data: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    #[serde(default)]
    id: String,
    /// GitHub Models catalog uses `id` or nested fields; keep a few aliases.
    #[serde(default, alias = "name")]
    name: String,
}

/// Fetch the provider's model ids for **this** api_key / OAuth token.
///
/// `base_url` is the catalog base (no trailing slash required).
/// `provider_id` enables provider-specific auth headers / URL rewrites only —
/// never invents models the host did not return.
pub fn fetch_model_ids(
    base_url: &str,
    api_key: &str,
    provider_id: Option<&str>,
) -> Result<Vec<String>, String> {
    let pid = provider_id.unwrap_or("");
    if api_key.trim().is_empty() {
        // key_optional local servers still answer /models without auth.
        let p = crate::providers::by_id(pid);
        if !p.map(|x| x.key_optional).unwrap_or(false) {
            return Err(
                "no credentials for this provider — /login (API key or browser) first, then /model"
                    .into(),
            );
        }
    }

    let urls = model_list_urls(base_url, pid);
    let mut last_err = String::from("no /models endpoint tried");

    for url in urls {
        match fetch_once(&url, api_key, pid) {
            Ok(ids) => {
                if ids.is_empty() {
                    last_err = "provider returned no models for this credential".into();
                    continue;
                }
                // Live list only — do not merge static catalogs.
                return Ok(ids);
            }
            Err(e) => last_err = e,
        }
    }

    Err(format!(
        "{last_err} · only live /models for your key or OAuth is shown — no offline catalog. \
         Type a model id with /model <id> if you know one."
    ))
}

fn model_list_urls(base_url: &str, provider_id: &str) -> Vec<String> {
    let base = base_url.trim_end_matches('/').to_string();
    let mut urls = Vec::new();
    match provider_id {
        "github-models" => {
            // Catalog is the official list; inference base may 404 on /models.
            urls.push("https://models.github.ai/catalog/models".into());
            urls.push(format!("{base}/models"));
            urls.push("https://models.github.ai/inference/models".into());
        }
        "anthropic" => {
            urls.push(format!("{base}/models"));
            // Some OAuth sessions expect the unversioned host path.
            if !base.contains("api.anthropic.com") {
                urls.push("https://api.anthropic.com/v1/models".into());
            }
        }
        _ => {
            urls.push(format!("{base}/models"));
        }
    }
    urls
}

fn fetch_once(url: &str, api_key: &str, provider_id: &str) -> Result<Vec<String>, String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("nur-cli/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("client error: {e}"))?;

    let mut req = client.get(url);
    if !api_key.is_empty() {
        match provider_id {
            "anthropic" => {
                // Console API keys → x-api-key. Claude OAuth (sk-ant-oat*) → Bearer + beta.
                req = req.header("anthropic-version", "2023-06-01");
                if crate::api::anthropic::is_oauth_token(api_key) {
                    req = req
                        .bearer_auth(api_key)
                        .header("anthropic-beta", crate::api::anthropic::OAUTH_BETA);
                } else {
                    req = req.header("x-api-key", api_key);
                }
            }
            "github-models" => {
                req = req
                    .bearer_auth(api_key)
                    .header("Accept", "application/vnd.github+json")
                    .header("X-GitHub-Api-Version", "2022-11-28");
            }
            _ => {
                req = req.bearer_auth(api_key);
            }
        }
    }
    req = req.header("Accept", "application/json");

    let res = req.send().map_err(|e| format!("request failed: {e}"))?;
    let status = res.status();
    let body = res.text().unwrap_or_default();
    if !status.is_success() {
        let snippet: String = body.trim().chars().take(160).collect();
        let mut msg = format!("HTTP {} · {}", status.as_u16(), snippet);
        if matches!(status.as_u16(), 400 | 401 | 403) {
            msg.push_str(
                " · tip: use this provider's /login (key or OAuth) — wrong host credentials hide the real plan list",
            );
        }
        return Err(msg);
    }

    let mut ids = parse_model_ids(&body)?;
    ids.retain(|id| !id.trim().is_empty());
    if ids.is_empty() {
        return Err("provider returned no models for this credential".to_string());
    }
    Ok(ids)
}

/// Parse a `/models` (or GitHub catalog) response body into sorted, de-duplicated ids.
pub fn parse_model_ids(body: &str) -> std::result::Result<Vec<String>, String> {
    // 1) OpenAI `{ "data": [ { "id": … } ] }`
    if let Ok(list) = serde_json::from_str::<ModelList>(body) {
        let mut ids: Vec<String> = list
            .data
            .into_iter()
            .map(|m| {
                if !m.id.trim().is_empty() {
                    m.id
                } else {
                    m.name
                }
            })
            .filter(|s| !s.trim().is_empty())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        return Ok(ids);
    }
    // 2) Bare array of `{id}` / `{name}`
    if let Ok(arr) = serde_json::from_str::<Vec<ModelEntry>>(body) {
        let mut ids: Vec<String> = arr
            .into_iter()
            .map(|m| {
                if !m.id.trim().is_empty() {
                    m.id
                } else {
                    m.name
                }
            })
            .filter(|s| !s.trim().is_empty())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        return Ok(ids);
    }
    // 3) GitHub catalog: top-level array with richer objects (`id` field)
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(arr) = v.as_array() {
            let mut ids = Vec::new();
            for item in arr {
                if let Some(id) = item.get("id").and_then(|x| x.as_str()) {
                    if !id.trim().is_empty() {
                        ids.push(id.to_string());
                    }
                } else if let Some(id) = item.get("name").and_then(|x| x.as_str()) {
                    if !id.trim().is_empty() {
                        ids.push(id.to_string());
                    }
                }
            }
            if !ids.is_empty() {
                ids.sort_unstable();
                ids.dedup();
                return Ok(ids);
            }
        }
        // 4) `{ "models": [ … ] }` wrapper some gateways use
        if let Some(arr) = v.get("models").and_then(|m| m.as_array()) {
            let mut ids = Vec::new();
            for item in arr {
                if let Some(id) = item.as_str() {
                    ids.push(id.to_string());
                } else if let Some(id) = item.get("id").and_then(|x| x.as_str()) {
                    ids.push(id.to_string());
                }
            }
            if !ids.is_empty() {
                ids.sort_unstable();
                ids.dedup();
                return Ok(ids);
            }
        }
    }
    Err("unexpected /models response shape".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_openai_data_shape() {
        let body = r#"{"object":"list","data":[{"id":"gpt-5.5"},{"id":"gpt-4o"}]}"#;
        assert_eq!(parse_model_ids(body).unwrap(), vec!["gpt-4o", "gpt-5.5"]);
    }

    #[test]
    fn parses_bare_array_fallback() {
        let body = r#"[{"id":"claude-sonnet-5"},{"id":"claude-opus-4-8"}]"#;
        assert_eq!(
            parse_model_ids(body).unwrap(),
            vec!["claude-opus-4-8", "claude-sonnet-5"]
        );
    }

    #[test]
    fn parses_github_catalog_style() {
        let body = r#"[{"id":"openai/gpt-4o","name":"GPT-4o"},{"id":"meta/llama-3"}]"#;
        assert_eq!(
            parse_model_ids(body).unwrap(),
            vec!["meta/llama-3", "openai/gpt-4o"]
        );
    }

    #[test]
    fn dedupes_and_drops_blanks() {
        let body = r#"{"data":[{"id":"a"},{"id":""},{"id":"a"},{"id":"b"}]}"#;
        assert_eq!(parse_model_ids(body).unwrap(), vec!["a", "b"]);
    }

    #[test]
    fn empty_key_errors_for_non_local_providers() {
        let err = fetch_model_ids("https://api.x.ai/v1", "", Some("xai")).unwrap_err();
        assert!(
            err.contains("no credentials") || err.contains("/login"),
            "got: {err}"
        );
    }

    #[test]
    fn no_soft_catalog_when_network_unreachable() {
        // Garbage host: must fail with live error, not invent a soft list.
        let err = fetch_model_ids(
            "https://127.0.0.1:1",
            "sk-test-not-real",
            Some("openai"),
        )
        .unwrap_err();
        assert!(
            !err.contains("grok-4"),
            "must not inject xAI soft catalog: {err}"
        );
        assert!(
            err.contains("live /models") || err.contains("request failed") || err.contains("HTTP"),
            "got: {err}"
        );
    }
}

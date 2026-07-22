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
    #[serde(default)]
    slug: String,
    /// GitHub Models catalog uses `id` or nested fields; keep a few aliases.
    #[serde(default, alias = "name")]
    name: String,
}

impl ModelEntry {
    fn into_id(self) -> String {
        if !self.id.trim().is_empty() {
            self.id
        } else if !self.slug.trim().is_empty() {
            self.slug
        } else {
            self.name
        }
    }
}

struct ModelFetchError {
    message: String,
    status: Option<u16>,
}

impl ModelFetchError {
    fn request(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status: None,
        }
    }
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

    let mut current_key = api_key.to_string();
    let mut oauth = crate::auth::oauth_request_context(pid, &current_key);
    let urls = model_list_urls(base_url, pid, oauth.is_some());
    let mut last_err = String::from("no /models endpoint tried");
    let mut oauth_refreshed = false;
    let mut opencode_ids = Vec::new();

    for url in urls {
        loop {
            match fetch_once(&url, &current_key, pid, oauth.as_ref()) {
                Ok(mut ids) => {
                    if ids.is_empty() {
                        last_err = "provider returned no models for this credential".into();
                        break;
                    }
                    if pid == "google" {
                        for id in &mut ids {
                            if let Some(stripped) = id.strip_prefix("models/") {
                                *id = stripped.to_string();
                            }
                        }
                        ids.sort_unstable();
                        ids.dedup();
                    }
                    // Live list only — do not merge static catalogs.
                    if pid == "opencode" {
                        // Go shares the OpenCode credential with Zen, but its
                        // requests require a different endpoint. Preserve that
                        // route in the picker id so selection can switch bases.
                        if url.starts_with(crate::providers::OPENCODE_GO_BASE_URL) {
                            opencode_ids
                                .extend(ids.into_iter().map(|id| format!("opencode-go/{id}")));
                        } else {
                            opencode_ids.extend(ids);
                        }
                        break;
                    }
                    return Ok(ids);
                }
                Err(error) if error.status == Some(401) && oauth.is_some() && !oauth_refreshed => {
                    oauth_refreshed = true;
                    if crate::auth::force_refresh_oauth(pid).unwrap_or(false) {
                        if let Ok(Some(token)) = crate::auth::resolve_oauth_access_token(pid) {
                            current_key = token;
                            oauth = crate::auth::oauth_request_context(pid, &current_key);
                            continue;
                        }
                    }
                    last_err = error.message;
                    break;
                }
                Err(error) => {
                    last_err = error.message;
                    break;
                }
            }
        }
    }

    if !opencode_ids.is_empty() {
        opencode_ids.sort_unstable();
        opencode_ids.dedup();
        return Ok(opencode_ids);
    }

    // Kimi OAuth often returns HTTP 402 on GET /models even when inference works
    // (plan gate). Fall back to the catalog default so /model is still usable.
    if pid == "kimi"
        && (last_err.contains("402")
            || last_err.to_ascii_lowercase().contains("payment")
            || last_err.to_ascii_lowercase().contains("quota"))
    {
        if let Some(p) = crate::providers::by_id("kimi") {
            return Ok(vec![
                p.default_model.to_string(),
                "kimi-for-coding".into(),
                "kimi-latest".into(),
            ]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect());
        }
    }

    Err(format!(
        "{last_err} · only live /models for your key or OAuth is shown — no offline catalog. \
         Type a model id with /model <id> if you know one."
    ))
}

/// For local inference servers (`ollama`, `lmstudio`, `llamacpp`, `vllm`, `jan`),
/// the shipped `local-model` placeholder provably 400s on real servers
/// (Group C observed `POST {"model":"local-model"}` → 400 on a live llama.cpp
/// instance, while a real id from `GET /v1/models` → 200). The fix is lazy
/// `/models` resolution, not a better placeholder.
///
/// This helper is the blocking, synchronous path used at config load / CLI
/// startup. For the async streaming path see `ApiClient::resolve_model_async`.
pub fn resolve_local_model_if_needed(
    base_url: &str,
    provider_id: &str,
    api_key: &str,
    model: &str,
) -> String {
    if !crate::providers::is_placeholder_local_model(model) {
        return model.to_string();
    }
    if !crate::providers::is_local_provider(provider_id) {
        return model.to_string();
    }
    // Attempt live list; on any failure keep the placeholder — the later
    // request will surface the real error and the user can `/model <id>`.
    if let Ok(ids) = fetch_model_ids(base_url, api_key, Some(provider_id)) {
        if let Some(first) = ids.into_iter().next() {
            if !first.trim().is_empty() {
                return first;
            }
        }
    }
    model.to_string()
}

fn model_list_urls(base_url: &str, provider_id: &str, is_oauth: bool) -> Vec<String> {
    let base = base_url.trim_end_matches('/').to_string();
    let mut urls = Vec::new();
    match provider_id {
        "openai" if is_oauth => {
            urls.push(format!(
                "{}/models?client_version={}",
                crate::providers::OPENAI_OAUTH_BASE_URL,
                openai_codex_client_version()
            ));
        }
        "xai" if is_oauth => {
            urls.push(format!("{}/models", crate::providers::XAI_OAUTH_BASE_URL));
        }
        "kimi" if is_oauth => {
            urls.push(format!("{}/models", crate::providers::KIMI_CODE_BASE_URL));
        }
        "google" if is_oauth => {
            // Google's OAuth quickstart documents this endpoint; the
            // OpenAI-compatible base does not consistently expose /models.
            urls.push("https://generativelanguage.googleapis.com/v1/models".into());
            urls.push(format!("{base}/models"));
        }
        "github-copilot" => {
            urls.push(format!("{base}/models"));
            urls.push("https://api.githubcopilot.com/models".into());
        }
        "github-models" => {
            // Catalog is the official list; inference base may 404 on /models.
            urls.push("https://models.github.ai/catalog/models".into());
            urls.push(format!("{base}/models"));
            urls.push("https://models.github.ai/inference/models".into());
        }
        "opencode" => {
            // Zen and Go are separate live catalogs under one OpenCode key.
            // Always ask *both*, anchored on the catalog's Zen base rather than
            // the configured one: after picking a Go model the active base_url
            // IS the Go endpoint, and using it here listed Go twice and hid Zen
            // entirely — plus the second, unprefixed copy of the Go ids routed
            // back to Zen on selection and 404'd.
            let zen = crate::providers::by_id("opencode")
                .map(|p| p.base_url)
                .unwrap_or("https://opencode.ai/zen/v1");
            urls.push(format!("{}/models", zen.trim_end_matches('/')));
            urls.push(format!("{}/models", crate::providers::OPENCODE_GO_BASE_URL));
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
    // Several arms hard-code a URL that the generic `{base}/models` form also
    // produces for the shipped base (both GitHub providers did exactly this),
    // and every duplicate costs a full request timeout on the failure path.
    // `Vec::dedup` only collapses *adjacent* repeats, so filter to first
    // occurrence instead — the probe order still matters.
    let mut seen = std::collections::HashSet::new();
    urls.retain(|u| seen.insert(u.clone()));
    urls
}

/// The ChatGPT model catalog gates entries by Codex compatibility version.
/// Nur's own semver is unrelated (and can hide every model), so prefer the
/// locally installed Codex metadata and keep a current protocol fallback.
fn openai_codex_client_version() -> String {
    if let Ok(value) = std::env::var("NUR_CODEX_CLIENT_VERSION") {
        let value = value.trim();
        if !value.is_empty() {
            return value.to_string();
        }
    }
    if let Some(home) = dirs::home_dir() {
        for (file, field) in [
            ("models_cache.json", "client_version"),
            ("version.json", "latest_version"),
        ] {
            let path = home.join(".codex").join(file);
            if let Ok(text) = std::fs::read_to_string(path) {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                    if let Some(version) = value.get(field).and_then(|value| value.as_str()) {
                        if !version.trim().is_empty() {
                            return version.to_string();
                        }
                    }
                }
            }
        }
    }
    "0.144.5".to_string()
}

fn fetch_once(
    url: &str,
    api_key: &str,
    provider_id: &str,
    oauth: Option<&crate::auth::OAuthRequestContext>,
) -> Result<Vec<String>, ModelFetchError> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("nur-cli/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| ModelFetchError::request(format!("client error: {e}")))?;

    let mut req = client.get(url);
    if !api_key.is_empty() {
        match provider_id {
            "anthropic" => {
                // Console API keys → x-api-key. Claude OAuth (sk-ant-oat*) → Bearer + beta.
                req = req.header("anthropic-version", "2023-06-01");
                if oauth.is_some() || crate::api::anthropic::is_oauth_token(api_key) {
                    req = req
                        .bearer_auth(api_key)
                        .header("anthropic-beta", crate::api::anthropic::OAUTH_BETAS)
                        .header("x-app", "cli")
                        .header(
                            "User-Agent",
                            format!("claude-cli/{}", env!("CARGO_PKG_VERSION")),
                        );
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
            "github-copilot" => {
                req = req
                    .bearer_auth(api_key)
                    .header("Editor-Version", "vscode/1.104.1")
                    .header("Editor-Plugin-Version", "copilot-chat/0.26.7")
                    .header("Copilot-Integration-Id", "vscode-chat")
                    .header("User-Agent", "GitHubCopilotChat/0.26.7");
            }
            "poolside" => {
                // Poolside documents `application/problem+json` for errors on the
                // Platform and self-hosted deployments; asking for it means a
                // failure comes back as a readable RFC 7807 body instead of a
                // bare status line.
                req = req
                    .bearer_auth(api_key)
                    .header("Accept", "application/json, application/problem+json");
            }
            "openai" if oauth.is_some() => {
                req = req.bearer_auth(api_key);
                if let Some(account_id) = oauth.and_then(|context| context.account_id.as_deref()) {
                    req = req.header("ChatGPT-Account-ID", account_id);
                }
                if oauth.is_some_and(|context| context.is_fedramp) {
                    req = req.header("X-OpenAI-Fedramp", "true");
                }
            }
            "google" if oauth.is_some() => {
                req = req.bearer_auth(api_key);
                if let Some(project_id) = oauth.and_then(|context| context.project_id.as_deref()) {
                    req = req.header("x-goog-user-project", project_id);
                }
            }
            "kimi" if oauth.is_some() => {
                req = req.bearer_auth(api_key);
                if let Ok(headers) = crate::oauth::kimi_request_headers() {
                    for (name, value) in headers {
                        req = req.header(name, value);
                    }
                }
            }
            "xai" if oauth.is_some() => {
                let ver = crate::providers::xai_grok_cli_version();
                req = req
                    .bearer_auth(api_key)
                    .header("x-grok-client-version", ver.as_str())
                    .header("X-XAI-Token-Auth", "xai-grok-cli")
                    .header("User-Agent", format!("xai-grok-workspace/{ver}"));
            }
            _ => {
                req = req.bearer_auth(api_key);
            }
        }
    }
    req = req.header("Accept", "application/json");

    let res = req
        .send()
        .map_err(|e| ModelFetchError::request(format!("request failed: {e}")))?;
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
        return Err(ModelFetchError {
            message: msg,
            status: Some(status.as_u16()),
        });
    }

    let mut ids = parse_model_ids(&body).map_err(ModelFetchError::request)?;
    ids.retain(|id| !id.trim().is_empty());
    if ids.is_empty() {
        return Err(ModelFetchError::request(
            "provider returned no models for this credential",
        ));
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
            .map(ModelEntry::into_id)
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
            .map(ModelEntry::into_id)
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
                if item
                    .get("supported_in_api")
                    .and_then(|value| value.as_bool())
                    == Some(false)
                    || item.get("hidden").and_then(|value| value.as_bool()) == Some(true)
                    || item.get("visibility").and_then(|value| value.as_str()) == Some("hide")
                    || item
                        .get("supportedGenerationMethods")
                        .and_then(|value| value.as_array())
                        .is_some_and(|methods| {
                            !methods
                                .iter()
                                .any(|method| method.as_str() == Some("generateContent"))
                        })
                {
                    continue;
                }
                if let Some(id) = item.as_str() {
                    ids.push(id.to_string());
                } else if let Some(id) = item.get("id").and_then(|x| x.as_str()) {
                    ids.push(id.to_string());
                } else if let Some(id) = item.get("slug").and_then(|x| x.as_str()) {
                    ids.push(id.to_string());
                } else if let Some(id) = item.get("name").and_then(|x| x.as_str()) {
                    ids.push(id.to_string());
                }
            }
            if !ids.is_empty() {
                ids.sort_unstable();
                ids.dedup();
                return Ok(ids);
            }
        }
        // 5) Grok Build proxy/cache shape: `{ "models": { key: {info: …} } }`.
        if let Some(models) = v.get("models").and_then(|value| value.as_object()) {
            let mut ids = Vec::new();
            for (key, item) in models {
                let info = item.get("info").unwrap_or(item);
                if info
                    .get("supported_in_api")
                    .and_then(|value| value.as_bool())
                    == Some(false)
                    || info.get("hidden").and_then(|value| value.as_bool()) == Some(true)
                {
                    continue;
                }
                let id = info
                    .get("id")
                    .or_else(|| info.get("model"))
                    .or_else(|| info.get("slug"))
                    .and_then(|value| value.as_str())
                    .unwrap_or(key);
                if !id.trim().is_empty() {
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
    fn parses_openai_oauth_models_and_filters_unusable_entries() {
        let body = r#"{
            "models": [
                {"slug":"gpt-5.5","visibility":"list","supported_in_api":true},
                {"slug":"gpt-hidden","visibility":"hide","supported_in_api":true},
                {"slug":"gpt-web-only","visibility":"list","supported_in_api":false}
            ]
        }"#;
        assert_eq!(parse_model_ids(body).unwrap(), vec!["gpt-5.5"]);
    }

    #[test]
    fn parses_xai_oauth_model_map_and_filters_hidden_entries() {
        let body = r#"{
            "models": {
                "grok-4.5": {"info":{"id":"grok-4.5","supported_in_api":true}},
                "internal": {"info":{"id":"internal","hidden":true}}
            }
        }"#;
        assert_eq!(parse_model_ids(body).unwrap(), vec!["grok-4.5"]);
    }

    #[test]
    fn google_oauth_models_keep_only_generate_content_capability() {
        let body = r#"{
            "models": [
                {"name":"models/gemini-3-pro","supportedGenerationMethods":["generateContent"]},
                {"name":"models/text-embedding","supportedGenerationMethods":["embedContent"]}
            ]
        }"#;
        assert_eq!(parse_model_ids(body).unwrap(), vec!["models/gemini-3-pro"]);
    }

    #[test]
    fn openai_oauth_uses_chatgpt_model_endpoint_with_client_version() {
        let urls = model_list_urls("https://api.openai.com/v1", "openai", true);
        assert_eq!(urls.len(), 1);
        assert!(urls[0].starts_with(crate::providers::OPENAI_OAUTH_BASE_URL));
        assert!(urls[0].contains("client_version="));
        assert!(!urls[0].contains(&format!("client_version={}", env!("CARGO_PKG_VERSION"))));
    }

    #[test]
    fn kimi_oauth_uses_managed_coding_model_endpoint() {
        let urls = model_list_urls("https://example.invalid/v1", "kimi", true);
        assert_eq!(
            urls,
            vec![format!("{}/models", crate::providers::KIMI_CODE_BASE_URL)]
        );
    }

    #[test]
    fn opencode_merges_zen_and_go_catalog_endpoints() {
        let urls = model_list_urls("https://opencode.ai/zen/v1", "opencode", false);
        assert_eq!(
            urls,
            vec![
                "https://opencode.ai/zen/v1/models".into(),
                format!("{}/models", crate::providers::OPENCODE_GO_BASE_URL),
            ]
        );
    }

    /// Once a Go model is selected the active base_url *is* the Go endpoint.
    /// The picker must still show both catalogs, and must not list the Go ids a
    /// second time without their `opencode-go/` route prefix.
    #[test]
    fn opencode_lists_both_catalogs_even_when_already_on_the_go_route() {
        let urls = model_list_urls(crate::providers::OPENCODE_GO_BASE_URL, "opencode", false);
        assert_eq!(
            urls,
            vec![
                "https://opencode.ai/zen/v1/models".into(),
                format!("{}/models", crate::providers::OPENCODE_GO_BASE_URL),
            ]
        );
        // Exactly one request per catalog — no duplicate Go fetch.
        assert_eq!(
            urls.iter()
                .filter(|u| u.starts_with(crate::providers::OPENCODE_GO_BASE_URL))
                .count(),
            1
        );
    }

    /// A repeated URL burns a full request timeout on every failure path. Both
    /// GitHub providers hard-coded a URL that `{base}/models` already yields.
    #[test]
    fn no_provider_probes_the_same_catalog_url_twice() {
        for p in crate::providers::PROVIDERS {
            for oauth in [false, true] {
                let urls = model_list_urls(p.base_url, p.id, oauth);
                let mut seen = std::collections::HashSet::new();
                for u in &urls {
                    assert!(
                        seen.insert(u.clone()),
                        "{} (oauth={oauth}) probes {u} more than once",
                        p.id
                    );
                }
            }
        }
    }

    #[test]
    fn every_browser_provider_has_a_model_detection_endpoint() {
        for id in crate::providers::oauth_browser_provider_ids() {
            let provider = crate::providers::by_id(id).unwrap();
            let urls = model_list_urls(provider.base_url, id, true);
            assert!(!urls.is_empty(), "{id} has no OAuth model endpoint");
            assert!(
                urls.iter().all(|url| url.starts_with("https://")),
                "{id} has an insecure OAuth model endpoint: {urls:?}"
            );
        }
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
        let err =
            fetch_model_ids("https://127.0.0.1:1", "sk-test-not-real", Some("openai")).unwrap_err();
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

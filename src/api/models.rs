//! Live model discovery for the `/model` picker.
//!
//! Almost every provider in [`crate::providers`] is OpenAI-compatible and
//! answers `GET {base_url}/models` with `{ "data": [ { "id": … } ] }`. This
//! fetches that list (blocking — call from a background thread, same pattern as
//! the OAuth flow) so the picker can show what a provider actually offers
//! instead of making the user memorize model ids.
//!
//! Provider-specific quirks (Anthropic headers, GitHub catalog URL, Thinking
//! Machines curated catalog merge) live here so every API key / OAuth session
//! gets a full, usable list.

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

/// Known Thinking Machines / Tinker model ids (from their published catalog).
/// Live `/models` can be sparse or incomplete; we merge these so a TINKER key
/// can switch to any advertised model, not just the default Inkling.
const THINKING_MACHINES_CATALOG: &[&str] = &[
    "thinkingmachines/Inkling",
    "thinkingmachines/Inkling:peft:262144",
    "nvidia/NVIDIA-Nemotron-3-Ultra-550B-A55B-BF16",
    "nvidia/NVIDIA-Nemotron-3-Ultra-550B-A55B-BF16:peft:262144",
    "nvidia/NVIDIA-Nemotron-3-Super-120B-A12B-BF16",
    "nvidia/NVIDIA-Nemotron-3-Super-120B-A12B-BF16:peft:262144",
    "nvidia/NVIDIA-Nemotron-3-Nano-30B-A3B-BF16",
    "moonshotai/Kimi-K2.6",
    "moonshotai/Kimi-K2.6:peft:131072",
    "Qwen/Qwen3.6-35B-A3B",
    "Qwen/Qwen3.6-27B",
    "Qwen/Qwen3.5-397B-A17B",
    "Qwen/Qwen3.5-397B-A17B:peft:262144",
    "Qwen/Qwen3.5-35B-A3B-Base",
    "Qwen/Qwen3.5-9B",
    "Qwen/Qwen3.5-9B-Base",
    "Qwen/Qwen3.5-4B",
    "Qwen/Qwen3-8B",
    "openai/gpt-oss-120b",
    "openai/gpt-oss-120b:peft:131072",
    "openai/gpt-oss-20b",
    "deepseek-ai/DeepSeek-V3.1",
];

/// Fetch the provider's model ids.
///
/// `base_url` is the catalog base (no trailing slash required).
/// `provider_id` (optional) enables provider-specific auth headers / URL
/// rewrites and catalog merges.
pub fn fetch_model_ids(
    base_url: &str,
    api_key: &str,
    provider_id: Option<&str>,
) -> Result<Vec<String>, String> {
    let pid = provider_id.unwrap_or("");
    let urls = model_list_urls(base_url, pid);
    let mut last_err = String::from("no /models endpoint tried");

    for url in urls {
        match fetch_once(&url, api_key, pid) {
            Ok(mut ids) => {
                merge_catalog(pid, &mut ids);
                if ids.is_empty() {
                    last_err = "provider returned no models".into();
                    continue;
                }
                return Ok(ids);
            }
            Err(e) => last_err = e,
        }
    }

    // Soft fallback: curated catalog when the live list is unreachable.
    if let Some(cat) = static_catalog(pid) {
        let mut ids: Vec<String> = cat.iter().map(|s| (*s).to_string()).collect();
        ids.sort_unstable();
        ids.dedup();
        return Ok(ids);
    }

    Err(last_err)
}

/// Back-compat wrapper used by older call sites.
pub fn fetch_model_ids_simple(base_url: &str, api_key: &str) -> Result<Vec<String>, String> {
    fetch_model_ids(base_url, api_key, None)
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
        req = req.bearer_auth(api_key);
        // Anthropic accepts bearer OAuth tokens; also send version header.
        // Some legacy key flows still want x-api-key — try both when provider matches.
        if provider_id == "anthropic" {
            req = req
                .header("anthropic-version", "2023-06-01")
                .header("x-api-key", api_key);
        }
        if provider_id == "github-models" {
            req = req
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28");
        }
    }
    // Prefer JSON; some hosts misbehave without Accept.
    req = req.header("Accept", "application/json");

    let res = req.send().map_err(|e| format!("request failed: {e}"))?;
    let status = res.status();
    let body = res.text().unwrap_or_default();
    if !status.is_success() {
        let snippet: String = body.trim().chars().take(160).collect();
        let mut msg = format!("HTTP {} · {}", status.as_u16(), snippet);
        // Multi-provider footgun: wrong credential type for this host.
        if matches!(status.as_u16(), 400 | 401 | 403) {
            msg.push_str(
                " · tip: use this provider's /login or its env key (XAI_API_KEY, OPENAI_API_KEY, …) — a leftover MODEL_API_KEY must not be sent to other hosts",
            );
        }
        return Err(msg);
    }

    let mut ids = parse_model_ids(&body)?;
    ids.retain(|id| !id.trim().is_empty());
    if ids.is_empty() {
        return Err("provider returned no models".to_string());
    }
    Ok(ids)
}

/// Common xAI chat ids — soft fallback if live list fails (wrong key, outage).
const XAI_CATALOG: &[&str] = &[
    "grok-4",
    "grok-4.3",
    "grok-4.5",
    "grok-4.20-0309-reasoning",
    "grok-4.20-0309-non-reasoning",
    "grok-4.20-multi-agent-0309",
    "grok-build-0.1",
    "grok-code-fast-1",
    "grok-3",
    "grok-3-mini",
    "grok-2-1212",
    "grok-2-vision-1212",
];

fn static_catalog(provider_id: &str) -> Option<&'static [&'static str]> {
    match provider_id {
        "thinkingmachines" => Some(THINKING_MACHINES_CATALOG),
        "xai" => Some(XAI_CATALOG),
        _ => None,
    }
}

fn merge_catalog(provider_id: &str, ids: &mut Vec<String>) {
    if let Some(cat) = static_catalog(provider_id) {
        for m in cat {
            if !ids.iter().any(|x| x == m) {
                ids.push((*m).to_string());
            }
        }
        ids.sort_unstable();
        ids.dedup();
    }
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
        let body = r#"{"data":[{"id":"a"},{"id":"a"},{"id":" "}]}"#;
        assert_eq!(parse_model_ids(body).unwrap(), vec!["a"]);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_model_ids("not json").is_err());
    }

    #[test]
    fn thinking_machines_catalog_has_more_than_inkling() {
        let cat = static_catalog("thinkingmachines").unwrap();
        assert!(cat.len() > 5);
        assert!(cat.iter().any(|m| m.contains("Inkling")));
        assert!(cat.iter().any(|m| m.contains("Kimi") || m.contains("Qwen")));
    }

    #[test]
    fn merge_catalog_adds_missing() {
        let mut ids = vec!["thinkingmachines/Inkling".to_string()];
        merge_catalog("thinkingmachines", &mut ids);
        assert!(ids.len() > 1);
        assert!(ids.iter().any(|m| m.contains("gpt-oss")));
    }
}

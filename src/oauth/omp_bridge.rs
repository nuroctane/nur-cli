//! Universal Oh My Pi (OMP) credential bridge.
//!
//! Upstream: https://github.com/open-horizon-labs/oh-omp · https://omp.sh
//!
//! OMP stores API keys and OAuth sessions in `~/.omp/agent/agent.db`. The
//! supported export surface is `omp token <provider>` (see `omp token --help`).
//! Nur uses that CLI as a last-resort credential source for **every** catalog
//! provider so a login in OMP is enough to drive nur / failover / subagents
//! without re-pasting keys.
//!
//! Callers must try nur-saved credentials (auth.json / provider keys / sessions /
//! env) **before** calling into this bridge. OMP never outranks an explicit
//! `/login` or pasted key.
//!
//! Resolution order inside the bridge for a nur provider id:
//! 1. Mapped OMP provider aliases (e.g. `openai` → `openai-codex`, `openai`)
//! 2. Bare nur id (when ids already match)
//!
//! Results are cached briefly so failover / health probes do not spawn dozens
//! of `omp token` processes per turn.

use super::OAuthTokens;
use crate::auth::OauthMeta;
use crate::error::Result;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(90);
const TOKEN_TIMEOUT_MS: u64 = 20_000;

#[derive(Clone)]
enum CacheEntry {
    Hit(OAuthTokens),
    Miss,
}

fn cache() -> &'static Mutex<HashMap<String, (Instant, CacheEntry)>> {
    static CACHE: OnceLock<Mutex<HashMap<String, (Instant, CacheEntry)>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Map a nur catalog provider id to candidate OMP provider ids (tried in order).
pub fn omp_provider_aliases(nur_provider: &str) -> Vec<&'static str> {
    match nur_provider {
        "openai" | "openai-cc" => vec!["openai-codex", "openai"],
        "anthropic" => vec!["anthropic"],
        "google" | "google-oauth" => vec!["google-gemini-cli", "google", "google-generative-ai"],
        "antigravity" => vec!["google-gemini-cli", "google", "antigravity"],
        "xai" => vec!["xai", "grok"],
        "github-copilot" => vec!["github-copilot", "github"],
        "github-models" => vec!["github-models", "github"],
        "kimi" => vec!["kimi", "moonshot", "kimi-coding"],
        "moonshot" => vec!["moonshot", "kimi"],
        "azure" => vec!["azure-openai-responses", "azure", "azure-openai"],
        "meta" => vec!["meta"],
        "groq" => vec!["groq"],
        "cerebras" => vec!["cerebras"],
        "openrouter" => vec!["openrouter"],
        "huggingface" => vec!["huggingface", "hf"],
        "mistral" => vec!["mistral"],
        "deepseek" => vec!["deepseek"],
        "zhipu" => vec!["zhipu", "zai"],
        "qwen" => vec!["qwen", "alibaba"],
        "minimax" => vec!["minimax"],
        "together" => vec!["together"],
        "fireworks" => vec!["fireworks"],
        "cohere" => vec!["cohere"],
        "vercel" => vec!["vercel", "vercel-ai-gateway"],
        "opencode" => vec!["opencode", "opencode-go"],
        "cursor" => vec!["cursor"],
        "ollama" => vec!["ollama"],
        "lmstudio" => vec!["lm-studio", "lmstudio"],
        "llamacpp" => vec!["llama.cpp", "llamacpp"],
        "perplexity" => vec!["perplexity"],
        "ai21" => vec!["ai21"],
        "novita" => vec!["novita"],
        "deepinfra" => vec!["deepinfra"],
        "hyperbolic" => vec!["hyperbolic"],
        "nebius" => vec!["nebius"],
        "sambanova" => vec!["sambanova"],
        "nvidia" => vec!["nvidia"],
        "baseten" => vec!["baseten"],
        "friendli" => vec!["friendli"],
        "chutes" => vec!["chutes"],
        "venice" => vec!["venice"],
        "stepfun" => vec!["stepfun"],
        "baichuan" => vec!["baichuan"],
        "requesty" => vec!["requesty"],
        "glama" => vec!["glama"],
        "portkey" => vec!["portkey"],
        "litellm" => vec!["litellm"],
        "cloudflare" => vec!["cloudflare"],
        "featherless" => vec!["featherless"],
        "nano-gpt" => vec!["nano-gpt", "nanogpt"],
        "helicone" => vec!["helicone"],
        "aimlapi" => vec!["aimlapi"],
        "bedrock" => vec!["bedrock", "amazon-bedrock"],
        "reka" => vec!["reka"],
        "inception" => vec!["inception"],
        "writer" => vec!["writer"],
        "upstage" => vec!["upstage"],
        "thinkingmachines" => vec!["thinkingmachines"],
        "poolside" => vec!["poolside"],
        "vllm" => vec!["vllm"],
        "jan" => vec!["jan"],
        other => {
            // Leak-free: for unknown ids we only try the id itself via a
            // static empty list + dynamic path in import.
            let _ = other;
            vec![]
        }
    }
}

fn candidate_ids(nur_provider: &str) -> Vec<String> {
    let mut out: Vec<String> = omp_provider_aliases(nur_provider)
        .into_iter()
        .map(|s| s.to_string())
        .collect();
    let bare = nur_provider.trim();
    if !bare.is_empty() && !out.iter().any(|id| id == bare) {
        out.push(bare.to_string());
    }
    out
}

fn looks_like_secret(s: &str) -> bool {
    let t = s.trim();
    if t.len() < 16 || t.contains('\n') || t.contains(' ') {
        return false;
    }
    // Reject help/error chatter that sometimes lands on stdout.
    let lower = t.to_ascii_lowercase();
    if lower.starts_with("usage")
        || lower.starts_with("error")
        || lower.starts_with("no ")
        || lower.starts_with('$')
        || lower.contains("not found")
        || lower.contains("no oauth")
        || lower.contains("no api key")
        || lower.contains("not authenticated")
        || lower.contains("provider")
    {
        return false;
    }
    true
}

fn parse_token_output(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Prefer the last non-empty line (CLI may print status above the token).
    let line = trimmed
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .next_back()
        .unwrap_or(trimmed);
    // Nested JSON credential blobs (e.g. github-copilot --raw) — dig for a token.
    if line.starts_with('{') {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            for key in [
                "access_token",
                "accessToken",
                "api_key",
                "apiKey",
                "token",
                "key",
            ] {
                if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
                    if looks_like_secret(s) {
                        return Some(s.trim().to_string());
                    }
                }
            }
        }
    }
    looks_like_secret(line).then(|| line.to_string())
}

fn omp_token_once(bin: &str, omp_provider: &str) -> Option<String> {
    let output =
        crate::ecosystem::run_capture(bin, &["token", omp_provider], None, TOKEN_TIMEOUT_MS)
            .ok()?;
    parse_token_output(&output)
}

/// True when tokens were produced by [`import_omp_token`] (issuer / refresh marker).
pub fn is_omp_import(tokens: &OAuthTokens) -> bool {
    tokens
        .meta
        .as_ref()
        .is_some_and(|m| m.issuer.eq_ignore_ascii_case("omp"))
        || tokens
            .refresh_token
            .as_deref()
            .is_some_and(|r| r.starts_with("omp:"))
}

/// Import a credential from OMP for a nur provider id.
///
/// Returns `Ok(None)` when omp is missing or has no stored credential.
/// Never panics; never logs the secret.
///
/// Does **not** read nur's credential stores - callers (`resolve_api_key_for`,
/// `resolve_target_key`) must prefer saved keys first.
pub fn import_omp_token(nur_provider: &str) -> Result<Option<OAuthTokens>> {
    let nur_provider = nur_provider.trim();
    if nur_provider.is_empty() {
        return Ok(None);
    }

    if let Ok(cache) = cache().lock() {
        if let Some((at, entry)) = cache.get(nur_provider) {
            if at.elapsed() < CACHE_TTL {
                return Ok(match entry {
                    CacheEntry::Hit(t) => Some(t.clone()),
                    CacheEntry::Miss => None,
                });
            }
        }
    }

    let Some(bin) = crate::ecosystem::find_omp() else {
        remember(nur_provider, CacheEntry::Miss);
        return Ok(None);
    };

    for omp_id in candidate_ids(nur_provider) {
        if let Some(access) = omp_token_once(&bin, &omp_id) {
            let tokens = OAuthTokens {
                access_token: access,
                refresh_token: Some(format!("omp:{omp_id}")),
                expires_at: None,
                meta: Some(OauthMeta {
                    issuer: "omp".into(),
                    client_id: "omp-token".into(),
                    extra: serde_json::json!({
                        "imported_from": "omp-token",
                        "omp_provider": omp_id,
                        "nur_provider": nur_provider,
                    }),
                }),
            };
            remember(nur_provider, CacheEntry::Hit(tokens.clone()));
            return Ok(Some(tokens));
        }
    }

    remember(nur_provider, CacheEntry::Miss);
    Ok(None)
}

fn remember(nur_provider: &str, entry: CacheEntry) {
    if let Ok(mut cache) = cache().lock() {
        cache.insert(nur_provider.to_string(), (Instant::now(), entry));
    }
}

/// Invalidate the short-lived omp token cache (after `/login` or logout).
#[allow(dead_code)]
pub fn invalidate_omp_token_cache() {
    if let Ok(mut cache) = cache().lock() {
        cache.clear();
    }
}

/// Read OMP `modelRoles` (smol/slow/plan/…) when available — used to seed
/// nur economy / subagent cheap-model hints without inventing role names.
#[allow(dead_code)] // consumed by omp economy routing / future role sync
pub fn omp_model_role(role: &str) -> Option<String> {
    let bin = crate::ecosystem::find_omp()?;
    let output = crate::ecosystem::run_capture(
        &bin,
        &["config", "get", "modelRoles", "--json"],
        None,
        15_000,
    )
    .ok()?;
    let value: serde_json::Value = serde_json::from_str(&output).ok()?;
    let roles = value.get("value").unwrap_or(&value);
    roles
        .get(role)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_maps_to_codex_first() {
        let aliases = omp_provider_aliases("openai");
        assert_eq!(aliases.first().copied(), Some("openai-codex"));
        assert!(aliases.contains(&"openai"));
    }

    #[test]
    fn google_includes_gemini_cli() {
        assert!(omp_provider_aliases("google").contains(&"google-gemini-cli"));
        assert!(omp_provider_aliases("antigravity").contains(&"google-gemini-cli"));
    }

    #[test]
    fn candidate_ids_always_include_bare_id() {
        let ids = candidate_ids("groq");
        assert!(ids.iter().any(|id| id == "groq"));
    }

    #[test]
    fn parse_token_rejects_help_text() {
        assert!(parse_token_output("Usage\n  $ omp token PROVIDER").is_none());
        assert!(parse_token_output("No OAuth accounts found for provider \"x\".").is_none());
    }

    #[test]
    fn parse_token_accepts_jwt_and_json() {
        let jwt = "eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiIxIn0.signaturepaddingvaluehere";
        assert_eq!(parse_token_output(jwt).as_deref(), Some(jwt));
        let json = r#"{"access_token":"sk-ant-oat-abcdefghijklmnopqrstuvwxyz012345"}"#;
        assert_eq!(
            parse_token_output(json).as_deref(),
            Some("sk-ant-oat-abcdefghijklmnopqrstuvwxyz012345")
        );
    }

    #[test]
    fn parse_token_takes_last_line() {
        let out = "fetching…\nsk-test-abcdefghijklmnopqrstuvwxyz0123456789";
        assert_eq!(
            parse_token_output(out).as_deref(),
            Some("sk-test-abcdefghijklmnopqrstuvwxyz0123456789")
        );
    }

    #[test]
    fn detects_omp_import_markers() {
        let tokens = OAuthTokens {
            access_token: "sk-test-abcdefghijklmnopqrstuvwxyz0123456789".into(),
            refresh_token: Some("omp:anthropic".into()),
            expires_at: None,
            meta: Some(crate::auth::OauthMeta {
                issuer: "omp".into(),
                client_id: "omp-token".into(),
                extra: serde_json::json!({}),
            }),
        };
        assert!(is_omp_import(&tokens));
        let vendor = OAuthTokens {
            access_token: "sk-test-abcdefghijklmnopqrstuvwxyz0123456789".into(),
            refresh_token: Some("refresh-xyz".into()),
            expires_at: None,
            meta: None,
        };
        assert!(!is_omp_import(&vendor));
    }
}

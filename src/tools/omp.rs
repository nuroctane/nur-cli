//! Oh My Pi backend delegation - https://omp.sh and https://github.com/can1357/oh-my-pi
//!
//! OMP is a coding agent with LSP-wired edits, debugger support, AST rewrites,
//! and a broad provider catalog. Nur uses its headless one-shot entry point,
//! captures exact delegated usage, and resolves economy work onto an
//! authenticated low-cost model instead of assuming a `pi/smol` role exists.

use super::{arg_str, Tool, ToolContext};
use crate::ecosystem;
use crate::error::{MuseError, Result};
use crate::usage::TokenUsage;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT_SECS: u64 = 300;
const MIN_TIMEOUT_SECS: u64 = 30;
const MAX_TIMEOUT_SECS: u64 = 600;
const MAX_PROMPT_CHARS: usize = 20_000;
const FOCUSED_TOOLS: &str = "read,grep,glob,lsp,edit,write,bash";
const ROUTE_CACHE_TTL: Duration = Duration::from_secs(300);

pub struct OmpTool;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OmpAction {
    Run,
    Status,
    Version,
}

impl OmpAction {
    fn from_value(args: &Value) -> Self {
        match args.get("action").and_then(Value::as_str) {
            Some("status") => Self::Status,
            Some("version") => Self::Version,
            _ => Self::Run,
        }
    }

    fn is_read_only(self) -> bool {
        matches!(self, Self::Status | Self::Version)
    }
}

/// Only `status` and `version` are read-only. A run gives OMP write access to
/// the workspace and is approval-gated by Nur.
pub fn is_read_only_value(args: &Value) -> bool {
    OmpAction::from_value(args).is_read_only()
}

#[derive(Debug, Serialize, Deserialize)]
struct OmpRunEnvelope {
    backend: String,
    cost_mode: String,
    provider: Option<String>,
    model: Option<String>,
    output: String,
    usage: TokenUsage,
}

/// Extract delegated usage from a successful OMP tool result so the agent loop
/// can fold it into Nur's session budget, status, and usage display.
pub fn delegated_usage(result: &str) -> Option<TokenUsage> {
    let envelope: OmpRunEnvelope = serde_json::from_str(result).ok()?;
    (envelope.backend == "omp").then_some(envelope.usage)
}

impl Tool for OmpTool {
    fn name(&self) -> &str {
        "omp"
    }

    fn description(&self) -> &str {
        "Delegate a focused coding task to the Oh My Pi backend. Runs are write-class, \
         approval-gated, cancellation-aware, and included in Nur token/cost budgets. \
         cost_mode=economy (default) uses an authenticated configured smol role or the \
         cheapest suitable authenticated model, low thinking, and a focused tool set; \
         use balanced only when the task needs OMP's configured default model. \
         Strongest at LSP refactors, debugger-driven diagnosis, and AST rewrites. \
         action=run|status|version."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["run", "status", "version"],
                    "default": "run"
                },
                "prompt": {
                    "type": "string",
                    "maxLength": MAX_PROMPT_CHARS,
                    "description": "For run: a bounded task with scope and acceptance criteria"
                },
                "cost_mode": {
                    "type": "string",
                    "enum": ["economy", "balanced"],
                    "default": "economy",
                    "description": "economy resolves an authenticated low-cost model, low thinking, and focused tools"
                },
                "model": {
                    "type": "string",
                    "description": "Optional exact OMP model selector; overrides cost_mode model routing"
                },
                "thinking": {
                    "type": "string",
                    "enum": ["off", "minimal", "low", "medium", "high", "xhigh", "auto"],
                    "description": "Optional OMP thinking level; economy defaults to low"
                },
                "tool_profile": {
                    "type": "string",
                    "enum": ["focused", "full"],
                    "description": "Optional tool surface; economy defaults to focused"
                },
                "timeout_seconds": {
                    "type": "integer",
                    "minimum": MIN_TIMEOUT_SECS,
                    "maximum": MAX_TIMEOUT_SECS,
                    "default": DEFAULT_TIMEOUT_SECS
                }
            }
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let bin = ecosystem::find_bin("omp").ok_or_else(|| {
            MuseError::Tool(
                "omp CLI not found. Install Bun (bun.sh) then `nur ecosystem ensure`, \
                 or install directly: bun install -g @oh-my-pi/pi-coding-agent \
                 (Windows: irm https://omp.sh/install.ps1 | iex)"
                    .into(),
            )
        })?;

        match OmpAction::from_value(args) {
            OmpAction::Status => omp_status(&bin),
            OmpAction::Version => omp_version(&bin).map_err(MuseError::Tool),
            OmpAction::Run => run_omp(&bin, args, ctx),
        }
    }
}

#[derive(Debug, Serialize)]
struct OmpStatus {
    version: String,
    authenticated_providers: Vec<String>,
    model_roles: Value,
    economy_model: Option<String>,
    warnings: Vec<String>,
}

fn omp_version(bin: &str) -> std::result::Result<String, String> {
    ecosystem::run_capture(bin, &["--version"], None, 30_000).map(|version| {
        if version.starts_with("omp") {
            version
        } else {
            format!("omp {version}")
        }
    })
}

fn omp_status(bin: &str) -> Result<String> {
    let version = omp_version(bin).map_err(MuseError::Tool)?;
    let roles = read_model_roles(bin).unwrap_or_else(|_| Value::Object(Default::default()));
    let usage = read_usage(bin);
    let mut warnings = Vec::new();
    let authenticated_providers = match usage {
        Ok(ref usage) => authenticated_providers(usage),
        Err(ref error) => {
            warnings.push(format!("could not inspect OMP authentication: {error}"));
            Vec::new()
        }
    };
    let economy_model = match resolve_economy_model_uncached(bin) {
        Ok(model) => Some(model),
        Err(error) => {
            warnings.push(error);
            None
        }
    };
    let status = OmpStatus {
        version,
        authenticated_providers,
        model_roles: roles,
        economy_model,
        warnings,
    };
    serde_json::to_string_pretty(&status).map_err(|error| MuseError::Tool(error.to_string()))
}

fn read_model_roles(bin: &str) -> std::result::Result<Value, String> {
    let output = ecosystem::run_capture(
        bin,
        &["config", "get", "modelRoles", "--json"],
        None,
        30_000,
    )?;
    let value: Value = serde_json::from_str(&output)
        .map_err(|error| format!("invalid modelRoles JSON: {error}"))?;
    Ok(value.get("value").cloned().unwrap_or(value))
}

fn read_usage(bin: &str) -> std::result::Result<Value, String> {
    let output = ecosystem::run_capture(bin, &["usage", "--json", "--redact"], None, 45_000)?;
    serde_json::from_str(&output).map_err(|error| format!("invalid OMP usage JSON: {error}"))
}

fn authenticated_providers(usage: &Value) -> Vec<String> {
    let mut providers = BTreeSet::new();
    if let Some(reports) = usage.get("reports").and_then(Value::as_array) {
        for report in reports {
            let allowed = report
                .pointer("/metadata/allowed")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let exhausted = report
                .pointer("/metadata/limitReached")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let healthy = !matches!(
                report.get("status").and_then(Value::as_str),
                Some("error" | "disabled")
            );
            if allowed && !exhausted && healthy {
                if let Some(provider) = report.get("provider").and_then(Value::as_str) {
                    providers.insert(provider.to_string());
                }
            }
        }
    }
    if let Some(accounts) = usage.get("accountsWithoutUsage").and_then(Value::as_array) {
        for account in accounts {
            let provider = account
                .as_str()
                .or_else(|| account.get("provider").and_then(Value::as_str));
            if let Some(provider) = provider {
                providers.insert(provider.to_string());
            }
        }
    }
    providers.into_iter().collect()
}

#[derive(Debug)]
struct EconomyCandidate {
    selector: String,
    economy_named: bool,
    cost: f64,
}

fn economy_candidate(models: &Value) -> Option<EconomyCandidate> {
    let models = models.get("models").and_then(Value::as_array)?;
    models
        .iter()
        .filter(|model| {
            model
                .get("input")
                .and_then(Value::as_array)
                .is_none_or(|inputs| inputs.iter().any(|input| input.as_str() == Some("text")))
        })
        .filter(|model| {
            model
                .get("contextWindow")
                .and_then(Value::as_u64)
                .unwrap_or(32_000)
                >= 32_000
        })
        .filter_map(|model| {
            let selector = model.get("selector").and_then(Value::as_str)?.to_string();
            let label = format!(
                "{} {}",
                model.get("id").and_then(Value::as_str).unwrap_or_default(),
                model
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            )
            .to_ascii_lowercase();
            let economy_named = [
                "smol", "mini", "nano", "luna", "flash", "haiku", "lite", "small", "fast",
            ]
            .iter()
            .any(|needle| label.contains(needle));
            let input = model
                .pointer("/cost/input")
                .and_then(Value::as_f64)
                .unwrap_or(1_000_000.0);
            let output = model
                .pointer("/cost/output")
                .and_then(Value::as_f64)
                .unwrap_or(1_000_000.0);
            Some(EconomyCandidate {
                selector,
                economy_named,
                cost: input + output,
            })
        })
        .min_by(|left, right| {
            (!left.economy_named)
                .cmp(&(!right.economy_named))
                .then_with(|| left.cost.total_cmp(&right.cost))
                .then_with(|| left.selector.cmp(&right.selector))
        })
}

fn resolve_economy_model_uncached(bin: &str) -> std::result::Result<String, String> {
    for variable in ["OMP_SMOL_MODEL", "PI_SMOL_MODEL"] {
        if let Ok(model) = std::env::var(variable) {
            let model = model.trim();
            if !model.is_empty() {
                return Ok(model.to_string());
            }
        }
    }

    if let Ok(roles) = read_model_roles(bin) {
        if let Some(model) = roles.get("smol").and_then(Value::as_str) {
            let model = model.trim();
            if !model.is_empty() {
                return Ok(model.to_string());
            }
        }
    }

    let usage = read_usage(bin).map_err(|error| {
        format!(
            "OMP economy routing could not inspect authenticated accounts: {error}. \
             Run `omp /login` or `omp auth-broker login <provider>`, then retry."
        )
    })?;
    let providers = authenticated_providers(&usage);
    if providers.is_empty() {
        return Err(
            "OMP has no healthy authenticated provider for economy delegation. \
             Run `omp /login` or `omp auth-broker login <provider>`, or set \
             modelRoles.smol / OMP_SMOL_MODEL."
                .into(),
        );
    }

    let mut candidates = Vec::new();
    let mut failures = Vec::new();
    for provider in &providers {
        match ecosystem::run_capture(bin, &["models", provider, "--json"], None, 45_000) {
            Ok(output) => match serde_json::from_str::<Value>(&output) {
                Ok(models) => {
                    if let Some(candidate) = economy_candidate(&models) {
                        candidates.push(candidate);
                    }
                }
                Err(error) => failures.push(format!("{provider}: invalid model JSON ({error})")),
            },
            Err(error) => failures.push(format!("{provider}: {error}")),
        }
    }
    candidates
        .into_iter()
        .min_by(|left, right| {
            (!left.economy_named)
                .cmp(&(!right.economy_named))
                .then_with(|| left.cost.total_cmp(&right.cost))
                .then_with(|| left.selector.cmp(&right.selector))
        })
        .map(|candidate| candidate.selector)
        .ok_or_else(|| {
            let detail = if failures.is_empty() {
                "no text-capable model with at least a 32k context window was listed".into()
            } else {
                failures.join("; ")
            };
            format!(
                "OMP has authenticated providers ({}) but no usable economy model: {detail}",
                providers.join(", ")
            )
        })
}

fn economy_route_cache() -> &'static Mutex<Option<(Instant, String)>> {
    static CACHE: OnceLock<Mutex<Option<(Instant, String)>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn resolve_economy_model(bin: &str) -> std::result::Result<String, String> {
    if let Ok(cache) = economy_route_cache().lock() {
        if let Some((stored_at, model)) = cache.as_ref() {
            if stored_at.elapsed() < ROUTE_CACHE_TTL {
                return Ok(model.clone());
            }
        }
    }
    let model = resolve_economy_model_uncached(bin)?;
    if let Ok(mut cache) = economy_route_cache().lock() {
        *cache = Some((Instant::now(), model.clone()));
    }
    Ok(model)
}

fn run_omp(bin: &str, args: &Value, ctx: &ToolContext) -> Result<String> {
    let prompt = arg_str(args, "prompt")?;
    let prompt_chars = prompt.chars().count();
    if prompt.trim().is_empty() {
        return Err(MuseError::Tool("omp prompt cannot be empty".into()));
    }
    if prompt_chars > MAX_PROMPT_CHARS {
        return Err(MuseError::Tool(format!(
            "omp prompt is {prompt_chars} characters; keep delegated context under {MAX_PROMPT_CHARS}"
        )));
    }

    let cost_mode = args
        .get("cost_mode")
        .and_then(Value::as_str)
        .unwrap_or("economy");
    if !matches!(cost_mode, "economy" | "balanced") {
        return Err(MuseError::Tool(
            "omp cost_mode must be economy or balanced".into(),
        ));
    }

    let timeout_secs = args
        .get("timeout_seconds")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_TIMEOUT_SECS);
    if !(MIN_TIMEOUT_SECS..=MAX_TIMEOUT_SECS).contains(&timeout_secs) {
        return Err(MuseError::Tool(format!(
            "omp timeout_seconds must be {MIN_TIMEOUT_SECS}..={MAX_TIMEOUT_SECS}"
        )));
    }

    let handoff = format!(
        "{prompt}\n\nNur handoff contract: stay within the requested scope, avoid unrelated \
         work, verify the result, and return only a compact outcome with files changed and checks run."
    );
    let explicit_model = args
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|model| !model.is_empty());
    let economy_model = if cost_mode == "economy" && explicit_model.is_none() {
        Some(resolve_economy_model(bin).map_err(MuseError::Tool)?)
    } else {
        None
    };
    let argv = build_run_args(
        args,
        cost_mode,
        timeout_secs,
        handoff,
        economy_model.as_deref(),
    );
    let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
    let wrapper_timeout_ms = timeout_secs.saturating_add(15).saturating_mul(1_000);
    let output = ecosystem::run_capture_cancelled(
        bin,
        &refs,
        Some(&ctx.cwd),
        wrapper_timeout_ms,
        &ctx.cancel,
    )
    .map_err(MuseError::Tool)?;
    let envelope = parse_json_run(&output, cost_mode)?;
    serde_json::to_string_pretty(&envelope).map_err(|error| MuseError::Tool(error.to_string()))
}

fn build_run_args(
    args: &Value,
    cost_mode: &str,
    timeout_secs: u64,
    prompt: String,
    economy_model: Option<&str>,
) -> Vec<String> {
    let mut argv = vec![
        "--mode".into(),
        "json".into(),
        "--no-session".into(),
        "--no-title".into(),
        // Nur supplies the bounded handoff and owns skill activation. Ambient
        // OMP extensions/skills duplicated huge prompt catalogs and coupled a
        // focused delegation to unrelated local plugin state.
        "--no-extensions".into(),
        "--no-skills".into(),
        "--max-time".into(),
        timeout_secs.to_string(),
        "--approval-mode".into(),
        "yolo".into(),
    ];

    let explicit_model = args
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|model| !model.is_empty());
    if let Some(model) = explicit_model.or(economy_model) {
        argv.extend(["--model".into(), model.into()]);
    }

    let thinking = args
        .get("thinking")
        .and_then(Value::as_str)
        .or((cost_mode == "economy").then_some("low"));
    if let Some(thinking) = thinking {
        argv.extend(["--thinking".into(), thinking.into()]);
    }

    let tool_profile =
        args.get("tool_profile")
            .and_then(Value::as_str)
            .unwrap_or(if cost_mode == "economy" {
                "focused"
            } else {
                "full"
            });
    if tool_profile == "focused" {
        argv.extend(["--tools".into(), FOCUSED_TOOLS.into()]);
    }

    argv.extend(["-p".into(), prompt]);
    argv
}

fn parse_json_run(output: &str, cost_mode: &str) -> Result<OmpRunEnvelope> {
    let mut final_text = String::new();
    let mut provider = None;
    let mut model = None;
    let mut usage = TokenUsage::default();
    let mut saw_usage = false;
    let mut run_error = None;

    for line in output.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(error) = omp_event_error(&event) {
            run_error = Some(error);
        }
        if event.get("type").and_then(Value::as_str) != Some("message_end") {
            continue;
        }
        let Some(message) = event.get("message") else {
            continue;
        };
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }

        provider = message
            .get("provider")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or(provider);
        model = message
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or(model);

        if let Some(parsed) = message.get("usage").and_then(parse_usage) {
            usage.add(&parsed);
            saw_usage = true;
        }
        if let Some(text) = assistant_text(message) {
            final_text = text;
        }
    }

    if let Some(error) = run_error {
        let route = match (&provider, &model) {
            (Some(provider), Some(model)) => format!(" ({provider}/{model})"),
            (Some(provider), None) => format!(" ({provider})"),
            _ => String::new(),
        };
        let partial = if final_text.trim().is_empty() {
            String::new()
        } else {
            format!("\n\nPartial OMP output before failure:\n{final_text}")
        };
        return Err(MuseError::Tool(format!(
            "OMP run failed{route}: {error}{partial}"
        )));
    }

    if final_text.trim().is_empty() {
        let tail: String = output
            .chars()
            .rev()
            .take(1_500)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        return Err(MuseError::Tool(format!(
            "omp returned no final assistant message. Output tail:\n{tail}"
        )));
    }
    usage.cost_known = saw_usage;
    Ok(OmpRunEnvelope {
        backend: "omp".into(),
        cost_mode: cost_mode.into(),
        provider,
        model,
        output: final_text,
        usage,
    })
}

fn omp_event_error(event: &Value) -> Option<String> {
    let event_type = event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let message = event.get("message");
    let stop_reason = message
        .and_then(|message| message.get("stopReason"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let is_error = event_type == "error"
        || event_type.ends_with("_error")
        || stop_reason.eq_ignore_ascii_case("error");
    if !is_error {
        return None;
    }

    let detail = event
        .pointer("/error/message")
        .or_else(|| event.get("errorMessage"))
        .or_else(|| event.pointer("/message/error/message"))
        .or_else(|| event.pointer("/message/errorMessage"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|detail| !detail.is_empty())
        .map(str::to_string)
        .or_else(|| message.and_then(assistant_text))
        .unwrap_or_else(|| "OMP reported an unspecified backend error".into());
    let status = event
        .get("errorStatus")
        .or_else(|| event.pointer("/message/errorStatus"))
        .and_then(|status| {
            status
                .as_str()
                .map(str::to_string)
                .or_else(|| status.as_u64().map(|status| status.to_string()))
        });
    Some(match status {
        Some(status) => format!("{detail} (status {status})"),
        None => detail,
    })
}

fn parse_usage(value: &Value) -> Option<TokenUsage> {
    let input = value.get("input").and_then(Value::as_u64).unwrap_or(0);
    let output = value.get("output").and_then(Value::as_u64).unwrap_or(0);
    let cache_read = value.get("cacheRead").and_then(Value::as_u64).unwrap_or(0);
    let cache_write = value.get("cacheWrite").and_then(Value::as_u64).unwrap_or(0);
    let total = value
        .get("totalTokens")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| input + output + cache_read + cache_write);
    let cost = value
        .pointer("/cost/total")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    Some(TokenUsage {
        input_tokens: input + cache_read + cache_write,
        output_tokens: output,
        total_tokens: total,
        reasoning_tokens: value
            .get("reasoningTokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cached_tokens: cache_read,
        cost_usd: cost,
        cost_known: true,
    })
}

fn assistant_text(message: &Value) -> Option<String> {
    if let Some(text) = message.get("content").and_then(Value::as_str) {
        return (!text.trim().is_empty()).then(|| text.to_string());
    }
    let text = message
        .get("content")?
        .as_array()?
        .iter()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    (!text.trim().is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn economy_args_use_the_resolved_model_and_bounded_headless_mode() {
        let args = serde_json::json!({"action": "run", "prompt": "fix it"});
        let argv = build_run_args(
            &args,
            "economy",
            300,
            "fix it".into(),
            Some("openai-codex/gpt-mini"),
        );
        assert!(argv
            .windows(2)
            .any(|v| v == ["--model", "openai-codex/gpt-mini"]));
        assert!(argv.windows(2).any(|v| v == ["--thinking", "low"]));
        assert!(argv.windows(2).any(|v| v == ["--mode", "json"]));
        assert!(argv.windows(2).any(|v| v == ["--max-time", "300"]));
        assert!(argv.windows(2).any(|v| v == ["--tools", FOCUSED_TOOLS]));
        assert!(argv.contains(&"--no-session".to_string()));
        assert!(argv.contains(&"--no-extensions".to_string()));
        assert!(argv.contains(&"--no-skills".to_string()));
        assert!(argv.windows(2).any(|v| v == ["--approval-mode", "yolo"]));
    }

    #[test]
    fn explicit_model_wins_and_balanced_keeps_the_full_surface() {
        let args = serde_json::json!({"model": "openai/gpt-test", "thinking": "medium"});
        let argv = build_run_args(&args, "balanced", 120, "task".into(), Some("ignored/model"));
        assert!(argv.windows(2).any(|v| v == ["--model", "openai/gpt-test"]));
        assert!(argv.windows(2).any(|v| v == ["--thinking", "medium"]));
        assert!(!argv.contains(&"--tools".to_string()));
    }

    #[test]
    fn json_events_yield_compact_output_and_exact_usage() {
        let raw = r#"{"type":"message_end","message":{"role":"assistant","provider":"openai","model":"gpt-test","content":[{"type":"toolCall","name":"read"}],"usage":{"input":10,"output":2,"cacheRead":3,"cacheWrite":4,"totalTokens":19,"reasoningTokens":1,"cost":{"total":0.004}}}}
{"type":"message_end","message":{"role":"assistant","provider":"openai","model":"gpt-test","content":[{"type":"text","text":"done"}],"usage":{"input":5,"output":6,"cacheRead":1,"cacheWrite":0,"totalTokens":12,"cost":{"total":0.006}}}}"#;
        let parsed = parse_json_run(raw, "economy").unwrap();
        assert_eq!(parsed.output, "done");
        assert_eq!(parsed.provider.as_deref(), Some("openai"));
        assert_eq!(parsed.model.as_deref(), Some("gpt-test"));
        assert_eq!(parsed.usage.input_tokens, 23);
        assert_eq!(parsed.usage.output_tokens, 8);
        assert_eq!(parsed.usage.total_tokens, 31);
        assert_eq!(parsed.usage.cached_tokens, 4);
        assert_eq!(parsed.usage.reasoning_tokens, 1);
        assert!((parsed.usage.cost_usd - 0.01).abs() < f64::EPSILON);
        assert!(parsed.usage.cost_known);

        let encoded = serde_json::to_string(&parsed).unwrap();
        assert_eq!(delegated_usage(&encoded).unwrap().total_tokens, 31);
    }

    #[test]
    fn action_classification_is_typed_and_fail_closed() {
        assert!(is_read_only_value(&serde_json::json!({"action": "status"})));
        assert!(is_read_only_value(
            &serde_json::json!({"action": "version"})
        ));
        assert!(!is_read_only_value(&serde_json::json!({"action": "run"})));
    }

    #[test]
    fn authenticated_provider_detection_excludes_exhausted_accounts() {
        let usage = serde_json::json!({
            "reports": [
                {"provider": "openai-codex", "status": "ok",
                 "metadata": {"allowed": true, "limitReached": false}},
                {"provider": "anthropic", "status": "ok",
                 "metadata": {"allowed": true, "limitReached": true}},
                {"provider": "broken", "status": "error"}
            ],
            "accountsWithoutUsage": [
                {"provider": "google"},
                "openrouter"
            ]
        });
        assert_eq!(
            authenticated_providers(&usage),
            vec!["google", "openai-codex", "openrouter"]
        );
    }

    #[test]
    fn economy_model_selection_prefers_named_small_models_then_cost() {
        let models = serde_json::json!({"models": [
            {"selector":"p/huge","id":"huge","contextWindow":200000,
             "input":["text"],"cost":{"input":0.1,"output":0.1}},
            {"selector":"p/luna","id":"luna","contextWindow":200000,
             "input":["text"],"cost":{"input":2.0,"output":6.0}},
            {"selector":"p/mini","id":"mini","contextWindow":200000,
             "input":["text"],"cost":{"input":1.0,"output":3.0}},
            {"selector":"p/tiny-context-mini","id":"mini","contextWindow":16000,
             "input":["text"],"cost":{"input":0.0,"output":0.0}}
        ]});
        assert_eq!(economy_candidate(&models).unwrap().selector, "p/mini");
    }

    #[test]
    fn zero_exit_json_backend_errors_are_not_reported_as_success() {
        let raw = r#"{"type":"message_end","message":{"role":"assistant","provider":"anthropic","model":"claude-test","stopReason":"error","errorMessage":"invalid x-api-key","errorStatus":401,"content":[{"type":"text","text":"request failed"}]}}"#;
        let error = parse_json_run(raw, "balanced").unwrap_err().to_string();
        assert!(error.contains("OMP run failed (anthropic/claude-test)"));
        assert!(error.contains("invalid x-api-key"));
        assert!(error.contains("status 401"));
        assert!(error.contains("Partial OMP output"));
    }
}

//! Enclave (enclave.ai): autonomous security agents, reached natively over MCP.
//!
//! Enclave is not a chat model provider. Its developer surface is a remote MCP
//! server whose tools run and read security agent work: pentests, code
//! security review, findings, CVE monitoring. nur speaks the Streamable HTTP
//! transport itself ([`crate::mcp_http`]) and authenticates with the API key
//! Enclave issues under Settings > MCP (`enc_sk_...`), sent as a bearer token.
//!
//! Key order: `ENCLAVE_MCP_API_KEY` (the variable Enclave's own Codex setup
//! names), then `nur auth login --provider enclave`. Docs: `docs/enclave.md`.

use crate::config::EnclaveConfig;
use crate::error::{NurError, Result};
use crate::mcp_http::{Connection, McpError, ServerInfo};
use serde_json::Value;
use std::sync::Mutex;
use std::time::Duration;

pub const PROVIDER_ID: &str = "enclave";
pub const DEFAULT_URL: &str = "https://mcp.enclave.ai/";
pub const KEY_ENV: &str = "ENCLAVE_MCP_API_KEY";
/// Where Enclave issues MCP keys.
pub const KEYS_PAGE: &str = "https://app.enclave.ai/settings?section=mcp";

/// The key and where it came from. The environment wins, so a shell can
/// override a stored key.
pub fn api_key() -> Option<(String, &'static str)> {
    if let Some(key) = std::env::var(KEY_ENV)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    {
        return Some((key, "env ENCLAVE_MCP_API_KEY"));
    }
    crate::auth::load_provider_key(PROVIDER_ID)
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .map(|k| (k, "saved key"))
}

/// The configured endpoint. The bearer key never travels over plain http
/// except to a loopback host.
pub fn endpoint(cfg: &EnclaveConfig) -> Result<String> {
    let raw = match cfg.url.trim() {
        "" => DEFAULT_URL,
        url => url,
    };
    let url = reqwest::Url::parse(raw)
        .map_err(|e| NurError::Config(format!("[enclave] url `{raw}` is not a URL: {e}")))?;
    // `host_str` brackets IPv6 literals.
    let loopback = url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    match url.scheme() {
        "https" => Ok(url.into()),
        "http" if loopback => Ok(url.into()),
        _ => Err(NurError::Config(format!(
            "[enclave] url must use https (plain http only for localhost testing): `{raw}`"
        ))),
    }
}

fn missing_key() -> NurError {
    NurError::Tool(format!(
        "no Enclave key. Create one at {KEYS_PAGE} (MCP > Create Key, `enc_sk_...`), then run \
         `nur auth login --provider enclave` or set {KEY_ENV}."
    ))
}

fn explain(error: McpError, url: &str) -> NurError {
    NurError::Tool(match error {
        McpError::Unauthorized(detail) => format!(
            "Enclave rejected the key: {detail}. Keys expire after 90, 180 or 365 days; create \
             a new one at {KEYS_PAGE} and run `nur auth login --provider enclave` (or set \
             {KEY_ENV})."
        ),
        McpError::SessionExpired => {
            "Enclave ended the MCP session again right after it was reopened; retry shortly.".into()
        }
        McpError::Rpc { code, message } => format!("Enclave returned error {code}: {message}"),
        McpError::Transport(detail) => format!("Enclave MCP request to {url} failed: {detail}"),
    })
}

/// A session is reused across tool calls for the same endpoint and key.
struct Cached {
    url: String,
    key: String,
    connection: Connection,
}

static SESSION: Mutex<Option<Cached>> = Mutex::new(None);

/// Run `operation` on a session, reopening once if the server ended it. The
/// session is taken out of the cache for the duration, so concurrent callers
/// never wait on each other's network I/O.
fn with_session<T>(
    cfg: &EnclaveConfig,
    operation: impl Fn(&mut Connection) -> std::result::Result<T, McpError>,
) -> Result<(T, ServerInfo)> {
    let url = endpoint(cfg)?;
    let (key, _) = api_key().ok_or_else(missing_key)?;
    let timeout = Duration::from_secs(cfg.timeout_secs.max(5));
    let open = || Connection::open(&url, Some(&key), timeout).map_err(|e| explain(e, &url));
    let cached = SESSION
        .lock()
        .ok()
        .and_then(|mut slot| slot.take())
        .filter(|c| c.url == url && c.key == key);
    let mut connection = match cached {
        Some(cached) => cached.connection,
        None => open()?,
    };
    let mut outcome = operation(&mut connection);
    if matches!(outcome, Err(McpError::SessionExpired)) {
        connection = open()?;
        outcome = operation(&mut connection);
    }
    // A JSON-RPC error leaves the session healthy; anything else drops it.
    if matches!(outcome, Ok(_) | Err(McpError::Rpc { .. })) {
        let server = connection.server.clone();
        if let Ok(mut slot) = SESSION.lock() {
            *slot = Some(Cached {
                url: url.clone(),
                key,
                connection,
            });
        }
        return outcome
            .map(|value| (value, server))
            .map_err(|e| explain(e, &url));
    }
    Err(explain(
        outcome.err().unwrap_or(McpError::SessionExpired),
        &url,
    ))
}

/// Human/model-readable state: endpoint, key source and, with a key, a live
/// check of the server and its tools. Never fails; problems are reported.
pub fn status(cfg: &EnclaveConfig) -> String {
    // The card already says what failed; drop the error-kind prefix.
    let plain = |error: NurError| match error {
        NurError::Tool(message) | NurError::Config(message) => message,
        other => other.to_string(),
    };
    let url = match endpoint(cfg) {
        Ok(url) => url,
        Err(e) => return format!("enclave · misconfigured\n  {}", plain(e)),
    };
    let Some((key, source)) = api_key() else {
        return format!(
            "enclave · no key yet\n  endpoint   {url}\n  \
             1. create a key at {KEYS_PAGE} (MCP > Create Key)\n  \
             2. nur auth login --provider enclave   (or set {KEY_ENV})"
        );
    };
    let mut out = format!(
        "enclave · security agents over MCP\n  endpoint   {url}\n  key        {} ({source})",
        crate::auth::key_fingerprint(&key)
    );
    match with_session(cfg, |c| c.list_tools()) {
        Ok((tools, server)) => {
            out.push_str(&format!("\n  server     {}", server_line(&server)));
            let names: Vec<&str> = tools
                .iter()
                .filter_map(|t| t.get("name").and_then(Value::as_str))
                .collect();
            out.push_str(&format!(
                "\n  tools      {} ({})",
                names.len(),
                names.join(", ")
            ));
        }
        Err(e) => out.push_str(&format!("\n  connection {}", plain(e))),
    }
    out
}

/// The tool catalog, or one tool's full schema when `name` is given.
pub fn tools(cfg: &EnclaveConfig, name: Option<&str>) -> Result<String> {
    let (tools, server) = with_session(cfg, |c| c.list_tools())?;
    if let Some(name) = name.map(str::trim).filter(|n| !n.is_empty()) {
        let tool = tools
            .iter()
            .find(|t| t.get("name").and_then(Value::as_str) == Some(name))
            .ok_or_else(|| {
                NurError::Tool(format!(
                    "Enclave has no tool `{name}`; available: {}",
                    tool_names(&tools)
                ))
            })?;
        return Ok(describe_tool(tool));
    }
    let mut out = format!(
        "enclave · {} tool(s) · {}",
        tools.len(),
        server_line(&server)
    );
    if let Some(instructions) = &server.instructions {
        out.push_str("\n\nserver instructions:\n");
        out.push_str(&clip(instructions, 2_000));
    }
    out.push('\n');
    for tool in &tools {
        out.push('\n');
        out.push_str(&summarize_tool(tool));
    }
    out.push_str(
        "\n\nfull schema: enclave action=tools tool=<name> · run: enclave action=call \
         tool=<name> arguments={...}",
    );
    Ok(out)
}

/// Run one Enclave tool and render its result as text.
pub fn call(cfg: &EnclaveConfig, name: &str, arguments: Value) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(NurError::Tool(
            "enclave call needs `tool` (see action=tools)".into(),
        ));
    }
    let arguments = match arguments {
        Value::Null => Value::Object(Default::default()),
        Value::String(text) if text.trim().is_empty() => Value::Object(Default::default()),
        Value::String(text) => serde_json::from_str(&text).map_err(|e| {
            NurError::Tool(format!("enclave call: `arguments` is not valid JSON: {e}"))
        })?,
        other => other,
    };
    if !arguments.is_object() {
        return Err(NurError::Tool(
            "enclave call: `arguments` must be a JSON object".into(),
        ));
    }
    let (result, _) = with_session(cfg, |c| c.call_tool(name, arguments.clone()))?;
    render_result(name, &result)
}

fn server_line(server: &ServerInfo) -> String {
    let name = if server.name.is_empty() {
        "server"
    } else {
        server.name.as_str()
    };
    let version = if server.version.is_empty() {
        String::new()
    } else {
        format!(" {}", server.version)
    };
    format!("{name}{version} (MCP {})", server.protocol_version)
}

fn tool_names(tools: &[Value]) -> String {
    tools
        .iter()
        .filter_map(|t| t.get("name").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join(", ")
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max).collect();
    format!("{cut}...")
}

/// `name(param: type, optional?: type)  [hints]` plus a description line.
fn summarize_tool(tool: &Value) -> String {
    let name = tool.get("name").and_then(Value::as_str).unwrap_or("?");
    let schema = tool.get("inputSchema").unwrap_or(&Value::Null);
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|r| r.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let params: Vec<String> = schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|props| {
            props
                .iter()
                .map(|(param, spec)| {
                    let kind = match spec.get("type") {
                        Some(Value::String(t)) => t.clone(),
                        Some(Value::Array(ts)) => ts
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join("|"),
                        _ if spec.get("enum").is_some() => "enum".into(),
                        _ => "any".into(),
                    };
                    let optional = if required.contains(&param.as_str()) {
                        ""
                    } else {
                        "?"
                    };
                    format!("{param}{optional}: {kind}")
                })
                .collect()
        })
        .unwrap_or_default();
    let mut hints = Vec::new();
    let annotations = tool.get("annotations");
    let hint = |key: &str| {
        annotations
            .and_then(|a| a.get(key))
            .and_then(Value::as_bool)
    };
    if hint("readOnlyHint") == Some(true) {
        hints.push("read-only");
    }
    if hint("destructiveHint") == Some(true) {
        hints.push("destructive");
    }
    if hint("openWorldHint") == Some(true) {
        hints.push("open-world");
    }
    let hints = if hints.is_empty() {
        String::new()
    } else {
        format!("  [{}]", hints.join(", "))
    };
    let description = tool
        .get("description")
        .and_then(Value::as_str)
        .or_else(|| tool.get("title").and_then(Value::as_str))
        .unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut line = format!("- {name}({}){hints}", params.join(", "));
    if !description.is_empty() {
        line.push_str("\n    ");
        line.push_str(&clip(&description, 320));
    }
    line
}

fn describe_tool(tool: &Value) -> String {
    let name = tool.get("name").and_then(Value::as_str).unwrap_or("?");
    let mut out = format!("enclave tool `{name}`");
    if let Some(title) = tool.get("title").and_then(Value::as_str) {
        out.push_str(&format!(" · {title}"));
    }
    if let Some(description) = tool.get("description").and_then(Value::as_str) {
        out.push_str(&format!("\n\n{}", description.trim()));
    }
    if let Some(annotations) = tool.get("annotations") {
        out.push_str(&format!("\n\nannotations: {annotations}"));
    }
    let schema = tool.get("inputSchema").cloned().unwrap_or(Value::Null);
    out.push_str(&format!(
        "\n\ninput schema:\n{}",
        serde_json::to_string_pretty(&schema).unwrap_or_default()
    ));
    out
}

/// Text for the model. Binary content is described, not inlined; structured
/// content is used when the tool sent no text (servers usually mirror it).
fn render_result(name: &str, result: &Value) -> Result<String> {
    let mut parts: Vec<String> = Vec::new();
    for item in result
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let field = |key: &str| item.get(key).and_then(Value::as_str).unwrap_or("");
        match field("type") {
            "text" => parts.push(field("text").to_string()),
            kind @ ("image" | "audio") => parts.push(format!(
                "[{kind} {} · {} base64 chars, not shown]",
                field("mimeType"),
                field("data").len()
            )),
            "resource_link" => {
                parts.push(format!("[resource {}: {}]", field("name"), field("uri")))
            }
            "resource" => {
                let resource = item.get("resource").unwrap_or(&Value::Null);
                let uri = resource.get("uri").and_then(Value::as_str).unwrap_or("");
                match resource.get("text").and_then(Value::as_str) {
                    Some(text) => parts.push(format!("[resource {uri}]\n{text}")),
                    None => parts.push(format!("[resource {uri} · binary, not shown]")),
                }
            }
            _ => parts.push(item.to_string()),
        }
    }
    if parts.is_empty() {
        if let Some(structured) = result.get("structuredContent") {
            parts.push(serde_json::to_string_pretty(structured).unwrap_or_default());
        }
    }
    let text = parts.join("\n\n");
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        return Err(NurError::Tool(format!(
            "Enclave tool `{name}` failed: {}",
            if text.is_empty() { "no detail" } else { &text }
        )));
    }
    if text.trim().is_empty() {
        return Ok(format!("Enclave tool `{name}` returned no content."));
    }
    Ok(text)
}

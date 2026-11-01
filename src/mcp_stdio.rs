//! MCP over stdio: spawn a server child and speak newline-delimited JSON-RPC 2.0.
//!
//! `mcp_http.rs` covers remote servers; most MCP servers in the wild are local
//! processes (`npx -y @modelcontextprotocol/server-x`, `uvx ...`) that speak
//! one JSON object per line on stdin/stdout. This module spawns them lazily,
//! performs the `initialize` handshake, caches `tools/list`, and dispatches
//! `tools/call`.
//!
//! Nothing here is approval-gated on its own: the `mcp` tool classifies `call`
//! as mutating unless the server is configured `read_only = true`.

use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

pub const PROTOCOL_VERSION: &str = "2025-06-18";
/// A server that has not answered `initialize` after this long is broken.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
/// One request's reply. Long tool calls can legitimately take a while.
const CALL_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_TOOLS_PAGES: usize = 20;

/// `[mcp.servers.<name>]`.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct McpServerConfig {
    /// Executable to spawn (stdio transport).
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Extra environment for the child.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// `true` marks every tool on this server read-only for approval purposes
    /// (a filesystem read server, a docs server). Default false: calls are
    /// treated as mutating and go through the normal approval path.
    #[serde(default)]
    pub read_only: bool,
    /// Remote MCP endpoint (Streamable HTTP). When set, `command` is ignored
    /// and calls go over HTTP.
    #[serde(default)]
    pub url: String,
    /// Environment variable holding the bearer token for `url` servers (the
    /// token an OAuth flow minted, or an API key). Read at call time.
    #[serde(default)]
    pub bearer_env: String,
}

/// `[mcp]`.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct McpConfig {
    #[serde(default)]
    pub servers: BTreeMap<String, McpServerConfig>,
}

struct Session {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: i64,
    tools: Vec<Value>,
}

static SPECS: OnceLock<Mutex<HashMap<String, McpServerConfig>>> = OnceLock::new();
static SESSIONS: OnceLock<Mutex<HashMap<String, Session>>> = OnceLock::new();

fn specs() -> &'static Mutex<HashMap<String, McpServerConfig>> {
    SPECS.get_or_init(|| Mutex::new(HashMap::new()))
}
fn sessions() -> &'static Mutex<HashMap<String, Session>> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Install `[mcp] servers` (called from config load, like the LSP table).
pub fn set_servers(map: BTreeMap<String, McpServerConfig>) {
    if let Ok(mut guard) = specs().lock() {
        *guard = map.into_iter().collect();
    }
}

pub fn configured() -> Vec<(String, bool)> {
    specs()
        .lock()
        .ok()
        .map(|g| {
            let mut v: Vec<(String, bool)> = g
                .iter()
                .map(|(name, cfg)| (name.clone(), cfg.read_only))
                .collect();
            v.sort();
            v
        })
        .unwrap_or_default()
}

pub fn is_read_only_server(name: &str) -> bool {
    specs()
        .lock()
        .ok()
        .and_then(|g| g.get(name).map(|c| c.read_only))
        .unwrap_or(false)
}

fn spec_of(name: &str) -> Result<McpServerConfig, String> {
    specs()
        .lock()
        .map_err(|_| "mcp: spec map poisoned".to_string())?
        .get(name)
        .cloned()
        .ok_or_else(|| format!("mcp: unknown server '{name}'"))
}

/// The bearer token for a remote server: its configured env variable first
/// (an API key or an OAuth token minted elsewhere), then a token stored by
/// `mcp login`.
fn bearer_for(spec: &McpServerConfig, name: &str) -> Option<String> {
    let env = spec.bearer_env.trim();
    if !env.is_empty() {
        if let Ok(v) = std::env::var(env) {
            let v = v.trim().to_string();
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    crate::mcp_oauth::stored_token(name)
}

fn http_server(name: &str, spec: &McpServerConfig) -> Result<crate::mcp_http::Connection, String> {
    let bearer = bearer_for(spec, name);
    crate::mcp_http::Connection::open(&spec.url, bearer.as_deref(), CALL_TIMEOUT)
        .map_err(|e| format!("mcp {name}: {e}"))
}

/// Spawn + handshake `name` if it is not running, and return its cached tools.
pub fn tools(name: &str) -> Result<Vec<Value>, String> {
    let spec = spec_of(name)?;
    if !spec.url.trim().is_empty() {
        let mut conn = http_server(name, &spec)?;
        return conn
            .list_tools()
            .map_err(|e| format!("mcp {name}: {e}"));
    }
    ensure(name)?;
    let mut guard = sessions().lock().map_err(|_| "mcp: session map poisoned")?;
    let session = guard
        .get_mut(name)
        .ok_or_else(|| format!("mcp: {name} is not running"))?;
    Ok(session.tools.clone())
}

/// Send `tools/call` to a running server (stdio) or a remote endpoint (HTTP).
pub fn call(name: &str, tool: &str, arguments: &Value) -> Result<String, String> {
    let spec = spec_of(name)?;
    if !spec.url.trim().is_empty() {
        let mut conn = http_server(name, &spec)?;
        let payload = conn
            .call_tool(tool, arguments.clone())
            .map_err(|e| format!("mcp {name}/{tool}: {e}"))?;
        return Ok(render_result(&payload));
    }
    ensure(name)?;
    // Serialize access per server: one child speaks one conversation.
    let mut guard = sessions().lock().map_err(|_| "mcp: session map poisoned")?;
    let session = guard
        .get_mut(name)
        .ok_or_else(|| format!("mcp: {name} is not running"))?;
    let result = session.request(
        "tools/call",
        json!({ "name": tool, "arguments": arguments }),
        CALL_TIMEOUT,
    )?;
    if let Some(err) = result.get("error") {
        return Err(format!("mcp {name}/{tool}: {err}"));
    }
    let payload = result.get("result").cloned().unwrap_or(Value::Null);
    Ok(render_result(&payload))
}

/// Render an MCP tool result as text: `content[].text`, or the JSON.
fn render_result(payload: &Value) -> String {
    if let Some(items) = payload.get("content").and_then(Value::as_array) {
        let mut out = String::new();
        for item in items {
            match item.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(text) = item.get("text").and_then(Value::as_str) {
                        out.push_str(text);
                        out.push('\n');
                    }
                }
                Some(kind) => out.push_str(&format!("[{kind} content]\n")),
                None => {}
            }
        }
        if !out.is_empty() {
            return out.trim_end().to_string();
        }
    }
    serde_json::to_string_pretty(payload).unwrap_or_else(|_| payload.to_string())
}

/// Spawn the server and complete the handshake when it is not running.
fn ensure(name: &str) -> Result<(), String> {
    if sessions()
        .lock()
        .map_err(|_| "mcp: session map poisoned")?
        .contains_key(name)
    {
        return Ok(());
    }
    let spec = specs()
        .lock()
        .map_err(|_| "mcp: spec map poisoned")?
        .get(name)
        .cloned()
        .ok_or_else(|| format!("mcp: unknown server '{name}'"))?;
    if spec.command.trim().is_empty() {
        return Err(format!(
            "mcp: server '{name}' has no `command`; stdio transport needs one"
        ));
    }
    let mut cmd = Command::new(&spec.command);
    cmd.args(&spec.args)
        .envs(&spec.env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("mcp: could not start {name}: {e}"))?;
    let stdin = child.stdin.take().ok_or("mcp: no stdin")?;
    let stdout = child.stdout.take().ok_or("mcp: no stdout")?;
    let mut session = Session {
        child,
        stdin,
        stdout: BufReader::new(stdout),
        next_id: 0,
        tools: Vec::new(),
    };
    let init = session.request(
        "initialize",
        json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": { "name": "nur", "version": env!("CARGO_PKG_VERSION") },
        }),
        HANDSHAKE_TIMEOUT,
    )?;
    if let Some(err) = init.get("error") {
        return Err(format!("mcp {name}: initialize failed: {err}"));
    }
    session.notify("notifications/initialized", json!({}));
    let mut cursor = Value::Null;
    for _ in 0..MAX_TOOLS_PAGES {
        let mut params = json!({});
        if !cursor.is_null() {
            params["cursor"] = cursor.clone();
        }
        let listed = session.request("tools/list", params, HANDSHAKE_TIMEOUT)?;
        let result = listed.get("result").cloned().unwrap_or(Value::Null);
        if let Some(tools) = result.get("tools").and_then(Value::as_array) {
            session.tools.extend(tools.iter().cloned());
        }
        match result.get("nextCursor").cloned() {
            Some(next) if !next.is_null() => cursor = next,
            _ => break,
        }
    }
    sessions()
        .lock()
        .map_err(|_| "mcp: session map poisoned")?
        .insert(name.to_string(), session);
    Ok(())
}

impl Session {
    fn notify(&mut self, method: &str, params: Value) {
        let body = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        let _ = self.send(&body);
    }

    fn request(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let body = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.send(&body)?;
        self.recv_id(id, timeout)
    }

    fn send(&mut self, body: &Value) -> Result<(), String> {
        let line = body.to_string();
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|_| self.stdin.write_all(b"\n"))
            .and_then(|_| self.stdin.flush())
            .map_err(|e| format!("mcp: write failed: {e}"))
    }

    /// Read until the reply with `id`, skipping server notifications (roots,
    /// sampling, progress). Anything addressed at us is declined politely.
    fn recv_id(&mut self, id: i64, timeout: Duration) -> Result<Value, String> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if std::time::Instant::now() >= deadline {
                return Err(format!("mcp: no reply to request {id} within {timeout:?}"));
            }
            let mut line = String::new();
            match self.stdout.read_line(&mut line) {
                Ok(0) => return Err("mcp: server closed its stdout".into()),
                Ok(_) => {}
                Err(e) => return Err(format!("mcp: read failed: {e}")),
            }
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let msg: Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if msg.get("id").and_then(Value::as_i64) == Some(id) {
                return Ok(msg);
            }
            // Server-initiated request (sampling/roots/elicitation): decline.
            if msg.get("method").is_some() && msg.get("id").is_some() {
                let body = json!({
                    "jsonrpc": "2.0",
                    "id": msg.get("id").cloned().unwrap_or(Value::Null),
                    "error": { "code": -32601, "message": "not supported by nur" },
                });
                let _ = self.send(&body);
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_to_no_servers() {
        let cfg = McpConfig::default();
        assert!(cfg.servers.is_empty());
        assert!(configured().is_empty());
    }

    #[test]
    fn result_render_flattens_text_content() {
        let payload = json!({
            "content": [
                { "type": "text", "text": "hello" },
                { "type": "text", "text": "world" }
            ]
        });
        assert_eq!(render_result(&payload), "hello\nworld");
    }

    #[test]
    fn result_render_falls_back_to_json() {
        let payload = json!({ "value": 3 });
        let out = render_result(&payload);
        assert!(out.contains("\"value\""), "{out}");
    }

    #[test]
    fn unknown_server_is_a_clear_error() {
        let err = tools("definitely-not-configured").unwrap_err();
        assert!(err.contains("unknown server"), "{err}");
    }

    #[test]
    fn read_only_marks_only_that_server() {
        let mut map = BTreeMap::new();
        map.insert(
            "docs".to_string(),
            McpServerConfig {
                command: "true".into(),
                read_only: true,
                ..Default::default()
            },
        );
        set_servers(map);
        assert!(is_read_only_server("docs"));
        assert!(!is_read_only_server("other"));
        set_servers(BTreeMap::new());
    }
}

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
use std::sync::{mpsc, Arc, Mutex, MutexGuard, OnceLock, TryLockError};
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
    /// ACP client servers run in their session's working directory.
    #[serde(skip)]
    pub session_cwd: Option<std::path::PathBuf>,
}

/// `[mcp]`.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct McpConfig {
    #[serde(default)]
    pub servers: BTreeMap<String, McpServerConfig>,
}

struct Session {
    child: Child,
    tree: Option<crate::process_tree::ProcessTree>,
    stdin: ChildStdin,
    stdout: mpsc::Receiver<Result<String, String>>,
    next_id: i64,
    tools: Vec<Value>,
}

static SPECS: OnceLock<Mutex<HashMap<String, McpServerConfig>>> = OnceLock::new();
type SessionSlot = Arc<Mutex<Option<Session>>>;
static SESSIONS: OnceLock<Mutex<HashMap<String, SessionSlot>>> = OnceLock::new();
// Client-provided servers are visible only within their owning ACP session.
type ScopedServers = HashMap<String, BTreeMap<String, (String, McpServerConfig)>>;
static SCOPED: OnceLock<Mutex<ScopedServers>> = OnceLock::new();
static ENV_SECRETS: OnceLock<Mutex<Vec<(String, String)>>> = OnceLock::new();
fn scoped() -> &'static Mutex<ScopedServers> {
    SCOPED.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn redaction_pairs() -> Vec<(String, String)> {
    ENV_SECRETS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .map(|pairs| pairs.clone())
        .unwrap_or_default()
}

pub(crate) fn register_scoped(
    session: &str,
    entries: BTreeMap<String, McpServerConfig>,
) -> Result<(), String> {
    let mut servers = BTreeMap::new();
    let mut secrets = ENV_SECRETS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .map_err(|_| "mcp: secret map poisoned")?;
    for (index, (name, spec)) in entries.into_iter().enumerate() {
        for (key, value) in &spec.env {
            let upper = key.to_ascii_uppercase();
            let credential_name = [
                "KEY",
                "TOKEN",
                "SECRET",
                "PASSWORD",
                "PASSWD",
                "CREDENTIAL",
                "AUTH",
            ]
            .iter()
            .any(|part| upper.contains(part));
            let ordinary = matches!(
                value.to_ascii_lowercase().as_str(),
                "development"
                    | "production"
                    | "staging"
                    | "localhost"
                    | "debug"
                    | "release"
                    | "true"
                    | "false"
            ) || (value.len() <= 5
                && value.bytes().all(|byte| byte.is_ascii_digit()));
            if value.is_empty() || (!credential_name && ordinary) {
                continue;
            }
            let pair = ("MCP_ENV".to_string(), value.clone());
            if !secrets.contains(&pair) {
                secrets.push(pair);
            }
        }
        servers.insert(name, (format!("__nur_acp_{session}_{index}"), spec));
    }
    drop(secrets);
    scoped()
        .lock()
        .map_err(|_| "mcp: scope map poisoned")?
        .insert(session.to_string(), servers);
    Ok(())
}

pub(crate) fn remove_scoped(session: &str) {
    let entries = scoped()
        .lock()
        .ok()
        .and_then(|mut scopes| scopes.remove(session));
    if let Some(entries) = entries {
        if let Ok(mut running) = sessions().lock() {
            for (_, (internal, _)) in entries {
                running.remove(&internal);
            }
        }
    }
}

pub(crate) fn configured_for(session: &str) -> Vec<(String, bool, String)> {
    let mut result: Vec<_> = configured()
        .into_iter()
        .map(|(name, read_only)| (name.clone(), read_only, name))
        .collect();
    if let Ok(scopes) = scoped().lock() {
        if let Some(entries) = scopes.get(session) {
            for (name, (internal, spec)) in entries {
                result.retain(|(existing, _, _)| existing != name);
                result.push((name.clone(), spec.read_only, internal.clone()));
            }
        }
    }
    result.sort_by(|a, b| a.0.cmp(&b.0));
    result
}

pub(crate) fn resolve_for(session: &str, name: &str) -> Option<String> {
    configured_for(session)
        .into_iter()
        .find(|(visible, _, _)| visible == name)
        .map(|(_, _, internal)| internal)
}

fn specs() -> &'static Mutex<HashMap<String, McpServerConfig>> {
    SPECS.get_or_init(|| Mutex::new(HashMap::new()))
}
fn sessions() -> &'static Mutex<HashMap<String, SessionSlot>> {
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
    // A scoped server can shadow a global read-only name. Since classification
    // is shared, fail closed while any such client server exists.
    if scoped()
        .lock()
        .map(|scopes| scopes.values().any(|entries| entries.contains_key(name)))
        .unwrap_or(true)
    {
        return false;
    }
    specs()
        .lock()
        .ok()
        .and_then(|g| g.get(name).map(|c| c.read_only))
        .unwrap_or(false)
}

fn spec_of(name: &str) -> Result<McpServerConfig, String> {
    if let Some(spec) = scoped().lock().ok().and_then(|scopes| {
        scopes
            .values()
            .flat_map(|entries| entries.values())
            .find(|(internal, _)| internal == name)
            .map(|(_, spec)| spec.clone())
    }) {
        return Ok(spec);
    }
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
fn external_bearer(spec: &McpServerConfig) -> Option<String> {
    let env = spec.bearer_env.trim();
    if !env.is_empty() {
        if let Ok(v) = std::env::var(env) {
            let v = v.trim().to_string();
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

pub(crate) fn bearer_for(spec: &McpServerConfig, name: &str) -> Result<Option<String>, String> {
    if let Some(token) = external_bearer(spec) {
        return Ok(Some(token));
    }
    crate::mcp_oauth::stored_token(name, &spec.url)
}

fn http_server(name: &str, spec: &McpServerConfig) -> Result<crate::mcp_http::Connection, String> {
    let bearer = bearer_for(spec, name)?;
    if bearer.is_some() {
        crate::mcp_oauth::secure_url(&spec.url)?;
    }
    match crate::mcp_http::Connection::open(&spec.url, bearer.as_deref(), CALL_TIMEOUT) {
        Err(crate::mcp_http::McpError::Unauthorized { status: 401, .. })
            if external_bearer(spec).is_none() && bearer.is_some() =>
        {
            let fresh = crate::mcp_oauth::refresh_rejected_token(
                name,
                &spec.url,
                bearer.as_deref().unwrap_or_default(),
            )?;
            crate::mcp_http::Connection::open(&spec.url, fresh.as_deref(), CALL_TIMEOUT)
                .map_err(|e| format!("mcp {name}: {e}"))
        }
        result => result.map_err(|e| format!("mcp {name}: {e}")),
    }
}

fn reopen_after_rejection(
    name: &str,
    spec: &McpServerConfig,
    conn: &crate::mcp_http::Connection,
    error: &crate::mcp_http::McpError,
) -> Result<Option<crate::mcp_http::Connection>, String> {
    if !matches!(
        error,
        crate::mcp_http::McpError::Unauthorized { status: 401, .. }
    ) || external_bearer(spec).is_some()
    {
        return Ok(None);
    }
    let Some(rejected) = conn.bearer_token() else {
        return Ok(None);
    };
    let Some(fresh) = crate::mcp_oauth::refresh_rejected_token(name, &spec.url, rejected)? else {
        return Ok(None);
    };
    crate::mcp_http::Connection::open(&spec.url, Some(&fresh), CALL_TIMEOUT)
        .map(Some)
        .map_err(|error| format!("mcp {name}: {error}"))
}

/// Spawn + handshake `name` if it is not running, and return its cached tools.
pub fn tools(
    name: &str,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<Vec<Value>, String> {
    let spec = spec_of(name)?;
    if !spec.url.trim().is_empty() {
        let value = http_child(name, &spec, "list", "", &Value::Null, cancel)?;
        return serde_json::from_value(value)
            .map_err(|error| format!("mcp: invalid helper response: {error}"));
    }
    let slot = ensure(name, cancel)?;
    let guard = lock_slot(&slot, cancel)?;
    let session = guard
        .as_ref()
        .ok_or_else(|| format!("mcp: {name} is not running"))?;
    Ok(session.tools.clone())
}

/// Send `tools/call` to a running server (stdio) or a remote endpoint (HTTP).
pub fn call(
    name: &str,
    tool: &str,
    arguments: &Value,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<String, String> {
    let spec = spec_of(name)?;
    if !spec.url.trim().is_empty() {
        let value = http_child(name, &spec, "call", tool, arguments, cancel)?;
        return value
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| "mcp: invalid helper response".to_string());
    }
    let slot = ensure(name, cancel)?;
    // Serialize each server independently; unrelated sessions can progress.
    let mut guard = lock_slot(&slot, cancel)?;
    let session = guard
        .as_mut()
        .ok_or_else(|| format!("mcp: {name} is not running"))?;
    let result = session.request(
        "tools/call",
        json!({ "name": tool, "arguments": arguments }),
        CALL_TIMEOUT,
        Some(cancel),
    );
    if cancel.is_cancelled() {
        guard.take();
        return Err("mcp: cancelled".into());
    }
    let result = result?;
    if let Some(err) = result.get("error") {
        return Err(format!("mcp {name}/{tool}: {err}"));
    }
    let payload = result.get("result").cloned().unwrap_or(Value::Null);
    Ok(render_result(&payload))
}

/// A blocking HTTP transport runs in an owned helper, so cancelling a turn
/// can stop connection, response-body and SSE reads without detaching work.
fn http_child(
    name: &str,
    spec: &McpServerConfig,
    action: &str,
    tool: &str,
    arguments: &Value,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<Value, String> {
    use std::io::Read;
    if cancel.is_cancelled() {
        return Err("mcp: cancelled".into());
    }
    // The helper may rotate or replace the store before this process sees
    // another tool result, so retain both the old and the new credentials.
    let _ = crate::mcp_oauth::redaction_pairs();
    if let Some(token) = external_bearer(spec) {
        let mut secrets = ENV_SECRETS
            .get_or_init(|| Mutex::new(Vec::new()))
            .lock()
            .map_err(|_| "mcp: secret map poisoned")?;
        let pair = ("MCP_ENV".to_string(), token);
        if !secrets.contains(&pair) {
            secrets.push(pair);
        }
    }
    let executable =
        std::env::current_exe().map_err(|e| format!("mcp: cannot find helper: {e}"))?;
    let input = serde_json::to_vec(&json!({"name": name, "spec": spec, "action": action, "tool": tool, "arguments": arguments})).map_err(|e| e.to_string())?;
    let mut command = Command::new(executable);
    command
        .arg("--nur-mcp-http-internal")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let (mut child, tree) = crate::process_tree::ProcessTree::spawn_with_flags(
        &mut command,
        crate::process_tree::CREATE_NO_WINDOW,
    )
    .map_err(|e| format!("mcp: cannot start helper: {e}"))?;
    let Some(mut stdin) = child.stdin.take() else {
        tree.terminate(&mut child);
        return Err("mcp: helper has no stdin".into());
    };
    let Some(stdout) = child.stdout.take() else {
        tree.terminate(&mut child);
        return Err("mcp: helper has no stdout".into());
    };
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let reader = std::thread::spawn(move || {
        let mut body = Vec::new();
        let result = stdout.take(20_000_001).read_to_end(&mut body);
        (result, body)
    });
    let deadline = std::time::Instant::now() + CALL_TIMEOUT + Duration::from_secs(10);
    let status = loop {
        if cancel.is_cancelled() || std::time::Instant::now() >= deadline {
            tree.terminate(&mut child);
            let _ = writer.join();
            let _ = reader.join();
            return Err(if cancel.is_cancelled() {
                "mcp: cancelled"
            } else {
                "mcp: helper timed out"
            }
            .into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(30)),
            Err(error) => {
                tree.terminate(&mut child);
                let _ = writer.join();
                let _ = reader.join();
                return Err(format!("mcp: helper failed: {error}"));
            }
        }
    };
    writer
        .join()
        .map_err(|_| "mcp: helper input failed".to_string())?
        .map_err(|e| format!("mcp: helper input failed: {e}"))?;
    let (read, body) = reader
        .join()
        .map_err(|_| "mcp: helper output failed".to_string())?;
    read.map_err(|e| format!("mcp: helper output failed: {e}"))?;
    if !status.success() || body.len() > 20_000_000 {
        return Err("mcp: helper failed or returned too much data".into());
    }
    let reply: Value = serde_json::from_slice(&body)
        .map_err(|_| "mcp: helper returned invalid JSON".to_string())?;
    if let Some(error) = reply.get("error").and_then(Value::as_str) {
        return Err(error.to_string());
    }
    reply
        .get("result")
        .cloned()
        .ok_or_else(|| "mcp: helper returned no result".into())
}

pub(crate) fn run_http_helper() {
    // reqwest's blocking client owns a runtime and must not be dropped on the
    // async main thread. The helper's sole worker performs the whole request.
    let response = std::thread::spawn(|| -> Result<Value, String> {
        use std::io::Read;
        let mut input = Vec::new();
        std::io::stdin()
            .take(20_000_001)
            .read_to_end(&mut input)
            .map_err(|e| e.to_string())?;
        if input.len() > 20_000_000 {
            return Err("mcp: helper input too large".into());
        }
        let request: Value = serde_json::from_slice(&input).map_err(|e| e.to_string())?;
        let name = request
            .get("name")
            .and_then(Value::as_str)
            .ok_or("missing server name")?;
        let spec: McpServerConfig =
            serde_json::from_value(request.get("spec").cloned().ok_or("missing server spec")?)
                .map_err(|e| e.to_string())?;
        if spec.url.trim().is_empty() {
            return Err("mcp: helper requires an HTTP server".into());
        }
        let mut conn = http_server(name, &spec)?;
        match request.get("action").and_then(Value::as_str) {
            Some("list") => {
                let result = match conn.list_tools() {
                    Ok(tools) => tools,
                    Err(error) => match reopen_after_rejection(name, &spec, &conn, &error)? {
                        Some(mut reopened) => reopened
                            .list_tools()
                            .map_err(|e| format!("mcp {name}: {e}"))?,
                        None => return Err(format!("mcp {name}: {error}")),
                    },
                };
                Ok(json!(result))
            }
            Some("call") => {
                let tool = request
                    .get("tool")
                    .and_then(Value::as_str)
                    .ok_or("missing tool name")?;
                let arguments = request.get("arguments").cloned().unwrap_or(Value::Null);
                let result = match conn.call_tool(tool, arguments.clone()) {
                    Ok(payload) => payload,
                    Err(error) => match reopen_after_rejection(name, &spec, &conn, &error)? {
                        Some(mut reopened) => reopened
                            .call_tool(tool, arguments)
                            .map_err(|e| format!("mcp {name}/{tool}: {e}"))?,
                        None => return Err(format!("mcp {name}/{tool}: {error}")),
                    },
                };
                Ok(json!(render_result(&result)))
            }
            _ => Err("mcp: invalid helper action".into()),
        }
    })
    .join()
    .unwrap_or_else(|_| Err("mcp: helper failed".into()));
    let reply = match response {
        Ok(result) => json!({"result": result}),
        Err(error) => json!({"error": crate::secrets::redact(&error).into_owned()}),
    };
    let body = reply.to_string();
    if body.len() > 20_000_000 {
        let _ = std::io::stdout().write_all(br#"{"error":"mcp: helper response too large"}"#);
    } else {
        let _ = std::io::stdout().write_all(body.as_bytes());
    }
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

fn lock_slot<'a>(
    slot: &'a Mutex<Option<Session>>,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<MutexGuard<'a, Option<Session>>, String> {
    loop {
        if cancel.is_cancelled() {
            return Err("mcp: cancelled".into());
        }
        match slot.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(20)),
            Err(TryLockError::Poisoned(_)) => return Err("mcp: server session poisoned".into()),
        }
    }
}

/// Spawn and handshake once per server, without blocking unrelated servers.
fn ensure(name: &str, cancel: &tokio_util::sync::CancellationToken) -> Result<SessionSlot, String> {
    let slot = sessions()
        .lock()
        .map_err(|_| "mcp: session map poisoned")?
        .entry(name.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(None)))
        .clone();
    let mut guard = lock_slot(&slot, cancel)?;
    if guard.is_some() {
        drop(guard);
        return Ok(slot);
    }
    let spec = spec_of(name)?;
    if spec.command.trim().is_empty() {
        return Err(format!(
            "mcp: server '{name}' has no `command`; stdio transport needs one"
        ));
    }
    let mut cmd = Command::new(&spec.command);
    crate::tools::tool_env(&mut cmd);
    if let Some(cwd) = &spec.session_cwd {
        cmd.current_dir(cwd);
    }
    cmd.args(&spec.args)
        .envs(&spec.env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let (mut child, tree) = crate::process_tree::ProcessTree::spawn(&mut cmd)
        .map_err(|e| format!("mcp: could not start {name}: {e}"))?;
    let Some(stdin) = child.stdin.take() else {
        tree.terminate(&mut child);
        return Err("mcp: no stdin".into());
    };
    let Some(stdout) = child.stdout.take() else {
        tree.terminate(&mut child);
        return Err("mcp: no stdout".into());
    };
    let rx = read_stdout(stdout);
    let mut session = Session {
        child,
        tree: Some(tree),
        stdin,
        stdout: rx,
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
        Some(cancel),
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
        let listed = session.request("tools/list", params, HANDSHAKE_TIMEOUT, Some(cancel))?;
        let result = listed.get("result").cloned().unwrap_or(Value::Null);
        if let Some(tools) = result.get("tools").and_then(Value::as_array) {
            session.tools.extend(tools.iter().cloned());
        }
        match result.get("nextCursor").cloned() {
            Some(next) if !next.is_null() => cursor = next,
            _ => break,
        }
    }
    *guard = Some(session);
    drop(guard);
    Ok(slot)
}

fn read_stdout(stdout: ChildStdout) -> mpsc::Receiver<Result<String, String>> {
    let (tx, rx) = mpsc::sync_channel(64);
    std::thread::spawn(move || {
        let mut stdout = BufReader::new(stdout);
        loop {
            let mut line = String::new();
            let result = match stdout.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => Ok(line),
                Err(e) => Err(format!("mcp: read failed: {e}")),
            };
            let failed = result.is_err();
            if tx.send(result).is_err() || failed {
                break;
            }
        }
    });
    rx
}

impl Session {
    fn notify(&mut self, method: &str, params: Value) {
        let body = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        let _ = self.send(&body);
    }

    fn request(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
        cancel: Option<&tokio_util::sync::CancellationToken>,
    ) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let body = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.send(&body)?;
        self.recv_id(id, timeout, cancel)
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
    fn recv_id(
        &mut self,
        id: i64,
        timeout: Duration,
        cancel: Option<&tokio_util::sync::CancellationToken>,
    ) -> Result<Value, String> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if cancel.is_some_and(tokio_util::sync::CancellationToken::is_cancelled) {
                return Err("mcp: cancelled".into());
            }
            if std::time::Instant::now() >= deadline {
                return Err(format!("mcp: no reply to request {id} within {timeout:?}"));
            }
            let line = match self.stdout.recv_timeout(
                deadline
                    .saturating_duration_since(std::time::Instant::now())
                    .min(Duration::from_millis(100)),
            ) {
                Ok(result) => result?,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("mcp: server closed its stdout".into())
                }
            };
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
        if let Some(tree) = &self.tree {
            tree.terminate(&mut self.child);
        } else {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silent_server_times_out_without_waiting_for_eof() {
        let mut child = Command::new("python")
            .args(["-c", "import time; time.sleep(10)"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = read_stdout(child.stdout.take().unwrap());
        let mut session = Session {
            child,
            tree: None,
            stdin,
            stdout,
            next_id: 0,
            tools: Vec::new(),
        };
        let start = std::time::Instant::now();
        let err = session
            .request("initialize", json!({}), Duration::from_millis(100), None)
            .unwrap_err();
        assert!(err.contains("no reply"), "{err}");
        assert!(start.elapsed() < Duration::from_secs(3));
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
        let err = tools(
            "definitely-not-configured",
            &tokio_util::sync::CancellationToken::new(),
        )
        .unwrap_err();
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

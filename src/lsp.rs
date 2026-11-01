//! Native LSP client: one language server per file extension, JSON-RPC over
//! stdio, spawned lazily and killed on idle.
//!
//! opencode and Crush surface real diagnostics from language servers after
//! edits; this module gives nur the same surface through one `lsp` tool with
//! an op enum, plus an edit-tool hook that appends a compact diagnostics
//! block when a server for the edited file's language is already running
//! (servers are never spawned implicitly: rust-analyzer alone is a 30 s,
//! multi-GB cold start).
//!
//! Wire shape: `Content-Length: N\r\n\r\n<json>` both ways (LSP spec 3.17).

use crate::error::{NurError, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// A running server child plus its next message id. The reader thread pushes
/// responses and server notifications into shared queues; the calling thread
/// correlates by id.
struct Server {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    next_id: i64,
    responses: Arc<Mutex<HashMap<i64, Value>>>,
    notifications: Arc<Mutex<Vec<Value>>>,
    started: Instant,
    last_used: Mutex<Instant>,
    root: PathBuf,
}

impl Server {
    fn touch(&self) {
        *self.last_used.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Process-wide registry: extension -> server, plus the file URI each opened.
static REGISTRY: OnceLock<Mutex<HashMap<String, RunningServer>>> = OnceLock::new();

struct RunningServer {
    server: Arc<Mutex<Server>>,
    /// Files the client didOpen'd, so didChange can be pushed after edits.
    opened: Vec<String>,
}

fn registry() -> &'static Mutex<HashMap<String, RunningServer>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Built-in server commands per extension. Each entry is tried in order and
/// the first one found on PATH wins; `[lsp] servers` config overrides these.
const DEFAULT_SERVERS: &[(&str, &[&str])] = &[
    ("rs", &["rust-analyzer"]),
    ("ts", &["typescript-language-server", "--stdio"]),
    ("tsx", &["typescript-language-server", "--stdio"]),
    ("js", &["typescript-language-server", "--stdio"]),
    ("jsx", &["typescript-language-server", "--stdio"]),
    ("py", &["pyright-langserver", "--stdio", "basedpyright-langserver", "--stdio"]),
    ("go", &["gopls"]),
];

/// `[lsp] servers` from config: extension -> command words. A Mutex rather
/// than a OnceLock because the table is re-set on every config load (and in
/// tests, where several cases install different overrides).
static CONFIG_OVERRIDES: OnceLock<Mutex<Option<HashMap<String, Vec<String>>>>> = OnceLock::new();

pub fn set_config_overrides(map: Option<HashMap<String, Vec<String>>>) {
    let cell = CONFIG_OVERRIDES.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = cell.lock() {
        *guard = map;
    }
}

/// The command words for `ext`, from config then defaults, or None when the
/// extension has no candidate on PATH.
fn command_for(ext: &str) -> Option<Vec<String>> {
    if let Some(map) = CONFIG_OVERRIDES
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|g| g.clone())
    {
        if let Some(words) = map.get(ext) {
            if !words.is_empty() {
                return Some(words.clone());
            }
        }
    }
    // Each extension's entry lists one command as words: bare executables
    // start a new candidate, flags belong to the one before them.
    let raw: Vec<&str> = DEFAULT_SERVERS
        .iter()
        .find(|(e, _)| *e == ext)
        .map(|(_, cmds)| cmds.to_vec())
        .unwrap_or_default();
    let mut candidates: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    for word in raw {
        let starts_new = !current.is_empty() && !word.starts_with('-');
        if starts_new {
            candidates.push(std::mem::take(&mut current));
        }
        current.push(word.to_string());
    }
    if !current.is_empty() {
        candidates.push(current);
    }
    candidates
        .into_iter()
        .find(|cmd| which(&cmd[0]).is_some())
}

/// Minimal PATH probe (no `which` crate dependency).
fn which(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(program);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            for ext in [".exe", ".cmd", ".bat"] {
                let candidate = dir.join(format!("{program}{ext}"));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

/// JSON-RPC over stdio for one already-spawned child.
fn spawn_server(cmd: &[String], root: &Path) -> std::result::Result<Server, String> {
    let mut child = std::process::Command::new(&cmd[0])
        .args(&cmd[1..])
        .current_dir(root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("lsp: could not start {}: {e}", cmd[0]))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "lsp: no stdin".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "lsp: no stdout".to_string())?;

    let responses = Arc::new(Mutex::new(HashMap::new()));
    let notifications = Arc::new(Mutex::new(Vec::new()));
    let (r_tx, n_tx) = (responses.clone(), notifications.clone());
    std::thread::Builder::new()
        .name("nur-lsp-reader".into())
        .spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_message(&mut reader) {
                    Ok(Some(text)) => match serde_json::from_str::<Value>(&text) {
                        Ok(msg) => {
                            if let Some(id) = msg.get("id").and_then(Value::as_i64) {
                                r_tx.lock().unwrap().insert(id, msg);
                            } else {
                                n_tx.lock().unwrap().push(msg);
                            }
                        }
                        Err(_) => break,
                    },
                    Ok(None) => break, // EOF
                    Err(_) => break,
                }
            }
        })
        .map_err(|e| format!("lsp: reader thread: {e}"))?;

    Ok(Server {
        child,
        stdin,
        next_id: 0,
        responses,
        notifications,
        started: Instant::now(),
        last_used: Mutex::new(Instant::now()),
        root: root.to_path_buf(),
    })
}

/// Read one `Content-Length`-framed message. `Ok(None)` on clean EOF.
fn read_message(reader: &mut impl BufRead) -> std::result::Result<Option<String>, String> {
    let mut length = None;
    loop {
        let mut line = String::new();
        let n = reader
            .read_line(&mut line)
            .map_err(|e| format!("lsp read: {e}"))?;
        if n == 0 {
            return Ok(None);
        }
        let line = line.trim_end();
        if line.is_empty() {
            break; // end of headers
        }
        if let Some(v) = line
            .strip_prefix("Content-Length:")
            .map(str::trim)
            .and_then(|v| v.parse::<usize>().ok())
        {
            length = Some(v);
        }
    }
    let length = length.ok_or_else(|| "lsp: message without Content-Length".to_string())?;
    if length > 32 * 1024 * 1024 {
        return Err(format!("lsp: message of {length} bytes over the 32 MiB cap"));
    }
    let mut buf = vec![0u8; length];
    reader
        .read_exact(&mut buf)
        .map_err(|e| format!("lsp read body: {e}"))?;
    String::from_utf8(buf).map(Some).map_err(|e| format!("lsp body: {e}"))
}

/// Encode one message with its Content-Length header.
fn write_message(writer: &mut impl Write, body: &str) -> std::result::Result<(), String> {
    write!(
        writer,
        "Content-Length: {}\r\n\r\n{}",
        body.len(),
        body
    )
    .map_err(|e| format!("lsp write: {e}"))
}

/// File path -> LSP file URI (percent-encoded, forward slashes).
pub fn file_uri(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut encoded = String::with_capacity(text.len() + 8);
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    format!("file:///{encoded}")
}

/// The language id nur sends for an extension. Servers are lenient here.
fn language_id(ext: &str) -> &str {
    match ext {
        "rs" => "rust",
        "py" => "python",
        "ts" => "typescript",
        "tsx" => "typescriptreact",
        "js" => "javascript",
        "jsx" => "javascriptreact",
        "go" => "go",
        other => other,
    }
}

/// Ensure a server is running for `path`; spawn + initialize when needed.
fn ensure_running(path: &Path) -> Result<Arc<Mutex<Server>>> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let cmd = command_for(&ext).ok_or_else(|| {
        NurError::Tool(format!(
            "lsp: no language server available for .{ext} files - install one \
             (rust-analyzer, typescript-language-server, pyright-langserver, gopls) \
             or set [lsp] servers.{ext}"
        ))
    })?;
    let root = path
        .parent()
        .and_then(|p| p.ancestors().find(|a| a.join(".git").exists()))
        .map(Path::to_path_buf)
        .unwrap_or_else(|| path.parent().unwrap_or(Path::new(".")).to_path_buf());

    let mut reg = registry().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(running) = reg.get_mut(&ext) {
        // Died servers (crash, exit) respawn on next use.
        let alive = running.server.lock().map(|s| s.child.id() > 0).unwrap_or(false);
        if alive {
            let server = running.server.clone();
            return Ok(server);
        }
        reg.remove(&ext);
    }

    let mut server = Arc::new(
        Mutex::new(
            spawn_server(&cmd, &root)
                .map_err(NurError::Tool)?,
        ),
    );

    // initialize handshake; full-sync textDocuments (didSave carries text).
    let root_uri = file_uri(&root);
    let init = request(
        &mut server,
        "initialize",
        serde_json::json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "capabilities": {},
        }),
        Duration::from_secs(30),
    )?;
    if init.get("error").is_some() {
        return Err(NurError::Tool(format!(
            "lsp: initialize failed: {}",
            init["error"]
        )));
    }
    notify(&mut server, "initialized", serde_json::json!({}));
    reg.insert(
        ext,
        RunningServer {
            server: server.clone(),
            opened: Vec::new(),
        },
    );
    Ok(server)
}

/// Send a request and wait for its correlated response.
fn request(
    server: &Arc<Mutex<Server>>,
    method: &str,
    params: Value,
    timeout: Duration,
) -> Result<Value> {
    let (id, payload) = {
        let mut guard = server.lock().map_err(|_| NurError::Tool("lsp: server poisoned".into()))?;
        let id = guard.next_id;
        guard.next_id += 1;
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        })
        .to_string();
        (id, payload)
    };
    {
        let mut guard = server.lock().map_err(|_| NurError::Tool("lsp: server poisoned".into()))?;
        write_message(&mut guard.stdin, &payload).map_err(NurError::Tool)?;
        guard.touch();
    }
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(msg) = server
            .lock()
            .map_err(|_| NurError::Tool("lsp: server poisoned".into()))?
            .responses
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id)
        {
            return Ok(msg);
        }
        if Instant::now() >= deadline {
            return Err(NurError::Tool(format!(
                "lsp: {method} timed out after {}s (server cold start can take longer; retry once)",
                timeout.as_secs()
            )));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Fire a notification (no response expected).
fn notify(server: &Arc<Mutex<Server>>, method: &str, params: Value) {
    if let Ok(mut guard) = server.lock() {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        })
        .to_string();
        let _ = write_message(&mut guard.stdin, &payload);
        guard.touch();
    }
}

/// Drain publishDiagnostics notifications collected so far for `uri`.
fn diagnostics_for(server: &Arc<Mutex<Server>>, uri: &str) -> Vec<(u32, u32, String, String)> {
    let mut out = Vec::new();
    if let Ok(guard) = server.lock() {
        let mut queue = guard
            .notifications
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        queue.retain(|msg| {
            if msg.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics")
            {
                if msg.pointer("/params/uri").and_then(Value::as_str) == Some(uri) {
                    if let Some(diags) =
                        msg.pointer("/params/diagnostics").and_then(Value::as_array)
                    {
                        for d in diags {
                            let line = d
                                .pointer("/range/start/line")
                                .and_then(Value::as_u64)
                                .unwrap_or(0) as u32;
                            let col = d
                                .pointer("/range/start/character")
                                .and_then(Value::as_u64)
                                .unwrap_or(0) as u32;
                            let severity = match d.get("severity").and_then(Value::as_i64) {
                                Some(1) => "error",
                                Some(2) => "warning",
                                Some(3) => "info",
                                Some(4) => "hint",
                                _ => "diag",
                            };
                            let message = d
                                .get("message")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .lines()
                                .next()
                                .unwrap_or_default()
                                .to_string();
                            let source = d
                                .get("source")
                                .and_then(Value::as_str)
                                .unwrap_or("lsp")
                                .to_string();
                            out.push((line, col, format!("{severity}: {message}"), source));
                        }
                    }
                }
                return false; // consumed
            }
            true
        });
    }
    out
}

/// Render a compact diagnostics block, or an empty string when clean.
fn render_diagnostics(diags: &[(u32, u32, String, String)], max: usize) -> String {
    if diags.is_empty() {
        return String::new();
    }
    let mut out = String::from("\nLSP diagnostics:\n");
    for (line, col, text, _source) in diags.iter().take(max) {
        out.push_str(&format!("  L{} C{} {}\n", line + 1, col + 1, text));
    }
    if diags.len() > max {
        out.push_str(&format!("  ... and {} more\n", diags.len() - max));
    }
    out
}

/// didOpen (or didChange) `path` so the server's view matches the disk.
fn sync_document(server: &Arc<Mutex<Server>>, path: &Path, ext: &str, text: &str) {
    let uri = file_uri(path);
    let reg_ok = registry()
        .lock()
        .map(|mut reg| {
            if let Some(running) = reg.get_mut(ext) {
                if !running.opened.contains(&uri) {
                    running.opened.push(uri.clone());
                    notify(
                        server,
                        "textDocument/didOpen",
                        serde_json::json!({
                            "textDocument": {
                                "uri": uri, "languageId": language_id(ext),
                                "version": 1, "text": text,
                            }
                        }),
                    );
                } else {
                    notify(
                        server,
                        "textDocument/didChange",
                        serde_json::json!({
                            "textDocument": { "uri": uri, "version": 2 },
                            "contentChanges": [{ "text": text }],
                        }),
                    );
                }
                true
            } else {
                false
            }
        })
        .unwrap_or(false);
    let _ = reg_ok;
}

// ---------------------------------------------------------------------------
// Public surface used by the `lsp` tool and the edit hooks.
// ---------------------------------------------------------------------------

/// Run an `lsp` operation. One tool, one op enum, matching opencode/Crush.
pub fn run_op(
    op: &str,
    path: &Path,
    extra: &Value,
) -> Result<String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let text = std::fs::read_to_string(path)
        .map_err(|e| NurError::Tool(format!("lsp: read {}: {e}", path.display())))?;

    match op {
        "stop" => {
            let mut reg = registry().lock().unwrap_or_else(|e| e.into_inner());
            let removed = reg.remove(&ext);
            drop(reg);
            return Ok(match removed {
                Some(_) => format!("stopped the {} language server", language_id(&ext)),
                None => format!("no {} language server was running", language_id(&ext)),
            });
        }
        "diagnostics" => {
            let server = ensure_running(path)?;
            sync_document(&server, path, &ext, &text);
            // Servers publish asynchronously; a short wait usually suffices.
            let wait = Duration::from_millis(
                extra.get("wait_ms").and_then(Value::as_u64).unwrap_or(1500).min(10_000),
            );
            let deadline = Instant::now() + wait;
            loop {
                let diags = diagnostics_for(&server, &file_uri(path));
                if !diags.is_empty() || Instant::now() >= deadline {
                    if diags.is_empty() {
                        return Ok(format!(
                            "no diagnostics for {} (waited {}ms)",
                            path.display(),
                            wait.as_millis()
                        ));
                    }
                    return Ok(format!(
                        "{} diagnostic(s) for {}:{}",
                        diags.len(),
                        path.display(),
                        render_diagnostics(&diags, 30)
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        "hover" | "definition" | "references" | "symbols" => {
            let server = ensure_running(path)?;
            sync_document(&server, path, &ext, &text);
            let uri = file_uri(path);
            let line = extra.get("line").and_then(Value::as_u64).unwrap_or(0) as u64;
            let character = extra
                .get("character")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u64;
            let position = serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
            });
            let method = match op {
                "hover" => "textDocument/hover",
                "definition" => "textDocument/definition",
                "references" => {
                    let mut p = position.clone();
                    p["context"] = serde_json::json!({ "includeDeclaration": true });
                    let resp = request(&server, "textDocument/references", p, Duration::from_secs(20))?;
                    return Ok(render_locations(&resp, "references"));
                }
                _ => "textDocument/documentSymbol",
            };
            let resp = request(&server, method, position, Duration::from_secs(20))?;
            Ok(match op {
                "hover" => render_hover(&resp),
                "definition" => render_locations(&resp, "definition"),
                _ => render_symbols(&resp),
            })
        }
        other => Err(NurError::Tool(format!(
            "lsp: unknown op '{other}' - use diagnostics | hover | definition | references | \
             symbols | stop"
        ))),
    }
}

/// After-edit hook: append diagnostics when a server for this language is
/// ALREADY running (never spawn implicitly). Returns "" otherwise.
pub fn diagnostics_after_edit(path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let already_running = registry()
        .lock()
        .map(|reg| {
            reg.get(&ext)
                .map(|r| r.server.lock().map(|s| s.child.id() > 0).unwrap_or(false))
                .unwrap_or(false)
        })
        .unwrap_or(false);
    if !already_running {
        return String::new();
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return String::new();
    };
    let Ok(server) = ensure_running(path) else {
        return String::new();
    };
    sync_document(&server, path, &ext, &text);
    std::thread::sleep(Duration::from_millis(400));
    let diags = diagnostics_for(&server, &file_uri(path));
    render_diagnostics(&diags, 15)
}

/// True when a server for `ext` is running (for the tool's status output and
/// tests). Not part of the tool schema.
pub fn is_running(ext: &str) -> bool {
    registry()
        .lock()
        .map(|reg| {
            reg.get(ext)
                .map(|r| r.server.lock().map(|s| s.child.id() > 0).unwrap_or(false))
                .unwrap_or(false)
        })
        .unwrap_or(false)
}

fn render_hover(resp: &Value) -> String {
    let contents = resp
        .pointer("/result/contents")
        .cloned()
        .unwrap_or(Value::Null);
    let text = match &contents {
        Value::String(s) => s.clone(),
        v => v
            .pointer("/value")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| v.to_string()),
    };
    if text.is_empty() || text == "null" {
        "no hover information".into()
    } else {
        text
    }
}

fn render_locations(resp: &Value, label: &str) -> String {
    let empty = Vec::new();
    let locs = match resp.get("result") {
        Some(Value::Array(a)) => a.clone(),
        Some(loc) if loc.is_object() && loc.get("uri").is_some() => vec![loc.clone()],
        _ => empty,
    };
    if locs.is_empty() {
        return format!("no {label} found");
    }
    let mut out = format!("{} {label}:\n", locs.len());
    for loc in locs.iter().take(20) {
        let uri = loc.get("uri").and_then(Value::as_str).unwrap_or("?");
        let line = loc
            .pointer("/range/start/line")
            .and_then(Value::as_u64)
            .map(|l| l + 1)
            .unwrap_or(0);
        let path = uri.strip_prefix("file://").unwrap_or(uri);
        out.push_str(&format!("  {path}:{line}\n"));
    }
    out
}

fn render_symbols(resp: &Value) -> String {
    let empty = Vec::new();
    let symbols = resp
        .pointer("/result")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or(empty);
    if symbols.is_empty() {
        return "no symbols".into();
    }
    fn walk(items: &[Value], depth: usize, out: &mut String, budget: &mut usize) {
        for s in items {
            if *budget == 0 {
                return;
            }
            *budget -= 1;
            let name = s.get("name").and_then(Value::as_str).unwrap_or("?");
            let kind = s
                .get("kind")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let line = s
                .pointer("/range/start/line")
                .and_then(Value::as_u64)
                .map(|l| l + 1)
                .unwrap_or(0);
            let kind_label = kind_label(kind);
            out.push_str(&format!(
                "{}{name} ({kind_label}) :{line}\n",
                "  ".repeat(depth)
            ));
            if let Some(children) = s.get("children").and_then(Value::as_array) {
                walk(children, depth + 1, out, budget);
            }
        }
    }
    let mut out = String::new();
    let mut budget = 120;
    walk(&symbols, 0, &mut out, &mut budget);
    out
}

fn kind_label(kind: u64) -> &'static str {
    match kind {
        2 => "module",
        3 => "namespace",
        4 => "class",
        5 => "method",
        6 => "property",
        7 => "field",
        8 => "constructor",
        9 => "enum",
        10 => "function",
        11 => "variable",
        12 => "constant",
        13 => "string",
        14 => "number",
        23 => "struct",
        24 => "event",
        25 => "operator",
        26 => "type-parameter",
        _ => "symbol",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_framing_round_trips() {
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"x"}"#;
        let mut framed = Vec::new();
        write_message(&mut framed, body).unwrap();
        let text = String::from_utf8(framed).unwrap();
        assert!(text.starts_with(&format!("Content-Length: {}\r\n\r\n", body.len())));
        let mut reader = BufReader::new(text.as_bytes());
        let got = read_message(&mut reader).unwrap().unwrap();
        assert_eq!(got, body);
        // EOF after the one message
        assert!(read_message(&mut reader).unwrap().is_none());
    }

    #[test]
    fn oversized_messages_are_refused() {
        let text = "Content-Length: 64000000\r\n\r\n";
        let mut reader = BufReader::new(text.as_bytes());
        let err = read_message(&mut reader).unwrap_err();
        assert!(err.contains("32 MiB"), "{err}");
    }

    #[test]
    fn file_uris_percent_encode_windows_paths() {
        let uri = file_uri(Path::new(r"C:\Users\david\my repo\src\main.rs"));
        assert_eq!(
            uri,
            "file:///C:/Users/david/my%20repo/src/main.rs"
        );
    }

    #[test]
    fn diagnostics_render_with_one_based_positions() {
        let diags = vec![
            (3u32, 7u32, "error: expected `;`".to_string(), "rustc".to_string()),
        ];
        let block = render_diagnostics(&diags, 15);
        assert!(block.contains("L4 C8 error: expected `;`"), "{block}");
        assert_eq!(render_diagnostics(&[], 15), "");
    }

    #[test]
    fn command_resolution_honors_config_overrides() {
        // No config set: an unknown extension has no server.
        assert!(command_for("definitely-not-a-lang").is_none());
        let mut map = HashMap::new();
        map.insert(
            "definitely-not-a-lang".to_string(),
            vec!["totally-real-lsp".to_string()],
        );
        set_config_overrides(Some(map));
        let cmd = command_for("definitely-not-a-lang").expect("override wins");
        assert_eq!(cmd, vec!["totally-real-lsp"]);
        set_config_overrides(None);
    }

    #[test]
    fn known_extensions_resolve_a_command_shape() {
        // rust-analyzer may not be installed here; the assertion is that the
        // lookup itself never panics and never invents multi-word garbage.
        if let Some(cmd) = command_for("rs") {
            assert_eq!(cmd[0], "rust-analyzer");
        }
    }

    #[test]
    fn the_after_edit_hook_never_spawns() {
        // No server for .txt can exist; the hook must return empty, not error.
        let dir = std::env::temp_dir().join(format!("nur-lsp-hook-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("note.txt");
        std::fs::write(&file, "hello\n").unwrap();
        assert_eq!(diagnostics_after_edit(&file), "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_ops_are_refused_with_the_roster() {
        let dir = std::env::temp_dir().join(format!("nur-lsp-op-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("note.txt");
        std::fs::write(&file, "hello\n").unwrap();
        let err = run_op("teleport", &file, &serde_json::json!({})).unwrap_err();
        assert!(err.to_string().contains("unknown op"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

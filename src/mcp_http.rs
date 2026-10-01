//! Minimal MCP client over the Streamable HTTP transport.
//!
//! Covers what tool use needs from a remote server: `initialize` (session id
//! and negotiated protocol revision), `tools/list` with pagination, and
//! `tools/call`. A response may arrive as a JSON body or on an SSE stream. On a
//! stream, server pings are answered and every other server request is
//! declined: nur offers remote servers no sampling, roots or elicitation.
//! Spec: <https://modelcontextprotocol.io/specification/2025-06-18/basic/transports>

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read};
use std::time::Duration;

/// Requested protocol revision. A server answers with the revision it speaks;
/// listing and calling tools is the same in every revision in use.
pub const PROTOCOL_VERSION: &str = "2025-06-18";
/// Upper bound on one response body, JSON or SSE.
const MAX_BODY_BYTES: u64 = 16 * 1024 * 1024;
/// Upper bound on `tools/list` pages, so a cursor loop cannot spin forever.
const MAX_TOOL_PAGES: usize = 50;

#[derive(Debug)]
pub enum McpError {
    /// HTTP 401/403: the credential is missing, wrong, expired or revoked.
    Unauthorized(String),
    /// HTTP 404 on a request that carried a session id: the server ended it.
    SessionExpired,
    /// A JSON-RPC error object.
    Rpc { code: i64, message: String },
    /// Connection, timeout, unexpected HTTP status or malformed response.
    Transport(String),
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McpError::Unauthorized(detail) => write!(f, "unauthorized ({detail})"),
            McpError::SessionExpired => write!(f, "the server ended the MCP session"),
            McpError::Rpc { code, message } => write!(f, "server error {code}: {message}"),
            McpError::Transport(detail) => write!(f, "{detail}"),
        }
    }
}

/// Server identity reported by `initialize`.
#[derive(Debug, Clone, Default)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
    pub protocol_version: String,
    pub instructions: Option<String>,
}

/// One initialized session with a remote MCP server.
pub struct Connection {
    http: reqwest::blocking::Client,
    url: String,
    bearer: Option<String>,
    session_id: Option<String>,
    protocol_version: Option<String>,
    next_id: u64,
    pub server: ServerInfo,
}

impl Connection {
    /// Open a session: `initialize`, then `notifications/initialized`.
    pub fn open(url: &str, bearer: Option<&str>, timeout: Duration) -> Result<Self, McpError> {
        let http = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(timeout)
            .user_agent(format!("nur-cli/{}", env!("CARGO_PKG_VERSION")))
            // A redirected POST would replay the bearer credential to a host
            // the user never configured.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| McpError::Transport(e.to_string()))?;
        let mut connection = Connection {
            http,
            url: url.to_string(),
            bearer: bearer.map(str::to_string),
            session_id: None,
            protocol_version: None,
            next_id: 0,
            server: ServerInfo::default(),
        };
        let result = connection.request(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "nur", "version": env!("CARGO_PKG_VERSION")},
            }),
        )?;
        let text = |value: Option<&Value>| {
            value
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        let negotiated = result
            .get("protocolVersion")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
            .unwrap_or(PROTOCOL_VERSION)
            .to_string();
        connection.server = ServerInfo {
            name: text(result.pointer("/serverInfo/name")),
            version: text(result.pointer("/serverInfo/version")),
            protocol_version: negotiated.clone(),
            instructions: result
                .get("instructions")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        };
        connection.protocol_version = Some(negotiated);
        connection.notify("notifications/initialized", json!({}))?;
        Ok(connection)
    }

    /// Every tool the server offers, following `nextCursor` pages.
    pub fn list_tools(&mut self) -> Result<Vec<Value>, McpError> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_TOOL_PAGES {
            let params = match &cursor {
                Some(c) => json!({ "cursor": c }),
                None => json!({}),
            };
            let page = self.request("tools/list", params)?;
            if let Some(items) = page.get("tools").and_then(Value::as_array) {
                tools.extend(items.iter().cloned());
            }
            match page.get("nextCursor").and_then(Value::as_str) {
                Some(next) if !next.is_empty() => cursor = Some(next.to_string()),
                _ => break,
            }
        }
        Ok(tools)
    }

    /// `tools/call`; the raw `CallToolResult` (content, isError, structuredContent).
    pub fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value, McpError> {
        self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        )
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, McpError> {
        self.next_id += 1;
        let id = self.next_id;
        let response = self.post(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))?;
        if self.session_id.is_none() {
            self.session_id = response
                .headers()
                .get("mcp-session-id")
                .and_then(|v| v.to_str().ok())
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string);
        }
        let streamed = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.trim_start().starts_with("text/event-stream"));
        let message = if streamed {
            self.read_stream(response, id)?
        } else {
            let mut body = Vec::new();
            response
                .take(MAX_BODY_BYTES)
                .read_to_end(&mut body)
                .map_err(|e| McpError::Transport(format!("reading {method} response: {e}")))?;
            let value: Value = serde_json::from_slice(&body).map_err(|e| {
                McpError::Transport(format!("{method} returned a non-JSON body: {e}"))
            })?;
            messages(value)
                .into_iter()
                .find(|m| is_response_to(m, id))
                .ok_or_else(|| McpError::Transport(format!("{method} returned no response")))?
        };
        if let Some(error) = message.get("error") {
            return Err(McpError::Rpc {
                code: error.get("code").and_then(Value::as_i64).unwrap_or(0),
                message: error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unspecified error")
                    .to_string(),
            });
        }
        Ok(message.get("result").cloned().unwrap_or(Value::Null))
    }

    fn notify(&self, method: &str, params: Value) -> Result<(), McpError> {
        self.post(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
            .map(drop)
    }

    fn post(&self, body: &Value) -> Result<reqwest::blocking::Response, McpError> {
        let mut request = self
            .http
            .post(&self.url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            )
            .body(body.to_string());
        if let Some(bearer) = &self.bearer {
            request = request.bearer_auth(bearer);
        }
        if let Some(session) = &self.session_id {
            request = request.header("Mcp-Session-Id", session);
        }
        if let Some(version) = &self.protocol_version {
            request = request.header("MCP-Protocol-Version", version);
        }
        let response = request.send().map_err(describe)?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let detail = error_detail(response);
        Err(match status.as_u16() {
            401 | 403 => McpError::Unauthorized(format!("HTTP {}{detail}", status.as_u16())),
            404 if self.session_id.is_some() => McpError::SessionExpired,
            code => McpError::Transport(format!("HTTP {code}{detail}")),
        })
    }

    /// Read SSE events until the response to `id` arrives.
    fn read_stream(
        &self,
        response: reqwest::blocking::Response,
        id: u64,
    ) -> Result<Value, McpError> {
        let mut reader = BufReader::new(response.take(MAX_BODY_BYTES));
        let mut events = SseEvents::default();
        let mut line = Vec::new();
        loop {
            line.clear();
            let read = reader
                .read_until(b'\n', &mut line)
                .map_err(|e| McpError::Transport(format!("reading the event stream: {e}")))?;
            let data = if read == 0 {
                events.finish()
            } else {
                events.push_line(&line)
            };
            if let Some(data) = data {
                let Ok(value) = serde_json::from_str::<Value>(&data) else {
                    continue;
                };
                for message in messages(value) {
                    if is_response_to(&message, id) {
                        return Ok(message);
                    }
                    self.answer_server_request(&message);
                }
            }
            if read == 0 {
                return Err(McpError::Transport(
                    "the event stream ended before the response arrived".into(),
                ));
            }
        }
    }

    /// Reply to a request the server sent on a stream: pong a ping, decline
    /// the rest. Notifications (no id) need no reply.
    fn answer_server_request(&self, message: &Value) {
        let (Some(method), Some(id)) = (
            message.get("method").and_then(Value::as_str),
            message.get("id"),
        ) else {
            return;
        };
        let reply = if method == "ping" {
            json!({ "jsonrpc": "2.0", "id": id, "result": {} })
        } else {
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": format!("nur does not support {method}")},
            })
        };
        let _ = self.post(&reply);
    }
}

/// A JSON-RPC payload may be one message or a batch.
fn messages(value: Value) -> Vec<Value> {
    match value {
        Value::Array(items) => items,
        other => vec![other],
    }
}

fn is_response_to(message: &Value, id: u64) -> bool {
    message.get("method").is_none()
        && message.get("id").and_then(Value::as_u64) == Some(id)
        && (message.get("result").is_some() || message.get("error").is_some())
}

fn describe(error: reqwest::Error) -> McpError {
    let detail = if error.is_timeout() {
        "timed out".to_string()
    } else if error.is_connect() {
        format!("cannot connect: {error}")
    } else {
        error.to_string()
    };
    McpError::Transport(detail)
}

/// A short, single-line excerpt of an error body for the message.
fn error_detail(response: reqwest::blocking::Response) -> String {
    let mut body = Vec::new();
    let _ = response.take(2048).read_to_end(&mut body);
    let text = String::from_utf8_lossy(&body);
    let message = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| {
            v.get("message")
                .or_else(|| v.pointer("/error/message"))
                .or_else(|| v.get("error"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| text.trim().to_string());
    let line: String = message
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(240)
        .collect();
    if line.is_empty() {
        String::new()
    } else {
        format!(": {line}")
    }
}

/// Server-sent event framing: `data:` lines accumulate until a blank line.
#[derive(Default)]
struct SseEvents {
    data: Vec<String>,
}

impl SseEvents {
    /// Feed one line, with or without its line ending. Returns the event's
    /// data when this line completes an event.
    fn push_line(&mut self, raw: &[u8]) -> Option<String> {
        let line = String::from_utf8_lossy(raw);
        let line = line.trim_end_matches(['\n', '\r']);
        if line.is_empty() {
            return self.finish();
        }
        if line.starts_with(':') {
            return None;
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        if field == "data" {
            self.data.push(value.to_string());
        }
        None
    }

    /// Complete a pending event at a blank line or end of stream.
    fn finish(&mut self) -> Option<String> {
        if self.data.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.data).join("\n"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(stream: &str) -> Vec<String> {
        let mut parser = SseEvents::default();
        let mut out = Vec::new();
        for line in stream.split_inclusive('\n') {
            out.extend(parser.push_line(line.as_bytes()));
        }
        out.extend(parser.finish());
        out
    }

    // Failure modes: CRLF endings leak `\r` into JSON, multi-line data is
    // split into separate events, comments/keepalives become events, and an
    // event that ends at EOF without a blank line is lost.
    #[test]
    fn sse_framing_survives_crlf_multiline_comments_and_eof() {
        let stream = ": keepalive\r\nevent: message\r\nid: 7\r\ndata: {\"a\":\r\ndata: 1}\r\n\r\n\
                      data:{\"b\":2}\n\ndata: {\"c\":3}";
        assert_eq!(
            events(stream),
            vec!["{\"a\":\n1}", "{\"b\":2}", "{\"c\":3}"]
        );
        for data in events(stream) {
            assert!(serde_json::from_str::<Value>(&data).is_ok(), "{data:?}");
        }
    }

    // Failure modes: a notification or a server request carrying the same id
    // is mistaken for the response, and a batch hides the response.
    #[test]
    fn only_a_matching_response_completes_a_request() {
        let batch = json!([
            {"jsonrpc": "2.0", "method": "notifications/progress", "params": {}},
            {"jsonrpc": "2.0", "id": 3, "method": "ping"},
            {"jsonrpc": "2.0", "id": 2, "result": {}},
            {"jsonrpc": "2.0", "id": 3, "result": {"ok": true}},
        ]);
        let found: Vec<Value> = messages(batch)
            .into_iter()
            .filter(|m| is_response_to(m, 3))
            .collect();
        assert_eq!(
            found,
            vec![json!({"jsonrpc": "2.0", "id": 3, "result": {"ok": true}})]
        );
        assert!(is_response_to(
            &json!({"jsonrpc": "2.0", "id": 3, "error": {"code": -1, "message": "x"}}),
            3
        ));
    }
}

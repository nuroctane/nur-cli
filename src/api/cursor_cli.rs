//! Cursor Agent CLI transport - auth and chat without a pasted API key.
//!
//! Cursor's public Agent endpoint (`api2.cursor.sh`) is not OpenAI Chat
//! Completions. Nur drives Cursor the t3code way: the logged-in `cursor-agent`
//! binary holds the session (keychain / platform auth), and inference is
//! `cursor-agent -p --output-format stream-json`.

use crate::api::types::{
    ApiResponse, ApiUsage, ContentPart, OutputItem, ResponseAccounting, ResponseRequest,
};
use crate::api::StreamEvent;
use crate::error::{NurError, Result};
use crate::usage::{CostProvenance, TokenUsage};
use serde_json::Value;
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Stored in `auth.json` when the user is signed in via `cursor-agent login`.
/// Not a real Bearer token - ApiClient routes Cursor through this module.
pub const CURSOR_CLI_SESSION_TOKEN: &str = "cursor-cli-session";

pub fn is_cli_session_token(token: &str) -> bool {
    let t = token.trim();
    t == CURSOR_CLI_SESSION_TOKEN || t.starts_with("cursor-cli:")
}

/// How to launch Cursor Agent without Windows stdout deadlocks.
///
/// Current Cursor builds initialize their stdio/worker bridge in the supported
/// wrapper. Invoking versioned `node.exe index.js` directly can complete and
/// persist a turn while emitting zero bytes and leaving the worker alive. Use
/// the installed wrapper when present; direct Node remains a last-resort path
/// for incomplete installs.
#[derive(Debug, Clone)]
enum CursorLaunch {
    /// Direct Node entry (last-resort fallback).
    Node { node: PathBuf, index: PathBuf },
    /// Fallback wrapper script / binary on PATH.
    Wrapper(PathBuf),
}

/// Resolve a path useful for diagnostics (Node `index.js` or wrapper script).
#[allow(dead_code)]
pub fn cursor_agent_bin() -> Option<PathBuf> {
    resolve_launch().map(|l| match l {
        CursorLaunch::Node { index, .. } => index,
        CursorLaunch::Wrapper(p) => p,
    })
}

fn resolve_launch() -> Option<CursorLaunch> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        #[cfg(windows)]
        {
            // PowerShell resolves `cursor-agent` to the ps1 wrapper on this
            // platform. Prefer it over the cmd shim (which merely adds another
            // shell) and over unsupported direct index.js execution.
            for name in ["cursor-agent.exe", "cursor-agent.ps1", "cursor-agent.cmd"] {
                let p = dir.join(name);
                if p.is_file() {
                    return Some(CursorLaunch::Wrapper(p));
                }
            }
        }
        let c = dir.join("cursor-agent");
        if c.is_file() {
            if let Some(parent) = c.parent() {
                if let Some(node) = resolve_node_launch(parent) {
                    return Some(node);
                }
            }
            return Some(CursorLaunch::Wrapper(c));
        }
    }
    // Default install location even if PATH is stale.
    #[cfg(windows)]
    {
        if let Some(local) = dirs::data_local_dir() {
            let dir = local.join("cursor-agent");
            for name in ["cursor-agent.exe", "cursor-agent.ps1", "cursor-agent.cmd"] {
                let wrapper = dir.join(name);
                if wrapper.is_file() {
                    return Some(CursorLaunch::Wrapper(wrapper));
                }
            }
            if let Some(node) = resolve_node_launch(&dir) {
                return Some(node);
            }
        }
    }
    None
}

/// Match `cursor-agent.ps1`: same-dir node.exe, else newest `versions/*`.
fn resolve_node_launch(script_dir: &std::path::Path) -> Option<CursorLaunch> {
    let node = script_dir.join("node.exe");
    let index = script_dir.join("index.js");
    if node.is_file() && index.is_file() {
        return Some(CursorLaunch::Node { node, index });
    }
    let versions = script_dir.join("versions");
    if !versions.is_dir() {
        return None;
    }
    let mut dirs: Vec<_> = std::fs::read_dir(&versions)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.path())
        .filter(|p| p.join("node.exe").is_file() && p.join("index.js").is_file())
        .collect();
    // Names are YYYY.MM.DD-… — lexicographic descending picks newest.
    dirs.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    let best = dirs.into_iter().next()?;
    Some(CursorLaunch::Node {
        node: best.join("node.exe"),
        index: best.join("index.js"),
    })
}

fn spawn_agent(args: &[&str]) -> std::io::Result<std::process::Child> {
    let launch = resolve_launch().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "cursor-agent not found on PATH",
        )
    })?;
    spawn_launch(&launch, args)
}

fn spawn_launch(launch: &CursorLaunch, args: &[&str]) -> std::io::Result<std::process::Child> {
    #[cfg(windows)]
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    match launch {
        CursorLaunch::Node { node, index } => {
            let mut c = Command::new(node);
            c.arg(index).args(args);
            c.env("CURSOR_INVOKED_AS", "cursor-agent");
            if std::env::var_os("NODE_COMPILE_CACHE").is_none() {
                if let Some(local) = dirs::data_local_dir() {
                    c.env("NODE_COMPILE_CACHE", local.join("cursor-compile-cache"));
                }
            }
            c.stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                c.creation_flags(CREATE_NO_WINDOW);
            }
            c.spawn()
        }
        CursorLaunch::Wrapper(bin) => {
            #[cfg(windows)]
            {
                let lower = bin.to_string_lossy().to_ascii_lowercase();
                let mut cmd = if lower.ends_with(".cmd") || lower.ends_with(".bat") {
                    // Last resort — prefer Node launch above. cmd→ps1 buffers pipes.
                    let mut c = Command::new("cmd.exe");
                    c.arg("/D").arg("/C").arg(bin);
                    for a in args {
                        c.arg(a);
                    }
                    c
                } else if lower.ends_with(".ps1") {
                    let mut c = Command::new("powershell.exe");
                    c.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                        .arg(bin);
                    for a in args {
                        c.arg(a);
                    }
                    c
                } else {
                    let mut c = Command::new(bin);
                    c.args(args);
                    c
                };
                // Wrapper shells (powershell/cmd/exe) end up running the same
                // Node CLI underneath - give them the identical compile-cache
                // bump the direct Node launch gets so spawns stay fast.
                if std::env::var_os("NODE_COMPILE_CACHE").is_none() {
                    if let Some(local) = dirs::data_local_dir() {
                        cmd.env("NODE_COMPILE_CACHE", local.join("cursor-compile-cache"));
                    }
                }
                cmd.stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                {
                    use std::os::windows::process::CommandExt;
                    cmd.creation_flags(CREATE_NO_WINDOW);
                }
                cmd.spawn()
            }
            #[cfg(not(windows))]
            {
                let mut c = Command::new(bin);
                c.args(args);
                if std::env::var_os("NODE_COMPILE_CACHE").is_none() {
                    if let Some(local) = dirs::data_local_dir() {
                        c.env("NODE_COMPILE_CACHE", local.join("cursor-compile-cache"));
                    }
                }
                c.stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
            }
        }
    }
}

fn join_capture_bounded(
    handle: std::thread::JoinHandle<Vec<u8>>,
    cap: Duration,
) -> Result<Vec<u8>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(handle.join().ok());
    });
    match rx.recv_timeout(cap) {
        Ok(Some(buf)) => Ok(buf),
        Ok(None) => Ok(Vec::new()),
        Err(_) => Err(NurError::Other(
            "cursor-agent status drain timed out".into(),
        )),
    }
}

fn run_capture(args: &[&str]) -> Result<String> {
    let mut child = spawn_agent(args)
        .map_err(|e| NurError::Other(format!("failed to launch cursor-agent: {e}")))?;
    let _ = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let out_h = stdout.map(|mut pipe| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = std::io::Read::read_to_end(&mut pipe, &mut buf);
            buf
        })
    });
    let err_h = stderr.map(|mut pipe| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = std::io::Read::read_to_end(&mut pipe, &mut buf);
            buf
        })
    });
    const CAPTURE_TIMEOUT: Duration = Duration::from_secs(20);
    let deadline = Instant::now() + CAPTURE_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(NurError::Other(
                    "cursor-agent status timed out after 20s".into(),
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(NurError::Other(format!("cursor-agent failed: {e}"))),
        }
    };
    // Grandchildren can keep inherited pipes open after the wrapper exits
    // (same wedge as bash drain). Bound the join so auth checks cannot hang.
    const DRAIN_CAP: Duration = Duration::from_secs(2);
    let stdout_bytes = match out_h {
        Some(h) => join_capture_bounded(h, DRAIN_CAP)?,
        None => Vec::new(),
    };
    let stderr_bytes = match err_h {
        Some(h) => join_capture_bounded(h, DRAIN_CAP)?,
        None => Vec::new(),
    };
    if !status.success() {
        let err = String::from_utf8_lossy(&stderr_bytes);
        return Err(NurError::Other(format!(
            "cursor-agent exited {}: {}",
            status,
            err.chars().take(300).collect::<String>()
        )));
    }
    Ok(String::from_utf8_lossy(&stdout_bytes).into_owned())
}

/// Brief cache so every chat turn does not pay for a fresh `status` spawn.
fn auth_cache() -> &'static std::sync::Mutex<(std::time::Instant, bool)> {
    use std::sync::OnceLock;
    static CACHE: OnceLock<std::sync::Mutex<(std::time::Instant, bool)>> = OnceLock::new();
    CACHE.get_or_init(|| {
        std::sync::Mutex::new((
            std::time::Instant::now()
                .checked_sub(std::time::Duration::from_secs(600))
                .unwrap_or_else(std::time::Instant::now),
            false,
        ))
    })
}

/// `cursor-agent status --format json` → authenticated?
pub fn cli_is_authenticated() -> bool {
    {
        let guard = auth_cache().lock().unwrap_or_else(|e| e.into_inner());
        if guard.0.elapsed() < std::time::Duration::from_secs(600) {
            return guard.1;
        }
    }
    if resolve_launch().is_none() {
        return false;
    }
    let ok = match run_capture(&["status", "--format", "json"]) {
        Ok(text) => parse_status_authenticated(&text),
        Err(_) => {
            // A hung wrapper must not flip the cache to logged-out and must not
            // retry on every subsequent turn.
            let mut guard = auth_cache().lock().unwrap_or_else(|e| e.into_inner());
            guard.0 = Instant::now();
            return guard.1;
        }
    };
    if let Ok(mut guard) = auth_cache().lock() {
        *guard = (std::time::Instant::now(), ok);
    }
    ok
}

pub fn parse_status_authenticated(text: &str) -> bool {
    let text = text.trim();
    if text.is_empty() {
        return false;
    }
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        if v.get("isAuthenticated").and_then(|x| x.as_bool()) == Some(true) {
            return true;
        }
        if v.get("status").and_then(|x| x.as_str()) == Some("authenticated") {
            return true;
        }
        if v.get("hasAccessToken").and_then(|x| x.as_bool()) == Some(true) {
            return true;
        }
    }
    let lower = text.to_ascii_lowercase();
    lower.contains("logged in") && !lower.contains("not logged")
}

/// Tokens representing a live `cursor-agent` login (no secret copied into nur).
pub fn session_tokens_from_cli() -> Option<crate::oauth::OAuthTokens> {
    if !cli_is_authenticated() {
        return None;
    }
    Some(crate::oauth::OAuthTokens {
        access_token: CURSOR_CLI_SESSION_TOKEN.into(),
        refresh_token: Some("cursor-agent".into()),
        expires_at: None,
        meta: Some(crate::auth::OauthMeta {
            issuer: "cursor".into(),
            client_id: "cursor-agent".into(),
            extra: serde_json::json!({
                "imported_from": "cursor-agent-status",
                "cli_session": true,
            }),
        }),
    })
}

/// List model ids from `cursor-agent models` / `--list-models`.
pub fn list_models() -> Result<Vec<String>> {
    if resolve_launch().is_none() {
        return Err(NurError::Other(
            "cursor-agent not found on PATH. Install Cursor Agent, then run `cursor-agent login`."
                .into(),
        ));
    }
    if !cli_is_authenticated()
        && std::env::var("CURSOR_API_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty())
            .is_none()
    {
        return Err(NurError::Other(
            "Cursor Agent is not logged in. Run /login → Cursor → Sign in with browser \
             (or `cursor-agent login`)."
                .into(),
        ));
    }
    let output = run_capture(&["models"]).or_else(|_| run_capture(&["--list-models"]))?;
    let mut ids = parse_model_list(&output);
    if ids.is_empty() {
        ids.push("auto".into());
    }
    Ok(ids)
}

fn parse_model_list(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        if let Some(arr) = v
            .as_array()
            .or_else(|| v.get("models").and_then(|m| m.as_array()))
        {
            for item in arr {
                let id = item
                    .as_str()
                    .or_else(|| item.get("id").and_then(|x| x.as_str()))
                    .or_else(|| item.get("name").and_then(|x| x.as_str()));
                if let Some(id) = id {
                    if !out.iter().any(|x| x == id) {
                        out.push(id.to_string());
                    }
                }
            }
        }
        if !out.is_empty() {
            return out;
        }
    }
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cleaned = line
            .trim_start_matches(['-', '*', '•', ' '])
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_matches(|c: char| c == '`' || c == '"' || c == '\'');
        if cleaned.is_empty() || cleaned.starts_with("http") {
            continue;
        }
        if cleaned.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '[' | ']' | '=' | ',')
        }) && cleaned.len() < 80
            && !out.iter().any(|x: &String| x == cleaned)
        {
            out.push(cleaned.to_string());
        }
    }
    out
}

/// When set (`1`/`true`/`yes`), Cursor runs as a native Agent CLI delegate
/// (`--force`) and ignores nur's tool harness. Default is nur-tools protocol so
/// subagents, plan mode, approvals, and the rest of the agent loop keep working.
pub fn native_delegate_enabled() -> bool {
    std::env::var("NUR_CURSOR_NATIVE")
        .ok()
        .is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
}

fn tools_wire(req: &ResponseRequest) -> bool {
    req.tools.as_ref().is_some_and(|t| !t.is_empty()) && !native_delegate_enabled()
}

/// Flatten a Responses-shaped request into a text prompt for the Agent CLI.
pub fn flatten_prompt(req: &ResponseRequest) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(instr) = &req.instructions {
        let t = instr.trim();
        if !t.is_empty() {
            parts.push(format!("System:\n{t}"));
        }
    }
    if tools_wire(req) {
        parts.push(nur_tools_protocol_block(req));
    } else if req.tools.as_ref().is_some_and(|t| !t.is_empty()) {
        // Native Cursor Agent delegate (NUR_CURSOR_NATIVE): Cursor owns tools.
        parts.push(
            "Note: You are running as Cursor Agent with your own tools. \
             Complete the user's task end-to-end (read, edit, shell as needed). \
             Reply with a clear final summary of what you did."
                .into(),
        );
    }
    if let Value::Array(items) = &req.input {
        for item in items {
            let ty = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
            match ty {
                "message" => {
                    let role = item.get("role").and_then(|r| r.as_str()).unwrap_or("user");
                    let text = message_text(item);
                    if !text.trim().is_empty() {
                        parts.push(format!(
                            "{}:\n{}",
                            match role {
                                "assistant" => "Assistant",
                                "system" | "developer" => "System",
                                _ => "User",
                            },
                            text.trim()
                        ));
                    }
                }
                "function_call" => {
                    let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("tool");
                    let args = item
                        .get("arguments")
                        .map(|a| match a {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        })
                        .unwrap_or_default();
                    parts.push(format!("Tool call ({name}):\n{args}"));
                }
                "function_call_output" | "tool_result" => {
                    let text = item
                        .get("output")
                        .or_else(|| item.get("content"))
                        .map(|o| match o {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        })
                        .unwrap_or_default();
                    parts.push(format!("Tool result:\n{text}"));
                }
                "reasoning" => {}
                _ => {
                    let text = message_text(item);
                    if !text.trim().is_empty() {
                        parts.push(text);
                    }
                }
            }
        }
    } else if let Value::String(s) = &req.input {
        parts.push(s.clone());
    }
    if parts.is_empty() {
        parts.push("Respond briefly.".into());
    }
    parts.join("\n\n")
}

/// Cursor chat chain state, owned by the [`crate::api::client::ApiClient`] and
/// threaded into every `cursor_cli` call. The CLI's `--resume <session_id>`
/// recalls the whole prior conversation server-side (with cache reads), so each
/// round only ships the NEW user messages and tool results instead of
/// re-flattening the full history into a fresh spawn. That is what grew input
/// tokens to 157k over 48 rounds in session 26940d90.
///
/// Scoping: the client persists across rounds and turns for one runner (the
/// TUI's main conversation; each subagent gets its own client), so the chat id
/// scopes correctly. `/model` or `/provider` rebuilds the client, which resets
/// the chain - the next round then full-flattens fresh.
#[derive(Default)]
pub(crate) struct CursorChatState {
    /// Session id echoed by the CLI's `system`/`init` event; `None` until the
    /// first successful round (or after a failure reset).
    pub session_id: Option<String>,
    /// How many items of the request's `input` array the resumed session has
    /// already seen.
    pub items_sent: usize,
}

/// Incremental prompt for a `--resume` round: only the messages and tool
/// results since the last reply. Cursor's own session history already contains
/// the assistant turns, tool calls, and reasoning, so re-sending them would
/// duplicate every round.
pub(crate) fn flatten_incremental(items: &[Value], tools: bool) -> String {
    let mut parts: Vec<String> = vec![
        "Continuing the same nur session - new messages and tool results since your last reply:"
            .into(),
    ];
    if tools {
        parts.push(
            "More tools needed: end your reply with the same ```nur-tools fence (JSON array of \
             {name,arguments}). Final answer: plain text, no fence."
                .into(),
        );
    }
    let mut pushed = 0usize;
    for item in items {
        let ty = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match ty {
            "message" => {
                let role = item.get("role").and_then(|r| r.as_str()).unwrap_or("user");
                if role != "user" {
                    continue; // assistant history lives in Cursor's session
                }
                let text = message_text(item);
                if !text.trim().is_empty() {
                    parts.push(format!("User:\n{}", text.trim()));
                    pushed += 1;
                }
            }
            "function_call_output" | "tool_result" => {
                let text = item
                    .get("output")
                    .or_else(|| item.get("content"))
                    .map(|o| match o {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    })
                    .unwrap_or_default();
                parts.push(format!("Tool result:\n{text}"));
                pushed += 1;
            }
            // Assistant messages, function_call items, and reasoning are already
            // recorded in the resumed session's own history.
            _ => {}
        }
    }
    if pushed == 0 {
        parts.push("Respond briefly.".into());
    }
    parts.join("\n\n")
}

fn nur_tools_protocol_block(req: &ResponseRequest) -> String {
    let mut out = String::from(
        "You are the model behind the nur agent harness. Nur owns tools, approvals, \
         plan mode, and subagents - you do NOT edit files yourself.\n\
         When you need a tool, end your reply with EXACTLY this fence (JSON array):\n\
         ```nur-tools\n\
         [{\"name\":\"tool_name\",\"arguments\":{...}}]\n\
         ```\n\
         You may put brief commentary before the fence. For a final answer with no \
         more tools, reply in plain text and do NOT include a nur-tools fence.\n\
         Available tools:\n",
    );
    if let Some(tools) = &req.tools {
        for t in tools {
            let desc = t.description.as_deref().unwrap_or("");
            let params = t
                .parameters
                .as_ref()
                .map(|p| p.to_string())
                .unwrap_or_else(|| "{}".into());
            // Keep schemas bounded so Windows cmdline / prompt size stay sane.
            let params_short: String = params.chars().take(1200).collect();
            out.push_str(&format!(
                "- {} — {}\n  params: {}\n",
                t.name, desc, params_short
            ));
        }
    }
    out
}

/// Split `text` into (commentary, tool calls) when a ````nur-tools` fence is
/// present. Tolerant by design: models add info suffixes ("```nur-tools json"),
/// trailing prose inside the fence, or a single JSON object instead of the
/// array - all of those still carry the call, so recover them instead of
/// treating the turn as a final answer. Returns the first fence that yields at
/// least one call; `None` means a plain final answer.
pub fn split_nur_tools(text: &str) -> Option<(String, Vec<(String, String)>)> {
    const START: &str = "```nur-tools";
    let mut search_from = 0usize;
    while let Some(rel) = text[search_from..].find(START) {
        let start_idx = search_from + rel;
        let marker_end = start_idx + START.len();
        // The rest of the fence line is an info suffix ("nur-tools json") -
        // ignore it, unless the JSON itself starts on that line (some models
        // put the array right after the marker).
        let line_end = text[marker_end..]
            .find('\n')
            .map(|i| marker_end + i)
            .unwrap_or(text.len());
        let body_start = match text[marker_end..line_end].find(['[', '{']) {
            Some(i) => marker_end + i,
            None => (line_end + 1).min(text.len()),
        };
        let commentary = text[..start_idx].trim().to_string();
        // Body runs to the next closing fence (or end of text).
        let (body_end, next_from) = match text[body_start..].find("```") {
            Some(i) => (body_start + i, (body_start + i + 3).min(text.len())),
            None => (text.len(), text.len()),
        };
        let json_body = text[body_start..body_end].trim();
        if let Some(calls) = parse_tool_calls(json_body) {
            return Some((commentary, calls));
        }
        search_from = next_from.max(marker_end);
    }
    None
}

/// Parse a fence body into tool calls, escalating tolerance: strict JSON
/// array, then the substring from the first `[` to the last `]` (trailing
/// prose inside the fence), then a single `{name, arguments|input}` object.
fn parse_tool_calls(json_body: &str) -> Option<Vec<(String, String)>> {
    if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(json_body) {
        let calls = calls_from_array(&arr);
        if !calls.is_empty() {
            return Some(calls);
        }
    }
    if let (Some(a), Some(b)) = (json_body.find('['), json_body.rfind(']')) {
        if a < b {
            if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(&json_body[a..=b]) {
                let calls = calls_from_array(&arr);
                if !calls.is_empty() {
                    return Some(calls);
                }
            }
        }
    }
    if let (Some(a), Some(b)) = (json_body.find('{'), json_body.rfind('}')) {
        if a < b {
            if let Ok(obj) = serde_json::from_str::<Value>(&json_body[a..=b]) {
                if let Some(call) = call_from_item(&obj) {
                    return Some(vec![call]);
                }
            }
        }
    }
    None
}

fn calls_from_array(arr: &[Value]) -> Vec<(String, String)> {
    arr.iter().filter_map(call_from_item).collect()
}

fn call_from_item(item: &Value) -> Option<(String, String)> {
    let name = item
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if name.is_empty() {
        return None;
    }
    let args = match item.get("arguments").or_else(|| item.get("input")) {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => "{}".into(),
    };
    Some((name, args))
}

fn message_text(item: &Value) -> String {
    if let Some(s) = item.get("content").and_then(|c| c.as_str()) {
        return s.to_string();
    }
    if let Some(arr) = item.get("content").and_then(|c| c.as_array()) {
        let mut out = String::new();
        for part in arr {
            if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(t);
            } else if let Some(t) = part.as_str() {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(t);
            }
        }
        return out;
    }
    String::new()
}

fn response_from_cli_text(
    model: &str,
    text: &str,
    parse_tools: bool,
    usage: Option<ApiUsage>,
) -> ApiResponse {
    let id = format!("cursor-cli-{}", uuid_simple());
    if parse_tools {
        if let Some((commentary, calls)) = split_nur_tools(text) {
            let mut output = Vec::new();
            if !commentary.is_empty() {
                output.push(OutputItem::Message {
                    id: None,
                    role: Some("assistant".into()),
                    status: Some("completed".into()),
                    content: vec![ContentPart {
                        type_: "output_text".into(),
                        text: Some(commentary),
                    }],
                    phase: Some("commentary".into()),
                    reasoning_content: None,
                });
            }
            for (i, (name, args)) in calls.into_iter().enumerate() {
                let call_id = format!("call_{id}_{i}");
                output.push(OutputItem::FunctionCall {
                    id: Some(call_id.clone()),
                    call_id: Some(call_id),
                    name: Some(name),
                    arguments: Some(args),
                    status: Some("completed".into()),
                });
            }
            return ApiResponse {
                id: Some(id),
                status: Some("completed".into()),
                model: Some(model.to_string()),
                output,
                usage,
                error: None,
                accounting: Some(ResponseAccounting {
                    // The actual input estimate is attached by ApiClient where
                    // the full request is available. This placeholder means
                    // Cursor's subscription usage is unavailable, never zero.
                    estimated_usage: TokenUsage {
                        cost_provenance: CostProvenance::SubscriptionUnknown,
                        ..TokenUsage::unknown()
                    },
                    native_cost_usd: None,
                    upstream_provider: Some("cursor".into()),
                }),
            };
        }
    }
    ApiResponse {
        id: Some(id),
        status: Some("completed".into()),
        model: Some(model.to_string()),
        output: vec![OutputItem::Message {
            id: None,
            role: Some("assistant".into()),
            status: Some("completed".into()),
            content: vec![ContentPart {
                type_: "output_text".into(),
                text: Some(text.to_string()),
            }],
            phase: None,
            reasoning_content: None,
        }],
        usage,
        error: None,
        accounting: Some(ResponseAccounting {
            estimated_usage: TokenUsage {
                cost_provenance: CostProvenance::SubscriptionUnknown,
                ..TokenUsage::unknown()
            },
            native_cost_usd: None,
            upstream_provider: Some("cursor".into()),
        }),
    }
}

fn uuid_simple() -> String {
    format!(
        "{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    )
}

/// Run a non-streaming Cursor Agent completion.
pub fn complete(
    req: &ResponseRequest,
    cancel: &tokio_util::sync::CancellationToken,
    chat: Option<&Arc<Mutex<CursorChatState>>>,
) -> Result<ApiResponse> {
    let parse_tools = tools_wire(req);
    let (text, model, usage) = run_print(req, None, cancel, provider_turn_timeout(), chat)?;
    Ok(response_from_cli_text(&model, &text, parse_tools, usage))
}

/// Stream Cursor Agent output; `on_event` receives text deltas.
pub fn complete_stream(
    req: &ResponseRequest,
    mut on_event: impl FnMut(StreamEvent),
    cancel: &tokio_util::sync::CancellationToken,
    chat: Option<&Arc<Mutex<CursorChatState>>>,
) -> Result<ApiResponse> {
    let parse_tools = tools_wire(req);
    // Cursor can spend tens of seconds booting configured MCP servers before
    // its first JSON event. Give the TUI immediate, honest progress instead of
    // making turn 1 look dead.
    on_event(StreamEvent::ReasoningDelta(
        "cursor-agent · launching\n".into(),
    ));
    // Always stream progress. When nur owns tools, route live tokens to the
    // thinking cell so turn 1 is not a silent hang; final commentary lands as
    // TextDelta after the nur-tools fence is stripped.
    //
    // Total cap sits 30s above the agent-loop select (same
    // NUR_PROVIDER_TURN_TIMEOUT_SECS, default 300) so the loop's token fires
    // first and cancels us cleanly instead of racing this internal kill.
    let (text, model, usage) = run_print(
        req,
        Some(&mut |ev| {
            if parse_tools {
                match ev {
                    StreamEvent::TextDelta(d) => on_event(StreamEvent::ReasoningDelta(d)),
                    other => on_event(other),
                }
            } else {
                on_event(ev);
            }
        }),
        cancel,
        provider_turn_timeout() + Duration::from_secs(30),
        chat,
    )?;
    let resp = response_from_cli_text(&model, &text, parse_tools, usage);
    if parse_tools {
        let commentary = resp.output_text();
        if !commentary.is_empty() {
            on_event(StreamEvent::TextDelta(commentary));
        }
    }
    on_event(StreamEvent::Completed(resp.clone()));
    Ok(resp)
}

/// Cursor's Windows headless CLI has shipped versions that finish a turn and
/// persist it successfully while emitting zero bytes to redirected stdout.
/// Snapshot the durable transcript set before launch so that a newly completed
/// turn can be recovered without ever confusing it with an older session.
struct TranscriptSnapshot {
    known: HashSet<PathBuf>,
}

impl TranscriptSnapshot {
    fn capture() -> Self {
        Self {
            known: cursor_transcript_paths().into_iter().collect(),
        }
    }

    fn recover(&self, prompt: &str) -> Option<String> {
        let mut candidates: Vec<_> = cursor_transcript_paths()
            .into_iter()
            .filter(|path| !self.known.contains(path))
            .filter_map(|path| {
                let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
                Some((modified, path))
            })
            .collect();
        candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.0));
        candidates.into_iter().find_map(|(_, path)| {
            let text = std::fs::read_to_string(path).ok()?;
            parse_completed_transcript(&text, prompt)
        })
    }
}

fn cursor_transcript_paths() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let root = home.join(".cursor").join("projects");
    if !root.is_dir() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let Ok(projects) = std::fs::read_dir(root) else {
        return out;
    };
    for project in projects.flatten() {
        let transcripts = project.path().join("agent-transcripts");
        let Ok(sessions) = std::fs::read_dir(transcripts) else {
            continue;
        };
        for session in sessions.flatten() {
            let name = session.file_name();
            let path = session.path().join(name).with_extension("jsonl");
            if path.is_file() {
                out.push(path);
            }
        }
    }
    out
}

fn transcript_message_text(value: &Value) -> String {
    value
        .pointer("/message/content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("")
}

fn parse_completed_transcript(jsonl: &str, prompt: &str) -> Option<String> {
    let signature: String = prompt
        .trim()
        .chars()
        .rev()
        .take(240)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    let mut prompt_matches = signature.is_empty();
    let mut assistant = String::new();
    let mut completed = false;
    for line in jsonl.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match value.get("role").and_then(Value::as_str) {
            Some("user") => {
                if transcript_message_text(&value).contains(&signature) {
                    prompt_matches = true;
                }
            }
            Some("assistant") => assistant.push_str(&transcript_message_text(&value)),
            _ => {}
        }
        if value.get("type").and_then(Value::as_str) == Some("turn_ended")
            && value.get("status").and_then(Value::as_str) == Some("success")
        {
            completed = true;
        }
    }
    if completed && prompt_matches && !assistant.trim().is_empty() {
        Some(assistant)
    } else {
        None
    }
}

fn provider_turn_timeout() -> Duration {
    std::env::var("NUR_PROVIDER_TURN_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(300))
}

/// Idle cutoff: no stdout line for this long → kill, try transcript recovery,
/// error if none. Catches Cursor's Windows silent-spawn hang far earlier than
/// the total cap, and doubles as the trigger condition for the resume retry.
fn cursor_idle_timeout() -> Duration {
    std::env::var("NUR_CURSOR_IDLE_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(120))
}

/// Failures that mean a `--resume` attempt produced nothing the CLI will ever
/// surface: a bogus session id does not error, it just hangs silently until an
/// idle/total cap, or exits with no assistant text.
fn is_retryable_resume_failure(err: &NurError) -> bool {
    let msg = err.to_string();
    msg.contains("idle timeout")
        || msg.contains("provider turn timeout")
        || msg.contains("produced no assistant text")
}

fn run_print(
    req: &ResponseRequest,
    mut on_event: Option<&mut dyn FnMut(StreamEvent)>,
    cancel: &tokio_util::sync::CancellationToken,
    total_timeout: Duration,
    chat: Option<&Arc<Mutex<CursorChatState>>>,
) -> Result<(String, String, Option<ApiUsage>)> {
    if resolve_launch().is_none() {
        return Err(NurError::Other(
            "cursor-agent not found on PATH. Install Cursor Agent (https://cursor.com/docs/cli)."
                .into(),
        ));
    }
    let has_api_key = std::env::var("CURSOR_API_KEY")
        .ok()
        .is_some_and(|k| !k.trim().is_empty());
    if !cli_is_authenticated() && !has_api_key {
        return Err(NurError::Other(
            "Cursor Agent is not logged in. Run /login → Cursor → Sign in with browser \
             (`cursor-agent login`), or set CURSOR_API_KEY."
                .into(),
        ));
    }

    let items_len = match &req.input {
        Value::Array(items) => items.len(),
        _ => 0,
    };
    // Resume the same CLI session when the chain holds an id and the
    // conversation has grown since the last completed round.
    let (resume_id, resume_from): (Option<String>, usize) = match chat.and_then(|c| c.lock().ok()) {
        Some(state) if state.session_id.is_some() && items_len > state.items_sent => {
            (state.session_id.clone(), state.items_sent)
        }
        _ => (None, 0),
    };
    let mut retried_resume = false;
    loop {
        let attempt_resume = if retried_resume {
            None
        } else {
            resume_id.clone()
        };
        let prompt = match &attempt_resume {
            Some(_) => {
                let items = req.input.as_array().cloned().unwrap_or_default();
                let from = resume_from.min(items.len());
                flatten_incremental(&items[from..], tools_wire(req))
            }
            None => flatten_prompt(req),
        };
        let attempt = run_print_once(
            &prompt,
            req,
            attempt_resume.as_deref(),
            &mut on_event,
            cancel,
            total_timeout,
        );
        match attempt {
            Ok((text, model, usage, captured_session)) => {
                if let Some(chat) = chat {
                    if let Ok(mut st) = chat.lock() {
                        st.session_id = captured_session.or_else(|| resume_id.clone());
                        st.items_sent = items_len;
                    }
                }
                return Ok((text, model, usage));
            }
            Err(e) => {
                // Any failure resets the chain so the next round full-flattens.
                if let Some(chat) = chat {
                    if let Ok(mut st) = chat.lock() {
                        st.session_id = None;
                        st.items_sent = 0;
                    }
                }
                // A stale/bogus session id hangs the CLI silently; retry a
                // resume failure once in full mode before surfacing the error.
                let retryable = is_retryable_resume_failure(&e);
                if attempt_resume.is_some() && !retried_resume && retryable {
                    retried_resume = true;
                    continue;
                }
                return Err(e);
            }
        }
    }
}

/// One spawn of `cursor-agent -p`. `prompt` is the already-flattened text;
/// `resume_id` (when set) adds `--resume <id>` so the CLI recalls its own
/// session instead of nur re-sending the whole conversation. The callback
/// Option is passed by exclusive reference so the retry loop can hand it to
/// each attempt (a trait object behind `&mut` is invariant, so reborrows of
/// `Option<&mut dyn FnMut>` cannot be shortened per iteration).
fn run_print_once(
    prompt: &str,
    req: &ResponseRequest,
    resume_id: Option<&str>,
    on_event: &mut Option<&mut dyn FnMut(StreamEvent)>,
    cancel: &tokio_util::sync::CancellationToken,
    total_timeout: Duration,
) -> Result<(String, String, Option<ApiUsage>, Option<String>)> {
    let model = if req.model.trim().is_empty() || req.model == "auto" {
        None
    } else {
        Some(req.model.trim().to_string())
    };

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let cwd_s = cwd.to_string_lossy().into_owned();
    let harness_tools = tools_wire(req);
    let native = req.tools.as_ref().is_some_and(|t| !t.is_empty()) && native_delegate_enabled();

    // Default: ask mode so nur owns tools/approvals/subagents via nur-tools fence.
    // NUR_CURSOR_NATIVE=1: full Cursor Agent (`--force`) like t3code delegate.
    // Always stream-json — text mode + redirected pipes can hang on Windows.
    let mut owned: Vec<String> = vec![
        "-p".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--stream-partial-output".into(),
        "--trust".into(),
        // A configured MCP server otherwise opens an approval prompt that has
        // no interactive console in `--print` mode and waits forever on turn 1.
        // This approves server startup only; Nur still owns tool permissions in
        // ask mode unless NUR_CURSOR_NATIVE explicitly delegates them.
        "--approve-mcps".into(),
        "--sandbox".into(),
        "disabled".into(),
        "--workspace".into(),
        cwd_s,
    ];
    if native {
        owned.push("--force".into());
    } else {
        owned.push("--mode".into());
        owned.push("ask".into());
    }
    let _ = harness_tools;
    if let Some(ref m) = model {
        owned.push("--model".into());
        owned.push(m.clone());
    }
    if let Some(id) = resume_id {
        // Resume the CLI's own session: it recalls the prior conversation
        // server-side (with cache reads) so nur only ships the delta.
        owned.push("--resume".into());
        owned.push(id.to_string());
    }

    // Windows CreateProcess cmdline ~8191 chars. Prefer stdin for the prompt
    // when large; also pass a short arg so CLIs that ignore stdin still work.
    const ARG_BUDGET: usize = 4_000;
    let pass_as_arg = prompt.len() <= ARG_BUDGET;
    if pass_as_arg {
        owned.push(prompt.to_string());
    }

    let transcript = TranscriptSnapshot::capture();
    let arg_refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    let mut child = spawn_agent(&arg_refs)
        .map_err(|e| NurError::Other(format!("failed to launch cursor-agent: {e}")))?;

    if let Some(mut stdin) = child.stdin.take() {
        if !pass_as_arg {
            let _ = stdin.write_all(prompt.as_bytes());
            let _ = stdin.write_all(b"\n");
        }
        drop(stdin);
    }

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| NurError::Other("cursor-agent stdout missing".into()))?;
    let stderr_pipe = child.stderr.take();
    let (err_tx, err_rx) = std::sync::mpsc::channel();
    let _stderr_reader = std::thread::spawn(move || {
        if let Some(err) = stderr_pipe {
            for line in BufReader::new(err).lines() {
                if err_tx.send(line).is_err() {
                    break;
                }
            }
        }
    });

    let (line_tx, line_rx) = std::sync::mpsc::channel();
    let _stdout_reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if line_tx.send(line).is_err() {
                break;
            }
        }
    });

    let started = Instant::now();
    let timeout = total_timeout;
    let idle_timeout = cursor_idle_timeout();
    // Probe the durable transcripts only after the CLI had a fair chance to
    // produce stream-json (15s), then every 2s - polling ~/.cursor/projects
    // on Windows is not free.
    let mut next_transcript_probe = started + Duration::from_secs(15);
    let mut final_text = String::new();
    let mut streamed = String::new();
    let mut recovered = None;
    let mut cli_error = None;
    let mut status = None;
    let mut last_stdout = Instant::now();
    // Any stdout line counts as liveness, not just assistant chunks.
    let mut last_activity = Instant::now();
    let mut err_text = String::new();
    let mut native_usage = None;
    let mut captured_session: Option<String> = None;
    loop {
        for line in err_rx.try_iter().flatten() {
            if !err_text.is_empty() {
                err_text.push('\n');
            }
            err_text.push_str(&line);
        }
        if cancel.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(NurError::Interrupted);
        }
        if started.elapsed() >= timeout {
            recovered = transcript.recover(prompt);
            let _ = child.kill();
            let _ = child.wait();
            if recovered.is_none() {
                cli_error = Some(format!(
                    "cursor-agent exceeded the provider turn timeout ({}s, \
                     NUR_PROVIDER_TURN_TIMEOUT_SECS adjusts it)",
                    timeout.as_secs()
                ));
            }
            break;
        }
        // Idle: the CLI is alive but silent (Windows silent-spawn bug, or a
        // bogus --resume id that hangs without an error). Kill, try to recover
        // the durable transcript, error if there is nothing to show.
        if last_activity.elapsed() >= idle_timeout && status.is_none() {
            recovered = transcript.recover(prompt);
            let _ = child.kill();
            let _ = child.wait();
            if recovered.is_none() {
                cli_error = Some(format!(
                    "cursor-agent produced no output for {}s (idle timeout, \
                     NUR_CURSOR_IDLE_TIMEOUT_SECS adjusts it)",
                    idle_timeout.as_secs()
                ));
            }
            break;
        }

        match line_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(line)) => {
                last_stdout = Instant::now();
                last_activity = last_stdout;
                let line = line.trim();
                if let Ok(ev) = serde_json::from_str::<Value>(line) {
                    if let Some(usage) = parse_cursor_usage(&ev) {
                        native_usage = Some(usage);
                    }
                    let ty = ev.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    match ty {
                        "system" => {
                            if ev.get("subtype").and_then(|s| s.as_str()) == Some("init") {
                                if let Some(sid) = ev
                                    .get("session_id")
                                    .and_then(|s| s.as_str())
                                    .filter(|s| !s.is_empty())
                                {
                                    captured_session = Some(sid.to_string());
                                }
                                let model_label =
                                    ev.get("model").and_then(|m| m.as_str()).unwrap_or("Cursor");
                                if let Some(cb) = on_event.as_mut() {
                                    cb(StreamEvent::ReasoningDelta(format!(
                                        "cursor-agent · {model_label}\n"
                                    )));
                                }
                            }
                        }
                        "thinking" | "reasoning" => {
                            if ev.get("subtype").and_then(|s| s.as_str()) != Some("completed") {
                                let chunk = ev
                                    .get("text")
                                    .or_else(|| ev.get("delta"))
                                    .and_then(|t| t.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                if !chunk.is_empty() {
                                    if let Some(cb) = on_event.as_mut() {
                                        cb(StreamEvent::ReasoningDelta(chunk));
                                    }
                                }
                            }
                        }
                        "assistant" => {
                            let has_ts = ev.get("timestamp_ms").is_some();
                            let has_mc = ev.get("model_call_id").is_some();
                            if !(has_mc && !has_ts) {
                                let chunk = assistant_text(&ev);
                                if !chunk.is_empty() && (has_ts || streamed.is_empty()) {
                                    streamed.push_str(&chunk);
                                    if let Some(cb) = on_event.as_mut() {
                                        cb(StreamEvent::TextDelta(chunk));
                                    }
                                }
                            }
                        }
                        "result" => {
                            if let Some(result) = ev.get("result").and_then(|r| r.as_str()) {
                                final_text = result.to_string();
                            }
                            if ev.get("is_error").and_then(|x| x.as_bool()) == Some(true) {
                                cli_error = Some(
                                    ev.get("result")
                                        .and_then(|r| r.as_str())
                                        .unwrap_or("cursor-agent reported an error")
                                        .to_string(),
                                );
                                let _ = child.kill();
                                let _ = child.wait();
                                break;
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(Err(_)) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }

        if status.is_none() {
            status = child
                .try_wait()
                .map_err(|e| NurError::Other(format!("cursor-agent wait: {e}")))?;
        }
        // Cursor's detached worker may inherit stdout/stderr and keep those
        // pipes open after the foreground CLI exits. Joining the reader threads
        // therefore hangs forever. Let the foreground exit establish completion,
        // then allow a short quiet window to drain its final JSON records.
        if status.is_some() && last_stdout.elapsed() >= Duration::from_millis(350) {
            break;
        }
        // Affected Cursor builds persist a complete JSONL turn but never write
        // stream-json to the pipe. Poll that durable result while the child is
        // alive, then terminate its orphan-prone worker cleanly.
        if Instant::now() >= next_transcript_probe {
            next_transcript_probe = Instant::now() + Duration::from_secs(2);
            if let Some(text) = transcript.recover(prompt) {
                recovered = Some(text);
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }

    for line in err_rx.try_iter().flatten() {
        if !err_text.is_empty() {
            err_text.push('\n');
        }
        err_text.push_str(&line);
    }
    if let Some(error) = cli_error {
        return Err(NurError::Other(error));
    }
    if status.is_some_and(|exit| !exit.success())
        && final_text.is_empty()
        && streamed.is_empty()
        && recovered.is_none()
    {
        let status = status.expect("checked above");
        return Err(NurError::Other(format!(
            "cursor-agent failed (exit {status}){}",
            if err_text.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", err_text.chars().take(400).collect::<String>())
            }
        )));
    }

    let text = if !final_text.is_empty() {
        final_text
    } else if let Some(recovered) = recovered.or_else(|| transcript.recover(prompt)) {
        recovered
    } else {
        streamed
    };
    if text.is_empty() {
        return Err(NurError::Other(
            "cursor-agent produced no assistant text. Try `cursor-agent status` and re-login."
                .into(),
        ));
    }
    let model_name = model.unwrap_or_else(|| "auto".into());
    Ok((text, model_name, native_usage, captured_session))
}

/// Cursor CLI's event schema has changed across releases. Accept the common
/// usage envelopes without turning absent telemetry into a synthetic zero.
pub fn parse_cursor_usage(event: &Value) -> Option<ApiUsage> {
    let usage = event
        .get("usage")
        .or_else(|| event.get("usage_metadata"))
        .or_else(|| event.get("token_usage"))
        .or_else(|| event.pointer("/result/usage"))?;
    let number = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| usage.get(*key).and_then(Value::as_u64))
            .unwrap_or(0)
    };
    // camelCase keys are what current cursor-agent builds actually emit in
    // their stream-json result envelope; keep the snake_case spellings for
    // older builds.
    let input = number(&["input_tokens", "prompt_tokens", "input", "inputTokens"]);
    let output = number(&[
        "output_tokens",
        "completion_tokens",
        "output",
        "outputTokens",
    ]);
    let total = number(&["total_tokens", "total", "totalTokens"]);
    let cached = usage
        .pointer("/input_tokens_details/cached_tokens")
        .or_else(|| usage.pointer("/prompt_tokens_details/cached_tokens"))
        .or_else(|| usage.get("cached_tokens"))
        .or_else(|| usage.get("cacheReadTokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cache_write = usage
        .get("cache_write_tokens")
        .or_else(|| usage.get("cacheWriteTokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if input == 0 && output == 0 && total == 0 {
        return None;
    }
    Some(ApiUsage {
        input_tokens: input,
        output_tokens: output,
        total_tokens: total.max(input.saturating_add(output)),
        output_tokens_details: None,
        input_tokens_details: (cached > 0).then_some(crate::api::types::InputTokensDetails {
            cached_tokens: cached,
        }),
        cache_write_tokens: cache_write,
    })
}

fn assistant_text(ev: &Value) -> String {
    let mut out = String::new();
    if let Some(arr) = ev.pointer("/message/content").and_then(|c| c.as_array()) {
        for part in arr {
            if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                out.push_str(t);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn session_token_detection() {
        assert!(is_cli_session_token(CURSOR_CLI_SESSION_TOKEN));
        assert!(is_cli_session_token("cursor-cli:abc"));
        assert!(!is_cli_session_token("sk-real-key"));
    }

    #[test]
    fn resolve_node_launch_from_install_tree() {
        let Some(local) = dirs::data_local_dir() else {
            return;
        };
        let dir = local.join("cursor-agent");
        if !dir.join("versions").is_dir() {
            return;
        }
        let launch = resolve_node_launch(&dir).expect("versioned node launch");
        match launch {
            CursorLaunch::Node { node, index } => {
                assert!(node.is_file(), "{node:?}");
                assert!(index.is_file(), "{index:?}");
            }
            CursorLaunch::Wrapper(_) => panic!("expected Node launch"),
        }
    }

    #[test]
    fn status_json_authenticated() {
        assert!(parse_status_authenticated(
            r#"{"status":"authenticated","isAuthenticated":true}"#
        ));
        assert!(!parse_status_authenticated(
            r#"{"status":"unauthenticated","isAuthenticated":false}"#
        ));
    }

    #[test]
    fn flatten_includes_roles() {
        let req = ResponseRequest {
            model: "auto".into(),
            input: json!([
                {"type":"message","role":"user","content":[{"type":"text","text":"hi"}]}
            ]),
            instructions: Some("be brief".into()),
            tools: None,
            tool_choice: None,
            store: None,
            include: None,
            reasoning: None,
            stream: None,
            parallel_tool_calls: None,
            prompt_cache_key: None,
            max_output_tokens: None,
        };
        let p = flatten_prompt(&req);
        assert!(p.contains("System:"));
        assert!(p.contains("User:"));
        assert!(p.contains("hi"));
    }

    #[test]
    fn split_nur_tools_fence() {
        let text = "Looking around.\n\n```nur-tools\n[{\"name\":\"list_dir\",\"arguments\":{\"path\":\".\"}}]\n```\n";
        let (c, calls) = split_nur_tools(text).expect("fence");
        assert!(c.contains("Looking around"));
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "list_dir");
        assert!(calls[0].1.contains("path"));
    }

    #[test]
    fn split_nur_tools_fence_with_info_suffix() {
        let text =
            "```nur-tools json\n[{\"name\":\"read_file\",\"arguments\":{\"path\":\"a.rs\"}}]\n```";
        let (_, calls) = split_nur_tools(text).expect("fence");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "read_file");
    }

    #[test]
    fn split_nur_tools_single_object_instead_of_array() {
        let text = "```nur-tools\n{\"name\":\"grep\",\"input\":{\"pattern\":\"TODO\"}}\n```";
        let (_, calls) = split_nur_tools(text).expect("fence");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "grep");
        assert!(calls[0].1.contains("TODO"));
    }

    #[test]
    fn split_nur_tools_trailing_text_inside_fence() {
        let text = "```nur-tools\n[{\"name\":\"list_dir\",\"arguments\":{}}] hope this helps\n```";
        let (_, calls) = split_nur_tools(text).expect("fence");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "list_dir");
    }

    #[test]
    fn split_nur_tools_pretty_printed_array() {
        let text = "commentary\n```nur-tools\n[\n  {\n    \"name\": \"agent\",\n    \"arguments\": {\n      \"prompt\": \"hi\"\n    }\n  }\n]\n```";
        let (c, calls) = split_nur_tools(text).expect("fence");
        assert!(c.contains("commentary"));
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "agent");
    }

    #[test]
    fn flatten_incremental_only_sends_new_user_and_tool_output() {
        let items = json!([
            {"type":"message","role":"user","content":[{"type":"text","text":"first ask"}]},
            {"type":"reasoning","summary":[]},
            {"type":"function_call","name":"list_dir","arguments":"{}","call_id":"c1"},
            {"type":"function_call_output","call_id":"c1","output":"dir listing"},
            {"type":"message","role":"user","content":[{"type":"text","text":"second ask"}]}
        ]);
        let arr = items.as_array().expect("input array");
        let out = flatten_incremental(arr, true);
        assert!(out.contains("Continuing the same nur session"));
        assert!(out.contains("nur-tools"));
        assert!(out.contains("User:\nfirst ask"));
        assert!(out.contains("Tool result:\ndir listing"));
        assert!(out.contains("User:\nsecond ask"));
        // Assistant/tool-call/reasoning history must not be re-sent.
        assert_eq!(out.matches("Tool call").count(), 0);

        let empty = flatten_incremental(&[], false);
        assert!(empty.contains("Respond briefly."));
    }

    #[test]
    fn cursor_camelcase_usage_envelope() {
        // Exact shape probed from cursor-agent 2026.08.11 stream-json result.
        let usage = parse_cursor_usage(&serde_json::json!({
            "type": "result",
            "subtype": "success",
            "usage": {
                "inputTokens": 20204,
                "outputTokens": 42,
                "cacheReadTokens": 2176,
                "cacheWriteTokens": 0
            }
        }))
        .expect("usage");
        assert_eq!(usage.input_tokens, 20204);
        assert_eq!(usage.output_tokens, 42);
        assert_eq!(usage.total_tokens, 20246);
        let cached = usage
            .input_tokens_details
            .expect("cached bucket")
            .cached_tokens;
        assert_eq!(cached, 2176);
    }

    #[test]
    fn response_parses_tool_calls() {
        let text = "```nur-tools\n[{\"name\":\"agent\",\"arguments\":{\"prompt\":\"review\",\"provider\":\"anthropic\"}}]\n```";
        let resp = response_from_cli_text("auto", text, true, None);
        assert!(matches!(
            resp.output.first(),
            Some(OutputItem::FunctionCall { name: Some(n), .. }) if n == "agent"
        ));
    }

    #[test]
    fn completed_transcript_recovers_matching_assistant_text() {
        let transcript = concat!(
            r#"{"role":"user","message":{"content":[{"type":"text","text":"<user_query>hello cursor</user_query>"}]}}"#,
            "\n",
            r#"{"role":"assistant","message":{"content":[{"type":"text","text":"hello back"}]}}"#,
            "\n",
            r#"{"type":"turn_ended","status":"success"}"#,
        );
        assert_eq!(
            parse_completed_transcript(transcript, "hello cursor").as_deref(),
            Some("hello back")
        );
        assert!(parse_completed_transcript(transcript, "different prompt").is_none());
    }

    #[test]
    fn incomplete_transcript_is_not_recovered() {
        let transcript = concat!(
            r#"{"role":"user","message":{"content":[{"type":"text","text":"hello cursor"}]}}"#,
            "\n",
            r#"{"role":"assistant","message":{"content":[{"type":"text","text":"partial"}]}}"#,
        );
        assert!(parse_completed_transcript(transcript, "hello cursor").is_none());
    }

    #[test]
    fn cursor_native_usage_is_preserved_when_available() {
        let usage = parse_cursor_usage(&serde_json::json!({
            "type": "result",
            "usage": {"prompt_tokens": 120, "completion_tokens": 8, "total_tokens": 128}
        }))
        .expect("usage");
        assert_eq!(usage.input_tokens, 120);
        assert_eq!(usage.output_tokens, 8);
        assert_eq!(usage.total_tokens, 128);
    }

    #[test]
    fn cursor_credit_only_event_is_not_fake_zero_token_usage() {
        assert!(parse_cursor_usage(&serde_json::json!({
            "usage": {"credits_used": 1}
        }))
        .is_none());
    }

    #[test]
    #[ignore = "requires an installed, authenticated Cursor Agent"]
    fn live_cursor_completion_recovers_silent_windows_stdout() {
        let req = ResponseRequest {
            // `auto` is the catalog default and avoids pinning this transport
            // regression to a model that Cursor may temporarily withdraw.
            model: "auto".into(),
            input: json!([{
                "type":"message",
                "role":"user",
                "content":[{"type":"text","text":"Reply with exactly NUR_CURSOR_E2E_OK"}]
            }]),
            instructions: None,
            tools: None,
            tool_choice: None,
            store: None,
            include: None,
            reasoning: None,
            stream: Some(true),
            parallel_tool_calls: None,
            prompt_cache_key: None,
            max_output_tokens: None,
        };
        let cancel = tokio_util::sync::CancellationToken::new();
        let response = complete(&req, &cancel, None).expect("live Cursor completion");
        assert!(response.output_text().contains("NUR_CURSOR_E2E_OK"));
    }

    #[test]
    #[ignore = "requires an installed, authenticated Cursor Agent and a 1s timeout env"]
    fn live_cursor_timeout_terminates_the_child() {
        if std::env::var("NUR_PROVIDER_TURN_TIMEOUT_SECS").as_deref() != Ok("1") {
            return;
        }
        let req = ResponseRequest {
            model: "composer-2.5-fast".into(),
            input: json!([{
                "type":"message",
                "role":"user",
                "content":[{"type":"text","text":"Wait before replying with NUR_TIMEOUT_TEST"}]
            }]),
            instructions: None,
            tools: None,
            tool_choice: None,
            store: None,
            include: None,
            reasoning: None,
            stream: Some(true),
            parallel_tool_calls: None,
            prompt_cache_key: None,
            max_output_tokens: None,
        };
        let cancel = tokio_util::sync::CancellationToken::new();
        let error = complete(&req, &cancel, None).expect_err("the 1s live test should time out");
        assert!(error.to_string().contains("provider turn timeout"));
    }

    /// End-to-end `--resume` chain: the second round must recall the secret
    /// from the FIRST round's CLI session without nur re-sending it, and the
    /// shared chat state must hold the session id with the full item count.
    #[test]
    #[ignore = "requires an installed, authenticated Cursor Agent"]
    fn live_cursor_resume_round_trip() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(async {
            let client = crate::api::client::ApiClient::for_provider(
                "https://api2.cursor.sh",
                CURSOR_CLI_SESSION_TOKEN,
                "cursor",
            )
            .expect("cursor ApiClient");
            let first = ResponseRequest {
                model: "auto".into(),
                input: json!([{
                    "type":"message",
                    "role":"user",
                    "content":[{"type":"text","text":"My secret phrase is ZEBRA_4711. Remember it."}]
                }]),
                instructions: None,
                tools: None,
                tool_choice: None,
                store: None,
                include: None,
                reasoning: None,
                stream: Some(true),
                parallel_tool_calls: None,
                prompt_cache_key: None,
                max_output_tokens: None,
            };
            let r1 = client.create_response(&first).await.expect("first round");
            assert!(!r1.output_text().trim().is_empty());

            let second = ResponseRequest {
                model: "auto".into(),
                input: json!([
                    {
                        "type":"message",
                        "role":"user",
                        "content":[{"type":"text","text":"My secret phrase is ZEBRA_4711. Remember it."}]
                    },
                    {
                        "type":"message",
                        "role":"user",
                        "content":[{"type":"text","text":"What was my secret phrase? Answer with just the phrase."}]
                    }
                ]),
                instructions: None,
                tools: None,
                tool_choice: None,
                store: None,
                include: None,
                reasoning: None,
                stream: Some(true),
                parallel_tool_calls: None,
                prompt_cache_key: None,
                max_output_tokens: None,
            };
            let r2 = client.create_response(&second).await.expect("second round");
            assert!(
                r2.output_text().contains("ZEBRA_4711"),
                "resume lost the secret; answer was: {}",
                r2.output_text()
            );
            let state = client
                .cursor_chat_for_test()
                .lock()
                .expect("chat state lock");
            assert!(state.session_id.is_some(), "no session id captured");
            assert_eq!(state.items_sent, 2, "items_sent must cover both messages");
        });
    }
}

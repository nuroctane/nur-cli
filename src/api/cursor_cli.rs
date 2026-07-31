//! Cursor Agent CLI transport - auth and chat without a pasted API key.
//!
//! Cursor's public Agent endpoint (`api2.cursor.sh`) is not OpenAI Chat
//! Completions. Nur drives Cursor the t3code way: the logged-in `cursor-agent`
//! binary holds the session (keychain / platform auth), and inference is
//! `cursor-agent -p --output-format stream-json`.

use crate::api::types::{ApiResponse, ApiUsage, ContentPart, OutputItem, ResponseRequest};
use crate::api::StreamEvent;
use crate::error::{MuseError, Result};
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Stored in `auth.json` when the user is signed in via `cursor-agent login`.
/// Not a real Bearer token - ApiClient routes Cursor through this module.
pub const CURSOR_CLI_SESSION_TOKEN: &str = "cursor-cli-session";

pub fn is_cli_session_token(token: &str) -> bool {
    let t = token.trim();
    t == CURSOR_CLI_SESSION_TOKEN || t.starts_with("cursor-cli:")
}

/// Resolve `cursor-agent` on PATH (never bare `agent` - that can be Grok).
pub fn cursor_agent_bin() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        #[cfg(windows)]
        {
            for name in ["cursor-agent.cmd", "cursor-agent.exe", "cursor-agent.ps1"] {
                let p = dir.join(name);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
        let c = dir.join("cursor-agent");
        if c.is_file() {
            return Some(c);
        }
    }
    None
}

fn spawn_agent(bin: &PathBuf, args: &[&str]) -> std::io::Result<std::process::Child> {
    #[cfg(windows)]
    {
        let lower = bin.to_string_lossy().to_ascii_lowercase();
        let mut cmd = if lower.ends_with(".cmd") || lower.ends_with(".bat") {
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
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    }
    #[cfg(not(windows))]
    {
        Command::new(bin)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    }
}

fn run_capture(bin: &PathBuf, args: &[&str]) -> Result<String> {
    let mut child = spawn_agent(bin, args)
        .map_err(|e| MuseError::Other(format!("failed to launch cursor-agent: {e}")))?;
    let _ = child.stdin.take();
    let out = child
        .wait_with_output()
        .map_err(|e| MuseError::Other(format!("cursor-agent failed: {e}")))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(MuseError::Other(format!(
            "cursor-agent exited {}: {}",
            out.status,
            err.chars().take(300).collect::<String>()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `cursor-agent status --format json` → authenticated?
pub fn cli_is_authenticated() -> bool {
    let Some(bin) = cursor_agent_bin() else {
        return false;
    };
    let Ok(text) = run_capture(&bin, &["status", "--format", "json"]) else {
        return false;
    };
    parse_status_authenticated(&text)
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
    let bin = cursor_agent_bin().ok_or_else(|| {
        MuseError::Other(
            "cursor-agent not found on PATH. Install Cursor Agent, then run `cursor-agent login`."
                .into(),
        )
    })?;
    if !cli_is_authenticated()
        && std::env::var("CURSOR_API_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty())
            .is_none()
    {
        return Err(MuseError::Other(
            "Cursor Agent is not logged in. Run /login → Cursor → Sign in with browser \
             (or `cursor-agent login`)."
                .into(),
        ));
    }
    let output = run_capture(&bin, &["models"])
        .or_else(|_| run_capture(&bin, &["--list-models"]))?;
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
        if cleaned
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '[' | ']' | '=' | ','))
            && cleaned.len() < 80
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
                    let role = item
                        .get("role")
                        .and_then(|r| r.as_str())
                        .unwrap_or("user");
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

/// Split `text` into (commentary, tool calls) when a ````nur-tools` fence is present.
pub fn split_nur_tools(text: &str) -> Option<(String, Vec<(String, String)>)> {
    const START: &str = "```nur-tools";
    let idx = text.find(START)?;
    let after = &text[idx + START.len()..];
    let after = after.strip_prefix('\r').unwrap_or(after);
    let after = after.strip_prefix('\n').unwrap_or(after);
    let end = after.find("```")?;
    let json_body = after[..end].trim();
    let commentary = text[..idx].trim().to_string();
    let parsed: Value = serde_json::from_str(json_body).ok()?;
    let arr = parsed.as_array()?;
    let mut calls = Vec::new();
    for item in arr {
        let name = item
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if name.is_empty() {
            continue;
        }
        let args = match item.get("arguments").or_else(|| item.get("input")) {
            Some(Value::String(s)) => s.clone(),
            Some(other) => other.to_string(),
            None => "{}".into(),
        };
        calls.push((name, args));
    }
    if calls.is_empty() {
        return None;
    }
    Some((commentary, calls))
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

fn response_from_cli_text(model: &str, text: &str, parse_tools: bool) -> ApiResponse {
    let id = format!("cursor-cli-{}", uuid_simple());
    let usage = Some(ApiUsage {
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 0,
        output_tokens_details: None,
        input_tokens_details: None,
    });
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
        }],
        usage,
        error: None,
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
) -> Result<ApiResponse> {
    let parse_tools = tools_wire(req);
    let (text, model) = run_print(req, None, cancel)?;
    Ok(response_from_cli_text(&model, &text, parse_tools))
}

/// Stream Cursor Agent output; `on_event` receives text deltas.
pub fn complete_stream(
    req: &ResponseRequest,
    mut on_event: impl FnMut(StreamEvent),
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<ApiResponse> {
    let parse_tools = tools_wire(req);
    // When tool-calling is expected, buffer output so the nur-tools fence is not
    // painted into the transcript; emit commentary (if any) once at the end.
    let (text, model) = if parse_tools {
        run_print(req, None, cancel)?
    } else {
        run_print(req, Some(&mut on_event), cancel)?
    };
    let resp = response_from_cli_text(&model, &text, parse_tools);
    if parse_tools {
        let commentary = resp.output_text();
        if !commentary.is_empty() {
            on_event(StreamEvent::TextDelta(commentary));
        }
    }
    on_event(StreamEvent::Completed(resp.clone()));
    Ok(resp)
}

fn run_print(
    req: &ResponseRequest,
    mut on_event: Option<&mut dyn FnMut(StreamEvent)>,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<(String, String)> {
    let bin = cursor_agent_bin().ok_or_else(|| {
        MuseError::Other(
            "cursor-agent not found on PATH. Install Cursor Agent (https://cursor.com/docs/cli)."
                .into(),
        )
    })?;
    let has_api_key = std::env::var("CURSOR_API_KEY")
        .ok()
        .is_some_and(|k| !k.trim().is_empty());
    if !cli_is_authenticated() && !has_api_key {
        return Err(MuseError::Other(
            "Cursor Agent is not logged in. Run /login → Cursor → Sign in with browser \
             (`cursor-agent login`), or set CURSOR_API_KEY."
                .into(),
        ));
    }

    let prompt = flatten_prompt(req);
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
    let mut owned: Vec<String> = vec![
        "-p".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--stream-partial-output".into(),
        "--trust".into(),
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

    // Windows CreateProcess cmdline ~8191 chars. Prefer stdin for the prompt
    // when large; also pass a short arg so CLIs that ignore stdin still work.
    const ARG_BUDGET: usize = 4_000;
    let pass_as_arg = prompt.len() <= ARG_BUDGET;
    if pass_as_arg {
        owned.push(prompt.clone());
    }

    let arg_refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    let mut child = spawn_agent(&bin, &arg_refs)
        .map_err(|e| MuseError::Other(format!("failed to launch cursor-agent: {e}")))?;

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
        .ok_or_else(|| MuseError::Other("cursor-agent stdout missing".into()))?;
    let stderr_pipe = child.stderr.take();
    let stderr_handle = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(err) = stderr_pipe {
            let _ = Read::read_to_string(&mut BufReader::new(err), &mut buf);
        }
        buf
    });

    let mut final_text = String::new();
    let mut streamed = String::new();
    let reader = BufReader::new(stdout);
    for line in reader.lines() {
        if cancel.is_cancelled() {
            let _ = child.kill();
            let _ = stderr_handle.join();
            return Err(MuseError::Other("cancelled".into()));
        }
        let Ok(line) = line else {
            continue;
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(ev) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let ty = ev.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match ty {
            "assistant" => {
                // With --stream-partial-output, prefer timestamped deltas;
                // skip buffered duplicates that carry model_call_id.
                let has_ts = ev.get("timestamp_ms").is_some();
                let has_mc = ev.get("model_call_id").is_some();
                if has_mc && !has_ts {
                    continue;
                }
                let chunk = assistant_text(&ev);
                if chunk.is_empty() {
                    continue;
                }
                if has_ts || streamed.is_empty() {
                    streamed.push_str(&chunk);
                    if let Some(cb) = on_event.as_mut() {
                        cb(StreamEvent::TextDelta(chunk));
                    }
                }
            }
            "result" => {
                if let Some(r) = ev.get("result").and_then(|r| r.as_str()) {
                    final_text = r.to_string();
                }
                if ev.get("is_error").and_then(|x| x.as_bool()) == Some(true) {
                    let msg = ev
                        .get("result")
                        .and_then(|r| r.as_str())
                        .unwrap_or("cursor-agent reported an error");
                    let _ = child.kill();
                    let _ = stderr_handle.join();
                    return Err(MuseError::Other(msg.into()));
                }
            }
            _ => {}
        }
    }

    let err_text = stderr_handle.join().unwrap_or_default();
    let status = child
        .wait()
        .map_err(|e| MuseError::Other(format!("cursor-agent wait: {e}")))?;
    if !status.success() && final_text.is_empty() && streamed.is_empty() {
        return Err(MuseError::Other(format!(
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
    } else {
        streamed
    };
    if text.is_empty() {
        return Err(MuseError::Other(
            "cursor-agent produced no assistant text. Try `cursor-agent status` and re-login."
                .into(),
        ));
    }
    let model_name = model.unwrap_or_else(|| "auto".into());
    Ok((text, model_name))
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
    fn response_parses_tool_calls() {
        let text = "```nur-tools\n[{\"name\":\"agent\",\"arguments\":{\"prompt\":\"review\",\"provider\":\"anthropic\"}}]\n```";
        let resp = response_from_cli_text("auto", text, true);
        assert!(matches!(
            resp.output.first(),
            Some(OutputItem::FunctionCall { name: Some(n), .. }) if n == "agent"
        ));
    }
}

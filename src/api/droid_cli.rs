//! Factory Droid CLI transport - chat through `droid exec` with the
//! Factory-managed models.
//!
//! Factory has no public OpenAI-compatible chat endpoint; its inference runs
//! behind the `droid` binary (`droid` login or `FACTORY_API_KEY`). Nur drives
//! it the same way it drives Cursor: one `droid exec --output-format json`
//! per model round, nur's tools offered through the nur-tools fence, and the
//! CLI's own `--session-id` reused so later rounds ship only the delta.
//!
//! Without `--auto` the CLI is read-only (spec mode), which is exactly the
//! posture for a model that must answer with tool *requests*: it may read the
//! workspace to ground its reply but every mutation goes through nur's
//! approvals. `NUR_DROID_NATIVE=1` instead delegates the whole task to Droid
//! with `--auto <NUR_DROID_AUTO|medium>`.

use crate::api::cursor_cli::{
    flatten_incremental, flatten_prompt_with, response_from_agent_cli_text, CursorChatState,
};
use crate::api::types::{ApiResponse, ApiUsage, ResponseRequest};
use crate::api::StreamEvent;
use crate::error::{NurError, Result};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Catalog placeholder base URL. Never called over HTTP.
pub const FACTORY_APP_URL: &str = "https://app.factory.ai";

/// Model ids Factory-managed inference serves (`docs.factory.ai/docs/models`).
/// The CLI has no model-list command, so this is what `/model` offers; any id
/// the CLI accepts still works when typed.
pub const FACTORY_MODELS: &[&str] = &[
    "claude-opus-5-5",
    "claude-sonnet-5-5",
    "claude-haiku-5-5",
    "gpt-6.1-sol",
    "gpt-6-luna",
    "gemini-3.1-pro-preview",
    "grok-4.7",
    "inkling",
    "glm-5.3",
    "kimi-k3",
    "qwen3.8-max",
    "deepseek-v4.1-flash",
    "minimax-m3",
];

/// Session chain for `--session-id`; same shape as Cursor's resume chain.
pub(crate) type DroidChatState = CursorChatState;

/// When set (`1`/`true`/`yes`) Droid runs the task itself with its own tools
/// (`--auto`), ignoring nur's harness. Default is the nur-tools protocol.
pub fn native_delegate_enabled() -> bool {
    std::env::var("NUR_DROID_NATIVE")
        .ok()
        .is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
}

/// Autonomy tier for the native delegate; `medium` covers local dev work
/// without `git push` or production changes.
fn native_auto_level() -> String {
    std::env::var("NUR_DROID_AUTO")
        .ok()
        .map(|v| v.trim().to_ascii_lowercase())
        .filter(|v| matches!(v.as_str(), "low" | "medium" | "high"))
        .unwrap_or_else(|| "medium".into())
}

fn tools_wire(req: &ResponseRequest) -> bool {
    req.tools.as_ref().is_some_and(|t| !t.is_empty()) && !native_delegate_enabled()
}

/// Locate the `droid` launcher: the official installer's `~/.factory/bin`,
/// then the shared fixed-path/PATH/npm probe.
pub fn resolve_bin() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("NUR_DROID_BIN") {
        let p = PathBuf::from(explicit.trim());
        if p.is_file() {
            return Some(p);
        }
    }
    if let Some(home) = dirs::home_dir() {
        let bin = home.join(".factory").join("bin");
        for name in ["droid.exe", "droid.cmd", "droid"] {
            let p = bin.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    crate::ecosystem::find_bin("droid").map(PathBuf::from)
}

fn has_api_key() -> bool {
    std::env::var("FACTORY_API_KEY")
        .ok()
        .is_some_and(|k| !k.trim().is_empty())
}

fn provider_turn_timeout() -> Duration {
    std::env::var("NUR_PROVIDER_TURN_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|s| *s > 0)
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(300))
}

/// No stdout/stderr line for this long means the CLI is wedged (most often a
/// permission prompt that has no console in exec mode).
fn idle_timeout() -> Duration {
    std::env::var("NUR_DROID_IDLE_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|s| *s > 0)
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(180))
}

/// Run a non-streaming Droid completion. `api_key` is a key nur holds for the
/// `factory` provider (pasted into `/login`); it reaches the CLI as
/// `FACTORY_API_KEY` for that child only, so the CLI's own sign-in still
/// works when it is empty.
pub fn complete(
    req: &ResponseRequest,
    cancel: &tokio_util::sync::CancellationToken,
    chat: Option<&Arc<Mutex<DroidChatState>>>,
    api_key: Option<&str>,
) -> Result<ApiResponse> {
    let parse_tools = tools_wire(req);
    let (text, model, usage) = run_exec(req, None, cancel, provider_turn_timeout(), chat, api_key)?;
    Ok(response_from_agent_cli_text(
        "factory",
        &model,
        &text,
        parse_tools,
        usage,
    ))
}

/// Streaming variant. `droid exec --output-format json` prints only the final
/// object, so progress is limited to launch/completion notes; the final text
/// is delivered as one `TextDelta` so the TUI renders it like any other model.
pub fn complete_stream(
    req: &ResponseRequest,
    mut on_event: impl FnMut(StreamEvent),
    cancel: &tokio_util::sync::CancellationToken,
    chat: Option<&Arc<Mutex<DroidChatState>>>,
    api_key: Option<&str>,
) -> Result<ApiResponse> {
    let parse_tools = tools_wire(req);
    on_event(StreamEvent::ReasoningDelta(
        "droid exec · launching\n".into(),
    ));
    let (text, model, usage) = run_exec(
        req,
        Some(&mut |ev| on_event(ev)),
        cancel,
        provider_turn_timeout() + Duration::from_secs(30),
        chat,
        api_key,
    )?;
    let resp = response_from_agent_cli_text("factory", &model, &text, parse_tools, usage);
    let visible = resp.output_text();
    if !visible.is_empty() {
        on_event(StreamEvent::TextDelta(visible));
    }
    on_event(StreamEvent::Completed(resp.clone()));
    Ok(resp)
}

fn run_exec(
    req: &ResponseRequest,
    mut on_event: Option<&mut dyn FnMut(StreamEvent)>,
    cancel: &tokio_util::sync::CancellationToken,
    total_timeout: Duration,
    chat: Option<&Arc<Mutex<DroidChatState>>>,
    api_key: Option<&str>,
) -> Result<(String, String, Option<ApiUsage>)> {
    let Some(bin) = resolve_bin() else {
        return Err(NurError::Other(
            "droid not found. Install Factory's CLI (https://docs.factory.ai/cli/getting-started/quickstart: \
             `curl -fsSL https://app.factory.ai/cli | sh`, or `npm i -g @factory/cli`) and run `droid` once to sign in, \
             or set FACTORY_API_KEY."
                .into(),
        ));
    };
    let items_len = req.input.as_array().map(|a| a.len()).unwrap_or(0);
    let (resume_id, resume_from): (Option<String>, usize) = match chat.and_then(|c| c.lock().ok()) {
        Some(state) if state.session_id.is_some() && items_len > state.items_sent => {
            (state.session_id.clone(), state.items_sent)
        }
        _ => (None, 0),
    };
    let mut retried = false;
    loop {
        let attempt_resume = if retried { None } else { resume_id.clone() };
        let prompt = match &attempt_resume {
            Some(_) => {
                let items = req.input.as_array().cloned().unwrap_or_default();
                let from = resume_from.min(items.len());
                flatten_incremental(&items[from..], tools_wire(req))
            }
            None => flatten_prompt_with(req, tools_wire(req), "Factory Droid"),
        };
        let launch = Launch {
            bin: &bin,
            prompt: &prompt,
            resume_id: attempt_resume.as_deref(),
            api_key,
            total_timeout,
        };
        match run_exec_once(&launch, req, &mut on_event, cancel) {
            Ok((text, model, usage, session)) => {
                if let Some(chat) = chat {
                    if let Ok(mut st) = chat.lock() {
                        st.session_id = session.or_else(|| resume_id.clone());
                        st.items_sent = items_len;
                    }
                }
                return Ok((text, model, usage));
            }
            Err(e) => {
                if let Some(chat) = chat {
                    if let Ok(mut st) = chat.lock() {
                        st.session_id = None;
                        st.items_sent = 0;
                    }
                }
                // A session the CLI no longer has (pruned, other machine) is
                // reported as an error; retry once from a fresh session.
                if attempt_resume.is_some() && !retried && !matches!(e, NurError::Interrupted) {
                    retried = true;
                    continue;
                }
                return Err(e);
            }
        }
    }
}

/// Everything one `droid exec` launch needs besides the request itself.
struct Launch<'a> {
    bin: &'a PathBuf,
    prompt: &'a str,
    resume_id: Option<&'a str>,
    api_key: Option<&'a str>,
    total_timeout: Duration,
}

fn run_exec_once(
    launch: &Launch<'_>,
    req: &ResponseRequest,
    on_event: &mut Option<&mut dyn FnMut(StreamEvent)>,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<(String, String, Option<ApiUsage>, Option<String>)> {
    let Launch {
        bin,
        prompt,
        resume_id,
        api_key,
        total_timeout,
    } = *launch;
    let model = Some(req.model.trim())
        .filter(|m| !m.is_empty() && *m != "auto")
        .map(str::to_string);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let native = req.tools.as_ref().is_some_and(|t| !t.is_empty()) && native_delegate_enabled();

    let mut args: Vec<String> = vec![
        "exec".into(),
        "--output-format".into(),
        "json".into(),
        "--cwd".into(),
        cwd.to_string_lossy().into_owned(),
    ];
    if native {
        args.push("--auto".into());
        args.push(native_auto_level());
    }
    if let Some(m) = &model {
        args.push("--model".into());
        args.push(m.clone());
    }
    if let Some(effort) = req
        .reasoning
        .as_ref()
        .and_then(|r| r.effort.as_deref())
        .filter(|e| matches!(*e, "low" | "medium" | "high"))
    {
        args.push("--reasoning-effort".into());
        args.push(effort.into());
    }
    if let Some(id) = resume_id {
        args.push("--session-id".into());
        args.push(id.into());
    }
    // Windows CreateProcess caps the command line near 8k chars; the prompt
    // goes through stdin (documented `echo … | droid exec`) unless it is short.
    const ARG_BUDGET: usize = 4_000;
    let pass_as_arg = prompt.len() <= ARG_BUDGET && !prompt.starts_with('-');
    if pass_as_arg {
        args.push(prompt.to_string());
    }

    let mut cmd = Command::new(bin);
    cmd.args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("NO_COLOR", "1");
    if let Some(key) = api_key.map(str::trim).filter(|k| !k.is_empty()) {
        cmd.env("FACTORY_API_KEY", key);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| NurError::Other(format!("failed to launch droid: {e}")))?;
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
        .ok_or_else(|| NurError::Other("droid stdout missing".into()))?;
    let stderr = child.stderr.take();
    let (err_tx, err_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        if let Some(err) = stderr {
            for line in BufReader::new(err).lines() {
                if err_tx.send(line).is_err() {
                    break;
                }
            }
        }
    });
    let (out_tx, out_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if out_tx.send(line).is_err() {
                break;
            }
        }
    });

    let started = Instant::now();
    let idle = idle_timeout();
    let mut last_activity = Instant::now();
    let mut raw_out = String::new();
    let mut err_text = String::new();
    let mut status = None;
    let mut last_stdout = Instant::now();
    loop {
        for line in err_rx.try_iter().flatten() {
            last_activity = Instant::now();
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
        if started.elapsed() >= total_timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(NurError::Other(format!(
                "droid exec exceeded the provider turn timeout ({}s, NUR_PROVIDER_TURN_TIMEOUT_SECS adjusts it)",
                total_timeout.as_secs()
            )));
        }
        if status.is_none() && last_activity.elapsed() >= idle {
            let _ = child.kill();
            let _ = child.wait();
            return Err(NurError::Other(format!(
                "droid exec produced no output for {}s (idle timeout, NUR_DROID_IDLE_TIMEOUT_SECS adjusts it)",
                idle.as_secs()
            )));
        }
        match out_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(line)) => {
                last_stdout = Instant::now();
                last_activity = last_stdout;
                raw_out.push_str(&line);
                raw_out.push('\n');
            }
            Ok(Err(_)) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }
        if status.is_none() {
            status = child
                .try_wait()
                .map_err(|e| NurError::Other(format!("droid wait: {e}")))?;
        }
        if status.is_some() && last_stdout.elapsed() >= Duration::from_millis(250) {
            break;
        }
    }
    for line in err_rx.try_iter().flatten() {
        if !err_text.is_empty() {
            err_text.push('\n');
        }
        err_text.push_str(&line);
    }

    let parsed = parse_exec_output(&raw_out);
    if let Some(ExecResult {
        is_error: true,
        text,
        ..
    }) = &parsed
    {
        return Err(NurError::Other(format!(
            "droid exec reported an error: {}",
            text.chars().take(600).collect::<String>()
        )));
    }
    if parsed.is_none() {
        let exit = status
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unknown".into());
        let detail = if !err_text.trim().is_empty() {
            err_text.trim().chars().take(600).collect::<String>()
        } else {
            raw_out.trim().chars().take(600).collect::<String>()
        };
        let hint = if detail.to_ascii_lowercase().contains("not logged in")
            || detail.to_ascii_lowercase().contains("unauthorized")
            || detail.to_ascii_lowercase().contains("api key")
            || (!has_api_key() && detail.is_empty())
        {
            " Sign in with `droid` (interactive) or set FACTORY_API_KEY."
        } else {
            ""
        };
        return Err(NurError::Other(format!(
            "droid exec produced no result object (exit {exit}): {detail}{hint}"
        )));
    }
    let result = parsed.expect("checked above");
    if let Some(cb) = on_event.as_mut() {
        cb(StreamEvent::ReasoningDelta(format!(
            "droid exec · done in {:.1}s ({} turn{})\n",
            result.duration_ms as f64 / 1000.0,
            result.num_turns,
            if result.num_turns == 1 { "" } else { "s" }
        )));
    }
    if result.text.trim().is_empty() {
        return Err(NurError::Other(
            "droid exec produced no assistant text".into(),
        ));
    }
    Ok((
        result.text,
        model.unwrap_or_else(|| "auto".into()),
        result.usage,
        result.session_id,
    ))
}

/// The one object `--output-format json` prints at the end of a run.
#[derive(Debug, Clone)]
pub(crate) struct ExecResult {
    pub text: String,
    pub is_error: bool,
    pub session_id: Option<String>,
    pub duration_ms: u64,
    pub num_turns: u64,
    pub usage: Option<ApiUsage>,
}

/// Find the result object in the CLI's stdout. The CLI may print update or
/// warning lines before it (deprecation notices, telemetry), and the object
/// may be pretty-printed over several lines, so parse from the last `{` that
/// opens a `"type":"result"` document rather than assuming one JSON line.
pub(crate) fn parse_exec_output(raw: &str) -> Option<ExecResult> {
    let candidates = raw.match_indices('{').map(|(i, _)| i).collect::<Vec<_>>();
    for start in candidates.into_iter().rev() {
        let slice = raw[start..].trim_end();
        let Ok(v) = serde_json::from_str::<Value>(slice) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("result") {
            continue;
        }
        let text = match v.get("result") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Null) | None => String::new(),
            Some(other) => other.to_string(),
        };
        let is_error = v.get("is_error").and_then(|b| b.as_bool()).unwrap_or(false)
            || v.get("subtype").and_then(|s| s.as_str()) == Some("error");
        let usage = v.get("usage").and_then(|u| {
            let input = u
                .get("input_tokens")
                .or_else(|| u.get("prompt_tokens"))
                .and_then(|n| n.as_u64());
            let output = u
                .get("output_tokens")
                .or_else(|| u.get("completion_tokens"))
                .and_then(|n| n.as_u64());
            (input.is_some() || output.is_some()).then(|| ApiUsage {
                input_tokens: input.unwrap_or(0),
                output_tokens: output.unwrap_or(0),
                total_tokens: input.unwrap_or(0) + output.unwrap_or(0),
                output_tokens_details: None,
                input_tokens_details: None,
                cache_write_tokens: 0,
            })
        });
        return Some(ExecResult {
            text,
            is_error,
            session_id: v
                .get("session_id")
                .and_then(|s| s.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            duration_ms: v.get("duration_ms").and_then(|n| n.as_u64()).unwrap_or(0),
            num_turns: v.get("num_turns").and_then(|n| n.as_u64()).unwrap_or(0),
            usage,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ways the parse can fail: a warning line before the object, a
    /// pretty-printed object, an error result, and no object at all.
    #[test]
    fn result_object_is_found_after_noise_and_across_lines() {
        let raw = "warning: stream-json is deprecated\n{\n  \"type\": \"result\",\n  \"subtype\": \"success\",\n  \"is_error\": false,\n  \"duration_ms\": 5657,\n  \"num_turns\": 1,\n  \"result\": \"Hello {world}\",\n  \"session_id\": \"8af22e0a\"\n}\n";
        let r = parse_exec_output(raw).expect("result");
        assert_eq!(r.text, "Hello {world}");
        assert!(!r.is_error);
        assert_eq!(r.session_id.as_deref(), Some("8af22e0a"));
        assert_eq!(r.num_turns, 1);
    }

    #[test]
    fn error_results_and_missing_objects_are_distinguished() {
        let err = parse_exec_output(
            r#"{"type":"result","subtype":"error","is_error":true,"result":"Not logged in"}"#,
        )
        .expect("parsed");
        assert!(err.is_error);
        assert_eq!(err.text, "Not logged in");
        assert!(parse_exec_output("Factory Droid v1.2.3\nnothing here").is_none());
        assert!(parse_exec_output(r#"{"type":"system","subtype":"init"}"#).is_none());
    }

    #[test]
    fn usage_is_only_reported_when_the_cli_sent_it() {
        let with = parse_exec_output(
            r#"{"type":"result","result":"ok","usage":{"input_tokens":12,"output_tokens":3}}"#,
        )
        .expect("parsed");
        assert_eq!(with.usage.as_ref().map(|u| u.total_tokens), Some(15));
        let without = parse_exec_output(r#"{"type":"result","result":"ok"}"#).expect("parsed");
        assert!(without.usage.is_none());
    }
}

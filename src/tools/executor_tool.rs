//! Executor MCP gateway wrapper — <https://executor.sh/docs>
//!
//! Executor 2 (currently 2.0.0-beta.37; stable 2.0.0 not yet on npm
//! `latest`) removed every CLI command the old wrapper shelled out to
//! (`tools`, `call`, `mcp`, `web`, `install`) and moved tool operations
//! behind one streamable-HTTP MCP endpoint
//! (`http://127.0.0.1:4312/mcp`, six tools: `skills`, `search`,
//! `describe`, `call`, `execute`, `resume`). The CLI survives only for
//! daemon management (`service start|stop|status`, `pair`, `upgrade`).
//!
//! This tool is a hybrid: daemon management still goes through the CLI,
//! tool operations go through the MCP endpoint with the in-repo client
//! (`crate::mcp_http::Connection`). Version detection (`executor
//! --version`, regex-extracted semver) keeps the V1 CLI path as a
//! fallback while a V1 binary is installed.
//!
//! Auth (unverified live — the V2 daemon would not start in the dev VM,
//! "Local startup failed at composition"): the loopback endpoint may or
//! may not require a bearer. We try anonymous first and, on 401/403,
//! retry with a key from `EXECUTOR_API_KEY` or the `apiKey` in
//! `keys.json` under the data dir (`$EXECUTOR_DATA_DIR` or
//! `~/.executor/v2/cli`). If the endpoint answers anonymously, no key
//! is ever read.

use super::{arg_str, Tool, ToolContext};
use crate::ecosystem;
use crate::error::{NurError, Result};
use crate::mcp_http::{Connection, McpError};
use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::Duration;

pub struct ExecutorTool;

/// Executor 2's MCP endpoint. Port from `EXECUTOR_PORT`, else the V2
/// default 4312. Never hardcode the V1 port 4788.
fn endpoint_url() -> String {
    let port = std::env::var("EXECUTOR_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(4312);
    format!("http://127.0.0.1:{port}/mcp")
}

/// Extract `(major, full_version)` from `executor --version` output.
/// V2 prints `executor v2.0.0-beta.37` (first run also prints a download
/// notice line before it), so the semver is regex-extracted, never
/// assumed to be the first line or bare.
fn parse_version(output: &str) -> Option<(u64, String)> {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"v?(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?")
            .expect("executor version regex")
    });
    let caps = re.captures(output)?;
    let major: u64 = caps.get(1)?.as_str().parse().ok()?;
    // Build the canonical version from the groups, never from the whole
    // match, so a `v` prefix in the output (`executor v2.0.0-beta.37`)
    // does not leak into the version string.
    let minor: u64 = caps.get(2)?.as_str().parse().ok()?;
    let patch: u64 = caps.get(3)?.as_str().parse().ok()?;
    let version = match caps.get(4) {
        Some(pre) => format!("{major}.{minor}.{patch}-{}", pre.as_str()),
        None => format!("{major}.{minor}.{patch}"),
    };
    Some((major, version))
}

fn executor_version(bin: &str) -> Option<(u64, String)> {
    let out = ecosystem::run_capture(bin, &["--version"], None, 30_000).ok()?;
    parse_version(&out)
}

/// Local API key for the MCP endpoint: `EXECUTOR_API_KEY` first, else the
/// `apiKey` in `keys.json` under the data dir. The value is never logged.
fn local_api_key() -> Option<String> {
    if let Ok(key) = std::env::var("EXECUTOR_API_KEY") {
        if !key.trim().is_empty() {
            return Some(key);
        }
    }
    let data_dir = std::env::var("EXECUTOR_DATA_DIR")
        .ok()
        .filter(|d| !d.is_empty())
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| format!("{h}/.executor/v2/cli"))
        })?;
    let raw = std::fs::read_to_string(format!("{data_dir}/keys.json")).ok()?;
    serde_json::from_str::<Value>(&raw)
        .ok()?
        .get("apiKey")?
        .as_str()
        .filter(|k| !k.is_empty())
        .map(str::to_string)
}

/// Open an MCP session to the Executor endpoint: anonymous first, and on
/// 401/403 retry once with the local API key as bearer.
fn open_executor_mcp() -> std::result::Result<Connection, McpError> {
    let url = endpoint_url();
    match Connection::open(&url, None, Duration::from_secs(300)) {
        Ok(conn) => Ok(conn),
        Err(McpError::Unauthorized { .. }) => match local_api_key() {
            Some(key) => Connection::open(&url, Some(&key), Duration::from_secs(300)),
            None => Err(McpError::Unauthorized {
                status: 401,
                detail: format!(
                    "Executor at {url} requires auth and no key was found. \
                     Set EXECUTOR_API_KEY or sign in so keys.json exists under \
                     the Executor data dir, then retry."
                ),
            }),
        },
        Err(other) => Err(other),
    }
}

fn mcp_error(e: McpError) -> NurError {
    NurError::Tool(match e {
        McpError::Unauthorized { detail, .. } => detail,
        McpError::SessionExpired => "the Executor MCP session expired - retry the call".into(),
        McpError::Rpc { code, message } => format!("Executor MCP error {code}: {message}"),
        McpError::Transport(detail) => format!(
            "cannot reach the Executor MCP endpoint ({}). Is the daemon running? \
             `executor service start` starts it (no OS autostart - run it again after a reboot). \
             Detail: {detail}",
            endpoint_url()
        ),
    })
}

/// Render a `CallToolResult`: error results become tool errors, otherwise
/// the structured content (or the text content) as JSON.
fn render_call_result(result: Value) -> Result<String> {
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        let text = result
            .get("content")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|i| i.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "the Executor tool call failed".to_string());
        return Err(NurError::Tool(text));
    }
    if let Some(structured) = result.get("structuredContent") {
        return Ok(serde_json::to_string_pretty(structured)
            .unwrap_or_else(|_| structured.to_string()));
    }
    if let Some(content) = result.get("content").and_then(Value::as_array) {
        let text: Vec<&str> = content
            .iter()
            .filter_map(|i| i.get("text").and_then(Value::as_str))
            .collect();
        if !text.is_empty() {
            return Ok(text.join("\n"));
        }
    }
    Ok(serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()))
}

/// V2 tool operations over the MCP endpoint.
fn v2_mcp(action: &str, args: &Value) -> Result<String> {
    let mut conn = open_executor_mcp().map_err(mcp_error)?;
    let result = match action {
        // `skills({})` lists the configured apps (the V2 replacement for
        // the V1 integrations list).
        "sources" => conn
            .call_tool("skills", json!({}))
            .map_err(mcp_error)?,
        "search" => {
            // Pagination: the endpoint returns a `next` token; pass it back
            // as the whole search input on the next call.
            if let Ok(next) = arg_str(args, "next") {
                conn.call_tool("search", json!(next)).map_err(mcp_error)?
            } else {
                let q = arg_str(args, "query")?;
                conn.call_tool("search", json!({ "query": q }))
                    .map_err(mcp_error)?
            }
        }
        // Exact input shape unverified live (see module docs): the docs give
        // the JS form `tools.search.describe({ paths })`; we send `paths`.
        "describe" => {
            let paths = match args.get("paths") {
                Some(Value::Array(items)) => items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>(),
                Some(Value::String(one)) => vec![one.clone()],
                _ => {
                    return Err(NurError::Tool(
                        "describe needs `paths`: an array of exact tool paths from search"
                            .into(),
                    ));
                }
            };
            conn.call_tool("describe", json!({ "paths": paths }))
                .map_err(mcp_error)?
        }
        "call" => {
            // One dotted path (`tools.<app>.<tool>`), or namespace+tool
            // joined with `.` for back-compat with the V1 call shape.
            let path = match arg_str(args, "path") {
                Ok(p) => p,
                Err(_) => {
                    let ns = arg_str(args, "namespace")?;
                    let tool = arg_str(args, "tool")?;
                    format!("{ns}.{tool}")
                }
            };
            let input = match args.get("args_json") {
                Some(Value::String(s)) => serde_json::from_str::<Value>(s)
                    .map_err(|e| NurError::Tool(format!("args_json is not valid JSON: {e}")))?,
                Some(v @ Value::Object(_)) => v.clone(),
                _ => json!({}),
            };
            conn.call_tool("call", json!({ "calls": [{ "path": path, "input": input }] }))
                .map_err(mcp_error)?
        }
        // Answer an approval/input pause: requestId plus whatever the
        // server asked for (passed through `args_json` as an object).
        "resume" => {
            let request_id = arg_str(args, "request_id")?;
            let mut payload = json!({ "requestId": request_id });
            if let Some(Value::Object(extra)) = args.get("args_json") {
                if let Value::Object(map) = &mut payload {
                    for (k, v) in extra {
                        map.insert(k.clone(), v.clone());
                    }
                }
            }
            conn.call_tool("resume", payload).map_err(mcp_error)?
        }
        "execute" => {
            let program = arg_str(args, "program")?;
            conn.call_tool("execute", json!({ "program": program }))
                .map_err(mcp_error)?
        }
        other => return Err(NurError::Tool(format!("unknown executor action '{other}'"))),
    };
    render_call_result(result)
}

/// V1 CLI path (legacy, while a V1 binary is installed). Unchanged from
/// the pre-V2 wrapper.
fn v1_cli(bin: &str, action: &str, args: &Value) -> Result<String> {
    let argv: Vec<String> = match action {
        "status" | "help" => vec!["--help".into()],
        "install" | "start" => vec!["install".into()],
        // `tools sources` was renamed `tools integrations` (Executor 1.6).
        "sources" => vec!["tools".into(), "integrations".into()],
        "search" => {
            let q = arg_str(args, "query")?;
            vec!["tools".into(), "search".into(), q]
        }
        "call" => {
            let ns = arg_str(args, "namespace")?;
            let tool = arg_str(args, "tool")?;
            let mut a = vec!["call".into(), ns, tool];
            if let Ok(json) = arg_str(args, "args_json") {
                a.push(json);
            }
            a
        }
        other => return Err(NurError::Tool(format!("unknown executor action '{other}'"))),
    };
    let refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
    ecosystem::run_capture(bin, &refs, None, 120_000).map_err(NurError::Tool)
}

pub fn is_read_only_action(args: &str) -> bool {
    let action = serde_json::from_str::<Value>(args)
        .ok()
        .and_then(|v| v.get("action")?.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "status".into());
    matches!(
        action.as_str(),
        "status" | "sources" | "search" | "describe" | "help" | "pair"
    )
}

impl Tool for ExecutorTool {
    fn name(&self) -> &str {
        "executor"
    }

    fn description(&self) -> &str {
        "Executor MCP gateway (executor.sh): unified catalog of OpenAPI/GraphQL/MCP \
         integrations. action=status|start|stop|sources|search|describe|call|resume|execute|pair|upgrade. \
         Prefer for external SaaS/APIs; use native Meta tools for local repo work. \
         Auto-installed with Nur."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["status", "start", "stop", "sources", "search", "describe",
                             "call", "resume", "execute", "pair", "upgrade", "help"],
                    "default": "status"
                },
                "query": {
                    "type": "string",
                    "description": "For search: natural-language tool query"
                },
                "next": {
                    "type": "string",
                    "description": "For search: the `next` token from a previous search result, passed back as the whole input for the next page"
                },
                "paths": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "For describe: exact tool paths from search"
                },
                "path": {
                    "type": "string",
                    "description": "For call: dotted tool path, e.g. tools.github.issues.create"
                },
                "namespace": {
                    "type": "string",
                    "description": "For call: integration namespace (joined with tool when no path is given)"
                },
                "tool": {
                    "type": "string",
                    "description": "For call: tool name within namespace"
                },
                "args_json": {
                    "type": "string",
                    "description": "For call: JSON object string of arguments. For resume: JSON object with the server's requested answer fields"
                },
                "request_id": {
                    "type": "string",
                    "description": "For resume: the requestId of the paused call"
                },
                "program": {
                    "type": "string",
                    "description": "For execute: a JS program over the apps (tools.search(...), tools.<app>.<tool>(...))"
                }
            }
        })
    }

    fn execute(&self, args: &Value, _ctx: &ToolContext) -> Result<String> {
        let bin = ecosystem::find_bin("executor").ok_or_else(|| {
            NurError::Tool(
                "executor CLI not found. Meta auto-installs it — npm i -g executor@beta \
                 then nur ecosystem ensure"
                    .into(),
            )
        })?;

        let action = arg_str(args, "action").unwrap_or_else(|_| "status".into());
        let version = executor_version(&bin);
        // V2 (2.x) is the current line; unparseable output falls back to
        // the V1 CLI path, the pre-existing behaviour.
        let is_v2 = version.as_ref().is_some_and(|(major, _)| *major >= 2);
        if !is_v2 {
            return v1_cli(&bin, &action, args);
        }

        // Daemon management stays on the CLI in V2.
        let cli_argv: Option<Vec<&str>> = match action.as_str() {
            "status" => Some(vec!["service", "status"]),
            "help" => Some(vec!["--help"]),
            "start" | "install" => Some(vec!["service", "start"]),
            "stop" => Some(vec!["service", "stop"]),
            "pair" => Some(vec!["pair"]),
            "upgrade" => Some(vec!["upgrade", "--yes"]),
            _ => None,
        };
        if let Some(argv) = cli_argv {
            return ecosystem::run_capture(&bin, &argv, None, 120_000).map_err(NurError::Tool);
        }
        v2_mcp(&action, args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parses_v2_output_with_prefix_and_notice() {
        assert_eq!(
            parse_version("executor v2.0.0-beta.37"),
            Some((2, "2.0.0-beta.37".into()))
        );
        assert_eq!(
            parse_version(
                "Downloading Executor 2.0.0-beta.37 for linux-x64 (first run of this version)\nexecutor v2.0.0-beta.37"
            ),
            Some((2, "2.0.0-beta.37".into()))
        );
    }

    #[test]
    fn version_parses_v1_and_bare_forms() {
        assert_eq!(parse_version("1.6.11"), Some((1, "1.6.11".into())));
        assert_eq!(
            parse_version("executor version 1.6.10"),
            Some((1, "1.6.10".into()))
        );
        assert_eq!(parse_version("no version here"), None);
    }

    #[test]
    fn read_only_set_matches_the_v2_action_table() {
        let ro = |action: &str| {
            is_read_only_action(&serde_json::json!({"action": action}).to_string())
        };
        for action in ["status", "sources", "search", "describe", "help", "pair"] {
            assert!(ro(action), "{action} should be read-only");
        }
        for action in ["call", "execute", "resume", "start", "stop", "upgrade", "install"] {
            assert!(!ro(action), "{action} should not be read-only");
        }
    }

    #[test]
    fn error_results_become_tool_errors() {
        let err = render_call_result(json!({
            "isError": true,
            "content": [{"type": "text", "text": "no such tool"}]
        }));
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("no such tool"));
    }

    #[test]
    fn structured_content_renders_as_json() {
        let out = render_call_result(json!({
            "structuredContent": {"tools": ["a", "b"]}
        }))
        .unwrap();
        assert!(out.contains("\"a\""));
    }
}

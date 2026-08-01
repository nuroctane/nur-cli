//! In-terminal Chromium via [terminal-browser](https://terminal-browser.com/).
//!
//! Separate from the `browser` tool (real Chrome + MV3 extension). This one
//! paints Chromium inside a kitty-graphics terminal pane and drives open tabs
//! with an agent-browser-compatible `action` CLI.

use super::{arg_str, Tool, ToolContext};
use crate::error::{NurError, Result};
use crate::terminal_browser;
use serde_json::Value;
use std::path::Path;

pub struct TerminalBrowser;

/// Perception / status are free; open/setup/action that mutates pages need approval.
pub fn is_read_only_action(args: &str) -> bool {
    let action = serde_json::from_str::<Value>(args)
        .ok()
        .and_then(|v| v.get("action")?.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "status".into());
    matches!(
        action.as_str(),
        "status" | "doctor" | "help" | "ls" | "version"
    )
}

/// Plan mode: perception only - match `browser` (read-only + screenshot/snapshot).
/// `eval` maps to in-page JS exec and is intentionally excluded.
pub fn is_plan_safe_action(args: &str) -> bool {
    if is_read_only_action(args) {
        return true;
    }
    let Ok(v) = serde_json::from_str::<Value>(args) else {
        return false;
    };
    if v.get("action").and_then(|a| a.as_str()) != Some("action") {
        return false;
    }
    let cmd = action_passthrough(&v);
    cmd.first()
        .map(|c| matches!(c.as_str(), "screenshot" | "snapshot"))
        .unwrap_or(false)
}

impl Tool for TerminalBrowser {
    fn name(&self) -> &str {
        "terminal_browser"
    }

    fn description(&self) -> &str {
        "Terminal browser ([terminal-browser.com](https://terminal-browser.com/)) - \
         show a site or local HTML beside the agent, and drive open tabs. \
         Actions: status|doctor|help|ls|setup|open|action. \
         Prefer open with split=right (default). Then action with \
         command=\"snapshot\" / \"click @eN\" / \"fill @eN text\". \
         On Windows: uses a host fallback via agent-browser-cli (real Chrome) \
         when the upstream binary is unavailable; WSL native binary preferred \
         when installed. Distinct from the `browser` tool name."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["status", "doctor", "help", "ls", "setup", "open", "action", "version"],
                    "default": "status"
                },
                "url": {
                    "type": "string",
                    "description": "For open: URL, localhost:PORT, or path to a local .html file"
                },
                "split": {
                    "type": "string",
                    "enum": ["right", "left", "down", "up", "none"],
                    "description": "For open: pane split (default right). none = take over current pane when TTY allows"
                },
                "size": {
                    "type": "number",
                    "description": "For open+split: fraction of space 0.2–0.95"
                },
                "all": {
                    "type": "boolean",
                    "description": "For ls: every browser, not only this terminal tab"
                },
                "json": {
                    "type": "boolean",
                    "description": "For ls: machine-readable JSON"
                },
                "command": {
                    "type": "string",
                    "description": "For action: agent-browser command after -- (e.g. \"snapshot\", \"click @e14\", \"fill @e3 hello\")"
                },
                "args": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "For action: argv after -- (preferred over command when you need exact tokens)"
                },
                "browser": {
                    "type": "string",
                    "description": "For action: browser key from ls"
                },
                "tab": {
                    "type": "string",
                    "description": "For action: tab id from ls"
                },
                "target": {
                    "type": "string",
                    "description": "For action: CDP target id"
                },
                "follow": {
                    "type": "boolean",
                    "description": "For action: bring the tab to the front first"
                }
            }
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action").unwrap_or_else(|_| "status".into());
        match action.as_str() {
            "status" | "doctor" => Ok(terminal_browser::doctor_report()),
            "version" => terminal_browser::run_tb_cancelled(
                &["--version"],
                Some(&ctx.cwd),
                15_000,
                &ctx.cancel,
            )
            .or_else(|_| {
                terminal_browser::run_tb_cancelled(&["help"], Some(&ctx.cwd), 15_000, &ctx.cancel)
            })
            .map_err(NurError::Tool),
            "help" => {
                terminal_browser::run_tb_cancelled(&["help"], Some(&ctx.cwd), 15_000, &ctx.cancel)
                    .map_err(NurError::Tool)
            }
            "ls" => {
                let mut argv = vec!["ls".to_string()];
                if args.get("all").and_then(|v| v.as_bool()).unwrap_or(false) {
                    argv.push("--all".into());
                }
                if args.get("json").and_then(|v| v.as_bool()).unwrap_or(false) {
                    argv.push("--json".into());
                }
                let refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
                terminal_browser::run_tb_cancelled(&refs, Some(&ctx.cwd), 30_000, &ctx.cancel)
                    .map_err(NurError::Tool)
            }
            "setup" => {
                // Explicit user/agent opt-in: may run the published installer.
                let _ = terminal_browser::try_install_native();
                match terminal_browser::run_tb_cancelled(
                    &["setup"],
                    Some(&ctx.cwd),
                    60_000,
                    &ctx.cancel,
                ) {
                    Ok(s) => Ok(s),
                    Err(e) => Ok(format!(
                        "{e}\n\nInstall hint: {}\n\
                         On Windows, `nur ecosystem ensure` installs the agent-browser-cli host fallback.",
                        terminal_browser::INSTALL_HINT
                    )),
                }
            }
            "open" => open_browser(args, &ctx.cwd, &ctx.cancel),
            "action" => run_action(args, &ctx.cwd, &ctx.cancel),
            other => Err(NurError::Tool(format!(
                "unknown terminal_browser action '{other}'"
            ))),
        }
    }
}

fn open_browser(
    args: &Value,
    cwd: &Path,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<String> {
    let url = arg_str(args, "url").unwrap_or_default();
    let url = url.trim();
    let split = arg_str(args, "split").unwrap_or_else(|_| "right".into());
    let split = split.trim().to_ascii_lowercase();

    let mut argv = vec!["open".to_string()];
    if !url.is_empty() {
        argv.push(resolve_open_target(cwd, url)?);
    }
    if split != "none" && !split.is_empty() {
        if !matches!(split.as_str(), "right" | "left" | "down" | "up") {
            return Err(NurError::Tool(format!(
                "invalid split '{split}' (right|left|down|up|none)"
            )));
        }
        argv.push("--split".into());
        argv.push(split);
        if let Some(size) = args.get("size").and_then(|v| v.as_f64()) {
            if !(0.2..=0.95).contains(&size) {
                return Err(NurError::Tool(
                    "size must be a fraction between 0.2 and 0.95".into(),
                ));
            }
            argv.push("--size".into());
            argv.push(size.to_string());
        }
    }

    let refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
    // Opening a pane can take a few seconds while Chromium boots.
    let out = terminal_browser::run_tb_cancelled(&refs, Some(cwd), 60_000, cancel)
        .map_err(NurError::Tool)?;
    Ok(format!(
        "{out}\n\nTip: terminal_browser action command=\"snapshot\" then click/fill with @e refs."
    ))
}

fn run_action(
    args: &Value,
    cwd: &Path,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<String> {
    let passthrough = action_passthrough(args);
    if passthrough.is_empty() {
        return Err(NurError::Tool(
            "action requires command= or args=[…] after -- (e.g. command=\"snapshot\")".into(),
        ));
    }
    // Mirror upstream guardrails for dangerous agent-browser entrypoints.
    let head = passthrough[0].as_str();
    if matches!(head, "launch" | "install" | "connect" | "disconnect") {
        return Err(NurError::Tool(format!(
            "{head} is not available through terminal-browser action - use open for new panes"
        )));
    }

    let mut argv = vec!["action".to_string()];
    if let Ok(browser) = arg_str(args, "browser") {
        if !browser.trim().is_empty() {
            argv.push("--browser".into());
            argv.push(browser);
        }
    }
    if let Ok(tab) = arg_str(args, "tab") {
        if !tab.trim().is_empty() {
            argv.push("--tab".into());
            argv.push(tab);
        }
    }
    if let Ok(target) = arg_str(args, "target") {
        if !target.trim().is_empty() {
            argv.push("--target".into());
            argv.push(target);
        }
    }
    if args
        .get("follow")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        argv.push("--follow".into());
    }
    argv.push("--".into());
    argv.extend(passthrough);

    let refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
    terminal_browser::run_tb_cancelled(&refs, Some(cwd), 120_000, cancel).map_err(NurError::Tool)
}

fn action_passthrough(args: &Value) -> Vec<String> {
    if let Some(arr) = args.get("args").and_then(|v| v.as_array()) {
        let tokens: Vec<String> = arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .filter(|s| !s.is_empty())
            .collect();
        if !tokens.is_empty() {
            return tokens;
        }
    }
    let Ok(command) = arg_str(args, "command") else {
        return Vec::new();
    };
    shell_split(command.trim())
}

/// Minimal whitespace splitter that keeps quoted segments intact.
fn shell_split(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for ch in s.chars() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (None, '"' | '\'') => quote = Some(ch),
            (None, c) if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            (_, c) => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn resolve_open_target(cwd: &Path, url: &str) -> Result<String> {
    let trimmed = url.trim();
    // Allowlist schemes with :// (http/https/file). Reject javascript:/data:/…
    if let Some((scheme, rest)) = trimmed.split_once("://") {
        let scheme = scheme.to_ascii_lowercase();
        if !matches!(scheme.as_str(), "http" | "https" | "file") {
            return Err(NurError::Other(format!(
                "terminal_browser open: unsupported URL scheme `{scheme}` (use http/https/file)"
            )));
        }
        if scheme == "file" {
            let path = rest.trim_start_matches('/');
            let p = crate::tools::resolve_path(cwd, path)?;
            return Ok(p.to_string_lossy().into_owned());
        }
        return Ok(trimmed.to_string());
    }
    if trimmed.starts_with("localhost") || trimmed.starts_with("127.0.0.1") {
        return Ok(trimmed.to_string());
    }
    // host:port (digits only after ':') vs bare schemes like `javascript:alert(1)`.
    if let Some((host, port)) = trimmed.split_once(':') {
        if !host.is_empty()
            && !host.contains('\\')
            && !host.contains('/')
            && !port.is_empty()
            && port.chars().all(|c| c.is_ascii_digit())
        {
            return Ok(trimmed.to_string());
        }
        // Multi-letter alphabetic prefix without :// is a disallowed scheme.
        if host.len() > 1 && host.chars().all(|c| c.is_ascii_alphabetic()) {
            return Err(NurError::Other(format!(
                "terminal_browser open: unsupported URL scheme `{}` (use http/https/file)",
                host.to_ascii_lowercase()
            )));
        }
    }
    let p = crate::tools::resolve_path(cwd, trimmed)?;
    Ok(p.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_classification() {
        assert!(is_read_only_action(r#"{"action":"ls"}"#));
        assert!(is_read_only_action(r#"{"action":"status"}"#));
        assert!(!is_read_only_action(
            r#"{"action":"open","url":"https://x"}"#
        ));
        assert!(!is_read_only_action(
            r#"{"action":"action","command":"click @e1"}"#
        ));
    }

    #[test]
    fn plan_safe_snapshot() {
        assert!(is_plan_safe_action(
            r#"{"action":"action","command":"snapshot"}"#
        ));
        assert!(is_plan_safe_action(
            r#"{"action":"action","command":"screenshot"}"#
        ));
        assert!(!is_plan_safe_action(
            r#"{"action":"action","command":"click @e1"}"#
        ));
        assert!(
            !is_plan_safe_action(r#"{"action":"action","command":"eval 1+1"}"#),
            "eval is in-page JS - not plan-safe"
        );
    }

    #[test]
    fn open_rejects_javascript_scheme() {
        let err = resolve_open_target(Path::new("."), "javascript:alert(1)").unwrap_err();
        assert!(err.to_string().contains("unsupported URL scheme"));
    }

    #[test]
    fn shell_split_keeps_quotes() {
        assert_eq!(
            shell_split(r#"fill @e3 "hello world""#),
            vec!["fill", "@e3", "hello world"]
        );
    }

    #[test]
    fn passthrough_prefers_args_array() {
        let v = serde_json::json!({
            "args": ["fill", "@e3", "hi"],
            "command": "ignored"
        });
        assert_eq!(action_passthrough(&v), vec!["fill", "@e3", "hi"]);
    }
}

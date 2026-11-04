//! Native accessibility-tree desktop control via the agent-desktop executable.
use super::{arg_str, Tool, ToolContext};
use crate::error::{NurError, Result};
use serde_json::{json, Value};
use std::path::Path;

pub struct Desktop;

/// Resolve npm's Windows shim to its packaged binary without invoking cmd.exe.
fn native_executable(path: String) -> String {
    #[cfg(windows)]
    {
        if path.to_ascii_lowercase().ends_with(".cmd") {
            let arch = if cfg!(target_arch = "aarch64") {
                "arm64"
            } else {
                "x64"
            };
            if let Some(parent) = Path::new(&path).parent() {
                let binary = parent
                    .join("node_modules")
                    .join("agent-desktop")
                    .join("bin")
                    .join(format!("agent-desktop-win32-{arch}.exe"));
                if binary.is_file() {
                    return binary.to_string_lossy().into_owned();
                }
            }
        }
    }
    path
}

impl Tool for Desktop {
    fn name(&self) -> &str {
        "desktop"
    }

    fn description(&self) -> &str {
        "Inspect and operate native desktop apps with agent-desktop accessibility refs. \
         Start with status and list_apps, then snapshot an app, act on a qualified ref, \
         and snapshot again. Stale refs require a new snapshot. Actions can affect real apps; \
         after a timeout inspect the app before retrying, since input may already have been delivered."
    }

    fn parameters_schema(&self) -> Value {
        json!({"type":"object", "required":["action"], "properties": {
            "action": {"type":"string", "enum":["status","list_apps","list_windows","snapshot","click","double_click","right_click","type","set_value","clear","focus","select","toggle","check","uncheck","expand","collapse","scroll_to","press"]},
            "app": {"type":"string", "description":"App name for snapshot or press"},
            "window_id": {"type":"string", "description":"Window id for snapshot or press"},
            "target": {"type":"string", "description":"Qualified accessibility ref from a snapshot"},
            "text": {"type":"string", "description":"Text or selected option"},
            "key": {"type":"string", "description":"Key combination for press"},
            "headed": {"type":"boolean", "description":"Permit physical input for this action"}
        }})
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action")?;
        let executable = std::env::var("NUR_AGENT_DESKTOP_BIN").ok()
            .or_else(|| crate::ecosystem::find_bin("agent-desktop"))
            .ok_or_else(|| NurError::Tool("Desktop binary not found. Install the native agent-desktop release and put it on PATH, or set NUR_AGENT_DESKTOP_BIN.".into()))?;
        let executable = native_executable(executable);
        if !Path::new(&executable).is_file() {
            return Err(NurError::Tool(format!(
                "Desktop binary not found: {executable}"
            )));
        }
        let mut argv = Vec::<String>::new();
        if args.get("headed").and_then(Value::as_bool).unwrap_or(false) {
            argv.push("--headed".into());
        }
        let command = action.replace('_', "-");
        match action.as_str() {
            "status" | "list_apps" | "list_windows" => argv.push(command),
            "snapshot" => {
                argv.push(command);
                if let Some(app) = args.get("app").and_then(Value::as_str) {
                    argv.push(format!("--app={app}"));
                }
                if let Some(window) = args.get("window_id").and_then(Value::as_str) {
                    argv.push(format!("--window-id={window}"));
                }
                if let Some(root) = args.get("target").and_then(Value::as_str) {
                    argv.push(format!("--root={root}"));
                }
                argv.push("--compact".into());
            }
            "press" => {
                argv.push(command);
                if let Some(app) = args.get("app").and_then(Value::as_str) {
                    argv.push(format!("--app={app}"));
                }
                if let Some(window) = args.get("window_id").and_then(Value::as_str) {
                    argv.push(format!("--window-id={window}"));
                }
                argv.push("--".into());
                argv.push(arg_str(args, "key")?);
            }
            "click" | "double_click" | "right_click" | "type" | "set_value" | "clear" | "focus"
            | "select" | "toggle" | "check" | "uncheck" | "expand" | "collapse" | "scroll_to" => {
                let target = arg_str(args, "target")?;
                if !target.starts_with('@') {
                    return Err(NurError::Tool(
                        "Desktop actions require a qualified ref from a snapshot".into(),
                    ));
                }
                argv.push(command);
                argv.push("--".into());
                argv.push(target);
                if matches!(action.as_str(), "type" | "set_value" | "select") {
                    argv.push(arg_str(args, "text")?);
                }
            }
            _ => return Err(NurError::Tool(format!("Unknown desktop action: {action}"))),
        }
        let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
        crate::ecosystem::run_native_tool_capture(
            &executable,
            &refs,
            Some(&ctx.cwd),
            30_000,
            &ctx.cancel,
        )
        .map_err(NurError::Tool)
    }
}

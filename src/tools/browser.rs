//! Real-Chrome perception & control — agent-browser-cli
//! https://github.com/sleepinginsummer/agent-browser-cli
//!
//! Unlike `web_fetch` (text-only, no cookies), this drives the user's *actual*
//! Chrome session through a MV3 extension bridge, so login state is preserved:
//! scan tabs, snapshot pages into `@e` element refs, click/fill/type, run JS,
//! capture screenshots (feed them to `look` for vision), and record
//! network/console activity. Requires the CLI (auto-provisioned via npm) plus
//! the `tmwd_cdp_bridge` Chrome extension loaded once by the user.
//!
//! Cookie reading is deliberately NOT exposed — session secrets stay out of
//! the model's context.

use super::{arg_str, Tool, ToolContext};
use crate::ecosystem;
use crate::error::{MuseError, Result};
use serde_json::Value;

pub struct BrowserTool;

const BIN: &str = "agent-browser-cli";

/// Pure perception is free; anything that changes tabs/pages needs approval.
/// Screenshots write an image file (like `extract_frames`) so they are not
/// read-only, but plan mode allows them explicitly (see loop.rs).
pub fn is_read_only_action(args: &str) -> bool {
    let action = serde_json::from_str::<Value>(args)
        .ok()
        .and_then(|v| v.get("action")?.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "tabs".into());
    matches!(
        action.as_str(),
        "tabs" | "scan" | "snapshot" | "tabtree" | "status" | "console" | "network" | "doctor"
    )
}

/// Plan mode additionally allows `screenshot` — pure perception that happens
/// to write an image file, exactly like `extract_frames`.
pub fn is_plan_safe_action(args: &str) -> bool {
    if is_read_only_action(args) {
        return true;
    }
    serde_json::from_str::<Value>(args)
        .ok()
        .and_then(|v| v.get("action")?.as_str().map(|s| s == "screenshot"))
        .unwrap_or(false)
}

impl Tool for BrowserTool {
    fn name(&self) -> &str {
        "browser"
    }

    fn description(&self) -> &str {
        "Perceive and control the user's real Chrome (login state preserved) via \
         agent-browser-cli. Perception: tabs | scan | snapshot (page → @e element \
         refs) | tabtree | console | network | status | doctor. Control: open url | \
         click @e | fill @e text | send_keys | exec js | screenshot (then `look` at \
         the saved image) | close. Prefer web_fetch for plain pages; use this when \
         the page needs a session, interaction, or visual confirmation. Needs the \
         Chrome extension loaded once (chrome://extensions → load unpacked)."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["tabs", "scan", "snapshot", "tabtree", "status", "doctor",
                             "console", "network",
                             "open", "close", "click", "fill", "send_keys", "exec",
                             "screenshot"],
                    "default": "tabs"
                },
                "url": {"type": "string", "description": "For open: target URL"},
                "target": {"type": "string", "description": "For click/fill/send_keys: @e element ref from snapshot"},
                "text": {"type": "string", "description": "For fill: text to enter"},
                "keys": {"type": "string", "description": "For send_keys: key sequence, e.g. Enter"},
                "js": {"type": "string", "description": "For exec: JavaScript to run in the page"},
                "tab": {"type": "string", "description": "Optional tab id (from tabs/tabtree)"},
                "full_page": {"type": "boolean", "description": "For screenshot: capture the full page"}
            }
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let bin = ecosystem::find_bin(BIN).ok_or_else(|| {
            MuseError::Tool(
                "agent-browser-cli not found. Meta auto-installs it — or run \
                 `nur browser setup` to stage the extension and finish one-time \
                 setup for your default browser."
                    .into(),
            )
        })?;

        let action = arg_str(args, "action").unwrap_or_else(|_| "tabs".into());
        let mut argv: Vec<String> = match action.as_str() {
            "tabs" => vec!["tabs".into()],
            "scan" => vec!["scan".into()],
            "snapshot" => vec!["snapshot".into()],
            "tabtree" => vec!["tabtree".into()],
            "status" => {
                // Fold in the local setup state (default browser + extension
                // staging) so the model can self-diagnose a disconnected bridge.
                let setup = crate::ecosystem::browser_setup::setup_summary();
                let live = ecosystem::run_capture(&bin, &["status"], Some(&ctx.cwd), 30_000)
                    .unwrap_or_else(|e| format!("(bridge status unavailable: {e})"));
                return Ok(format!("{setup}\n\nbridge:\n{live}"));
            }
            "doctor" => vec!["doctor".into()],
            "console" => vec!["console".into(), "list".into()],
            "network" => vec!["network".into(), "list".into()],
            "open" => vec!["open".into(), arg_str(args, "url")?],
            "close" => vec!["close".into()],
            "click" => vec!["click".into(), arg_str(args, "target")?],
            "fill" => vec![
                "fill".into(),
                arg_str(args, "target")?,
                arg_str(args, "text")?,
            ],
            "send_keys" => {
                let mut a = vec!["send-keys".into(), arg_str(args, "keys")?];
                if let Ok(t) = arg_str(args, "target") {
                    a.push("--target".into());
                    a.push(t);
                }
                a
            }
            "exec" => vec!["exec".into(), arg_str(args, "js")?],
            "screenshot" => {
                let mut a = vec!["screenshot".into()];
                if args
                    .get("full_page")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    a.push("--full-page".into());
                }
                a
            }
            other => {
                return Err(MuseError::Tool(format!("unknown browser action '{other}'")));
            }
        };
        if let Ok(tab) = arg_str(args, "tab") {
            if !tab.trim().is_empty() {
                argv.push("--tab".into());
                argv.push(tab);
            }
        }

        let refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
        ecosystem::run_capture(&bin, &refs, Some(&ctx.cwd), 120_000).map_err(MuseError::Tool)
    }
}

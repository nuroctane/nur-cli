//! Read-only git diff / log — approval-free repo inspection.

use super::{arg_str, Tool, ToolContext};
use crate::error::{MuseError, Result};
use serde_json::Value;
use std::process::Command;

pub struct GitDiff;

impl Tool for GitDiff {
    fn name(&self) -> &str {
        "git_diff"
    }

    fn description(&self) -> &str {
        "Read-only git inspection. mode: diff (unstaged), staged, log (recent commits), \
         show (one commit). Optional path filter and ref (for show/log)."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "mode": {"type": "string", "enum": ["diff", "staged", "log", "show"], "default": "diff"},
                "path": {"type": "string", "description": "Limit to a file/dir (optional)"},
                "ref": {"type": "string", "description": "Commit-ish for show, or log start point (optional)"}
            }
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let mode = arg_str(args, "mode").unwrap_or_else(|_| "diff".into());
        let path = arg_str(args, "path").ok();
        let ref_ = arg_str(args, "ref").ok();
        // Refuse anything that parses as a flag — this tool is read-only.
        for v in [&path, &ref_] {
            if let Some(s) = v {
                if s.starts_with('-') {
                    return Err(MuseError::Tool(format!("invalid argument: {s}")));
                }
            }
        }

        let mut cmd = Command::new("git");
        cmd.current_dir(&ctx.cwd);
        match mode.as_str() {
            "diff" => {
                cmd.args(["diff", "--no-color"]);
            }
            "staged" => {
                cmd.args(["diff", "--staged", "--no-color"]);
            }
            "log" => {
                cmd.args(["log", "--oneline", "--decorate", "--no-color", "-n", "25"]);
                if let Some(r) = &ref_ {
                    cmd.arg(r);
                }
            }
            "show" => {
                cmd.args(["show", "--stat", "--no-color"]);
                cmd.arg(ref_.as_deref().unwrap_or("HEAD"));
            }
            other => {
                return Err(MuseError::Tool(format!(
                    "unknown mode '{other}' — use diff|staged|log|show"
                )))
            }
        }
        if let Some(p) = &path {
            cmd.args(["--", p]);
        }

        let out = cmd
            .output()
            .map_err(|e| MuseError::Tool(format!("git: {e}")))?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        if !out.status.success() {
            return Err(MuseError::Tool(format!(
                "git {mode} failed: {}",
                stderr.trim()
            )));
        }
        let text = stdout.trim();
        if text.is_empty() {
            return Ok(format!("(git {mode}: no output — clean)"));
        }
        Ok(truncate(text, 60_000))
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…\n[truncated {} chars]", &s[..end], s.len())
}

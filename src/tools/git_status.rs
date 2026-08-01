use super::{Tool, ToolContext};
use crate::error::{NurError, Result};
use serde_json::Value;
use std::process::Command;

pub struct GitStatus;

impl Tool for GitStatus {
    fn name(&self) -> &str {
        "git_status"
    }

    fn description(&self) -> &str {
        "Read-only git snapshot: branch, status --short, and recent log (5). Fast orientation."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn execute(&self, _args: &Value, ctx: &ToolContext) -> Result<String> {
        let branch = git(&ctx.cwd, &["rev-parse", "--abbrev-ref", "HEAD"])?;
        let status = git(&ctx.cwd, &["status", "--short", "-b"])?;
        let log = git(
            &ctx.cwd,
            &["log", "-5", "--oneline", "--decorate", "--no-color"],
        )
        .unwrap_or_else(|_| "(no commits)".into());
        Ok(format!(
            "branch: {}\n\nstatus:\n{}\n\nrecent:\n{}",
            branch.trim(),
            status.trim(),
            log.trim()
        ))
    }
}

fn git(cwd: &std::path::Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| NurError::Tool(format!("git failed: {e}")))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(NurError::Tool(format!("git error: {err}")));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

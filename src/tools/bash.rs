use super::sandbox;
use super::shell::{run_in_shell, shell_backend};
use super::{arg_str, arg_u64, Tool, ToolContext};
use crate::error::{MuseError, Result};
use serde_json::Value;

pub struct Bash;

impl Tool for Bash {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        // Honest description — name kept as `bash` for model familiarity.
        "Run a shell command in the workspace cwd. \
         On Windows prefers Git Bash, then pwsh/PowerShell, then cmd.exe (reported in output). \
         Prefer non-interactive commands. Captures stdout/stderr."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {"type": "string"},
                "timeout_ms": {"type": "integer", "description": "Timeout in ms (default 120000)"}
            },
            "required": ["command"]
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        if sandbox::is_dangerous_workspace(&ctx.cwd) {
            return Err(MuseError::Tool(
                "refused: workspace is filesystem root — start muse from a project directory \
                 (or --cwd) before running shell commands"
                    .into(),
            ));
        }

        let command = arg_str(args, "command")?;
        let timeout_ms = arg_u64(args, "timeout_ms").unwrap_or(120_000);

        let lower = command.to_lowercase();
        let dangerous = [
            "rm -rf /",
            "rm -rf ~",
            "del /f /s /q",
            "format ",
            "shutdown",
            "mkfs",
            "rd /s /q c:",
            "remove-item -recurse -force c:",
        ];
        if dangerous.iter().any(|d| lower.contains(d)) {
            return Err(MuseError::Tool(format!(
                "refused potentially destructive command: {command}"
            )));
        }

        run_in_shell(shell_backend(), &command, &ctx.cwd, timeout_ms, &ctx.cancel)
    }
}

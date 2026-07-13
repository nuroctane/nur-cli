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

        if let Some(reason) = check_destructive(&command, &ctx.cwd) {
            return Err(MuseError::Tool(reason));
        }

        run_in_shell(shell_backend(), &command, &ctx.cwd, timeout_ms, &ctx.cancel)
    }
}

fn check_destructive(cmd: &str, cwd: &std::path::Path) -> Option<String> {
    let lower = cmd.to_lowercase();
    let trimmed = lower.trim();
    let _ = cwd;

    // Block encoded PowerShell that hides intent.
    if lower.contains("encodedcommand")
        || lower.contains("-enc ")
        || lower.contains("frombase64string")
    {
        return Some(format!(
            "refused: encoded PowerShell obscures intent: {cmd}"
        ));
    }
    // Fork bomb
    if lower.contains(":(){:|:&};:") || lower.contains("fork bomb") {
        return Some("refused: fork bomb detected".into());
    }

    // High-signal destructive substrings (always refuse).
    let always_block = [
        "rm -rf /",
        "rm -rf /*",
        "rm -rf ~",
        "rm -rf $home",
        "rm -rf $userprofile",
        "mkfs.",
        "mkfs -t",
        "dd if=",
        ">/dev/sda",
        ">/dev/nvme",
        "shutdown -h",
        "shutdown -s",
        "shutdown /s",
        "halt -f",
        "poweroff -f",
        "del /f /s /q c:",
        "rd /s /q c:",
        "remove-item -recurse -force c:",
        "format c:",
        "format d:",
        r"\\?\c:",
    ];
    for d in always_block {
        if lower.contains(d) {
            return Some(format!("refused destructive pattern '{d}' in: {cmd}"));
        }
    }

    // Flexible `rm … -rf … /` or `~` (flags in any order after rm).
    static RM_ROOT: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static RM_HOME: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let rm_root = RM_ROOT.get_or_init(|| {
        regex::Regex::new(r"(?i)\brm\s+-[a-z]*r[a-z]*f[a-z]*\s+/(?:\s|;|&|$)")
            .expect("rm root pattern")
    });
    let rm_home = RM_HOME.get_or_init(|| {
        regex::Regex::new(r"(?i)\brm\s+-[a-z]*r[a-z]*f[a-z]*.*\s+~(?:\s|;|&|$)")
            .expect("rm home pattern")
    });
    if rm_root.is_match(trimmed) {
        return Some(format!("refused rm -rf / detected: {cmd}"));
    }
    if rm_home.is_match(trimmed) {
        return Some(format!("refused rm -rf ~ detected: {cmd}"));
    }
    None
}

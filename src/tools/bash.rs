use super::sandbox;
use super::shell::{
    clamp_timeout_ms, run_in_shell, shell_backend, DEFAULT_TIMEOUT_MS, MAX_TIMEOUT_MS,
};
use super::{arg_str, arg_u64, Tool, ToolContext};
use crate::error::{NurError, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Recent failing bash fingerprints (cwd + normalized command). Only timeout /
/// idle / cancel / hang-kill are tracked — ordinary non-zero exits can be retried
/// with a different approach without a 15-minute lockout.
fn recent_failures() -> &'static Mutex<HashMap<String, Instant>> {
    static RECENT_FAILURES: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    RECENT_FAILURES.get_or_init(|| Mutex::new(HashMap::new()))
}
const FAILURE_TTL: Duration = Duration::from_secs(15 * 60);
const MAX_TRACKED_FAILURES: usize = 64;

pub struct Bash;

impl Tool for Bash {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        // Honest description — name kept as `bash` for model familiarity.
        "Run a non-interactive shell command in the workspace cwd (not a full OS sandbox). \
         On Windows prefers Git Bash, then pwsh/PowerShell, then cmd.exe (reported in output). \
         Default timeout 60s (max 180s). Idle commands with no output are killed. \
         Never use for search/file-read (use grep/glob/list_dir/read_file). \
         Do not retry identical timed-out/hung commands."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {"type": "string"},
                "timeout_ms": {
                    "type": "integer",
                    "description": format!(
                        "Timeout in ms (default {DEFAULT_TIMEOUT_MS}, hard max {MAX_TIMEOUT_MS})"
                    )
                }
            },
            "required": ["command"]
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        if sandbox::is_dangerous_workspace(&ctx.cwd) {
            return Err(NurError::Tool(
                "refused: workspace is filesystem root - start nur from a project directory \
                 (or --cwd) before running shell commands"
                    .into(),
            ));
        }

        let command = arg_str(args, "command")?;
        let timeout_ms =
            clamp_timeout_ms(arg_u64(args, "timeout_ms").unwrap_or(DEFAULT_TIMEOUT_MS));

        if let Some(reason) = check_destructive(&command, &ctx.cwd) {
            return Err(NurError::Tool(reason));
        }
        if let Some(reason) = check_hang_prone(&command) {
            return Err(NurError::Tool(reason));
        }
        if let Some(reason) = refuse_recent_failure(&ctx.cwd, &command) {
            return Err(NurError::Tool(reason));
        }

        match run_in_shell(shell_backend(), &command, &ctx.cwd, timeout_ms, &ctx.cancel) {
            Ok(out) => Ok(out),
            Err(e) => {
                // Only lock out hang/timeout/cancel — not ordinary tool refusals
                // from this Err path that are already terminal.
                let msg = e.to_string();
                if msg.contains("timed out")
                    || msg.contains("idle for")
                    || msg.contains("cancelled")
                {
                    note_failure(&ctx.cwd, &command);
                }
                Err(e)
            }
        }
    }
}

fn fingerprint(cwd: &Path, cmd: &str) -> String {
    let cwd_key = cwd
        .canonicalize()
        .unwrap_or_else(|_| cwd.to_path_buf())
        .to_string_lossy()
        .to_ascii_lowercase();
    let cmd_key = cmd.split_whitespace().collect::<Vec<_>>().join(" ");
    format!("{cwd_key}\0{cmd_key}")
}

fn note_failure(cwd: &Path, cmd: &str) {
    let key = fingerprint(cwd, cmd);
    if key.ends_with('\0') {
        return;
    }
    if let Ok(mut map) = recent_failures().lock() {
        map.retain(|_, at| at.elapsed() < FAILURE_TTL);
        if map.len() >= MAX_TRACKED_FAILURES {
            if let Some(old) = map
                .iter()
                .min_by_key(|(_, at)| **at)
                .map(|(k, _)| k.clone())
            {
                map.remove(&old);
            }
        }
        map.insert(key, Instant::now());
    }
}

fn refuse_recent_failure(cwd: &Path, cmd: &str) -> Option<String> {
    let key = fingerprint(cwd, cmd);
    let Ok(mut map) = recent_failures().lock() else {
        return None;
    };
    map.retain(|_, at| at.elapsed() < FAILURE_TTL);
    if map.contains_key(&key) {
        let shown = cmd.split_whitespace().collect::<Vec<_>>().join(" ");
        Some(format!(
            "refused: identical bash command already timed out/hung in this cwd: `{shown}`. \
             Do not retry it. Use list_dir/read_file/grep/glob, change the command, or explain the blocker."
        ))
    } else {
        None
    }
}

fn command_segments(cmd: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = cmd.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'|' | b';' | b'\n' => {
                out.push(&cmd[start..i]);
                start = i + 1;
                i += 1;
            }
            b'&' if i + 1 < bytes.len() && bytes[i + 1] == b'&' => {
                out.push(&cmd[start..i]);
                start = i + 2;
                i += 2;
            }
            _ => i += 1,
        }
    }
    out.push(&cmd[start..]);
    out
}

fn strip_env_assigns(seg: &str) -> &str {
    let mut s = seg.trim();
    while let Some((key, rest)) = s.split_once('=') {
        if key.is_empty() || key.contains(' ') || key.contains('/') || key.contains('\\') {
            break;
        }
        // VAR=value rest…  (value may be quoted lightly; take next token boundary)
        let rest = rest.trim_start();
        if let Some(stripped) = rest.strip_prefix('"') {
            if let Some(end) = stripped.find('"') {
                s = stripped[end + 1..].trim_start();
                continue;
            }
        }
        if let Some(stripped) = rest.strip_prefix('\'') {
            if let Some(end) = stripped.find('\'') {
                s = stripped[end + 1..].trim_start();
                continue;
            }
        }
        let mut parts = rest.splitn(2, char::is_whitespace);
        let _val = parts.next();
        s = parts.next().unwrap_or("").trim_start();
        if s.is_empty() {
            break;
        }
    }
    s
}

/// Commands that almost never finish under a non-interactive agent (stdin null).
/// Matches segment *starts* so `echo "npm run dev"` is allowed.
fn check_hang_prone(cmd: &str) -> Option<String> {
    for seg in command_segments(cmd) {
        let s = strip_env_assigns(seg).to_ascii_lowercase();
        let s = s.trim();
        if s.is_empty() {
            continue;
        }

        let starters: &[(&str, &str)] = &[
            ("cargo watch", "use a one-shot `cargo check` / `cargo test`"),
            (
                "npm run dev",
                "dev servers hang forever - use build/test instead",
            ),
            (
                "npm start",
                "start/dev servers hang forever under the agent",
            ),
            (
                "pnpm dev",
                "dev servers hang forever - use build/test instead",
            ),
            (
                "pnpm start",
                "start/dev servers hang forever under the agent",
            ),
            (
                "yarn dev",
                "dev servers hang forever - use build/test instead",
            ),
            (
                "yarn start",
                "start/dev servers hang forever under the agent",
            ),
            ("next dev", "dev servers hang forever - use `next build`"),
            (
                "npx vite",
                "dev servers hang forever - use a one-shot build",
            ),
            (
                "pnpm vite",
                "dev servers hang forever - use a one-shot build",
            ),
            (
                "yarn vite",
                "dev servers hang forever - use a one-shot build",
            ),
            ("vite ", "dev servers hang forever - use a one-shot build"),
            ("webpack-dev-server", "dev servers hang forever"),
            ("webpack serve", "dev servers hang forever"),
            ("flask run", "dev servers hang forever"),
            ("tail -f", "follow/watch modes hang - use a bounded read"),
            ("get-content -wait", "follow/watch modes hang"),
            ("read -p", "interactive prompts hang (stdin is null)"),
            ("read -n", "interactive prompts hang (stdin is null)"),
            ("cmd /c pause", "interactive pause hangs"),
            ("pause", "interactive pause hangs"),
            ("get-credential", "interactive prompts hang"),
            ("while true", "infinite loops hang - use a bounded command"),
            ("while :", "infinite loops hang - use a bounded command"),
            (
                "python -m http.server",
                "servers hang forever under the agent",
            ),
            (
                "python3 -m http.server",
                "servers hang forever under the agent",
            ),
            ("npx serve", "servers hang forever under the agent"),
            ("live-server", "servers hang forever under the agent"),
        ];
        for (pat, hint) in starters {
            if s == *pat || s.starts_with(pat) {
                return Some(format!(
                    "refused hang-prone command starting with '{pat}' ({hint}). \
                     Prefer dedicated tools or a one-shot non-interactive command."
                ));
            }
        }
        if s == "vite" {
            return Some(
                "refused hang-prone `vite` (dev server). Prefer a one-shot build/test command."
                    .into(),
            );
        }
        // uvicorn … --reload
        if (s.starts_with("uvicorn ") || s.starts_with("uvicorn\t")) && s.contains("--reload") {
            return Some(
                "refused hang-prone uvicorn --reload (dev server). Prefer a one-shot check.".into(),
            );
        }
        if let Some(rest) = s.strip_prefix("npm run ") {
            let script = rest.split_whitespace().next().unwrap_or("");
            if matches!(
                script,
                "dev" | "start" | "watch" | "serve" | "develop" | "preview"
            ) {
                return Some(format!(
                    "refused hang-prone `npm run {script}` - use a one-shot build/test script instead"
                ));
            }
        }
    }
    None
}

fn check_destructive(cmd: &str, cwd: &std::path::Path) -> Option<String> {
    let lower = cmd.to_lowercase();
    let trimmed = lower.trim();
    let _ = cwd;

    if lower.contains("encodedcommand")
        || lower.contains("-enc ")
        || lower.contains("frombase64string")
    {
        return Some(format!(
            "refused: encoded PowerShell obscures intent: {cmd}"
        ));
    }
    if lower.contains(":(){:|:&};:") || lower.contains("fork bomb") {
        return Some("refused: fork bomb detected".into());
    }

    let always_block = [
        "rm -rf /",
        "rm -rf /*",
        "rm -rf ~",
        "rm -rf $home",
        "rm -rf $userprofile",
        "rm -rf %userprofile%",
        "rm -rf ..",
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
        "remove-item -recurse -force $home",
        "remove-item -recurse -force $env:userprofile",
        "format c:",
        "format d:",
        r"\\?\c:",
        "cipher /w:",
        ":(){ :|:& };:",
    ];
    for d in always_block {
        if lower.contains(d) {
            return Some(format!("refused destructive pattern '{d}' in: {cmd}"));
        }
    }

    static RM_ROOT: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static RM_HOME: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static RM_DOTDOT: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let rm_root = RM_ROOT.get_or_init(|| {
        regex::Regex::new(r"(?i)\brm\s+-[a-z]*r[a-z]*f[a-z]*\s+/(?:\s|;|&|$)")
            .expect("rm root pattern")
    });
    let rm_home = RM_HOME.get_or_init(|| {
        regex::Regex::new(r"(?i)\brm\s+-[a-z]*r[a-z]*f[a-z]*.*\s+~(?:\s|;|&|$)")
            .expect("rm home pattern")
    });
    let rm_dotdot = RM_DOTDOT.get_or_init(|| {
        regex::Regex::new(r"(?i)\brm\s+-[a-z]*r[a-z]*f[a-z]*\s+\.\.(?:\s|;|&|$)")
            .expect("rm dotdot pattern")
    });
    if rm_root.is_match(trimmed) {
        return Some(format!("refused rm -rf / detected: {cmd}"));
    }
    if rm_home.is_match(trimmed) {
        return Some(format!("refused rm -rf ~ detected: {cmd}"));
    }
    if rm_dotdot.is_match(trimmed) {
        return Some(format!("refused rm -rf .. detected: {cmd}"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn refuses_dev_servers_but_allows_echo() {
        assert!(check_hang_prone("npm run dev").is_some());
        assert!(check_hang_prone("cargo watch -x check").is_some());
        assert!(check_hang_prone("cargo check").is_none());
        assert!(check_hang_prone("npm test").is_none());
        assert!(
            check_hang_prone(r#"echo "npm run dev""#).is_none(),
            "echo of hang phrase must not refuse"
        );
        assert!(check_hang_prone("FOO=1 npm run dev").is_some());
    }

    #[test]
    fn failure_fingerprint_is_cwd_scoped() {
        let a = PathBuf::from("C:/proj-a");
        let b = PathBuf::from("C:/proj-b");
        let cmd = "unique-nur-bash-fail-test-abc";
        // Clear via TTL retain by inserting then checking different cwd.
        note_failure(&a, cmd);
        assert!(refuse_recent_failure(&a, cmd).is_some());
        assert!(refuse_recent_failure(&b, cmd).is_none());
    }
}

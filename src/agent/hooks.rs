//! Optional local tool hooks (`~/.nur/hooks.toml`).
//!
//! ```toml
//! pre_tool = "echo checking $NUR_TOOL"
//! post_tool = ""
//! timeout_ms = 5000
//! ```
//!
//! Env for commands: NUR_TOOL, NUR_ARGS_JSON, NUR_CWD, NUR_SESSION (legacy
//! META_* aliases are also set). Non-zero pre_tool exit blocks the tool.
//! Missing file = no hooks.

use crate::config::meta_home;
use crate::error::{MuseError, Result};
use serde::Deserialize;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct HooksConfig {
    #[serde(default)]
    pub pre_tool: Option<String>,
    #[serde(default)]
    pub post_tool: Option<String>,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

fn default_timeout() -> u64 {
    5_000
}

impl HooksConfig {
    pub fn load() -> Self {
        let path = meta_home().join("hooks.toml");
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        toml::from_str(&text).unwrap_or_default()
    }

    pub fn is_active(&self) -> bool {
        self.pre_tool
            .as_ref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
            || self
                .post_tool
                .as_ref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
    }

    /// Run pre-tool hook. Ok(()) to proceed; Err blocks the tool.
    pub fn run_pre(&self, tool: &str, args_json: &str, cwd: &Path, session_id: &str) -> Result<()> {
        let Some(cmd) = self
            .pre_tool
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        else {
            return Ok(());
        };
        match run_hook(cmd, tool, args_json, cwd, session_id, self.timeout_ms) {
            Ok(0) => Ok(()),
            Ok(code) => Err(MuseError::Tool(format!(
                "pre_tool hook blocked {tool} (exit {code})"
            ))),
            Err(e) => Err(MuseError::Tool(format!("pre_tool hook failed: {e}"))),
        }
    }

    pub fn run_post(&self, tool: &str, args_json: &str, cwd: &Path, session_id: &str) {
        let Some(cmd) = self
            .post_tool
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        else {
            return;
        };
        let _ = run_hook(cmd, tool, args_json, cwd, session_id, self.timeout_ms);
    }

    pub fn summary(&self) -> String {
        if !self.is_active() {
            return format!(
                "hooks inactive · optional file {}",
                meta_home().join("hooks.toml").display()
            );
        }
        format!(
            "hooks active\n  pre_tool   {}\n  post_tool  {}\n  timeout    {}ms\n  file       {}",
            self.pre_tool.as_deref().unwrap_or("(none)"),
            self.post_tool.as_deref().unwrap_or("(none)"),
            self.timeout_ms,
            meta_home().join("hooks.toml").display()
        )
    }
}

fn run_hook(
    cmd: &str,
    tool: &str,
    args_json: &str,
    cwd: &Path,
    session_id: &str,
    timeout_ms: u64,
) -> std::io::Result<i32> {
    let mut c = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", cmd]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", cmd]);
        c
    };
    c.current_dir(cwd)
        // NUR_* are the current names; META_* kept as aliases for existing hooks.
        .env("NUR_TOOL", tool)
        .env("NUR_ARGS_JSON", args_json)
        .env("NUR_CWD", cwd.display().to_string())
        .env("NUR_SESSION", session_id)
        .env("META_TOOL", tool)
        .env("META_ARGS_JSON", args_json)
        .env("META_CWD", cwd.display().to_string())
        .env("META_SESSION", session_id)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = c.spawn()?;
    // Cooperative-ish timeout: wait with polling.
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms.max(100));
    loop {
        match child.try_wait()? {
            Some(status) => return Ok(status.code().unwrap_or(1)),
            None if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(124); // timeout-like
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inactive_when_empty() {
        assert!(!HooksConfig::default().is_active());
    }
}

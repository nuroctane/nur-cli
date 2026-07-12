//! ADE (Orca, etc.) integration: hooks, terminal titles, usage discovery.

use crate::config::{muse_home, status_path};
use crate::usage::TokenUsage;
use serde_json::json;
use std::fs;
use std::io::Write;
use std::process::Command;

/// Set terminal title so Orca/agent detection can see "meta".
pub fn set_terminal_title(title: &str) {
    // OSC 0 / 2 title
    print!("\x1b]0;{title}\x07");
    let _ = std::io::stdout().flush();
}

/// Write ADE discovery file at ~/.muse/ade.json
pub fn write_ade_manifest(session_id: &str, model: &str, cwd: &str, usage: &TokenUsage) {
    let _ = crate::config::ensure_dirs();
    let body = json!({
        "schema_version": 1,
        "agent": "meta",
        "provider": "meta",
        "product": "Meta CLI (Muse Spark)",
        "model": model,
        "session_id": session_id,
        "cwd": cwd,
        "pid": std::process::id(),
        "status_path": status_path().display().to_string(),
        "usage_log_path": muse_home().join("usage.jsonl").display().to_string(),
        "latest_session_path": muse_home().join("latest_session.json").display().to_string(),
        "home": muse_home().display().to_string(),
        "usage": usage,
        "estimated_cost_usd": usage.estimated_cost_usd(),
        "env_keys": [
            "MUSE_STATUS_PATH",
            "MUSE_USAGE_LOG_PATH",
            "MUSE_SESSION_ID",
            "MUSE_MODEL",
            "MUSE_PROVIDER",
            "MUSE_USAGE_INPUT_TOKENS",
            "MUSE_USAGE_OUTPUT_TOKENS",
            "MUSE_USAGE_TOTAL_TOKENS",
            "MUSE_USAGE_COST_USD",
            "MODEL_API_KEY",
            "MUSE_API_KEY"
        ],
        "note": "Poll status_path for live Meta Model API token usage from the user's key."
    });
    let path = muse_home().join("ade.json");
    let _ = fs::write(path, serde_json::to_string_pretty(&body).unwrap_or_default());
}

/// Best-effort notify Orca agent hook (if running inside Orca terminal).
///
/// Fire-and-forget: the hook shells out to `cmd`/`curl` and waits on it, which
/// would otherwise block the async runtime (up to `--max-time`) on every API
/// response. Runs on a detached thread so the agent loop and TUI never stall.
pub fn notify_orca_hook(payload_json: &str) {
    // Only when Orca injects hook env — cheap check before spawning a thread.
    if std::env::var("ORCA_AGENT_HOOK_PORT")
        .map(|p| p.is_empty())
        .unwrap_or(true)
    {
        return;
    }
    let payload = payload_json.to_string();
    std::thread::spawn(move || notify_orca_hook_blocking(&payload));
}

fn notify_orca_hook_blocking(payload_json: &str) {
    // Only when Orca injects hook env
    let port = match std::env::var("ORCA_AGENT_HOOK_PORT") {
        Ok(p) if !p.is_empty() => p,
        _ => return,
    };
    let token = match std::env::var("ORCA_AGENT_HOOK_TOKEN") {
        Ok(t) if !t.is_empty() => t,
        _ => return,
    };
    let pane = std::env::var("ORCA_PANE_KEY").unwrap_or_default();
    if pane.is_empty() {
        return;
    }

    let muse_home = muse_home().display().to_string();
    let url = format!("http://127.0.0.1:{port}/hook/meta");

    // Prefer packaged hook script if present
    let hook = dirs::home_dir()
        .map(|h| h.join(".orca").join("agent-hooks").join("meta-hook.cmd"))
        .filter(|p| p.exists());

    if let Some(hook) = hook {
        let mut child = Command::new("cmd")
            .args(["/C", &hook.to_string_lossy()])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        if let Ok(ref mut c) = child {
            if let Some(stdin) = c.stdin.as_mut() {
                let _ = stdin.write_all(payload_json.as_bytes());
            }
            let _ = c.wait();
        }
        return;
    }

    // Direct curl fallback (same shape as other Orca hooks)
    let _ = Command::new("curl")
        .args([
            "-sS",
            "-X",
            "POST",
            &url,
            "--connect-timeout",
            "0.5",
            "--max-time",
            "1.5",
            "-H",
            "Content-Type: application/x-www-form-urlencoded",
            "-H",
            &format!("X-Orca-Agent-Hook-Token: {token}"),
            "--data-urlencode",
            &format!("paneKey={pane}"),
            "--data-urlencode",
            &format!(
                "tabId={}",
                std::env::var("ORCA_TAB_ID").unwrap_or_default()
            ),
            "--data-urlencode",
            &format!(
                "launchToken={}",
                std::env::var("ORCA_AGENT_LAUNCH_TOKEN").unwrap_or_default()
            ),
            "--data-urlencode",
            &format!(
                "worktreeId={}",
                std::env::var("ORCA_WORKTREE_ID").unwrap_or_default()
            ),
            "--data-urlencode",
            &format!("museHome={muse_home}"),
            "--data-urlencode",
            &format!("statusPath={}", status_path().display()),
            "--data-urlencode",
            &format!("payload={payload_json}"),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

pub fn install_orca_hook() -> crate::error::Result<()> {
    let dir = dirs::home_dir()
        .ok_or_else(|| crate::error::MuseError::Other("no home dir".into()))?
        .join(".orca")
        .join("agent-hooks");
    fs::create_dir_all(&dir)?;
    let path = dir.join("meta-hook.cmd");
    fs::write(&path, ORCA_HOOK_CMD)?;
    // Compat alias for older docs
    let _ = fs::write(dir.join("muse-hook.cmd"), ORCA_HOOK_CMD);
    if let Some(roaming) = dirs::data_dir() {
        let alt = roaming.join("Orca").join("agent-hooks");
        if alt.exists() || roaming.join("Orca").exists() {
            let _ = fs::create_dir_all(&alt);
            let _ = fs::write(alt.join("meta-hook.cmd"), ORCA_HOOK_CMD);
            let _ = fs::write(alt.join("muse-hook.cmd"), ORCA_HOOK_CMD);
        }
    }
    println!("installed Orca hook: {}", path.display());
    println!("Orca can poll: {}", status_path().display());
    println!("Launch with:  orca terminal create --command meta");
    Ok(())
}

const ORCA_HOOK_CMD: &str = r#"@echo off
setlocal
if defined ORCA_AGENT_HOOK_ENDPOINT if exist "%ORCA_AGENT_HOOK_ENDPOINT%" call "%ORCA_AGENT_HOOK_ENDPOINT%" 2>nul
if "%ORCA_AGENT_HOOK_PORT%"=="" exit /b 0
if "%ORCA_AGENT_HOOK_TOKEN%"=="" exit /b 0
if "%ORCA_PANE_KEY%"=="" exit /b 0
set "ORCA_META_HOME=%USERPROFILE%\.muse"
if not "%MUSE_HOME%"=="" set "ORCA_META_HOME=%MUSE_HOME%"
if not "%META_HOME%"=="" set "ORCA_META_HOME=%META_HOME%"
"%SystemRoot%\System32\curl.exe" -sS -X POST "http://127.0.0.1:%ORCA_AGENT_HOOK_PORT%/hook/meta" ^
  --connect-timeout 0.5 --max-time 1.5 ^
  -H "Content-Type: application/x-www-form-urlencoded" ^
  -H "X-Orca-Agent-Hook-Token: %ORCA_AGENT_HOOK_TOKEN%" ^
  --data-urlencode "paneKey=%ORCA_PANE_KEY%" ^
  --data-urlencode "tabId=%ORCA_TAB_ID%" ^
  --data-urlencode "launchToken=%ORCA_AGENT_LAUNCH_TOKEN%" ^
  --data-urlencode "worktreeId=%ORCA_WORKTREE_ID%" ^
  --data-urlencode "env=%ORCA_AGENT_HOOK_ENV%" ^
  --data-urlencode "version=%ORCA_AGENT_HOOK_VERSION%" ^
  --data-urlencode "metaHome=%ORCA_META_HOME%" ^
  --data-urlencode "statusPath=%ORCA_META_HOME%\status.json" ^
  --data-urlencode "payload@-" >nul 2>nul
exit /b 0
"#;

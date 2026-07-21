//! ADE (Orca, etc.) integration: hooks, terminal titles, usage discovery.

use crate::config::{meta_home, status_path};
use crate::usage::TokenUsage;
use serde_json::json;
use std::fs;
use std::io::Write;
use std::process::Command;

/// Set terminal title (OSC 0/2). Keep the word `nur` in the title so Orca/ADEs
/// can still recognize the agent process.
pub fn set_terminal_title(title: &str) {
    print!("\x1b]0;{title}\x07");
    let _ = std::io::stdout().flush();
}

/// Copy text to the system clipboard (best-effort; returns false if the
/// platform has no accessible clipboard, e.g. a bare SSH session).
pub fn copy_to_clipboard(text: &str) -> bool {
    match arboard::Clipboard::new() {
        Ok(mut cb) => cb.set_text(text.to_string()).is_ok(),
        Err(_) => false,
    }
}

/// Preferred session tab title: moon · nur · abbreviated first prompt.
///
/// Example: `🌕 nur · fix the login hang…`
pub fn session_window_title(prompt: &str) -> String {
    title_with_marker(crate::theme::TITLE_IDLE, prompt)
}

/// Animated window title while inference is running — the marker orb rotates
/// through moon phases so the tab visibly "works". Drive with the TUI's spinner
/// clock and throttle updates (~110ms) so we don't spam OSC every frame.
pub fn running_window_title(elapsed: std::time::Duration, prompt: &str) -> String {
    let marker = crate::theme::frame_at(crate::theme::TITLE_FRAMES, elapsed, 110);
    title_with_marker(marker, prompt)
}

fn title_with_marker(marker: &str, prompt: &str) -> String {
    let abbr = abbreviate_for_title(prompt, 48);
    if abbr.is_empty() || abbr == "ready" {
        format!("{marker} nur")
    } else {
        format!("{marker} nur · {abbr}")
    }
}

/// Collapse whitespace and truncate for a compact window/tab label.
pub fn abbreviate_for_title(prompt: &str, max_chars: usize) -> String {
    let collapsed: String = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    let collapsed = collapsed.trim();
    if collapsed.is_empty() {
        return String::new();
    }
    if collapsed.chars().count() <= max_chars {
        return collapsed.to_string();
    }
    let take = max_chars.saturating_sub(1).max(8);
    let mut s: String = collapsed.chars().take(take).collect();
    // Prefer breaking on a word boundary when close to the end.
    if let Some(i) = s.rfind(' ') {
        if i > take / 2 {
            s.truncate(i);
        }
    }
    s.push('…');
    s
}

/// Write ADE discovery file at ~/.nur/ade.json
///
/// `state` is the live agent state (`idle`, `thinking (turn N)`, `tool:<name>`,
/// …). Poll-based ADEs that read `ade.json` (rather than the push hook) rely on
/// this field + `updated_at` to know when a turn has finished.
pub fn write_ade_manifest(
    session_id: &str,
    model: &str,
    cwd: &str,
    usage: &TokenUsage,
    state: &str,
) {
    let _ = crate::config::ensure_dirs();
    let body = json!({
        "schema_version": 1,
        "agent": "nur",
        "provider": "meta",
        "product": "NurCLI",
        "model": model,
        "model_label": crate::config::model_display_name(model),
        "session_id": session_id,
        "cwd": cwd,
        "pid": std::process::id(),
        "state": state,
        "busy": state != "idle",
        "updated_at": chrono::Utc::now().to_rfc3339(),
        "status_path": status_path().display().to_string(),
        "usage_log_path": meta_home().join("usage.jsonl").display().to_string(),
        "latest_session_path": meta_home().join("latest_session.json").display().to_string(),
        "home": meta_home().display().to_string(),
        "usage": usage,
        "estimated_cost_usd": usage.estimated_cost_usd(),
        "env_keys": [
            "NUR_STATUS_PATH",
            "NUR_USAGE_LOG_PATH",
            "NUR_SESSION_ID",
            "NUR_MODEL",
            "NUR_PROVIDER",
            "NUR_USAGE_INPUT_TOKENS",
            "NUR_USAGE_OUTPUT_TOKENS",
            "NUR_USAGE_TOTAL_TOKENS",
            "NUR_USAGE_COST_USD",
            "NUR_HOME",
            "NUR_API_KEY",
            // Legacy aliases (still dual-exported for older host panels / Orca hooks)
            "META_STATUS_PATH",
            "META_USAGE_LOG_PATH",
            "META_SESSION_ID",
            "META_MODEL",
            "META_PROVIDER",
            "META_USAGE_INPUT_TOKENS",
            "META_USAGE_OUTPUT_TOKENS",
            "META_USAGE_TOTAL_TOKENS",
            "META_USAGE_COST_USD",
            "META_HOME",
            "META_API_KEY",
            "MODEL_API_KEY",
            "MUSE_API_KEY"
        ],
        "note": "Poll status_path (or this file's `state`/`updated_at`) for live agent state + token usage. state=='idle' means the turn finished. Prefer NUR_* env keys; META_*/MUSE_* are legacy aliases."
    });
    let path = meta_home().join("ade.json");
    let _ = fs::write(
        path,
        serde_json::to_string_pretty(&body).unwrap_or_default(),
    );
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

/// Push an agent **state transition** to Orca (working → tool → idle).
///
/// The usage hook (`meta.usage`) only fires when tokens are billed *mid*-turn,
/// so a host that relied on it never saw the turn *finish*. This fires on every
/// state change — critically the `idle` transition at turn end — so Orca learns
/// a query completed instead of just going silent. No-ops outside Orca.
pub fn notify_orca_state(session_id: &str, model: &str, provider: &str, turn: u32, state: &str) {
    if std::env::var("ORCA_AGENT_HOOK_PORT")
        .map(|p| p.is_empty())
        .unwrap_or(true)
    {
        return;
    }
    let payload = json!({
        "type": "meta.state",
        "session_id": session_id,
        "model": model,
        "provider": provider,
        "turn": turn,
        "state": state,
        "busy": state != "idle",
        "status_path": status_path().display().to_string(),
    });
    notify_orca_hook(&payload.to_string());
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

    let meta_home_s = meta_home().display().to_string();
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
            &format!("tabId={}", std::env::var("ORCA_TAB_ID").unwrap_or_default()),
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
            &format!("metaHome={meta_home_s}"),
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
    // Primary name is still meta-hook.cmd (Orca host panels look for it).
    // nur-hook.cmd / muse-hook.cmd are aliases for newer / older docs.
    let path = dir.join("meta-hook.cmd");
    fs::write(&path, ORCA_HOOK_CMD)?;
    let _ = fs::write(dir.join("nur-hook.cmd"), ORCA_HOOK_CMD);
    let _ = fs::write(dir.join("muse-hook.cmd"), ORCA_HOOK_CMD);
    if let Some(roaming) = dirs::data_dir() {
        let alt = roaming.join("Orca").join("agent-hooks");
        if alt.exists() || roaming.join("Orca").exists() {
            let _ = fs::create_dir_all(&alt);
            let _ = fs::write(alt.join("meta-hook.cmd"), ORCA_HOOK_CMD);
            let _ = fs::write(alt.join("nur-hook.cmd"), ORCA_HOOK_CMD);
            let _ = fs::write(alt.join("muse-hook.cmd"), ORCA_HOOK_CMD);
        }
    }
    println!("installed Orca hook: {}", path.display());
    println!("Orca can poll: {}", status_path().display());
    println!("Launch with:  orca terminal create --command nur");
    Ok(())
}

const ORCA_HOOK_CMD: &str = r#"@echo off
setlocal
if defined ORCA_AGENT_HOOK_ENDPOINT if exist "%ORCA_AGENT_HOOK_ENDPOINT%" call "%ORCA_AGENT_HOOK_ENDPOINT%" 2>nul
if "%ORCA_AGENT_HOOK_PORT%"=="" exit /b 0
if "%ORCA_AGENT_HOOK_TOKEN%"=="" exit /b 0
if "%ORCA_PANE_KEY%"=="" exit /b 0
set "ORCA_META_HOME=%USERPROFILE%\.nur"
REM Match Rust nur_home(): NUR_HOME, then META_HOME, then MUSE_HOME.
if not "%MUSE_HOME%"=="" set "ORCA_META_HOME=%MUSE_HOME%"
if not "%META_HOME%"=="" set "ORCA_META_HOME=%META_HOME%"
if not "%NUR_HOME%"=="" set "ORCA_META_HOME=%NUR_HOME%"
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

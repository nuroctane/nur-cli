//! ADE (Orca, etc.) integration: hooks, terminal titles, usage discovery.

use crate::config::{nur_home, status_path};
use crate::usage::TokenUsage;
use serde_json::json;
use std::fs;
use std::io::Write;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Headless runs never paint a title: their stdout is an answer or a log, and
/// an OSC sequence carrying the prompt's first words in front of every status
/// line is noise in both.
static TITLE_DISABLED: AtomicBool = AtomicBool::new(false);

pub fn disable_terminal_title() {
    TITLE_DISABLED.store(true, Ordering::Relaxed);
}

/// Set the terminal title (OSC 0). Written only when stdout is a terminal; a
/// redirected stdout gets no escape bytes.
///
/// Mutex-guarded: the TUI thread (110ms animation) and the agent thread
/// (post-tool re-asserts) both write here concurrently, and two interleaved OSC
/// sequences can corrupt the title mid-frame.
fn set_terminal_title(title: &str) {
    use std::io::IsTerminal;
    if TITLE_DISABLED.load(Ordering::Relaxed) || !std::io::stdout().is_terminal() {
        return;
    }
    static WRITE_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    let _guard = WRITE_LOCK
        .get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
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

/// The window title is the moon and nothing else: the full moon while idle,
/// the phases cycling (~110ms a frame) while a turn runs. No product name,
/// provider or prompt text rides along.
fn window_title(busy_for: Option<Duration>) -> &'static str {
    match busy_for {
        Some(elapsed) => crate::theme::frame_at(crate::theme::TITLE_FRAMES, elapsed, 110),
        None => crate::theme::TITLE_IDLE,
    }
}

static TITLE_BUSY: AtomicBool = AtomicBool::new(false);
static TITLE_BUSY_SINCE: std::sync::RwLock<Option<std::time::Instant>> =
    std::sync::RwLock::new(None);

/// Mirror the turn state (busy = inference running) and repaint. `true` also
/// stamps when the busy period began so any re-assert mid-turn renders the
/// running phases rather than flipping the tab back to the idle moon.
pub fn set_title_busy(busy: bool) {
    TITLE_BUSY.store(busy, Ordering::Relaxed);
    if busy {
        if let Ok(mut w) = TITLE_BUSY_SINCE.write() {
            *w = Some(std::time::Instant::now());
        }
    }
    render_title();
}

/// Repaint the title from shared state, so every writer (TUI animation loop,
/// agent-loop re-asserts after tool children, startup) draws the same moon.
pub fn render_title() {
    let busy_for = TITLE_BUSY.load(Ordering::Relaxed).then(|| {
        TITLE_BUSY_SINCE
            .read()
            .ok()
            .and_then(|g| *g)
            .map(|since| since.elapsed())
            .unwrap_or_default()
    });
    set_terminal_title(window_title(busy_for));
}

/// Animation tick for the TUI's 110ms loop: renders the running moon from the
/// TUI's own elapsed clock WITHOUT mutating state, so the animation clock
/// stays owned by the TUI.
pub fn tick_running_title(elapsed: Duration) {
    set_terminal_title(window_title(Some(elapsed)));
}

/// Child processes (omp, egaki, graphjin serve, …) rewrite the terminal title
/// with their own branding while they run (omp sets it to "π"). Call this
/// after any child completes, and periodically during long ones, to put the
/// moon back. Renders from the shared state, so a busy turn re-asserts the
/// running phases instead of fighting the TUI animation with the idle moon.
pub fn reassert_after_child() {
    render_title();
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
        "usage_log_path": nur_home().join("usage.jsonl").display().to_string(),
        "latest_session_path": nur_home().join("latest_session.json").display().to_string(),
        "home": nur_home().display().to_string(),
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
            "NUR_API_KEY"
        ],
        "note": "Poll status_path (or this file's `state`/`updated_at`) for live agent state + token usage. state=='idle' means the turn finished."
    });
    let path = nur_home().join("ade.json");
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

    let nur_home_s = nur_home().display().to_string();
    let url = format!("http://127.0.0.1:{port}/hook/meta");

    // Prefer packaged hook script if present
    let hook = crate::config::home_dir()
        .map(|h| h.join(".orca").join("agent-hooks").join("nur-hook.cmd"))
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
            &format!("metaHome={nur_home_s}"),
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
    let dir = crate::config::home_dir()
        .ok_or_else(|| crate::error::NurError::Other("no home dir".into()))?
        .join(".orca")
        .join("agent-hooks");
    fs::create_dir_all(&dir)?;
    let path = dir.join("nur-hook.cmd");
    fs::write(&path, ORCA_HOOK_CMD)?;
    if let Some(roaming) = dirs::data_dir() {
        let alt = roaming.join("Orca").join("agent-hooks");
        if alt.exists() || roaming.join("Orca").exists() {
            let _ = fs::create_dir_all(&alt);
            let _ = fs::write(alt.join("nur-hook.cmd"), ORCA_HOOK_CMD);
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
set "ORCA_NUR_HOME=%USERPROFILE%\.nur"
REM Match Rust nur_home().
if not "%NUR_HOME%"=="" set "ORCA_NUR_HOME=%NUR_HOME%"
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
  --data-urlencode "metaHome=%ORCA_NUR_HOME%" ^
  --data-urlencode "statusPath=%ORCA_NUR_HOME%\status.json" ^
  --data-urlencode "payload@-" >nul 2>nul
exit /b 0
"#;

#[cfg(test)]
mod tests {
    use super::*;

    // Failure modes: anything besides the moon leaks into the title (product
    // name, provider, prompt text), or idle and running render the same.
    #[test]
    fn the_title_is_only_the_moon() {
        assert_eq!(window_title(None), crate::theme::TITLE_IDLE);
        for ms in [0, 110, 220, 770, 5_000] {
            let running = window_title(Some(Duration::from_millis(ms)));
            assert!(crate::theme::TITLE_FRAMES.contains(&running), "{running}");
        }
        assert_ne!(
            window_title(Some(Duration::from_millis(0))),
            window_title(Some(Duration::from_millis(110)))
        );
    }
}

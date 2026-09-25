//! Per-provider browser / device-code / external-CLI login flows.

use super::{expires_in_to_at, open_browser, CancelFlag};
use crate::auth::{Auth, OauthMeta};
use crate::error::{NurError, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::{BufRead, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;
use uuid::Uuid;

/// Tokens returned by a successful browser login.
#[derive(Debug, Clone)]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<u64>,
    pub meta: Option<OauthMeta>,
}

/// Progress events for the TUI browser stage (Hugging Face–style URL + code).
#[derive(Debug, Clone)]
pub enum BrowserLoginProgress {
    Status(String),
    /// Device-code style: open this URL and enter the short code.
    DeviceCode {
        verification_url: String,
        user_code: String,
    },
    /// Loopback / SSO: browser opened (or open this URL).
    OpenUrl(String),
    Done(OAuthTokens),
    Failed(String),
}

pub type ProgressTx = Sender<BrowserLoginProgress>;

fn send(tx: &ProgressTx, ev: BrowserLoginProgress) {
    let _ = tx.send(ev);
}

fn oauth_error_summary(body: &str) -> String {
    let value = serde_json::from_str::<serde_json::Value>(body).ok();
    let detail = value.as_ref().and_then(|value| {
        ["/error_description", "/error/message", "/message", "/error"]
            .iter()
            .find_map(|pointer| value.pointer(pointer).and_then(|value| value.as_str()))
    });
    detail
        .map(|detail| detail.chars().take(240).collect())
        .unwrap_or_else(|| "response details withheld".into())
}

/// Run browser login for `provider_id` on a background-friendly thread path.
/// Blocks until success, failure, cancel, or timeout.
pub fn login_browser(provider_id: &str, tx: ProgressTx, cancel: CancelFlag) {
    let result = match provider_id {
        "openai" => openai::login(&tx, &cancel),
        "xai" => xai::login(&tx, &cancel),
        "kimi" => kimi::login(&tx, &cancel),
        "anthropic" => claude::login(&tx, &cancel),
        "google" => google::login(&tx, &cancel),
        "antigravity" => antigravity::login(&tx, &cancel),
        "azure" => azure::login(&tx, &cancel),
        "github-models" | "github-copilot" => github::login(provider_id, &tx, &cancel),
        "cursor" => cursor::login(&tx, &cancel),
        "opencode" => opencode::login(&tx, &cancel),
        "nous" => nous::login(&tx, &cancel),
        "commandcode" => commandcode::login(&tx, &cancel),
        "cline" => cline::login(&tx, &cancel),
        "meta" => super::harness::muse::login(&tx, &cancel),
        "deepseek" => super::harness::deepseek::login(&tx, &cancel),
        "zhipu" => super::harness::zhipu::login(&tx, &cancel),
        other => Err(NurError::Other(format!(
            "browser login not supported for '{other}'"
        ))),
    };
    // Do not persist here — the TUI decides active login vs failover-only
    // storage so a `/failover` browser capture never overwrites auth.json.
    match result {
        Ok(tokens) => send(&tx, BrowserLoginProgress::Done(tokens)),
        Err(e) => send(&tx, BrowserLoginProgress::Failed(e.to_string())),
    }
}

/// Stream a child CLI's stdout/stderr line-by-line: open the first https URL
/// promptly (so the browser round trip starts immediately) and forward other
/// lines as progress. Shared by the vendor-CLI logins (cursor / opencode /
/// cline).
fn watch_login_output(reader: impl Read + Send + 'static, tx: ProgressTx) {
    thread::spawn(move || {
        stream_login_output(reader, &tx, |url| {
            let _ = open_browser(url);
        })
    });
}

// Drain both pipes until EOF, including after the URL. Closing a pipe on the
// first URL can break the vendor process; waiting for EOF hides its login code.
fn stream_login_output(reader: impl Read, tx: &ProgressTx, mut open: impl FnMut(&str)) {
    let mut opened = false;
    for line in std::io::BufReader::new(reader)
        .lines()
        .map_while(std::result::Result::ok)
    {
        let line = crate::tui::ansi::strip(&line);
        let snippet: String = line.chars().take(240).collect();
        if !snippet.trim().is_empty() {
            send(tx, BrowserLoginProgress::Status(snippet));
        }
        if line.contains("microsoft.com/devicelogin") {
            send_az_device_code(tx, &line);
        }
        if let Some(code) = line
            .split_once("one-time code:")
            .and_then(|(_, tail)| tail.split_whitespace().next())
        {
            send(
                tx,
                BrowserLoginProgress::DeviceCode {
                    verification_url: "https://github.com/login/device".into(),
                    user_code: code.to_string(),
                },
            );
        }
        if !opened {
            for word in line.split_whitespace() {
                let url = word.trim_matches(|c: char| matches!(c, ')' | '(' | '"' | '\'' | '`'));
                if url.starts_with("https://") {
                    send(tx, BrowserLoginProgress::OpenUrl(url.to_string()));
                    open(url);
                    opened = true;
                    break;
                }
            }
        }
    }
}

/// Import tokens from a first-party CLI session / OMP when present.
///
/// Used by `auth::resolve_api_key_for` and failover **only after** nur-saved
/// credentials (auth.json, per-provider keys/sessions, env) have already been
/// tried and found empty. Order inside this import path:
/// 1. Vendor CLI (Claude, Codex, Cursor, OpenCode, Grok, Kimi, Antigravity, …)
/// 2. **OMP** (`omp token <provider>`) for every catalog id - universal last
///    resort so a login in Oh My Pi covers nur without re-pasting keys.
pub fn import_existing_session(provider_id: &str) -> Result<Option<OAuthTokens>> {
    let specific = match provider_id {
        "openai" => openai::import_codex_cli(),
        "xai" => xai::import_grok_cli(),
        "kimi" => kimi::import_kimi_cli(),
        "anthropic" => claude::import_claude_cli(),
        "huggingface" => Ok(huggingface::import_hf_token()),
        "cursor" => cursor::import_cursor_cli(),
        "opencode" => opencode::import_opencode_cli(),
        "nous" => nous::import_hermes_cli(),
        "commandcode" => commandcode::import_commandcode_cli(),
        "cline" => cline::import_cline_cli(),
        "meta" => super::harness::muse::import_muse_cli(),
        "deepseek" => super::harness::deepseek::import_dsh_cli(),
        "zhipu" => super::harness::zhipu::import_zcode_cli(),
        "qwen" => super::harness::qwen::import_qwen_cli(),
        "minimax" => super::harness::minimax::import_mmx_cli(),
        // Google / Antigravity: try Antigravity CLI first (Windows credman + file),
        // then gcloud ADC.
        // antigravity::import_existing already ends with a google ADC probe,
        // so one call covers the whole family - the old second
        // google::import_existing here spawned `gcloud` twice per resolution.
        "google" | "antigravity" | "google-oauth" => match antigravity::import_existing() {
            Ok(Some(t)) => Ok(Some(t)),
            Ok(None) => Ok(None),
            Err(_) => Ok(None),
        },
        _ => Ok(None),
    };
    match specific {
        Ok(Some(tokens)) => Ok(Some(tokens)),
        Ok(None) | Err(_) => {
            // Universal OMP bridge — never fail the import chain on omp errors.
            match super::omp_bridge::import_omp_token(provider_id) {
                Ok(Some(tokens)) => Ok(Some(tokens)),
                _ => Ok(None),
            }
        }
    }
}

pub mod cursor {
    use super::*;
    use std::path::PathBuf;

    /// Prefer `cursor-agent` only. Bare `agent` on PATH is often Grok's binary.
    /// On Windows prefer `.cmd` / `.exe` over `.ps1` (CreateProcess cannot run
    /// PowerShell scripts directly).
    fn cursor_agent_bin() -> Option<PathBuf> {
        let path = std::env::var_os("PATH")?;
        for dir in std::env::split_paths(&path) {
            #[cfg(windows)]
            {
                for name in ["cursor-agent.cmd", "cursor-agent.exe", "cursor-agent.ps1"] {
                    let p = dir.join(name);
                    if p.is_file() {
                        return Some(p);
                    }
                }
            }
            let c = dir.join("cursor-agent");
            if c.is_file() {
                return Some(c);
            }
        }
        None
    }

    /// Windows-safe spawn: `.cmd`/`.bat` via `cmd /D /C`, `.ps1` via powershell -File.
    fn spawn_cursor_agent(bin: &PathBuf, args: &[&str]) -> std::io::Result<std::process::Child> {
        #[cfg(windows)]
        {
            let lower = bin.to_string_lossy().to_ascii_lowercase();
            let mut cmd = if lower.ends_with(".cmd") || lower.ends_with(".bat") {
                let mut c = Command::new("cmd.exe");
                c.arg("/D").arg("/C").arg(bin);
                for a in args {
                    c.arg(a);
                }
                c
            } else if lower.ends_with(".ps1") {
                let mut c = Command::new("powershell.exe");
                c.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                    .arg(bin);
                for a in args {
                    c.arg(a);
                }
                c
            } else {
                let mut c = Command::new(bin);
                c.args(args);
                c
            };
            cmd.stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        }
        #[cfg(not(windows))]
        {
            Command::new(bin)
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        }
    }

    fn looks_like_cursor_secret(s: &str) -> bool {
        let t = s.trim();
        if t.len() < 20 {
            return false;
        }
        // Documented Agent API keys often use a crsr_ prefix; also accept
        // long opaque bearer-looking strings from auth.json accessToken fields.
        t.starts_with("crsr_")
            || t.starts_with("key_")
            || (t.len() >= 32 && !t.contains(' ') && !t.contains('\n'))
    }

    fn token_from_json(v: &serde_json::Value) -> Option<String> {
        // Prefer explicit API-key / access-token fields; avoid bare `token`
        // which often holds unrelated IDs in Cursor config blobs.
        const KEYS: &[&str] = &["api_key", "apiKey", "access_token", "accessToken"];
        for k in KEYS {
            if let Some(s) = v.get(*k).and_then(|x| x.as_str()) {
                if looks_like_cursor_secret(s) {
                    return Some(s.trim().to_string());
                }
            }
        }
        for nest in ["auth", "credentials"] {
            if let Some(inner) = v.get(nest) {
                if let Some(t) = token_from_json(inner) {
                    return Some(t);
                }
            }
        }
        None
    }

    fn tokens_from_key(access: String, via: &str, path: Option<&str>) -> Option<OAuthTokens> {
        // A stored JWT already past `exp` is a signed-out session, not a
        // credential — returning it made login "succeed" with a dead key.
        if expired_jwt(&access) {
            return None;
        }
        Some(OAuthTokens {
            access_token: access,
            // Marker so refresh re-imports from the CLI / env.
            refresh_token: Some("cursor-agent".into()),
            expires_at: None,
            meta: Some(OauthMeta {
                issuer: "cursor".into(),
                client_id: "cursor-agent".into(),
                extra: serde_json::json!({
                    "imported_from": via,
                    "path": path.unwrap_or(""),
                    // Cursor's credential is a plain API key in every shape
                    // this module imports; it must not take OAuth-only routes.
                    "credential_kind": "api_key",
                }),
            }),
        })
    }

    /// Import credentials from `CURSOR_API_KEY`, Cursor Agent session files, or
    /// a live `cursor-agent status` login (OS keychain - no pasted key).
    ///
    /// Does **not** treat `mcp.json` / `cli-config.json` as auth (those are
    /// MCP servers and UI settings).
    pub fn import_cursor_cli() -> Result<Option<OAuthTokens>> {
        if let Ok(key) = std::env::var("CURSOR_API_KEY") {
            let key = key.trim().to_string();
            if !key.is_empty() {
                return Ok(tokens_from_key(key, "CURSOR_API_KEY", None));
            }
        }

        let mut dirs = Vec::new();
        if let Ok(dir) = std::env::var("CURSOR_AGENT_HOME") {
            dirs.push(PathBuf::from(dir));
        }
        // No CWD-relative fallback: without a home dir, a repo-planted
        // .cursor/auth.json must not be trusted as a credential store.
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(".cursor"));
            dirs.push(home.join(".config").join("cursor"));
        }
        // Cursor Agent `getAuthFilePath`: Windows → `%APPDATA%\<TitleCase>\auth.json`
        // (e.g. Cursor / Cursor-agent). macOS → `~/.cursor/auth.json` (already above).
        if let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) {
            dirs.push(appdata.join("Cursor"));
            dirs.push(appdata.join("Cursor-agent"));
            dirs.push(appdata.join("Cursor Agent"));
        }

        for dir in dirs {
            if !dir.exists() {
                continue;
            }
            // auth.json (legacy / post-login) first; skip mcp.json and
            // cli-config.json (not credential stores).
            for file in ["auth.json", "cursor-auth.json"] {
                let p = dir.join(file);
                if !p.is_file() {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&p) else {
                    continue;
                };
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
                    continue;
                };
                if let Some(key) = token_from_json(&v) {
                    return Ok(tokens_from_key(
                        key,
                        "cursor-cli",
                        Some(&p.display().to_string()),
                    ));
                }
            }
        }

        // Keychain / platform login: `cursor-agent status` is enough (t3code).
        if let Some(tokens) = crate::api::cursor_cli::session_tokens_from_cli() {
            return Ok(Some(tokens));
        }
        Ok(None)
    }

    /// Sign in through the official Cursor Agent CLI (`cursor-agent login`).
    pub fn login(tx: &ProgressTx, cancel: &CancelFlag) -> Result<OAuthTokens> {
        if let Ok(Some(t)) = import_cursor_cli() {
            send(
                tx,
                BrowserLoginProgress::Status("using existing Cursor Agent session".into()),
            );
            return Ok(t);
        }

        let bin = cursor_agent_bin().ok_or_else(|| {
            NurError::Other(
                "cursor-agent not found on PATH. Install Cursor Agent (https://cursor.com/docs/cli), open a new terminal, then retry - or paste a CURSOR_API_KEY via /login.".into(),
            )
        })?;

        send(
            tx,
            BrowserLoginProgress::Status(
                "launching Cursor browser login (cursor-agent login)…".into(),
            ),
        );
        // Do not hardcode cursor.com/login - wait for the CLI's device/auth URL.

        let mut child = spawn_cursor_agent(&bin, &["login"]).map_err(|e| {
            NurError::Other(format!(
                "failed to launch cursor-agent ({e}). Install Cursor Agent, or paste CURSOR_API_KEY."
            ))
        })?;

        if let Some(err) = child.stderr.take() {
            watch_login_output(err, tx.clone());
        }
        if let Some(out) = child.stdout.take() {
            watch_login_output(out, tx.clone());
        }

        const CURSOR_LOGIN_TIMEOUT: Duration = Duration::from_secs(600);
        let started = std::time::Instant::now();
        loop {
            if cancel.is_cancelled() {
                let _ = child.kill();
                return Err(NurError::Other("login cancelled".into()));
            }
            if started.elapsed() > CURSOR_LOGIN_TIMEOUT {
                let _ = child.kill();
                return Err(NurError::Other(
                    "cursor-agent login did not finish within 10 minutes. Run \
                     `cursor-agent login` in a terminal, then retry /login (nur imports the \
                     session) — or paste CURSOR_API_KEY."
                        .into(),
                ));
            }
            match child.try_wait() {
                Ok(Some(status)) if status.success() => break,
                Ok(Some(status)) => {
                    return Err(NurError::Other(format!(
                        "cursor-agent login failed (exit {status}). Paste CURSOR_API_KEY as fallback."
                    )));
                }
                Ok(None) => thread::sleep(Duration::from_millis(200)),
                Err(e) => return Err(NurError::Other(e.to_string())),
            }
        }

        send(
            tx,
            BrowserLoginProgress::Status("importing Cursor Agent session…".into()),
        );
        match import_cursor_cli()? {
            Some(t) => Ok(t),
            None => Err(NurError::Other(
                "cursor-agent login finished, but nur still sees you as logged out \
                 (`cursor-agent status`). Run `cursor-agent login` again in a terminal, \
                 or set CURSOR_API_KEY as a fallback."
                    .into(),
            )),
        }
    }

    pub fn refresh(_auth: &Auth, _refresh: &str) -> Result<OAuthTokens> {
        import_cursor_cli()?.ok_or_else(|| {
            NurError::Other(
                "Cursor session expired or missing. Run `cursor-agent login`, or set CURSOR_API_KEY."
                    .into(),
            )
        })
    }
    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn tokens_from_key_marks_api_key_and_skips_expired_jwt() {
            let t = tokens_from_key("cursor-plain-key-123".into(), "CURSOR_API_KEY", None)
                .expect("plain key imports");
            let extra = t.meta.expect("meta").extra;
            assert_eq!(extra["credential_kind"], "api_key");
            assert_eq!(extra["imported_from"], "CURSOR_API_KEY");
            assert_eq!(t.refresh_token.as_deref(), Some("cursor-agent"));

            // A JWT whose exp is already past is a signed-out session, not a
            // credential - the import used to hand it back as success.
            let dead = format!("x.{}.y", URL_SAFE_NO_PAD.encode(br#"{"exp":1}"#));
            assert!(tokens_from_key(dead, "cursor-cli", None).is_none());
        }

        #[test]
        fn secret_shape_gate() {
            // crsr_/key_ prefixes, or a long opaque bearer-shaped string.
            assert!(looks_like_cursor_secret("crsr_abc123def456ghi789"));
            assert!(looks_like_cursor_secret("key_abc123def456ghi789"));
            assert!(looks_like_cursor_secret(
                "a-long-opaque-access-token-value-without-spaces-1234567890"
            ));
            assert!(!looks_like_cursor_secret("crsr_short"));
            assert!(!looks_like_cursor_secret("no"));
            assert!(!looks_like_cursor_secret(""));
        }

        #[test]
        fn token_from_json_prefers_key_fields() {
            let v = serde_json::json!({"access_token": "crsr_access_token_value_1"});
            assert_eq!(
                token_from_json(&v).as_deref(),
                Some("crsr_access_token_value_1")
            );
            let nested = serde_json::json!({"auth": {"api_key": "crsr_nested_key_value_1"}});
            assert_eq!(
                token_from_json(&nested).as_deref(),
                Some("crsr_nested_key_value_1")
            );
            // Bare `token` is deliberately ignored (often an unrelated id).
            let bare = serde_json::json!({"token": "crsr_bare_token_value_1"});
            assert_eq!(token_from_json(&bare), None);
        }
    }
}

pub mod opencode {
    use super::*;
    use std::io::BufRead;
    use std::path::PathBuf;

    /// OpenCode stores credentials at `$XDG_DATA_HOME/opencode/auth.json`
    /// (default `~/.local/share/opencode/auth.json`) as a map of
    /// `providerId -> { type: api|oauth|wellknown, key|access|… }`.
    /// Prefer `opencode` / `opencode-go` for nur's OpenCode gateway provider.
    fn auth_json_paths() -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Ok(dir) = std::env::var("OPENCODE_HOME") {
            out.push(PathBuf::from(dir).join("auth.json"));
        }
        if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
            out.push(PathBuf::from(xdg).join("opencode").join("auth.json"));
        }
        // No CWD-relative fallback: without a home dir, a repo-planted
        // auth.json must not be trusted as a credential store.
        if let Some(home) = dirs::home_dir() {
            out.push(
                home.join(".local")
                    .join("share")
                    .join("opencode")
                    .join("auth.json"),
            );
            // Legacy / config-dir guesses (older installs).
            out.push(home.join(".config").join("opencode").join("auth.json"));
            out.push(home.join(".opencode").join("auth.json"));
        }
        out
    }

    fn token_from_entry(entry: &serde_json::Value) -> Option<(String, Option<String>)> {
        let ty = entry.get("type").and_then(|t| t.as_str()).unwrap_or("api");
        match ty {
            "api" | "wellknown" => {
                let key = entry
                    .get("key")
                    .or_else(|| entry.get("token"))
                    .and_then(|x| x.as_str())?
                    .trim();
                if key.is_empty() {
                    return None;
                }
                Some((key.to_string(), None))
            }
            "oauth" => {
                let access = entry
                    .get("access")
                    .or_else(|| entry.get("access_token"))
                    .and_then(|x| x.as_str())?
                    .trim();
                if access.is_empty() {
                    return None;
                }
                let refresh = entry
                    .get("refresh")
                    .or_else(|| entry.get("refresh_token"))
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                Some((access.to_string(), refresh))
            }
            _ => None,
        }
    }

    pub fn import_opencode_cli() -> Result<Option<OAuthTokens>> {
        if let Ok(key) = std::env::var("OPENCODE_API_KEY") {
            let key = key.trim().to_string();
            // An expired JWT is a signed-out session, not a credential.
            if !key.is_empty() && !expired_jwt(&key) {
                return Ok(Some(OAuthTokens {
                    access_token: key,
                    refresh_token: Some("opencode".into()),
                    expires_at: None,
                    meta: Some(OauthMeta {
                        issuer: "opencode".into(),
                        client_id: "opencode-cli".into(),
                        extra: serde_json::json!({
                            "imported_from": "OPENCODE_API_KEY",
                            "credential_kind": "api_key",
                        }),
                    }),
                }));
            }
        }

        for p in auth_json_paths() {
            if !p.is_file() {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&p) else {
                continue;
            };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            // Modern shape: { "opencode": { "type":"api", "key":"…" }, … }
            if let Some(map) = v.as_object() {
                for id in ["opencode", "opencode-go"] {
                    if let Some(entry) = map.get(id) {
                        if let Some((access, refresh)) = token_from_entry(entry) {
                            // Expired stored tokens are dead: `refresh` has no
                            // upstream grant to exercise, so skip to the next
                            // path instead of importing a guaranteed 401.
                            if expired_jwt(&access) {
                                continue;
                            }
                            let entry_type =
                                entry.get("type").and_then(|x| x.as_str()).unwrap_or("api");
                            let extra = serde_json::json!({
                                "imported_from": "opencode-cli",
                                "path": p.display().to_string(),
                                "provider_entry": id,
                                // `api` / `wellknown` entries hold plain keys;
                                // only `oauth` entries may take OAuth routes.
                                "credential_kind": if entry_type == "oauth" {
                                    "oauth"
                                } else {
                                    "api_key"
                                },
                            });
                            return Ok(Some(OAuthTokens {
                                access_token: access,
                                refresh_token: refresh.or_else(|| Some("opencode".into())),
                                expires_at: None,
                                meta: Some(OauthMeta {
                                    issuer: "opencode".into(),
                                    client_id: "opencode-cli".into(),
                                    extra,
                                }),
                            }));
                        }
                    }
                }
            }
            // Legacy flat key
            if let Some(key) = v
                .get("api_key")
                .or_else(|| v.get("apiKey"))
                .and_then(|x| x.as_str())
            {
                let key = key.trim();
                if !key.is_empty() && !expired_jwt(key) {
                    return Ok(Some(OAuthTokens {
                        access_token: key.to_string(),
                        refresh_token: Some("opencode".into()),
                        expires_at: None,
                        meta: Some(OauthMeta {
                            issuer: "opencode".into(),
                            client_id: "opencode-cli".into(),
                            extra: serde_json::json!({
                                "imported_from": "opencode-cli-legacy",
                                "path": p.display().to_string(),
                                "credential_kind": "api_key",
                            }),
                        }),
                    }));
                }
            }
        }
        Ok(None)
    }

    fn opencode_bin() -> Option<PathBuf> {
        which_cli("opencode").or_else(|| {
            #[cfg(windows)]
            {
                which_cli("opencode.cmd").or_else(|| which_cli("opencode.exe"))
            }
            #[cfg(not(windows))]
            {
                None
            }
        })
    }

    /// `opencode auth login` (interactive provider picker / OAuth).
    pub fn login(tx: &ProgressTx, cancel: &CancelFlag) -> Result<OAuthTokens> {
        if let Ok(Some(t)) = import_opencode_cli() {
            send(
                tx,
                BrowserLoginProgress::Status("using existing OpenCode auth.json session".into()),
            );
            return Ok(t);
        }
        let bin = opencode_bin().ok_or_else(|| {
            NurError::Other(
                "opencode not found on PATH. Install OpenCode (https://opencode.ai), then retry \
                 or paste an OPENCODE_API_KEY via /login."
                    .into(),
            )
        })?;
        send(
            tx,
            BrowserLoginProgress::Status("launching `opencode auth login`…".into()),
        );
        let mut child = Command::new(&bin)
            .args(["auth", "login"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                NurError::Other(format!(
                    "failed to launch opencode ({e}). Run `opencode auth login` in a terminal, \
                     or paste OPENCODE_API_KEY."
                ))
            })?;
        if let Some(err) = child.stderr.take() {
            let tx = tx.clone();
            thread::spawn(move || {
                for line in std::io::BufReader::new(err)
                    .lines()
                    .map_while(|line| line.ok())
                {
                    let snippet: String = line.chars().take(160).collect();
                    if !snippet.trim().is_empty() {
                        send(&tx, BrowserLoginProgress::Status(snippet));
                    }
                }
            });
        }
        if let Some(out) = child.stdout.take() {
            let tx = tx.clone();
            thread::spawn(move || {
                for line in std::io::BufReader::new(out)
                    .lines()
                    .map_while(|line| line.ok())
                {
                    for word in line.split_whitespace() {
                        let url = word
                            .trim_matches(|c: char| c == ')' || c == '(' || c == '"' || c == '\'');
                        if url.starts_with("https://") {
                            send(&tx, BrowserLoginProgress::OpenUrl(url.to_string()));
                            let _ = open_browser(url);
                            break;
                        }
                    }
                }
            });
        }
        let started = std::time::Instant::now();
        // `opencode auth login`'s provider picker is interactive-only (its own
        // issue tracker asks for a non-interactive flag; there is none yet), so
        // with stdin at EOF the child usually exits without authenticating.
        // We still spawn: some versions print the auth URL before the picker,
        // and the watcher above opens it. The deadline + message below are the
        // real path for everyone else.
        const OPENCODE_LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
        loop {
            if cancel.is_cancelled() {
                let _ = child.kill();
                return Err(NurError::Other("login cancelled".into()));
            }
            if started.elapsed() > OPENCODE_LOGIN_TIMEOUT {
                let _ = child.kill();
                return Err(NurError::Other(
                    "opencode auth login did not finish. Its provider picker is interactive \
                     and cannot run inside nur's login screen: run `opencode auth login` in a \
                     separate terminal and select OpenCode, then retry /login (nur imports the \
                     session) — or paste OPENCODE_API_KEY."
                        .into(),
                ));
            }
            match child.try_wait() {
                Ok(Some(status)) if status.success() => break,
                Ok(Some(status)) => {
                    return Err(NurError::Other(format!(
                        "opencode auth login failed (exit {status}). Its provider picker is \
                         interactive: run `opencode auth login` in a separate terminal and \
                         select OpenCode, then retry /login — or paste OPENCODE_API_KEY."
                    )));
                }
                Ok(None) => thread::sleep(Duration::from_millis(200)),
                Err(e) => return Err(NurError::Other(e.to_string())),
            }
        }
        import_opencode_cli()?.ok_or_else(|| {
            NurError::Other(
                "opencode auth login finished, but nur found no `opencode` / `opencode-go` key in \
                 ~/.local/share/opencode/auth.json. Run `opencode auth login` in a terminal and \
                 select the OpenCode (Zen/Go) provider, then retry /login — or set \
                 OPENCODE_API_KEY."
                    .into(),
            )
        })
    }

    pub fn refresh(_auth: &Auth, _refresh: &str) -> Result<OAuthTokens> {
        import_opencode_cli()?.ok_or_else(|| {
            NurError::Other(
                "OpenCode session missing. Run `opencode auth login`, or set OPENCODE_API_KEY."
                    .into(),
            )
        })
    }
    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn token_from_entry_parses_all_shapes() {
            let api = serde_json::json!({"type": "api", "key": "sk-opencode-1234567890"});
            let (access, refresh) = token_from_entry(&api).expect("api entry");
            assert_eq!(access, "sk-opencode-1234567890");
            assert!(refresh.is_none());

            let wellknown = serde_json::json!({"type": "wellknown", "key": "sk-oc-wk-123456"});
            assert!(token_from_entry(&wellknown).is_some());

            let oauth = serde_json::json!({
                "type": "oauth",
                "access": "access-token-value-1",
                "refresh": "refresh-token-value-1",
            });
            let (access, refresh) = token_from_entry(&oauth).expect("oauth entry");
            assert_eq!(access, "access-token-value-1");
            assert_eq!(refresh.as_deref(), Some("refresh-token-value-1"));

            // Aliases for the access field.
            let legacy = serde_json::json!({"type": "oauth", "access_token": "atv-2", "refresh_token": "rtv-2"});
            let (access, refresh) = token_from_entry(&legacy).expect("legacy oauth entry");
            assert_eq!(access, "atv-2");
            assert_eq!(refresh.as_deref(), Some("rtv-2"));

            // Unknown type / empty key => nothing.
            assert!(token_from_entry(&serde_json::json!({"type": "wat"})).is_none());
            assert!(token_from_entry(&serde_json::json!({"type": "api", "key": ""})).is_none());
        }

        /// Env-pasted and file `api` entries must be marked credential_kind
        /// api_key (they must not take OAuth-only routes), while `oauth`
        /// entries keep OAuth semantics.
        #[test]
        fn import_marks_credential_kinds() {
            // Direct env-path check via the same marker the importer writes.
            let dir = std::env::temp_dir().join(format!("nur-oc-test-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("auth.json");
            std::fs::write(
                &path,
                serde_json::json!({
                    "opencode": {"type": "api", "key": "sk-oc-file-1234567890"},
                })
                .to_string(),
            )
            .unwrap();
            let text = std::fs::read_to_string(&path).unwrap();
            let v: serde_json::Value = serde_json::from_str(&text).unwrap();
            let entry = &v["opencode"];
            let (access, refresh) = token_from_entry(entry).unwrap();
            assert_eq!(access, "sk-oc-file-1234567890");
            assert!(refresh.is_none());
            let entry_type = entry.get("type").and_then(|x| x.as_str()).unwrap_or("api");
            assert_ne!(entry_type, "oauth", "api entries are keys, not sessions");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

/// CSPRNG-backed URL-safe random string (v4 UUID bytes → base64url, no pad).
///
/// The single source of randomness for anything credential-shaped: PKCE
/// verifiers, OAuth `state`, and pairing tokens. Do not hand-roll another —
/// an LCG seeded from a `DefaultHasher` looks fine and silently collapses to a
/// handful of possible outputs.
pub(crate) fn random_urlsafe(nbytes: usize) -> String {
    let mut raw = Vec::with_capacity(nbytes);
    while raw.len() < nbytes {
        raw.extend_from_slice(Uuid::new_v4().as_bytes());
    }
    raw.truncate(nbytes);
    URL_SAFE_NO_PAD.encode(&raw)
}

/// `exp` claim of a JWT-shaped credential, when parseable (unix seconds).
pub(crate) fn jwt_expiry(token: &str) -> Option<u64> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    v.get("exp")
        .and_then(|x| x.as_u64().or_else(|| x.as_f64().map(|f| f as u64)))
}

/// True when a JWT-shaped credential is already past its `exp` (5-minute
/// skew, same floor as `crate::auth::oauth_expired`). Non-JWT values are
/// never considered expired — most vendor keys are opaque and long-lived.
fn expired_jwt(token: &str) -> bool {
    jwt_expiry(token).is_some_and(|exp| exp <= crate::oauth::now_unix() + 300)
}

fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

fn http() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent(format!("nur-cli/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| NurError::Other(e.to_string()))
}

/// Resolve a vendor CLI binary: PATH first, then common Windows/macOS install dirs.
fn resolve_cli(name: &str, windows_names: &[&str], extra_dirs: &[PathBuf]) -> Option<PathBuf> {
    // Explicit PATH lookup (Command::new also searches PATH, but we want a real path).
    if let Some(path) = which_cli(name) {
        return Some(path);
    }
    for alt in windows_names {
        if let Some(path) = which_cli(alt) {
            return Some(path);
        }
    }
    for dir in extra_dirs {
        for fname in std::iter::once(name).chain(windows_names.iter().copied()) {
            let p = dir.join(fname);
            if p.is_file() {
                return Some(p);
            }
            #[cfg(windows)]
            {
                let cmd = dir.join(format!("{fname}.cmd"));
                if cmd.is_file() {
                    return Some(cmd);
                }
                let exe = dir.join(format!("{fname}.exe"));
                if exe.is_file() {
                    return Some(exe);
                }
            }
        }
    }
    None
}

fn which_cli(name: &str) -> Option<PathBuf> {
    // Minimal which: scan PATH entries.
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            for ext in ["cmd", "exe", "bat"] {
                let p = dir.join(format!("{name}.{ext}"));
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    None
}

fn gcloud_bin() -> Option<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join("google-cloud-sdk").join("bin"));
        dirs.push(
            home.join("AppData")
                .join("Local")
                .join("Google")
                .join("Cloud SDK")
                .join("google-cloud-sdk")
                .join("bin"),
        );
    }
    #[cfg(windows)]
    {
        if let Ok(pf) = std::env::var("ProgramFiles") {
            dirs.push(
                PathBuf::from(pf)
                    .join("Google")
                    .join("Cloud SDK")
                    .join("google-cloud-sdk")
                    .join("bin"),
            );
        }
        if let Ok(pf) = std::env::var("ProgramFiles(x86)") {
            dirs.push(
                PathBuf::from(pf)
                    .join("Google")
                    .join("Cloud SDK")
                    .join("google-cloud-sdk")
                    .join("bin"),
            );
        }
    }
    resolve_cli("gcloud", &["gcloud.cmd"], &dirs)
}

fn az_bin() -> Option<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(windows)]
    {
        if let Ok(pf) = std::env::var("ProgramFiles") {
            dirs.push(
                PathBuf::from(&pf)
                    .join("Microsoft SDKs")
                    .join("Azure")
                    .join("CLI2")
                    .join("wbin"),
            );
            dirs.push(PathBuf::from(&pf).join("Azure").join("CLI2").join("wbin"));
        }
    }
    resolve_cli("az", &["az.cmd"], &dirs)
}

fn gh_bin() -> Option<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(windows)]
    {
        if let Ok(pf) = std::env::var("ProgramFiles") {
            dirs.push(PathBuf::from(pf).join("GitHub CLI"));
        }
        if let Some(local) = dirs::home_dir() {
            dirs.push(
                local
                    .join("AppData")
                    .join("Local")
                    .join("Programs")
                    .join("GitHub CLI"),
            );
        }
    }
    resolve_cli("gh", &["gh.cmd", "gh.exe"], &dirs)
}

fn validate_callback_state(expected: Option<&str>, received: Option<&str>) -> Result<()> {
    if let Some(expected) = expected {
        if received != Some(expected) {
            return Err(NurError::Other("OAuth state mismatch".into()));
        }
    }
    Ok(())
}

/// Minimal localhost OAuth callback: waits for `?code=` (and optional state).
fn wait_localhost_code_on(
    listener: TcpListener,
    expected_state: Option<&str>,
    cancel: &CancelFlag,
    timeout: Duration,
) -> Result<String> {
    listener
        .set_nonblocking(true)
        .map_err(|e| NurError::Other(e.to_string()))?;
    let start = std::time::Instant::now();
    loop {
        if cancel.is_cancelled() {
            return Err(NurError::Other("login cancelled".into()));
        }
        if start.elapsed() > timeout {
            return Err(NurError::Other("browser login timed out".into()));
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                // Read to end of headers - a single 4096-byte read could
                // truncate a request whose request line exceeds one buffer.
                let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
                let mut buf: Vec<u8> = Vec::with_capacity(4096);
                let mut chunk = [0u8; 4096];
                let head = loop {
                    match stream.read(&mut chunk) {
                        Ok(0) => break String::from_utf8_lossy(&buf).into_owned(),
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        Err(_) => break String::from_utf8_lossy(&buf).into_owned(),
                    }
                    if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break String::from_utf8_lossy(&buf[..pos]).into_owned();
                    }
                    if buf.len() > 64 * 1024 {
                        break String::from_utf8_lossy(&buf).into_owned();
                    }
                };
                let line = head.lines().next().unwrap_or("");
                // GET /callback?code=...&state=... HTTP/1.1
                let path = line.split_whitespace().nth(1).unwrap_or("");
                let q = path.split('?').nth(1).unwrap_or("");
                let mut code = None;
                let mut state = None;
                let mut error = None;
                let mut error_description = None;
                for pair in q.split('&') {
                    let mut it = pair.splitn(2, '=');
                    let k = it.next().unwrap_or("");
                    let v = it.next().unwrap_or("");
                    match k {
                        "code" => code = Some(urlencoding_decode(v)),
                        "state" => state = Some(urlencoding_decode(v)),
                        "error" => error = Some(urlencoding_decode(v)),
                        "error_description" => error_description = Some(urlencoding_decode(v)),
                        _ => {}
                    }
                }
                let state_valid = validate_callback_state(expected_state, state.as_deref()).is_ok();
                let valid_code = code.as_deref().is_some_and(|c| !c.trim().is_empty())
                    && line.starts_with("GET ");
                let accepted = valid_code && error.is_none() && state_valid;
                let body = if accepted {
                    "<html><body><h2>Signed in - you can close this tab and return to NurCLI.</h2></body></html>"
                } else {
                    "<html><body><h2>Login callback rejected - return to NurCLI and try again.</h2></body></html>"
                };
                let resp = format!(
                    "HTTP/1.1 {}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    if accepted { "200 OK" } else { "400 Bad Request" },
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
                // An OAuth error redirect (user denied consent, IdP failure)
                // must fail fast - waiting out the full timeout with no
                // feedback made denials look like hangs.
                if state_valid {
                    if let Some(err) = error {
                        return Err(NurError::Other(format!(
                            "sign-in was denied by the provider: {}",
                            error_description.unwrap_or(err)
                        )));
                    }
                }
                match code {
                    // A code WITH a matching state completes the login.
                    Some(c) if accepted => return Ok(c),
                    // A code with a WRONG state is not ours: stray tab,
                    // prefetcher, replay. Keep listening - aborting here let
                    // any single probe kill a legitimate in-progress login.
                    _ => {}
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(150));
            }
            Err(e) => return Err(NurError::Other(format!("callback accept: {e}"))),
        }
    }
}

/// Wait for a spawned vendor-CLI login to finish: success, failure, cancel,
/// or a hard deadline (these loops used to run unbounded on cancel alone, so
/// a wedged child hung the login thread forever).
fn wait_child_with_deadline(
    child: &mut std::process::Child,
    cancel: &CancelFlag,
    timeout: Duration,
) -> Result<()> {
    let started = std::time::Instant::now();
    loop {
        if cancel.is_cancelled() {
            let _ = child.kill();
            return Err(NurError::Other("login cancelled".into()));
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            return Err(NurError::Other(
                "vendor login did not finish in time - run the vendor's login command in a terminal, then retry /login (nur imports that session)"
                    .into(),
            ));
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(NurError::Other(format!(
                    "login process failed (exit {status})"
                )))
            }
            Ok(None) => thread::sleep(Duration::from_millis(200)),
            Err(e) => return Err(NurError::Other(e.to_string())),
        }
    }
}

const VENDOR_CLI_LOGIN_TIMEOUT: Duration = Duration::from_secs(900);

fn urlencoding_decode(s: &str) -> String {
    // Decode into bytes first: pushing each %XX byte as a `char` mangled
    // multibyte UTF-8 (é became Ã©).
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < b.len() => {
                let hex = &s[i + 1..i + 3];
                if let Ok(v) = u8::from_str_radix(hex, 16) {
                    out.push(v);
                    i += 3;
                } else {
                    out.push(b'%');
                    i += 1;
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn jwt_claims(token: &str) -> Option<serde_json::Value> {
    let payload = token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&decoded).ok()
}

fn jwt_expiration(token: &str) -> Option<u64> {
    jwt_claims(token)?.get("exp")?.as_u64()
}

fn chatgpt_account_meta(id_token: &str) -> (Option<String>, bool) {
    let Some(claims) = jwt_claims(id_token) else {
        return (None, false);
    };
    let auth = claims
        .get("https://api.openai.com/auth")
        .and_then(|value| value.as_object());
    let account_id = auth
        .and_then(|value| value.get("chatgpt_account_id"))
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let is_fedramp = auth
        .and_then(|value| value.get("chatgpt_account_is_fedramp"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    (account_id, is_fedramp)
}

// ── OpenAI (ChatGPT OAuth / Codex backend) ─────────────────────────────────

/// Cline (`api.cline.bot`) — one Bearer credential for Anthropic, OpenAI,
/// Google, MiniMax, Grok and the rest of Cline's catalog.
///
/// Cline's own credential is the account session minted by `cline auth cline`.
/// nur does not re-implement that browser flow: it drives the vendor CLI the
/// same way it does for Cursor and OpenCode, then imports what the Cline SDK
/// persists. A pasted `CLINE_API_KEY` works too, and is the documented
/// programmatic path.
///
/// Store shape verified in the Cline SDK
/// (`sdk/packages/core/src/services/storage/provider-settings-manager.ts` and
/// `types/provider-settings.ts`): `~/.cline/data/settings/providers.json`
/// (data dir overridable with `CLINE_DATA_DIR`) holds
///
/// ```json
/// { "version": 1, "providers": { "cline": { "settings": {
///     "apiKey": "…",
///     "auth": { "accessToken": "…", "refreshToken": "…", "expiresAt": 0 } } } } }
/// ```
///
/// The stored `accessToken` is a **bare** WorkOS token while the API expects it
/// behind the `workos:` scheme prefix Cline's own `formatClineApiKey` adds, so
/// the import applies it; a pasted `apiKey` passes through verbatim.
pub mod cline {
    use super::*;
    use std::path::PathBuf;

    /// Schemed access-token form. Mirrors `WORKOS_TOKEN_PREFIX` in Cline's
    /// `auth/provider-auth-registry.ts`; `formatClineApiKey` is idempotent.
    pub const WORKOS_TOKEN_PREFIX: &str = "workos:";

    /// Refresh marker for sessions born from a plain API key: there is no grant
    /// to exchange, so `refresh` re-imports instead.
    const API_KEY_MARKER: &str = "cline-cli";

    /// The vendor CLI (`npm i -g cline`). npm shims it as `cline.cmd` on
    /// Windows, and a standalone install may sit in the npm global prefix or
    /// `~/.local/bin`.
    fn cline_bin() -> Option<PathBuf> {
        let mut extra = Vec::new();
        if let Some(home) = dirs::home_dir() {
            extra.push(home.join(".local").join("bin"));
            #[cfg(windows)]
            {
                if let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) {
                    extra.push(appdata.join("npm"));
                }
            }
        }
        resolve_cli("cline", &["cline.cmd", "cline.exe"], &extra)
    }

    /// `~/.cline/data/settings/providers.json`. `CLINE_DATA_DIR` replaces the
    /// `~/.cline/data` root (`cline --data-dir` does the same per run).
    fn providers_json_paths() -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Ok(dir) = std::env::var("CLINE_DATA_DIR") {
            let p = PathBuf::from(dir.trim());
            if p.is_absolute() {
                roots.push(p);
            }
        }
        // No CWD-relative fallback: without a home dir, a repo-planted
        // providers.json must not be trusted as a credential store.
        if let Some(home) = dirs::home_dir() {
            roots.push(home.join(".cline").join("data"));
        }
        roots
            .into_iter()
            .map(|root| root.join("settings").join("providers.json"))
            .collect()
    }

    /// The API expects the account token behind the `workos:` scheme prefix.
    fn with_workos_prefix(token: &str) -> String {
        let t = token.trim();
        if t.to_ascii_lowercase().starts_with(WORKOS_TOKEN_PREFIX) {
            t.to_string()
        } else {
            format!("{WORKOS_TOKEN_PREFIX}{t}")
        }
    }

    fn iso_to_unix(s: &str) -> Option<u64> {
        chrono::DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|dt| dt.timestamp().max(0) as u64)
    }

    /// One credential found in `providers.json`.
    struct Stored {
        access: String,
        refresh: Option<String>,
        expires_at: Option<u64>,
        /// `oauth` for the account session, `api_key` for a pasted key.
        kind: &'static str,
    }

    /// Pull the `cline` credential out of a `providers.json` body.
    ///
    /// Only the `cline` entry is read, so a neighbouring provider's key can
    /// never be imported as ours. (`cline-pass` shares this entry: Cline's
    /// registry maps it to the same storage id.)
    fn stored_from_settings(text: &str) -> Option<Stored> {
        let v: serde_json::Value = serde_json::from_str(text).ok()?;
        let settings = v.get("providers")?.get("cline")?.get("settings")?;
        // The account session leads: it is what `cline auth cline` writes, and
        // it outranks a leftover pasted key.
        if let Some(auth) = settings.get("auth") {
            let access = auth
                .get("accessToken")
                .and_then(|x| x.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty());
            if let Some(access) = access {
                let refresh = auth
                    .get("refreshToken")
                    .and_then(|x| x.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());
                let expires_at = auth.get("expiresAt").and_then(|x| {
                    // Cline stores ms-epoch when it converts the ISO field.
                    x.as_u64()
                        .map(|ms| ms / 1000)
                        .or_else(|| x.as_str().and_then(iso_to_unix))
                });
                return Some(Stored {
                    access: with_workos_prefix(access),
                    refresh,
                    expires_at,
                    kind: "oauth",
                });
            }
        }
        // Cline's own resolution is `settings.apiKey || settings.auth.apiKey`
        // (`resolveProviderApiKeyFromSettings`), so match that order.
        let key = settings
            .get("apiKey")
            .or_else(|| settings.get("auth").and_then(|auth| auth.get("apiKey")))
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())?;
        Some(Stored {
            access: key.to_string(),
            refresh: None,
            expires_at: None,
            kind: "api_key",
        })
    }

    fn tokens_for(stored: Stored, via: &str, path: Option<&str>) -> OAuthTokens {
        let credential_kind = stored.kind;
        OAuthTokens {
            access_token: stored.access,
            refresh_token: Some(stored.refresh.unwrap_or_else(|| API_KEY_MARKER.to_string())),
            expires_at: stored.expires_at,
            meta: Some(OauthMeta {
                issuer: "cline".into(),
                client_id: "cline-cli".into(),
                extra: serde_json::json!({
                    "imported_from": via,
                    "path": path.unwrap_or(""),
                    "credential_kind": credential_kind,
                }),
            }),
        }
    }

    pub fn import_cline_cli() -> Result<Option<OAuthTokens>> {
        // 1. `CLINE_API_KEY` is the documented headless credential.
        if let Ok(key) = std::env::var("CLINE_API_KEY") {
            let key = key.trim().to_string();
            if !key.is_empty() {
                return Ok(Some(tokens_for(
                    Stored {
                        access: key,
                        refresh: None,
                        expires_at: None,
                        kind: "api_key",
                    },
                    "CLINE_API_KEY",
                    None,
                )));
            }
        }

        for p in providers_json_paths() {
            if !p.is_file() {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&p) else {
                continue;
            };
            let Some(stored) = stored_from_settings(&text) else {
                continue;
            };
            // A stored account token already past `exp` is a signed-out
            // session, but one carrying a refresh token can still be revived by
            // `refresh` - so only an unrenewable dead token is skipped here.
            if expired_jwt(&stored.access) && stored.refresh.is_none() {
                continue;
            }
            return Ok(Some(tokens_for(
                stored,
                "cline-cli",
                Some(&p.display().to_string()),
            )));
        }
        Ok(None)
    }

    pub fn login(tx: &ProgressTx, cancel: &CancelFlag) -> Result<OAuthTokens> {
        // Already signed in through the Cline CLI? One import, no prompt.
        if let Ok(Some(t)) = import_cline_cli() {
            send(
                tx,
                BrowserLoginProgress::Status("using existing Cline session".into()),
            );
            return Ok(t);
        }

        let bin = cline_bin().ok_or_else(|| {
            NurError::Other(
                "cline not found on PATH. Install the Cline CLI (`npm i -g cline`), run \
                 `cline auth cline` in a terminal - or paste a CLINE_API_KEY from \
                 app.cline.bot → Settings → API Keys."
                    .into(),
            )
        })?;

        send(
            tx,
            BrowserLoginProgress::Status("launching Cline sign-in (cline auth cline)…".into()),
        );
        // Never hardcode Cline's login URL: wait for the CLI's own auth URL.
        let mut child = Command::new(&bin)
            .args(["auth", "cline"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                NurError::Other(format!(
                    "failed to launch cline ({e}). Run `cline auth cline` in a terminal, or paste \
                     a CLINE_API_KEY."
                ))
            })?;

        if let Some(err) = child.stderr.take() {
            watch_login_output(err, tx.clone());
        }
        if let Some(out) = child.stdout.take() {
            watch_login_output(out, tx.clone());
        }

        // Cline's OAuth hands back to the CLI over a loopback callback on
        // 48801-48811; the browser round trip is the slow part.
        const CLINE_LOGIN_TIMEOUT: Duration = Duration::from_secs(600);
        let started = std::time::Instant::now();
        loop {
            if cancel.is_cancelled() {
                let _ = child.kill();
                return Err(NurError::Other("login cancelled".into()));
            }
            if started.elapsed() > CLINE_LOGIN_TIMEOUT {
                let _ = child.kill();
                return Err(NurError::Other(
                    "cline auth cline did not finish within 10 minutes. Run it in a terminal, \
                     then retry /login (nur imports the session) - or paste a CLINE_API_KEY."
                        .into(),
                ));
            }
            match child.try_wait() {
                Ok(Some(status)) if status.success() => break,
                Ok(Some(status)) => {
                    return Err(NurError::Other(format!(
                        "cline auth cline failed (exit {status}). Paste a CLINE_API_KEY from \
                         app.cline.bot as a fallback."
                    )));
                }
                Ok(None) => thread::sleep(Duration::from_millis(200)),
                Err(e) => return Err(NurError::Other(e.to_string())),
            }
        }

        send(
            tx,
            BrowserLoginProgress::Status("importing Cline session…".into()),
        );
        import_cline_cli()?.ok_or_else(|| {
            NurError::Other(
                "cline auth cline finished, but nur found no `cline` credential in \
                 ~/.cline/data/settings/providers.json. Run `cline auth cline` in a terminal and \
                 finish the browser sign-in, then retry /login - or paste a CLINE_API_KEY."
                    .into(),
            )
        })
    }

    /// Exchange the stored refresh token for a fresh access token.
    ///
    /// Cline's refresh endpoint takes only the token itself (no client id), so
    /// nur can renew the session without the vendor CLI running.
    pub fn refresh(_auth: &Auth, refresh_token: &str) -> Result<OAuthTokens> {
        let refresh_token = refresh_token.trim();
        if refresh_token.is_empty() || refresh_token == API_KEY_MARKER {
            // API-key session: the credential lives in the env or the CLI store.
            return import_cline_cli()?.ok_or_else(|| {
                NurError::Other(
                    "Cline session missing. Run `cline auth cline`, or set CLINE_API_KEY.".into(),
                )
            });
        }

        let url = format!("{}/auth/refresh", crate::providers::CLINE_BASE_URL);
        let body = serde_json::json!({
            "refreshToken": refresh_token,
            "grantType": "refresh_token",
        });
        let client = http()?;
        let response = client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .map_err(|e| NurError::Other(format!("Cline refresh request failed: {e}")))?;
        let status = response.status();
        let text = response.text().unwrap_or_default();
        if !status.is_success() {
            return Err(NurError::Other(format!(
                "Cline refresh failed (HTTP {}): {} · run `cline auth cline` to sign in again",
                status.as_u16(),
                oauth_error_summary(&text)
            )));
        }
        let v: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| NurError::Other(format!("Cline refresh returned non-JSON: {e}")))?;
        let data = v.get("data").unwrap_or(&v);
        let access = data
            .get("accessToken")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                NurError::Other(
                    "Cline refresh response carried no accessToken · run `cline auth cline`".into(),
                )
            })?;
        let rotated = data
            .get("refreshToken")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        let expires_at = data.get("expiresAt").and_then(|x| {
            x.as_u64()
                .map(|ms| ms / 1000)
                .or_else(|| x.as_str().and_then(iso_to_unix))
        });
        Ok(OAuthTokens {
            access_token: with_workos_prefix(access),
            refresh_token: Some(rotated.unwrap_or_else(|| refresh_token.to_string())),
            expires_at,
            meta: Some(OauthMeta {
                issuer: "cline".into(),
                client_id: "cline-cli".into(),
                extra: serde_json::json!({"credential_kind": "oauth"}),
            }),
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        const OAUTH_STORE: &str = r#"{
            "version": 1,
            "lastUsedProvider": "cline",
            "modes": {},
            "providers": {
                "cline": {
                    "settings": {
                        "provider": "cline",
                        "auth": {
                            "accessToken": "eyJhbGciOiJIUzI1NiJ9.b2.zzz",
                            "refreshToken": "refresh-abc",
                            "expiresAt": 4102444800000
                        }
                    },
                    "updatedAt": "2026-09-16T00:00:00.000Z",
                    "tokenSource": "oauth"
                },
                "anthropic": { "settings": { "apiKey": "sk-ant-not-ours" } }
            }
        }"#;

        #[test]
        fn reads_the_account_session_and_schemes_the_token() {
            let s = stored_from_settings(OAUTH_STORE).expect("cline entry");
            assert_eq!(s.access, "workos:eyJhbGciOiJIUzI1NiJ9.b2.zzz");
            assert_eq!(s.refresh.as_deref(), Some("refresh-abc"));
            // ms epoch in the store -> seconds for nur.
            assert_eq!(s.expires_at, Some(4_102_444_800));
            assert_eq!(s.kind, "oauth");
        }

        #[test]
        fn falls_back_to_a_pasted_key_and_never_borrows_a_neighbour() {
            let pasted = r#"{"version":1,"providers":{"cline":{"settings":{"apiKey":"cline-key-1234567890"}}}}"#;
            let s = stored_from_settings(pasted).expect("pasted key");
            assert_eq!(s.access, "cline-key-1234567890");
            assert_eq!(s.kind, "api_key");
            assert!(s.refresh.is_none());

            // Cline's own fallback order also accepts `auth.apiKey`.
            let in_auth = r#"{"version":1,"providers":{"cline":{"settings":{"auth":{"apiKey":"cline-key-auth-1"}}}}}"#;
            let s = stored_from_settings(in_auth).expect("auth.apiKey");
            assert_eq!(s.access, "cline-key-auth-1");
            assert_eq!(s.kind, "api_key");

            // An account session outranks a leftover pasted key.
            let both = r#"{"version":1,"providers":{"cline":{"settings":{"apiKey":"cline-key-1234567890","auth":{"accessToken":"aaaa.bbbb.cccc"}}}}}"#;
            let s = stored_from_settings(both).expect("session wins");
            assert_eq!(s.access, "workos:aaaa.bbbb.cccc");
            assert_eq!(s.kind, "oauth");

            // Only the `cline` entry counts: another provider's key must not
            // satisfy the import.
            let other =
                r#"{"version":1,"providers":{"openai":{"settings":{"apiKey":"sk-openai-x"}}}}"#;
            assert!(stored_from_settings(other).is_none());
            assert!(stored_from_settings("not json").is_none());
            assert!(stored_from_settings(r#"{"version":1,"providers":{}}"#).is_none());
            // Blank/whitespace credentials are not credentials.
            let blank = r#"{"version":1,"providers":{"cline":{"settings":{"apiKey":"   "}}}}"#;
            assert!(stored_from_settings(blank).is_none());
        }

        /// `CLINE_DATA_DIR` relocates the data root, so the importer must follow
        /// it rather than only reading `~/.cline/data` - otherwise a sandboxed
        /// or relocated Cline install is invisible to nur.
        #[test]
        fn import_follows_cline_data_dir() {
            let dir = std::env::temp_dir().join(format!("nur-cline-e2e-{}", std::process::id()));
            let settings = dir.join("settings");
            std::fs::create_dir_all(&settings).expect("temp settings dir");
            std::fs::write(settings.join("providers.json"), OAUTH_STORE).expect("write store");

            let previous = std::env::var("CLINE_DATA_DIR").ok();
            std::env::set_var("CLINE_DATA_DIR", &dir);
            let imported = import_cline_cli();
            match previous {
                Some(v) => std::env::set_var("CLINE_DATA_DIR", v),
                None => std::env::remove_var("CLINE_DATA_DIR"),
            }
            let _ = std::fs::remove_dir_all(&dir);

            let tokens = imported
                .expect("import runs")
                .expect("session imported from CLINE_DATA_DIR");
            assert_eq!(tokens.access_token, "workos:eyJhbGciOiJIUzI1NiJ9.b2.zzz");
            assert_eq!(tokens.refresh_token.as_deref(), Some("refresh-abc"));
            assert_eq!(tokens.expires_at, Some(4_102_444_800));
            let extra = tokens.meta.expect("meta").extra;
            assert_eq!(extra["credential_kind"], "oauth");
            assert_eq!(extra["imported_from"], "cline-cli");
            assert!(
                extra["path"]
                    .as_str()
                    .is_some_and(|p| p.contains("providers.json")),
                "path should name the store it read: {extra:?}"
            );
        }

        #[test]
        fn an_already_schemed_token_is_not_double_prefixed() {
            assert_eq!(with_workos_prefix("workos:abc"), "workos:abc");
            assert_eq!(with_workos_prefix("WORKOS:abc"), "WORKOS:abc");
            assert_eq!(with_workos_prefix("  abc  "), "workos:abc");
        }

        #[test]
        fn iso_expiry_parses_and_junk_fails_soft() {
            assert_eq!(iso_to_unix("2029-01-01T00:00:00Z"), Some(1_861_920_000));
            assert!(iso_to_unix("soon").is_none());
        }
    }
}

pub mod openai {
    use super::*;

    pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
    const ISSUER: &str = "https://auth.openai.com";
    const CALLBACK_PORTS: &[u16] = &[1455, 1457];

    #[derive(Deserialize)]
    struct TokenResp {
        access_token: Option<String>,
        refresh_token: Option<String>,
        id_token: Option<String>,
        expires_in: Option<u64>,
        error: Option<serde_json::Value>,
    }

    #[derive(Deserialize)]
    struct CodexAuthFile {
        tokens: CodexTokenSet,
    }

    #[derive(Deserialize)]
    struct CodexTokenSet {
        access_token: String,
        refresh_token: Option<String>,
        id_token: Option<String>,
        account_id: Option<String>,
    }

    /// Originator OpenAI's auth + Codex backend accept (see codex-rs default_client).
    /// Unknown values (e.g. `nur_cli`) make authorize return missing_required_parameter.
    pub const ORIGINATOR: &str = "codex_cli_rs";

    pub fn login(tx: &ProgressTx, cancel: &CancelFlag) -> Result<OAuthTokens> {
        // Prefer an existing Codex CLI session — no browser round-trip.
        if let Ok(Some(imported)) = import_codex_cli() {
            send(
                tx,
                BrowserLoginProgress::Status("using existing Codex CLI session".into()),
            );
            return Ok(imported);
        }

        let (listener, port) = CALLBACK_PORTS
            .iter()
            .find_map(|port| {
                TcpListener::bind(("127.0.0.1", *port))
                    .ok()
                    .map(|listener| (listener, *port))
            })
            .ok_or_else(|| {
                NurError::Other(
                    "OpenAI login needs localhost port 1455 or 1457, but both are in use. Close Codex or free those ports, or run `codex login` and choose “Use existing CLI session”.".into(),
                )
            })?;
        // Codex registers localhost (not 127.0.0.1) + /auth/callback on 1455/1457.
        let redirect = format!("http://localhost:{port}/auth/callback");
        let verifier = random_urlsafe(64);
        let challenge = pkce_challenge(&verifier);
        let state = random_urlsafe(32);
        let scope = "openid profile email offline_access api.connectors.read api.connectors.invoke";
        // Param set mirrors codex-rs login (originator must be a known Codex client).
        let auth_url = format!(
            "{ISSUER}/oauth/authorize?response_type=code&client_id={CLIENT_ID}&redirect_uri={}&scope={}&code_challenge={challenge}&code_challenge_method=S256&id_token_add_organizations=true&codex_cli_simplified_flow=true&state={state}&originator={ORIGINATOR}",
            urlencoding_encode(&redirect),
            urlencoding_encode(scope),
        );

        send(tx, BrowserLoginProgress::OpenUrl(auth_url.clone()));
        send(
            tx,
            BrowserLoginProgress::Status(
                "complete OpenAI / ChatGPT sign-in in the browser…".into(),
            ),
        );
        let _ = open_browser(&auth_url);
        let code =
            wait_localhost_code_on(listener, Some(&state), cancel, Duration::from_secs(600))?;
        send(
            tx,
            BrowserLoginProgress::Status("exchanging OpenAI authorization code…".into()),
        );

        let form = [
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("client_id", CLIENT_ID),
            ("code_verifier", verifier.as_str()),
        ];
        let response = http()?
            .post(format!("{ISSUER}/oauth/token"))
            .header(
                "Content-Type",
                "application/x-www-form-urlencoded;charset=utf-8",
            )
            .form(&form)
            .send()
            .map_err(|error| NurError::Other(format!("OpenAI token exchange failed: {error}")))?;
        parse_token_response(response, None, None)
    }

    pub fn refresh(auth: &Auth, refresh_token: &str) -> Result<OAuthTokens> {
        // Deliberately a JSON body while the login exchange uses form encoding
        // - this mirrors codex-rs exactly; do not "symmetrize" without testing
        // against auth.openai.com.
        let body = serde_json::json!({
            "client_id": CLIENT_ID,
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
        });
        let response = http()?
            .post(format!("{ISSUER}/oauth/token"))
            .json(&body)
            .send()
            .map_err(|error| NurError::Other(format!("OpenAI token refresh failed: {error}")))?;
        parse_token_response(response, Some(refresh_token), auth.oauth_meta.clone())
    }

    /// Reuse the official Codex CLI login when present. This reads only the
    /// first-party token cache and converts it into Nur's normal OAuth shape.
    pub fn import_codex_cli() -> Result<Option<OAuthTokens>> {
        // Respect CODEX_HOME exactly like the official CLI. Reading only
        // ~/.codex made a valid isolated/workspace login look signed out.
        let path =
            crate::t3code::driver_config_dir(crate::t3code::DriverId::Codex).join("auth.json");
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(path)?;
        let mut tokens = codex_tokens_from_json(&text)?;
        if tokens
            .expires_at
            .is_some_and(|expiry| expiry <= super::super::now_unix().saturating_add(300))
        {
            if let Some(refresh_token) = tokens.refresh_token.clone() {
                let auth = Auth {
                    api_key: tokens.access_token.clone(),
                    source: "oauth".into(),
                    auth_method: crate::auth::AuthMethod::Oauth,
                    provider: "openai".into(),
                    refresh_token: Some(refresh_token.clone()),
                    expires_at: tokens.expires_at,
                    oauth_meta: tokens.meta.clone(),
                };
                tokens = refresh(&auth, &refresh_token)?;
            }
        }
        Ok(Some(tokens))
    }

    pub(super) fn codex_tokens_from_json(text: &str) -> Result<OAuthTokens> {
        let parsed: CodexAuthFile = serde_json::from_str(text)
            .map_err(|error| NurError::Other(format!("invalid Codex auth file: {error}")))?;
        let access_token = parsed.tokens.access_token.trim().to_string();
        if access_token.is_empty() {
            return Err(NurError::Other(
                "Codex auth file has no access token; run `codex login` again".into(),
            ));
        }
        let (claim_account_id, is_fedramp) = parsed
            .tokens
            .id_token
            .as_deref()
            .map(chatgpt_account_meta)
            .unwrap_or((None, false));
        let account_id = parsed
            .tokens
            .account_id
            .filter(|value| !value.trim().is_empty())
            .or(claim_account_id);
        Ok(OAuthTokens {
            expires_at: jwt_expiration(&access_token),
            access_token,
            refresh_token: parsed.tokens.refresh_token,
            meta: Some(OauthMeta {
                issuer: ISSUER.into(),
                client_id: CLIENT_ID.into(),
                extra: serde_json::json!({
                    "account_id": account_id,
                    "is_fedramp": is_fedramp,
                    "imported_from": "codex-cli",
                }),
            }),
        })
    }

    fn parse_token_response(
        response: reqwest::blocking::Response,
        previous_refresh: Option<&str>,
        previous_meta: Option<OauthMeta>,
    ) -> Result<OAuthTokens> {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        let parsed: TokenResp = serde_json::from_str(&body).map_err(|error| {
            NurError::Other(format!(
                "OpenAI returned an invalid token response ({status}): {error}"
            ))
        })?;
        if !status.is_success() {
            let detail = parsed
                .error
                .map(|value| value.to_string())
                .unwrap_or_else(|| format!("HTTP {}", status.as_u16()));
            return Err(NurError::Other(format!("OpenAI OAuth failed: {detail}")));
        }
        let access_token = parsed
            .access_token
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                NurError::Other("OpenAI OAuth response did not include an access token".into())
            })?;
        let refresh_token = parsed
            .refresh_token
            .or_else(|| previous_refresh.map(str::to_string));
        let mut meta = previous_meta.unwrap_or(OauthMeta {
            issuer: ISSUER.into(),
            client_id: CLIENT_ID.into(),
            extra: serde_json::json!({}),
        });
        if let Some(id_token) = parsed.id_token.as_deref() {
            let (account_id, is_fedramp) = chatgpt_account_meta(id_token);
            meta.extra = serde_json::json!({
                "account_id": account_id,
                "is_fedramp": is_fedramp,
            });
        }
        let expires_at =
            expires_in_to_at(parsed.expires_in).or_else(|| jwt_expiration(&access_token));
        Ok(OAuthTokens {
            access_token,
            refresh_token,
            expires_at,
            meta: Some(meta),
        })
    }
}

// ── xAI Grok (device code / Grok CLI import) ───────────────────────────────

pub mod xai {
    use super::*;

    /// Public Grok CLI OIDC client (same as ~/.grok/auth.json entries).
    pub const CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
    pub const ISSUER: &str = "https://auth.x.ai";

    #[derive(Deserialize)]
    struct DeviceCodeResp {
        device_code: String,
        user_code: String,
        verification_uri: Option<String>,
        verification_uri_complete: Option<String>,
        #[serde(default)]
        expires_in: u64,
        #[serde(default = "default_interval")]
        interval: u64,
    }
    fn default_interval() -> u64 {
        5
    }

    #[derive(Deserialize)]
    struct TokenResp {
        access_token: Option<String>,
        refresh_token: Option<String>,
        expires_in: Option<u64>,
        error: Option<String>,
        error_description: Option<String>,
    }

    pub fn login(tx: &ProgressTx, cancel: &CancelFlag) -> Result<OAuthTokens> {
        send(
            tx,
            BrowserLoginProgress::Status("requesting xAI device code…".into()),
        );
        let client = http()?;
        // OIDC device authorization endpoint (Grok / auth.x.ai).
        let endpoints = [
            format!("{ISSUER}/oauth/device/code"),
            format!("{ISSUER}/oauth2/device/code"),
            "https://accounts.x.ai/oauth/device/code".to_string(),
        ];
        let mut device: Option<DeviceCodeResp> = None;
        let mut last_err = String::new();
        for url in &endpoints {
            let form = [
                ("client_id", CLIENT_ID),
                (
                    "scope",
                    "openid profile email offline_access grok-cli:access api:access",
                ),
            ];
            match client.post(url).form(&form).send() {
                Ok(res) => {
                    let status = res.status();
                    let body = res.text().unwrap_or_default();
                    if status.is_success() {
                        match serde_json::from_str::<DeviceCodeResp>(&body) {
                            Ok(d) => {
                                device = Some(d);
                                break;
                            }
                            Err(e) => last_err = format!("parse device code: {e}"),
                        }
                    } else {
                        last_err = format!("{url} -> {status}: {}", oauth_error_summary(&body));
                    }
                }
                Err(e) => last_err = e.to_string(),
            }
        }
        let device = device.ok_or_else(|| {
            NurError::Other(format!(
                "xAI device code failed ({last_err}). Paste an XAI_API_KEY or sign in with the Grok CLI first."
            ))
        })?;

        let verify = device
            .verification_uri_complete
            .clone()
            .or_else(|| {
                device.verification_uri.clone().map(|u| {
                    let sep = if u.contains('?') { '&' } else { '?' };
                    format!("{u}{sep}user_code={}", device.user_code)
                })
            })
            .unwrap_or_else(|| {
                format!(
                    "https://accounts.x.ai/connect?user_code={}",
                    device.user_code
                )
            });

        send(
            tx,
            BrowserLoginProgress::DeviceCode {
                verification_url: verify.clone(),
                user_code: device.user_code.clone(),
            },
        );
        let _ = open_browser(&verify);

        let token_urls = [
            format!("{ISSUER}/oauth/token"),
            format!("{ISSUER}/oauth2/token"),
            "https://accounts.x.ai/oauth/token".to_string(),
        ];
        let deadline = std::time::Instant::now()
            + Duration::from_secs(if device.expires_in > 0 {
                device.expires_in
            } else {
                900
            });
        let base_interval = device.interval.max(3);
        let mut poll = super::super::DevicePoll::new(base_interval);
        let mut terminal_errors: Vec<String> = Vec::new();

        while std::time::Instant::now() < deadline {
            if cancel.is_cancelled() {
                return Err(NurError::Other("login cancelled".into()));
            }
            poll.wait(cancel, deadline)?;
            for turl in &token_urls {
                let form = [
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ("device_code", device.device_code.as_str()),
                    ("client_id", CLIENT_ID),
                ];
                let Ok(res) = client.post(turl).form(&form).send() else {
                    continue;
                };
                let body = res.text().unwrap_or_default();
                let parsed: TokenResp = serde_json::from_str(&body).unwrap_or(TokenResp {
                    access_token: None,
                    refresh_token: None,
                    expires_in: None,
                    error: Some("parse".into()),
                    error_description: Some(body.clone()),
                });
                match classify_device_poll_error(&parsed) {
                    DevicePollOutcome::NoError => {}
                    DevicePollOutcome::Pending => break,
                    DevicePollOutcome::SlowDown => {
                        poll.slow_down();
                        break;
                    }
                    DevicePollOutcome::Unparseable => continue,
                    DevicePollOutcome::Terminal(reason) => {
                        // Terminal for THIS endpoint only. Another candidate
                        // host may still be the right one - remember it and
                        // fail only after every candidate has refused.
                        terminal_errors.push(reason);
                        continue;
                    }
                }
                if let Some(access) = parsed.access_token {
                    return Ok(OAuthTokens {
                        access_token: access,
                        refresh_token: parsed.refresh_token,
                        expires_at: expires_in_to_at(parsed.expires_in),
                        meta: Some(OauthMeta {
                            issuer: ISSUER.into(),
                            client_id: CLIENT_ID.into(),
                            extra: serde_json::json!({}),
                        }),
                    });
                }
            }
            // Every candidate refused with a definitive error and nothing is
            // pending anywhere - fail now with the collected reasons.
            if let Some(msg) = terminal_failure(&terminal_errors, token_urls.len()) {
                return Err(NurError::Other(msg));
            }
            terminal_errors.clear();
            send(
                tx,
                BrowserLoginProgress::Status("waiting for browser approval…".into()),
            );
        }
        Err(NurError::Other("xAI device login timed out".into()))
    }

    pub fn refresh(auth: &Auth, refresh: &str) -> Result<OAuthTokens> {
        let client = http()?;
        let client_id = auth
            .oauth_meta
            .as_ref()
            .map(|m| m.client_id.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(CLIENT_ID);
        let form = [
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh),
            ("client_id", client_id),
        ];
        let res = client
            .post(format!("{ISSUER}/oauth/token"))
            .form(&form)
            .send()
            .map_err(|e| NurError::Other(e.to_string()))?;
        let body = res.text().unwrap_or_default();
        let parsed: TokenResp = serde_json::from_str(&body)
            .map_err(|e| NurError::Other(format!("invalid xAI refresh response: {e}")))?;
        let access = parsed.access_token.ok_or_else(|| {
            NurError::Other(format!(
                "xAI refresh failed: {}",
                oauth_error_summary(&body)
            ))
        })?;
        Ok(OAuthTokens {
            access_token: access,
            refresh_token: parsed.refresh_token.or_else(|| Some(refresh.to_string())),
            expires_at: expires_in_to_at(parsed.expires_in),
            meta: auth.oauth_meta.clone(),
        })
    }

    pub fn import_grok_cli() -> Result<Option<OAuthTokens>> {
        let mut candidates: Vec<PathBuf> =
            vec![crate::t3code::driver_config_dir(crate::t3code::DriverId::Grok).join("auth.json")];
        // No CWD-relative fallback when home is unavailable.
        if let Some(home) = dirs::home_dir() {
            candidates.push(home.join(".grok").join("auth.json"));
        }
        for path in candidates {
            if !path.exists() {
                continue;
            }
            let text = std::fs::read_to_string(&path)?;
            let v: serde_json::Value = serde_json::from_str(&text)?;
            // Map of "issuer::client_id" -> session object.
            if let Some(map) = v.as_object() {
                if let Some(tokens) = pick_grok_session(map) {
                    return Ok(Some(tokens));
                }
            }
        }
        Ok(None)
    }

    /// Pick a session from a `~/.grok/auth.json` map: a LIVE session wins over
    /// map ordering (the old first-non-empty pick kept importing a stale entry
    /// even when a valid one sat right next to it). Only-stale maps fall back
    /// to the stale session - the refresh path surfaces a clean relogin.
    fn pick_grok_session(map: &serde_json::Map<String, serde_json::Value>) -> Option<OAuthTokens> {
        let mut fallback: Option<OAuthTokens> = None;
        for (_k, sess) in map {
            let access = sess
                .get("key")
                .or_else(|| sess.get("access_token"))
                .and_then(|x| x.as_str())
                .unwrap_or("");
            if access.is_empty() {
                continue;
            }
            let expires_at = sess
                .get("expires_at")
                .and_then(|x| x.as_str())
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.timestamp() as u64);
            let is_expired = expires_at.is_some_and(|exp| exp <= crate::oauth::now_unix() + 300);
            let refresh = sess
                .get("refresh_token")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string());
            let client_id = sess
                .get("oidc_client_id")
                .and_then(|x| x.as_str())
                .unwrap_or(CLIENT_ID)
                .to_string();
            let issuer = sess
                .get("oidc_issuer")
                .and_then(|x| x.as_str())
                .unwrap_or(ISSUER)
                .to_string();
            let tokens = OAuthTokens {
                access_token: access.to_string(),
                refresh_token: refresh,
                expires_at,
                meta: Some(OauthMeta {
                    issuer,
                    client_id,
                    extra: serde_json::json!({"imported_from": "grok-cli"}),
                }),
            };
            if is_expired {
                fallback.get_or_insert(tokens);
            } else {
                return Some(tokens);
            }
        }
        fallback
    }

    /// Device-poll error classification, shared by the poll loop and tests.
    #[derive(Debug, PartialEq, Eq)]
    enum DevicePollOutcome {
        /// No `error` field - the caller checks for an access token.
        NoError,
        Pending,
        SlowDown,
        /// Body was not the expected JSON shape (mapped to `error: "parse"`).
        Unparseable,
        /// Definitive refusal from this endpoint, with the formatted reason.
        Terminal(String),
    }

    fn classify_device_poll_error(parsed: &TokenResp) -> DevicePollOutcome {
        let Some(err) = parsed.error.as_deref().filter(|e| !e.is_empty()) else {
            return DevicePollOutcome::NoError;
        };
        match err {
            "authorization_pending" => DevicePollOutcome::Pending,
            "slow_down" => DevicePollOutcome::SlowDown,
            "parse" => DevicePollOutcome::Unparseable,
            other => DevicePollOutcome::Terminal(format!(
                "{other} {}",
                parsed.error_description.clone().unwrap_or_default()
            )),
        }
    }

    /// All candidate endpoints refused with definitive errors and nothing is
    /// pending anywhere - surface the collected reasons as one failure.
    fn terminal_failure(errors: &[String], endpoint_count: usize) -> Option<String> {
        (errors.len() >= endpoint_count).then(|| format!("xAI token error: {}", errors.join("; ")))
    }
    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn device_code_resp_parses_with_defaults() {
            let d: DeviceCodeResp = serde_json::from_str(
                r#"{"device_code":"dc_1","user_code":"ABCD-EFGH",
                    "verification_uri":"https://accounts.x.ai/connect"}"#,
            )
            .expect("minimal payload");
            assert_eq!(d.device_code, "dc_1");
            assert_eq!(d.user_code, "ABCD-EFGH");
            assert!(d.verification_uri_complete.is_none());
            assert_eq!(d.expires_in, 0, "expires_in defaults to 0 -> 900s fallback");
            assert_eq!(d.interval, 5, "interval default");

            let full: DeviceCodeResp = serde_json::from_str(
                r#"{"device_code":"dc","user_code":"U","interval":2,"expires_in":600,
                    "verification_uri":"u","verification_uri_complete":"uc"}"#,
            )
            .expect("full payload");
            assert_eq!(full.interval, 2);
            assert_eq!(full.expires_in, 600);
            // Garbage must not parse into a half-valid response.
            assert!(serde_json::from_str::<DeviceCodeResp>("{}").is_err());
        }

        #[test]
        fn token_resp_parses_success_and_error_shapes() {
            let ok: TokenResp = serde_json::from_str(
                r#"{"access_token":"at","refresh_token":"rt","expires_in":3600}"#,
            )
            .unwrap();
            assert_eq!(ok.access_token.as_deref(), Some("at"));
            assert_eq!(ok.error, None);

            let err: TokenResp =
                serde_json::from_str(r#"{"error":"access_denied","error_description":"nope"}"#)
                    .unwrap();
            assert_eq!(err.access_token, None);
            assert_eq!(err.error.as_deref(), Some("access_denied"));

            // Non-JSON poll bodies become a synthetic "parse" error upstream;
            // classification treats that as retryable-unparseable, not fatal.
            let parse_shape = TokenResp {
                access_token: None,
                refresh_token: None,
                expires_in: None,
                error: Some("parse".into()),
                error_description: None,
            };
            assert_eq!(
                classify_device_poll_error(&parse_shape),
                DevicePollOutcome::Unparseable
            );
        }

        #[test]
        fn poll_error_classification_matches_the_loop_semantics() {
            let mk = |error: Option<&str>, desc: Option<&str>| TokenResp {
                access_token: None,
                refresh_token: None,
                expires_in: None,
                error: error.map(str::to_string),
                error_description: desc.map(str::to_string),
            };
            assert_eq!(
                classify_device_poll_error(&mk(None, None)),
                DevicePollOutcome::NoError
            );
            assert_eq!(
                classify_device_poll_error(&mk(Some(""), None)),
                DevicePollOutcome::NoError
            );
            assert_eq!(
                classify_device_poll_error(&mk(Some("authorization_pending"), None)),
                DevicePollOutcome::Pending
            );
            assert_eq!(
                classify_device_poll_error(&mk(Some("slow_down"), None)),
                DevicePollOutcome::SlowDown
            );
            assert_eq!(
                classify_device_poll_error(&mk(Some("expired_token"), Some("gone"))),
                DevicePollOutcome::Terminal("expired_token gone".into())
            );
        }

        /// A terminal error from ONE candidate endpoint must not fail the
        /// login while other candidates remain; only a full round of refusals
        /// surfaces the collected reasons.
        #[test]
        fn terminal_errors_accumulate_before_failing() {
            let mut errors: Vec<String> = Vec::new();
            // First endpoint refuses.
            errors.push("expired_token gone".to_string());
            assert!(
                terminal_failure(&errors, 3).is_none(),
                "2 endpoints still pending"
            );
            // Nothing pending anywhere -> next cycle would collect all three.
            errors.push("access_denied no".to_string());
            errors.push("server_error boom".to_string());
            let msg = terminal_failure(&errors, 3).expect("all endpoints refused");
            assert!(msg.contains("expired_token gone"));
            assert!(msg.contains("access_denied no"));
            assert!(msg.contains("server_error boom"));
        }

        #[test]
        fn grok_session_pick_prefers_live_over_stale() {
            let far_future = "2099-01-01T00:00:00Z";
            let near_past = "2020-01-01T00:00:00Z";
            let mut map = serde_json::Map::new();
            // Stale session FIRST - map ordering must not decide.
            map.insert(
                "stale::c".into(),
                serde_json::json!({
                    "key": "stale-key-value",
                    "refresh_token": "rt-stale",
                    "expires_at": near_past,
                }),
            );
            map.insert(
                "live::c".into(),
                serde_json::json!({
                    "key": "live-key-value",
                    "refresh_token": "rt-live",
                    "expires_at": far_future,
                }),
            );
            let picked = pick_grok_session(&map).expect("live session wins");
            assert_eq!(picked.access_token, "live-key-value");
            assert_eq!(picked.refresh_token.as_deref(), Some("rt-live"));

            // Only stale sessions -> fallback (refresh surfaces the relogin).
            let mut stale_only = serde_json::Map::new();
            stale_only.insert(
                "stale::c".into(),
                serde_json::json!({"key": "stale-key-value", "expires_at": near_past}),
            );
            let picked = pick_grok_session(&stale_only).expect("stale fallback");
            assert_eq!(picked.access_token, "stale-key-value");
            assert!(crate::auth::oauth_expired(picked.expires_at));

            // Empty keys are skipped entirely.
            let mut empty = serde_json::Map::new();
            empty.insert("e::c".into(), serde_json::json!({"key": ""}));
            assert!(pick_grok_session(&empty).is_none());
        }
    }
}

// ── Nous Portal (RFC 8628 device flow, same first-party client as Hermes) ──

/// Nous Portal — Nous Research's unified inference gateway (300+ models incl.
/// free tiers). Device-code OAuth identical to Hermes Agent's `hermes auth add
/// nous`: same portal host, same `hermes-cli` client id, `inference:invoke`
/// scope. The resulting access token is a short-lived invoke JWT used as a
/// plain Bearer on the OpenAI-compatible inference API; the refresh token
/// rotates on every refresh (OAuth 2.1 rotation — always persist the newest).
///
/// Cross-CLI interop: when Hermes Agent is installed, its `~/.hermes/auth.json`
/// `providers.nous` state is imported first (shared token store semantics —
/// one Portal login covers both agents), so `nur auth login --provider nous`
/// is instant for existing Hermes users.
pub mod nous {
    use super::*;

    pub const PORTAL: &str = crate::providers::NOUS_PORTAL_URL;
    pub const INFERENCE: &str = crate::providers::NOUS_PORTAL_BASE_URL;
    pub const CLIENT_ID: &str = crate::providers::NOUS_OAUTH_CLIENT_ID;
    pub const SCOPE: &str = "inference:invoke";

    #[derive(Deserialize)]
    struct DeviceCodeResp {
        device_code: String,
        user_code: String,
        verification_uri: Option<String>,
        verification_uri_complete: Option<String>,
        #[serde(default)]
        expires_in: u64,
        #[serde(default = "default_interval")]
        interval: u64,
    }
    fn default_interval() -> u64 {
        1
    }

    #[derive(Deserialize)]
    struct TokenResp {
        access_token: Option<String>,
        refresh_token: Option<String>,
        expires_in: Option<u64>,
        scope: Option<String>,
        error: Option<String>,
        #[serde(default)]
        error_description: Option<String>,
    }

    fn token_from(payload: TokenResp, client_id: &str) -> Result<OAuthTokens> {
        let access = payload
            .access_token
            .filter(|t| !t.trim().is_empty())
            .ok_or_else(|| NurError::Other("Nous token response missing access_token".into()))?;
        Ok(OAuthTokens {
            access_token: access,
            refresh_token: payload.refresh_token,
            expires_at: expires_in_to_at(payload.expires_in),
            meta: Some(OauthMeta {
                issuer: PORTAL.into(),
                client_id: client_id.into(),
                extra: serde_json::json!({
                    "inference_base_url": INFERENCE,
                    "scope": payload.scope.unwrap_or_else(|| SCOPE.into()),
                }),
            }),
        })
    }

    /// Import an existing Hermes Agent Portal session (`~/.hermes/auth.json`
    /// → `providers.nous`). Hermes stores `access_token` (invoke JWT) +
    /// rotating `refresh_token` under that key; both CLIs share the same
    /// Portal client, so the credential is directly usable here.
    pub fn import_hermes_cli() -> Result<Option<OAuthTokens>> {
        let Some(home) = dirs::home_dir() else {
            return Ok(None);
        };
        let path = home.join(".hermes").join("auth.json");
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Ok(None);
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
            return Ok(None);
        };
        let state = v.pointer("/providers/nous").cloned().unwrap_or_default();
        let refresh = state
            .get("refresh_token")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let access = state
            .get("access_token")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if refresh.is_empty() && access.is_empty() {
            return Ok(None);
        }
        // Quarantined / expired states carry an `error` marker. Keep reading
        // the tokens (a live refresh token can still recover), but surface the
        // marker so `login` does not short-circuit on a provably dead session.
        let error_marker = state
            .get("error")
            .map(|e| !e.is_null() && e.as_str() != Some(""))
            .unwrap_or(false);
        let expires_at = state
            .get("expires_at")
            .and_then(|x| x.as_str())
            .and_then(parse_iso_to_unix);
        Ok(Some(OAuthTokens {
            access_token: access,
            refresh_token: if refresh.is_empty() {
                None
            } else {
                Some(refresh)
            },
            expires_at,
            meta: Some(OauthMeta {
                issuer: PORTAL.into(),
                client_id: CLIENT_ID.into(),
                extra: serde_json::json!({
                    "inference_base_url": INFERENCE,
                    "source": "hermes-cli",
                    "hermes_error": error_marker,
                }),
            }),
        }))
    }

    fn parse_iso_to_unix(s: &str) -> Option<u64> {
        // `2026-08-21T17:00:00Z` (or with offset) → unix secs. chrono is already
        // a dependency; parse leniently and fail soft.
        chrono::DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|dt| dt.timestamp().max(0) as u64)
    }

    pub fn login(tx: &ProgressTx, cancel: &CancelFlag) -> Result<OAuthTokens> {
        // Hermes installed with a live Portal session? One import, no browser.
        // A provably DEAD session (expired, or quarantined with an `error`
        // marker) must fall through to the device flow — importing it anyway
        // pinned /login into a loop: 401 on use, refresh says "run /login",
        // and /login re-imported the same dead file forever.
        match import_hermes_cli() {
            Ok(Some(t)) if !t.access_token.is_empty() || t.refresh_token.is_some() => {
                let dead = t.meta.as_ref().is_some_and(|m| {
                    m.extra
                        .get("hermes_error")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                }) || expired_jwt(&t.access_token)
                    || crate::auth::oauth_expired(t.expires_at);
                if !dead {
                    send(
                        tx,
                        BrowserLoginProgress::Status(
                            "imported Nous Portal session from Hermes Agent (~/.hermes/auth.json)"
                                .into(),
                        ),
                    );
                    return Ok(t);
                }
                send(
                    tx,
                    BrowserLoginProgress::Status(
                        "Hermes Nous session is expired or quarantined — starting the Portal device login instead…"
                            .into(),
                    ),
                );
            }
            _ => {}
        }

        send(
            tx,
            BrowserLoginProgress::Status("requesting Nous Portal device code…".into()),
        );
        let client = http()?;
        let res = client
            .post(format!("{PORTAL}/api/oauth/device/code"))
            .form(&[("client_id", CLIENT_ID), ("scope", SCOPE)])
            .send()
            .map_err(|e| NurError::Other(format!("nous device code: {e}")))?;
        let status = res.status();
        let body = res.text().unwrap_or_default();
        if !status.is_success() {
            return Err(NurError::Other(format!(
                "Nous device code failed ({status}: {}). Portal login page: {PORTAL}/login",
                oauth_error_summary(&body)
            )));
        }
        let device: DeviceCodeResp = serde_json::from_str(&body)
            .map_err(|e| NurError::Other(format!("nous device code parse: {e} — body withheld")))?;

        let verify = device
            .verification_uri_complete
            .clone()
            .filter(|u| !u.trim().is_empty())
            .unwrap_or_else(|| {
                device
                    .verification_uri
                    .clone()
                    .unwrap_or_else(|| format!("{PORTAL}/login"))
            });

        send(
            tx,
            BrowserLoginProgress::DeviceCode {
                verification_url: verify.clone(),
                user_code: device.user_code.clone(),
            },
        );
        let _ = open_browser(&verify);

        let deadline = std::time::Instant::now()
            + Duration::from_secs(if device.expires_in > 0 {
                device.expires_in
            } else {
                900
            });
        // Server asks for >=1s; device_poll_sleep floors at 3s, so polling is
        // always slower than the Portal cap (RFC 8628-compliant and safe).
        let base_interval = device.interval.max(3);
        let mut poll = super::super::DevicePoll::new(base_interval);
        while std::time::Instant::now() < deadline {
            if cancel.is_cancelled() {
                return Err(NurError::Other("login cancelled".into()));
            }
            poll.wait(cancel, deadline)?;
            let res = client
                .post(format!("{PORTAL}/api/oauth/token"))
                .form(&[
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ("client_id", CLIENT_ID),
                    ("device_code", device.device_code.as_str()),
                ])
                .send();
            let Ok(res) = res else { continue };
            let parsed: TokenResp = serde_json::from_str(&res.text().unwrap_or_default())
                .unwrap_or(TokenResp {
                    access_token: None,
                    refresh_token: None,
                    expires_in: None,
                    scope: None,
                    error: Some("parse".into()),
                    error_description: None,
                });
            match parsed.error.as_deref() {
                None | Some("") | Some("parse") => {}
                Some("authorization_pending") => {
                    send(
                        tx,
                        BrowserLoginProgress::Status("waiting for Portal approval…".into()),
                    );
                    continue;
                }
                Some("slow_down") => {
                    poll.slow_down();
                    continue;
                }
                Some(err) => {
                    return Err(NurError::Other(format!(
                        "Nous token error: {err} {} — finish signing in at {PORTAL}/login then retry",
                        parsed.error_description.clone().unwrap_or_default()
                    )));
                }
            }
            if parsed.access_token.is_some() {
                return token_from(parsed, CLIENT_ID);
            }
        }
        Err(NurError::Other(
            "Nous device login timed out. Sign in at portal.nousresearch.com/login in a normal \
             browser tab (CAPTCHA loops are the usual cause), then retry /login."
                .into(),
        ))
    }

    /// Refresh: POST {portal}/api/oauth/token with the `x-nous-refresh-token`
    /// header (Hermes's wire shape). The refresh token ROTATES — callers must
    /// persist the returned one.
    pub fn refresh(_auth: &Auth, refresh_token: &str) -> Result<OAuthTokens> {
        let client = http()?;
        let res = client
            .post(format!("{PORTAL}/api/oauth/token"))
            .header("x-nous-refresh-token", refresh_token)
            .form(&[("grant_type", "refresh_token"), ("client_id", CLIENT_ID)])
            .send()
            .map_err(|e| NurError::Other(format!("nous refresh: {e}")))?;
        let status = res.status();
        let body = res.text().unwrap_or_default();
        if status.as_u16() == 401 || status.as_u16() == 400 {
            // invalid_grant / refresh_token_reused → relogin required.
            return Err(NurError::Other(format!(
                "Nous Portal session expired (refresh rejected, {status}). Run \
                 /login → Nous Portal again (or `hermes auth add nous`); the old token \
                 was rotated or revoked."
            )));
        }
        if !status.is_success() {
            return Err(NurError::Other(format!(
                "Nous refresh failed ({status}: {})",
                oauth_error_summary(&body)
            )));
        }
        let parsed: TokenResp = serde_json::from_str(&body)
            .map_err(|e| NurError::Other(format!("nous refresh parse: {e}")))?;
        token_from(parsed, CLIENT_ID)
    }
}

// ── Command Code (studio browser sign-in / `cmd` CLI auth.json import) ──────

/// Command Code's login is the same first-party flow its own CLI drives, so
/// nur runs it directly — no `cmd` binary needed. Sequence (mirrors
/// `command-code`'s `createAuthServer` / `runBrowserAuth`, verified against the
/// shipped bundle):
///   1. local server on `127.0.0.1` (ephemeral port), random base64url state;
///   2. open `{studio}/studio/auth/cli?callback=<loopback>&state=<state>&mode=redirect`;
///   3. after "Authorize", the studio page POSTs form-urlencoded (or legacy
///      JSON) `{apiKey, state, userId, userName, keyName}` to the callback —
///      or `{error, error_description, state}` on denial;
///   4. the credential is a long-lived API key: same bearer the Provider API
///      documents. The `cmd` CLI stores it at `~/.commandcode/auth.json`.
pub mod commandcode {
    use super::*;

    pub const STUDIO: &str = "https://commandcode.ai";
    pub const CALLBACK_PATH: &str = "/callback";
    /// Browser login window. The vendor CLI caps at 2 minutes; nur allows a
    /// slower hand-off (copying the URL into another browser) to still finish.
    const LOGIN_TIMEOUT: Duration = Duration::from_secs(600);
    /// Origins the studio page may POST from (mirrors the CLI's allowlist).
    const ALLOWED_ORIGINS: &[&str] = &[
        "https://commandcode.ai",
        "https://staging.commandcode.ai",
        "http://localhost:3000",
    ];

    /// Credentials the studio page delivers on success.
    #[derive(Debug, Clone)]
    struct CommandCreds {
        api_key: String,
        user_id: String,
        user_name: String,
        key_name: String,
    }

    fn auth_json_paths() -> Vec<PathBuf> {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let mut out = Vec::new();
        if let Ok(dir) = std::env::var("COMMANDCODE_HOME") {
            out.push(PathBuf::from(dir).join("auth.json"));
        }
        out.push(home.join(".commandcode").join("auth.json"));
        out
    }

    fn tokens_from_key(api_key: &str, extra: serde_json::Value) -> OAuthTokens {
        OAuthTokens {
            access_token: api_key.trim().to_string(),
            // Marker consumed by `refresh` below: a Command Code credential is
            // a long-lived Studio API key with nothing to rotate, so a
            // mid-session 401 just re-reads the on-disk CLI session.
            refresh_token: Some("commandcode".into()),
            expires_at: None,
            meta: Some(OauthMeta {
                issuer: STUDIO.into(),
                client_id: "cmd-cli".into(),
                extra,
            }),
        }
    }

    /// Import an existing `cmd login` session (`~/.commandcode/auth.json`).
    pub fn import_commandcode_cli() -> Result<Option<OAuthTokens>> {
        for var in ["COMMAND_CODE_API_KEY", "CMD_API_KEY"] {
            if let Ok(key) = std::env::var(var) {
                let key = key.trim();
                if !key.is_empty() {
                    return Ok(Some(tokens_from_key(
                        key,
                        serde_json::json!({ "imported_from": var }),
                    )));
                }
            }
        }
        for p in auth_json_paths() {
            if !p.is_file() {
                continue;
            }
            if let Some(tokens) = import_commandcode_auth_file(&p) {
                return Ok(Some(tokens));
            }
        }
        Ok(None)
    }

    /// Parse one `auth.json` (`{apiKey, userId, userName, keyName, …}`).
    /// Exposed for tests; returns `None` for missing/empty keys.
    fn import_commandcode_auth_file(p: &std::path::Path) -> Option<OAuthTokens> {
        let text = std::fs::read_to_string(p).ok()?;
        let v = serde_json::from_str::<serde_json::Value>(&text).ok()?;
        let key = v
            .get("apiKey")
            .or_else(|| v.get("api_key"))
            .and_then(|x| x.as_str())?;
        if key.trim().is_empty() {
            return None;
        }
        Some(tokens_from_key(
            key,
            serde_json::json!({
                "imported_from": "cmd-cli",
                "path": p.display().to_string(),
                "user_id": v.get("userId").and_then(|x| x.as_str()).unwrap_or(""),
                "user_name": v.get("userName").and_then(|x| x.as_str()).unwrap_or(""),
                "key_name": v.get("keyName").and_then(|x| x.as_str()).unwrap_or(""),
            }),
        ))
    }

    pub fn login(tx: &ProgressTx, cancel: &CancelFlag) -> Result<OAuthTokens> {
        // A `cmd login` on this machine already issued a usable key.
        if let Ok(Some(t)) = import_commandcode_cli() {
            send(
                tx,
                BrowserLoginProgress::Status(
                    "using existing Command Code CLI session (~/.commandcode/auth.json)".into(),
                ),
            );
            return Ok(t);
        }

        let listener = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|e| NurError::Other(format!("failed to bind loopback: {e}")))?;
        let port = listener
            .local_addr()
            .map(|a| a.port())
            .map_err(|e| NurError::Other(format!("loopback port: {e}")))?;
        let state = random_urlsafe(32);
        let callback = format!("http://127.0.0.1:{port}{CALLBACK_PATH}");
        let auth_url = format!(
            "{STUDIO}/studio/auth/cli?callback={}&state={}&mode=redirect",
            urlencoding_encode(&callback),
            urlencoding_encode(&state),
        );

        send(tx, BrowserLoginProgress::OpenUrl(auth_url.clone()));
        send(
            tx,
            BrowserLoginProgress::Status("complete Command Code sign-in in the browser…".into()),
        );
        let _ = open_browser(&auth_url);

        let creds = wait_commandcode_credentials_on(listener, &state, cancel, LOGIN_TIMEOUT)?;
        send(
            tx,
            BrowserLoginProgress::Status(format!(
                "signed in as {} (key: {})",
                if creds.user_name.is_empty() {
                    "Command Code user"
                } else {
                    &creds.user_name
                },
                if creds.key_name.is_empty() {
                    "unnamed"
                } else {
                    &creds.key_name
                }
            )),
        );
        Ok(tokens_from_key(
            &creds.api_key,
            serde_json::json!({
                "imported_from": "nur-browser",
                "user_id": creds.user_id,
                "user_name": creds.user_name,
                "key_name": creds.key_name,
            }),
        ))
    }

    /// The credential is a long-lived Studio API key — nothing to rotate. A
    /// refresh attempt just re-reads the latest CLI session / env key.
    pub fn refresh(_auth: &Auth, _refresh: &str) -> Result<OAuthTokens> {
        import_commandcode_cli()?.ok_or_else(|| {
            NurError::Other(
                "Command Code session missing. Run /login → Command Code again, or `cmd login`, \
                 or paste COMMAND_CODE_API_KEY."
                    .into(),
            )
        })
    }

    // ── loopback callback server ─────────────────────────────────────────────

    fn html_escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }

    fn html_page(heading: &str, detail: &str) -> String {
        format!(
            "<!DOCTYPE html><html><head><title>{heading} - Command Code</title></head>\
             <body style=\"font-family:system-ui;padding:40px;text-align:center;\">\
             <h1>{heading}</h1><p>{detail}</p></body></html>"
        )
    }

    /// Minimal form/percent decoding: `+` → space, `%XX` → byte (UTF-8 pass-through).
    fn form_urldecode(s: &str) -> String {
        let bytes = s.replace('+', " ").into_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                if let Ok(b) = u8::from_str_radix(hex, 16) {
                    out.push(b);
                    i += 3;
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    fn parse_form_pairs(body: &str) -> Vec<(String, String)> {
        body.split('&')
            .filter(|p| !p.is_empty())
            .map(|pair| {
                let mut it = pair.splitn(2, '=');
                (
                    form_urldecode(it.next().unwrap_or("")),
                    form_urldecode(it.next().unwrap_or("")),
                )
            })
            .collect()
    }

    fn field<'a>(pairs: &'a [(String, String)], name: &str) -> Option<&'a str> {
        pairs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
            .filter(|v| !v.is_empty())
    }

    fn creds_from_pairs(pairs: &[(String, String)]) -> Option<CommandCreds> {
        Some(CommandCreds {
            api_key: field(pairs, "apiKey")?.to_string(),
            user_id: field(pairs, "userId").unwrap_or("").to_string(),
            user_name: field(pairs, "userName").unwrap_or("").to_string(),
            key_name: field(pairs, "keyName").unwrap_or("").to_string(),
        })
    }

    fn write_response(
        stream: &mut std::net::TcpStream,
        status: &str,
        content_type: &str,
        body: &str,
        origin: Option<&str>,
        private_network: bool,
    ) {
        let mut res = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nCache-Control: no-store\r\n\
             Connection: close\r\nContent-Length: {}\r\n",
            body.len()
        );
        if let Some(origin) = origin {
            res.push_str(&format!("Access-Control-Allow-Origin: {origin}\r\n"));
            res.push_str("Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n");
            res.push_str("Access-Control-Allow-Headers: Content-Type\r\n");
            if private_network {
                // Chrome Local Network Access: the studio page POSTs to a
                // loopback server and must pass the preflight for it.
                res.push_str("Access-Control-Allow-Private-Network: true\r\n");
            }
        }
        res.push_str("\r\n");
        res.push_str(body);
        let _ = stream.write_all(res.as_bytes());
        let _ = stream.flush();
    }

    fn allowed_origin(origin: Option<&str>) -> Option<&'static str> {
        origin.and_then(|o| {
            ALLOWED_ORIGINS
                .iter()
                .find(|allowed| allowed.eq_ignore_ascii_case(o))
                .copied()
        })
    }

    /// One accepted connection → optional credentials or a denial error.
    /// Everything else (404s, probes, state mismatches) returns None and the
    /// loop keeps waiting.
    #[derive(Debug)]
    enum CallbackHit {
        None,
        Creds(CommandCreds),
        Denied(String),
    }

    fn handle_connection(stream: &mut std::net::TcpStream, expected_state: &str) -> CallbackHit {
        // Read to end of headers, then content-length body bytes.
        let mut buf: Vec<u8> = Vec::with_capacity(4096);
        let mut chunk = [0u8; 4096];
        let head_end = loop {
            match stream.read(&mut chunk) {
                Ok(0) => return CallbackHit::None,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(_) => return CallbackHit::None,
            }
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break pos + 4;
            }
            if buf.len() > 64 * 1024 {
                return CallbackHit::None;
            }
        };
        let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
        let mut lines = head.lines();
        let request_line = lines.next().unwrap_or("");
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or("").to_ascii_uppercase();
        let target = parts.next().unwrap_or("/").to_string();
        let origin = head
            .lines()
            .find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.eq_ignore_ascii_case("origin")
                    .then(|| v.trim().to_string())
            })
            .filter(|v| !v.is_empty());
        let wants_private_network = head
            .to_ascii_lowercase()
            .contains("access-control-request-private-network: true");
        let cors = allowed_origin(origin.as_deref());

        if method == "OPTIONS" {
            write_response(
                stream,
                "204 No Content",
                "text/plain",
                "",
                cors,
                wants_private_network,
            );
            return CallbackHit::None;
        }

        let (path, query) = match target.split_once('?') {
            Some((p, q)) => (p.to_string(), Some(q.to_string())),
            None => (target.clone(), None),
        };
        if path != CALLBACK_PATH {
            write_response(
                stream,
                "404 Not Found",
                "text/html",
                "Not Found",
                cors,
                false,
            );
            return CallbackHit::None;
        }

        // Assemble the field set: POST body (form or legacy JSON), with a
        // lenient GET-query fallback.
        let content_length = head
            .lines()
            .find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.eq_ignore_ascii_case("content-length")
                    .then(|| v.trim().parse::<usize>().ok())?
            })
            .unwrap_or(0)
            .min(64 * 1024);
        while buf.len() < head_end + content_length {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(_) => break,
            }
        }
        let body_bytes = buf[head_end.min(buf.len())..].to_vec();
        let content_type = head
            .to_ascii_lowercase()
            .contains("application/json")
            .then_some("json");
        // Field sources, first match wins:
        //   1. POST body — form-urlencoded, or the legacy JSON shape (its
        //      `state`/`error`/`error_description` fields carry the state
        //      check and denial reason, exactly like the form shape);
        //   2. the callback query string (lenient GET fallback).
        let body_pairs: Vec<(String, String)> = if content_type == Some("json") {
            serde_json::from_slice::<serde_json::Value>(&body_bytes)
                .ok()
                .and_then(|v| {
                    v.as_object().map(|o| {
                        o.iter()
                            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                            .collect()
                    })
                })
                .unwrap_or_default()
        } else {
            parse_form_pairs(&String::from_utf8_lossy(&body_bytes))
        };
        let query_pairs: Vec<(String, String)> =
            query.as_deref().map(parse_form_pairs).unwrap_or_default();
        let lookup = |name: &str| -> Option<String> {
            field(&body_pairs, name)
                .or_else(|| field(&query_pairs, name))
                .map(str::to_string)
        };
        let delivered = creds_from_pairs(&body_pairs).or_else(|| creds_from_pairs(&query_pairs));
        let error_field = lookup("error");
        let error_description = lookup("error_description");
        let delivered_state = lookup("state");

        let Some(delivered_state) = delivered_state else {
            write_response(
                stream,
                "400 Bad Request",
                "text/html; charset=utf-8",
                &html_page("Invalid Request", "No credential payload received."),
                cors,
                false,
            );
            return CallbackHit::None;
        };
        if delivered_state != expected_state {
            write_response(
                stream,
                "403 Forbidden",
                "text/html; charset=utf-8",
                &html_page(
                    "Authentication Failed",
                    "Invalid state parameter. Please try again.",
                ),
                cors,
                false,
            );
            return CallbackHit::None;
        }
        if let Some(err) = error_field {
            let detail = error_description.unwrap_or(err.clone());
            let heading = if err == "access_denied" {
                "Authorization Denied"
            } else {
                "Authentication Failed"
            };
            write_response(
                stream,
                "200 OK",
                "text/html; charset=utf-8",
                &html_page(heading, &html_escape(&detail)),
                cors,
                false,
            );
            return CallbackHit::Denied(detail);
        }
        let Some(creds) = delivered else {
            write_response(
                stream,
                "400 Bad Request",
                "text/html; charset=utf-8",
                &html_page(
                    "Invalid Request",
                    "No authorization payload received. Please try again.",
                ),
                cors,
                false,
            );
            return CallbackHit::None;
        };
        let user = html_escape(if creds.user_name.is_empty() {
            "Command Code user"
        } else {
            &creds.user_name
        });
        write_response(
            stream,
            "200 OK",
            "text/html; charset=utf-8",
            &html_page(
                "Sign in successful",
                &format!(
                    "Signed in as <strong>{user}</strong>.<br />Close this tab and return to nur."
                ),
            ),
            cors,
            false,
        );
        CallbackHit::Creds(creds)
    }

    fn wait_commandcode_credentials_on(
        listener: TcpListener,
        expected_state: &str,
        cancel: &CancelFlag,
        timeout: Duration,
    ) -> Result<CommandCreds> {
        listener
            .set_nonblocking(true)
            .map_err(|e| NurError::Other(e.to_string()))?;
        // Each connection is handled on its own thread: a client that drips
        // bytes to dodge the per-read timeout used to be able to stall the
        // single accept loop past cancel and the login deadline.
        let (hit_tx, hit_rx) = mpsc::channel::<CallbackHit>();
        let start = std::time::Instant::now();
        loop {
            if cancel.is_cancelled() {
                return Err(NurError::Other("login cancelled".into()));
            }
            if start.elapsed() > timeout {
                return Err(NurError::Other(
                    "Command Code browser login timed out — reopen /login and finish the \
                     sign-in within 10 minutes."
                        .into(),
                ));
            }
            match listener.accept() {
                Ok((stream, _)) => {
                    let tx = hit_tx.clone();
                    let state = expected_state.to_string();
                    thread::spawn(move || {
                        let mut stream = stream;
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
                        let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
                        let _ = tx.send(handle_connection(&mut stream, &state));
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => {}
            }
            match hit_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(CallbackHit::Creds(creds)) => return Ok(creds),
                Ok(CallbackHit::Denied(detail)) => {
                    return Err(NurError::Other(format!(
                        "Command Code sign-in was denied: {detail}"
                    )))
                }
                Ok(CallbackHit::None) => {}
                Err(_) => {}
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
            list.iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect()
        }

        #[test]
        fn form_decode_handles_percent_and_plus() {
            assert_eq!(form_urldecode("a+b"), "a b");
            assert_eq!(form_urldecode("kimi%20k3"), "kimi k3");
            assert_eq!(form_urldecode("100%25"), "100%");
            assert_eq!(form_urldecode("plain"), "plain");
        }

        #[test]
        fn creds_require_the_api_key_field() {
            let full = pairs(&[
                ("apiKey", "cc_sk_123"),
                ("state", "s1"),
                ("userId", "u1"),
                ("userName", "david"),
                ("keyName", "nur"),
            ]);
            let creds = creds_from_pairs(&full).expect("full payload parses");
            assert_eq!(creds.api_key, "cc_sk_123");
            assert_eq!(creds.user_name, "david");
            let missing = pairs(&[("state", "s1"), ("userName", "david")]);
            assert!(creds_from_pairs(&missing).is_none());
        }

        /// The legacy JSON shape must parse through the same pair pipeline the
        /// form path uses (state included) - it used to be dropped entirely.
        #[test]
        fn legacy_json_payload_parses() {
            let body =
                br#"{"apiKey":"cc_sk_9","state":"s","userId":"u","userName":"n","keyName":"k"}"#;
            let v: serde_json::Value = serde_json::from_slice(body).unwrap();
            let pairs: Vec<(String, String)> = v
                .as_object()
                .map(|o| {
                    o.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect()
                })
                .unwrap_or_default();
            let creds = creds_from_pairs(&pairs).expect("json payload");
            assert_eq!(creds.api_key, "cc_sk_9");
            assert_eq!(creds.key_name, "k");
            assert_eq!(field(&pairs, "state"), Some("s"));
        }

        /// Socket-level: a legacy-JSON POST carrying `state` in the BODY must
        /// be accepted - body-borne state used to be invisible, hanging the
        /// login until timeout.
        #[test]
        fn json_post_with_body_state_delivers_credentials() {
            let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            let server = thread::spawn(move || {
                listener.set_nonblocking(false).unwrap();
                let (mut stream, _) = listener.accept().unwrap();
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                handle_connection(&mut stream, "json-state")
            });
            let mut client = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            let body = br#"{"apiKey":"cc_sk_js","state":"json-state","userId":"u","userName":"n","keyName":"k"}"#;
            let req = format!(
                "POST /callback HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            client.write_all(req.as_bytes()).unwrap();
            client.write_all(body.as_slice()).unwrap();
            let mut resp = String::new();
            let _ = client.read_to_string(&mut resp);
            assert!(resp.starts_with("HTTP/1.1 200"), "{resp}");
            match server.join().unwrap() {
                CallbackHit::Creds(creds) => assert_eq!(creds.api_key, "cc_sk_js"),
                other => panic!("expected credentials, got {other:?}"),
            }
        }

        /// Socket-level: a JSON denial (error + state in body) must surface as
        /// Denied instead of spinning to the timeout.
        #[test]
        fn json_denial_fails_fast() {
            let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            let server = thread::spawn(move || {
                listener.set_nonblocking(false).unwrap();
                let (mut stream, _) = listener.accept().unwrap();
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                handle_connection(&mut stream, "s2")
            });
            let mut client = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            let body = br#"{"error":"access_denied","error_description":"user clicked deny","state":"s2"}"#;
            let req = format!(
                "POST /callback HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            client.write_all(req.as_bytes()).unwrap();
            client.write_all(body.as_slice()).unwrap();
            let mut resp = String::new();
            let _ = client.read_to_string(&mut resp);
            assert!(resp.contains("200 OK"), "{resp}");
            match server.join().unwrap() {
                CallbackHit::Denied(detail) => assert!(detail.contains("deny"), "{detail}"),
                other => panic!("expected denial, got {other:?}"),
            }
        }

        /// A state mismatch must be rejected before any credential is accepted.
        #[test]
        fn state_mismatch_yields_no_credentials() {
            let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            let server = thread::spawn(move || {
                listener.set_nonblocking(false).unwrap();
                let (mut stream, _) = listener.accept().unwrap();
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                matches!(
                    handle_connection(&mut stream, "expected"),
                    CallbackHit::None
                )
            });
            let mut client = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            let body = "apiKey=cc_sk_evil&state=wrong&userId=u&userName=n&keyName=k";
            let req = format!(
                "POST /callback HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: \
                 application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            client.write_all(req.as_bytes()).unwrap();
            let mut resp = String::new();
            let _ = client.read_to_string(&mut resp);
            assert!(resp.starts_with("HTTP/1.1 403"), "{resp}");
            assert!(
                server.join().unwrap(),
                "mismatched state must not yield creds"
            );
        }

        #[test]
        fn options_preflight_gets_cors_and_private_network_headers() {
            let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            let server = thread::spawn(move || {
                listener.set_nonblocking(false).unwrap();
                let (mut stream, _) = listener.accept().unwrap();
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                handle_connection(&mut stream, "s")
            });
            let mut client = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            let req = "OPTIONS /callback HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: \
                       https://commandcode.ai\r\nAccess-Control-Request-Private-Network: true\r\n\r\n";
            client.write_all(req.as_bytes()).unwrap();
            let mut resp = String::new();
            let _ = client.read_to_string(&mut resp);
            assert!(resp.starts_with("HTTP/1.1 204"), "{resp}");
            assert!(resp.contains("Access-Control-Allow-Origin: https://commandcode.ai"));
            assert!(resp.contains("Access-Control-Allow-Private-Network: true"));
            assert!(matches!(server.join().unwrap(), CallbackHit::None));
        }

        /// The full happy path: a studio-style POST delivers credentials.
        #[test]
        fn form_post_delivers_credentials() {
            let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            let server = thread::spawn(move || {
                listener.set_nonblocking(false).unwrap();
                let (mut stream, _) = listener.accept().unwrap();
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                handle_connection(&mut stream, "good-state")
            });
            let mut client = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            let body = "apiKey=cc_sk_ok&state=good-state&userId=u1&userName=David&keyName=nur";
            let req = format!(
                "POST /callback HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: \
                 https://commandcode.ai\r\nContent-Type: \
                 application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            client.write_all(req.as_bytes()).unwrap();
            let mut resp = Vec::new();
            let _ = client.read_to_end(&mut resp);
            let resp = String::from_utf8_lossy(&resp).into_owned();
            assert!(resp.starts_with("HTTP/1.1 200"), "{resp}");
            assert!(resp.contains("Sign in successful"), "{resp}");
            match server.join().unwrap() {
                CallbackHit::Creds(creds) => {
                    assert_eq!(creds.api_key, "cc_sk_ok");
                    assert_eq!(creds.user_name, "David");
                }
                other => panic!("expected credentials, got {other:?}"),
            }
        }

        #[test]
        fn import_reads_the_cmd_cli_auth_json() {
            let dir = std::env::temp_dir().join(format!("nur-cmd-test-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("auth.json");
            std::fs::write(
                &path,
                r#"{"apiKey":"cc_sk_cli","userId":"u9","userName":"cli-user","keyName":"default","authenticatedAt":"2026-09-10T00:00:00Z"}"#,
            )
            .unwrap();
            let tokens = import_commandcode_auth_file(&path).expect("cli session");
            assert_eq!(tokens.access_token, "cc_sk_cli");
            let extra = tokens.meta.unwrap().extra;
            assert_eq!(extra["user_name"], "cli-user");
            // An empty or missing key must not import.
            std::fs::write(&path, r#"{"userId":"u9"}"#).unwrap();
            assert!(import_commandcode_auth_file(&path).is_none());
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

// ── Kimi Code (RFC 8628 device authorization / Kimi CLI import) ────────────

// Kimi uses the same managed bearer for model discovery and inference.
pub mod kimi {
    use super::*;
    use reqwest::blocking::RequestBuilder;

    /// Public client used by the first-party Kimi Code CLI. No secret is used.
    pub const CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";
    pub const ISSUER: &str = "https://auth.kimi.com";

    #[derive(Deserialize)]
    struct DeviceCodeResp {
        device_code: String,
        user_code: String,
        #[serde(default)]
        verification_uri: String,
        #[serde(default)]
        verification_uri_complete: String,
        #[serde(default)]
        expires_in: u64,
        #[serde(default = "default_interval")]
        interval: u64,
    }

    fn default_interval() -> u64 {
        5
    }

    #[derive(Deserialize)]
    struct TokenResp {
        access_token: Option<String>,
        refresh_token: Option<String>,
        expires_in: Option<u64>,
        #[serde(default)]
        scope: String,
        #[serde(default)]
        token_type: String,
        error: Option<String>,
        error_description: Option<String>,
    }

    fn oauth_host() -> String {
        ["KIMI_CODE_OAUTH_HOST", "KIMI_OAUTH_HOST"]
            .into_iter()
            .find_map(|name| {
                std::env::var(name)
                    .ok()
                    .map(|value| value.trim().trim_end_matches('/').to_string())
                    .filter(|value| !value.is_empty() && value.starts_with("https://"))
            })
            .unwrap_or_else(|| ISSUER.to_string())
    }

    /// `None` when neither KIMI_SHARE_DIR nor a home dir is available - the
    /// caller skips rather than trusting a CWD-relative credential store.
    fn kimi_share_dir() -> Option<PathBuf> {
        std::env::var("KIMI_SHARE_DIR")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|home| home.join(".kimi")))
    }

    fn device_id() -> Result<String> {
        // Reuse the first-party CLI identity when present. Otherwise keep a
        // Nur-specific stable id so polls and refreshes describe one device.
        let kimi_path = kimi_share_dir()
            .ok_or_else(|| NurError::Other("no home directory for Kimi credentials".into()))?
            .join("device_id");
        if let Ok(value) = std::fs::read_to_string(&kimi_path) {
            let value = value.trim();
            if let Ok(id) = Uuid::parse_str(value) {
                return Ok(id.to_string());
            }
        }
        let path = crate::config::nur_home().join("kimi_device_id");
        if let Ok(value) = std::fs::read_to_string(&path) {
            let value = value.trim();
            if let Ok(id) = Uuid::parse_str(value) {
                return Ok(id.to_string());
            }
        }
        let value = Uuid::new_v4().to_string();
        crate::config::atomic_write(&path, value.as_bytes())
            .map_err(|e| NurError::Other(format!("failed to save Kimi device id: {e}")))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(value)
    }

    /// Device identity headers required by Kimi's managed OAuth API for token,
    /// model, and inference requests.
    pub fn request_headers() -> Result<Vec<(&'static str, String)>> {
        let device_name = std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| "unknown".to_string());
        let device_model = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
        let os_version = std::env::var("OS").unwrap_or_else(|_| std::env::consts::OS.to_string());
        let ascii = |value: String| {
            value
                .chars()
                .take(256)
                .map(|ch| {
                    if ch.is_ascii_graphic() || ch == ' ' {
                        ch
                    } else {
                        '_'
                    }
                })
                .collect::<String>()
        };
        Ok(vec![
            // Kimi's managed API fingerprints the first-party CLI; a bare nur
            // semver can 402 model listing. Prefer a kimi_cli-shaped version.
            ("X-Msh-Platform", "kimi_cli".to_string()),
            (
                "X-Msh-Version",
                std::env::var("NUR_KIMI_CLI_VERSION")
                    .ok()
                    .filter(|v| !v.trim().is_empty())
                    .unwrap_or_else(|| "0.79.0".into()),
            ),
            ("X-Msh-Device-Name", ascii(device_name)),
            ("X-Msh-Device-Model", ascii(device_model)),
            ("X-Msh-Os-Version", ascii(os_version)),
            ("X-Msh-Device-Id", device_id()?),
        ])
    }

    fn with_device_headers(mut req: RequestBuilder) -> Result<RequestBuilder> {
        for (name, value) in request_headers()? {
            req = req.header(name, value);
        }
        Ok(req)
    }

    fn token_error(parsed: &TokenResp, status: u16) -> String {
        parsed
            .error_description
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or(parsed.error.as_deref())
            .map(str::to_string)
            .unwrap_or_else(|| format!("HTTP {status}"))
    }

    fn into_tokens(
        parsed: TokenResp,
        previous_refresh: Option<&str>,
        meta: Option<OauthMeta>,
    ) -> Result<OAuthTokens> {
        let access_token = parsed
            .access_token
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| NurError::Other("Kimi token response omitted access_token".into()))?;
        let refresh_token = parsed
            .refresh_token
            .filter(|value| !value.trim().is_empty())
            .or_else(|| previous_refresh.map(str::to_string));
        Ok(OAuthTokens {
            access_token,
            refresh_token,
            expires_at: expires_in_to_at(parsed.expires_in),
            meta,
        })
    }

    fn meta(extra: serde_json::Value) -> OauthMeta {
        OauthMeta {
            issuer: oauth_host(),
            client_id: CLIENT_ID.into(),
            extra,
        }
    }

    pub fn login(tx: &ProgressTx, cancel: &CancelFlag) -> Result<OAuthTokens> {
        send(
            tx,
            BrowserLoginProgress::Status("requesting Kimi device code…".into()),
        );
        let client = http()?;
        let host = oauth_host();
        let request = with_device_headers(
            client
                .post(format!("{host}/api/oauth/device_authorization"))
                .form(&[("client_id", CLIENT_ID)]),
        )?;
        let response = request
            .send()
            .map_err(|e| NurError::Other(format!("Kimi device authorization failed: {e}")))?;
        let status = response.status();
        let body = response.text().unwrap_or_default();
        if !status.is_success() {
            return Err(NurError::Other(format!(
                "Kimi device authorization failed (HTTP {})",
                status.as_u16()
            )));
        }
        let device: DeviceCodeResp = serde_json::from_str(&body)
            .map_err(|e| NurError::Other(format!("invalid Kimi device response: {e}")))?;
        if device.device_code.trim().is_empty() || device.user_code.trim().is_empty() {
            return Err(NurError::Other(
                "Kimi device response omitted the authorization code".into(),
            ));
        }
        let verification_url = if !device.verification_uri_complete.trim().is_empty() {
            device.verification_uri_complete.clone()
        } else {
            device.verification_uri.clone()
        };
        if verification_url.trim().is_empty() {
            return Err(NurError::Other(
                "Kimi device response omitted the verification URL".into(),
            ));
        }
        send(
            tx,
            BrowserLoginProgress::DeviceCode {
                verification_url: verification_url.clone(),
                user_code: device.user_code.clone(),
            },
        );
        let _ = open_browser(&verification_url);

        let deadline = std::time::Instant::now()
            + Duration::from_secs(if device.expires_in > 0 {
                device.expires_in
            } else {
                900
            });
        let interval = device.interval.max(1);
        let mut poll = super::super::DevicePoll::new(interval);
        while std::time::Instant::now() < deadline {
            if cancel.is_cancelled() {
                return Err(NurError::Other("login cancelled".into()));
            }
            poll.wait(cancel, deadline)?;
            let request =
                with_device_headers(client.post(format!("{host}/api/oauth/token")).form(&[
                    ("client_id", CLIENT_ID),
                    ("device_code", device.device_code.as_str()),
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ]))?;
            let Ok(response) = request.send() else {
                continue;
            };
            let status = response.status().as_u16();
            let body = response.text().unwrap_or_default();
            let Ok(parsed) = serde_json::from_str::<TokenResp>(&body) else {
                if status >= 500 {
                    continue;
                }
                return Err(NurError::Other(format!(
                    "invalid Kimi token response (HTTP {status})"
                )));
            };
            if parsed.access_token.is_some() {
                let extra = serde_json::json!({
                    "scope": parsed.scope,
                    "token_type": parsed.token_type,
                });
                return into_tokens(parsed, None, Some(meta(extra)));
            }
            match parsed.error.as_deref() {
                Some("authorization_pending") | None => {}
                Some("slow_down") => poll.slow_down(),
                Some("expired_token") => {
                    return Err(NurError::Other(
                        "Kimi device code expired; start browser sign-in again".into(),
                    ));
                }
                Some("access_denied") => {
                    return Err(NurError::Other("Kimi authorization was denied".into()));
                }
                Some(_) if status >= 500 || status == 429 => continue,
                Some(_) => {
                    return Err(NurError::Other(format!(
                        "Kimi token error: {}",
                        token_error(&parsed, status)
                    )));
                }
            }
            send(
                tx,
                BrowserLoginProgress::Status("waiting for Kimi browser approval…".into()),
            );
        }
        Err(NurError::Other("Kimi device login timed out".into()))
    }

    pub fn refresh(auth: &Auth, refresh: &str) -> Result<OAuthTokens> {
        let client = http()?;
        let host = oauth_host();
        let request = with_device_headers(client.post(format!("{host}/api/oauth/token")).form(&[
            ("client_id", CLIENT_ID),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh),
        ]))?;
        let response = request
            .send()
            .map_err(|e| NurError::Other(format!("Kimi token refresh failed: {e}")))?;
        let status = response.status().as_u16();
        let body = response.text().unwrap_or_default();
        let parsed: TokenResp = serde_json::from_str(&body).map_err(|_| {
            NurError::Other(format!("invalid Kimi refresh response (HTTP {status})"))
        })?;
        if !(200..300).contains(&status) || parsed.access_token.is_none() {
            return Err(NurError::Other(format!(
                "Kimi token refresh failed: {}",
                token_error(&parsed, status)
            )));
        }
        into_tokens(parsed, Some(refresh), auth.oauth_meta.clone())
    }

    pub fn import_kimi_cli() -> Result<Option<OAuthTokens>> {
        let Some(base) = kimi_share_dir() else {
            return Ok(None);
        };
        let path = base.join("credentials").join("kimi-code.json");
        if !path.exists() {
            return Ok(None);
        }
        let body = std::fs::read_to_string(path)?;
        let value: serde_json::Value = serde_json::from_str(&body)?;
        let access_token = value
            .get("access_token")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .trim();
        if access_token.is_empty() {
            return Ok(None);
        }
        let refresh_token = value
            .get("refresh_token")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let expires_at = value
            .get("expires_at")
            .and_then(|value| value.as_u64().or_else(|| value.as_f64().map(|v| v as u64)))
            .filter(|value| *value > 0);
        Ok(Some(OAuthTokens {
            access_token: access_token.to_string(),
            refresh_token,
            expires_at,
            meta: Some(meta(serde_json::json!({"imported_from": "kimi-cli"}))),
        }))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn token_conversion_keeps_rotated_refresh_and_expiry() {
            let parsed = TokenResp {
                access_token: Some("new-access".into()),
                refresh_token: Some("new-refresh".into()),
                expires_in: Some(900),
                scope: "kimi-code".into(),
                token_type: "Bearer".into(),
                error: None,
                error_description: None,
            };
            let before = crate::oauth::now_unix();
            let tokens = into_tokens(parsed, Some("old-refresh"), None).unwrap();
            assert_eq!(tokens.access_token, "new-access");
            assert_eq!(tokens.refresh_token.as_deref(), Some("new-refresh"));
            assert!(tokens.expires_at.unwrap() >= before + 900);
        }

        #[test]
        fn token_conversion_preserves_refresh_when_server_omits_rotation() {
            let parsed = TokenResp {
                access_token: Some("new-access".into()),
                refresh_token: None,
                expires_in: Some(900),
                scope: String::new(),
                token_type: String::new(),
                error: None,
                error_description: None,
            };
            let tokens = into_tokens(parsed, Some("old-refresh"), None).unwrap();
            assert_eq!(tokens.refresh_token.as_deref(), Some("old-refresh"));
        }
    }
}

// ── Anthropic Claude (PKCE + Claude CLI import) ────────────────────────────
//
// Mirrors Claude Code's current OAuth endpoints (as of Code ≥2.1.x). The old
// `https://claude.ai/oauth/authorize` host drops query params and surfaces
// "Missing redirect_uri parameter"; Claude.ai login now uses
// `https://claude.com/cai/oauth/authorize` with `code=true`.

pub mod claude {
    use super::*;

    /// Public Claude Code OAuth client id.
    pub const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
    /// Claude.ai subscription authorize (primary).
    const AUTHORIZE_CLAUDE_AI: &str = "https://claude.com/cai/oauth/authorize";
    /// Console / API-plan authorize (fallback).
    const AUTHORIZE_CONSOLE: &str = "https://platform.claude.com/oauth/authorize";
    const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
    /// Manual paste callback used by Claude Code for headless / fallback.
    const MANUAL_REDIRECT: &str = "https://platform.claude.com/oauth/code/callback";
    /// Full scope set from Claude Code (`Cdi` = console + claude.ai scopes).
    const SCOPES: &str = "org:create_api_key user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";
    /// Claude Code refreshes with the claude.ai scopes only (no console scope).
    const REFRESH_SCOPES: &str =
        "user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";
    /// Prefer these loopback ports (Claude Code uses ephemeral; we pin a few).
    const CALLBACK_PORTS: &[u16] = &[54545, 54546, 54547, 21865];
    /// PKCE `state` entropy; Claude Code uses `randomBytes(32)`.
    const STATE_BYTES: usize = 32;

    #[derive(Deserialize)]
    struct TokenResp {
        access_token: Option<String>,
        refresh_token: Option<String>,
        expires_in: Option<u64>,
        error: Option<String>,
        error_description: Option<String>,
    }

    fn build_auth_url(authorize: &str, redirect: &str, state: &str, challenge: &str) -> String {
        // Order and `code=true` match Claude Code's generateAuthUrl.
        format!(
            "{authorize}?code=true&client_id={CLIENT_ID}&response_type=code&redirect_uri={}&scope={}&code_challenge={challenge}&code_challenge_method=S256&state={state}",
            urlencoding_encode(redirect),
            urlencoding_encode(SCOPES),
        )
    }

    fn exchange_code(
        code: &str,
        redirect: &str,
        verifier: &str,
        state: &str,
    ) -> Result<OAuthTokens> {
        let client = http()?;
        // JSON body, exactly as Claude Code's exchangeCodeForTokens sends it.
        let body = serde_json::json!({
            "grant_type": "authorization_code",
            "code": code,
            "redirect_uri": redirect,
            "client_id": CLIENT_ID,
            "code_verifier": verifier,
            "state": state,
        });
        let mut last = String::new();
        for url in [TOKEN_URL, "https://api.anthropic.com/v1/oauth/token"] {
            let res = match client
                .post(url)
                .header("Content-Type", "application/json")
                .body(body.to_string())
                .send()
            {
                Ok(r) => r,
                Err(e) => {
                    last = e.to_string();
                    continue;
                }
            };
            let body = res.text().unwrap_or_default();
            let parsed: TokenResp = match serde_json::from_str(&body) {
                Ok(p) => p,
                Err(e) => {
                    last = format!("invalid token response: {e}");
                    continue;
                }
            };
            if let Some(err) = parsed.error {
                last = format!("{err} {}", parsed.error_description.unwrap_or_default());
                continue;
            }
            if let Some(access) = parsed.access_token {
                return Ok(OAuthTokens {
                    access_token: access,
                    refresh_token: parsed.refresh_token,
                    expires_at: expires_in_to_at(parsed.expires_in),
                    meta: Some(OauthMeta {
                        issuer: "https://claude.ai".into(),
                        client_id: CLIENT_ID.into(),
                        extra: serde_json::json!({}),
                    }),
                });
            }
            last = oauth_error_summary(&body);
        }
        Err(NurError::Other(format!(
            "Claude token exchange failed: {last}"
        )))
    }

    pub fn login(tx: &ProgressTx, cancel: &CancelFlag) -> Result<OAuthTokens> {
        // Prefer an already-signed-in Claude Code session (no browser needed) —
        // but only a live one. Importing an expired session made /login
        // "succeed" and left the user in a refresh-failure loop; falling
        // through to the browser flow actually recovers. Same gate as
        // antigravity: None = never expires, Some = must be > now + 5 min.
        if let Ok(Some(imported)) = import_claude_cli() {
            if !crate::auth::oauth_expired(imported.expires_at) {
                send(
                    tx,
                    BrowserLoginProgress::Status("using existing Claude Code session".into()),
                );
                return Ok(imported);
            }
            send(
                tx,
                BrowserLoginProgress::Status(
                    "Claude Code session is expired — starting browser sign-in…".into(),
                ),
            );
        }

        let verifier = random_urlsafe(32);
        let challenge = pkce_challenge(&verifier);
        // 32 bytes like Claude Code; a 16-byte state is rejected after
        // sign-in with "Authorization failed - Invalid request format".
        let state = random_urlsafe(STATE_BYTES);

        // ── Prefer loopback (same as interactive Claude Code) ────────────
        let bound = CALLBACK_PORTS.iter().find_map(|port| {
            TcpListener::bind(("127.0.0.1", *port))
                .ok()
                .map(|listener| (listener, *port))
        });

        if let Some((listener, port)) = bound {
            let redirect = format!("http://localhost:{port}/callback");
            // Claude.ai subscription first; console as second open if user prefers.
            let auth_url = build_auth_url(AUTHORIZE_CLAUDE_AI, &redirect, &state, &challenge);
            let _console_url = build_auth_url(AUTHORIZE_CONSOLE, &redirect, &state, &challenge);

            send(tx, BrowserLoginProgress::OpenUrl(auth_url.clone()));
            send(
                tx,
                BrowserLoginProgress::Status(
                    "complete Claude sign-in in the browser (Claude.ai subscription)…".into(),
                ),
            );
            let _ = open_browser(&auth_url);

            let code =
                wait_localhost_code_on(listener, Some(&state), cancel, Duration::from_secs(600))?;
            send(
                tx,
                BrowserLoginProgress::Status("exchanging Claude authorization code…".into()),
            );
            return exchange_code(&code, &redirect, &verifier, &state);
        }

        // ── Manual paste fallback (Claude Code headless path) ────────────
        // platform.claude.com shows the code on a page; user pastes it here.
        let auth_url = build_auth_url(AUTHORIZE_CLAUDE_AI, MANUAL_REDIRECT, &state, &challenge);
        send(tx, BrowserLoginProgress::OpenUrl(auth_url.clone()));
        send(
            tx,
            BrowserLoginProgress::DeviceCode {
                verification_url: auth_url.clone(),
                user_code: "(paste the code from the browser after Authorize)".into(),
            },
        );
        send(
            tx,
            BrowserLoginProgress::Status(
                "localhost ports busy — open the URL, authorize, then paste the code and press Enter".into(),
            ),
        );
        let _ = open_browser(&auth_url);

        let pasted = wait_manual_code_paste(cancel, Duration::from_secs(600))?;
        let code = split_manual_code(&pasted, &state)?;
        send(
            tx,
            BrowserLoginProgress::Status("exchanging Claude authorization code…".into()),
        );
        exchange_code(code, MANUAL_REDIRECT, &verifier, &state)
    }

    /// The manual callback page shows `code#state`; Claude Code splits on
    /// `#` and checks the state. A bare code (no `#`) is accepted as-is.
    fn split_manual_code<'a>(pasted: &'a str, expected_state: &str) -> Result<&'a str> {
        let code = match pasted.trim().split_once('#') {
            Some((code, got)) if got == expected_state => Ok(code),
            Some(_) => Err(NurError::Other(
                "pasted Claude code belongs to a different sign-in (state mismatch) - start /login again".into(),
            )),
            None => Ok(pasted.trim()),
        }?;
        if code.trim().is_empty() {
            return Err(NurError::Other("empty Claude authorization code".into()));
        }
        Ok(code)
    }

    /// Wait for a CLI or TUI paste scoped to this exact login attempt.
    fn wait_manual_code_paste(cancel: &CancelFlag, timeout: Duration) -> Result<String> {
        let start = std::time::Instant::now();
        loop {
            if cancel.is_cancelled() {
                return Err(NurError::Other("login cancelled".into()));
            }
            if start.elapsed() > timeout {
                cancel.cancel();
                return Err(NurError::Other(
                    "Claude login timed out waiting for pasted code".into(),
                ));
            }
            if let Some(code) = cancel.take_manual_code() {
                return Ok(code);
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    pub fn refresh(refresh: &str) -> Result<OAuthTokens> {
        let client = http()?;
        // JSON body with the claude.ai scope set, as Claude Code's refresh sends.
        let body = serde_json::json!({
            "grant_type": "refresh_token",
            "refresh_token": refresh,
            "client_id": CLIENT_ID,
            "scope": REFRESH_SCOPES,
        });
        let mut last = String::from("no response");
        for url in [TOKEN_URL, "https://api.anthropic.com/v1/oauth/token"] {
            let res = match client
                .post(url)
                .header("Content-Type", "application/json")
                .body(body.to_string())
                .send()
            {
                Ok(r) => r,
                Err(e) => {
                    last = e.to_string();
                    continue;
                }
            };
            let body = res.text().unwrap_or_default();
            last = oauth_error_summary(&body);
            if let Ok(parsed) = serde_json::from_str::<TokenResp>(&body) {
                if let Some(access) = parsed.access_token {
                    return Ok(OAuthTokens {
                        access_token: access,
                        refresh_token: parsed.refresh_token.or_else(|| Some(refresh.to_string())),
                        expires_at: expires_in_to_at(parsed.expires_in),
                        meta: Some(OauthMeta {
                            issuer: "https://claude.ai".into(),
                            client_id: CLIENT_ID.into(),
                            extra: serde_json::json!({}),
                        }),
                    });
                }
            }
        }
        Err(NurError::Other(format!(
            "Claude token refresh failed: {last}"
        )))
    }

    pub fn import_claude_cli() -> Result<Option<OAuthTokens>> {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let config_dir = crate::t3code::driver_config_dir(crate::t3code::DriverId::Claude);
        let candidates = [
            config_dir.join(".credentials.json"),
            config_dir.join("credentials.json"),
            home.join(".config")
                .join("claude")
                .join(".credentials.json"),
        ];
        for path in candidates {
            if !path.exists() {
                continue;
            }
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(_) => continue,
            };
            let v: serde_json::Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if let Some(tokens) = parse_claude_credentials(&v) {
                return Ok(Some(tokens));
            }
        }
        Ok(None)
    }

    /// Tokens from a parsed Claude Code credentials file, or `None` when it
    /// holds no usable session.
    fn parse_claude_credentials(v: &serde_json::Value) -> Option<OAuthTokens> {
        let oauth = v
            .get("claudeAiOauth")
            .or_else(|| v.get("claude_ai_oauth"))?;
        let access = oauth
            .get("accessToken")
            .or_else(|| oauth.get("access_token"))
            .and_then(|x| x.as_str())
            .unwrap_or("");
        if access.is_empty() {
            return None;
        }
        let refresh = oauth
            .get("refreshToken")
            .or_else(|| oauth.get("refresh_token"))
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        // Claude stores expiresAt as ms epoch sometimes.
        let expires_at = oauth
            .get("expiresAt")
            .or_else(|| oauth.get("expires_at"))
            .and_then(|x| {
                x.as_u64()
                    .map(|n| if n > 10_000_000_000 { n / 1000 } else { n })
            });
        // A signed-out / long-idle Claude Code session keeps an expired
        // access token next to an expired refresh token. Importing that
        // only loops on refresh failures; treat it as no session.
        let refresh_expired = oauth
            .get("refreshTokenExpiresAt")
            .and_then(|x| x.as_u64())
            .map(|n| if n > 10_000_000_000 { n / 1000 } else { n })
            .is_some_and(|at| at <= super::super::now_unix());
        if refresh_expired && crate::auth::oauth_expired(expires_at) {
            return None;
        }
        Some(OAuthTokens {
            access_token: access.to_string(),
            refresh_token: refresh,
            expires_at,
            meta: Some(OauthMeta {
                issuer: "https://claude.ai".into(),
                client_id: CLIENT_ID.into(),
                extra: serde_json::json!({"imported_from": "claude-code"}),
            }),
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn state_matches_claude_code_length() {
            // base64url(32 bytes) = 43 chars; shorter states are rejected by
            // claude.com with "Invalid request format".
            assert_eq!(random_urlsafe(STATE_BYTES).len(), 43);
        }

        #[test]
        fn auth_url_carries_full_state() {
            let state = random_urlsafe(STATE_BYTES);
            let url = build_auth_url(
                AUTHORIZE_CLAUDE_AI,
                "http://localhost:54545/callback",
                &state,
                "challenge",
            );
            assert!(url.starts_with("https://claude.com/cai/oauth/authorize?code=true&"));
            assert!(url.ends_with(&format!("&state={state}")));
        }

        #[test]
        fn claude_code_import_skips_dead_sessions() {
            let now_ms = super::super::super::now_unix() * 1000;
            let creds = |access: &str, exp: u64, rt_exp: u64| {
                serde_json::json!({"claudeAiOauth": {
                    "accessToken": access, "refreshToken": "rt",
                    "expiresAt": exp, "refreshTokenExpiresAt": rt_exp,
                }})
            };
            // Signed out: empty tokens (what Claude Code leaves behind).
            assert!(parse_claude_credentials(&creds("", 0, now_ms + 1)).is_none());
            // Access and refresh both expired.
            assert!(parse_claude_credentials(&creds("at", 1, now_ms - 1)).is_none());
            // Access expired but refresh still valid: importable, nur refreshes.
            let t = parse_claude_credentials(&creds("at", 1, now_ms + 86_400_000)).unwrap();
            assert_eq!(t.refresh_token.as_deref(), Some("rt"));
            // Live access token.
            assert!(
                parse_claude_credentials(&creds("at", now_ms + 3_600_000, now_ms - 1)).is_some()
            );
        }

        #[test]
        fn refresh_scopes_are_login_scopes_minus_console() {
            assert_eq!(
                SCOPES.strip_prefix("org:create_api_key "),
                Some(REFRESH_SCOPES)
            );
        }

        #[test]
        fn manual_paste_splits_code_and_checks_state() {
            assert_eq!(split_manual_code(" abc#st \n", "st").unwrap(), "abc");
            assert_eq!(split_manual_code("abc", "st").unwrap(), "abc");
            assert!(split_manual_code("abc#other", "st").is_err());
            assert!(split_manual_code("#st", "st").is_err());
            assert!(split_manual_code(" ", "st").is_err());
        }
    }
}

fn urlencoding_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ── Google Gemini (browser SSO via gcloud - no embedded OAuth secrets) ─────

pub mod google {
    use super::*;

    /// Browser sign-in through the official Google Cloud SDK (`gcloud auth login`),
    /// then mint an access token for API calls. No OAuth client secrets ship in-repo
    /// (GitHub push protection). Users without gcloud can still paste a Gemini API key.
    pub fn login(tx: &ProgressTx, cancel: &CancelFlag) -> Result<OAuthTokens> {
        // Already signed in?
        if let Ok(t) = fetch_access_token() {
            send(
                tx,
                BrowserLoginProgress::Status("using existing gcloud session".into()),
            );
            return Ok(t);
        }
        send(
            tx,
            BrowserLoginProgress::Status(
                "launching Google browser login (gcloud auth login)…".into(),
            ),
        );
        // No placeholder tab: the real auth URL arrives from gcloud's output
        // below, and a stub accounts.google.com tab only confused users.
        let gcloud = gcloud_bin().ok_or_else(|| {
            NurError::Other(
                "gcloud not found on PATH (and not under common install dirs). Install Google Cloud SDK from https://cloud.google.com/sdk/docs/install, open a new terminal, then retry — or paste a Gemini API key via /login."
                    .into(),
            )
        })?;
        let mut child = Command::new(&gcloud)
            .args([
                "auth",
                "login",
                "--brief",
                "--update-adc",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                NurError::Other(format!(
                    "failed to launch gcloud ({e}). Install Google Cloud SDK, or choose “Enter API key” with a Gemini key."
                ))
            })?;
        if let Some(err) = child.stderr.take() {
            watch_login_output(err, tx.clone());
        }
        if let Some(out) = child.stdout.take() {
            watch_login_output(out, tx.clone());
        }
        if let Err(e) = wait_child_with_deadline(&mut child, cancel, VENDOR_CLI_LOGIN_TIMEOUT) {
            let _ = child.kill();
            let msg = e.to_string();
            if msg.contains("login process failed") {
                return Err(NurError::Other(
                    "gcloud auth login failed - paste a Gemini API key as fallback.".into(),
                ));
            }
            return Err(e);
        }
        send(
            tx,
            BrowserLoginProgress::Status("fetching Google access token…".into()),
        );
        fetch_access_token()
    }

    /// Transient import: try to reuse an existing gcloud ADC token without launching
    /// browser login. Returns `Ok(None)` when gcloud is not installed or not logged in,
    /// so the caller can fall through to other import methods.
    pub fn import_existing() -> Result<Option<OAuthTokens>> {
        match fetch_access_token() {
            Ok(t) => Ok(Some(t)),
            Err(_) => Ok(None),
        }
    }

    pub(crate) fn fetch_access_token() -> Result<OAuthTokens> {
        let gcloud = gcloud_bin().ok_or_else(|| {
            NurError::Other(
                "gcloud not found. Install Google Cloud SDK (https://cloud.google.com/sdk/docs/install) and ensure it is on PATH."
                    .into(),
            )
        })?;
        let out = Command::new(&gcloud)
            .args(["auth", "application-default", "print-access-token"])
            .output()
            .map_err(|e| NurError::Other(format!("gcloud ADC print-access-token: {e}")))?;
        if !out.status.success() {
            return Err(NurError::Other(format!(
                "gcloud application-default print-access-token failed: {}",
                String::from_utf8_lossy(&out.stderr)
            )));
        }
        let access = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if access.is_empty() {
            return Err(NurError::Other("empty token from gcloud".into()));
        }
        let project_id = std::env::var("GOOGLE_CLOUD_PROJECT")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .or_else(|| {
                let out = Command::new(&gcloud)
                    .args(["config", "get-value", "project"])
                    .output()
                    .ok()?;
                if !out.status.success() {
                    return None;
                }
                let value = String::from_utf8_lossy(&out.stdout).trim().to_string();
                (!value.is_empty() && value != "(unset)").then_some(value)
            })
            .ok_or_else(|| {
                NurError::Other(
                    "Google OAuth needs a quota project. Run `gcloud config set project PROJECT_ID` or set GOOGLE_CLOUD_PROJECT, then retry /login."
                        .into(),
                )
            })?;
        Ok(OAuthTokens {
            access_token: access,
            // Marker so ensure_fresh_oauth can re-call gcloud.
            refresh_token: Some("gcloud".into()),
            expires_at: Some(super::super::now_unix() + 3300),
            meta: Some(OauthMeta {
                issuer: "https://accounts.google.com".into(),
                client_id: "gcloud".into(),
                extra: serde_json::json!({
                    "product": "gemini-api",
                    "via": "gcloud application-default login",
                    "project_id": project_id,
                }),
            }),
        })
    }

    pub fn refresh(_auth: &Auth, _refresh: &str) -> Result<OAuthTokens> {
        fetch_access_token()
    }
}

/// Google Antigravity (agy CLI + Cloud Code OAuth) — browser login via
/// Google OAuth with the Antigravity client ID, then loadCodeAssist + onboard
/// to obtain a quota project. Also imports existing agy credentials from
/// Windows Credential Manager (`gemini:antigravity`) and from Gemini CLI
/// supported local credential locations.
pub mod antigravity {
    use super::*;
    use std::path::PathBuf;

    /// Google OAuth app credentials — supplied at runtime, never compiled in.
    ///
    /// nur-cli is a public repository, so an embedded client secret would be
    /// published to everyone who clones it (GitHub push protection blocks it
    /// outright). Set `NUR_GOOGLE_CLIENT_ID` + `NUR_GOOGLE_CLIENT_SECRET` (or
    /// the `GOOGLE_*` equivalents) to enable Google browser sign-in.
    ///
    /// Leaving them unset costs only the browser flow: signing in by importing
    /// an existing Antigravity or gcloud CLI session still works, as does an
    /// API key.
    fn client_id() -> String {
        std::env::var("NUR_GOOGLE_CLIENT_ID")
            .or_else(|_| std::env::var("GOOGLE_CLIENT_ID"))
            .map(|v| v.trim().to_string())
            .unwrap_or_default()
    }

    fn client_secret() -> String {
        std::env::var("NUR_GOOGLE_CLIENT_SECRET")
            .or_else(|_| std::env::var("GOOGLE_CLIENT_SECRET"))
            .map(|v| v.trim().to_string())
            .unwrap_or_default()
    }

    /// Whether a Google OAuth app is configured for the browser flow.
    fn oauth_app_configured() -> bool {
        !client_id().is_empty() && !client_secret().is_empty()
    }

    const OAUTH_APP_UNSET: &str = "Google browser sign-in needs an OAuth app: set NUR_GOOGLE_CLIENT_ID and NUR_GOOGLE_CLIENT_SECRET. Or sign in with the Antigravity/gcloud CLI (nur imports that session automatically), or paste a Gemini API key.";
    const AUTHORIZE_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
    const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
    const LOAD_ASSIST_URL: &str = "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist";
    const ONBOARD_URL: &str = "https://cloudcode-pa.googleapis.com/v1internal:onboardUser";
    const SCOPES: &[&str] = &[
        "https://www.googleapis.com/auth/cloud-platform",
        "https://www.googleapis.com/auth/userinfo.email",
        "https://www.googleapis.com/auth/userinfo.profile",
        "https://www.googleapis.com/auth/cclog",
        "https://www.googleapis.com/auth/experimentsandconfigs",
    ];
    // Aliased from providers.rs so the fingerprint cannot drift between the
    // catalog docs and the wire.
    const USER_AGENT: &str = crate::providers::CLOUD_CODE_USER_AGENT;
    const API_CLIENT: &str = crate::providers::CLOUD_CODE_API_CLIENT;
    const CLIENT_METADATA: &str = crate::providers::CLOUD_CODE_CLIENT_METADATA;
    /// Free-tier Code Assist uses a Google-managed project. Passing a project
    /// into `onboardUser` for free-tier causes Precondition Failed (gemini-cli).
    const FREE_TIER: &str = "free-tier";

    #[derive(Deserialize)]
    struct TokenResponse {
        access_token: String,
        refresh_token: Option<String>,
        expires_in: Option<u64>,
        error: Option<String>,
        error_description: Option<String>,
    }

    #[derive(Deserialize)]
    struct LoadAssistResponse {
        #[serde(rename = "cloudaicompanionProject")]
        cloud_project: Option<serde_json::Value>,
        #[serde(rename = "allowedTiers")]
        allowed_tiers: Option<Vec<Tier>>,
        #[serde(rename = "currentTier")]
        current_tier: Option<Tier>,
        #[serde(rename = "paidTier")]
        paid_tier: Option<Tier>,
        #[serde(rename = "ineligibleTiers")]
        ineligible_tiers: Option<Vec<IneligibleTier>>,
    }

    #[derive(Deserialize)]
    struct Tier {
        id: String,
        #[serde(rename = "isDefault")]
        is_default: Option<bool>,
        #[serde(rename = "hasOnboardedPreviously")]
        has_onboarded_previously: Option<bool>,
    }

    #[derive(Deserialize)]
    struct IneligibleTier {
        #[serde(rename = "reasonMessage")]
        reason_message: Option<String>,
    }

    #[derive(Deserialize)]
    struct OnboardResponse {
        done: Option<bool>,
        response: Option<OnboardInner>,
    }

    #[derive(Deserialize)]
    struct OnboardInner {
        #[serde(rename = "cloudaicompanionProject")]
        project: Option<serde_json::Value>,
    }

    /// Result of full Code Assist setup (load + optional free-tier onboard).
    #[derive(Debug, Clone, Default)]
    pub struct CodeAssistSetup {
        pub project_id: String,
        pub tier_id: String,
    }

    /// `cloudaicompanionProject` arrives either as a bare id or as an object
    /// carrying one, on both loadCodeAssist and onboardUser.
    fn project_id_of(v: Option<serde_json::Value>) -> String {
        match v {
            Some(serde_json::Value::String(s)) => s,
            Some(serde_json::Value::Object(mut obj)) => obj
                .remove("id")
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_default(),
            _ => String::new(),
        }
    }

    // ── Import existing Antigravity / Gemini CLI credentials ────────────────

    /// Try to import an existing Antigravity session.
    ///
    /// Order:
    /// 1. Windows Credential Manager (`gemini:antigravity` / `LegacyGeneric:...`)
    /// 2. File probes: `~/.gemini/oauth_creds.json`, `~/.config/gemini/...`, etc.
    /// 3. gcloud ADC as final fallback (so google provider still works)
    pub fn import_existing() -> Result<Option<OAuthTokens>> {
        // 1. Windows credential manager
        #[cfg(windows)]
        {
            if let Some(t) = import_from_wincred() {
                return Ok(Some(t));
            }
        }

        // 2. File-based Gemini CLI oauth (common locations)
        if let Some(t) = import_from_gemini_cli_file() {
            return Ok(Some(t));
        }

        // 3. gcloud fallback for google alias
        if let Ok(Some(t)) = crate::oauth::flows::google::import_existing() {
            return Ok(Some(t));
        }

        Ok(None)
    }

    #[cfg(windows)]
    fn import_from_wincred() -> Option<OAuthTokens> {
        // Use powershell to read credential blob (UTF-8 JSON)
        // This mirrors the manual extraction we validated.
        let ps_script = r#"
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public class CredManager2 {
    [DllImport("advapi32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
    public static extern bool CredRead(string target, int type, int flags, out IntPtr credential);
    [DllImport("advapi32.dll")]
    public static extern void CredFree(IntPtr cred);
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
    public struct CREDENTIAL {
        public int Flags;
        public int Type;
        public string TargetName;
        public string Comment;
        public long LastWritten;
        public int CredentialBlobSize;
        public IntPtr CredentialBlob;
        public int Persist;
        public int AttributeCount;
        public IntPtr Attributes;
        public string TargetAlias;
        public string UserName;
    }
    public static byte[] ReadBytes(string target) {
        IntPtr credPtr;
        if (!CredRead(target, 1, 0, out credPtr)) return null;
        var cred = (CREDENTIAL)System.Runtime.InteropServices.Marshal.PtrToStructure(credPtr, typeof(CREDENTIAL));
        byte[] bytes = new byte[cred.CredentialBlobSize];
        System.Runtime.InteropServices.Marshal.Copy(cred.CredentialBlob, bytes, 0, cred.CredentialBlobSize);
        CredFree(credPtr);
        return bytes;
    }
}
"@ -Language CSharp
$targets = @('LegacyGeneric:target=gemini:antigravity','gemini:antigravity','LegacyGeneric:target=gemini-cli:oauth','gemini-cli:oauth')
foreach ($t in $targets) {
  $b = [CredManager2]::ReadBytes($t)
  if ($b) { [Text.Encoding]::UTF8.GetString($b); break }
}
"#;
        let powershell = which_cli("powershell").unwrap_or_else(|| {
            PathBuf::from("C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe")
        });
        let out = Command::new(&powershell)
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                ps_script,
            ])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if text.is_empty() {
            return None;
        }
        // Expect JSON like {"token":{"access_token":"ya29...","refresh_token":"...","expiry":"..."}}
        let v: serde_json::Value = serde_json::from_str(&text).ok()?;
        let token_obj = v.get("token")?;
        let access = token_obj.get("access_token")?.as_str()?.trim();
        if access.is_empty() {
            return None;
        }
        let refresh = token_obj
            .get("refresh_token")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        let expiry_str = token_obj.get("expiry").and_then(|x| x.as_str());
        let expires_at = expiry_str.and_then(parse_expiry_to_unix);

        // Best-effort: full Code Assist setup (load + free-tier onboard when needed).
        // Mirrors gemini-cli `setupUser` so free-tier managed projects activate.
        // Skipped for a token we already know is dead: onboarding is a
        // state-changing network call, and the caller rejects expired imports.
        let mut extra = serde_json::json!({
            "via": "antigravity-cli-wincred",
            "auth_method": v.get("auth_method").and_then(|x| x.as_str()).unwrap_or("consumer"),
        });
        if !crate::auth::oauth_expired(expires_at) {
            if let Ok(setup) = setup_code_assist(access, None) {
                if !setup.project_id.is_empty() {
                    extra["project_id"] = serde_json::Value::String(setup.project_id);
                }
                if !setup.tier_id.is_empty() {
                    extra["tier_id"] = serde_json::Value::String(setup.tier_id);
                }
            }
        }

        Some(OAuthTokens {
            access_token: access.to_string(),
            refresh_token: refresh,
            expires_at,
            meta: Some(OauthMeta {
                issuer: "https://accounts.google.com".into(),
                client_id: client_id(),
                extra,
            }),
        })
    }

    fn parse_expiry_to_unix(s: &str) -> Option<u64> {
        // Format: 2026-07-22T13:47:44.591428-05:00 (RFC3339 with fractional)
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
            return Some(dt.timestamp() as u64);
        }
        // Try without fractional
        // chrono handles it anyway, but fallback: try naive
        None
    }

    fn import_from_gemini_cli_file() -> Option<OAuthTokens> {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let candidates = [
            home.join(".gemini").join("oauth_creds.json"),
            home.join(".config").join("gemini").join("oauth_creds.json"),
            home.join(".config")
                .join("gemini-cli")
                .join("oauth_creds.json"),
            home.join(".gemini")
                .join("antigravity-cli")
                .join("oauth_creds.json"),
        ];
        for path in candidates {
            if !path.exists() {
                continue;
            }
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(_) => continue,
            };
            let v: serde_json::Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(_) => continue,
            };
            // Try various shapes: flat {access_token, refresh_token, expiry...} or nested {token: {...}}
            let (access, refresh, expiry) = if let Some(token) = v.get("token") {
                (
                    token
                        .get("access_token")
                        .and_then(|x| x.as_str())
                        .unwrap_or(""),
                    token
                        .get("refresh_token")
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string()),
                    token
                        .get("expiry")
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string()),
                )
            } else {
                (
                    v.get("access_token").and_then(|x| x.as_str()).unwrap_or(""),
                    v.get("refresh_token")
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string()),
                    v.get("expiry")
                        .or_else(|| v.get("expires_at"))
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string()),
                )
            };
            if access.is_empty() {
                continue;
            }
            let expires_at = expiry
                .as_deref()
                .and_then(parse_expiry_to_unix)
                .or_else(|| {
                    v.get("expires_in")
                        .and_then(|x| x.as_u64())
                        .map(|secs| crate::oauth::now_unix() + secs)
                });
            let mut extra = serde_json::json!({
                "via": "gemini-cli-file",
                "path": path.display().to_string(),
            });
            // Setup is a state-changing network call - skip it for a token we
            // already know is dead; the caller rejects expired imports.
            if !crate::auth::oauth_expired(expires_at) {
                if let Ok(setup) = setup_code_assist(access, None) {
                    if !setup.project_id.is_empty() {
                        extra["project_id"] = serde_json::Value::String(setup.project_id);
                    }
                    if !setup.tier_id.is_empty() {
                        extra["tier_id"] = serde_json::Value::String(setup.tier_id);
                    }
                }
            }
            return Some(OAuthTokens {
                access_token: access.to_string(),
                refresh_token: refresh,
                expires_at,
                meta: Some(OauthMeta {
                    issuer: "https://accounts.google.com".into(),
                    client_id: client_id(),
                    extra,
                }),
            });
        }
        None
    }

    // ── Browser OAuth login ──────────────────────────────────────────────────

    pub fn login(tx: &ProgressTx, cancel: &CancelFlag) -> Result<OAuthTokens> {
        // If agy already logged in, reuse
        if let Ok(Some(existing)) = import_existing() {
            // Check not expired
            if let Some(exp) = existing.expires_at {
                if exp > crate::oauth::now_unix() + 300 {
                    send(
                        tx,
                        BrowserLoginProgress::Status(
                            "using existing Antigravity CLI session".into(),
                        ),
                    );
                    return Ok(existing);
                }
            } else {
                send(
                    tx,
                    BrowserLoginProgress::Status("using existing Antigravity CLI session".into()),
                );
                return Ok(existing);
            }
        }

        // Importing a CLI session above needs no OAuth app; the browser flow
        // does. Fail here with something actionable rather than after the user
        // has already been sent through a consent screen.
        if !oauth_app_configured() {
            return Err(NurError::Other(OAUTH_APP_UNSET.into()));
        }

        // Loopback server
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|e| NurError::Other(format!("failed to bind loopback: {e}")))?;
        let port = listener
            .local_addr()
            .map(|a| a.port())
            .map_err(|e| NurError::Other(format!("loopback port: {e}")))?;
        let redirect_uri = format!("http://localhost:{port}/callback");
        let state = random_urlsafe(32);

        let auth_url = format!(
            "{AUTHORIZE_URL}?client_id={}&response_type=code&redirect_uri={}&scope={}&state={}&access_type=offline&prompt=consent",
            urlencoding_encode(&client_id()),
            urlencoding_encode(&redirect_uri),
            urlencoding_encode(&SCOPES.join(" ")),
            urlencoding_encode(&state),
        );

        send(tx, BrowserLoginProgress::OpenUrl(auth_url.clone()));
        send(
            tx,
            BrowserLoginProgress::Status(
                "complete Antigravity / Google sign-in in the browser…".into(),
            ),
        );
        let _ = open_browser(&auth_url);

        let code =
            wait_localhost_code_on(listener, Some(&state), cancel, Duration::from_secs(600))?;

        send(
            tx,
            BrowserLoginProgress::Status("exchanging Antigravity authorization code…".into()),
        );

        let tokens = exchange_code(&code, &redirect_uri)?;

        send(
            tx,
            BrowserLoginProgress::Status("loading Code Assist project…".into()),
        );

        send(
            tx,
            BrowserLoginProgress::Status(
                "setting up Code Assist (load + onboard if needed)…".into(),
            ),
        );
        let env_project = crate::providers::explicit_google_cloud_project_from_env();
        let setup = match setup_code_assist(&tokens.access_token, env_project.as_deref()) {
            Ok(s) => s,
            Err(e) => {
                send(
                    tx,
                    BrowserLoginProgress::Status(format!(
                        "Code Assist setup warning: {e} – continuing with token only"
                    )),
                );
                CodeAssistSetup::default()
            }
        };
        if !setup.project_id.is_empty() {
            send(
                tx,
                BrowserLoginProgress::Status(format!(
                    "Code Assist project {} (tier {})",
                    setup.project_id, setup.tier_id
                )),
            );
        }

        let mut extra = serde_json::json!({
            "via": "antigravity-oauth",
            "auth_method": "consumer",
        });
        if !setup.project_id.is_empty() {
            extra["project_id"] = serde_json::Value::String(setup.project_id.clone());
        }
        if !setup.tier_id.is_empty() {
            extra["tier_id"] = serde_json::Value::String(setup.tier_id.clone());
        }

        Ok(OAuthTokens {
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            expires_at: tokens.expires_at,
            meta: Some(OauthMeta {
                issuer: "https://accounts.google.com".into(),
                client_id: client_id(),
                extra,
            }),
        })
    }

    fn exchange_code(code: &str, redirect_uri: &str) -> Result<OAuthTokens> {
        let (cid, csecret) = (client_id(), client_secret());
        if cid.is_empty() || csecret.is_empty() {
            return Err(NurError::Other(OAUTH_APP_UNSET.into()));
        }
        let form = [
            ("grant_type", "authorization_code"),
            ("client_id", cid.as_str()),
            ("client_secret", csecret.as_str()),
            ("code", code),
            ("redirect_uri", redirect_uri),
        ];
        let resp = http()?
            .post(TOKEN_URL)
            .form(&form)
            .send()
            .map_err(|e| NurError::Other(format!("Antigravity token exchange failed: {e}")))?;

        let status = resp.status();
        let text = resp
            .text()
            .map_err(|e| NurError::Other(format!("read token response failed: {e}")))?;

        if !status.is_success() {
            return Err(NurError::Other(format!(
                "Antigravity token exchange failed ({}): {}",
                status,
                oauth_error_summary(&text)
            )));
        }

        let parsed: TokenResponse = serde_json::from_str(&text)
            .map_err(|e| NurError::Other(format!("invalid token JSON: {e}")))?;

        if let Some(err) = parsed.error {
            return Err(NurError::Other(format!(
                "Antigravity OAuth error: {} - {}",
                err,
                parsed.error_description.unwrap_or_default()
            )));
        }

        if parsed.access_token.trim().is_empty() {
            return Err(NurError::Other(
                "empty access token from Antigravity".into(),
            ));
        }

        Ok(OAuthTokens {
            access_token: parsed.access_token,
            refresh_token: parsed.refresh_token,
            expires_at: expires_in_to_at(parsed.expires_in),
            meta: None,
        })
    }

    /// Call `loadCodeAssist` the way gemini-cli does: optional env project +
    /// `metadata.duetProject`. Returns the full parsed response.
    fn load_code_assist(
        access_token: &str,
        env_project: Option<&str>,
    ) -> Result<LoadAssistResponse> {
        let mut metadata = serde_json::json!({
            "ideType": "IDE_UNSPECIFIED",
            "platform": "PLATFORM_UNSPECIFIED",
            "pluginType": "GEMINI"
        });
        if let Some(p) = env_project.filter(|s| !s.is_empty()) {
            metadata["duetProject"] = serde_json::Value::String(p.to_string());
        }
        let mut body = serde_json::json!({ "metadata": metadata });
        if let Some(p) = env_project.filter(|s| !s.is_empty()) {
            body["cloudaicompanionProject"] = serde_json::Value::String(p.to_string());
        }

        let resp = http()?
            .post(LOAD_ASSIST_URL)
            .header("Authorization", format!("Bearer {access_token}"))
            .header("Content-Type", "application/json")
            .header("User-Agent", USER_AGENT)
            .header("X-Goog-Api-Client", API_CLIENT)
            .header("Client-Metadata", CLIENT_METADATA)
            .json(&body)
            .send()
            .map_err(|e| NurError::Other(format!("loadCodeAssist request failed: {e}")))?;

        let status = resp.status();
        let text = resp
            .text()
            .map_err(|e| NurError::Other(format!("read loadCodeAssist response failed: {e}")))?;

        if !status.is_success() {
            return Err(NurError::Other(format!(
                "loadCodeAssist failed ({}): {}",
                status,
                oauth_error_summary(&text)
            )));
        }

        serde_json::from_str(&text)
            .map_err(|e| NurError::Other(format!("invalid loadCodeAssist JSON: {e}")))
    }

    /// Full Code Assist setup matching gemini-cli `setupUser`:
    /// 1. Prefer `GOOGLE_CLOUD_PROJECT` env when set
    /// 2. loadCodeAssist
    /// 3. If already has currentTier + project → done (unless `force`)
    /// 4. Else onboardUser — free-tier must NOT send cloudaicompanionProject
    /// 5. Poll LRO until done; use server-assigned project id
    pub fn setup_code_assist(
        access_token: &str,
        env_project: Option<&str>,
    ) -> Result<CodeAssistSetup> {
        setup_code_assist_inner(access_token, env_project, false)
    }

    /// Like [`setup_code_assist`], but re-runs `onboardUser` even when
    /// `currentTier` is already present. Used for 403 Cloud Code Private API
    /// recovery where a stored free-tier project is not yet activated.
    pub fn setup_code_assist_force(
        access_token: &str,
        env_project: Option<&str>,
    ) -> Result<CodeAssistSetup> {
        setup_code_assist_inner(access_token, env_project, true)
    }

    fn setup_code_assist_inner(
        access_token: &str,
        env_project: Option<&str>,
        force: bool,
    ) -> Result<CodeAssistSetup> {
        let env_project = env_project
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(crate::providers::explicit_google_cloud_project_from_env);

        let load = load_code_assist(access_token, env_project.as_deref())?;

        // Already onboarded: currentTier present (skip when force-recovering).
        if !force {
            if let Some(ref current) = load.current_tier {
                let project = project_id_of(load.cloud_project.clone());
                let tier_id = load
                    .paid_tier
                    .as_ref()
                    .map(|t| t.id.clone())
                    .unwrap_or_else(|| current.id.clone());
                if !project.is_empty() {
                    return Ok(CodeAssistSetup {
                        project_id: project,
                        tier_id,
                    });
                }
                // currentTier but no server project — fall back to env project.
                if let Some(p) = env_project.clone() {
                    return Ok(CodeAssistSetup {
                        project_id: p,
                        tier_id,
                    });
                }
                // Continue to onboard if allowed tiers exist.
            }
        }

        let tier = pick_onboard_tier(&load);
        let tier_id = tier.id.clone();

        // free-tier: never send cloudaicompanionProject (Precondition Failed).
        // paid/standard: send env project when available.
        // force free-tier recovery also omits project so the server can assign
        // a fresh managed project when the stored one returns 403.
        let assigned = complete_onboarding(
            access_token,
            if tier_id == FREE_TIER {
                None
            } else {
                env_project.as_deref()
            },
            &tier_id,
        )?;

        let project_id = assigned
            .filter(|s| !s.is_empty())
            .or_else(|| {
                // On force free-tier, prefer the newly assigned project only;
                // fall back to loadCodeAssist project / env last.
                if force && tier_id == FREE_TIER {
                    let from_load = project_id_of(load.cloud_project.clone());
                    if !from_load.is_empty() {
                        return Some(from_load);
                    }
                }
                env_project.clone()
            })
            .or_else(|| {
                let from_load = project_id_of(load.cloud_project.clone());
                (!from_load.is_empty()).then_some(from_load)
            })
            .unwrap_or_default();

        if project_id.is_empty() {
            // Surface free-tier ineligibility if the server explained it.
            if let Some(reasons) = load.ineligible_tiers.as_ref() {
                let msg = reasons
                    .iter()
                    .filter_map(|t| t.reason_message.as_deref())
                    .collect::<Vec<_>>()
                    .join("; ");
                if !msg.is_empty() {
                    return Err(NurError::Other(format!(
                        "Code Assist ineligible: {msg}. Set GOOGLE_CLOUD_PROJECT for a paid/workspace project, or fix the account at the validation URL. Or run `/login antigravity` after signing into the Antigravity/Gemini CLI."
                    )));
                }
            }
            return Err(NurError::Other(
                "Code Assist setup returned no project id. Run `/login antigravity`, enable the Cloud Code API, or set GOOGLE_CLOUD_PROJECT to a project where Code Assist is available.".into(),
            ));
        }

        Ok(CodeAssistSetup {
            project_id,
            tier_id,
        })
    }

    fn clone_tier(tier: &Tier) -> Tier {
        Tier {
            id: tier.id.clone(),
            is_default: tier.is_default,
            has_onboarded_previously: tier.has_onboarded_previously,
        }
    }

    /// Tier pick: default allowed tier → free-tier preference → first allowed → free-tier.
    fn pick_onboard_tier(load: &LoadAssistResponse) -> Tier {
        if let Some(tiers) = load.allowed_tiers.as_ref() {
            if let Some(tier) = tiers.iter().find(|t| t.is_default.unwrap_or(false)) {
                return clone_tier(tier);
            }
            if let Some(tier) = tiers.iter().find(|t| {
                let id = t.id.to_ascii_lowercase();
                id == FREE_TIER || id.contains("free")
            }) {
                return clone_tier(tier);
            }
            if let Some(first) = tiers.first() {
                return clone_tier(first);
            }
        }
        // Prefer currentTier when allowed list is empty.
        if let Some(ref current) = load.current_tier {
            if !current.id.trim().is_empty() {
                return clone_tier(current);
            }
        }
        Tier {
            id: FREE_TIER.to_string(),
            is_default: Some(true),
            has_onboarded_previously: None,
        }
    }

    /// Public: resolve the Cloud Code project id for an access token via full
    /// setup (load + onboard when needed). Used by the API layer when a Gemini
    /// Cloud Code request has no stored `project_id` on its OAuth session.
    pub fn resolve_project_id(access_token: &str) -> Result<String> {
        let setup = setup_code_assist(access_token, None)?;
        if setup.project_id.is_empty() {
            return Err(NurError::Other(
                "Code Assist setup returned no cloudaicompanionProject".into(),
            ));
        }
        Ok(setup.project_id)
    }

    /// Onboard the user. For free-tier, `project_id` must be `None` (gemini-cli).
    /// Polls the LRO until done and returns the server-assigned project id.
    fn complete_onboarding(
        access_token: &str,
        project_id: Option<&str>,
        tier_id: &str,
    ) -> Result<Option<String>> {
        let mut body = serde_json::json!({
            "tierId": tier_id,
            "metadata": {
                "ideType": "IDE_UNSPECIFIED",
                "platform": "PLATFORM_UNSPECIFIED",
                "pluginType": "GEMINI"
            },
        });
        if let Some(p) = project_id.filter(|s| !s.is_empty()) {
            body["cloudaicompanionProject"] = serde_json::Value::String(p.to_string());
            body["metadata"]["duetProject"] = serde_json::Value::String(p.to_string());
        }

        let mut last_err = String::new();
        for attempt in 0..12 {
            let resp = http()?
                .post(ONBOARD_URL)
                .header("Authorization", format!("Bearer {access_token}"))
                .header("Content-Type", "application/json")
                .header("User-Agent", USER_AGENT)
                .header("X-Goog-Api-Client", API_CLIENT)
                .header("Client-Metadata", CLIENT_METADATA)
                .json(&body)
                .send();

            let resp = match resp {
                Ok(r) => r,
                Err(e) => {
                    last_err = e.to_string();
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };

            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            if !status.is_success() {
                last_err = format!("{status}: {text}");
                // Precondition on free-tier with project set: retry without project.
                if tier_id == FREE_TIER && body.get("cloudaicompanionProject").is_some() {
                    if let Some(o) = body.as_object_mut() {
                        o.remove("cloudaicompanionProject");
                        if let Some(serde_json::Value::Object(m)) = o.get_mut("metadata") {
                            m.remove("duetProject");
                        }
                    }
                }
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }

            let parsed: OnboardResponse = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(e) => {
                    last_err = e.to_string();
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };

            if parsed.done.unwrap_or(false) {
                let assigned = project_id_of(parsed.response.and_then(|r| r.project));
                return Ok((!assigned.is_empty()).then_some(assigned));
            }

            // LRO still running — if we got a name, keep polling via onboardUser
            // with the same body (server is eventually consistent).
            if attempt < 11 {
                std::thread::sleep(Duration::from_secs(5));
            }
        }

        if !last_err.is_empty() {
            return Err(NurError::Other(format!(
                "onboardUser did not complete: {last_err}"
            )));
        }
        // Non-fatal: onboarding may already be done server-side
        Ok(None)
    }

    pub fn refresh(auth: &Auth, refresh_token: &str) -> Result<OAuthTokens> {
        // If marker "gcloud", delegate to gcloud refresh
        if refresh_token == "gcloud" {
            return super::google::fetch_access_token();
        }

        // Standard Google refresh via a configured OAuth app.
        let (cid, csecret) = (client_id(), client_secret());
        if cid.is_empty() || csecret.is_empty() {
            // CLI-import users (agy / Gemini CLI) have no nur-side Google OAuth
            // app configured, so the direct refresh_token grant is unavailable.
            // The vendor CLI, however, keeps its own auto-refreshing session and
            // rewrites a fresh access token into Windows Credential Manager /
            // its creds file. Re-import that instead of dead-ending on
            // OAUTH_APP_UNSET - this is what makes `antigravity` work for
            // CLI-only users (no NUR_GOOGLE_CLIENT_ID required).
            if let Ok(Some(fresh)) = import_existing() {
                if !fresh.access_token.trim().is_empty()
                    && !crate::auth::oauth_expired(fresh.expires_at)
                {
                    return Ok(fresh);
                }
            }
            return Err(NurError::Other(OAUTH_APP_UNSET.into()));
        }
        let form = [
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", cid.as_str()),
            ("client_secret", csecret.as_str()),
        ];

        let resp = http()?
            .post(TOKEN_URL)
            .form(&form)
            .send()
            .map_err(|e| NurError::Other(format!("Antigravity refresh failed: {e}")))?;

        let status = resp.status();
        let text = resp
            .text()
            .map_err(|e| NurError::Other(format!("read refresh response failed: {e}")))?;

        if !status.is_success() {
            return Err(NurError::Other(format!(
                "Antigravity refresh failed ({}): {}",
                status,
                oauth_error_summary(&text)
            )));
        }

        let parsed: TokenResponse = serde_json::from_str(&text)
            .map_err(|e| NurError::Other(format!("invalid refresh JSON: {e}")))?;

        if parsed.access_token.trim().is_empty() {
            return Err(NurError::Other("empty access token on refresh".into()));
        }

        // Preserve existing meta; re-run setup when project is missing so free-tier
        // managed projects stay activated after token refresh.
        let mut extra = auth
            .oauth_meta
            .as_ref()
            .map(|m| m.extra.clone())
            .unwrap_or_else(|| serde_json::json!({}));

        let needs_setup = extra
            .get("project_id")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().is_empty())
            .unwrap_or(true);
        if needs_setup {
            if let Ok(setup) = setup_code_assist(&parsed.access_token, None) {
                if !setup.project_id.is_empty() {
                    extra["project_id"] = serde_json::Value::String(setup.project_id);
                }
                if !setup.tier_id.is_empty() {
                    extra["tier_id"] = serde_json::Value::String(setup.tier_id);
                }
            }
        }

        Ok(OAuthTokens {
            access_token: parsed.access_token,
            refresh_token: parsed
                .refresh_token
                .or_else(|| Some(refresh_token.to_string())),
            expires_at: expires_in_to_at(parsed.expires_in),
            meta: Some(OauthMeta {
                issuer: "https://accounts.google.com".into(),
                client_id: client_id(),
                extra,
            }),
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn project_id_of_accepts_string_and_object() {
            assert_eq!(
                project_id_of(Some(serde_json::json!("vivid-question-5fs6l"))),
                "vivid-question-5fs6l"
            );
            assert_eq!(
                project_id_of(Some(serde_json::json!({"id": "my-proj"}))),
                "my-proj"
            );
            assert_eq!(project_id_of(None), "");
            assert_eq!(project_id_of(Some(serde_json::json!(null))), "");
            assert_eq!(project_id_of(Some(serde_json::json!({}))), "");
        }

        #[test]
        fn pick_onboard_tier_prefers_default_then_free() {
            let load = LoadAssistResponse {
                cloud_project: None,
                allowed_tiers: Some(vec![
                    Tier {
                        id: "standard-tier".into(),
                        is_default: Some(false),
                        has_onboarded_previously: None,
                    },
                    Tier {
                        id: FREE_TIER.into(),
                        is_default: Some(true),
                        has_onboarded_previously: Some(false),
                    },
                ]),
                current_tier: None,
                paid_tier: None,
                ineligible_tiers: None,
            };
            assert_eq!(pick_onboard_tier(&load).id, FREE_TIER);
        }

        #[test]
        fn pick_onboard_tier_prefers_free_when_no_default() {
            let load = LoadAssistResponse {
                cloud_project: None,
                allowed_tiers: Some(vec![
                    Tier {
                        id: "standard-tier".into(),
                        is_default: Some(false),
                        has_onboarded_previously: None,
                    },
                    Tier {
                        id: FREE_TIER.into(),
                        is_default: Some(false),
                        has_onboarded_previously: None,
                    },
                ]),
                current_tier: None,
                paid_tier: None,
                ineligible_tiers: None,
            };
            assert_eq!(pick_onboard_tier(&load).id, FREE_TIER);
        }

        #[test]
        fn pick_onboard_tier_falls_back_to_current_then_free() {
            let with_current = LoadAssistResponse {
                cloud_project: None,
                allowed_tiers: None,
                current_tier: Some(Tier {
                    id: "legacy-tier".into(),
                    is_default: None,
                    has_onboarded_previously: Some(true),
                }),
                paid_tier: None,
                ineligible_tiers: None,
            };
            assert_eq!(pick_onboard_tier(&with_current).id, "legacy-tier");

            let empty = LoadAssistResponse {
                cloud_project: None,
                allowed_tiers: None,
                current_tier: None,
                paid_tier: None,
                ineligible_tiers: None,
            };
            assert_eq!(pick_onboard_tier(&empty).id, FREE_TIER);
        }

        #[test]
        fn parse_load_assist_response_shape() {
            // No network: exercise the same serde shape setup_code_assist uses.
            let text = r#"{
                "cloudaicompanionProject": {"id": "vivid-question-5fs6l"},
                "currentTier": {"id": "free-tier", "isDefault": true},
                "allowedTiers": [
                    {"id": "free-tier", "isDefault": true, "hasOnboardedPreviously": true},
                    {"id": "standard-tier", "isDefault": false}
                ],
                "paidTier": {"id": "standard-tier"}
            }"#;
            let parsed: LoadAssistResponse = serde_json::from_str(text).unwrap();
            assert_eq!(
                project_id_of(parsed.cloud_project.clone()),
                "vivid-question-5fs6l"
            );
            assert_eq!(
                parsed.current_tier.as_ref().map(|t| t.id.as_str()),
                Some(FREE_TIER)
            );
            // When already onboarded, pick still returns free-tier as default.
            assert_eq!(pick_onboard_tier(&parsed).id, FREE_TIER);

            let bare = r#"{"cloudaicompanionProject":"proj-bare-string"}"#;
            let bare_parsed: LoadAssistResponse = serde_json::from_str(bare).unwrap();
            assert_eq!(project_id_of(bare_parsed.cloud_project), "proj-bare-string");
        }
    }
}

// ── GitHub Models (browser SSO via the official `gh` CLI) ───────────────────

pub mod github {
    use super::*;

    /// Sign in through the official GitHub CLI (`gh auth login --web`), then mint
    /// a token for GitHub Models. No OAuth client secrets ship in-repo. If `gh`
    /// is already authenticated, the existing session is reused. Users without
    /// `gh` can still paste a GitHub PAT (with `models:read`) via "Enter API key".
    pub fn login(provider_id: &str, tx: &ProgressTx, cancel: &CancelFlag) -> Result<OAuthTokens> {
        // Already signed in? Reuse the existing gh token when it carries the
        // permission required by the selected product.
        let existing = fetch_token(provider_id).ok();
        if let Some(t) = existing.as_ref().filter(|tokens| {
            provider_id != "github-models" || models_scope_available(&tokens.access_token)
        }) {
            send(
                tx,
                BrowserLoginProgress::Status("using existing GitHub CLI session".into()),
            );
            return Ok(t.clone());
        }
        send(
            tx,
            BrowserLoginProgress::Status("launching GitHub browser login (gh auth login)…".into()),
        );
        send(
            tx,
            BrowserLoginProgress::OpenUrl("https://github.com/login/device".into()),
        );
        // `--web` opens the device flow; feed newlines so the "press Enter to
        // open the browser" prompt proceeds without a TTY.
        let gh = gh_bin().ok_or_else(|| {
            NurError::Other(
                "gh not found on PATH. Install GitHub CLI (https://cli.github.com/), open a new terminal, then retry — or paste a GitHub PAT (models:read) via /login."
                    .into(),
            )
        })?;
        let args = if provider_id == "github-models" && existing.is_some() {
            vec![
                "auth",
                "refresh",
                "--hostname",
                "github.com",
                "--scopes",
                "models",
            ]
        } else {
            let mut args = vec![
                "auth",
                "login",
                "--web",
                "--hostname",
                "github.com",
                "--git-protocol",
                "https",
            ];
            if provider_id == "github-models" {
                args.extend(["--scopes", "models"]);
            }
            args
        };
        let mut child = Command::new(&gh)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                NurError::Other(format!(
                    "failed to launch gh ({e}). Install GitHub CLI, or paste a GitHub PAT (models:read)."
                ))
            })?;
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(b"\n\n");
        }
        if let Some(err) = child.stderr.take() {
            watch_login_output(err, tx.clone());
        }
        if let Some(out) = child.stdout.take() {
            watch_login_output(out, tx.clone());
        }
        if let Err(e) = wait_child_with_deadline(&mut child, cancel, VENDOR_CLI_LOGIN_TIMEOUT) {
            let _ = child.kill();
            let msg = e.to_string();
            if msg.contains("login process failed") {
                return Err(NurError::Other(
                    "gh auth login failed - paste a GitHub PAT (models:read) as fallback.".into(),
                ));
            }
            return Err(e);
        }
        send(
            tx,
            BrowserLoginProgress::Status("fetching GitHub token…".into()),
        );
        fetch_token(provider_id)
    }

    fn fetch_token(provider_id: &str) -> Result<OAuthTokens> {
        let gh = gh_bin().ok_or_else(|| {
            NurError::Other(
                "gh not found. Install GitHub CLI (https://cli.github.com/) and ensure it is on PATH."
                    .into(),
            )
        })?;
        let out = Command::new(&gh)
            .args(["auth", "token", "--hostname", "github.com"])
            .output()
            .map_err(|e| NurError::Other(format!("gh auth token: {e}")))?;
        if !out.status.success() {
            return Err(NurError::Other(format!(
                "gh auth token failed: {}",
                String::from_utf8_lossy(&out.stderr)
            )));
        }
        let access = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if access.is_empty() {
            return Err(NurError::Other("empty token from gh".into()));
        }
        Ok(OAuthTokens {
            access_token: access,
            // Marker so ensure_fresh_oauth can re-call `gh auth token`.
            refresh_token: Some("gh".into()),
            // gh manages token lifetime; re-fetch opportunistically.
            expires_at: None,
            meta: Some(OauthMeta {
                issuer: "https://github.com".into(),
                client_id: "gh".into(),
                extra: serde_json::json!({"product": provider_id, "via": "gh auth login"}),
            }),
        })
    }

    fn models_scope_available(token: &str) -> bool {
        http()
            .and_then(|client| {
                client
                    .get("https://models.github.ai/catalog/models")
                    .bearer_auth(token)
                    .header("Accept", "application/vnd.github+json")
                    .send()
                    .map_err(|error| NurError::Other(error.to_string()))
            })
            .is_ok_and(|response| response.status().is_success())
    }

    pub fn refresh(auth: &Auth, _refresh: &str) -> Result<OAuthTokens> {
        fetch_token(&auth.provider)
    }
}

// ── Hugging Face (device code — same spirit as `hf auth login`) ────────────

pub mod huggingface {
    use super::*;

    /// Import token written by `huggingface-cli login` / hub cache.
    pub fn import_hf_token() -> Option<OAuthTokens> {
        let home = dirs::home_dir()?;
        let candidates = [
            home.join(".cache").join("huggingface").join("token"),
            home.join(".huggingface").join("token"),
        ];
        for path in candidates {
            if let Ok(text) = std::fs::read_to_string(&path) {
                let token = text.trim().to_string();
                if token.starts_with("hf_") && token.len() > 10 {
                    return Some(OAuthTokens {
                        access_token: token,
                        refresh_token: None,
                        expires_at: None,
                        meta: Some(OauthMeta {
                            issuer: "https://huggingface.co".into(),
                            client_id: "hf-token-file".into(),
                            extra: serde_json::json!({"imported_from": path.display().to_string()}),
                        }),
                    });
                }
            }
        }
        if let Ok(token) =
            std::env::var("HF_TOKEN").or_else(|_| std::env::var("HUGGING_FACE_HUB_TOKEN"))
        {
            let token = token.trim().to_string();
            if token.starts_with("hf_") {
                return Some(OAuthTokens {
                    access_token: token,
                    refresh_token: None,
                    expires_at: None,
                    meta: Some(OauthMeta {
                        issuer: "https://huggingface.co".into(),
                        client_id: "hf-token-env".into(),
                        extra: serde_json::json!({}),
                    }),
                });
            }
        }
        None
    }

    /// Hugging Face credentials are long-lived static tokens (`hf_…`): there
    /// is no short-lived access token to rotate, so "refresh" correctly means
    /// re-reading the token file/env. The device-flow login above is
    /// unreachable dead code (browser_auth: false, no login_browser arm) -
    /// if it ever ships, an OAuth refresh grant goes here.
    pub fn refresh(_refresh: &str) -> Result<OAuthTokens> {
        if let Some(t) = import_hf_token() {
            return Ok(t);
        }
        Err(NurError::Other(
            "Hugging Face token refresh not available — re-run browser login or paste HF_TOKEN"
                .into(),
        ))
    }
}

// ── Azure OpenAI (Entra via `az login`, like Azure CLI) ────────────────────

pub mod azure {
    use super::*;

    pub fn login(tx: &ProgressTx, cancel: &CancelFlag) -> Result<OAuthTokens> {
        // If already logged in, just mint a token.
        if let Ok(t) = fetch_token() {
            send(
                tx,
                BrowserLoginProgress::Status("using existing Azure CLI session".into()),
            );
            return Ok(t);
        }
        send(
            tx,
            BrowserLoginProgress::Status("launching Azure device login (az login)…".into()),
        );
        send(
            tx,
            BrowserLoginProgress::DeviceCode {
                verification_url: "https://microsoft.com/devicelogin".into(),
                user_code: "(see az output — opening browser)".into(),
            },
        );
        let _ = open_browser("https://microsoft.com/devicelogin");
        let az = az_bin().ok_or_else(|| {
            NurError::Other(
                "Azure CLI (`az`) not found on PATH. Install from https://aka.ms/installazurecliwindows, open a new terminal, then retry — or paste AZURE_OPENAI_API_KEY via /login."
                    .into(),
            )
        })?;
        let mut child = Command::new(&az)
            .args(["login", "--use-device-code", "--output", "none"])
            .env("AZURE_CORE_LOGIN_EXPERIENCE_V2", "off")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                NurError::Other(format!(
                    "failed to launch az ({e}). Install Azure CLI or paste AZURE_OPENAI_API_KEY."
                ))
            })?; // Best-effort parse device code from az stderr/stdout while waiting.
        if let Some(err) = child.stderr.take() {
            watch_login_output(err, tx.clone());
        }
        if let Some(out) = child.stdout.take() {
            watch_login_output(out, tx.clone());
        }
        if let Err(e) = wait_child_with_deadline(&mut child, cancel, VENDOR_CLI_LOGIN_TIMEOUT) {
            let _ = child.kill();
            let msg = e.to_string();
            if msg.contains("login process failed") {
                return Err(NurError::Other(
                    "az login failed - paste AZURE_OPENAI_API_KEY as fallback.".into(),
                ));
            }
            return Err(e);
        }
        send(
            tx,
            BrowserLoginProgress::Status("fetching Cognitive Services token…".into()),
        );
        fetch_token()
    }

    fn fetch_token() -> Result<OAuthTokens> {
        let az = az_bin().ok_or_else(|| {
            NurError::Other(
                "Azure CLI (`az`) not found. Install from https://aka.ms/installazurecliwindows or paste AZURE_OPENAI_API_KEY."
                    .into(),
            )
        })?;
        let out = Command::new(&az)
            .args([
                "account",
                "get-access-token",
                "--resource",
                "https://cognitiveservices.azure.com",
                "-o",
                "json",
            ])
            .output()
            .map_err(|e| {
                NurError::Other(format!(
                    "Azure CLI not available ({e}). Install `az`, run `az login`, or paste AZURE_OPENAI_API_KEY."
                ))
            })?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(NurError::Other(format!(
                "az get-access-token failed: {err}. Fix: `az login` then retry, or paste AZURE_OPENAI_API_KEY in /login."
            )));
        }
        // Prefer structured JSON (stable az contract).
        #[derive(Deserialize)]
        struct AzToken {
            #[serde(rename = "accessToken")]
            access_token: Option<String>,
            /// az's LOCAL-time string (every azure-cli version).
            #[serde(rename = "expiresOn")]
            expires_on_local: Option<String>,
            /// Timezone-independent unix seconds (azure-cli ≥ 2.54.0).
            #[serde(default)]
            expires_on_unix: Option<String>,
        }
        let parsed: AzToken = serde_json::from_slice(&out.stdout).map_err(|e| {
            NurError::Other(format!(
                "could not parse az JSON token output ({e}). Update Azure CLI or use API key path."
            ))
        })?;
        let access = parsed
            .access_token
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                NurError::Other(
                    "az returned empty accessToken. Run `az login` or paste AZURE_OPENAI_API_KEY."
                        .into(),
                )
            })?;
        let expires_at = az_expiry_unix(
            parsed.expires_on_unix.as_deref(),
            parsed.expires_on_local.as_deref(),
        );
        Ok(OAuthTokens {
            access_token: access,
            refresh_token: Some("az-cli".into()),
            expires_at,
            meta: Some(OauthMeta {
                issuer: "https://login.microsoftonline.com".into(),
                client_id: "azure-cli".into(),
                extra: serde_json::json!({"via": "az login"}),
            }),
        })
    }

    pub fn refresh() -> Result<OAuthTokens> {
        fetch_token()
    }
}

// ── AWS Bedrock (IAM Identity Center via `aws sso login`) ──────────────────

/// Parse the az CLI's two expiry fields into unix seconds.
///
/// `expires_on` (unix seconds, azure-cli ≥ 2.54.0) is timezone-independent
/// and preferred. `expiresOn` is az's LOCAL-time string — reading it as UTC
/// (the old behavior) skewed the stored expiry by the machine's UTC offset:
/// used hours-past-expiry on UTC+X, re-minted hours-early on UTC-X.
/// Scan az login output for the device code and surface it. az prints:
/// "To sign in, use a web browser to open the page
///  https://microsoft.com/devicelogin and enter the code XXXXXXXXX"
fn send_az_device_code(tx: &ProgressTx, buf: &str) {
    const URL: &str = "https://microsoft.com/devicelogin";
    let code = buf
        .split_whitespace()
        .find(|w| {
            w.len() >= 8
                && w.len() <= 15
                && w.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                && w.contains(|c: char| c.is_ascii_uppercase())
        })
        .unwrap_or("")
        .to_string();
    if !code.is_empty() {
        send(
            tx,
            BrowserLoginProgress::DeviceCode {
                verification_url: URL.into(),
                user_code: code,
            },
        );
    }
}

fn az_expiry_unix(expires_on_unix: Option<&str>, expires_on_local: Option<&str>) -> Option<u64> {
    if let Some(secs) = expires_on_unix.and_then(|s| s.parse::<i64>().ok()) {
        return Some(u64::try_from(secs.max(0)).unwrap_or(0));
    }
    let s = expires_on_local?.trim();
    let ndt = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S"))
        .ok()?;
    use chrono::TimeZone;
    chrono::Local
        .from_local_datetime(&ndt)
        .single()
        .map(|dt| u64::try_from(dt.timestamp().max(0)).unwrap_or(0))
}

pub mod bedrock {
    use super::*;

    pub fn refresh() -> Result<OAuthTokens> {
        if let Ok(token) = std::env::var("AWS_BEARER_TOKEN_BEDROCK") {
            if !token.is_empty() {
                return Ok(OAuthTokens {
                    access_token: token,
                    refresh_token: Some("aws-sso".into()),
                    expires_at: Some(super::super::now_unix() + 3600),
                    meta: None,
                });
            }
        }
        Err(NurError::Other(
            "AWS Bedrock refresh: re-run /login browser (aws sso login)".into(),
        ))
    }
}

// silence unused import warning for mpsc in some builds
fn _channel_ty() -> mpsc::Sender<u8> {
    let (tx, _) = mpsc::channel();
    tx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unsigned_jwt(payload: serde_json::Value) -> String {
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap());
        format!("header.{payload}.signature")
    }

    /// az's unix-seconds field must win, and `expiresOn` must be read as
    /// LOCAL time — the old code parsed it as UTC, skewing the stored expiry
    /// by the machine's UTC offset.
    #[test]
    fn az_expiry_prefers_unix_seconds_and_reads_local_time() {
        // Unix key wins over anything in the local-time field.
        assert_eq!(
            az_expiry_unix(Some("1900000000"), Some("1999-01-01 00:00:00")),
            Some(1_900_000_000)
        );
        // Negative unix values parse fine and mean "already expired" — they
        // must NOT fall through to None, which downstream means "never
        // expires" and would make a dead token immortal.
        assert_eq!(az_expiry_unix(Some("-5"), None), Some(0));
        // Local parse: a far-future local wall clock lands in the same year
        // regardless of the machine's offset (UTC reading would also pass
        // this; the regression it guards is the ±offset skew, which any
        // non-UTC machine would have caught in the old and_utc() behavior).
        let got = az_expiry_unix(None, Some("2030-06-01 12:00:00")).expect("local parse");
        let ics = chrono::DateTime::parse_from_rfc3339("2030-06-01T12:00:00+00:00")
            .unwrap()
            .timestamp();
        assert!(
            (got as i64 - ics).abs() < 36 * 3600,
            "local parse off by more than any timezone: {got} vs {ics}"
        );
        // Fractional seconds and garbage.
        assert!(az_expiry_unix(None, Some("2030-06-01 12:00:00.123")).is_some());
        assert_eq!(az_expiry_unix(None, Some("not-a-date")), None);
        assert_eq!(az_expiry_unix(None, None), None);
    }

    /// Vendor key imports must skip JWTs that are already expired (dead
    /// sessions used to import as success), and non-JWT keys never count as
    /// expired.
    #[test]
    fn jwt_expiry_and_expired_jwt() {
        let live = format!("x.{}.y", URL_SAFE_NO_PAD.encode(br#"{"exp":9999999999}"#));
        assert_eq!(jwt_expiry(&live), Some(9_999_999_999));
        assert!(!expired_jwt(&live));

        let dead = format!("x.{}.y", URL_SAFE_NO_PAD.encode(br#"{"exp":1}"#));
        assert!(expired_jwt(&dead));

        assert!(!expired_jwt("plain-opaque-vendor-key"));
        assert_eq!(jwt_expiry("plain-opaque-vendor-key"), None);
        assert!(!expired_jwt("only.two"));
    }

    /// az's device code is scanned from its output; both the positive case
    /// and the silent no-code case are covered.
    #[test]
    fn vendor_login_streams_code_before_exit_and_drains_after_url() {
        use std::net::{TcpListener, TcpStream};
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let mut writer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (reader, _) = listener.accept().unwrap();
        let (tx, rx) = mpsc::channel();
        let worker = thread::spawn(move || stream_login_output(reader, &tx, |_| {}));
        writer
            .write_all(b"! First copy your one-time code: ABCD-1234\n")
            .unwrap();
        let mut code_seen = false;
        for _ in 0..2 {
            if let BrowserLoginProgress::DeviceCode { user_code, .. } =
                rx.recv_timeout(Duration::from_secs(2)).unwrap()
            {
                assert_eq!(user_code, "ABCD-1234");
                code_seen = true;
            }
        }
        assert!(
            code_seen,
            "device code must arrive while the vendor is waiting"
        );
        writer
            .write_all(b"https://github.com/login/device\n")
            .unwrap();
        for _ in 0..2 {
            rx.recv_timeout(Duration::from_secs(2)).unwrap();
        }
        writer.write_all(b"authorization completed\n").unwrap();
        assert!(
            matches!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), BrowserLoginProgress::Status(s) if s == "authorization completed")
        );
        drop(writer);
        worker.join().unwrap();
    }

    #[test]
    fn azure_login_stream_extracts_code_without_ansi() {
        let (tx, rx) = mpsc::channel();
        stream_login_output(std::io::Cursor::new(b"To sign in visit https://microsoft.com/devicelogin and enter the code \x1b[32mA1B2C3D4E\x1b[0m\n"), &tx, |_| {});
        assert!(rx.try_iter().any(|event| matches!(event, BrowserLoginProgress::DeviceCode { user_code, .. } if user_code == "A1B2C3D4E")));
    }

    #[test]
    fn az_device_code_scan() {
        let (tx, rx) = mpsc::channel();
        send_az_device_code(
            &tx,
            "To sign in, use a web browser to open the page              https://microsoft.com/devicelogin and enter the code A1B2C3D4E",
        );
        match rx
            .recv_timeout(Duration::from_secs(1))
            .expect("device code")
        {
            BrowserLoginProgress::DeviceCode { user_code, .. } => {
                assert_eq!(user_code, "A1B2C3D4E")
            }
            other => panic!("expected DeviceCode, got {other:?}"),
        }

        let (tx2, rx2) = mpsc::channel();
        send_az_device_code(&tx2, "nothing resembling a code here");
        assert!(rx2.recv_timeout(Duration::from_millis(100)).is_err());
    }

    #[test]
    fn openai_id_token_yields_expiry_and_account_context() {
        let token = unsigned_jwt(serde_json::json!({
            "exp": 1_900_000_000_u64,
            "https://api.openai.com/auth": {
                "chatgpt_account_id": "acct_test",
                "chatgpt_account_is_fedramp": true
            }
        }));

        assert_eq!(jwt_expiration(&token), Some(1_900_000_000));
        assert_eq!(
            chatgpt_account_meta(&token),
            (Some("acct_test".to_string()), true)
        );
    }

    #[test]
    fn malformed_openai_id_token_has_no_account_context() {
        assert_eq!(jwt_expiration("not-a-jwt"), None);
        assert_eq!(chatgpt_account_meta("not-a-jwt"), (None, false));
    }

    #[test]
    fn oauth_callback_requires_state_when_flow_declares_one() {
        assert!(validate_callback_state(Some("expected"), Some("expected")).is_ok());
        assert!(validate_callback_state(Some("expected"), Some("wrong")).is_err());
        assert!(validate_callback_state(Some("expected"), None).is_err());
        assert!(validate_callback_state(None, None).is_ok());
    }

    #[test]
    fn oauth_error_summary_never_echoes_token_fields() {
        let body = r#"{"access_token":"secret-access","refresh_token":"secret-refresh","error":{"message":"invalid grant"}}"#;
        let summary = oauth_error_summary(body);
        assert_eq!(summary, "invalid grant");
        assert!(!summary.contains("secret"));
        assert_eq!(
            oauth_error_summary(r#"{"access_token":"secret-only"}"#),
            "response details withheld"
        );
    }

    #[test]
    fn imports_current_codex_auth_shape_without_exposing_api_key_field() {
        let access = unsigned_jwt(serde_json::json!({"exp": 1_900_000_000_u64}));
        let id = unsigned_jwt(serde_json::json!({
            "https://api.openai.com/auth": {
                "chatgpt_account_id": "claim-account",
                "chatgpt_account_is_fedramp": false
            }
        }));
        let text = serde_json::json!({
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": null,
            "tokens": {
                "id_token": id,
                "access_token": access,
                "refresh_token": "refresh-test",
                "account_id": "file-account"
            }
        })
        .to_string();

        let tokens = openai::codex_tokens_from_json(&text).unwrap();
        assert_eq!(tokens.expires_at, Some(1_900_000_000));
        assert_eq!(tokens.refresh_token.as_deref(), Some("refresh-test"));
        assert_eq!(
            tokens.meta.unwrap().extra["account_id"],
            serde_json::json!("file-account")
        );
    }
}

//! Per-provider browser / device-code / external-CLI login flows.

use super::{expires_in_to_at, open_browser, CancelFlag};
use crate::auth::{Auth, OauthMeta};
use crate::error::{MuseError, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
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

/// Run browser login for `provider_id` on a background-friendly thread path.
/// Blocks until success, failure, cancel, or timeout.
pub fn login_browser(provider_id: &str, tx: ProgressTx, cancel: CancelFlag) {
    let result = match provider_id {
        "openai" => openai::login(&tx, &cancel),
        "xai" => xai::login(&tx, &cancel),
        "kimi" => kimi::login(&tx, &cancel),
        "anthropic" => claude::login(&tx, &cancel),
        "google" => google::login(&tx, &cancel),
        "azure" => azure::login(&tx, &cancel),
        "github-models" | "github-copilot" => github::login(provider_id, &tx, &cancel),
        other => Err(MuseError::Other(format!(
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

/// Import tokens from a first-party CLI session file when present.
pub fn import_existing_session(provider_id: &str) -> Result<Option<OAuthTokens>> {
    match provider_id {
        "openai" => openai::import_codex_cli(),
        "xai" => xai::import_grok_cli(),
        "kimi" => kimi::import_kimi_cli(),
        "anthropic" => claude::import_claude_cli(),
        "huggingface" => Ok(huggingface::import_hf_token()),
        "cursor" => cursor::import_cursor_cli(),
        "opencode" => opencode::import_opencode_cli(),
        _ => Ok(None),
    }
}

pub mod cursor {
    use super::*;
    use std::path::PathBuf;

    pub fn import_cursor_cli() -> Result<Option<OAuthTokens>> {
        // t3code-style probing: respect CURSOR_AGENT_HOME, then ~/.cursor, then ~/.config/cursor
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let candidates = [
            std::env::var("CURSOR_AGENT_HOME")
                .ok()
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".cursor")),
            home.join(".cursor"),
            home.join(".config").join("cursor"),
        ];
        for dir in candidates {
            if !dir.exists() {
                continue;
            }
            // Cursor stores auth in various files — probe without reading secrets if possible
            for file in ["auth.json", "config.json", "mcp.json"] {
                let p = dir.join(file);
                if p.exists() {
                    if let Ok(text) = std::fs::read_to_string(&p) {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                            // Try to find api key or token
                            if let Some(key) = v
                                .get("api_key")
                                .or_else(|| v.get("apiKey"))
                                .or_else(|| v.get("token"))
                                .or_else(|| v.get("access_token"))
                                .and_then(|x| x.as_str())
                            {
                                if !key.trim().is_empty() {
                                    return Ok(Some(OAuthTokens {
                                        access_token: key.to_string(),
                                        refresh_token: None,
                                        expires_at: None,
                                        meta: Some(OauthMeta {
                                            issuer: "cursor".into(),
                                            client_id: "cursor-cli".into(),
                                            extra: serde_json::json!({"imported_from": "cursor-cli", "path": p.display().to_string()}),
                                        }),
                                    }));
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(None)
    }
}

pub mod opencode {
    use super::*;
    use std::path::PathBuf;

    pub fn import_opencode_cli() -> Result<Option<OAuthTokens>> {
        // t3code-style: OPENCODE_HOME, then ~/.config/opencode
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let candidates = [
            std::env::var("OPENCODE_HOME")
                .ok()
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".config").join("opencode")),
            home.join(".config").join("opencode"),
            home.join(".opencode"),
        ];
        for dir in candidates {
            if !dir.exists() {
                continue;
            }
            for file in ["auth.json", "config.json", "opencode.json"] {
                let p = dir.join(file);
                if p.exists() {
                    if let Ok(text) = std::fs::read_to_string(&p) {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                            if let Some(key) = v
                                .get("api_key")
                                .or_else(|| v.get("apiKey"))
                                .or_else(|| v.get("token"))
                                .or_else(|| v.get("access_token"))
                                .and_then(|x| x.as_str())
                            {
                                if !key.trim().is_empty() {
                                    return Ok(Some(OAuthTokens {
                                        access_token: key.to_string(),
                                        refresh_token: None,
                                        expires_at: None,
                                        meta: Some(OauthMeta {
                                            issuer: "opencode".into(),
                                            client_id: "opencode-cli".into(),
                                            extra: serde_json::json!({"imported_from": "opencode-cli", "path": p.display().to_string()}),
                                        }),
                                    }));
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(None)
    }
}

fn random_urlsafe(nbytes: usize) -> String {
    let mut raw = Vec::with_capacity(nbytes);
    while raw.len() < nbytes {
        raw.extend_from_slice(Uuid::new_v4().as_bytes());
    }
    raw.truncate(nbytes);
    URL_SAFE_NO_PAD.encode(&raw)
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
        .map_err(|e| MuseError::Other(e.to_string()))
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

#[allow(dead_code)] // legacy Bedrock SSO helper; bearer-only catalog path is advertised
fn aws_bin() -> Option<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(windows)]
    {
        if let Ok(pf) = std::env::var("ProgramFiles") {
            dirs.push(PathBuf::from(pf).join("Amazon").join("AWSCLIV2"));
        }
    }
    resolve_cli("aws", &["aws.cmd", "aws.exe"], &dirs)
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
/// Minimal localhost OAuth callback: waits for `?code=` (and optional state).
fn wait_localhost_code_on(
    listener: TcpListener,
    expected_state: Option<&str>,
    cancel: &CancelFlag,
    timeout: Duration,
) -> Result<String> {
    listener
        .set_nonblocking(true)
        .map_err(|e| MuseError::Other(e.to_string()))?;
    let start = std::time::Instant::now();
    loop {
        if cancel.is_cancelled() {
            return Err(MuseError::Other("login cancelled".into()));
        }
        if start.elapsed() > timeout {
            return Err(MuseError::Other("browser login timed out".into()));
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let line = req.lines().next().unwrap_or("");
                // GET /callback?code=...&state=... HTTP/1.1
                let path = line.split_whitespace().nth(1).unwrap_or("");
                let q = path.split('?').nth(1).unwrap_or("");
                let mut code = None;
                let mut state = None;
                for pair in q.split('&') {
                    let mut it = pair.splitn(2, '=');
                    let k = it.next().unwrap_or("");
                    let v = it.next().unwrap_or("");
                    match k {
                        "code" => code = Some(urlencoding_decode(v)),
                        "state" => state = Some(urlencoding_decode(v)),
                        _ => {}
                    }
                }
                let body = if code.is_some() {
                    "<html><body><h2>Signed in — you can close this tab and return to NurCLI.</h2></body></html>"
                } else {
                    "<html><body><h2>Missing code — try again from NurCLI.</h2></body></html>"
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
                if let Some(c) = code {
                    if let (Some(exp), Some(got)) = (expected_state, state.as_deref()) {
                        if exp != got {
                            return Err(MuseError::Other("OAuth state mismatch".into()));
                        }
                    }
                    return Ok(c);
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(150));
            }
            Err(e) => return Err(MuseError::Other(format!("callback accept: {e}"))),
        }
    }
}

fn urlencoding_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => {
                out.push(' ');
                i += 1;
            }
            b'%' if i + 2 < b.len() => {
                let hex = &s[i + 1..i + 3];
                if let Ok(v) = u8::from_str_radix(hex, 16) {
                    out.push(v as char);
                    i += 3;
                } else {
                    out.push('%');
                    i += 1;
                }
            }
            c => {
                out.push(c as char);
                i += 1;
            }
        }
    }
    out
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
                MuseError::Other(
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
            .map_err(|error| MuseError::Other(format!("OpenAI token exchange failed: {error}")))?;
        parse_token_response(response, None, None)
    }

    pub fn refresh(auth: &Auth, refresh_token: &str) -> Result<OAuthTokens> {
        let body = serde_json::json!({
            "client_id": CLIENT_ID,
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
        });
        let response = http()?
            .post(format!("{ISSUER}/oauth/token"))
            .json(&body)
            .send()
            .map_err(|error| MuseError::Other(format!("OpenAI token refresh failed: {error}")))?;
        parse_token_response(response, Some(refresh_token), auth.oauth_meta.clone())
    }

    /// Reuse the official Codex CLI login when present. This reads only the
    /// first-party token cache and converts it into Nur's normal OAuth shape.
    pub fn import_codex_cli() -> Result<Option<OAuthTokens>> {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let path = home.join(".codex").join("auth.json");
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
            .map_err(|error| MuseError::Other(format!("invalid Codex auth file: {error}")))?;
        let access_token = parsed.tokens.access_token.trim().to_string();
        if access_token.is_empty() {
            return Err(MuseError::Other(
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
            MuseError::Other(format!(
                "OpenAI returned an invalid token response ({status}): {error}"
            ))
        })?;
        if !status.is_success() {
            let detail = parsed
                .error
                .map(|value| value.to_string())
                .unwrap_or_else(|| format!("HTTP {}", status.as_u16()));
            return Err(MuseError::Other(format!("OpenAI OAuth failed: {detail}")));
        }
        let access_token = parsed
            .access_token
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                MuseError::Other("OpenAI OAuth response did not include an access token".into())
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
                            Err(e) => last_err = format!("parse device code: {e} · body={body}"),
                        }
                    } else {
                        last_err = format!("{url} → {status}: {body}");
                    }
                }
                Err(e) => last_err = e.to_string(),
            }
        }
        let device = device.ok_or_else(|| {
            MuseError::Other(format!(
                "xAI device code failed ({last_err}). Paste an XAI_API_KEY or sign in with the Grok CLI first."
            ))
        })?;

        let verify = device
            .verification_uri_complete
            .clone()
            .or_else(|| {
                device
                    .verification_uri
                    .clone()
                    .map(|u| format!("{u}?user_code={}", device.user_code))
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
        let mut attempt = 0u32;
        let mut slow = false;

        while std::time::Instant::now() < deadline {
            if cancel.is_cancelled() {
                return Err(MuseError::Other("login cancelled".into()));
            }
            thread::sleep(crate::oauth::device_poll_sleep(
                base_interval,
                slow,
                attempt,
            ));
            attempt = attempt.saturating_add(1);
            slow = false;
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
                if let Some(err) = parsed.error.as_deref() {
                    if err == "authorization_pending" {
                        continue;
                    }
                    if err == "slow_down" {
                        slow = true;
                        continue;
                    }
                    if err == "parse" {
                        continue;
                    }
                    return Err(MuseError::Other(format!(
                        "xAI token error: {err} {}",
                        parsed.error_description.unwrap_or_default()
                    )));
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
            send(
                tx,
                BrowserLoginProgress::Status("waiting for browser approval…".into()),
            );
        }
        Err(MuseError::Other("xAI device login timed out".into()))
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
            .map_err(|e| MuseError::Other(e.to_string()))?;
        let body = res.text().unwrap_or_default();
        let parsed: TokenResp =
            serde_json::from_str(&body).map_err(|e| MuseError::Other(format!("{e}: {body}")))?;
        let access = parsed
            .access_token
            .ok_or_else(|| MuseError::Other(format!("refresh failed: {body}")))?;
        Ok(OAuthTokens {
            access_token: access,
            refresh_token: parsed.refresh_token.or_else(|| Some(refresh.to_string())),
            expires_at: expires_in_to_at(parsed.expires_in),
            meta: auth.oauth_meta.clone(),
        })
    }

    pub fn import_grok_cli() -> Result<Option<OAuthTokens>> {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let path = home.join(".grok").join("auth.json");
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path)?;
        let v: serde_json::Value = serde_json::from_str(&text)?;
        // Map of "issuer::client_id" → session object.
        if let Some(map) = v.as_object() {
            for (_k, sess) in map {
                let access = sess
                    .get("key")
                    .or_else(|| sess.get("access_token"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("");
                if access.is_empty() {
                    continue;
                }
                let refresh = sess
                    .get("refresh_token")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let expires_at = sess
                    .get("expires_at")
                    .and_then(|x| x.as_str())
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.timestamp() as u64);
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
                return Ok(Some(OAuthTokens {
                    access_token: access.to_string(),
                    refresh_token: refresh,
                    expires_at,
                    meta: Some(OauthMeta {
                        issuer,
                        client_id,
                        extra: serde_json::json!({"imported_from": "grok-cli"}),
                    }),
                }));
            }
        }
        Ok(None)
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

    fn kimi_share_dir() -> PathBuf {
        std::env::var("KIMI_SHARE_DIR")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|home| home.join(".kimi")))
            .unwrap_or_else(|| PathBuf::from(".kimi"))
    }

    fn device_id() -> Result<String> {
        // Reuse the first-party CLI identity when present. Otherwise keep a
        // Nur-specific stable id so polls and refreshes describe one device.
        let kimi_path = kimi_share_dir().join("device_id");
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
            .map_err(|e| MuseError::Other(format!("failed to save Kimi device id: {e}")))?;
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
            .ok_or_else(|| MuseError::Other("Kimi token response omitted access_token".into()))?;
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
            .map_err(|e| MuseError::Other(format!("Kimi device authorization failed: {e}")))?;
        let status = response.status();
        let body = response.text().unwrap_or_default();
        if !status.is_success() {
            return Err(MuseError::Other(format!(
                "Kimi device authorization failed (HTTP {})",
                status.as_u16()
            )));
        }
        let device: DeviceCodeResp = serde_json::from_str(&body)
            .map_err(|e| MuseError::Other(format!("invalid Kimi device response: {e}")))?;
        if device.device_code.trim().is_empty() || device.user_code.trim().is_empty() {
            return Err(MuseError::Other(
                "Kimi device response omitted the authorization code".into(),
            ));
        }
        let verification_url = if !device.verification_uri_complete.trim().is_empty() {
            device.verification_uri_complete.clone()
        } else {
            device.verification_uri.clone()
        };
        if verification_url.trim().is_empty() {
            return Err(MuseError::Other(
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
        let mut attempt = 0u32;
        let mut slow_down = false;
        while std::time::Instant::now() < deadline {
            if cancel.is_cancelled() {
                return Err(MuseError::Other("login cancelled".into()));
            }
            thread::sleep(crate::oauth::device_poll_sleep(
                interval, slow_down, attempt,
            ));
            attempt = attempt.saturating_add(1);
            slow_down = false;
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
                return Err(MuseError::Other(format!(
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
                Some("slow_down") => slow_down = true,
                Some("expired_token") => {
                    return Err(MuseError::Other(
                        "Kimi device code expired; start browser sign-in again".into(),
                    ));
                }
                Some("access_denied") => {
                    return Err(MuseError::Other("Kimi authorization was denied".into()));
                }
                Some(_) if status >= 500 || status == 429 => continue,
                Some(_) => {
                    return Err(MuseError::Other(format!(
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
        Err(MuseError::Other("Kimi device login timed out".into()))
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
            .map_err(|e| MuseError::Other(format!("Kimi token refresh failed: {e}")))?;
        let status = response.status().as_u16();
        let body = response.text().unwrap_or_default();
        let parsed: TokenResp = serde_json::from_str(&body).map_err(|_| {
            MuseError::Other(format!("invalid Kimi refresh response (HTTP {status})"))
        })?;
        if !(200..300).contains(&status) || parsed.access_token.is_none() {
            return Err(MuseError::Other(format!(
                "Kimi token refresh failed: {}",
                token_error(&parsed, status)
            )));
        }
        into_tokens(parsed, Some(refresh), auth.oauth_meta.clone())
    }

    pub fn import_kimi_cli() -> Result<Option<OAuthTokens>> {
        let path = kimi_share_dir().join("credentials").join("kimi-code.json");
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
    /// Prefer these loopback ports (Claude Code uses ephemeral; we pin a few).
    const CALLBACK_PORTS: &[u16] = &[54545, 54546, 54547, 21865];

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
        let form = [
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect),
            ("client_id", CLIENT_ID),
            ("code_verifier", verifier),
            ("state", state),
        ];
        let mut last = String::new();
        for url in [TOKEN_URL, "https://api.anthropic.com/v1/oauth/token"] {
            let res = match client
                .post(url)
                .header(
                    "Content-Type",
                    "application/x-www-form-urlencoded;charset=utf-8",
                )
                .form(&form)
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
                    last = format!("{e}: {body}");
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
            last = body;
        }
        Err(MuseError::Other(format!(
            "Claude token exchange failed: {last}"
        )))
    }

    pub fn login(tx: &ProgressTx, cancel: &CancelFlag) -> Result<OAuthTokens> {
        // Prefer an already-signed-in Claude Code session (no browser needed).
        if let Ok(Some(imported)) = import_claude_cli() {
            send(
                tx,
                BrowserLoginProgress::Status("using existing Claude Code session".into()),
            );
            return Ok(imported);
        }

        let verifier = random_urlsafe(32);
        let challenge = pkce_challenge(&verifier);
        let state = random_urlsafe(16);

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

        let code = wait_manual_code_paste(cancel, Duration::from_secs(600))?;
        send(
            tx,
            BrowserLoginProgress::Status("exchanging Claude authorization code…".into()),
        );
        exchange_code(&code, MANUAL_REDIRECT, &verifier, &state)
    }

    /// Read a one-line authorization code from stdin (manual Claude flow).
    /// The TUI also stores pastes into the login key buffer when we surface
    /// DeviceCode; for the background thread we poll a small file drop zone
    /// under ~/.nur so the TUI can write the pasted code without sharing stdin.
    fn wait_manual_code_paste(cancel: &CancelFlag, timeout: Duration) -> Result<String> {
        let path = crate::config::nur_home().join("oauth_paste_code.txt");
        let _ = std::fs::remove_file(&path);
        let start = std::time::Instant::now();
        loop {
            if cancel.is_cancelled() {
                let _ = std::fs::remove_file(&path);
                return Err(MuseError::Other("login cancelled".into()));
            }
            if start.elapsed() > timeout {
                let _ = std::fs::remove_file(&path);
                return Err(MuseError::Other(
                    "Claude login timed out waiting for pasted code".into(),
                ));
            }
            if let Ok(text) = std::fs::read_to_string(&path) {
                let code = text.trim().to_string();
                let _ = std::fs::remove_file(&path);
                if !code.is_empty() {
                    return Ok(code);
                }
            }
            thread::sleep(Duration::from_millis(200));
        }
    }

    pub fn refresh(refresh: &str) -> Result<OAuthTokens> {
        let client = http()?;
        let form = [
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh),
            ("client_id", CLIENT_ID),
        ];
        for url in [TOKEN_URL, "https://api.anthropic.com/v1/oauth/token"] {
            let Ok(res) = client
                .post(url)
                .header(
                    "Content-Type",
                    "application/x-www-form-urlencoded;charset=utf-8",
                )
                .form(&form)
                .send()
            else {
                continue;
            };
            let body = res.text().unwrap_or_default();
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
        Err(MuseError::Other("Claude token refresh failed".into()))
    }

    pub fn import_claude_cli() -> Result<Option<OAuthTokens>> {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let candidates = [
            home.join(".claude").join(".credentials.json"),
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
            let oauth = v
                .get("claudeAiOauth")
                .or_else(|| v.get("claude_ai_oauth"))
                .cloned();
            let Some(oauth) = oauth else {
                continue;
            };
            let access = oauth
                .get("accessToken")
                .or_else(|| oauth.get("access_token"))
                .and_then(|x| x.as_str())
                .unwrap_or("");
            if access.is_empty() {
                continue;
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
                    if let Some(n) = x.as_u64() {
                        Some(if n > 10_000_000_000 { n / 1000 } else { n })
                    } else {
                        None
                    }
                });
            return Ok(Some(OAuthTokens {
                access_token: access.to_string(),
                refresh_token: refresh,
                expires_at,
                meta: Some(OauthMeta {
                    issuer: "https://claude.ai".into(),
                    client_id: CLIENT_ID.into(),
                    extra: serde_json::json!({"imported_from": "claude-code"}),
                }),
            }));
        }
        Ok(None)
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
        send(
            tx,
            BrowserLoginProgress::OpenUrl("https://accounts.google.com/".into()),
        );
        let gcloud = gcloud_bin().ok_or_else(|| {
            MuseError::Other(
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
                MuseError::Other(format!(
                    "failed to launch gcloud ({e}). Install Google Cloud SDK, or choose “Enter API key” with a Gemini key."
                ))
            })?;
        // Surface any https URL from gcloud stderr (device / browser flow).
        if let Some(mut err) = child.stderr.take() {
            let tx2 = tx.clone();
            thread::spawn(move || {
                let mut buf = String::new();
                let _ = err.read_to_string(&mut buf);
                for word in buf.split_whitespace() {
                    if word.starts_with("https://") {
                        send(&tx2, BrowserLoginProgress::OpenUrl(word.to_string()));
                        let _ = open_browser(word);
                        break;
                    }
                }
                // Device-code style lines from older gcloud
                if buf.contains("enter the code") || buf.contains("verification code") {
                    send(
                        &tx2,
                        BrowserLoginProgress::Status(buf.chars().take(240).collect()),
                    );
                }
            });
        }
        loop {
            if cancel.is_cancelled() {
                let _ = child.kill();
                return Err(MuseError::Other("login cancelled".into()));
            }
            match child.try_wait() {
                Ok(Some(status)) if status.success() => break,
                Ok(Some(status)) => {
                    let message = format!(
                        "gcloud auth login failed (exit {status}). Paste a Gemini API key as fallback."
                    );
                    return Err(MuseError::Other(message));
                }
                Ok(None) => thread::sleep(Duration::from_millis(200)),
                Err(e) => return Err(MuseError::Other(e.to_string())),
            }
        }
        send(
            tx,
            BrowserLoginProgress::Status("fetching Google access token…".into()),
        );
        fetch_access_token()
    }

    fn fetch_access_token() -> Result<OAuthTokens> {
        let gcloud = gcloud_bin().ok_or_else(|| {
            MuseError::Other(
                "gcloud not found. Install Google Cloud SDK (https://cloud.google.com/sdk/docs/install) and ensure it is on PATH."
                    .into(),
            )
        })?;
        let out = Command::new(&gcloud)
            .args(["auth", "application-default", "print-access-token"])
            .output()
            .map_err(|e| MuseError::Other(format!("gcloud ADC print-access-token: {e}")))?;
        if !out.status.success() {
            return Err(MuseError::Other(format!(
                "gcloud application-default print-access-token failed: {}",
                String::from_utf8_lossy(&out.stderr)
            )));
        }
        let access = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if access.is_empty() {
            return Err(MuseError::Other("empty token from gcloud".into()));
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
                MuseError::Other(
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
            MuseError::Other(
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
                MuseError::Other(format!(
                    "failed to launch gh ({e}). Install GitHub CLI, or paste a GitHub PAT (models:read)."
                ))
            })?;
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(b"\n\n");
        }
        // Surface the one-time code + verification URL from gh's stderr.
        if let Some(mut err) = child.stderr.take() {
            let tx2 = tx.clone();
            thread::spawn(move || {
                let mut buf = String::new();
                let _ = err.read_to_string(&mut buf);
                for word in buf.split_whitespace() {
                    if word.starts_with("https://") {
                        send(&tx2, BrowserLoginProgress::OpenUrl(word.to_string()));
                        let _ = open_browser(word);
                        break;
                    }
                }
                if let Some(idx) = buf.find("one-time code:") {
                    let code: String = buf[idx..].chars().take(40).collect();
                    send(&tx2, BrowserLoginProgress::Status(code));
                }
            });
        }
        loop {
            if cancel.is_cancelled() {
                let _ = child.kill();
                return Err(MuseError::Other("login cancelled".into()));
            }
            match child.try_wait() {
                Ok(Some(status)) if status.success() => break,
                Ok(Some(status)) => {
                    return Err(MuseError::Other(format!(
                        "gh auth login failed (exit {status}). Paste a GitHub PAT (models:read) as fallback."
                    )))
                }
                Ok(None) => thread::sleep(Duration::from_millis(200)),
                Err(e) => return Err(MuseError::Other(e.to_string())),
            }
        }
        send(
            tx,
            BrowserLoginProgress::Status("fetching GitHub token…".into()),
        );
        fetch_token(provider_id)
    }

    fn fetch_token(provider_id: &str) -> Result<OAuthTokens> {
        let gh = gh_bin().ok_or_else(|| {
            MuseError::Other(
                "gh not found. Install GitHub CLI (https://cli.github.com/) and ensure it is on PATH."
                    .into(),
            )
        })?;
        let out = Command::new(&gh)
            .args(["auth", "token", "--hostname", "github.com"])
            .output()
            .map_err(|e| MuseError::Other(format!("gh auth token: {e}")))?;
        if !out.status.success() {
            return Err(MuseError::Other(format!(
                "gh auth token failed: {}",
                String::from_utf8_lossy(&out.stderr)
            )));
        }
        let access = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if access.is_empty() {
            return Err(MuseError::Other("empty token from gh".into()));
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
                    .map_err(|error| MuseError::Other(error.to_string()))
            })
            .is_ok_and(|response| response.status().is_success())
    }

    pub fn refresh(auth: &Auth, _refresh: &str) -> Result<OAuthTokens> {
        fetch_token(&auth.provider)
    }
}

// ── Hugging Face (device code — same spirit as `hf auth login`) ────────────

#[allow(dead_code)] // optional custom-client OAuth remains import-compatible but is not advertised
pub mod huggingface {
    use super::*;

    #[derive(Deserialize)]
    struct DeviceCodeResp {
        #[serde(default)]
        device_code: String,
        #[serde(default)]
        user_code: String,
        #[serde(default)]
        verification_uri: String,
        #[serde(default)]
        verification_uri_complete: Option<String>,
        #[serde(default)]
        expires_in: u64,
        #[serde(default = "default_interval")]
        interval: u64,
        // Some HF endpoints nest under different shapes.
        #[serde(default)]
        #[allow(dead_code)]
        request_id: Option<String>,
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
        #[allow(dead_code)]
        error_description: Option<String>,
        // HF classic: {"token":"..."}
        token: Option<String>,
    }

    pub fn login(tx: &ProgressTx, cancel: &CancelFlag) -> Result<OAuthTokens> {
        send(
            tx,
            BrowserLoginProgress::Status("starting Hugging Face device login…".into()),
        );
        let client = http()?;

        // Prefer an existing huggingface-cli / hub token (no OAuth app required).
        if let Some(tokens) = import_hf_token() {
            send(
                tx,
                BrowserLoginProgress::Status("using existing Hugging Face token from disk".into()),
            );
            return Ok(tokens);
        }

        // Official device endpoint is POST /oauth/device (not /oauth/device/code).
        // Client id must be a registered OAuth app — set NUR_HF_OAUTH_CLIENT_ID, or
        // paste a token via /login key path.
        let client_id = std::env::var("NUR_HF_OAUTH_CLIENT_ID")
            .or_else(|_| std::env::var("HF_OAUTH_CLIENT_ID"))
            .unwrap_or_default();
        let client_id = client_id.trim().to_string();
        let mut device: Option<DeviceCodeResp> = None;
        let mut last = String::new();
        if !client_id.is_empty() {
            let device_endpoints = [
                "https://huggingface.co/oauth/device",
                "https://huggingface.co/oauth/device/code",
            ];
            for url in device_endpoints {
                let form = [
                    ("client_id", client_id.as_str()),
                    ("scope", "openid profile email"),
                ];
                match client.post(url).form(&form).send() {
                    Ok(res) => {
                        let status = res.status();
                        let body = res.text().unwrap_or_default();
                        if status.is_success() {
                            if let Ok(d) = serde_json::from_str::<DeviceCodeResp>(&body) {
                                if !d.user_code.is_empty() || !d.device_code.is_empty() {
                                    device = Some(d);
                                    break;
                                }
                            }
                            last = body;
                        } else {
                            last = format!("{status}: {body}");
                        }
                    }
                    Err(e) => last = e.to_string(),
                }
            }
        } else {
            last = "no NUR_HF_OAUTH_CLIENT_ID (register an OAuth app at huggingface.co/settings/applications)".into();
        }

        if let Some(device) = device {
            let verify = device
                .verification_uri_complete
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| {
                    if device.verification_uri.is_empty() {
                        format!(
                            "https://huggingface.co/login/device?user_code={}",
                            device.user_code
                        )
                    } else {
                        device.verification_uri.clone()
                    }
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
            let base_interval = device.interval.max(3);
            let mut attempt = 0u32;
            let mut slow = false;
            while std::time::Instant::now() < deadline {
                if cancel.is_cancelled() {
                    return Err(MuseError::Other("login cancelled".into()));
                }
                thread::sleep(crate::oauth::device_poll_sleep(
                    base_interval,
                    slow,
                    attempt,
                ));
                attempt = attempt.saturating_add(1);
                slow = false;
                let form = [
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ("device_code", device.device_code.as_str()),
                    ("client_id", client_id.as_str()),
                ];
                for turl in [
                    "https://huggingface.co/oauth/token",
                    "https://huggingface.co/api/oauth/token",
                ] {
                    let Ok(res) = client.post(turl).form(&form).send() else {
                        continue;
                    };
                    let body = res.text().unwrap_or_default();
                    let parsed: TokenResp = serde_json::from_str(&body).unwrap_or(TokenResp {
                        access_token: None,
                        refresh_token: None,
                        expires_in: None,
                        error: Some("pending".into()),
                        error_description: None,
                        token: None,
                    });
                    if let Some(err) = parsed.error.as_deref() {
                        if err == "authorization_pending" || err == "pending" {
                            continue;
                        }
                        if err == "slow_down" {
                            slow = true;
                            continue;
                        }
                    }
                    if let Some(access) = parsed.access_token.or(parsed.token) {
                        return Ok(OAuthTokens {
                            access_token: access,
                            refresh_token: parsed.refresh_token,
                            expires_at: expires_in_to_at(parsed.expires_in),
                            meta: Some(OauthMeta {
                                issuer: "https://huggingface.co".into(),
                                client_id: client_id.clone(),
                                extra: serde_json::json!({}),
                            }),
                        });
                    }
                }
                send(
                    tx,
                    BrowserLoginProgress::Status("waiting for Hugging Face approval…".into()),
                );
            }
            return Err(MuseError::Other("Hugging Face login timed out".into()));
        }

        // Fallback: open token page and instruct user to use API key path.
        let url = "https://huggingface.co/settings/tokens";
        send(tx, BrowserLoginProgress::OpenUrl(url.into()));
        Err(MuseError::Other(format!(
            "HF device flow unavailable ({last}). Open {url}, create a token (or set HF_TOKEN), then choose “Enter API key” in /login. Optional: register an OAuth app and set NUR_HF_OAUTH_CLIENT_ID for browser device flow."
        )))
    }

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

    pub fn refresh(_refresh: &str) -> Result<OAuthTokens> {
        if let Some(t) = import_hf_token() {
            return Ok(t);
        }
        Err(MuseError::Other(
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
            MuseError::Other(
                "Azure CLI (`az`) not found on PATH. Install from https://aka.ms/installazurecliwindows, open a new terminal, then retry — or paste AZURE_OPENAI_API_KEY via /login."
                    .into(),
            )
        })?;
        let mut child = Command::new(&az)
            .args(["login", "--use-device-code"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                MuseError::Other(format!(
                    "failed to launch az ({e}). Install Azure CLI or paste AZURE_OPENAI_API_KEY."
                ))
            })?; // Best-effort parse device code from az stderr/stdout while waiting.
        let stderr = child.stderr.take();
        if let Some(mut err) = stderr {
            let tx2 = tx.clone();
            thread::spawn(move || {
                let mut buf = String::new();
                let _ = err.read_to_string(&mut buf);
                // az prints: To sign in, use a web browser to open the page https://microsoft.com/devicelogin
                // and enter the code XXXXXXXXX
                let url = "https://microsoft.com/devicelogin";
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
                        &tx2,
                        BrowserLoginProgress::DeviceCode {
                            verification_url: url.into(),
                            user_code: code,
                        },
                    );
                }
            });
        }
        loop {
            if cancel.is_cancelled() {
                let _ = child.kill();
                return Err(MuseError::Other("login cancelled".into()));
            }
            match child.try_wait() {
                Ok(Some(status)) if status.success() => break,
                Ok(Some(status)) => {
                    return Err(MuseError::Other(format!(
                        "az login failed (exit {status}). Paste AZURE_OPENAI_API_KEY as fallback."
                    )))
                }
                Ok(None) => thread::sleep(Duration::from_millis(200)),
                Err(e) => return Err(MuseError::Other(e.to_string())),
            }
        }
        send(
            tx,
            BrowserLoginProgress::Status("fetching Cognitive Services token…".into()),
        );
        fetch_token()
    }

    fn fetch_token() -> Result<OAuthTokens> {
        let az = az_bin().ok_or_else(|| {
            MuseError::Other(
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
                MuseError::Other(format!(
                    "Azure CLI not available ({e}). Install `az`, run `az login`, or paste AZURE_OPENAI_API_KEY."
                ))
            })?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(MuseError::Other(format!(
                "az get-access-token failed: {err}. Fix: `az login` then retry, or paste AZURE_OPENAI_API_KEY in /login."
            )));
        }
        // Prefer structured JSON (stable az contract).
        #[derive(Deserialize)]
        struct AzToken {
            #[serde(rename = "accessToken")]
            access_token: Option<String>,
            #[serde(rename = "expiresOn")]
            expires_on: Option<String>,
            #[serde(default)]
            expires_on_ts: Option<String>,
        }
        let parsed: AzToken = serde_json::from_slice(&out.stdout).map_err(|e| {
            MuseError::Other(format!(
                "could not parse az JSON token output ({e}). Update Azure CLI or use API key path."
            ))
        })?;
        let access = parsed
            .access_token
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                MuseError::Other(
                    "az returned empty accessToken. Run `az login` or paste AZURE_OPENAI_API_KEY."
                        .into(),
                )
            })?;
        let expires_at = parsed
            .expires_on
            .as_deref()
            .and_then(|s| {
                chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f")
                    .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S"))
                    .ok()
            })
            .map(|ndt| ndt.and_utc().timestamp() as u64)
            .or_else(|| parsed.expires_on_ts.as_deref().and_then(|s| s.parse().ok()));
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

#[allow(dead_code)] // legacy sessions only; SigV4 is not supported by Nur's bearer transport
pub mod bedrock {
    use super::*;

    pub fn login(tx: &ProgressTx, cancel: &CancelFlag) -> Result<OAuthTokens> {
        send(
            tx,
            BrowserLoginProgress::Status("launching AWS SSO login (aws sso login)…".into()),
        );
        send(
            tx,
            BrowserLoginProgress::Status(
                "complete browser SSO when prompted by the AWS CLI…".into(),
            ),
        );
        // Prefer sso login; fall back to `aws login` if present.
        let mut ok = false;
        let mut last = String::new();
        let aws = match aws_bin() {
            Some(p) => p,
            None => {
                return Err(MuseError::Other(
                    "AWS CLI (`aws`) not found on PATH. Install AWS CLI v2 (https://aws.amazon.com/cli/), configure SSO, then retry — or paste a Bedrock bearer/API key."
                        .into(),
                ));
            }
        };
        for args in [vec!["sso", "login"], vec!["login"]] {
            let mut child = match Command::new(&aws)
                .args(&args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    last = format!("aws spawn failed: {e}");
                    continue;
                }
            }; // AWS SSO prints a URL — try to surface it.
            if let Some(mut err) = child.stderr.take() {
                let tx2 = tx.clone();
                thread::spawn(move || {
                    let mut buf = String::new();
                    let _ = err.read_to_string(&mut buf);
                    for word in buf.split_whitespace() {
                        if word.starts_with("https://") {
                            send(&tx2, BrowserLoginProgress::OpenUrl(word.to_string()));
                            let _ = open_browser(word);
                            break;
                        }
                    }
                    if buf.to_lowercase().contains("user code") || buf.contains("enter the code") {
                        send(
                            &tx2,
                            BrowserLoginProgress::Status(buf.chars().take(200).collect()),
                        );
                    }
                });
            }
            loop {
                if cancel.is_cancelled() {
                    let _ = child.kill();
                    return Err(MuseError::Other("login cancelled".into()));
                }
                match child.try_wait() {
                    Ok(Some(s)) if s.success() => {
                        ok = true;
                        break;
                    }
                    Ok(Some(s)) => {
                        last = format!("aws {} exit {s}", args.join(" "));
                        break;
                    }
                    Ok(None) => thread::sleep(Duration::from_millis(200)),
                    Err(e) => {
                        last = e.to_string();
                        break;
                    }
                }
            }
            if ok {
                break;
            }
        }
        if !ok {
            return Err(MuseError::Other(format!(
                "AWS SSO login failed ({last}). Install AWS CLI v2, configure SSO, or paste a bearer/token if you use a Bedrock gateway."
            )));
        }

        // AWS SSO credentials are SigV4 material, not Bedrock bearer tokens. Nur's
        // OpenAI-compatible HTTP path can only use an actual Bedrock API key/token.
        // Never persist an access-key marker as a bearer: it makes login appear
        // successful and guarantees every subsequent request will be rejected.
        send(
            tx,
            BrowserLoginProgress::Status("checking for a Bedrock bearer token…".into()),
        );
        if let Ok(token) = std::env::var("AWS_BEARER_TOKEN_BEDROCK") {
            if !token.is_empty() {
                return Ok(OAuthTokens {
                    access_token: token,
                    refresh_token: Some("aws-sso".into()),
                    expires_at: Some(super::super::now_unix() + 3600),
                    meta: Some(OauthMeta {
                        issuer: "aws-sso".into(),
                        client_id: "aws-cli".into(),
                        extra: serde_json::json!({"via": "env AWS_BEARER_TOKEN_BEDROCK"}),
                    }),
                });
            }
        }

        Err(MuseError::Other(
            "AWS SSO completed, but SSO credentials require SigV4 and cannot be sent as a bearer token. Generate a short-term Bedrock API key, set AWS_BEARER_TOKEN_BEDROCK, then retry /login; or paste a Bedrock API key. The AWS CLI SSO session remains active."
                .into(),
        ))
    }

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
        Err(MuseError::Other(
            "AWS Bedrock refresh: re-run /login browser (aws sso login)".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unsigned_jwt(payload: serde_json::Value) -> String {
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap());
        format!("header.{payload}.signature")
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

// silence unused import warning for mpsc in some builds
#[allow(dead_code)]
fn _channel_ty() -> mpsc::Sender<u8> {
    let (tx, _) = mpsc::channel();
    tx
}

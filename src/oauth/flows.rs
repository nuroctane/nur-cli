//! Per-provider browser / device-code / external-CLI login flows.

use super::{expires_in_to_at, open_browser, CancelFlag};
use crate::auth::{save_oauth_session, Auth, OauthMeta};
use crate::error::{MuseError, Result};
use base64::engine::general_purpose::{URL_SAFE_NO_PAD, STANDARD};
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
        "xai" => xai::login(&tx, &cancel),
        "anthropic" => claude::login(&tx, &cancel),
        "antigravity" => antigravity::login(&tx, &cancel),
        "huggingface" => huggingface::login(&tx, &cancel),
        "azure" => azure::login(&tx, &cancel),
        "bedrock" => bedrock::login(&tx, &cancel),
        other => Err(MuseError::Other(format!(
            "browser login not supported for '{other}'"
        ))),
    };
    match result {
        Ok(tokens) => {
            if let Err(e) = save_oauth_session(
                provider_id,
                &tokens.access_token,
                tokens.refresh_token.clone(),
                tokens.expires_at,
                tokens.meta.clone(),
            ) {
                send(&tx, BrowserLoginProgress::Failed(e.to_string()));
            } else {
                send(&tx, BrowserLoginProgress::Done(tokens));
            }
        }
        Err(e) => send(&tx, BrowserLoginProgress::Failed(e.to_string())),
    }
}

/// Import tokens from a first-party CLI session file when present.
pub fn import_existing_session(provider_id: &str) -> Result<Option<OAuthTokens>> {
    match provider_id {
        "xai" => xai::import_grok_cli(),
        "anthropic" => claude::import_claude_cli(),
        _ => Ok(None),
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
        .user_agent(format!("meta-cli/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| MuseError::Other(e.to_string()))
}

/// Minimal localhost OAuth callback: waits for `?code=` (and optional state).
fn wait_localhost_code(
    port: u16,
    expected_state: Option<&str>,
    cancel: &CancelFlag,
    timeout: Duration,
) -> Result<String> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| MuseError::Other(format!("cannot bind localhost:{port}: {e}")))?;
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
                    "<html><body><h2>Signed in — you can close this tab and return to Meta CLI.</h2></body></html>"
                } else {
                    "<html><body><h2>Missing code — try again from Meta CLI.</h2></body></html>"
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
        // Prefer importing an existing Grok CLI session.
        if let Some(t) = import_grok_cli()? {
            send(
                tx,
                BrowserLoginProgress::Status("imported existing Grok CLI session".into()),
            );
            return Ok(t);
        }
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
            .unwrap_or_else(|| format!("https://accounts.x.ai/connect?user_code={}", device.user_code));

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
            thread::sleep(crate::oauth::device_poll_sleep(base_interval, slow, attempt));
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

// ── Anthropic Claude (PKCE + Claude CLI import) ────────────────────────────

pub mod claude {
    use super::*;

    /// Public Claude Code OAuth client id.
    pub const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
    const REDIRECT: &str = "http://localhost:54545/callback";
    const PORT: u16 = 54545;

    #[derive(Deserialize)]
    struct TokenResp {
        access_token: Option<String>,
        refresh_token: Option<String>,
        expires_in: Option<u64>,
        error: Option<String>,
        error_description: Option<String>,
    }

    pub fn login(tx: &ProgressTx, cancel: &CancelFlag) -> Result<OAuthTokens> {
        if let Some(t) = import_claude_cli()? {
            send(
                tx,
                BrowserLoginProgress::Status("imported existing Claude Code session".into()),
            );
            return Ok(t);
        }
        let verifier = random_urlsafe(32);
        let challenge = pkce_challenge(&verifier);
        let state = random_urlsafe(16);
        // Prefer platform.claude.com / claude.ai authorize endpoints.
        let auth_url = format!(
            "https://claude.ai/oauth/authorize?client_id={CLIENT_ID}&response_type=code&redirect_uri={}&scope={}&state={state}&code_challenge={challenge}&code_challenge_method=S256",
            urlencoding_encode(REDIRECT),
            urlencoding_encode("org:create_api_key user:profile user:inference"),
        );
        // Fallback-friendly: also try console host if user has console login.
        let _alt = format!(
            "https://console.anthropic.com/oauth/authorize?client_id={CLIENT_ID}&response_type=code&redirect_uri={}&scope={}&state={state}&code_challenge={challenge}&code_challenge_method=S256",
            urlencoding_encode(REDIRECT),
            urlencoding_encode("org:create_api_key user:profile user:inference"),
        );

        send(tx, BrowserLoginProgress::OpenUrl(auth_url.clone()));
        send(
            tx,
            BrowserLoginProgress::Status("complete sign-in in the browser…".into()),
        );
        let _ = open_browser(&auth_url);

        let code = wait_localhost_code(PORT, Some(&state), cancel, Duration::from_secs(600))?;
        send(
            tx,
            BrowserLoginProgress::Status("exchanging code for tokens…".into()),
        );

        let client = http()?;
        let token_urls = [
            "https://platform.claude.com/v1/oauth/token",
            "https://console.anthropic.com/v1/oauth/token",
        ];
        let form = [
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", REDIRECT),
            ("client_id", CLIENT_ID),
            ("code_verifier", verifier.as_str()),
            ("state", state.as_str()),
        ];
        let mut last = String::new();
        for url in token_urls {
            let res = match client.post(url).form(&form).send() {
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

    pub fn refresh(refresh: &str) -> Result<OAuthTokens> {
        let client = http()?;
        let form = [
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh),
            ("client_id", CLIENT_ID),
        ];
        for url in [
            "https://platform.claude.com/v1/oauth/token",
            "https://console.anthropic.com/v1/oauth/token",
        ] {
            let Ok(res) = client.post(url).form(&form).send() else {
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
        let path = home.join(".claude").join(".credentials.json");
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path)?;
        let v: serde_json::Value = serde_json::from_str(&text)?;
        let oauth = v
            .get("claudeAiOauth")
            .ok_or_else(|| MuseError::Other("no claudeAiOauth".into()))
            .ok();
        let Some(oauth) = oauth else {
            return Ok(None);
        };
        let access = oauth
            .get("accessToken")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        if access.is_empty() {
            return Ok(None);
        }
        let refresh = oauth
            .get("refreshToken")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        // Claude stores expiresAt as ms epoch sometimes.
        let expires_at = oauth.get("expiresAt").and_then(|x| {
            if let Some(n) = x.as_u64() {
                Some(if n > 10_000_000_000 { n / 1000 } else { n })
            } else {
                None
            }
        });
        Ok(Some(OAuthTokens {
            access_token: access.to_string(),
            refresh_token: refresh,
            expires_at,
            meta: Some(OauthMeta {
                issuer: "https://claude.ai".into(),
                client_id: CLIENT_ID.into(),
                extra: serde_json::json!({"imported_from": "claude-code"}),
            }),
        }))
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

// ── Google Antigravity (browser SSO via gcloud — no embedded OAuth secrets) ─

pub mod antigravity {
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
        let mut child = Command::new("gcloud")
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
                    "gcloud not found ({e}). Install Google Cloud SDK, or choose “Enter API key” with a Gemini key."
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
                    return Err(MuseError::Other(format!(
                        "gcloud auth login failed (exit {status}). Paste a Gemini API key as fallback."
                    )))
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
        let out = Command::new("gcloud")
            .args(["auth", "print-access-token"])
            .output()
            .map_err(|e| MuseError::Other(format!("gcloud print-access-token: {e}")))?;
        if !out.status.success() {
            return Err(MuseError::Other(format!(
                "gcloud print-access-token failed: {}",
                String::from_utf8_lossy(&out.stderr)
            )));
        }
        let access = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if access.is_empty() {
            return Err(MuseError::Other("empty token from gcloud".into()));
        }
        Ok(OAuthTokens {
            access_token: access,
            // Marker so ensure_fresh_oauth can re-call gcloud.
            refresh_token: Some("gcloud".into()),
            expires_at: Some(super::super::now_unix() + 3300),
            meta: Some(OauthMeta {
                issuer: "https://accounts.google.com".into(),
                client_id: "gcloud".into(),
                extra: serde_json::json!({"product": "antigravity", "via": "gcloud auth login"}),
            }),
        })
    }

    pub fn refresh(_auth: &Auth, _refresh: &str) -> Result<OAuthTokens> {
        fetch_access_token()
    }
}

// ── Hugging Face (device code — same spirit as `hf auth login`) ────────────

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

        // Try OAuth device flow; fall back to token page + poll is not available —
        // fall back to opening token settings and asking user to paste is Key path.
        let device_endpoints = [
            "https://huggingface.co/oauth/device/code",
            "https://huggingface.co/api/oauth/device/code",
        ];
        let mut device: Option<DeviceCodeResp> = None;
        let mut last = String::new();
        for url in device_endpoints {
            // Public HF OAuth app client used by huggingface_hub (community-known).
            let form = [
                ("client_id", "85c97818-78c2-455a-9472-9a0f2e8a1b0d"),
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
                thread::sleep(crate::oauth::device_poll_sleep(base_interval, slow, attempt));
                attempt = attempt.saturating_add(1);
                slow = false;
                let form = [
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ("device_code", device.device_code.as_str()),
                    ("client_id", "85c97818-78c2-455a-9472-9a0f2e8a1b0d"),
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
                                client_id: "huggingface".into(),
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
            "HF device flow unavailable ({last}). Open {url}, create a token, and choose “Enter API key” in /login."
        )))
    }

    pub fn refresh(_refresh: &str) -> Result<OAuthTokens> {
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
        let mut child = Command::new("az")
            .args(["login", "--use-device-code"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                MuseError::Other(format!(
                    "Azure CLI not found ({e}). Install `az` or paste AZURE_OPENAI_API_KEY."
                ))
            })?;
        // Best-effort parse device code from az stderr/stdout while waiting.
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
        let out = Command::new("az")
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
            .or_else(|| {
                parsed
                    .expires_on_ts
                    .as_deref()
                    .and_then(|s| s.parse().ok())
            });
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
        for args in [vec!["sso", "login"], vec!["login"]] {
            let mut child = match Command::new("aws")
                .args(&args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    last = format!("aws not found: {e}");
                    continue;
                }
            };
            // AWS SSO prints a URL — try to surface it.
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
                    if buf.to_lowercase().contains("user code") || buf.contains("enter the code")
                    {
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

        // Prefer env bearer (OpenAI-compat Bedrock gateways) then JSON credential export.
        send(
            tx,
            BrowserLoginProgress::Status("exporting AWS session credentials…".into()),
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

        // JSON-first (stable AWS CLI contract). Fall back to process env format.
        if let Ok(out) = Command::new("aws")
            .args(["configure", "export-credentials", "--format", "process"])
            .output()
        {
            if out.status.success() {
                #[derive(Deserialize)]
                struct AwsProcessCreds {
                    #[serde(rename = "AccessKeyId")]
                    access_key_id: Option<String>,
                    #[serde(rename = "SessionToken")]
                    session_token: Option<String>,
                    #[serde(rename = "Expiration")]
                    expiration: Option<String>,
                }
                if let Ok(c) = serde_json::from_slice::<AwsProcessCreds>(&out.stdout) {
                    if let Some(ak) = c.access_key_id.filter(|s| !s.is_empty()) {
                        // Not a pure bearer for Bedrock OpenAI path — store marker + session hint.
                        let token_part = c
                            .session_token
                            .as_deref()
                            .unwrap_or("")
                            .chars()
                            .take(24)
                            .collect::<String>();
                        let marker = format!(
                            "aws-sso-session:{}:{}",
                            &ak.chars().take(8).collect::<String>(),
                            STANDARD.encode(token_part.as_bytes())
                        );
                        let expires_at = c
                            .expiration
                            .as_deref()
                            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                            .map(|dt| dt.timestamp() as u64)
                            .or(Some(super::super::now_unix() + 3600));
                        return Ok(OAuthTokens {
                            access_token: marker,
                            refresh_token: Some("aws-sso".into()),
                            expires_at,
                            meta: Some(OauthMeta {
                                issuer: "aws-sso".into(),
                                client_id: "aws-cli".into(),
                                extra: serde_json::json!({
                                    "via": "aws sso login",
                                    "hint": "SSO session active. For Bedrock OpenAI-compat set AWS_BEARER_TOKEN_BEDROCK or a gateway key; native Bedrock uses SigV4 via AWS env."
                                }),
                            }),
                        });
                    }
                }
            }
        }

        Err(MuseError::Other(
            "AWS SSO completed, but no Bedrock bearer was found. Set AWS_BEARER_TOKEN_BEDROCK or paste a gateway key (/login → API key). SSO is active for the AWS CLI (`aws sso logout` to drop it)."
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

// silence unused import warning for mpsc in some builds
#[allow(dead_code)]
fn _channel_ty() -> mpsc::Sender<u8> {
    let (tx, _) = mpsc::channel();
    tx
}

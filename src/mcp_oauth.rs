//! MCP OAuth 2.1: Authorization Code + PKCE for remote (HTTP) MCP servers.
//!
//! MCP's authorization spec is OAuth 2.1 with resource indicators. This module
//! performs the practical path: discover the authorization server from the
//! MCP endpoint, register a loopback client (DCR), open the browser, take the
//! code on a local listener, exchange it, and store the access token so
//! `bearer_env`/`[mcp.servers.x] url` calls can authenticate.
//!
//! Tokens live in `~/.nur/mcp-tokens.json` (owner-only, like secrets.json).
//! `databearer_env` still wins when set, so an externally minted token works.

use base64::Engine as _;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::Duration;

const B64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::URL_SAFE_NO_PAD;
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// A URL-safe random string with `bytes` bytes of entropy.
fn random_token(bytes: usize) -> String {
    // uuid v4 carries 122 random bits; concatenate enough for the request.
    let mut hex = String::new();
    while hex.len() < bytes * 2 {
        hex.push_str(&uuid::Uuid::new_v4().simple().to_string());
    }
    let raw = hex.as_bytes();
    let mut out = Vec::with_capacity(bytes);
    let mut i = 0;
    while out.len() < bytes && i + 1 < raw.len() {
        let pair = std::str::from_utf8(&raw[i..i + 2]).unwrap_or("00");
        out.push(u8::from_str_radix(pair, 16).unwrap_or(0));
        i += 2;
    }
    B64.encode(&out)
}

fn pkce_challenge(verifier: &str) -> String {
    use sha2::{Digest, Sha256};
    B64.encode(Sha256::digest(verifier.as_bytes()))
}

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .user_agent(format!("nur-cli/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())
}

fn get_json(url: &str) -> Result<Value, String> {
    let resp = client()?.get(url).send().map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.json().map_err(|e| e.to_string())
}

/// `~/.nur/mcp-tokens.json`, a plain name -> token map.
fn store_path() -> std::path::PathBuf {
    let home = crate::config::home_dir().unwrap_or_default();
    home.join(".nur").join("mcp-tokens.json")
}

fn read_store() -> serde_json::Map<String, Value> {
    std::fs::read_to_string(store_path())
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

/// The stored access token for `name`, if any.
pub fn stored_token(name: &str) -> Option<String> {
    read_store()
        .get(name)?
        .get("access_token")?
        .as_str()
        .map(str::to_string)
}

fn save_token(name: &str, access: &str, refresh: Option<&str>) -> Result<(), String> {
    let mut store = read_store();
    store.insert(
        name.to_string(),
        json!({ "access_token": access, "refresh_token": refresh }),
    );
    let path = store_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_string_pretty(&Value::Object(store)).map_err(|e| e.to_string())?;
    std::fs::write(&path, body).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Run the Authorization Code + PKCE flow for server `name` at `url`.
/// Returns a human-readable result line.
pub fn login(name: &str, url: &str) -> Result<String, String> {
    let base = url.trim_end_matches('/');
    let origin = {
        let mut parts = base.splitn(4, '/');
        let _scheme = parts.next().unwrap_or("");
        let _empty = parts.next().unwrap_or("");
        let host = parts.next().unwrap_or("");
        let scheme = base.split("://").next().unwrap_or("https");
        format!("{scheme}://{host}")
    };
    let meta = get_json(&format!("{origin}/.well-known/oauth-authorization-server"))
        .or_else(|_| get_json(&format!("{origin}/.well-known/openid-configuration")))
        .map_err(|e| {
            format!(
                "mcp login {name}: no OAuth metadata at {origin} ({e}). This server may not \
                 require OAuth - set bearer_env instead, or use its own token."
            )
        })?;
    let authorize = meta
        .get("authorization_endpoint")
        .and_then(Value::as_str)
        .ok_or("mcp login: discovery has no authorization_endpoint")?
        .to_string();
    let token_endpoint = meta
        .get("token_endpoint")
        .and_then(Value::as_str)
        .ok_or("mcp login: discovery has no token_endpoint")?
        .to_string();

    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("mcp login: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("mcp login: {e}"))?
        .port();
    let redirect = format!("http://127.0.0.1:{port}/callback");

    // Dynamic client registration; a server that needs a pre-registered client
    // must publish `client_id` in its metadata instead.
    let client_id = if let Some(reg) = meta.get("registration_endpoint").and_then(Value::as_str) {
        let body = json!({
            "client_name": "nur",
            "redirect_uris": [redirect],
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none",
        });
        let resp = client()?
            .post(reg)
            .json(&body)
            .send()
            .map_err(|e| e.to_string())?;
        let v: Value = resp.json().map_err(|e| e.to_string())?;
        v.get("client_id")
            .and_then(Value::as_str)
            .ok_or("mcp login: registration returned no client_id")?
            .to_string()
    } else {
        meta.get("client_id")
            .and_then(Value::as_str)
            .ok_or("mcp login: no registration_endpoint and no client_id in metadata")?
            .to_string()
    };

    let verifier = random_token(32);
    let state = random_token(16);
    let url = format!(
        "{authorize}?response_type=code&client_id={client_id}&redirect_uri={}&scope={}&\
         code_challenge={}&code_challenge_method=S256&state={state}&resource={}",
        urlencode(&redirect),
        urlencode("mcp"),
        pkce_challenge(&verifier),
        urlencode(base),
    );
    crate::open_uri::open(&url).map_err(|e| format!("mcp login: could not open the browser: {e}"))?;

    let (mut stream, _) = listener
        .accept()
        .map_err(|e| format!("mcp login: callback failed: {e}"))?;
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .map_err(|e| format!("mcp login: {e}"))?;
    let query = request_line
        .split_whitespace()
        .nth(1)
        .and_then(|target| target.split_once('?'))
        .map(|(_, q)| q.to_string())
        .unwrap_or_default();
    let params: std::collections::HashMap<String, String> = query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (k.to_string(), urldecode(v)))
        .collect();
    let reply = if params.get("state") != Some(&state) {
        "state mismatch - login rejected"
    } else {
        "nur is signed in to this MCP server. You can close this window."
    };
    let body = format!(
        "<!doctype html><meta charset=utf-8><title>nur</title><p>{reply}</p>"
    );
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    if params.get("state") != Some(&state) {
        return Err("mcp login: state mismatch; the callback did not match this request".into());
    }
    let code = params
        .get("code")
        .ok_or("mcp login: callback carried no code (authorization was declined?)")?
        .clone();

    let resp = client()?
        .post(&token_endpoint)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("client_id", client_id.as_str()),
            ("code_verifier", verifier.as_str()),
        ])
        .send()
        .map_err(|e| format!("mcp login: token exchange failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "mcp login: token endpoint returned HTTP {}",
            resp.status()
        ));
    }
    let v: Value = resp.json().map_err(|e| e.to_string())?;
    let access = v
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or("mcp login: token endpoint returned no access_token")?;
    let refresh = v.get("refresh_token").and_then(Value::as_str);
    save_token(name, access, refresh)?;
    Ok(format!(
        "mcp login {name}: token stored (mcp-tokens.json). Set \
         [mcp.servers.{name}] url = \"{base}\" to use it."
    ))
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
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

fn urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("00");
                out.push(u8::from_str_radix(hex, 16).unwrap_or(0));
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_is_s256_of_the_verifier() {
        // RFC 7636 appendix B vector.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn random_tokens_differ_and_are_url_safe() {
        let a = random_token(32);
        let b = random_token(32);
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn url_coding_round_trips() {
        let s = "http://127.0.0.1:1234/callback";
        assert_eq!(urldecode(&urlencode(s)), s);
    }

    #[test]
    fn stored_token_is_none_for_unknown_names() {
        assert!(stored_token("definitely-not-stored").is_none());
    }
}

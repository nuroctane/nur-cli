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
use std::io::{BufRead, BufReader, Read, Write};
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
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(format!("nur-cli/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())
}

fn response_json(response: reqwest::blocking::Response) -> Result<Value, String> {
    const LIMIT: usize = 1024 * 1024;
    let mut body = Vec::new();
    response
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut body)
        .map_err(|error| error.to_string())?;
    if body.len() > LIMIT {
        return Err("OAuth response exceeded the size limit".into());
    }
    serde_json::from_slice(&body).map_err(|error| format!("invalid OAuth response: {error}"))
}

fn read_callback_line(stream: impl Read) -> Result<String, String> {
    const LIMIT: u64 = 8192;
    let mut reader = BufReader::new(stream).take(LIMIT + 1);
    let mut line = String::new();
    let bytes = reader
        .read_line(&mut line)
        .map_err(|error| format!("mcp login: {error}"))?;
    if bytes as u64 > LIMIT || !line.ends_with('\n') {
        return Err("mcp login: callback request line is invalid or too long".into());
    }
    Ok(line)
}

/// Credentials and authorization codes may cross HTTP only on loopback.
pub(crate) fn secure_url(raw: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(raw).map_err(|e| format!("invalid MCP URL: {e}"))?;
    if url.fragment().is_some() {
        return Err("MCP authentication URLs cannot contain a fragment".into());
    }
    let loopback = url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if !url.username().is_empty()
        || url.password().is_some()
        || url.host_str().is_none()
        || !(url.scheme() == "https" || (url.scheme() == "http" && loopback))
    {
        return Err("MCP authentication requires HTTPS, except on loopback".into());
    }
    Ok(url)
}

/// `~/.nur/mcp-tokens.json`, a plain name -> token map.
fn store_path() -> std::path::PathBuf {
    crate::config::nur_home().join("mcp-tokens.json")
}

fn read_store() -> serde_json::Map<String, Value> {
    std::fs::read_to_string(store_path())
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

/// Saved MCP credentials participate in output redaction but are never
/// injected into shell environments. Retain rotated values for this process
/// so an in-flight result cannot disclose a token that was just replaced.
#[cfg(not(test))]
pub(crate) fn redaction_pairs() -> std::sync::Arc<Vec<(String, String)>> {
    type Pairs = std::sync::Arc<Vec<(String, String)>>;
    type Stamp = (
        std::path::PathBuf,
        Option<std::time::SystemTime>,
        Option<u64>,
    );
    static CACHE: std::sync::Mutex<Option<(Stamp, Pairs)>> = std::sync::Mutex::new(None);
    let path = store_path();
    let metadata = std::fs::metadata(&path).ok();
    let stamp = (
        path,
        metadata.as_ref().and_then(|m| m.modified().ok()),
        metadata.map(|m| m.len()),
    );
    let mut cache = CACHE.lock().unwrap_or_else(|error| error.into_inner());
    if let Some((seen, pairs)) = cache.as_ref() {
        if *seen == stamp {
            return pairs.clone();
        }
    }
    let mut pairs = cache
        .as_ref()
        .map(|(_, pairs)| pairs.as_ref().clone())
        .unwrap_or_default();
    for entry in read_store().values() {
        for field in ["access_token", "refresh_token"] {
            if let Some(token) = entry
                .get(field)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
            {
                if !pairs.iter().any(|(_, existing)| existing == token) {
                    pairs.push(("MCP_TOKEN".into(), token.to_string()));
                }
            }
        }
    }
    let pairs = std::sync::Arc::new(pairs);
    *cache = Some((stamp, pairs.clone()));
    pairs
}

#[cfg(test)]
pub(crate) fn redaction_pairs() -> std::sync::Arc<Vec<(String, String)>> {
    // Tests that exercise the real store run the compiled binary with NUR_HOME.
    // Unit tests must never consult the user's actual home.
    std::sync::Arc::new(Vec::new())
}

fn lock_store() -> Result<std::fs::File, String> {
    let path = store_path().with_extension("lock");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.lock().map_err(|error| error.to_string())?;
    Ok(file)
}

fn write_store(store: &serde_json::Map<String, Value>) -> Result<(), String> {
    let body =
        serde_json::to_string_pretty(&Value::Object(store.clone())).map_err(|e| e.to_string())?;
    crate::config::private_atomic_write(&store_path(), body.as_bytes()).map_err(|e| e.to_string())
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn refresh_entry(entry: &Value, resource: &str) -> Result<Value, String> {
    let endpoint = entry
        .get("token_endpoint")
        .and_then(Value::as_str)
        .ok_or("missing token endpoint")?;
    secure_url(endpoint)?;
    let client_id = entry
        .get("client_id")
        .and_then(Value::as_str)
        .ok_or("missing client id")?
        .to_string();
    let token = json!({
        "access_token": entry.get("access_token").and_then(Value::as_str).ok_or("missing access token")?,
        "refresh_token": entry.get("refresh_token").and_then(Value::as_str).ok_or("missing refresh token")?,
        "token_type": "Bearer",
    });
    let endpoint = endpoint.to_string();
    let resource = resource.to_string();
    let response = std::thread::spawn(move || {
        use rmcp::transport::auth::{
            AuthorizationManager, CredentialStore, InMemoryCredentialStore, StoredCredentials,
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        runtime.block_on(async {
            let mut manager = AuthorizationManager::new(&resource)
                .await
                .map_err(|e| e.to_string())?;
            let metadata = serde_json::from_value(
                json!({"authorization_endpoint": resource, "token_endpoint": endpoint}),
            )
            .map_err(|e: serde_json::Error| e.to_string())?;
            manager.set_metadata(metadata);
            let token =
                serde_json::from_value(token).map_err(|e: serde_json::Error| e.to_string())?;
            let store = InMemoryCredentialStore::new();
            store
                .save(StoredCredentials::new(
                    client_id.clone(),
                    Some(token),
                    vec![],
                    None,
                ))
                .await
                .map_err(|e| e.to_string())?;
            manager.set_credential_store(store);
            manager
                .configure_client_id(&client_id)
                .map_err(|e| e.to_string())?;
            let response = manager.refresh_token().await.map_err(|e| e.to_string())?;
            serde_json::to_value(response).map_err(|e| e.to_string())
        })
    })
    .join()
    .map_err(|_| "OAuth refresh worker failed".to_string())??;
    let mut updated = entry.clone();
    let object = updated.as_object_mut().ok_or("invalid token entry")?;
    for field in ["access_token", "refresh_token"] {
        if let Some(value) = response.get(field) {
            object.insert(field.into(), value.clone());
        }
    }
    match response.get("expires_in").and_then(Value::as_u64) {
        Some(seconds) => {
            object.insert(
                "expires_at".into(),
                json!(now_secs().saturating_add(seconds)),
            );
        }
        None => {
            object.remove("expires_at");
        }
    }
    Ok(updated)
}

/// The stored access token for `name`, if any.
pub fn stored_token(name: &str, url: &str) -> Result<Option<String>, String> {
    stored_token_inner(name, url, None)
}

/// Refresh after a 401 unless another process already replaced the rejected token.
pub(crate) fn refresh_rejected_token(
    name: &str,
    url: &str,
    rejected: &str,
) -> Result<Option<String>, String> {
    stored_token_inner(name, url, Some(rejected))
}

fn stored_token_inner(
    name: &str,
    url: &str,
    rejected: Option<&str>,
) -> Result<Option<String>, String> {
    // A public server needs no local credential store. Avoid creating or
    // locking one unless this server already has a saved token.
    if !read_store().contains_key(name) {
        return Ok(None);
    }
    let _lock = lock_store()?;
    let mut store = read_store();
    let Some(entry) = store.get(name) else {
        return Ok(None);
    };
    let resource = secure_url(url)?.to_string();
    if entry.get("resource").and_then(Value::as_str) != Some(resource.as_str()) {
        return Err(format!(
            "mcp {name}: saved token is not bound to this server URL; use mcp action=login again"
        ));
    }
    // Remember credentials in use even if another process replaces the store.
    let _ = redaction_pairs();
    let current = entry.get("access_token").and_then(Value::as_str);
    let rejected_current = rejected.is_some_and(|token| Some(token) == current);
    if rejected_current
        || entry
            .get("expires_at")
            .and_then(Value::as_u64)
            .is_some_and(|expiry| expiry <= now_secs().saturating_add(30))
    {
        let updated = refresh_entry(entry, &resource)
            .map_err(|_| format!("mcp {name}: token refresh failed; use mcp action=login again"))?;
        let token = updated
            .get("access_token")
            .and_then(Value::as_str)
            .map(str::to_string);
        store.insert(name.to_string(), updated);
        write_store(&store)?;
        return Ok(token);
    }
    Ok(entry
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::to_string))
}

fn save_token(
    name: &str,
    url: &str,
    access: &str,
    refresh: Option<&str>,
    refresh_info: Option<(&str, &str, Option<u64>)>,
) -> Result<(), String> {
    let _lock = lock_store()?;
    let _ = redaction_pairs();
    let mut store = read_store();
    let mut entry = json!({ "resource": secure_url(url)?.to_string(), "access_token": access, "refresh_token": refresh });
    if let Some((client_id, endpoint, expires_in)) = refresh_info {
        entry["client_id"] = json!(client_id);
        entry["token_endpoint"] = json!(secure_url(endpoint)?.to_string());
        if let Some(seconds) = expires_in {
            entry["expires_at"] = json!(now_secs().saturating_add(seconds));
        }
    }
    store.insert(name.to_string(), entry);
    write_store(&store)
}

/// Run the Authorization Code + PKCE flow for server `name` at `url`.
/// Returns a human-readable result line.
fn discover_metadata(name: &str, url: &str) -> Result<Value, String> {
    let resource = secure_url(url)?.to_string();
    // A real initialize request can advertise a nonstandard metadata URL
    // and required scopes. This probe carries no credential.
    let challenge = client()?.post(&resource)
        .header(reqwest::header::ACCEPT, "application/json, text/event-stream")
        .json(&json!({"jsonrpc":"2.0", "id":1, "method":"initialize", "params": {
            "protocolVersion": crate::mcp_http::PROTOCOL_VERSION,
            "capabilities": {}, "clientInfo": {"name":"nur", "version":env!("CARGO_PKG_VERSION")}
        }}))
        .send().ok()
        .filter(|response| response.status() == reqwest::StatusCode::UNAUTHORIZED)
        .and_then(|response| response.headers().get(reqwest::header::WWW_AUTHENTICATE)
            .and_then(|value| value.to_str().ok()).map(str::to_string));
    // Tool execution is synchronous and may itself run inside a Tokio runtime.
    // Keep the SDK's async discovery on its own short-lived runtime.
    let result = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        runtime.block_on(async {
            let mut manager = rmcp::transport::auth::AuthorizationManager::new(&resource)
                .await
                .map_err(|error| error.to_string())?;
            let resolution = manager
                .resolve_metadata_from_challenge(challenge.as_deref())
                .await
                .map_err(|error| error.to_string())?;
            if !resolution.source.is_discovered() {
                return Err("server did not publish OAuth metadata".to_string());
            }
            let metadata = resolution.metadata;
            secure_url(&metadata.authorization_endpoint)?;
            secure_url(&metadata.token_endpoint)?;
            if let Some(registration) = &metadata.registration_endpoint {
                secure_url(registration)?;
            }
            manager.set_metadata(metadata.clone());
            let scope = manager.select_initial_scopes(&[]).await.join(" ");
            let mut value = serde_json::to_value(metadata).map_err(|error| error.to_string())?;
            value["_nur_scope"] = json!(scope);
            Ok(value)
        })
    })
    .join()
    .map_err(|_| "OAuth discovery worker failed".to_string())?;
    result.map_err(|error| format!("mcp login {name}: OAuth discovery failed ({error})"))
}

pub fn login(name: &str, url: &str) -> Result<String, String> {
    let base = secure_url(url)?.to_string();
    let meta = discover_metadata(name, url)?;
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
    secure_url(&authorize)?;
    secure_url(&token_endpoint)?;
    let scope = meta.get("_nur_scope").and_then(Value::as_str).unwrap_or("");

    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("mcp login: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("mcp login: {e}"))?
        .port();
    let redirect = format!("http://127.0.0.1:{port}/callback");

    // Dynamic client registration; a server that needs a pre-registered client
    // must publish `client_id` in its metadata instead.
    let client_id = if let Some(reg) = meta.get("registration_endpoint").and_then(Value::as_str) {
        secure_url(reg)?;
        let mut body = json!({
            "client_name": "nur",
            "redirect_uris": [redirect],
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none",
        });
        if !scope.is_empty() {
            body["scope"] = json!(scope);
        }
        let resp = client()?
            .post(reg)
            .json(&body)
            .send()
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!(
                "mcp login: registration returned HTTP {}",
                resp.status()
            ));
        }
        let v = response_json(resp)?;
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
    let mut auth_url = secure_url(&authorize)?;
    let mut query = auth_url.query_pairs_mut();
    query
        .append_pair("response_type", "code")
        .append_pair("client_id", &client_id)
        .append_pair("redirect_uri", &redirect)
        .append_pair("code_challenge", &pkce_challenge(&verifier))
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &state)
        .append_pair("resource", &base);
    if !scope.is_empty() {
        query.append_pair("scope", scope);
    }
    drop(query);
    crate::open_uri::open(auth_url.as_str())
        .map_err(|e| format!("mcp login: could not open the browser: {e}"))?;

    listener
        .set_nonblocking(true)
        .map_err(|e| format!("mcp login: {e}"))?;
    let deadline = std::time::Instant::now() + Duration::from_secs(180);
    let (mut stream, _) = loop {
        match listener.accept() {
            Ok(connection) => break connection,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    return Err("mcp login: callback timed out".into());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(format!("mcp login: callback failed: {e}")),
        }
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| format!("mcp login: {e}"))?;
    let request_line = read_callback_line(stream.try_clone().map_err(|e| e.to_string())?)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if method != "GET" || path != "/callback" {
        return Err("mcp login: unexpected callback request".into());
    }
    let mut params = std::collections::HashMap::new();
    for (key, value) in query.split('&').filter_map(|pair| pair.split_once('=')) {
        if params.insert(key.to_string(), urldecode(value)).is_some() {
            return Err("mcp login: duplicate callback parameter".into());
        }
    }
    let reply = if params.get("state") != Some(&state) {
        "state mismatch - login rejected"
    } else if !params.contains_key("code") {
        "authorization was declined; return to nur for details"
    } else {
        "authorization received; return to nur to finish signing in"
    };
    let body = format!("<!doctype html><meta charset=utf-8><title>nur</title><p>{reply}</p>");
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
            ("resource", base.as_str()),
        ])
        .send()
        .map_err(|e| format!("mcp login: token exchange failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "mcp login: token endpoint returned HTTP {}",
            resp.status()
        ));
    }
    let v = response_json(resp)?;
    let access = v
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or("mcp login: token endpoint returned no access_token")?;
    let refresh = v.get("refresh_token").and_then(Value::as_str);
    save_token(
        name,
        url,
        access,
        refresh,
        Some((
            &client_id,
            &token_endpoint,
            v.get("expires_in").and_then(Value::as_u64),
        )),
    )?;
    Ok(format!(
        "mcp login {name}: token stored (mcp-tokens.json). Set \
         [mcp.servers.{name}] url = \"{base}\" to use it."
    ))
}

#[cfg(test)]
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
    fn discovers_a_separate_authorization_server() {
        use std::io::Read;
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        let resource = TcpListener::bind("127.0.0.1:0").unwrap();
        let authorization = TcpListener::bind("127.0.0.1:0").unwrap();
        let resource_url = format!("http://{}/mcp", resource.local_addr().unwrap());
        let issuer = format!("http://{}", authorization.local_addr().unwrap());
        let done = Arc::new(AtomicBool::new(false));
        let serve = |listener: TcpListener,
                     resource_url: String,
                     issuer: String,
                     is_resource: bool,
                     done: Arc<AtomicBool>| {
            listener.set_nonblocking(true).unwrap();
            std::thread::spawn(move || {
                while !done.load(Ordering::Relaxed) {
                    let Ok((mut stream, _)) = listener.accept() else {
                        std::thread::sleep(Duration::from_millis(10));
                        continue;
                    };
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut buf = [0; 4096];
                    let len = stream.read(&mut buf).unwrap_or(0);
                    let request = String::from_utf8_lossy(&buf[..len]);
                    let body = if is_resource
                        && request.starts_with("GET /.well-known/oauth-protected-resource/mcp ")
                    {
                        Some(json!({"resource": resource_url, "authorization_servers": [issuer]}))
                    } else if !is_resource
                        && request.starts_with("GET /.well-known/oauth-authorization-server ")
                    {
                        Some(
                            json!({"issuer": issuer, "authorization_endpoint": format!("{issuer}/authorize"), "token_endpoint": format!("{issuer}/token")}),
                        )
                    } else {
                        None
                    };
                    let body = body.map(|value| value.to_string());
                    let status = if body.is_some() {
                        "200 OK"
                    } else {
                        "404 Not Found"
                    };
                    let body = body.unwrap_or_default();
                    let _ = write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                }
            })
        };
        let resource_server = serve(
            resource,
            resource_url.clone(),
            issuer.clone(),
            true,
            done.clone(),
        );
        let auth_server = serve(
            authorization,
            resource_url.clone(),
            issuer.clone(),
            false,
            done.clone(),
        );
        let result = discover_metadata("test", &resource_url);
        done.store(true, Ordering::Relaxed);
        resource_server.join().unwrap();
        auth_server.join().unwrap();
        let metadata = result.unwrap();
        assert_eq!(metadata["token_endpoint"], format!("{issuer}/token"));
    }

    #[test]
    fn discovery_follows_the_server_challenge_and_scopes() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let resource = format!("{origin}/mcp");
        let stop = Arc::new(AtomicBool::new(false));
        let saw_custom = Arc::new(AtomicBool::new(false));
        listener.set_nonblocking(true).unwrap();
        let server = {
            let origin = origin.clone();
            let resource = resource.clone();
            let stop = stop.clone();
            let saw_custom = saw_custom.clone();
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let Ok((mut stream, _)) = listener.accept() else {
                        std::thread::sleep(Duration::from_millis(10));
                        continue;
                    };
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut buf = [0; 4096];
                    let len = stream.read(&mut buf).unwrap_or(0);
                    let request = String::from_utf8_lossy(&buf[..len]);
                    let (status, extra, body) = if request.starts_with("POST /mcp ") {
                        ("401 Unauthorized", format!("WWW-Authenticate: Bearer resource_metadata=\"{origin}/custom\", scope=\"needed\"\r\n"), String::new())
                    } else if request.starts_with("GET /custom ") {
                        saw_custom.store(true, Ordering::Relaxed);
                        (
                            "200 OK",
                            String::new(),
                            json!({"resource": resource, "authorization_servers": [origin]})
                                .to_string(),
                        )
                    } else if request.starts_with("GET /.well-known/oauth-authorization-server ") {
                        ("200 OK", String::new(), json!({"issuer": origin, "authorization_endpoint": format!("{origin}/authorize"), "token_endpoint": format!("{origin}/token"), "scopes_supported": ["needed", "offline_access"]}).to_string())
                    } else {
                        ("404 Not Found", String::new(), String::new())
                    };
                    let _ = write!(stream, "HTTP/1.1 {status}\r\n{extra}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                }
            })
        };
        let result = discover_metadata("test", &resource);
        stop.store(true, Ordering::Relaxed);
        server.join().unwrap();
        let metadata = result.unwrap();
        assert!(saw_custom.load(Ordering::Relaxed));
        let scope = metadata["_nur_scope"].as_str().unwrap();
        assert!(scope.split_whitespace().any(|item| item == "needed"));
        assert!(scope
            .split_whitespace()
            .any(|item| item == "offline_access"));
    }

    #[test]
    fn token_store_respects_custom_nur_home() {
        const CHILD: &str = "NUR_TEST_MCP_TOKEN_STORE_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let home = tempfile::tempdir().unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "mcp_oauth::tests::token_store_respects_custom_nur_home",
                ])
                .env(CHILD, "1")
                .env("NUR_HOME", home.path().join("custom-store"))
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }
        assert!(stored_token("public-server", "http://127.0.0.1/mcp")
            .unwrap()
            .is_none());
        assert!(!store_path().with_extension("lock").exists());
        save_token(
            "test-server",
            "https://valid.example/mcp",
            "test-token",
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            stored_token("test-server", "https://valid.example/mcp")
                .unwrap()
                .as_deref(),
            Some("test-token")
        );
        assert_eq!(
            refresh_rejected_token("test-server", "https://valid.example/mcp", "previous-token")
                .unwrap()
                .as_deref(),
            Some("test-token")
        );
        assert!(crate::config::nur_home().join("mcp-tokens.json").exists());
        let redirected = crate::mcp_stdio::McpServerConfig {
            url: "https://other.example/mcp".into(),
            ..Default::default()
        };
        assert!(crate::mcp_stdio::bearer_for(&redirected, "test-server").is_err());
        let mut legacy = read_store();
        legacy.insert("old-server".into(), json!({"access_token": "legacy-token"}));
        crate::config::private_atomic_write(
            &store_path(),
            serde_json::to_string(&legacy).unwrap().as_bytes(),
        )
        .unwrap();
        assert!(stored_token("old-server", "https://valid.example/mcp").is_err());
    }

    #[test]
    fn refreshes_expired_credentials() {
        const CHILD: &str = "NUR_TEST_MCP_REFRESH_CHILD";
        if std::env::var(CHILD).as_deref() == Ok("worker") {
            let resource = std::env::var("NUR_TEST_MCP_RESOURCE").unwrap();
            assert_eq!(
                stored_token("test-server", &resource).unwrap().as_deref(),
                Some("new-access")
            );
            return;
        }
        if std::env::var_os(CHILD).is_none() {
            let home = tempfile::tempdir().unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "mcp_oauth::tests::refreshes_expired_credentials"])
                .env(CHILD, "1")
                .env("NUR_HOME", home.path())
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let resource = format!("{origin}/mcp");
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while std::time::Instant::now() < deadline {
                if let Ok((mut stream, _)) = listener.accept() {
                    use std::io::Read;
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut request = [0; 4096];
                    let len = stream.read(&mut request).unwrap();
                    let text = String::from_utf8_lossy(&request[..len]);
                    assert!(text.starts_with("POST /token "));
                    let body = r#"{"access_token":"new-access","refresh_token":"rotated-refresh","token_type":"Bearer","expires_in":3600}"#;
                    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                    return true;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            false
        });
        let entry = json!({"resource": resource, "access_token": "expired-access", "refresh_token": "old-refresh", "client_id": "client", "token_endpoint": format!("{origin}/token"), "expires_at": 1});
        std::fs::create_dir_all(crate::config::nur_home()).unwrap();
        crate::config::private_atomic_write(
            &store_path(),
            json!({"test-server": entry}).to_string().as_bytes(),
        )
        .unwrap();
        let mut workers = (0..2)
            .map(|_| {
                std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "mcp_oauth::tests::refreshes_expired_credentials"])
                    .env(CHILD, "worker")
                    .env("NUR_TEST_MCP_RESOURCE", &resource)
                    .spawn()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        for worker in &mut workers {
            assert!(worker.wait().unwrap().success());
        }
        let requested = server.join().unwrap();
        assert!(requested, "refresh endpoint was not contacted");
        assert_eq!(
            read_store()["test-server"]["refresh_token"],
            "rotated-refresh"
        );
    }

    #[test]
    fn credentials_cannot_use_remote_plain_http_or_url_userinfo() {
        assert!(secure_url("http://example.test/mcp").is_err());
        assert!(secure_url("https://name:password@example.test/mcp").is_err());
        assert!(secure_url("http://127.0.0.1:1234/mcp").is_ok());
        assert!(secure_url("https://example.test/mcp").is_ok());
        assert!(secure_url("https://example.test/mcp#fragment").is_err());
    }

    #[test]
    fn callback_request_line_is_bounded_and_complete() {
        assert!(read_callback_line(std::io::Cursor::new(b"GET /callback HTTP/1.1\r\n")).is_ok());
        assert!(read_callback_line(std::io::Cursor::new(vec![b'x'; 8193])).is_err());
        assert!(read_callback_line(std::io::Cursor::new(b"GET /callback")).is_err());
    }

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
        assert!(a
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn url_coding_round_trips() {
        let s = "http://127.0.0.1:1234/callback";
        assert_eq!(urldecode(&urlencode(s)), s);
    }
}

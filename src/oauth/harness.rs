//! First-party harness / CLI session import and login.
//!
//! Official contracts (do not invent OAuth where the vendor has none):
//!
//! - **Muse Code** ([auth](https://dev.meta.ai/docs/muse-code/auth)): browser
//!   sign-in or `META_API_KEY` / `MODEL_API_KEY`. Session file
//!   `~/.config/muse/auth.json`. Isolation: `MUSE_CONFIG_DIR`.
//! - **DeepSeek Harness** ([providers](https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/user/guide/providers.md)):
//!   API key only, stored as `$DSH_HOME/.credentials.yaml` (`DEEPSEEK_API_KEY: …`).
//!   Isolation: `DSH_HOME` (default `~/.dsh`).
//! - **ZCode** ([connect](https://zcode.z.ai/en/docs/configuration)): Z.ai /
//!   BigModel account OAuth (`zcode login`) or an API key. Config
//!   `~/.zcode/v2/config.json`; `credentials.json` is device-encrypted and is
//!   not scraped. Coding Plan traffic uses `/api/coding/paas/v4`.
//! - **Qwen Code** ([auth](https://qwenlm.github.io/qwen-code-docs/en/users/configuration/auth/)):
//!   Qwen OAuth is discontinued. Import `~/.qwen/settings.json` keys only.
//! - **MiniMax CLI**: `mmx auth login --api-key` (no OAuth). Import a stored
//!   key when the CLI left one on disk.

use super::flows::{BrowserLoginProgress, OAuthTokens, ProgressTx};
use super::{open_browser, CancelFlag};
use crate::auth::OauthMeta;
use crate::error::{NurError, Result};
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn send(tx: &ProgressTx, ev: BrowserLoginProgress) {
    let _ = tx.send(ev);
}

fn looks_like_secret(s: &str) -> bool {
    let t = s.trim();
    t.len() >= 12 && !t.contains(char::is_whitespace)
}

fn credential_tokens(
    access: String,
    refresh: Option<String>,
    expires_at: Option<u64>,
    issuer: &str,
    client_id: &str,
    extra: serde_json::Value,
) -> OAuthTokens {
    OAuthTokens {
        access_token: access,
        refresh_token: refresh,
        expires_at,
        meta: Some(OauthMeta {
            issuer: issuer.into(),
            client_id: client_id.into(),
            extra,
        }),
    }
}

fn api_key_tokens(access: String, issuer: &str, via: &str, path: &str) -> OAuthTokens {
    credential_tokens(
        access,
        Some(format!("{issuer}-cli")),
        None,
        issuer,
        &format!("{issuer}-cli"),
        serde_json::json!({
            "imported_from": via,
            "path": path,
            "credential_kind": "api_key",
        }),
    )
}

fn which_cli(name: &str) -> Option<PathBuf> {
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

fn resolve_cli(name: &str, windows_names: &[&str], extra_dirs: &[PathBuf]) -> Option<PathBuf> {
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
                for ext in ["cmd", "exe", "bat"] {
                    let c = dir.join(format!("{fname}.{ext}"));
                    if c.is_file() {
                        return Some(c);
                    }
                }
            }
        }
    }
    None
}

fn spawn_cli(bin: &Path, args: &[&str]) -> std::io::Result<std::process::Child> {
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

fn pump_cli_output(tx: ProgressTx, stream: impl std::io::Read + Send + 'static) {
    thread::spawn(move || {
        for line in std::io::BufReader::new(stream)
            .lines()
            .map_while(|line| line.ok())
        {
            let snippet: String = line.chars().take(180).collect();
            if !snippet.trim().is_empty() {
                send(&tx, BrowserLoginProgress::Status(snippet.clone()));
            }
            for word in line.split_whitespace() {
                let url = word.trim_matches(|c: char| {
                    matches!(c, ')' | '(' | '"' | '\'' | '`' | ',' | '.' | ';')
                });
                if url.starts_with("https://") || url.starts_with("http://127.") || url.starts_with("http://localhost")
                {
                    send(&tx, BrowserLoginProgress::OpenUrl(url.to_string()));
                    let _ = open_browser(url);
                    break;
                }
            }
        }
    });
}

fn wait_child_or_cancel(
    child: &mut std::process::Child,
    cancel: &CancelFlag,
    timeout: Duration,
) -> Result<bool> {
    let started = Instant::now();
    loop {
        if cancel.is_cancelled() {
            let _ = child.kill();
            return Err(NurError::Other("login cancelled".into()));
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            return Err(NurError::Other("login timed out".into()));
        }
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status.success()),
            Ok(None) => thread::sleep(Duration::from_millis(200)),
            Err(e) => return Err(NurError::Other(e.to_string())),
        }
    }
}

fn json_string_field(v: &serde_json::Value, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(s) = v.get(*k).and_then(|x| x.as_str()) {
            let t = s.trim();
            if looks_like_secret(t) {
                return Some(t.to_string());
            }
        }
    }
    None
}

fn walk_json_secrets(v: &serde_json::Value, keys: &[&str], out: &mut Vec<(String, Option<String>)>) {
    if let Some(obj) = v.as_object() {
        if let Some(secret) = json_string_field(v, keys) {
            let base = obj
                .get("baseUrl")
                .or_else(|| obj.get("base_url"))
                .or_else(|| obj.get("endpoint"))
                .and_then(|x| x.as_str())
                .map(|s| s.to_string());
            out.push((secret, base));
        }
        for val in obj.values() {
            walk_json_secrets(val, keys, out);
        }
    } else if let Some(arr) = v.as_array() {
        for val in arr {
            walk_json_secrets(val, keys, out);
        }
    }
}

/// Minimal YAML mapping parser for DeepSeek's `.credentials.yaml`.
///
/// Official format is a flat `KEY: value` document — no version wrapper.
/// Quoted values, comments, and `KEY: "value"` are accepted. Nested maps
/// are ignored.
pub fn parse_simple_yaml_map(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("---") {
            continue;
        }
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let key = k.trim();
        if key.is_empty() || key.contains(' ') {
            continue;
        }
        let mut val = v.trim().to_string();
        if val.len() >= 2 {
            let bytes = val.as_bytes();
            if (bytes[0] == b'"' && *bytes.last().unwrap() == b'"')
                || (bytes[0] == b'\'' && *bytes.last().unwrap() == b'\'')
            {
                val = val[1..val.len() - 1].to_string();
            }
        }
        if val.is_empty() {
            continue;
        }
        out.push((key.to_string(), val));
    }
    out
}

pub fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

pub fn is_api_key_import(tokens: &OAuthTokens) -> bool {
    tokens.meta.as_ref().is_some_and(|m| {
        m.extra
            .get("credential_kind")
            .and_then(|v| v.as_str())
            .is_some_and(|k| k.eq_ignore_ascii_case("api_key"))
    })
}

pub mod muse {
    use super::*;

    pub fn config_dir() -> Option<PathBuf> {
        if let Ok(dir) = std::env::var("MUSE_CONFIG_DIR") {
            let p = PathBuf::from(dir.trim());
            if !p.as_os_str().is_empty() {
                return Some(p);
            }
        }
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            let p = PathBuf::from(xdg.trim());
            if !p.as_os_str().is_empty() {
                return Some(p.join("muse"));
            }
        }
        let home = home_dir()?;
        Some(home.join(".config").join("muse"))
    }

    fn auth_paths() -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Some(dir) = config_dir() {
            out.push(dir.join("auth.json"));
        }
        if let Some(home) = home_dir() {
            out.push(home.join(".muse").join("auth.json"));
            #[cfg(windows)]
            {
                if let Ok(appdata) = std::env::var("APPDATA") {
                    out.push(PathBuf::from(appdata).join("muse").join("auth.json"));
                }
            }
        }
        out
    }

    pub fn import_muse_cli() -> Result<Option<OAuthTokens>> {
        for path in auth_paths() {
            if let Some(tokens) = tokens_from_auth_file(&path) {
                return Ok(Some(tokens));
            }
        }
        Ok(None)
    }

    pub fn tokens_from_auth_file(path: &Path) -> Option<OAuthTokens> {
        let text = std::fs::read_to_string(path).ok()?;
        tokens_from_auth_json(&text, path)
    }

    pub fn tokens_from_auth_json(text: &str, path: &Path) -> Option<OAuthTokens> {
        let v: serde_json::Value = serde_json::from_str(text).ok()?;
        let root = v.get("auth").or_else(|| v.get("credentials")).unwrap_or(&v);
        let access = json_string_field(
            root,
            &[
                "access_token",
                "accessToken",
                "token",
                "api_key",
                "apiKey",
                "key",
            ],
        )?;
        let refresh = json_string_field(root, &["refresh_token", "refreshToken"]);
        let expires = root
            .get("expires_at")
            .or_else(|| root.get("expiresAt"))
            .or_else(|| root.get("expiry"))
            .and_then(|x| x.as_u64().or_else(|| x.as_i64().map(|n| n as u64)));
        let looks_oauth = refresh.is_some()
            || json_string_field(root, &["access_token", "accessToken"]).is_some();
        let mut extra = serde_json::json!({
            "imported_from": "muse-cli",
            "path": path.display().to_string(),
            "credential_kind": if looks_oauth { "oauth" } else { "api_key" },
        });
        if let Some(model) = root
            .get("model")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            extra["model"] = serde_json::Value::String(model.to_string());
        }
        Some(credential_tokens(
            access,
            refresh.or_else(|| Some("muse".into())),
            expires,
            "muse",
            "muse-code",
            extra,
        ))
    }

    fn muse_bin() -> Option<PathBuf> {
        let mut dirs = Vec::new();
        if let Some(home) = home_dir() {
            dirs.push(home.join(".local").join("bin"));
            dirs.push(home.join(".muse").join("bin"));
        }
        resolve_cli("muse", &["muse.exe", "muse.cmd"], &dirs)
    }

    pub fn login(tx: &ProgressTx, cancel: &CancelFlag) -> Result<OAuthTokens> {
        if let Ok(Some(t)) = import_muse_cli() {
            send(
                tx,
                BrowserLoginProgress::Status("using existing Muse Code session".into()),
            );
            return Ok(t);
        }
        let bin = muse_bin().ok_or_else(|| {
            NurError::Other(
                "muse not found on PATH. Install Muse Code (https://dev.meta.ai/docs/muse-code), \
                 run `muse` once to sign in, or paste META_API_KEY / MODEL_API_KEY."
                    .into(),
            )
        })?;
        send(
            tx,
            BrowserLoginProgress::Status("launching `muse login`…".into()),
        );
        // Official porcelain is first-run `muse` + `/login`, plus `muse auth set`
        // for keys. Community and CLI help expose `muse login`. Prefer that,
        // then `muse auth login`.
        let attempts: &[&[&str]] = &[&["login"], &["auth", "login"]];
        let mut last_err = None;
        for args in attempts {
            if cancel.is_cancelled() {
                return Err(NurError::Other("login cancelled".into()));
            }
            match spawn_cli(&bin, args) {
                Ok(mut child) => {
                    if let Some(err) = child.stderr.take() {
                        pump_cli_output(tx.clone(), err);
                    }
                    if let Some(out) = child.stdout.take() {
                        pump_cli_output(tx.clone(), out);
                    }
                    let before = import_muse_cli().ok().flatten().map(|t| t.access_token);
                    let started = Instant::now();
                    loop {
                        if cancel.is_cancelled() {
                            let _ = child.kill();
                            return Err(NurError::Other("login cancelled".into()));
                        }
                        if let Ok(Some(t)) = import_muse_cli() {
                            if before.as_ref() != Some(&t.access_token) || before.is_none() {
                                let _ = child.kill();
                                return Ok(t);
                            }
                        }
                        match child.try_wait() {
                            Ok(Some(status)) if status.success() => {
                                if let Ok(Some(t)) = import_muse_cli() {
                                    return Ok(t);
                                }
                                break;
                            }
                            Ok(Some(status)) => {
                                last_err = Some(format!("muse {} failed (exit {status})", args.join(" ")));
                                break;
                            }
                            Ok(None) => {
                                if started.elapsed() > Duration::from_secs(300) {
                                    let _ = child.kill();
                                    last_err = Some("muse login timed out".into());
                                    break;
                                }
                                thread::sleep(Duration::from_millis(250));
                            }
                            Err(e) => {
                                last_err = Some(e.to_string());
                                break;
                            }
                        }
                    }
                }
                Err(e) => last_err = Some(e.to_string()),
            }
        }
        import_muse_cli()?.ok_or_else(|| {
            NurError::Other(format!(
                "{}. Run `muse` and choose browser sign-in, or paste META_API_KEY.",
                last_err.unwrap_or_else(|| "Muse Code login did not store a session".into())
            ))
        })
    }

    pub fn refresh(_auth: &crate::auth::Auth, _refresh: &str) -> Result<OAuthTokens> {
        import_muse_cli()?.ok_or_else(|| {
            NurError::Other(
                "Muse Code session missing. Run `muse` to sign in, or set META_API_KEY.".into(),
            )
        })
    }
}

pub mod deepseek {
    use super::*;

    pub fn dsh_home() -> PathBuf {
        if let Ok(dir) = std::env::var("DSH_HOME") {
            let p = PathBuf::from(dir.trim());
            if !p.as_os_str().is_empty() {
                return p;
            }
        }
        home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".dsh")
    }

    pub fn credentials_path() -> PathBuf {
        dsh_home().join(".credentials.yaml")
    }

    pub fn tokens_from_credentials_yaml(text: &str, path: &Path) -> Option<OAuthTokens> {
        for (k, v) in parse_simple_yaml_map(text) {
            if k.eq_ignore_ascii_case("DEEPSEEK_API_KEY") && looks_like_secret(&v) {
                return Some(api_key_tokens(
                    v,
                    "deepseek",
                    "dsh-credentials",
                    &path.display().to_string(),
                ));
            }
        }
        None
    }

    pub fn import_dsh_cli() -> Result<Option<OAuthTokens>> {
        let path = credentials_path();
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Some(tokens) = tokens_from_credentials_yaml(&text, &path) {
                return Ok(Some(tokens));
            }
        }
        // Also honour a project-local .env sitting next to an explicit DSH_HOME.
        let env_path = dsh_home().join(".env");
        if let Ok(text) = std::fs::read_to_string(&env_path) {
            for (k, v) in parse_simple_yaml_map(&text) {
                if k.eq_ignore_ascii_case("DEEPSEEK_API_KEY") && looks_like_secret(&v) {
                    return Ok(Some(api_key_tokens(
                        v,
                        "deepseek",
                        "dsh-env",
                        &env_path.display().to_string(),
                    )));
                }
            }
        }
        Ok(None)
    }

    fn dsh_bin() -> Option<PathBuf> {
        let mut dirs = Vec::new();
        if let Some(home) = home_dir() {
            dirs.push(home.join(".local").join("bin"));
            dirs.push(dsh_home().join("bin"));
        }
        resolve_cli("dsh", &["dsh.exe", "dsh.cmd"], &dirs)
    }

    pub fn login(tx: &ProgressTx, cancel: &CancelFlag) -> Result<OAuthTokens> {
        if let Ok(Some(t)) = import_dsh_cli() {
            send(
                tx,
                BrowserLoginProgress::Status(
                    "using existing DeepSeek Harness credentials.yaml".into(),
                ),
            );
            return Ok(t);
        }
        // Official DeepSeek auth is an API key, entered in Settings → Models of
        // `dsh web` and stored in $DSH_HOME/.credentials.yaml. There is no
        // first-party OAuth for api.deepseek.com.
        if let Some(bin) = dsh_bin() {
            send(
                tx,
                BrowserLoginProgress::Status(
                    "launching `dsh web` — save the DeepSeek API key in Settings → Models".into(),
                ),
            );
            let mut child = spawn_cli(&bin, &["web"]).map_err(|e| {
                NurError::Other(format!(
                    "failed to launch dsh ({e}). Paste DEEPSEEK_API_KEY, or save it in DeepSeek Harness Settings → Models."
                ))
            })?;
            if let Some(err) = child.stderr.take() {
                pump_cli_output(tx.clone(), err);
            }
            if let Some(out) = child.stdout.take() {
                pump_cli_output(tx.clone(), out);
            }
            let started = Instant::now();
            loop {
                if cancel.is_cancelled() {
                    let _ = child.kill();
                    return Err(NurError::Other("login cancelled".into()));
                }
                if let Ok(Some(t)) = import_dsh_cli() {
                    let _ = child.kill();
                    return Ok(t);
                }
                if started.elapsed() > Duration::from_secs(300) {
                    let _ = child.kill();
                    break;
                }
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) => thread::sleep(Duration::from_millis(400)),
                    Err(e) => {
                        let _ = child.kill();
                        return Err(NurError::Other(e.to_string()));
                    }
                }
            }
        } else {
            let url = "https://platform.deepseek.com/api_keys";
            send(tx, BrowserLoginProgress::OpenUrl(url.into()));
            let _ = open_browser(url);
        }
        import_dsh_cli()?.ok_or_else(|| {
            NurError::Other(
                "DeepSeek has no OAuth — create a key at https://platform.deepseek.com/api_keys, \
                 save it in DeepSeek Harness (Settings → Models) or paste DEEPSEEK_API_KEY."
                    .into(),
            )
        })
    }

    pub fn refresh(_auth: &crate::auth::Auth, _refresh: &str) -> Result<OAuthTokens> {
        import_dsh_cli()?.ok_or_else(|| {
            NurError::Other(
                "DeepSeek Harness key missing. Save DEEPSEEK_API_KEY in $DSH_HOME/.credentials.yaml \
                 or paste it via /login."
                    .into(),
            )
        })
    }
}

pub mod zhipu {
    use super::*;

    pub fn zcode_home() -> PathBuf {
        for var in ["ZCODE_HOME", "ZCODE_CONFIG_DIR"] {
            if let Ok(dir) = std::env::var(var) {
                let p = PathBuf::from(dir.trim());
                if !p.as_os_str().is_empty() {
                    return p;
                }
            }
        }
        home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".zcode")
    }

    fn config_paths() -> Vec<PathBuf> {
        let home = zcode_home();
        vec![
            home.join("v2").join("config.json"),
            home.join("cli").join("config.json"),
            home.join("config.json"),
        ]
    }

    fn prefer_coding(base: Option<&str>) -> bool {
        base.is_some_and(|b| b.contains("/coding/") || b.contains("/anthropic"))
    }

    pub fn tokens_from_zcode_config(text: &str, path: &Path) -> Option<OAuthTokens> {
        let v: serde_json::Value = serde_json::from_str(text).ok()?;
        let mut found = Vec::new();
        walk_json_secrets(
            &v,
            &["api_key", "apiKey", "access_token", "accessToken", "token"],
            &mut found,
        );
        if found.is_empty() {
            return None;
        }
        // Prefer a Coding Plan / official Z.ai key over a third-party custom
        // provider that happens to sit in the same config.
        found.sort_by_key(|(_, base)| {
            let b = base.as_deref().unwrap_or("");
            if b.contains("api.z.ai") || b.contains("bigmodel.cn") {
                if prefer_coding(Some(b)) {
                    0
                } else {
                    1
                }
            } else {
                2
            }
        });
        let (secret, base) = found.into_iter().next()?;
        let coding = prefer_coding(base.as_deref());
        Some(credential_tokens(
            secret,
            Some("zcode".into()),
            None,
            "zcode",
            "zcode-cli",
            serde_json::json!({
                "imported_from": "zcode-cli",
                "path": path.display().to_string(),
                "credential_kind": if coding { "oauth" } else { "api_key" },
                "base_url": base,
                "route": if coding { "coding" } else { "general" },
            }),
        ))
    }

    pub fn import_zcode_cli() -> Result<Option<OAuthTokens>> {
        for path in config_paths() {
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Some(tokens) = tokens_from_zcode_config(&text, &path) {
                    return Ok(Some(tokens));
                }
            }
        }
        Ok(None)
    }

    fn zcode_bin() -> Option<PathBuf> {
        let mut dirs = Vec::new();
        if let Some(home) = home_dir() {
            dirs.push(home.join(".local").join("bin"));
            dirs.push(zcode_home().join("bin"));
            #[cfg(windows)]
            {
                dirs.push(home.join("AppData").join("Local").join("Programs").join("zcode"));
                dirs.push(home.join("AppData").join("Local").join("zcode"));
            }
        }
        resolve_cli("zcode", &["zcode.exe", "zcode.cmd"], &dirs)
    }

    pub fn login(tx: &ProgressTx, cancel: &CancelFlag) -> Result<OAuthTokens> {
        if let Ok(Some(t)) = import_zcode_cli() {
            send(
                tx,
                BrowserLoginProgress::Status("using existing ZCode config.json credential".into()),
            );
            return Ok(t);
        }
        let bin = zcode_bin().ok_or_else(|| {
            NurError::Other(
                "zcode not found on PATH. Install ZCode (https://zcode.z.ai), run `zcode login`, \
                 or paste a Z.AI API key (Coding Plan: https://api.z.ai/api/coding/paas/v4)."
                    .into(),
            )
        })?;
        send(
            tx,
            BrowserLoginProgress::Status("launching `zcode login`…".into()),
        );
        let mut child = spawn_cli(&bin, &["login"]).map_err(|e| {
            NurError::Other(format!(
                "failed to launch zcode ({e}). Run `zcode login` in a terminal, or paste ZAI_API_KEY."
            ))
        })?;
        if let Some(err) = child.stderr.take() {
            pump_cli_output(tx.clone(), err);
        }
        if let Some(out) = child.stdout.take() {
            pump_cli_output(tx.clone(), out);
        }
        let before = import_zcode_cli().ok().flatten().map(|t| t.access_token);
        let _ = wait_child_or_cancel(&mut child, cancel, Duration::from_secs(300))?;
        if let Ok(Some(t)) = import_zcode_cli() {
            if before.as_ref() != Some(&t.access_token) || before.is_none() {
                return Ok(t);
            }
            return Ok(t);
        }
        Err(NurError::Other(
            "zcode login finished, but nur found no API key in ~/.zcode/v2/config.json. \
             Official ZCode OAuth tokens are device-encrypted — after login, switch the \
             Z.ai provider to API Key mode or paste ZAI_API_KEY. Coding Plan endpoint: \
             https://api.z.ai/api/coding/paas/v4"
                .into(),
        ))
    }

    pub fn refresh(_auth: &crate::auth::Auth, _refresh: &str) -> Result<OAuthTokens> {
        import_zcode_cli()?.ok_or_else(|| {
            NurError::Other(
                "ZCode session missing. Run `zcode login`, or set ZAI_API_KEY.".into(),
            )
        })
    }
}

pub mod qwen {
    use super::*;

    fn qwen_home() -> PathBuf {
        if let Ok(dir) = std::env::var("QWEN_CONFIG_DIR") {
            let p = PathBuf::from(dir.trim());
            if !p.as_os_str().is_empty() {
                return p;
            }
        }
        home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".qwen")
    }

    pub fn import_qwen_cli() -> Result<Option<OAuthTokens>> {
        let path = qwen_home().join("settings.json");
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Ok(None);
        };
        Ok(tokens_from_qwen_settings(&text, &path))
    }

    pub fn tokens_from_qwen_settings(text: &str, path: &Path) -> Option<OAuthTokens> {
        let v: serde_json::Value = serde_json::from_str(text).ok()?;
        let env = v.get("env").and_then(|x| x.as_object());
        let pick = |name: &str| -> Option<String> {
            env.and_then(|m| m.get(name))
                .and_then(|x| x.as_str())
                .map(str::trim)
                .filter(|s| looks_like_secret(s))
                .map(|s| s.to_string())
        };
        // Official Qwen OAuth free tier was discontinued 2026-04-15. Import
        // DashScope / Coding Plan keys from settings.json only.
        if let Some(key) = pick("DASHSCOPE_API_KEY").or_else(|| pick("OPENAI_API_KEY")) {
            return Some(api_key_tokens(
                key,
                "qwen",
                "qwen-settings",
                &path.display().to_string(),
            ));
        }
        if let Some(key) = pick("BAILIAN_CODING_PLAN_API_KEY") {
            return Some(credential_tokens(
                key,
                Some("qwen-coding-plan".into()),
                None,
                "qwen",
                "qwen-code",
                serde_json::json!({
                    "imported_from": "qwen-coding-plan",
                    "path": path.display().to_string(),
                    "credential_kind": "api_key",
                    "base_url": "https://coding-intl.dashscope.aliyuncs.com/v1",
                }),
            ));
        }
        None
    }
}

pub mod minimax {
    use super::*;

    fn candidate_files() -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Some(home) = home_dir() {
            out.push(home.join(".mmx").join("config.json"));
            out.push(home.join(".mmx").join("auth.json"));
            out.push(home.join(".minimax").join("config.json"));
            out.push(home.join(".minimax").join("auth.json"));
        }
        out
    }

    pub fn import_mmx_cli() -> Result<Option<OAuthTokens>> {
        for path in candidate_files() {
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Some(tokens) = tokens_from_mmx_file(&text, &path) {
                    return Ok(Some(tokens));
                }
            }
        }
        Ok(None)
    }

    pub fn tokens_from_mmx_file(text: &str, path: &Path) -> Option<OAuthTokens> {
        let v: serde_json::Value = serde_json::from_str(text).ok()?;
        let mut found = Vec::new();
        walk_json_secrets(&v, &["api_key", "apiKey", "token", "access_token"], &mut found);
        let (secret, _) = found.into_iter().next()?;
        Some(api_key_tokens(
            secret,
            "minimax",
            "mmx-cli",
            &path.display().to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn yaml_map_parses_dsh_credentials_document() {
        let text = "# comment\nDEEPSEEK_API_KEY: sk-testkeyabcdefghijklmnop\nOPENAI_API_KEY: \"sk-other\"\n";
        let map = parse_simple_yaml_map(text);
        assert_eq!(map.len(), 2);
        assert_eq!(map[0].0, "DEEPSEEK_API_KEY");
        assert!(map[0].1.starts_with("sk-test"));
        assert_eq!(map[1].1, "sk-other");
    }

    #[test]
    fn dsh_import_reads_official_flat_mapping() {
        let text = "DEEPSEEK_API_KEY: sk-deepseek-harness-key-123456\n";
        let tokens = deepseek::tokens_from_credentials_yaml(text, Path::new("/tmp/.credentials.yaml"))
            .expect("key");
        assert!(is_api_key_import(&tokens));
        assert_eq!(tokens.access_token, "sk-deepseek-harness-key-123456");
    }

    #[test]
    fn muse_auth_json_oauth_and_key_shapes() {
        let oauth = r#"{
            "access_token": "muse-access-token-value-12345",
            "refresh_token": "muse-refresh-token-value-12345",
            "expires_at": 1700000000
        }"#;
        let t = muse::tokens_from_auth_json(oauth, Path::new("/tmp/auth.json")).unwrap();
        assert!(!is_api_key_import(&t));
        assert_eq!(t.refresh_token.as_deref(), Some("muse-refresh-token-value-12345"));

        let key = r#"{"api_key":"LLM|607358788850350|nx9abcdefghijklmnopLJY"}"#;
        let t = muse::tokens_from_auth_json(key, Path::new("/tmp/auth.json")).unwrap();
        assert!(is_api_key_import(&t));
        assert!(t.access_token.starts_with("LLM|"));
    }

    #[test]
    fn zcode_config_prefers_coding_plan_endpoint() {
        let text = r#"{
            "providers": {
                "custom": { "apiKey": "sk-custom-should-lose-123456", "baseUrl": "https://example.com/v1" },
                "zai": { "apiKey": "sk-zai-coding-key-1234567890", "baseUrl": "https://api.z.ai/api/coding/paas/v4" }
            }
        }"#;
        let t = zhipu::tokens_from_zcode_config(text, Path::new("/tmp/config.json")).unwrap();
        assert_eq!(t.access_token, "sk-zai-coding-key-1234567890");
        assert!(!is_api_key_import(&t));
        assert_eq!(
            t.meta.as_ref().unwrap().extra["route"].as_str(),
            Some("coding")
        );
    }

    #[test]
    fn qwen_settings_import_dashscope_key() {
        let text = r#"{
            "env": { "DASHSCOPE_API_KEY": "sk-dashscope-key-1234567890" },
            "model": { "name": "qwen3-coder-plus" }
        }"#;
        let t = qwen::tokens_from_qwen_settings(text, Path::new("/tmp/settings.json")).unwrap();
        assert!(is_api_key_import(&t));
        assert_eq!(t.access_token, "sk-dashscope-key-1234567890");
    }

    #[test]
    fn minimax_config_import() {
        let text = r#"{"apiKey":"sk-minimax-stored-key-123456"}"#;
        let t = minimax::tokens_from_mmx_file(text, Path::new("/tmp/config.json")).unwrap();
        assert!(is_api_key_import(&t));
    }
}

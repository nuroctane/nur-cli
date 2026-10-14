//! Factory Droid CLI transport - chat through `droid exec` with the
//! Factory-managed models.
//!
//! Factory has no public OpenAI-compatible chat endpoint; its inference runs
//! behind the `droid` binary (`droid` login or `FACTORY_API_KEY`). Nur drives
//! it the same way it drives Cursor: one `droid exec --output-format json`
//! per model round, nur's tools offered through the nur-tools fence, and the
//! CLI's own `--session-id` reused so later rounds ship only the delta.
//!
//! Without `--auto` the CLI is read-only (spec mode), which is exactly the
//! posture for a model that must answer with tool *requests*: it may read the
//! workspace to ground its reply but every mutation goes through nur's
//! approvals. `NUR_DROID_NATIVE=1` instead delegates the whole task to Droid
//! with `--auto <NUR_DROID_AUTO|medium>`.

use crate::api::cursor_cli::{
    flatten_incremental, flatten_prompt_with, response_from_agent_cli_text, CursorChatState,
};
use crate::api::types::{ApiResponse, ApiUsage, ResponseRequest};
use crate::api::StreamEvent;
use crate::error::{NurError, Result};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Catalog placeholder base URL. Never called over HTTP.
pub const FACTORY_APP_URL: &str = "https://app.factory.ai";

/// Model ids Factory-managed inference serves (`docs.factory.ai/docs/models`
/// as of this build). Only the fallback when the installed CLI cannot be
/// asked: each `droid` release ships its own catalog (see [`list_models`]),
/// and the two drift apart within weeks.
pub const FACTORY_MODELS: &[&str] = &[
    "claude-fable-5.1",
    "claude-fable-5",
    "claude-opus-5-5",
    "claude-opus-5-5-fast",
    "claude-opus-5",
    "claude-opus-5-fast",
    "claude-opus-4-8",
    "claude-opus-4-7",
    "claude-opus-4-6",
    "claude-sonnet-5-5",
    "claude-sonnet-5",
    "claude-sonnet-4-6",
    "claude-haiku-5-5",
    "claude-haiku-4-5-20251001",
    "gpt-6.1-sol",
    "gpt-6-astra",
    "gpt-6-sol",
    "gpt-6-luna",
    "gpt-5.6-sol",
    "gpt-5.6-terra",
    "gpt-5.6-luna",
    "gpt-5.5",
    "gpt-5.5-pro",
    "gpt-5.4",
    "gpt-5.4-mini",
    "gpt-5.3-codex",
    "gpt-5.2",
    "gemini-3.1-pro-preview",
    "gemini-3.8-flash",
    "gemini-3.7-flash",
    "gemini-3.6-flash",
    "gemini-3.5-flash",
    "gemini-3-flash-preview",
    "grok-4.7",
    "grok-4.6",
    "grok-4.5",
    "inkling",
    "mistral-large-4",
    "mistral-medium-3.5",
    "glm-5.3-flash",
    "glm-5.3",
    "glm-5.2",
    "kimi-k3",
    "qwen3.8-max",
    "nemotron-3-ultra",
    "deepseek-v4.1-flash",
    "deepseek-v4-pro",
    "minimax-m3",
];

/// Model ids the installed `droid` accepts. The CLI has no list command, but
/// `droid exec --help` prints an "Available Models:" table (every tier the
/// account can select, Droid Core and standard alike), and its "Invalid
/// model" error repeats the same ids as one comma-separated line; both forms
/// are parsed.
///
/// `droid exec` boots the whole agent before printing help (about 12 s on a
/// warm machine), so the answer is kept on disk keyed by the binary's
/// identity: the catalog is compiled into each release, so it only changes
/// when the binary does. [`warm_models_cache`] fills it off the hot path; a
/// cold `/model` waits once. Falls back to [`FACTORY_MODELS`] only when the
/// CLI is missing or prints no table.
pub fn list_models() -> Result<Vec<String>> {
    let Some(bin) = resolve_bin() else {
        return Ok(static_models());
    };
    let identity = bin_identity(&bin);
    if let Some(ids) = cached_models(&identity) {
        return Ok(ids);
    }
    if let Some(ids) = await_warm(&identity) {
        return Ok(ids);
    }
    let ids = capture_models(&bin)?;
    if ids.is_empty() {
        tracing::warn!("droid exec --help listed no models; using the built-in catalog");
        return Ok(static_models());
    }
    store_models(&identity, &ids);
    Ok(ids)
}

/// Set while a background capture runs so a cold `/model` joins it instead
/// of launching a second 12 s `droid` process.
static WARMING: Mutex<bool> = Mutex::new(false);

fn warming() -> bool {
    WARMING.lock().map(|b| *b).unwrap_or(false)
}

/// Wait for an in-flight warm to land, polling the cache; `None` when no
/// warm is running or it finished without a catalog.
fn await_warm(identity: &str) -> Option<Vec<String>> {
    if !warming() {
        return None;
    }
    let deadline = Instant::now() + CAPTURE_TIMEOUT;
    while warming() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    cached_models(identity)
}

/// Fill the on-disk catalog for the installed binary in the background when
/// it is missing or belongs to an older build. Returns at once; concurrent
/// callers share one capture.
pub fn warm_models_cache() {
    let Some(bin) = resolve_bin() else { return };
    let identity = bin_identity(&bin);
    if cached_models(&identity).is_some() {
        return;
    }
    if let Ok(mut busy) = WARMING.lock() {
        if *busy {
            return;
        }
        *busy = true;
    }
    let spawned = std::thread::Builder::new()
        .name("nur-droid-models".into())
        .spawn(move || {
            match capture_models(&bin) {
                Ok(ids) if !ids.is_empty() => store_models(&identity, &ids),
                Ok(_) => tracing::warn!("droid exec --help listed no models"),
                Err(e) => tracing::warn!("droid model catalog warm failed: {e}"),
            }
            if let Ok(mut busy) = WARMING.lock() {
                *busy = false;
            }
        });
    if spawned.is_err() {
        if let Ok(mut busy) = WARMING.lock() {
            *busy = false;
        }
    }
}

/// `droid exec` loads settings, plugins and MCP servers before it prints
/// help; 12 s is normal on a warm machine and a busy one takes longer.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(60);

fn capture_models(bin: &PathBuf) -> Result<Vec<String>> {
    let text = run_help_capture(bin)?;
    Ok(parse_model_list(&text))
}

fn static_models() -> Vec<String> {
    FACTORY_MODELS.iter().map(|m| m.to_string()).collect()
}

/// Path, size and mtime: a new droid release changes at least the latter two.
fn bin_identity(bin: &std::path::Path) -> String {
    let meta = std::fs::metadata(bin).ok();
    let len = meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let mtime = meta
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}|{len}|{mtime}", bin.display())
}

fn models_cache_path() -> PathBuf {
    crate::config::nur_home()
        .join("cache")
        .join("droid-models.json")
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ModelsCache {
    bin: String,
    ids: Vec<String>,
}

/// Process-wide copy so a picker reopened in the same session does not re-read
/// the file; the file makes the next session instant too.
static MEM_CACHE: Mutex<Option<ModelsCache>> = Mutex::new(None);

fn cached_models(identity: &str) -> Option<Vec<String>> {
    if let Ok(guard) = MEM_CACHE.lock() {
        if let Some(c) = guard.as_ref() {
            if c.bin == identity && !c.ids.is_empty() {
                return Some(c.ids.clone());
            }
        }
    }
    let text = std::fs::read_to_string(models_cache_path()).ok()?;
    let cache: ModelsCache = serde_json::from_str(&text).ok()?;
    if cache.bin != identity || cache.ids.is_empty() {
        return None;
    }
    let ids = cache.ids.clone();
    if let Ok(mut guard) = MEM_CACHE.lock() {
        *guard = Some(cache);
    }
    Some(ids)
}

fn store_models(identity: &str, ids: &[String]) {
    let cache = ModelsCache {
        bin: identity.to_string(),
        ids: ids.to_vec(),
    };
    if let Ok(text) = serde_json::to_string(&cache) {
        let path = models_cache_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = crate::config::atomic_write(&path, text.as_bytes());
    }
    if let Ok(mut guard) = MEM_CACHE.lock() {
        *guard = Some(cache);
    }
}

/// `droid exec --help`, stdout and stderr together (the launcher may print its
/// certificate warnings on stderr and some builds send help there too).
fn run_help_capture(bin: &PathBuf) -> Result<String> {
    let mut cmd = Command::new(bin);
    cmd.args(["exec", "--help"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("NO_COLOR", "1");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| NurError::Other(format!("failed to launch droid: {e}{}", auth_hint())))?;
    fn drain<R: std::io::Read + Send + 'static>(
        pipe: Option<R>,
    ) -> Option<std::thread::JoinHandle<Vec<u8>>> {
        pipe.map(|mut pipe| {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = pipe.read_to_end(&mut buf);
                buf
            })
        })
    }
    let readers = [drain(child.stdout.take()), drain(child.stderr.take())];
    let deadline = Instant::now() + CAPTURE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(NurError::Other(
                    "droid exec --help took over 60s; retry /model once the machine is idle".into(),
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(NurError::Other(format!("droid wait: {e}"))),
        }
    }
    let mut text = String::new();
    for handle in readers.into_iter().flatten() {
        if let Ok(bytes) = handle.join() {
            text.push_str(&String::from_utf8_lossy(&bytes));
            text.push('\n');
        }
    }
    Ok(text)
}

/// Ids from the help table (`  <id>   <label>` rows under "Available
/// Models:") or the error form (`Available built-in models:` followed by a
/// comma list). Stops at the first blank line or next section header.
pub(crate) fn parse_model_list(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let header = line.trim();
        let table = header.eq_ignore_ascii_case("available models:");
        let inline = header
            .to_ascii_lowercase()
            .starts_with("available built-in models");
        if !table && !inline {
            continue;
        }
        for row in lines.by_ref() {
            let trimmed = row.trim();
            if trimmed.is_empty() || (trimmed.ends_with(':') && !trimmed.contains(',')) {
                break;
            }
            let ids: Vec<&str> = if trimmed.contains(',') {
                trimmed.split(',').map(str::trim).collect()
            } else if inline && trimmed.contains(' ') {
                // Prose after the comma list ("No custom models configured…").
                break;
            } else {
                trimmed.split_whitespace().take(1).collect()
            };
            for id in ids {
                let id = id.trim_end_matches(',');
                if id.is_empty() || id.contains(' ') || out.iter().any(|x| x == id) {
                    continue;
                }
                out.push(id.to_string());
            }
        }
        if !out.is_empty() {
            break;
        }
    }
    out
}

/// Session chain for `--session-id`; same shape as Cursor's resume chain.
pub(crate) type DroidChatState = CursorChatState;

/// When set (`1`/`true`/`yes`) Droid runs the task itself with its own tools
/// (`--auto`), ignoring nur's harness. Default is the nur-tools protocol.
pub fn native_delegate_enabled() -> bool {
    std::env::var("NUR_DROID_NATIVE")
        .ok()
        .is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
}

/// Autonomy tier for the native delegate; `medium` covers local dev work
/// without `git push` or production changes.
fn native_auto_level() -> String {
    std::env::var("NUR_DROID_AUTO")
        .ok()
        .map(|v| v.trim().to_ascii_lowercase())
        .filter(|v| matches!(v.as_str(), "low" | "medium" | "high"))
        .unwrap_or_else(|| "medium".into())
}

fn tools_wire(req: &ResponseRequest) -> bool {
    req.tools.as_ref().is_some_and(|t| !t.is_empty()) && !native_delegate_enabled()
}

/// Token nur stores for the `factory` provider when the user is signed in
/// through the CLI itself (`droid`). Not a Bearer token: it only records that
/// the Factory session exists, so `droid exec` runs with its own OAuth
/// credentials and nothing secret is copied into nur.
pub const FACTORY_CLI_SESSION_TOKEN: &str = "factory-cli-session";

pub fn is_cli_session_token(token: &str) -> bool {
    let t = token.trim();
    t == FACTORY_CLI_SESSION_TOKEN || t.starts_with("factory-cli:")
}

/// `FACTORY_HOME` when set, else `~/.factory` - where the CLI keeps its
/// settings, sessions and the encrypted OAuth keyring.
pub fn factory_home() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("FACTORY_HOME") {
        let p = PathBuf::from(dir);
        if p.is_dir() {
            return Some(p);
        }
    }
    crate::config::home_dir().map(|h| h.join(".factory"))
}

/// True when `droid` holds a browser sign-in. The keyring is encrypted with a
/// secret only the CLI can unseal, so nur checks for its presence and lets the
/// CLI do the talking instead of trying to read the token.
pub fn oauth_session_present() -> bool {
    let Some(home) = factory_home() else {
        return false;
    };
    ["auth.v2.keyring", "auth.keyring", "auth.json"]
        .iter()
        .any(|name| std::fs::metadata(home.join(name)).is_ok_and(|m| m.is_file() && m.len() > 0))
}

/// Credentials nur records for a live `droid` sign-in (no secret copied).
pub fn session_tokens_from_cli() -> Option<crate::oauth::OAuthTokens> {
    if !oauth_session_present() {
        return None;
    }
    Some(crate::oauth::OAuthTokens {
        access_token: FACTORY_CLI_SESSION_TOKEN.into(),
        refresh_token: Some("droid".into()),
        expires_at: None,
        meta: Some(crate::auth::OauthMeta {
            issuer: "factory".into(),
            client_id: "droid".into(),
            extra: serde_json::json!({
                "imported_from": "factory-cli-keyring",
                "cli_session": true,
            }),
        }),
    })
}

/// Locate the `droid` launcher. The official installer and the npm package
/// disagree about where it lands, and neither guarantees it is on `PATH` for
/// the shell nur inherits, so probe every documented location before giving up.
pub fn resolve_bin() -> Option<PathBuf> {
    // A fresh install mid-session must be picked up, so the miss is only
    // cached briefly; a hit is re-validated the same way.
    static CACHE: std::sync::Mutex<Option<(Instant, Option<PathBuf>)>> =
        std::sync::Mutex::new(None);
    const TTL: Duration = Duration::from_secs(60);
    if let Ok(guard) = CACHE.lock() {
        if let Some((at, found)) = guard.as_ref() {
            if at.elapsed() < TTL && found.as_ref().is_none_or(|p| p.is_file()) {
                return found.clone();
            }
        }
    }
    let found = probe_bin();
    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some((Instant::now(), found.clone()));
    }
    found
}

fn probe_bin() -> Option<PathBuf> {
    const NAMES: [&str; 3] = ["droid.exe", "droid.cmd", "droid"];
    if let Ok(explicit) = std::env::var("NUR_DROID_BIN") {
        let p = PathBuf::from(explicit.trim());
        if p.is_file() {
            return Some(p);
        }
    }
    let mut dirs_to_try: Vec<PathBuf> = Vec::new();
    if let Some(home) = crate::config::home_dir() {
        // `curl … | sh` installs under ~/.factory/bin; older builds and the
        // Windows installer drop the launcher in ~/bin or ~/.local/bin.
        dirs_to_try.push(home.join(".factory").join("bin"));
        dirs_to_try.push(home.join(".local").join("bin"));
        dirs_to_try.push(home.join("bin"));
    }
    if let Some(local) = dirs::data_local_dir() {
        dirs_to_try.push(local.join("Programs").join("factory"));
        dirs_to_try.push(local.join("factory").join("bin"));
    }
    #[cfg(not(windows))]
    {
        dirs_to_try.push(PathBuf::from("/usr/local/bin"));
        dirs_to_try.push(PathBuf::from("/opt/factory/bin"));
    }
    for dir in dirs_to_try {
        for name in NAMES {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    crate::ecosystem::find_bin("droid").map(PathBuf::from)
}

/// True when a Factory key is available for this call: the one nur holds for
/// the provider, or the CLI's own environment variable.
fn has_api_key(passed: Option<&str>) -> bool {
    passed
        .map(str::trim)
        .is_some_and(|k| !k.is_empty() && !is_cli_session_token(k))
        || std::env::var("FACTORY_API_KEY")
            .ok()
            .is_some_and(|k| !k.trim().is_empty())
}

/// What the user should do when `droid exec` cannot run.
fn auth_hint() -> &'static str {
    if oauth_session_present() {
        " The Factory CLI is signed in, so this is a launcher problem: set NUR_DROID_BIN to the \
         full path of your `droid` binary."
    } else {
        " Sign in with `droid` (interactive, browser OAuth) or set FACTORY_API_KEY."
    }
}

fn provider_turn_timeout() -> Duration {
    std::env::var("NUR_PROVIDER_TURN_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|s| *s > 0)
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(300))
}

/// No stdout/stderr line for this long means the CLI is wedged (most often a
/// permission prompt that has no console in exec mode).
///
/// `--output-format json` prints nothing until the run is over, so silence is
/// normal: the default is the whole turn budget and only an explicit
/// `NUR_DROID_IDLE_TIMEOUT_SECS` cuts a quiet run short.
fn idle_timeout(total: Duration) -> Duration {
    std::env::var("NUR_DROID_IDLE_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|s| *s > 0)
        .map(Duration::from_secs)
        .unwrap_or(total)
}

/// Run a non-streaming Droid completion. `api_key` is a key nur holds for the
/// `factory` provider (pasted into `/login`); it reaches the CLI as
/// `FACTORY_API_KEY` for that child only, so the CLI's own sign-in still
/// works when it is empty.
pub fn complete(
    req: &ResponseRequest,
    cancel: &tokio_util::sync::CancellationToken,
    chat: Option<&Arc<Mutex<DroidChatState>>>,
    api_key: Option<&str>,
) -> Result<ApiResponse> {
    let parse_tools = tools_wire(req);
    let (text, model, usage) = run_exec(req, None, cancel, provider_turn_timeout(), chat, api_key)?;
    Ok(response_from_agent_cli_text(
        "factory",
        &model,
        &text,
        parse_tools,
        usage,
    ))
}

/// Streaming variant. `droid exec --output-format json` prints only the final
/// object, so progress is limited to launch/completion notes; the final text
/// is delivered as one `TextDelta` so the TUI renders it like any other model.
pub fn complete_stream(
    req: &ResponseRequest,
    mut on_event: impl FnMut(StreamEvent),
    cancel: &tokio_util::sync::CancellationToken,
    chat: Option<&Arc<Mutex<DroidChatState>>>,
    api_key: Option<&str>,
) -> Result<ApiResponse> {
    let parse_tools = tools_wire(req);
    on_event(StreamEvent::ReasoningDelta(
        "droid exec · launching\n".into(),
    ));
    let (text, model, usage) = run_exec(
        req,
        Some(&mut |ev| on_event(ev)),
        cancel,
        provider_turn_timeout() + Duration::from_secs(30),
        chat,
        api_key,
    )?;
    let resp = response_from_agent_cli_text("factory", &model, &text, parse_tools, usage);
    let visible = resp.output_text();
    if !visible.is_empty() {
        on_event(StreamEvent::TextDelta(visible));
    }
    on_event(StreamEvent::Completed(resp.clone()));
    Ok(resp)
}

fn run_exec(
    req: &ResponseRequest,
    mut on_event: Option<&mut dyn FnMut(StreamEvent)>,
    cancel: &tokio_util::sync::CancellationToken,
    total_timeout: Duration,
    chat: Option<&Arc<Mutex<DroidChatState>>>,
    api_key: Option<&str>,
) -> Result<(String, String, Option<ApiUsage>)> {
    let Some(bin) = resolve_bin() else {
        let base = if oauth_session_present() {
            "droid is signed in on this machine but its launcher was not found. Point nur at it \
             with NUR_DROID_BIN=<path to droid>, or add the binary's directory to PATH"
        } else {
            "droid not found. Install Factory's CLI (https://docs.factory.ai/droid-cli/quickstart: \
             `curl -fsSL https://app.factory.ai/cli | sh`, or `npm i -g @factory/cli`), run `droid` \
             once to sign in, and set NUR_DROID_BIN if it lands outside PATH"
        };
        return Err(NurError::Other(format!(
            "{base}. Factory has no HTTP chat endpoint, so FACTORY_API_KEY alone cannot replace the CLI."
        )));
    };
    let items_len = req.input.as_array().map(|a| a.len()).unwrap_or(0);
    let (resume_id, resume_from): (Option<String>, usize) = match chat.and_then(|c| c.lock().ok()) {
        Some(state) if state.session_id.is_some() && items_len > state.items_sent => {
            (state.session_id.clone(), state.items_sent)
        }
        _ => (None, 0),
    };
    let mut retried = false;
    loop {
        let attempt_resume = if retried { None } else { resume_id.clone() };
        let prompt = match &attempt_resume {
            Some(_) => {
                let items = req.input.as_array().cloned().unwrap_or_default();
                let from = resume_from.min(items.len());
                flatten_incremental(&items[from..], tools_wire(req))
            }
            None => flatten_prompt_with(req, tools_wire(req), "Factory Droid"),
        };
        let launch = Launch {
            bin: &bin,
            prompt: &prompt,
            resume_id: attempt_resume.as_deref(),
            api_key,
            total_timeout,
        };
        match run_exec_once(&launch, req, &mut on_event, cancel) {
            Ok((text, model, usage, session)) => {
                if let Some(chat) = chat {
                    if let Ok(mut st) = chat.lock() {
                        st.session_id = session.or_else(|| resume_id.clone());
                        st.items_sent = items_len;
                    }
                }
                return Ok((text, model, usage));
            }
            Err(e) => {
                if let Some(chat) = chat {
                    if let Ok(mut st) = chat.lock() {
                        st.session_id = None;
                        st.items_sent = 0;
                    }
                }
                // A session the CLI no longer has (pruned, other machine) is
                // reported as an error; retry once from a fresh session. A real
                // model, auth or quota failure repeats on retry, so it is not
                // worth paying for the whole prompt twice.
                if attempt_resume.is_some() && !retried && is_retryable_resume_failure(&e) {
                    retried = true;
                    continue;
                }
                return Err(e);
            }
        }
    }
}

/// Failures that mean the resumed session, not the request, was the problem:
/// a session id the CLI no longer has produces no result object, no assistant
/// text, or silence until a cap.
fn is_retryable_resume_failure(err: &NurError) -> bool {
    if matches!(err, NurError::Interrupted) {
        return false;
    }
    let msg = err.to_string();
    msg.contains("idle timeout")
        || msg.contains("provider turn timeout")
        || msg.contains("produced no assistant text")
        || msg.contains("produced no result object")
}

/// Everything one `droid exec` launch needs besides the request itself.
struct Launch<'a> {
    bin: &'a PathBuf,
    prompt: &'a str,
    resume_id: Option<&'a str>,
    api_key: Option<&'a str>,
    total_timeout: Duration,
}

fn run_exec_once(
    launch: &Launch<'_>,
    req: &ResponseRequest,
    on_event: &mut Option<&mut dyn FnMut(StreamEvent)>,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<(String, String, Option<ApiUsage>, Option<String>)> {
    let Launch {
        bin,
        prompt,
        resume_id,
        api_key,
        total_timeout,
    } = *launch;
    let model = Some(req.model.trim())
        .filter(|m| !m.is_empty() && *m != "auto")
        .map(str::to_string);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let native = req.tools.as_ref().is_some_and(|t| !t.is_empty()) && native_delegate_enabled();

    let mut args: Vec<String> = vec![
        "exec".into(),
        "--output-format".into(),
        "json".into(),
        "--cwd".into(),
        cwd.to_string_lossy().into_owned(),
    ];
    if native {
        args.push("--auto".into());
        args.push(native_auto_level());
    }
    if let Some(m) = &model {
        args.push("--model".into());
        args.push(m.clone());
    }
    if let Some(effort) = req
        .reasoning
        .as_ref()
        .and_then(|r| r.effort.as_deref())
        .filter(|e| matches!(*e, "low" | "medium" | "high"))
    {
        args.push("--reasoning-effort".into());
        args.push(effort.into());
    }
    if let Some(id) = resume_id {
        args.push("--session-id".into());
        args.push(id.into());
    }
    // Windows CreateProcess caps the command line near 8k chars; the prompt
    // goes through stdin (documented `echo … | droid exec`) unless it is short.
    // A `.cmd`/`.bat` launcher is routed through cmd.exe, which rejects
    // arguments containing newlines and expands `%VAR%`, so those never take
    // the prompt on argv.
    const ARG_BUDGET: usize = 4_000;
    let shim = bin
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    let pass_as_arg = !shim && prompt.len() <= ARG_BUDGET && !prompt.starts_with('-');
    if pass_as_arg {
        args.push(prompt.to_string());
    }

    let mut cmd = Command::new(bin);
    cmd.args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("NO_COLOR", "1");
    if let Some(key) = api_key
        .map(str::trim)
        .filter(|k| !k.is_empty() && !is_cli_session_token(k))
    {
        cmd.env("FACTORY_API_KEY", key);
    }
    // Owning the tree matters here: the launcher is usually a shim, so killing
    // the direct child would leave the real agent running (and billing).
    let (mut child, tree) = crate::process_tree::ProcessTree::spawn_with_flags(
        &mut cmd,
        crate::process_tree::CREATE_NO_WINDOW,
    )
    .map_err(|e| NurError::Other(format!("failed to launch droid: {e}{}", auth_hint())))?;
    let stdin = child.stdin.take();
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| NurError::Other("droid stdout missing".into()))?;
    let stderr = child.stderr.take();
    // A prompt larger than the pipe buffer deadlocks if it is written before
    // the output readers exist, so it goes out on its own thread.
    let stdin_prompt = (!pass_as_arg).then(|| prompt.to_string());
    std::thread::spawn(move || {
        if let Some(mut stdin) = stdin {
            if let Some(text) = stdin_prompt {
                let _ = stdin.write_all(text.as_bytes());
                let _ = stdin.write_all(b"\n");
            }
        }
    });
    let (err_tx, err_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        if let Some(err) = stderr {
            for line in BufReader::new(err).lines() {
                if err_tx.send(line).is_err() {
                    break;
                }
            }
        }
    });
    let (out_tx, out_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if out_tx.send(line).is_err() {
                break;
            }
        }
    });

    let started = Instant::now();
    let idle = idle_timeout(total_timeout);
    let mut last_activity = Instant::now();
    let mut raw_out = String::new();
    let mut err_text = String::new();
    let mut status = None;
    let mut last_stdout = Instant::now();
    loop {
        for line in err_rx.try_iter().flatten() {
            last_activity = Instant::now();
            if !err_text.is_empty() {
                err_text.push('\n');
            }
            err_text.push_str(&line);
        }
        if cancel.is_cancelled() {
            tree.terminate(&mut child);
            return Err(NurError::Interrupted);
        }
        if started.elapsed() >= total_timeout {
            tree.terminate(&mut child);
            return Err(NurError::Other(format!(
                "droid exec exceeded the provider turn timeout ({}s, NUR_PROVIDER_TURN_TIMEOUT_SECS adjusts it)",
                total_timeout.as_secs()
            )));
        }
        if status.is_none() && last_activity.elapsed() >= idle {
            tree.terminate(&mut child);
            return Err(NurError::Other(format!(
                "droid exec produced no output for {}s (idle timeout, NUR_DROID_IDLE_TIMEOUT_SECS adjusts it)",
                idle.as_secs()
            )));
        }
        match out_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(line)) => {
                last_stdout = Instant::now();
                last_activity = last_stdout;
                raw_out.push_str(&line);
                raw_out.push('\n');
            }
            Ok(Err(_)) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }
        if status.is_none() {
            match child.try_wait() {
                Ok(done) => status = done,
                Err(e) => {
                    tree.terminate(&mut child);
                    return Err(NurError::Other(format!("droid wait: {e}")));
                }
            }
        }
        if status.is_some() && last_stdout.elapsed() >= Duration::from_millis(250) {
            break;
        }
    }
    for line in err_rx.try_iter().flatten() {
        if !err_text.is_empty() {
            err_text.push('\n');
        }
        err_text.push_str(&line);
    }

    let parsed = parse_exec_output(&raw_out);
    if let Some(ExecResult {
        is_error: true,
        text,
        ..
    }) = &parsed
    {
        return Err(NurError::Other(format!(
            "droid exec reported an error: {}",
            text.chars().take(600).collect::<String>()
        )));
    }
    if parsed.is_none() {
        let exit = status
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unknown".into());
        let detail = if !err_text.trim().is_empty() {
            err_text.trim().chars().take(600).collect::<String>()
        } else {
            raw_out.trim().chars().take(600).collect::<String>()
        };
        let lower = detail.to_ascii_lowercase();
        let hint = if lower.contains("not logged in")
            || lower.contains("unauthorized")
            || lower.contains("api key")
            || (!has_api_key(api_key) && !oauth_session_present())
        {
            auth_hint()
        } else {
            ""
        };
        return Err(NurError::Other(format!(
            "droid exec produced no result object (exit {exit}): {detail}{hint}"
        )));
    }
    let result = parsed.expect("checked above");
    if let Some(cb) = on_event.as_mut() {
        cb(StreamEvent::ReasoningDelta(format!(
            "droid exec · done in {:.1}s ({} turn{})\n",
            result.duration_ms as f64 / 1000.0,
            result.num_turns,
            if result.num_turns == 1 { "" } else { "s" }
        )));
    }
    if result.text.trim().is_empty() {
        return Err(NurError::Other(
            "droid exec produced no assistant text".into(),
        ));
    }
    Ok((
        result.text,
        model.unwrap_or_else(|| "auto".into()),
        result.usage,
        result.session_id,
    ))
}

/// The one object `--output-format json` prints at the end of a run.
#[derive(Debug, Clone)]
pub(crate) struct ExecResult {
    pub text: String,
    pub is_error: bool,
    pub session_id: Option<String>,
    pub duration_ms: u64,
    pub num_turns: u64,
    pub usage: Option<ApiUsage>,
}

/// Find the result object in the CLI's stdout. The CLI may print update or
/// warning lines before it (deprecation notices, telemetry), and the object
/// may be pretty-printed over several lines, so parse from the last `{` that
/// opens a `"type":"result"` document rather than assuming one JSON line.
pub(crate) fn parse_exec_output(raw: &str) -> Option<ExecResult> {
    let candidates = raw.match_indices('{').map(|(i, _)| i).collect::<Vec<_>>();
    for start in candidates.into_iter().rev() {
        let slice = raw[start..].trim_end();
        // Read one document and ignore whatever the CLI printed after it
        // (update nags, telemetry notices), which a whole-slice parse rejects.
        let Some(Ok(v)) = serde_json::Deserializer::from_str(slice)
            .into_iter::<Value>()
            .next()
        else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("result") {
            continue;
        }
        let text = match v.get("result") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Null) | None => String::new(),
            Some(other) => other.to_string(),
        };
        let is_error = v.get("is_error").and_then(|b| b.as_bool()).unwrap_or(false)
            || v.get("subtype").and_then(|s| s.as_str()) == Some("error");
        let usage = v.get("usage").and_then(|u| {
            let input = u
                .get("input_tokens")
                .or_else(|| u.get("prompt_tokens"))
                .and_then(|n| n.as_u64());
            let output = u
                .get("output_tokens")
                .or_else(|| u.get("completion_tokens"))
                .and_then(|n| n.as_u64());
            (input.is_some() || output.is_some()).then(|| ApiUsage {
                input_tokens: input.unwrap_or(0),
                output_tokens: output.unwrap_or(0),
                total_tokens: input.unwrap_or(0) + output.unwrap_or(0),
                output_tokens_details: None,
                input_tokens_details: None,
                cache_write_tokens: 0,
            })
        });
        return Some(ExecResult {
            text,
            is_error,
            session_id: v
                .get("session_id")
                .and_then(|s| s.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            duration_ms: v.get("duration_ms").and_then(|n| n.as_u64()).unwrap_or(0),
            num_turns: v.get("num_turns").and_then(|n| n.as_u64()).unwrap_or(0),
            usage,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ways the parse can fail: a warning line before the object, a
    /// pretty-printed object, an error result, and no object at all.
    #[test]
    fn result_object_is_found_after_noise_and_across_lines() {
        let raw = "warning: stream-json is deprecated\n{\n  \"type\": \"result\",\n  \"subtype\": \"success\",\n  \"is_error\": false,\n  \"duration_ms\": 5657,\n  \"num_turns\": 1,\n  \"result\": \"Hello {world}\",\n  \"session_id\": \"8af22e0a\"\n}\n";
        let r = parse_exec_output(raw).expect("result");
        assert_eq!(r.text, "Hello {world}");
        assert!(!r.is_error);
        assert_eq!(r.session_id.as_deref(), Some("8af22e0a"));
        assert_eq!(r.num_turns, 1);
    }

    #[test]
    fn error_results_and_missing_objects_are_distinguished() {
        let err = parse_exec_output(
            r#"{"type":"result","subtype":"error","is_error":true,"result":"Not logged in"}"#,
        )
        .expect("parsed");
        assert!(err.is_error);
        assert_eq!(err.text, "Not logged in");
        assert!(parse_exec_output("Factory Droid v1.2.3\nnothing here").is_none());
        assert!(parse_exec_output(r#"{"type":"system","subtype":"init"}"#).is_none());
    }

    /// Ways the model list can fail: the help table with labels and a
    /// "(default)" marker, the error form's comma list wrapped over lines,
    /// warnings before the section, and output with no section at all.
    #[test]
    fn model_list_is_read_from_help_table_or_error_list() {
        let help = "warn: ignoring extra certs\nUsage: droid exec [options]\n\nTool Controls:\n  --list-tools   Print tools\n\nAvailable Models:\n  auto                         Auto Model\n  claude-opus-5                Opus 5 (default)\n  claude-sonnet-5              Sonnet 5\n  gpt-5.6-sol                  GPT-5.6 Sol\n  inkling                      Inkling (Droid Core)\n\nModel details:\n  - Auto Model: supports reasoning: No\n";
        assert_eq!(
            parse_model_list(help),
            [
                "auto",
                "claude-opus-5",
                "claude-sonnet-5",
                "gpt-5.6-sol",
                "inkling"
            ]
        );
        let error = "Invalid model: nope\n\nAvailable built-in models:\n  auto, claude-opus-5, claude-sonnet-5,\n  gpt-5.6-sol, inkling\n\nNo custom models configured. Add them to ~/.factory/settings.json\n";
        assert_eq!(
            parse_model_list(error),
            [
                "auto",
                "claude-opus-5",
                "claude-sonnet-5",
                "gpt-5.6-sol",
                "inkling"
            ]
        );
        assert!(parse_model_list(
            "Usage: droid exec [options]\n\nOptions:\n  -m, --model <id>  Model ID\n"
        )
        .is_empty());
    }

    #[test]
    fn usage_is_only_reported_when_the_cli_sent_it() {
        let with = parse_exec_output(
            r#"{"type":"result","result":"ok","usage":{"input_tokens":12,"output_tokens":3}}"#,
        )
        .expect("parsed");
        assert_eq!(with.usage.as_ref().map(|u| u.total_tokens), Some(15));
        let without = parse_exec_output(r#"{"type":"result","result":"ok"}"#).expect("parsed");
        assert!(without.usage.is_none());
    }
}

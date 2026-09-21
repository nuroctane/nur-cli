//! TypeSafe System One transport: one endpoint, batched questions, parallel
//! requests, typed answers.
//!
//! Upstream contract: `POST https://api.typesafe.ai/v1/systemone` with
//! `{ state, model, questions }` → `{ model, answers, usage }`. See
//! <https://docs.typesafe.ai/api>. `jev-latest` is TypeSafe's flagship System
//! One model.
//!
//! Shape of this client, and why:
//!
//! - **Questions are batched.** Independent questions over the same state go in
//!   one request; they run in parallel upstream and cost no extra round trip.
//! - **Batches within a batch.** A very large question set is split into
//!   `max_questions_per_request` chunks, sent on real threads, and merged. The
//!   loop never pays latency linearly in question count.
//! - **Partial failure is data, not death.** A batch that fails after its
//!   retries is reported in [`Batch::errors`]; every answer that did arrive is
//!   still returned. Callers treat a missing answer as "no judgment" and fall
//!   back to their own default - they never invent one.
//! - **One blocking core.** [`TypesafeClient::ask`] is blocking; callers in the
//!   async loop wrap it in `spawn_blocking` (see `agent::loop`), so there is no
//!   second HTTP implementation to keep in sync.
//!
//! Transport is injectable ([`Transport`]) so tests exercise batching, retries
//! and merging without the network or an API key.

use super::questions::{Answer, Question};
use super::telemetry;
use crate::config::TypesafeConfig;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
/// System One endpoint. Config `[typesafe] base_url` overrides it.
pub const DEFAULT_BASE_URL: &str = "https://api.typesafe.ai/v1/systemone";
/// TypeSafe's flagship System One model.
pub const DEFAULT_MODEL: &str = "jev-latest";
/// Stand-in "key" for a local engine, which authenticates by being on this
/// machine. Never a real credential: the loopback check runs before any request,
/// so it cannot reach a hosted endpoint.
pub const LOCAL_KEY_PLACEHOLDER: &str = "local-no-key-needed";
/// Env vars tried, in order, for the key.
pub const KEY_ENVS: &[&str] = &["TYPESAFE_API_KEY", "TYPESAFE_KEY", "JEV_API_KEY"];
/// Credential-store ids tried, in order (`nur auth login --provider typesafe`).
pub const KEY_PROVIDER_IDS: &[&str] = &["typesafe", "jev"];

/// Questions merged into one request when the caller does not say otherwise.
pub const DEFAULT_MAX_QUESTIONS_PER_REQUEST: usize = 24;

/// The typed answers for one `ask`.
#[derive(Debug, Clone, Default)]
pub struct Batch {
    pub model: String,
    pub answers: BTreeMap<String, Answer>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Requests actually sent (1 unless the question set was split).
    pub requests: usize,
    /// True when more than one request ran concurrently.
    pub parallel: bool,
    /// Per-batch failures, after retries. Never fatal on its own.
    pub errors: Vec<String>,
}

impl Batch {
    pub fn get(&self, id: &str) -> Option<&Answer> {
        self.answers.get(id)
    }

    pub fn is_empty(&self) -> bool {
        self.answers.is_empty()
    }

    /// Questions that produced no answer (failed batch, or omitted by the API).
    pub fn missing<'a>(&self, asked: impl IntoIterator<Item = &'a str>) -> Vec<String> {
        asked
            .into_iter()
            .filter(|id| !self.answers.contains_key(*id))
            .map(str::to_string)
            .collect()
    }
}

/// One HTTP request body → response body, or a transport error.
#[cfg(test)]
pub type TransportFn = dyn Fn(&Value) -> Result<Value, String> + Send + Sync;

/// Where requests go. `Fake` exists for tests and for replaying recorded runs.
#[derive(Clone)]
pub enum Transport {
    /// Real HTTP via `reqwest::blocking`.
    Http(reqwest::blocking::Client),
    /// In-process endpoint for tests: batching, retries and merging are
    /// exercised without a key or the network.
    #[cfg(test)]
    Fake(Arc<TransportFn>),
}

impl std::fmt::Debug for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(_) => f.write_str("Transport::Http"),
            #[cfg(test)]
            Self::Fake(_) => f.write_str("Transport::Fake"),
        }
    }
}

/// Run blocking HTTP work off any async runtime.
///
/// `reqwest::blocking::Client` spins up its own tokio runtime, and building or
/// dropping it while a runtime is active panics:
///
/// ```text
/// Cannot drop a runtime in a context where blocking is not allowed.
/// This happens when a runtime is dropped from within an asynchronous context.
/// ```
///
/// nur calls Jev from several places: inside `spawn_blocking` (the tool layer),
/// from plain threads (batching), and from the *prompt build* path, which runs on
/// a tokio worker (skill requirement selection). Rather than make every caller
/// remember to wrap the call, the blocking work hops to a plain scoped thread
/// whenever a runtime is active. The hop costs microseconds next to a request.
fn off_runtime<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    if tokio::runtime::Handle::try_current().is_err() {
        return f();
    }
    std::thread::scope(|scope| {
        scope
            .spawn(f)
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}

/// A configured TypeSafe endpoint with a resolved key.
pub struct TypesafeClient {
    key: String,
    base_url: String,
    model: String,
    retries: u32,
    max_questions: usize,
    max_parallel: usize,
    transport: Transport,
}

/// Whether the harness can reach TypeSafe right now.
pub enum Availability {
    Ready(Box<TypesafeClient>),
    /// No key (or no config): the harness keeps its non-Jev behaviour.
    Unavailable(String),
}

impl Availability {
    pub fn client(&self) -> Option<&TypesafeClient> {
        match self {
            Self::Ready(c) => Some(c),
            Self::Unavailable(_) => None,
        }
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Unavailable(r) => Some(r.as_str()),
            Self::Ready(_) => None,
        }
    }

    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready(_))
    }
}

/// The key for this machine, from the first source that has one.
///
/// Order: `[typesafe] api_key` → `TYPESAFE_API_KEY`/`TYPESAFE_KEY`/`JEV_API_KEY`
/// → nur's credential store (`nur auth login --provider typesafe`) →
/// `~/.nur/typesafe.key` → `~/.typesafe_key` → `~/.config/typesafe/key`.
pub fn api_key(cfg: &TypesafeConfig) -> Option<String> {
    let explicit = cfg.api_key.trim();
    if !explicit.is_empty() {
        // Config is already in memory: never cached, so an edit takes effect at
        // once.
        return Some(explicit.to_string());
    }
    // The probe reads the environment, the credential store and a few files.
    // That is far too much work for a path the agent loop walks on every tool
    // batch, so the *resolved* value - including "no key" - is cached briefly.
    // A key saved mid-session is therefore picked up within the TTL, which is
    // also what makes `doctor` and a fresh `/auth` login agree quickly.
    if let Ok(guard) = KEY_PROBE.lock() {
        if let Some((at, cached)) = guard.as_ref() {
            if at.elapsed() < KEY_PROBE_TTL {
                return cached.clone();
            }
        }
    }
    let resolved = api_key_from_env()
        .or_else(|| {
            KEY_PROVIDER_IDS.iter().find_map(|id| {
                crate::auth::load_provider_key(id)
                    .map(|k| k.trim().to_string())
                    .filter(|k| !k.is_empty())
            })
        })
        .or_else(api_key_from_files);
    if let Ok(mut guard) = KEY_PROBE.lock() {
        *guard = Some((Instant::now(), resolved.clone()));
    }
    resolved
}

/// How long a resolved key (or its absence) is trusted before re-probing.
const KEY_PROBE_TTL: Duration = Duration::from_secs(5);

static KEY_PROBE: std::sync::Mutex<Option<(Instant, Option<String>)>> = std::sync::Mutex::new(None);

/// Drop the cached probe. Called after a credential change so the next caller
/// sees it immediately rather than within the TTL.
pub fn invalidate_key_probe() {
    if let Ok(mut guard) = KEY_PROBE.lock() {
        *guard = None;
    }
}

fn api_key_from_env() -> Option<String> {
    for var in KEY_ENVS {
        if let Ok(v) = std::env::var(var) {
            let v = v.trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn api_key_from_files() -> Option<String> {
    let home = crate::config::nur_home();
    let candidates = [
        home.join("typesafe.key"),
        home.join("keys").join("typesafe.key"),
        dirs::home_dir().unwrap_or_default().join(".typesafe_key"),
        dirs::config_dir()
            .unwrap_or_default()
            .join("typesafe")
            .join("key"),
    ];
    for path in candidates {
        if let Ok(raw) = std::fs::read_to_string(&path) {
            let key = raw.trim();
            if !key.is_empty() {
                return Some(key.to_string());
            }
        }
    }
    None
}

/// The local-bridge base URL, when one is configured by environment.
///
/// `NUR_JEV_LOCAL_URL` is the ergonomic path for the local System One bridge
/// (`scripts/jev_local_bridge.py`): it overrides `[typesafe] base_url`, and it
/// needs no key, because the engine is on this machine.
pub fn local_bridge_env() -> Option<String> {
    let raw = std::env::var("NUR_JEV_LOCAL_URL").ok()?;
    normalize_local_bridge_url(&raw)
}

/// Accept either `http://127.0.0.1:8788` or the full endpoint path, so a user
/// cannot get the URL subtly wrong. Pure, so it is testable without touching the
/// process environment.
pub fn normalize_local_bridge_url(raw: &str) -> Option<String> {
    // Trim the trailing slash *before* testing for the endpoint path: otherwise
    // `http://127.0.0.1:8788/v1/systemone/` looked like a bare base URL and became
    // `…/v1/systemone/v1/systemone`, which 404s as a permanent error - and since a
    // loopback endpoint never needs a key, nothing else reveals the typo.
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.ends_with("/v1/systemone") {
        Some(trimmed.to_string())
    } else {
        Some(format!("{trimmed}/v1/systemone"))
    }
}

/// Is this endpoint on this machine?
///
/// A loopback engine was started and trusted by the user, so nur does not demand
/// a credential for it. Anything reachable off-box still needs a key.
pub fn is_loopback_endpoint(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    let authority = lower
        .split("://")
        .nth(1)
        .unwrap_or(&lower)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .trim_end_matches('/')
        .to_string();
    // Drop userinfo, then the port (keeping an IPv6 literal intact).
    let authority = authority
        .rsplit('@')
        .next()
        .unwrap_or(&authority)
        .to_string();
    let host = if authority.starts_with('[') {
        authority
            .split(']')
            .next()
            .unwrap_or(&authority)
            .trim_start_matches('[')
    } else {
        authority.split(':').next().unwrap_or(&authority)
    };
    matches!(
        host,
        "127.0.0.1" | "localhost" | "::1" | "0.0.0.0" | "host.docker.internal"
    )
}

/// The effective endpoint: a local-bridge env override wins over config.
pub fn effective_base_url(cfg: &TypesafeConfig) -> String {
    if let Some(local) = local_bridge_env() {
        return local;
    }
    let configured = cfg.base_url.trim();
    if configured.is_empty() {
        DEFAULT_BASE_URL.to_string()
    } else {
        configured.to_string()
    }
}

/// Where the key came from, for `doctor` (never the key itself).
pub fn key_provenance(cfg: &TypesafeConfig) -> Option<&'static str> {
    if !cfg.api_key.trim().is_empty() {
        return Some("config [typesafe] api_key");
    }
    if api_key_from_env().is_some() {
        return Some("env TYPESAFE_API_KEY");
    }
    for id in KEY_PROVIDER_IDS {
        if crate::auth::load_provider_key(id).is_some() {
            return Some("nur credential store (nur auth login --provider typesafe)");
        }
    }
    if api_key_from_files().is_some() {
        return Some("~/.nur/typesafe.key");
    }
    if local_bridge_env().is_some() {
        return Some("local bridge (NUR_JEV_LOCAL_URL - no key needed)");
    }
    None
}

/// A redacted fingerprint, safe to print.
pub fn key_fingerprint(key: &str) -> String {
    crate::auth::key_fingerprint(key)
}

/// Build a client when a key exists. `cfg.enabled=false` is an opt-out, not an
/// error: the caller keeps its own behaviour either way.
pub fn client(cfg: &TypesafeConfig) -> Availability {
    if !cfg.enabled {
        return Availability::Unavailable("disabled in config ([typesafe] enabled = false)".into());
    }
    let base_url = effective_base_url(cfg);
    let key =
        match api_key(cfg) {
            Some(k) => k,
            // A local engine needs no credential - that is the whole point of
            // running one.
            None if is_loopback_endpoint(&base_url) => LOCAL_KEY_PLACEHOLDER.to_string(),
            None => return Availability::Unavailable(
                "no TypeSafe key (set TYPESAFE_API_KEY, or `nur auth login --provider typesafe`), \
                 and this endpoint is not local - a loopback base_url or NUR_JEV_LOCAL_URL needs \
                 no key"
                    .into(),
            ),
        };
    let timeout = Duration::from_millis(cfg.timeout_ms.clamp(1_000, 180_000));
    // Off-runtime: constructing this client on a tokio worker panics (see
    // `off_runtime`).
    let http = match off_runtime(|| {
        reqwest::blocking::Client::builder()
            .timeout(timeout)
            .user_agent(format!("nur-cli/{}", env!("CARGO_PKG_VERSION")))
            .build()
    }) {
        Ok(c) => c,
        Err(e) => return Availability::Unavailable(format!("http client: {e}")),
    };
    Availability::Ready(Box::new(TypesafeClient {
        key,
        base_url,
        model: if cfg.model.trim().is_empty() {
            DEFAULT_MODEL.to_string()
        } else {
            cfg.model.trim().to_string()
        },
        retries: cfg.retries.min(5),
        max_questions: cfg
            .max_questions_per_request
            .clamp(1, super::questions::MAX_CHOICE_OPTIONS),
        max_parallel: cfg.max_parallel.clamp(1, 16),
        transport: Transport::Http(http),
    }))
}

/// Process-wide client cache keyed by a config fingerprint.
///
/// Building a `reqwest::blocking::Client` per call would throw away the
/// connection pool and add a TLS handshake to every judgment. The fingerprint
/// covers everything that changes routing, and uses a key *fingerprint* (never
/// the raw key) so no secret lands in a map key.
pub fn shared_client(cfg: &TypesafeConfig) -> Option<Arc<TypesafeClient>> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, Arc<TypesafeClient>>>,
    > = std::sync::OnceLock::new();
    if !cfg.enabled {
        return None;
    }
    let base_url = effective_base_url(cfg);
    let key = match api_key(cfg) {
        Some(k) => k,
        // A local bridge needs no key; hosted endpoints still do.
        None if is_loopback_endpoint(&base_url) => LOCAL_KEY_PLACEHOLDER.to_string(),
        None => return None,
    };
    let fingerprint = format!(
        "{}|{}|{}|{}|{}|{}|{}",
        key_fingerprint(&key),
        base_url,
        cfg.model,
        cfg.timeout_ms,
        cfg.max_questions_per_request,
        cfg.max_parallel,
        // Without this, editing `[typesafe] retries` mid-session kept using the
        // cached client's old value until restart.
        cfg.retries
    );
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    if let Ok(g) = cache.lock() {
        if let Some(c) = g.get(&fingerprint) {
            return Some(c.clone());
        }
    }
    let client = match client(cfg) {
        Availability::Ready(c) => Arc::new(*c),
        Availability::Unavailable(_) => return None,
    };
    if let Ok(mut g) = cache.lock() {
        g.insert(fingerprint, client.clone());
    }
    Some(client)
}

impl TypesafeClient {
    /// A client with an injected transport (tests).
    #[cfg(test)]
    pub fn with_transport(cfg: &TypesafeConfig, transport: Transport) -> Self {
        Self {
            key: api_key(cfg).unwrap_or_else(|| "test".into()),
            base_url: if cfg.base_url.trim().is_empty() {
                DEFAULT_BASE_URL.to_string()
            } else {
                cfg.base_url.trim().to_string()
            },
            model: if cfg.model.trim().is_empty() {
                DEFAULT_MODEL.to_string()
            } else {
                cfg.model.trim().to_string()
            },
            retries: cfg.retries.min(5),
            max_questions: cfg
                .max_questions_per_request
                .clamp(1, super::questions::MAX_CHOICE_OPTIONS),
            max_parallel: cfg.max_parallel.clamp(1, 16),
            transport,
        }
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    /// The credential in use (never printed). `LOCAL_KEY_PLACEHOLDER` means no
    /// key is involved because the endpoint is on this machine.
    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Ask every question in one request when they fit the batch size.
    pub fn ask(&self, state: &Value, questions: &[(String, Question)]) -> Result<Batch, String> {
        self.ask_batched(state, questions, self.max_questions)
    }

    /// Split `questions` into chunks of at most `per_request`, run the chunks
    /// concurrently on threads, merge the answers.
    pub fn ask_batched(
        &self,
        state: &Value,
        questions: &[(String, Question)],
        per_request: usize,
    ) -> Result<Batch, String> {
        if questions.is_empty() {
            return Ok(Batch::default());
        }
        for (id, q) in questions {
            q.validate()
                .map_err(|e| format!("question {id:?} is malformed: {e}"))?;
        }
        let per = per_request.clamp(1, super::questions::MAX_CHOICE_OPTIONS);
        let chunks = plan_batches(questions.len(), per);
        let mut out = Batch {
            model: self.model.clone(),
            requests: chunks.len(),
            parallel: chunks.len() > 1,
            ..Batch::default()
        };

        if chunks.len() == 1 {
            let one = self.ask_chunk(state, questions, &chunks[0])?;
            merge_chunk(&mut out, one);
            return Ok(out);
        }

        // Real concurrency: batches are independent HTTP calls. `scope` keeps
        // the borrow of `questions`/`state` without cloning the payload, and
        // groups of `max_parallel` bound how many are in flight at once.
        let mut results: Vec<Result<Batch, String>> = Vec::with_capacity(chunks.len());
        for group in chunks.chunks(self.max_parallel) {
            let part = std::thread::scope(|scope| {
                let handles: Vec<_> = group
                    .iter()
                    .map(|chunk| scope.spawn(move || self.ask_chunk(state, questions, chunk)))
                    .collect();
                handles
                    .into_iter()
                    .map(|h| {
                        h.join()
                            .unwrap_or_else(|_| Err("typesafe batch thread panicked".to_string()))
                    })
                    .collect::<Vec<_>>()
            });
            results.extend(part);
        }

        let mut failed_chunks = 0usize;
        for r in results {
            match r {
                Ok(b) => merge_chunk(&mut out, b),
                Err(e) => {
                    failed_chunks += 1;
                    out.errors.push(e);
                    telemetry::record_failure();
                }
            }
        }
        // Counted on chunk failures rather than on `errors` being empty: an
        // answer-level error merged from a chunk is not a lost chunk, and it
        // must not suppress the parallel-capability signal.
        if failed_chunks == 0 {
            telemetry::record_parallel(chunks.len().saturating_sub(1) as u64);
        }
        Ok(out)
    }

    fn ask_chunk(
        &self,
        state: &Value,
        all: &[(String, Question)],
        idx: &[usize],
    ) -> Result<Batch, String> {
        let mut map = Map::new();
        for i in idx {
            let (id, q) = &all[*i];
            map.insert(id.clone(), q.to_json());
        }
        let body = json!({
            "state": state,
            "model": self.model,
            "questions": Value::Object(map),
        });
        let response = self.post_with_retry(&body)?;
        let mut batch = parse_response(&response)?;
        batch.requests = 1;
        telemetry::record_request(
            &self.base_url,
            &batch.model,
            batch.input_tokens,
            batch.output_tokens,
            idx.len() as u64,
        );
        Ok(batch)
    }

    fn post_with_retry(&self, body: &Value) -> Result<Value, String> {
        let mut attempt = 0u32;
        loop {
            match self.post_once(body) {
                Ok(v) => return Ok(v),
                Err(PostError::Permanent(msg)) => return Err(msg),
                Err(PostError::Retryable(msg)) => {
                    if attempt >= self.retries {
                        return Err(format!("{msg} (retries exhausted)"));
                    }
                    // Exponential backoff for 429/529/5xx - the documented
                    // handling path for rate limits and overload.
                    let wait = 200u64.saturating_mul(3u64.pow(attempt));
                    std::thread::sleep(Duration::from_millis(wait.min(5_000)));
                    attempt += 1;
                }
            }
        }
    }

    fn post_once(&self, body: &Value) -> Result<Value, PostError> {
        match &self.transport {
            #[cfg(test)]
            Transport::Fake(f) => f(body).map_err(PostError::Retryable),
            Transport::Http(http) => {
                // The whole send/read happens off-runtime: a blocking client
                // used from an async worker hits the same runtime-drop panic.
                let (status, text) = off_runtime(|| {
                    let resp = http
                        .post(&self.base_url)
                        .bearer_auth(&self.key)
                        .header("content-type", "application/json")
                        .json(body)
                        .send()
                        .map_err(|e| format!("request failed: {e}"))?;
                    let status = resp.status();
                    let text = resp.text().map_err(|e| format!("body read failed: {e}"))?;
                    Ok::<_, String>((status, text))
                })
                .map_err(PostError::Retryable)?;
                if status.is_success() {
                    serde_json::from_str::<Value>(&text)
                        .map_err(|e| PostError::Permanent(format!("invalid JSON response: {e}")))
                } else if status.as_u16() == 429
                    || status.as_u16() == 529
                    || status.is_server_error()
                {
                    Err(PostError::Retryable(format!(
                        "typesafe {status}: {}",
                        short(&text)
                    )))
                } else {
                    // 401 / 422 are the caller's problem, not a network blip.
                    Err(PostError::Permanent(format!(
                        "typesafe {status}: {}",
                        short(&text)
                    )))
                }
            }
        }
    }
}

enum PostError {
    Retryable(String),
    Permanent(String),
}

/// Split `n` questions into index chunks of at most `per`.
pub fn plan_batches(n: usize, per: usize) -> Vec<Vec<usize>> {
    let per = per.max(1);
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let end = (i + per).min(n);
        out.push((i..end).collect());
        i = end;
    }
    out
}

fn merge_chunk(into: &mut Batch, from: Batch) {
    if !from.model.is_empty() {
        into.model = from.model;
    }
    into.input_tokens += from.input_tokens;
    into.output_tokens += from.output_tokens;
    for (k, v) in from.answers {
        into.answers.insert(k, v);
    }
    // Per-answer problems (a malformed answer, an endpoint that refused one
    // question) live in `errors`; dropping them here meant the *common*
    // single-chunk path reported a missing answer with no explanation.
    into.errors.extend(from.errors);
}

fn parse_response(v: &Value) -> Result<Batch, String> {
    let model = v
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let answers_obj = v
        .get("answers")
        .and_then(Value::as_object)
        .ok_or_else(|| "response has no answers map".to_string())?;
    let mut answers = BTreeMap::new();
    let mut errors = Vec::new();
    for (id, raw) in answers_obj {
        match Answer::from_json(raw) {
            Ok(a) => {
                answers.insert(id.clone(), a);
            }
            Err(e) => errors.push(format!("answer {id:?}: {e}")),
        }
    }
    // The local bridge reports why a question was refused (token budget, a
    // backend that answered nothing, a contract violation) in a sibling `errors`
    // array. Without this the only thing a user saw was "no answer for this
    // question" - indistinguishable from a dead endpoint.
    match v.get("errors") {
        Some(Value::Array(items)) => {
            for e in items {
                if let Some(s) = e.as_str() {
                    errors.push(s.to_string());
                }
            }
        }
        Some(Value::Object(map)) => {
            for (id, e) in map {
                errors.push(format!("{id}: {}", short(&e.to_string())));
            }
        }
        _ => {}
    }
    let input_tokens = v
        .pointer("/usage/input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output_tokens = v
        .pointer("/usage/output_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Ok(Batch {
        model,
        answers,
        input_tokens,
        output_tokens,
        requests: 1,
        parallel: false,
        errors,
    })
}

fn short(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() <= 300 {
        return s.to_string();
    }
    s.chars().take(300).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typesafe::questions::Question;

    fn cfg() -> TypesafeConfig {
        TypesafeConfig {
            enabled: true,
            api_key: "ts-test-key".into(),
            max_parallel: 4,
            retries: 1,
            ..TypesafeConfig::default()
        }
    }

    fn client_with(
        seen: Arc<std::sync::Mutex<Vec<Value>>>,
        fail_ids: Vec<String>,
    ) -> TypesafeClient {
        let t: Arc<TransportFn> = Arc::new(move |body: &Value| {
            let qs = body.get("questions").and_then(Value::as_object).unwrap();
            for id in qs.keys() {
                if fail_ids.contains(id) {
                    return Err("boom".into());
                }
            }
            let mut answers = Map::new();
            for (id, q) in qs {
                let a = match q.get("type").and_then(Value::as_str) {
                    Some("noul") => json!({"type":"noul","noul":0.9}),
                    Some("choice") => {
                        let first = q
                            .get("criteria")
                            .and_then(Value::as_object)
                            .and_then(|m| m.keys().next().cloned())
                            .unwrap_or_else(|| "a".into());
                        let mut probs = Map::new();
                        probs.insert(first.clone(), json!(1.0));
                        json!({"type":"choice","choice":first,
                               "probabilities":Value::Object(probs),"confidence":0.9})
                    }
                    _ => {
                        json!({"type":"score","score":1.0,"legend":{"0":"lo","1":"hi"},"probabilities":{"1":1.0},"confidence":0.8})
                    }
                };
                answers.insert(id.clone(), a);
            }
            seen.lock().unwrap().push(body.clone());
            Ok(
                json!({"model":"jev-latest","answers":Value::Object(answers),
                      "usage":{"input_tokens":10,"output_tokens":2}}),
            )
        });
        TypesafeClient::with_transport(&cfg(), Transport::Fake(t))
    }

    #[test]
    fn single_request_batches_every_question() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let client = client_with(seen.clone(), vec![]);
        let qs = vec![
            ("a".to_string(), Question::noul("is it?")),
            (
                "b".to_string(),
                Question::choice_of("pick", &["x".into(), "y".into()]),
            ),
            (
                "c".to_string(),
                Question::score("rate", vec!["lo".into(), "hi".into()]),
            ),
        ];
        let batch = client.ask(&json!("state"), &qs).unwrap();
        assert_eq!(batch.requests, 1);
        assert!(!batch.parallel);
        assert_eq!(batch.answers.len(), 3);
        assert_eq!(batch.input_tokens, 10);
        assert_eq!(seen.lock().unwrap().len(), 1);
        // The state and model ride along untouched.
        let body = &seen.lock().unwrap()[0];
        assert_eq!(body["state"], "state");
        assert_eq!(body["model"], "jev-latest");
    }

    #[test]
    fn large_question_sets_split_and_merge() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let client = client_with(seen.clone(), vec![]);
        let qs: Vec<(String, Question)> = (0..50)
            .map(|i| (format!("q{i}"), Question::noul(format!("question {i}?"))))
            .collect();
        let batch = client.ask_batched(&json!("s"), &qs, 20).unwrap();
        assert_eq!(batch.requests, 3);
        assert!(batch.parallel);
        assert_eq!(batch.answers.len(), 50);
        assert_eq!(seen.lock().unwrap().len(), 3);
        // Every question id lands in exactly one request.
        let mut ids: Vec<String> = seen
            .lock()
            .unwrap()
            .iter()
            .flat_map(|b| {
                b["questions"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 50);
    }

    #[test]
    fn a_failed_batch_keeps_the_others_and_reports() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let client = client_with(seen.clone(), vec!["q1".into()]);
        let qs: Vec<(String, Question)> = (0..4)
            .map(|i| (format!("q{i}"), Question::noul(format!("q{i}?"))))
            .collect();
        let batch = client.ask_batched(&json!("s"), &qs, 1).unwrap();
        // q1's batch failed after its retry; the other three answered.
        assert_eq!(batch.answers.len(), 3);
        assert!(!batch.answers.contains_key("q1"));
        assert_eq!(batch.errors.len(), 1);
        assert_eq!(batch.missing(["q0", "q1"]), vec!["q1".to_string()]);
    }

    #[test]
    fn malformed_questions_are_rejected_before_any_request() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let client = client_with(seen.clone(), vec![]);
        let qs = vec![("bad".to_string(), Question::choice("pick", vec![]))];
        assert!(client.ask(&json!("s"), &qs).is_err());
        assert!(seen.lock().unwrap().is_empty());
    }

    /// A loopback HTTP server for tests.
    ///
    /// Blocking per connection; nonblocking only on `accept`, so the thread can
    /// still notice its stop flag between requests. A fully nonblocking loop
    /// proved flaky in the full suite: `read` could return before the request
    /// bytes arrived (so the response was written to a client that had not asked
    /// yet) and the accept deadline could expire on a loaded machine, which
    /// surfaced as a bogus "retries exhausted" send error.
    struct Loopback {
        addr: std::net::SocketAddr,
        requests: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
        handle: Option<std::thread::JoinHandle<()>>,
    }

    impl Loopback {
        fn start(status: &'static str, body: &'static str) -> Self {
            use std::io::{Read, Write};
            use std::net::TcpListener;
            use std::sync::atomic::{AtomicBool, Ordering};
            use std::sync::{Arc, Mutex};

            let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
            let addr = listener.local_addr().unwrap();
            listener.set_nonblocking(true).expect("nonblocking");
            let requests: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
            let stop = Arc::new(AtomicBool::new(false));
            let (reqs, stop2) = (requests.clone(), stop.clone());
            let handle = std::thread::spawn(move || {
                let backstop = std::time::Instant::now() + std::time::Duration::from_secs(120);
                while !stop2.load(Ordering::Relaxed) && std::time::Instant::now() < backstop {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            // An accepted socket can inherit the listener's
                            // nonblocking mode: force blocking so the request is
                            // read in full and the response written completely.
                            let _ = stream.set_nonblocking(false);
                            let _ = stream
                                .set_read_timeout(Some(std::time::Duration::from_secs(10)));
                            let mut buf = [0u8; 16 * 1024];
                            let mut raw: Vec<u8> = Vec::new();
                            let mut done = false;
                            while !done {
                                match stream.read(&mut buf) {
                                    Ok(0) => done = true,
                                    Ok(n) => {
                                        raw.extend_from_slice(&buf[..n]);
                                        done = raw.windows(4).any(|w| w == b"\r\n\r\n");
                                    }
                                    Err(_) => done = true,
                                }
                            }
                            if let Ok(mut g) = reqs.lock() {
                                g.push(String::from_utf8_lossy(&raw).to_string());
                            }
                            let _ = stream.write_all(&http_response(status, body));
                            let _ = stream.flush();
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(std::time::Duration::from_millis(5));
                        }
                        Err(_) => break,
                    }
                }
            });
            Loopback {
                addr,
                requests,
                stop,
                handle: Some(handle),
            }
        }

        fn url(&self) -> String {
            format!("http://{}/v1/systemone", self.addr)
        }

        fn seen(&self) -> usize {
            self.requests.lock().map(|g| g.len()).unwrap_or(0)
        }

        /// Wait up to `ms` for at least `n` requests; returns how many arrived.
        /// Used to prove a retry did *not* happen (no wait = no chance to see it).
        fn wait_for(&self, n: usize, ms: u64) -> usize {
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(ms);
            loop {
                let seen = self.seen();
                if seen >= n || std::time::Instant::now() >= deadline {
                    return seen;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }

        /// Stop serving and return every request received.
        fn finish(mut self) -> Vec<String> {
            let out = self.requests.lock().map(|g| g.clone()).unwrap_or_default();
            self.shutdown();
            out
        }

        fn shutdown(&mut self) {
            self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            if let Some(h) = self.handle.take() {
                let _ = h.join();
            }
        }
    }

    impl Drop for Loopback {
        fn drop(&mut self) {
            self.shutdown();
        }
    }

    /// Minimal HTTP/1.1 response bytes, built without escape sequences so the
    /// CRLF framing is unambiguous.
    fn http_response(status: &str, body: &str) -> Vec<u8> {
        let mut out = Vec::new();
        let headers = [
            status.to_string(),
            "content-type: application/json".to_string(),
            format!("content-length: {}", body.len()),
            "connection: close".to_string(),
            String::new(),
            body.to_string(),
        ];
        for line in headers {
            out.extend_from_slice(line.as_bytes());
            out.extend_from_slice(&[13, 10]); // CRLF
        }
        out
    }

    /// The real transport, against a real socket: proves the reqwest path
    /// (bearer header, request shape, response parsing) without a key or the
    /// public network.
    #[test]
    fn a_real_http_round_trip_is_parsed() {
        let srv = Loopback::start(
            "HTTP/1.1 200 OK",
            concat!(
                "{\"model\":\"jev-latest\",\"answers\":{\"q\":{\"type\":\"noul\",\"noul\":0.93}},",
                "\"usage\":{\"input_tokens\":11,\"output_tokens\":3}}"
            ),
        );

        let cfg = TypesafeConfig {
            enabled: true,
            api_key: "round-trip-key".into(),
            base_url: srv.url(),
            timeout_ms: 5_000,
            retries: 0,
            max_parallel: 1,
            ..TypesafeConfig::default()
        };
        let client = match client(&cfg) {
            Availability::Ready(c) => *c,
            Availability::Unavailable(reason) => panic!("unavailable: {reason}"),
        };
        let batch = client
            .ask(&json!("is it?"), &[("q".into(), Question::noul("is it?"))])
            .expect("the round trip succeeds");
        assert_eq!(batch.answers.get("q").and_then(Answer::noul), Some(0.93));
        assert_eq!(batch.input_tokens, 11);
        assert_eq!(batch.output_tokens, 3);
        assert_eq!(batch.model, "jev-latest");

        let request = srv
            .finish()
            .first()
            .cloned()
            .unwrap_or_default()
            .to_ascii_lowercase();
        assert!(request.starts_with("post /v1/systemone"), "{request}");
        assert!(
            request.contains("authorization: bearer round-trip-key"),
            "{request}"
        );
        assert!(request.contains("\"model\":\"jev-latest\""), "{request}");
        assert!(request.contains("\"instructions\":\"is it?\""), "{request}");
    }

    /// Regression: with a key present, `nur run` panicked inside
    /// `reqwest::blocking::ClientBuilder::build` because the prompt-build path
    /// (skill requirement selection) calls Jev from a tokio worker thread:
    ///
    /// ```text
    /// Cannot drop a runtime in a context where blocking is not allowed.
    /// ```
    ///
    /// Both the build and a real request must now work from inside a runtime.
    #[test]
    fn blocking_http_works_inside_an_async_runtime() {
        let srv = Loopback::start(
            "HTTP/1.1 200 OK",
            r#"{"model":"jev-test","answers":{"q":{"type":"noul","noul":0.9}},"usage":{"input_tokens":3,"output_tokens":1}}"#,
        );

        let cfg = TypesafeConfig {
            enabled: true,
            api_key: "async-context-key".into(),
            base_url: srv.url(),
            timeout_ms: 5_000,
            retries: 0,
            max_parallel: 1,
            ..TypesafeConfig::default()
        };

        // The async context: this is what a tokio worker thread looks like.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime");
        let batch = runtime.block_on(async {
            // 1. Building the client must not panic here.
            let client = match client(&cfg) {
                Availability::Ready(c) => *c,
                Availability::Unavailable(reason) => panic!("unavailable: {reason}"),
            };
            // 2. Nor may issuing a request. Both paths go off-runtime now.
            let direct = client
                .ask(&json!("s"), &[("q".into(), Question::noul("is it?"))])
                .expect("request from inside the runtime");
            // 3. And the shared/cached client used by the loop.
            assert!(
                crate::typesafe::harness::available(&cfg),
                "the shared client resolves inside a runtime"
            );
            direct
        });
        assert_eq!(batch.answers.get("q").and_then(Answer::noul), Some(0.9));
        let _ = srv.finish();
    }

    /// The same path the panic came from: skill requirement selection runs during
    /// prompt build, on the async thread.
    #[test]
    fn skill_requirement_selection_survives_an_async_context() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime");
        let selected = runtime.block_on(async {
            // No key in this environment: the call must degrade to "no
            // narrowing" rather than panic or block.
            let cfg = TypesafeConfig {
                enabled: true,
                api_key: String::new(),
                ..TypesafeConfig::default()
            };
            crate::typesafe::harness::select_requirements(
                &cfg,
                "fix the failing test",
                "tdd",
                &[
                    "You MUST write the failing test first".to_string(),
                    "NEVER refactor before the test passes".to_string(),
                ],
                8,
            )
        });
        // Without a key there is no narrowing; with one it may select rules. The
        // point is that neither panics nor hangs inside a runtime.
        let _ = selected;
        assert!(crate::typesafe::harness::available(
            &crate::config::TypesafeConfig {
                enabled: true,
                api_key: "k".into(),
                ..crate::config::TypesafeConfig::default()
            }
        ));
    }

    /// A 401 is permanent: no retry storm, and an actionable message.
    #[test]
    fn an_auth_failure_is_permanent_and_does_not_retry() {
        // Serves any number of connections so a retry would be *visible*: the old
        // single-accept server could never prove the absence of one.
        let srv = Loopback::start(
            "HTTP/1.1 401 Unauthorized",
            concat!(
                "{\"detail\":{\"error_type\":\"authentication_error\",",
                "\"message\":\"check your API key\"}}"
            ),
        );

        let cfg = TypesafeConfig {
            enabled: true,
            api_key: "bad-key".into(),
            base_url: srv.url(),
            timeout_ms: 5_000,
            retries: 3,
            ..TypesafeConfig::default()
        };
        let client = match client(&cfg) {
            Availability::Ready(c) => *c,
            Availability::Unavailable(reason) => panic!("unavailable: {reason}"),
        };
        let err = client
            .ask(&json!("s"), &[("q".into(), Question::noul("is it?"))])
            .unwrap_err();
        assert!(err.contains("401"), "{err}");
        // A permanent error returns immediately, so the retry budget is untouched.
        assert!(!err.contains("retries exhausted"), "{err}");
        let seen = srv.wait_for(2, 750);
        assert_eq!(seen, 1, "no retry on a 401");
        let _ = srv.finish();
    }

    #[test]
    fn plan_batches_covers_every_index_once() {
        assert_eq!(plan_batches(0, 5), Vec::<Vec<usize>>::new());
        assert_eq!(plan_batches(3, 5), vec![vec![0, 1, 2]]);
        assert_eq!(plan_batches(5, 2), vec![vec![0, 1], vec![2, 3], vec![4]]);
        // Every index appears exactly once, in order.
        let covered: Vec<usize> = plan_batches(9, 4).into_iter().flatten().collect();
        assert_eq!(covered, (0..9).collect::<Vec<_>>());
        // A zero/oversized `per` cannot produce empty or unbounded chunks.
        assert_eq!(plan_batches(2, 0), vec![vec![0], vec![1]]);
    }

    #[test]
    fn local_bridge_urls_normalize_to_exactly_one_endpoint_path() {
        // A trailing slash used to defeat the endpoint-path test, producing
        // `…/v1/systemone/v1/systemone`: a permanent 404 that no key check
        // reveals, because loopback endpoints are keyless by design.
        for raw in [
            "http://127.0.0.1:8788",
            "http://127.0.0.1:8788/",
            "  http://127.0.0.1:8788  ",
        ] {
            assert_eq!(
                normalize_local_bridge_url(raw).as_deref(),
                Some("http://127.0.0.1:8788/v1/systemone"),
                "{raw}"
            );
        }
        for raw in [
            "http://127.0.0.1:8788/v1/systemone",
            "http://127.0.0.1:8788/v1/systemone/",
            "http://127.0.0.1:8788/v1/systemone//",
        ] {
            assert_eq!(
                normalize_local_bridge_url(raw).as_deref(),
                Some("http://127.0.0.1:8788/v1/systemone"),
                "{raw}"
            );
        }
        assert_eq!(normalize_local_bridge_url("   "), None);
        assert_eq!(normalize_local_bridge_url("/"), None);
    }

    #[test]
    fn loopback_endpoints_are_recognised_and_remote_ones_are_not() {
        for local in [
            "http://127.0.0.1:8788/v1/systemone",
            "http://localhost:8788/v1/systemone",
            "http://localhost/v1/systemone",
            "https://[::1]:8443/v1/systemone",
            "http://0.0.0.0:8788",
            "http://user:pass@127.0.0.1:8788/v1/systemone",
        ] {
            assert!(is_loopback_endpoint(local), "{local} is local");
        }
        for remote in [
            "https://api.typesafe.ai/v1/systemone",
            "https://api.typesafe.ai.127.0.0.1.evil.example/v1/systemone",
            "http://10.0.0.5:8788/v1/systemone",
            "https://example.com",
            // A host that merely contains "localhost" is not loopback.
            "http://localhost.evil.example/v1/systemone",
            "http://notlocalhost/v1/systemone",
        ] {
            assert!(!is_loopback_endpoint(remote), "{remote} is remote");
        }
    }

    #[test]
    fn the_bridge_url_accepts_a_bare_host_or_a_full_endpoint() {
        assert_eq!(
            normalize_local_bridge_url("http://127.0.0.1:8788").as_deref(),
            Some("http://127.0.0.1:8788/v1/systemone")
        );
        assert_eq!(
            normalize_local_bridge_url("http://127.0.0.1:8788/").as_deref(),
            Some("http://127.0.0.1:8788/v1/systemone")
        );
        assert_eq!(
            normalize_local_bridge_url("http://127.0.0.1:8788/v1/systemone").as_deref(),
            Some("http://127.0.0.1:8788/v1/systemone")
        );
        assert_eq!(normalize_local_bridge_url("   "), None);
    }

    /// A local engine is trusted by presence: no credential, and the client says
    /// so. A hosted endpoint with no key still refuses.
    #[test]
    fn a_loopback_endpoint_needs_no_key_but_a_hosted_one_does() {
        // Skip when this machine has an ambient credential, which would answer
        // for both cases and tell us nothing.
        let probe = TypesafeConfig {
            enabled: true,
            api_key: String::new(),
            ..TypesafeConfig::default()
        };
        if api_key(&probe).is_some() {
            eprintln!("ambient TypeSafe key present - skipping the no-key assertions");
            return;
        }
        let local = TypesafeConfig {
            enabled: true,
            api_key: String::new(),
            base_url: "http://127.0.0.1:8788/v1/systemone".into(),
            ..TypesafeConfig::default()
        };
        let availability = client(&local);
        assert!(availability.is_ready(), "{:?}", availability.reason());
        assert_eq!(
            availability.client().map(|c| c.key()),
            Some(LOCAL_KEY_PLACEHOLDER),
            "a local engine carries the placeholder, not a credential"
        );
        assert!(
            crate::typesafe::harness::available(&local),
            "the shared client resolves for a local engine too"
        );
        // And the placeholder must never be sent to a hosted endpoint: the same
        // config without loopback is unavailable.
        let hosted = TypesafeConfig {
            base_url: String::new(), // default = https://api.typesafe.ai/v1/systemone
            ..local.clone()
        };
        let hosted_availability = client(&hosted);
        assert!(
            !hosted_availability.is_ready(),
            "hosted endpoints still need a real key"
        );
    }

    #[test]
    fn an_explicit_config_key_always_wins_and_is_never_cached() {
        let mut cfg = TypesafeConfig {
            enabled: true,
            api_key: "first-key".into(),
            ..TypesafeConfig::default()
        };
        assert_eq!(api_key(&cfg).as_deref(), Some("first-key"));
        // Edit the config in place: the next read must see the new value rather
        // than a cached probe.
        cfg.api_key = "second-key".into();
        assert_eq!(api_key(&cfg).as_deref(), Some("second-key"));
        // Whitespace-only counts as unset, so the probe path is taken instead.
        cfg.api_key = "   ".into();
        assert_ne!(
            api_key(&cfg).as_deref(),
            Some("second-key"),
            "a blank config key must not shadow the resolved credential"
        );
    }

    #[test]
    fn invalidating_the_probe_re_resolves_immediately() {
        let cfg = TypesafeConfig {
            enabled: true,
            api_key: String::new(),
            ..TypesafeConfig::default()
        };
        let first = api_key(&cfg);
        assert_eq!(api_key(&cfg), first, "the probe answer is cached");
        // A credential that appears after the probe was cached (a key saved via
        // /auth, an env var exported by a wrapper) is picked up as soon as the
        // cache is dropped - which is what the save paths do.
        std::env::set_var("TYPESAFE_KEY", "probe-test-key");
        assert_eq!(api_key(&cfg), first, "still cached until invalidated");
        invalidate_key_probe();
        assert_eq!(api_key(&cfg).as_deref(), Some("probe-test-key"));
        // Leave the environment and the cache clean for other tests.
        std::env::remove_var("TYPESAFE_KEY");
        invalidate_key_probe();
    }

    #[test]
    fn disabled_config_is_unavailable_not_an_error() {
        let cfg = TypesafeConfig {
            enabled: false,
            api_key: "k".into(),
            ..TypesafeConfig::default()
        };
        let a = client(&cfg);
        assert!(!a.is_ready());
        assert!(a.reason().unwrap().contains("disabled"));
    }

    #[test]
    fn missing_key_reports_actionable_reason() {
        let cfg = TypesafeConfig {
            enabled: true,
            api_key: String::new(),
            ..TypesafeConfig::default()
        };
        // Only meaningful when the machine has no ambient key.
        if api_key_from_env().is_none() && api_key_from_files().is_none() {
            let a = client(&cfg);
            if !a.is_ready() {
                assert!(a.reason().unwrap().contains("TYPESAFE_API_KEY"));
            }
        }
    }
}

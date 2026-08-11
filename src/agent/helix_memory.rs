//! HelixDB acceleration for Nur's native memory stack.
//!
//! The local JSONL hierarchy remains the source of truth. When configured,
//! every native memory write is appended to a durable local outbox and mirrored
//! to HelixDB on a background thread. Explicit routed reads fan out to Helix's
//! tenant-partitioned vector index. This gives larger/cross-process memory sets
//! a proper OLTP graph-vector resident without adding a network probe to launch
//! or prompt construction.

use crate::agent::{embed, memory_vector, native_memory};
use crate::config::{atomic_write, nur_home};
use helix_ast::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::io::Write;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const LABEL: &str = "NurMemory";
const VECTOR_PROPERTY: &str = "embedding";
const TENANT_PROPERTY: &str = "scope";

#[derive(Debug, Clone)]
struct Settings {
    base_url: String,
    api_key: Option<String>,
    timeout: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MirrorRecord {
    scope: String,
    entry: native_memory::MemoryEntry,
    embedding: Vec<f32>,
    embedding_source: String,
}

#[derive(Debug, Clone)]
pub struct HelixHit {
    pub id: String,
    pub text: String,
    pub score: f32,
}

fn queue_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn active_flushes() -> &'static Mutex<HashSet<String>> {
    static ACTIVE: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(HashSet::new()))
}

fn settings() -> Option<Settings> {
    let cfg = crate::config::load_config().ok()?;
    let mode = cfg.helix_memory.mode.trim().to_ascii_lowercase();
    if mode == "off" || mode == "false" || mode == "disabled" {
        return None;
    }
    let env_url = std::env::var("NUR_HELIX_URL")
        .ok()
        .or_else(|| std::env::var("HELIX_URL").ok())
        .filter(|s| !s.trim().is_empty());
    let configured_url =
        (!cfg.helix_memory.url.trim().is_empty()).then(|| cfg.helix_memory.url.trim().to_string());
    if mode == "auto" && env_url.is_none() && configured_url.is_none() {
        return None;
    }
    if mode != "auto" && mode != "on" && mode != "true" && mode != "enabled" {
        return None;
    }
    let base_url = env_url
        .or(configured_url)
        .unwrap_or_else(|| "http://127.0.0.1:6969".into())
        .trim_end_matches('/')
        .to_string();
    let key_env = if cfg.helix_memory.api_key_env.trim().is_empty() {
        "HELIX_API_KEY"
    } else {
        cfg.helix_memory.api_key_env.trim()
    };
    let api_key = std::env::var(key_env)
        .ok()
        .filter(|key| !key.trim().is_empty());
    Some(Settings {
        base_url,
        api_key,
        timeout: Duration::from_millis(cfg.helix_memory.timeout_ms.clamp(250, 30_000)),
    })
}

/// Whether Helix memory is enabled and has an endpoint to query.
pub fn is_configured() -> bool {
    settings().is_some()
}

fn safe_scope(scope: &str) -> String {
    scope
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect()
}

fn outbox_path(scope: &str) -> PathBuf {
    nur_home()
        .join("native-memory")
        .join(safe_scope(scope))
        .join("helix-outbox.jsonl")
}

fn record_for(scope: &str, entry: &native_memory::MemoryEntry) -> MirrorRecord {
    let vector = memory_vector::VectorStore::open(scope)
        .get(&entry.id)
        .unwrap_or_else(|| memory_vector::VectorDoc {
            id: entry.id.clone(),
            vec: embed::embed_local(&entry.text),
            source: "local-helix-fallback".into(),
        });
    MirrorRecord {
        scope: scope.to_string(),
        entry: entry.clone(),
        embedding: vector.vec,
        embedding_source: vector.source,
    }
}

/// Persist a mirror operation locally before attempting any network work.
/// The native memory write is already complete when this is called.
pub fn enqueue(scope: &str, entry: &native_memory::MemoryEntry) {
    if settings().is_none() {
        return;
    }
    let record = record_for(scope, entry);
    let Ok(_guard) = queue_lock().lock() else {
        return;
    };
    let path = outbox_path(scope);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let (Ok(mut file), Ok(line)) = (
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path),
        serde_json::to_string(&record),
    ) {
        let _ = writeln!(file, "{line}");
    }
    drop(_guard);
    spawn_flush(scope.to_string());
}

fn spawn_flush(scope: String) {
    let Ok(mut active) = active_flushes().lock() else {
        return;
    };
    if !active.insert(scope.clone()) {
        return;
    }
    drop(active);
    let _ = std::thread::Builder::new()
        .name("nur-helix-memory".into())
        .spawn(move || {
            let flushed = flush_outbox(&scope).is_ok();
            if let Ok(mut active) = active_flushes().lock() {
                active.remove(&scope);
            }
            // Close the enqueue-vs-exit race: a writer that observed this
            // scope as active may have appended after the flush snapshot.
            if flushed && !read_outbox(&scope).is_empty() {
                spawn_flush(scope);
            }
        });
}

fn read_outbox(scope: &str) -> Vec<MirrorRecord> {
    std::fs::read_to_string(outbox_path(scope))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn rewrite_outbox(scope: &str, records: &[MirrorRecord]) -> Result<(), String> {
    let mut body = String::new();
    for record in records {
        body.push_str(&serde_json::to_string(record).map_err(|e| e.to_string())?);
        body.push('\n');
    }
    atomic_write(&outbox_path(scope), body.as_bytes()).map_err(|e| e.to_string())
}

fn flush_outbox(scope: &str) -> Result<usize, String> {
    let cfg = settings().ok_or_else(|| "Helix memory is not configured".to_string())?;
    ensure_indexes(&cfg)?;
    let pending = {
        let _guard = queue_lock()
            .lock()
            .map_err(|_| "Helix outbox lock poisoned")?;
        read_outbox(scope)
    };
    if pending.is_empty() {
        return Ok(0);
    }
    let mut completed = HashSet::new();
    for record in &pending {
        post_indexed_query(&cfg, upsert_request(record))?;
        completed.insert((record.entry.id.clone(), record.entry.updated_unix));
    }
    let _guard = queue_lock()
        .lock()
        .map_err(|_| "Helix outbox lock poisoned")?;
    let remaining: Vec<_> = read_outbox(scope)
        .into_iter()
        .filter(|r| !completed.contains(&(r.entry.id.clone(), r.entry.updated_unix)))
        .collect();
    rewrite_outbox(scope, &remaining)?;
    Ok(completed.len())
}

fn props(record: &MirrorRecord) -> Vec<(&'static str, PropertyInput)> {
    let entry = &record.entry;
    vec![
        ("memory_id", entry.id.clone().into()),
        ("scope", record.scope.clone().into()),
        ("text", entry.text.clone().into()),
        ("tier", entry.tier.as_str().to_string().into()),
        (
            "voice",
            format!("{:?}", entry.voice).to_ascii_lowercase().into(),
        ),
        (
            "tags",
            PropertyValue::StringArray(entry.tags.clone()).into(),
        ),
        ("confidence", (entry.confidence as f64).into()),
        (
            "created_unix",
            (entry.created_unix.min(i64::MAX as u64) as i64).into(),
        ),
        (
            "updated_unix",
            (entry.updated_unix.min(i64::MAX as u64) as i64).into(),
        ),
        ("source", entry.source.clone().into()),
        ("retired", entry.retired.into()),
        ("embedding_source", record.embedding_source.clone().into()),
        (
            VECTOR_PROPERTY,
            PropertyValue::F32Array(record.embedding.clone()).into(),
        ),
    ]
}

fn same_memory(scope: &str, id: &str) -> Predicate {
    Predicate::and(vec![
        Predicate::eq("$label", LABEL),
        Predicate::eq(TENANT_PROPERTY, scope.to_string()),
        Predicate::eq("memory_id", id.to_string()),
    ])
}

fn upsert_request(record: &MirrorRecord) -> QueryRequest {
    QueryRequest::write(
        write_batch()
            .var_as(
                "old",
                g().n_where(same_memory(&record.scope, &record.entry.id))
                    .drop(),
            )
            .var_as(
                "memory",
                g().add_n(LABEL, props(record))
                    .value_map(Some(vec!["memory_id"])),
            )
            .returning(["memory"]),
    )
}

fn delete_scope_request(scope: &str) -> QueryRequest {
    QueryRequest::write(
        write_batch()
            .var_as(
                "deleted",
                g().n_where(Predicate::and(vec![
                    Predicate::eq("$label", LABEL),
                    Predicate::eq(TENANT_PROPERTY, scope.to_string()),
                ]))
                .drop(),
            )
            .returning(["deleted"]),
    )
}

fn ensure_request() -> QueryRequest {
    QueryRequest::write(
        write_batch()
            .var_as(
                "vector_index",
                g().create_vector_index_nodes(
                    LABEL,
                    VECTOR_PROPERTY,
                    NonZeroUsize::new(embed::EMBED_DIM).expect("embedding dimension is non-zero"),
                    VectorDistanceMetric::Cosine,
                    Some(TENANT_PROPERTY),
                ),
            )
            .var_as(
                "text_index",
                g().create_text_index_nodes(LABEL, "text", Some(TENANT_PROPERTY)),
            )
            .returning(["vector_index", "text_index"]),
    )
}

fn ensured_endpoints() -> &'static Mutex<HashSet<String>> {
    static ENSURED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    ENSURED.get_or_init(|| Mutex::new(HashSet::new()))
}

fn ensure_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn ensure_indexes(cfg: &Settings) -> Result<(), String> {
    if ensured_endpoints()
        .lock()
        .map(|set| set.contains(&cfg.base_url))
        .unwrap_or(false)
    {
        return Ok(());
    }
    let _guard = ensure_lock()
        .lock()
        .map_err(|_| "Helix index lock poisoned".to_string())?;
    // Another thread may have completed setup while this one was waiting.
    if ensured_endpoints()
        .lock()
        .map(|set| set.contains(&cfg.base_url))
        .unwrap_or(false)
    {
        return Ok(());
    }
    post_query(cfg, ensure_request())?;
    if let Ok(mut set) = ensured_endpoints().lock() {
        set.insert(cfg.base_url.clone());
    }
    Ok(())
}

fn forget_ensured(cfg: &Settings) {
    if let Ok(mut set) = ensured_endpoints().lock() {
        set.remove(&cfg.base_url);
    }
}

fn post_indexed_query(cfg: &Settings, request: QueryRequest) -> Result<Value, String> {
    let result = post_query(cfg, request);
    if result.is_err() {
        // The endpoint may have restarted in in-memory mode. Force index setup
        // on the next attempt instead of keeping a stale process-local cache.
        forget_ensured(cfg);
    }
    result
}

fn post_query(cfg: &Settings, request: QueryRequest) -> Result<Value, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(cfg.timeout)
        .build()
        .map_err(|e| format!("Helix client: {e}"))?;
    for attempt in 0..4 {
        let mut builder = client
            .post(format!("{}/v2/query", cfg.base_url))
            .json(&request);
        if let Some(key) = &cfg.api_key {
            builder = builder.bearer_auth(key);
        }
        let response = builder
            .send()
            .map_err(|e| format!("Helix transport: {e}"))?;
        let status = response.status();
        let body = response.text().unwrap_or_default();
        if status.is_success() {
            if body.trim().is_empty() {
                return Ok(Value::Null);
            }
            return serde_json::from_str(&body).map_err(|e| format!("Helix response: {e}"));
        }
        let transaction_conflict =
            status.as_u16() == 409 && body.to_ascii_lowercase().contains("transaction conflict");
        if transaction_conflict && attempt < 3 {
            std::thread::sleep(Duration::from_millis(25 * (1 << attempt)));
            continue;
        }
        return Err(format!(
            "Helix HTTP {}: {}",
            status.as_u16(),
            body.chars().take(300).collect::<String>()
        ));
    }
    unreachable!("bounded Helix retry loop always returns")
}

fn read_request(scope: &str, query: &str, vector: Vec<f32>, k: usize) -> QueryRequest {
    QueryRequest::read(
        read_batch()
            .var_as(
                "vector_hits",
                g().vector_search_nodes(
                    LABEL,
                    VECTOR_PROPERTY,
                    vector,
                    k.max(1),
                    Some(scope.to_string().into()),
                )
                .where_(Predicate::eq("retired", false))
                .project(vec![
                    Projection::property("memory_id", "memory_id"),
                    Projection::property("text", "text"),
                    Projection::property("$distance", "distance"),
                ]),
            )
            .var_as(
                "text_hits",
                g().text_search_nodes(
                    LABEL,
                    "text",
                    query.to_string(),
                    k.max(1),
                    Some(scope.to_string().into()),
                )
                .where_(Predicate::eq("retired", false))
                .project(vec![
                    Projection::property("memory_id", "memory_id"),
                    Projection::property("text", "text"),
                    Projection::property("$score", "score"),
                ]),
            )
            .returning(["vector_hits", "text_hits"]),
    )
}

fn rows<'a>(value: &'a Value, key: &str) -> Vec<&'a Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().collect())
        .unwrap_or_default()
}

/// Query Helix for an explicit, non-empty routed recall. The production memory
/// router shares one embedding across residents via `search_with_embedding`;
/// this convenience entry point remains useful to direct callers and the live
/// integration test.
#[allow(dead_code)]
pub fn search(scope: &str, query: &str, k: usize) -> Result<Vec<HelixHit>, String> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let query_embedding = embed::embed(query);
    search_with_embedding(scope, query, &query_embedding, k)
}

/// Query Helix with an embedding already computed by the memory router.
pub fn search_with_embedding(
    scope: &str,
    query: &str,
    query_embedding: &[f32],
    k: usize,
) -> Result<Vec<HelixHit>, String> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let cfg = settings().ok_or_else(|| "Helix memory is not configured".to_string())?;
    ensure_indexes(&cfg)?;
    let response = post_indexed_query(
        &cfg,
        read_request(scope, query, query_embedding.to_vec(), k.clamp(1, 32)),
    )?;
    let mut hits = Vec::new();
    let mut seen = HashSet::new();
    for row in rows(&response, "vector_hits") {
        let id = row
            .get("memory_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let text = row.get("text").and_then(Value::as_str).unwrap_or_default();
        if id.is_empty() || text.is_empty() || !seen.insert(id.to_string()) {
            continue;
        }
        let distance = row.get("distance").and_then(Value::as_f64).unwrap_or(1.0);
        hits.push(HelixHit {
            id: id.to_string(),
            text: text.to_string(),
            score: (1.0 - distance as f32).clamp(-1.0, 1.0),
        });
    }
    for row in rows(&response, "text_hits") {
        let id = row
            .get("memory_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let text = row.get("text").and_then(Value::as_str).unwrap_or_default();
        if id.is_empty() || text.is_empty() || !seen.insert(id.to_string()) {
            continue;
        }
        hits.push(HelixHit {
            id: id.to_string(),
            text: text.to_string(),
            score: row.get("score").and_then(Value::as_f64).unwrap_or(0.0) as f32,
        });
    }
    hits.truncate(k.max(1));
    Ok(hits)
}

/// Reconcile every live native memory in a scope into Helix. This is explicit
/// because a full rebuild may be expensive for a large history.
pub fn sync(scope: &str) -> Result<String, String> {
    let cfg = settings().ok_or_else(|| "Helix memory is not configured".to_string())?;
    ensure_indexes(&cfg)?;
    let entries = native_memory::load_entries(scope);
    // A sync is an authoritative reconciliation, not an additive replay. Clear
    // this tenant first so memories retired or removed while Helix was offline
    // cannot remain remotely searchable.
    post_indexed_query(&cfg, delete_scope_request(scope))?;
    for entry in &entries {
        post_indexed_query(&cfg, upsert_request(&record_for(scope, entry)))?;
    }
    let flushed = flush_outbox(scope).unwrap_or(0);
    Ok(format!(
        "Helix sync complete: {} live memories reconciled, {} queued updates flushed",
        entries.len(),
        flushed
    ))
}

pub fn status(scope: &str) -> String {
    let queued = read_outbox(scope).len();
    let Some(cfg) = settings() else {
        return format!(
            "Helix memory: inactive (mode auto needs HELIX_URL/NUR_HELIX_URL; or set [helix_memory] mode = \"on\")\nqueued: {queued}"
        );
    };
    let client = match reqwest::blocking::Client::builder()
        .timeout(cfg.timeout.min(Duration::from_secs(3)))
        .build()
    {
        Ok(c) => c,
        Err(e) => return format!("Helix memory: client error ({e})\nqueued: {queued}"),
    };
    let mut request = client.get(format!("{}/healthz", cfg.base_url));
    if let Some(key) = &cfg.api_key {
        request = request.bearer_auth(key);
    }
    match request.send() {
        Ok(resp) if resp.status().is_success() => format!(
            "Helix memory: ready\nendpoint: {}\nvector: {}d cosine, tenant-partitioned by scope\nqueued: {queued}",
            cfg.base_url,
            embed::EMBED_DIM
        ),
        Ok(resp) => format!(
            "Helix memory: endpoint returned HTTP {}\nendpoint: {}\nqueued: {queued}",
            resp.status().as_u16(),
            cfg.base_url
        ),
        Err(e) => format!(
            "Helix memory: unavailable ({e})\nendpoint: {}\nqueued: {queued}",
            cfg.base_url
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inactive_by_default_without_env() {
        if std::env::var("HELIX_URL").is_err() && std::env::var("NUR_HELIX_URL").is_err() {
            let cfg = crate::config::Config::default();
            assert_eq!(cfg.helix_memory.mode, "auto");
        }
    }

    #[test]
    fn safe_scope_is_stable() {
        assert_eq!(safe_scope("repo:session/one"), "repo_session_one");
    }

    #[test]
    fn requests_use_tenant_partitioned_indexes_and_live_filters() {
        let ensure = serde_json::to_value(ensure_request()).unwrap();
        let encoded = ensure.to_string();
        assert!(encoded.contains("node_vector"));
        assert!(encoded.contains("node_text"));
        assert!(encoded.contains("tenant_property"));
        assert!(encoded.contains(TENANT_PROPERTY));

        let read = serde_json::to_value(read_request(
            "scope-a",
            "hello",
            vec![0.0; embed::EMBED_DIM],
            4,
        ))
        .unwrap();
        let encoded = read.to_string();
        assert!(encoded.contains("vector_search_nodes"));
        assert!(encoded.contains("text_search_nodes"));
        assert!(encoded.contains("retired"));
        assert!(encoded.contains("scope-a"));
    }

    #[test]
    fn sync_delete_is_scoped_to_one_tenant() {
        let request = serde_json::to_value(delete_scope_request("repo-one")).unwrap();
        let encoded = request.to_string();
        assert!(encoded.contains("drop"));
        assert!(encoded.contains("NurMemory"));
        assert!(encoded.contains("repo-one"));
    }

    #[test]
    #[ignore = "requires a local HelixDB server and NUR_HELIX_URL"]
    fn e2e_helix_round_trip() {
        let scope = format!("helix-e2e-{}", uuid::Uuid::new_v4().simple());
        let entry = native_memory::remember(
            &scope,
            "Nur uses tenant-partitioned Helix memory for durable recall",
            native_memory::Tier::L1,
            native_memory::Voice::Observed,
            &["helix-e2e".into()],
            0.9,
            "test",
        )
        .unwrap();
        sync(&scope).unwrap();

        let mut found = Vec::new();
        for _ in 0..20 {
            found = search(&scope, "durable Helix recall", 4).unwrap_or_default();
            if found.iter().any(|hit| hit.id == entry.id) {
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        assert!(
            found.iter().any(|hit| hit.id == entry.id),
            "round-trip result missing {}: {found:?}",
            entry.id
        );
        let routed = crate::agent::memory_router::read(&scope, "durable Helix recall", 4, 2);
        assert_eq!(
            routed
                .matches("Nur uses tenant-partitioned Helix memory for durable recall")
                .count(),
            1,
            "local and Helix copies must merge by memory id: {routed}"
        );
        let _ = std::fs::remove_dir_all(nur_home().join("native-memory").join(safe_scope(&scope)));
    }
}

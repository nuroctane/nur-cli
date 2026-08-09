//! RLM-style context store - prompt-as-a-variable for nur.
//!
//! Inspired by *Recursive Language Models* (Zhang, Kraska, Khattab, arXiv:2512.24601)
//! and Prime Agent's RLM runtime: large working context lives **outside** the
//! transformer window as named, addressable variables. The model peeks, slices,
//! and searches programmatically instead of stuffing whole corpora into each turn.
//!
//! Edge cases handled for nur multi-provider use:
//! - Session-scoped (subagents get their own empty store unless parent injects ids)
//! - Sensitive bodies never stored (auth-shaped payloads rejected)
//! - Oversized vars spill to disk under `~/.nur/context-store/` and stay addressable
//! - Compaction of chat history must **not** drop variables (they are not in `input_items`)
//! - Thread-safe for concurrent tool batches

use crate::config::{atomic_write, nur_home};
use crate::tools::sensitive::body_looks_sensitive;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// Soft cap for in-memory body; larger content is spilled to disk.
const INLINE_MAX_CHARS: usize = 200_000;
/// Max vars per session (prevents unbounded growth).
const MAX_VARS_PER_SESSION: usize = 256;
/// Default retained payload cap for a single session. Config is deliberately
/// env-backed for now so old config files retain their exact shape.
const DEFAULT_SESSION_BYTES: u64 = 64 * 1024 * 1024;
/// Default cap across persisted context-store sessions.
const DEFAULT_GLOBAL_BYTES: u64 = 512 * 1024 * 1024;
const DEFAULT_RETENTION_DAYS: u64 = 30;
/// Default peek window (chars).
pub const DEFAULT_PEEK: usize = 2_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextVar {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub char_count: usize,
    pub source: String,
    pub created_unix: u64,
    /// Inline body when small enough.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Spill path when body was too large for memory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Default)]
struct SessionStore {
    vars: HashMap<String, ContextVar>,
    /// Insertion order for stable listing.
    order: Vec<String>,
    loaded: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedSession {
    version: u8,
    vars: HashMap<String, ContextVar>,
    order: Vec<String>,
}

/// Retention settings are exposed for the session/bootstrap owner. Environment
/// overrides are useful in managed installs before config schema grows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    pub session_bytes: u64,
    pub global_bytes: u64,
    pub max_age_days: u64,
}

pub fn retention_policy() -> RetentionPolicy {
    fn env_u64(name: &str, fallback: u64) -> u64 {
        std::env::var(name)
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(fallback)
    }
    RetentionPolicy {
        session_bytes: env_u64("NUR_CONTEXT_STORE_SESSION_BYTES", DEFAULT_SESSION_BYTES),
        global_bytes: env_u64("NUR_CONTEXT_STORE_GLOBAL_BYTES", DEFAULT_GLOBAL_BYTES),
        max_age_days: env_u64("NUR_CONTEXT_STORE_RETENTION_DAYS", DEFAULT_RETENTION_DAYS),
    }
}

fn global() -> &'static Mutex<HashMap<String, SessionStore>> {
    static G: OnceLock<Mutex<HashMap<String, SessionStore>>> = OnceLock::new();
    G.get_or_init(|| Mutex::new(HashMap::new()))
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn store_dir() -> PathBuf {
    nur_home().join("context-store")
}

fn session_dir(session_id: &str) -> PathBuf {
    let mut h = Sha256::new();
    h.update(session_id.as_bytes());
    let digest = h
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    store_dir().join(&digest[..24])
}

fn session_index_path(session_id: &str) -> PathBuf {
    session_dir(session_id).join("index.json")
}

fn session_payload_bytes(sess: &SessionStore) -> u64 {
    sess.vars
        .values()
        .map(|v| {
            v.body.as_ref().map(|b| b.len() as u64).unwrap_or_else(|| {
                v.path
                    .as_ref()
                    .and_then(|p| std::fs::metadata(p).ok())
                    .map(|m| m.len())
                    .unwrap_or(v.char_count as u64)
            })
        })
        .sum()
}

fn persist_session(session_id: &str, sess: &SessionStore) -> Result<(), String> {
    let disk = PersistedSession {
        version: 1,
        vars: sess.vars.clone(),
        order: sess.order.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&disk).map_err(|e| format!("context index: {e}"))?;
    atomic_write(&session_index_path(session_id), &bytes).map_err(|e| format!("context index: {e}"))
}

fn load_session(session_id: &str) -> SessionStore {
    let index = session_index_path(session_id);
    let Ok(text) = std::fs::read_to_string(index) else {
        return SessionStore {
            loaded: true,
            ..Default::default()
        };
    };
    let Ok(disk) = serde_json::from_str::<PersistedSession>(&text) else {
        return SessionStore {
            loaded: true,
            ..Default::default()
        };
    };
    let mut order: Vec<String> = disk
        .order
        .into_iter()
        .filter(|n| disk.vars.contains_key(n))
        .collect();
    for n in disk.vars.keys() {
        if !order.contains(n) {
            order.push(n.clone());
        }
    }
    SessionStore {
        vars: disk.vars,
        order,
        loaded: true,
    }
}

fn ensure_loaded<'a>(
    g: &'a mut HashMap<String, SessionStore>,
    session_id: &str,
) -> &'a mut SessionStore {
    let reload = g.get(session_id).map(|s| !s.loaded).unwrap_or(true);
    if reload {
        g.insert(session_id.to_string(), load_session(session_id));
    }
    g.get_mut(session_id).expect("context session inserted")
}

fn sanitize_name(raw: &str) -> String {
    let s: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    if s.is_empty() {
        format!("var_{}", &uuid::Uuid::new_v4().simple().to_string()[..8])
    } else {
        s
    }
}

fn load_body(var: &ContextVar) -> Result<String, String> {
    if let Some(body) = &var.body {
        return Ok(body.clone());
    }
    if let Some(path) = &var.path {
        let path = PathBuf::from(path);
        let owned =
            crate::tools::spill::is_under_tool_results(&path) || path_is_under(&path, &store_dir());
        if !owned {
            return Err("refused context-store body outside managed storage".into());
        }
        return std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()));
    }
    Err("variable has no body or path".into())
}

fn path_is_under(path: &Path, root: &Path) -> bool {
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    let Ok(path) = path.canonicalize() else {
        return false;
    };
    path.starts_with(root)
}

fn remove_legacy_body(var: &ContextVar) {
    // Content-addressed blobs can belong to a spill and/or another context var;
    // only old private context-store paths are safe to unlink directly.
    if let Some(path) = &var.path {
        let path = PathBuf::from(path);
        if path_is_under(&path, &store_dir()) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Register (or overwrite) a named context variable for `session_id`.
/// Returns the stored variable metadata (body omitted when spilled).
pub fn register(
    session_id: &str,
    name: &str,
    content: &str,
    kind: &str,
    source: &str,
) -> Result<ContextVar, String> {
    if session_id.trim().is_empty() {
        return Err("session_id required".into());
    }
    if content.is_empty() {
        return Err("content is empty".into());
    }
    if body_looks_sensitive(content) {
        return Err(
            "refused: content looks sensitive (secrets/keys) - not stored in context_store".into(),
        );
    }
    let name = sanitize_name(name);
    let char_count = content.chars().count();
    let id = format!(
        "{}-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..12],
        name
    );
    let mut var = ContextVar {
        id: id.clone(),
        name: name.clone(),
        kind: kind.chars().take(32).collect(),
        char_count,
        source: source.chars().take(64).collect(),
        created_unix: now_unix(),
        body: None,
        path: None,
    };
    if char_count <= INLINE_MAX_CHARS {
        var.body = Some(content.to_string());
    } else {
        let path = crate::tools::spill::write_content_addressed_blob(content)
            .map_err(|e| format!("spill failed: {e}"))?;
        var.path = Some(path.display().to_string());
    }

    let mut g = global()
        .lock()
        .map_err(|_| "context_store lock poisoned".to_string())?;
    let sess = ensure_loaded(&mut g, session_id);
    if !sess.vars.contains_key(&name) && sess.vars.len() >= MAX_VARS_PER_SESSION {
        // Evict oldest.
        if let Some(old) = sess.order.first().cloned() {
            sess.order.remove(0);
            if let Some(evicted) = sess.vars.remove(&old) {
                remove_legacy_body(&evicted);
            }
        }
    }
    if !sess.vars.contains_key(&name) {
        sess.order.push(name.clone());
    }
    sess.vars.insert(name.clone(), var.clone());
    let policy = retention_policy();
    while session_payload_bytes(sess) > policy.session_bytes && sess.order.len() > 1 {
        let old = sess.order.remove(0);
        if let Some(evicted) = sess.vars.remove(&old) {
            remove_legacy_body(&evicted);
        }
    }
    if session_payload_bytes(sess) > policy.session_bytes {
        sess.vars.remove(&name);
        sess.order.retain(|n| n != &name);
        let _ = persist_session(session_id, sess);
        return Err(format!(
            "context variable exceeds session storage quota ({} bytes)",
            policy.session_bytes
        ));
    }
    persist_session(session_id, sess)?;
    drop(g);
    let _ = cleanup_retention();
    // Don't return full body in register ack when huge.
    if var.char_count > DEFAULT_PEEK {
        var.body = None;
    }
    Ok(var)
}

/// Auto-register a large tool result. Returns a short pointer line for the model
/// when registration succeeds; `None` if the body is small or rejected.
pub fn maybe_register_tool_result(
    session_id: &str,
    tool: &str,
    body: &str,
    min_chars: usize,
) -> Option<String> {
    if session_id.is_empty() || body.chars().count() < min_chars {
        return None;
    }
    if body_looks_sensitive(body) {
        return None;
    }
    let name = format!(
        "tool_{}_{}",
        sanitize_name(tool),
        &uuid::Uuid::new_v4().simple().to_string()[..6]
    );
    match register(session_id, &name, body, "tool_result", tool) {
        Ok(v) => Some(format!(
            "[rlm context_store] registered `{name}` ({chars} chars, id={id}). \
             Use tool `context` action=peek|slice|search name={name} — full body is \
             preserved across compaction.\n",
            chars = v.char_count,
            id = v.id
        )),
        Err(_) => None,
    }
}

pub fn list(session_id: &str) -> Vec<ContextVar> {
    let Ok(mut g) = global().lock() else {
        return Vec::new();
    };
    let sess = ensure_loaded(&mut g, session_id);
    sess.order
        .iter()
        .filter_map(|n| {
            sess.vars.get(n).map(|v| {
                let mut c = v.clone();
                c.body = None; // never dump bodies in list
                c
            })
        })
        .collect()
}

pub fn get(session_id: &str, name: &str) -> Option<ContextVar> {
    let mut g = global().lock().ok()?;
    let sess = ensure_loaded(&mut g, session_id);
    sess.vars.get(name).cloned()
}

pub fn peek(
    session_id: &str,
    name: &str,
    offset: usize,
    max_chars: usize,
) -> Result<String, String> {
    let var = get(session_id, name).ok_or_else(|| format!("unknown context var `{name}`"))?;
    let body = load_body(&var)?;
    let max = max_chars.clamp(1, 100_000);
    let total = body.chars().count();
    let start = offset.min(total);
    let slice: String = body.chars().skip(start).take(max).collect();
    Ok(format!(
        "var=`{}` chars={total} offset={start} showing={}\n---\n{slice}",
        var.name,
        slice.chars().count()
    ))
}

pub fn slice(session_id: &str, name: &str, start: usize, end: usize) -> Result<String, String> {
    let var = get(session_id, name).ok_or_else(|| format!("unknown context var `{name}`"))?;
    let body = load_body(&var)?;
    let total = body.chars().count();
    let start = start.min(total);
    let end = end.min(total).max(start);
    if end - start > 100_000 {
        return Err("slice too large (max 100000 chars); use a smaller window".into());
    }
    let slice: String = body.chars().skip(start).take(end - start).collect();
    Ok(format!(
        "var=`{}` slice=[{start},{end}) of {total}\n---\n{slice}",
        var.name
    ))
}

pub fn search(
    session_id: &str,
    name: &str,
    pattern: &str,
    max_hits: usize,
) -> Result<String, String> {
    if pattern.is_empty() {
        return Err("pattern required".into());
    }
    let var = get(session_id, name).ok_or_else(|| format!("unknown context var `{name}`"))?;
    let body = load_body(&var)?;
    let max_hits = max_hits.clamp(1, 50);
    let lower_pat = pattern.to_ascii_lowercase();
    let mut hits = Vec::new();
    for (i, line) in body.lines().enumerate() {
        if line.to_ascii_lowercase().contains(&lower_pat) {
            let line_no = i + 1;
            let trimmed: String = line.chars().take(240).collect();
            hits.push(format!("L{line_no}: {trimmed}"));
            if hits.len() >= max_hits {
                break;
            }
        }
    }
    Ok(format!(
        "var=`{}` pattern={pattern:?} hits={}/{max_hits} (line-limited)\n{}",
        var.name,
        hits.len(),
        hits.join("\n")
    ))
}

pub fn delete(session_id: &str, name: &str) -> Result<String, String> {
    let mut g = global()
        .lock()
        .map_err(|_| "context_store lock poisoned".to_string())?;
    let sess = ensure_loaded(&mut g, session_id);
    let name = sanitize_name(name);
    let Some(var) = sess.vars.remove(&name) else {
        return Err(format!("unknown context var `{name}`"));
    };
    sess.order.retain(|n| n != &name);
    remove_legacy_body(&var);
    persist_session(session_id, sess)?;
    Ok(format!("deleted `{name}`"))
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CleanupReport {
    pub removed_sessions: usize,
    pub removed_blobs: usize,
    pub reclaimed_bytes: u64,
}

fn dir_size(path: &Path) -> u64 {
    walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

fn modified_unix(path: &Path) -> u64 {
    path.metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn referenced_blob_paths() -> std::collections::HashSet<PathBuf> {
    let mut paths = std::collections::HashSet::new();
    for e in walkdir::WalkDir::new(store_dir())
        .max_depth(2)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name() == "index.json")
    {
        let Ok(text) = std::fs::read_to_string(e.path()) else {
            continue;
        };
        let Ok(index) = serde_json::from_str::<PersistedSession>(&text) else {
            continue;
        };
        for var in index.vars.values() {
            if let Some(path) = &var.path {
                paths.insert(PathBuf::from(path));
            }
        }
    }
    paths
}

/// Enforce retention after writes and at session bootstrap. It is intentionally
/// conservative: referenced blobs live as long as their session index does;
/// unreferenced spill blobs are retained for the same age window and then
/// reclaimed. Call this during session resume for cold-session cleanup.
pub fn cleanup_retention() -> CleanupReport {
    let policy = retention_policy();
    let now = now_unix();
    let max_age = policy.max_age_days.saturating_mul(86_400);
    let mut report = CleanupReport::default();
    let mut sessions: Vec<PathBuf> = std::fs::read_dir(store_dir())
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();

    for path in &sessions {
        if now.saturating_sub(modified_unix(path)) > max_age {
            let bytes = dir_size(path);
            if std::fs::remove_dir_all(path).is_ok() {
                report.removed_sessions += 1;
                report.reclaimed_bytes += bytes;
            }
        }
    }
    sessions.retain(|p| p.exists());
    sessions.sort_by_key(|p| modified_unix(p));

    // A global hard cap makes abandoned session indexes bounded even when they
    // are frequently older than the age window.
    let mut total = dir_size(&store_dir()) + dir_size(&crate::tools::spill::shared_blob_dir());
    for path in sessions {
        if total <= policy.global_bytes {
            break;
        }
        let bytes = dir_size(&path);
        if std::fs::remove_dir_all(&path).is_ok() {
            total = total.saturating_sub(bytes);
            report.removed_sessions += 1;
            report.reclaimed_bytes += bytes;
        }
    }

    let references = referenced_blob_paths();
    let mut blobs: Vec<PathBuf> = std::fs::read_dir(crate::tools::spill::shared_blob_dir())
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    blobs.sort_by_key(|p| modified_unix(p));
    for blob in blobs {
        let old = now.saturating_sub(modified_unix(&blob)) > max_age;
        let over = total > policy.global_bytes;
        if (old || over) && !references.contains(&blob) {
            let bytes = std::fs::metadata(&blob).map(|m| m.len()).unwrap_or(0);
            if std::fs::remove_file(&blob).is_ok() {
                total = total.saturating_sub(bytes);
                report.removed_blobs += 1;
                report.reclaimed_bytes += bytes;
            }
        }
    }
    report
}

/// Explicit resume hook. Listing/getting already reload lazily; callers that
/// have the session id at startup can invoke this to perform bounded cleanup.
pub fn reload_session(session_id: &str) -> Vec<ContextVar> {
    let _ = cleanup_retention();
    list(session_id)
}

/// Compact summary for system/user injection after chat compaction (Prime: kernel survives).
pub fn prompt_inventory(session_id: &str) -> String {
    let vars = list(session_id);
    if vars.is_empty() {
        return String::new();
    }
    let mut lines = vec![
        "# RLM context store (survives compaction - use tool `context` to peek/slice/search)"
            .to_string(),
    ];
    for v in vars.iter().take(40) {
        lines.push(format!(
            "- `{}` ({}, {} chars, source={})",
            v.name, v.kind, v.char_count, v.source
        ));
    }
    if vars.len() > 40 {
        lines.push(format!("- … and {} more", vars.len() - 40));
    }
    lines.join("\n")
}

/// Drop an entire session store (optional cleanup).
#[allow(dead_code)]
pub fn clear_session(session_id: &str) {
    if let Ok(mut g) = global().lock() {
        if let Some(sess) = g.remove(session_id) {
            for v in sess.vars.values() {
                remove_legacy_body(v);
            }
        }
    }
    let _ = std::fs::remove_dir_all(session_dir(session_id));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sid() -> String {
        format!("test-{}", uuid::Uuid::new_v4().simple())
    }

    #[test]
    fn register_peek_slice_search_delete() {
        let s = sid();
        let body = "alpha\nbeta needle here\ngamma\n";
        let v = register(&s, "doc", body, "test", "unit").unwrap();
        assert_eq!(v.char_count, body.chars().count());
        let peek = peek(&s, "doc", 0, 10).unwrap();
        assert!(peek.contains("alpha"));
        let sl = slice(&s, "doc", 0, 5).unwrap();
        assert!(sl.contains("alpha") || sl.contains("alph"));
        let hits = search(&s, "doc", "needle", 5).unwrap();
        assert!(hits.contains("needle"));
        delete(&s, "doc").unwrap();
        assert!(get(&s, "doc").is_none());
        clear_session(&s);
    }

    #[test]
    fn rejects_sensitive() {
        let s = sid();
        let err = register(
            &s,
            "secrets",
            &format!("api_key=sk-{}", "x".repeat(40)),
            "test",
            "unit",
        )
        .unwrap_err();
        assert!(err.contains("sensitive"));
    }

    #[test]
    fn inventory_lists_without_bodies() {
        let s = sid();
        register(&s, "a", "hello world content", "t", "u").unwrap();
        let inv = prompt_inventory(&s);
        assert!(inv.contains("`a`"));
        assert!(!inv.contains("hello world content"));
        clear_session(&s);
    }

    #[test]
    fn maybe_register_respects_min_chars() {
        let s = sid();
        assert!(maybe_register_tool_result(&s, "bash", "tiny", 100).is_none());
        let big = "y".repeat(150);
        let msg = maybe_register_tool_result(&s, "bash", &big, 100).unwrap();
        assert!(msg.contains("context_store"));
        clear_session(&s);
    }

    #[test]
    fn persisted_index_reloads_after_process_cache_is_dropped() {
        let s = sid();
        register(
            &s,
            "durable",
            "survives an in-process restart",
            "test",
            "unit",
        )
        .unwrap();
        global().lock().unwrap().remove(&s);
        let vars = reload_session(&s);
        assert_eq!(vars.len(), 1);
        assert_eq!(vars[0].name, "durable");
        assert!(peek(&s, "durable", 0, 100).unwrap().contains("survives"));
        clear_session(&s);
    }

    #[test]
    fn large_context_and_tool_spill_share_a_blob() {
        let s = sid();
        let body = format!("shared-{}", "x".repeat(INLINE_MAX_CHARS + 1));
        let context = register(&s, "large", &body, "test", "unit").unwrap();
        let direct = crate::tools::spill::write_content_addressed_blob(&body).unwrap();
        assert_eq!(
            context.path.as_deref(),
            Some(direct.to_string_lossy().as_ref())
        );
        clear_session(&s);
        let _ = std::fs::remove_file(direct);
    }
}

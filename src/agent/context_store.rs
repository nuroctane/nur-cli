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

/// Inline input limit for prompts, shared with the old input guardrail.
pub const PROMPT_INLINE_CHAR_LIMIT: usize = 500_000;

/// Oversized user prompts never stop a turn: the full text is registered as
/// a session context variable (RLM prompt-as-variable, done for the user)
/// and the caller runs with a pointer + preview instead. Returns
/// `Some((var name, replacement prompt))` only when the prompt exceeds the
/// inline limit. When registration refuses the body (sensitive-looking
/// content, quota), the replacement carries a head+tail excerpt so the turn
/// still proceeds - a size cap must never kill a session.
pub fn maybe_spill_oversized_prompt(session_id: &str, text: &str) -> Option<(String, String)> {
    let chars = text.chars().count();
    if chars <= PROMPT_INLINE_CHAR_LIMIT {
        return None;
    }
    const PREVIEW: usize = 4_000;
    let head: String = text.chars().take(PREVIEW).collect();
    let name = "user_paste";
    let registered = register(session_id, name, text, "user_prompt", "auto-spill");
    let pointer = match &registered {
        Ok(v) => format!(
            "The FULL text is preserved in context_store as `{name}` (id={id}, {chars} chars) - \
             use the `context` tool with action peek|slice|search name={name} to read any part.",
            id = v.id,
        ),
        Err(_) => format!(
            "The full text could not be auto-registered ({chars} chars); the middle was \
             elided below. Ask the user to re-share specific parts when needed."
        ),
    };
    let tail: String = if chars > PREVIEW * 2 {
        text.chars().skip(chars - PREVIEW).collect()
    } else {
        String::new()
    };
    let replacement = format!(
        "[Your message was too large to carry inline. {pointer}]\n\n\
         First {PREVIEW} characters:\n\n{head}\n\n\
         {marker}{tail}",
        marker = if tail.is_empty() {
            ""
        } else {
            "\n\n...[middle elided]...\n\n"
        },
    );
    Some((name.to_string(), replacement))
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

/// Reorder search hits by how well each answers `query`, in place.
///
/// Returns whether the order changed. Candidates are the hit lines themselves
/// (code-built), so the answer can only *select* one of them; a hit the answer
/// cannot place confidently keeps its line position, and ties keep line order.
fn rank_hits_by_relevance(query: &str, hits: &mut Vec<String>) -> bool {
    use crate::typesafe::{harness, policy};
    let cfg = crate::config::load_config()
        .map(|c| c.typesafe)
        .unwrap_or_default();
    if hits.len() < 4 || harness::ready(&cfg).is_none() {
        return false;
    }
    let levels: Vec<String> = harness::RELEVANCE_LEVELS
        .iter()
        .map(|s| s.to_string())
        .collect();
    let judged = harness::rank(&cfg, query, hits, &levels);
    let acted = |j: &policy::Judgment<f64>| j.action == policy::GateAction::Act;
    if !judged.iter().any(acted) {
        return false;
    }
    let scores = placement_scores(&judged);
    let before = hits.clone();
    *hits = reorder_by_scores(&before, &scores);
    *hits != before
}

/// The score each hit may be ordered by: only a *placed* (acted) hit contributes
/// one.
///
/// A sub-threshold answer must not reorder anything - the policy thresholds on
/// confidence, and an unplaced hit is documented to keep its line position.
/// Passing the raw score through for unplaced hits contradicted both; it looked
/// harmless because the ordering test fed `(false, None)` and never production's
/// `(false, Some(x))`.
fn placement_scores(
    judged: &[crate::typesafe::policy::Judgment<f64>],
) -> Vec<(bool, Option<f64>)> {
    judged
        .iter()
        .map(|j| {
            let placed = j.action == crate::typesafe::policy::GateAction::Act;
            (placed, placed.then(|| j.raw().copied()).flatten())
        })
        .collect()
}

/// Order hits by (placed confidently, score), keeping input order for everything
/// else. Pure, so the ordering rule is testable without a judgment layer.
fn reorder_by_scores(hits: &[String], scores: &[(bool, Option<f64>)]) -> Vec<String> {
    let mut order: Vec<usize> = (0..hits.len()).collect();
    let score_of = |i: usize| scores.get(i).copied().unwrap_or((false, None));
    order.sort_by(|a, b| {
        let (pa, sa) = score_of(*a);
        let (pb, sb) = score_of(*b);
        pb.cmp(&pa)
            .then(
                sb.unwrap_or(f64::NEG_INFINITY)
                    .partial_cmp(&sa.unwrap_or(f64::NEG_INFINITY))
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(a.cmp(b))
    });
    order.iter().map(|i| hits[*i].clone()).collect()
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
    // Relevance order, when the judgment layer can supply one. Pattern search
    // returns matches in *file* order, which for a long document is close to
    // random with respect to the question being asked; a hit that actually
    // answers the query should come first. Only confident placements move, and
    // the header says the order came from a judgment rather than from the file.
    let reranked = rank_hits_by_relevance(pattern, &mut hits);
    Ok(format!(
        "var=`{}` pattern={pattern:?} hits={}/{max_hits} (line-limited){})\n{}",
        if reranked {
            ", ordered by relevance"
        } else {
            ""
        },
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
    // Snapshot mtimes BEFORE sorting: a live re-read per comparison makes
    // the key move under the sort when parallel tests touch the same
    // store, and Rust's sort panics on comparators that are not a total
    // order.
    let mut keyed: Vec<(u64, PathBuf)> = sessions
        .into_iter()
        .map(|p| {
            let m = modified_unix(&p);
            (m, p)
        })
        .collect();
    keyed.sort_by_key(|(m, _)| *m);
    let sessions: Vec<PathBuf> = keyed.into_iter().map(|(_, p)| p).collect();

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
    let blobs: Vec<PathBuf> = std::fs::read_dir(crate::tools::spill::shared_blob_dir())
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    // Same snapshot rule as above: never sort by a live mtime read.
    let mut keyed_blobs: Vec<(u64, PathBuf)> = blobs
        .into_iter()
        .map(|p| {
            let m = modified_unix(&p);
            (m, p)
        })
        .collect();
    keyed_blobs.sort_by_key(|(m, _)| *m);
    let blobs: Vec<PathBuf> = keyed_blobs.into_iter().map(|(_, p)| p).collect();
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

/// Drop an entire session store (test cleanup).
#[cfg(test)]
fn clear_session(session_id: &str) {
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
mod rerank_tests {
    use super::{placement_scores, reorder_by_scores};

    fn hits(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("L{}: hit {i}", i + 1)).collect()
    }

    #[test]
    fn placed_hits_move_ahead_of_unplaced_ones() {
        let h = hits(4);
        // Hit 3 is confidently best, hit 1 confidently worst, 0 and 2 unplaced.
        let scores = vec![
            (false, None),
            (true, Some(0.1)),
            (false, None),
            (true, Some(2.5)),
        ];
        let out = reorder_by_scores(&h, &scores);
        assert_eq!(out[0], h[3], "the confident winner comes first");
        assert_eq!(out[1], h[1], "the confident loser follows");
        // Unplaced hits keep their relative line order.
        assert_eq!(out[2], h[0]);
        assert_eq!(out[3], h[2]);
    }

    #[test]
    fn ties_and_missing_scores_keep_line_order() {
        let h = hits(3);
        let out = reorder_by_scores(&h, &[(true, Some(1.0)), (true, Some(1.0)), (false, None)]);
        assert_eq!(out, h, "an all-equal ranking must not shuffle the hits");
        let short = reorder_by_scores(&h, &[]);
        assert_eq!(short, h, "no scores at all keeps the input order");
    }

    #[test]
    fn a_sub_threshold_score_does_not_move_a_hit() {
        // The bug this pins: `rank_hits_by_relevance` used to pass a hit's raw
        // score through even when the policy had *not* acted on it, so a
        // low-confidence placement reordered the results - the opposite of both
        // the documented rule ("unplaced keeps its line position") and the module
        // rule ("threshold on confidence, not on the answer").
        //
        // These tuples are what production now builds: `(placed, placed.then(score))`.
        let h = hits(4);
        let scores = vec![
            (false, None),
            (true, Some(0.1)),
            (false, None),
            (true, Some(2.5)),
        ];
        let out = reorder_by_scores(&h, &scores);
        assert_eq!(out[0], h[3]);
        assert_eq!(out[1], h[1]);
        assert_eq!(out[2], h[0], "unplaced hits stay in line order");
        assert_eq!(out[3], h[2]);
    }

    #[test]
    fn only_a_placed_hit_contributes_a_score() {
        // The mapping `rank_hits_by_relevance` feeds the ordering with. The old
        // code passed `j.raw()` through for *every* hit, so a confident-looking
        // score from an answer the policy refused to act on could still reorder
        // the results among the unplaced ones.
        use crate::typesafe::policy::{GateAction, Judgment, Thresholds};
        let t = Thresholds::default();
        let judged = vec![
            Judgment::<f64>::from_answer("r0", Some(9.9), Some(0.2), &t), // unplaced, high score
            Judgment::<f64>::from_answer("r1", Some(2.5), Some(0.95), &t), // placed
            Judgment::<f64>::unavailable("r2", "no key"),                  // nothing at all
        ];
        assert_eq!(judged[0].action, GateAction::Escalate, "test premise");
        let scores = placement_scores(&judged);
        assert_eq!(
            scores[0],
            (false, None),
            "an unacted hit carries no score, however high its raw value"
        );
        assert_eq!(scores[1], (true, Some(2.5)), "an acted hit carries its score");
        assert_eq!(scores[2], (false, None));
        // ... and ordering with those tuples keeps every unplaced hit in place.
        let h = hits(3);
        assert_eq!(reorder_by_scores(&h, &scores)[2], h[2]);
    }

    #[test]
    fn every_hit_survives_a_reorder() {
        let h = hits(5);
        let scores = vec![
            (true, Some(0.2)),
            (false, None),
            (true, Some(0.9)),
            (false, None),
            (true, Some(0.4)),
        ];
        let mut out = reorder_by_scores(&h, &scores);
        out.sort();
        let mut expected = h.clone();
        expected.sort();
        assert_eq!(out, expected, "reordering must not add or drop a hit");
    }
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
    fn oversized_prompt_spills_instead_of_blocking() {
        let s = sid();
        assert!(maybe_spill_oversized_prompt(&s, "just a normal prompt").is_none());

        let huge = format!("alpha {}\ntail marker zebra", "x".repeat(600_000));
        let (name, replacement) =
            maybe_spill_oversized_prompt(&s, &huge).expect("over-limit prompt must spill");
        assert_eq!(name, "user_paste");
        assert!(replacement.contains("user_paste"));
        assert!(replacement.contains("context"));
        assert!(replacement.contains("alpha"));
        // The replacement is far smaller than the original: the turn can
        // actually proceed within the window.
        assert!(replacement.chars().count() < huge.chars().count() / 10);
        // The full body is readable from the store.
        let hits = search(&s, "user_paste", "zebra", 5).unwrap();
        assert!(hits.contains("zebra"));
        delete(&s, "user_paste").unwrap();
        clear_session(&s);
    }

    #[test]
    fn oversized_sensitive_prompt_still_yields_a_replacement() {
        let s = sid();
        let huge = format!("api_key=sk-{}\n{}", "a".repeat(60), "x".repeat(600_000));
        let (name, replacement) = maybe_spill_oversized_prompt(&s, &huge)
            .expect("registration refusal must still return a usable replacement");
        assert_eq!(name, "user_paste");
        assert!(replacement.contains("elided"));
        assert!(get(&s, "user_paste").is_none());
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

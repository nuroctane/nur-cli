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
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// Soft cap for in-memory body; larger content is spilled to disk.
const INLINE_MAX_CHARS: usize = 200_000;
/// Max vars per session (prevents unbounded growth).
const MAX_VARS_PER_SESSION: usize = 256;
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
        return std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path));
    }
    Err("variable has no body or path".into())
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
        let dir = store_dir().join(session_id.chars().take(12).collect::<String>());
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("{id}.txt"));
        atomic_write(&path, content.as_bytes()).map_err(|e| format!("spill failed: {e}"))?;
        var.path = Some(path.display().to_string());
    }

    let mut g = global()
        .lock()
        .map_err(|_| "context_store lock poisoned".to_string())?;
    let sess = g.entry(session_id.to_string()).or_default();
    if !sess.vars.contains_key(&name) && sess.vars.len() >= MAX_VARS_PER_SESSION {
        // Evict oldest.
        if let Some(old) = sess.order.first().cloned() {
            sess.order.remove(0);
            if let Some(evicted) = sess.vars.remove(&old) {
                if let Some(p) = evicted.path {
                    let _ = std::fs::remove_file(p);
                }
            }
        }
    }
    if !sess.vars.contains_key(&name) {
        sess.order.push(name.clone());
    }
    sess.vars.insert(name, var.clone());
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
    let Ok(g) = global().lock() else {
        return Vec::new();
    };
    let Some(sess) = g.get(session_id) else {
        return Vec::new();
    };
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
    let g = global().lock().ok()?;
    let sess = g.get(session_id)?;
    sess.vars.get(name).cloned()
}

pub fn peek(session_id: &str, name: &str, offset: usize, max_chars: usize) -> Result<String, String> {
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
    let sess = g
        .get_mut(session_id)
        .ok_or_else(|| "no context store for session".to_string())?;
    let name = sanitize_name(name);
    let Some(var) = sess.vars.remove(&name) else {
        return Err(format!("unknown context var `{name}`"));
    };
    sess.order.retain(|n| n != &name);
    if let Some(p) = var.path {
        let _ = std::fs::remove_file(p);
    }
    Ok(format!("deleted `{name}`"))
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
                if let Some(p) = &v.path {
                    let _ = std::fs::remove_file(p);
                }
            }
        }
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
}

use crate::config::{
    atomic_write, ensure_dirs, legacy_muse_home, muse_home, sessions_dir,
};
use crate::error::{MuseError, Result};
use crate::usage::TokenUsage;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub model: String,
    pub cwd: String,
    pub messages: Vec<SessionMessage>,
    pub usage: TokenUsage,
    /// Full Responses input item history for multi-turn.
    #[serde(default)]
    pub input_items: Vec<Value>,
    /// TUI transcript cards (thought · tools · answers) for reload. Additive.
    #[serde(default)]
    pub ui_log: Vec<UiLogItem>,
}

/// One persisted transcript card (display only — not API wire format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiLogItem {
    /// `user` | `assistant` | `thinking` | `tool` | `turn_done` | `info` | `error`
    pub kind: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub args: Option<String>,
    #[serde(default)]
    pub ok: Option<bool>,
    #[serde(default)]
    pub ms: Option<u64>,
    #[serde(default)]
    pub thought_ms: Option<u64>,
    #[serde(default)]
    pub interrupted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    pub role: String,
    pub content: String,
    pub ts: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub updated_at: DateTime<Utc>,
    pub model: String,
    pub cwd: String,
    pub messages: usize,
    pub total_tokens: u64,
    pub estimated_cost_usd: f64,
    /// First user prompt, trimmed (for the sessions picker). Empty if none.
    #[serde(default)]
    pub preview: String,
}

/// Lightweight parse — deliberately **omits** `input_items` so huge multimodal
/// session files don't get fully materialised just to list them.
#[derive(Debug, Deserialize)]
struct SessionLite {
    id: String,
    updated_at: DateTime<Utc>,
    #[serde(default)]
    model: String,
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    messages: Vec<SessionMessage>,
    #[serde(default)]
    usage: TokenUsage,
}

impl Session {
    pub fn new(model: &str, cwd: &str) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            created_at: now,
            updated_at: now,
            model: model.to_string(),
            cwd: cwd.to_string(),
            messages: Vec::new(),
            usage: TokenUsage::default(),
            input_items: Vec::new(),
            ui_log: Vec::new(),
        }
    }

    pub fn path(&self) -> PathBuf {
        sessions_dir().join(format!("{}.json", self.id))
    }

    pub fn save(&self) -> Result<()> {
        ensure_dirs()?;
        // Never destroy a previous session body without a sidecar backup.
        // Revert / compact / mid-turn saves all go through here — if something
        // goes wrong, `*.json.bak` is the previous full snapshot.
        let path = self.path();
        if path.is_file() {
            let bak = path.with_extension("json.bak");
            let _ = fs::copy(&path, &bak);
        }
        let text = serde_json::to_string_pretty(self)?;
        atomic_write(&path, text.as_bytes())
            .map_err(|e| MuseError::Other(format!("session atomic save failed: {e}")))?;
        // Pointer for --continue and ADEs
        let latest = muse_home().join("latest_session.json");
        let ptr = serde_json::json!({
            "session_id": self.id,
            "cwd": self.cwd,
            "updated_at": self.updated_at,
            "path": self.path().display().to_string(),
            "model": self.model,
            "usage": self.usage,
            "estimated_cost_usd": self.usage.estimated_cost_usd(),
        });
        atomic_write(
            &latest,
            serde_json::to_string_pretty(&ptr)?.as_bytes(),
        )
        .map_err(|e| MuseError::Other(format!("latest_session atomic write failed: {e}")))?;

        // Per-cwd last session map (for continue in same project)
        let map_path = muse_home().join("cwd_sessions.json");
        let mut map: serde_json::Map<String, Value> = if map_path.exists() {
            serde_json::from_str(&fs::read_to_string(&map_path).unwrap_or_default())
                .unwrap_or_default()
        } else {
            serde_json::Map::new()
        };
        let key = normalize_cwd(&self.cwd);
        map.insert(key, Value::String(self.id.clone()));
        atomic_write(
            &map_path,
            serde_json::to_string_pretty(&Value::Object(map))?.as_bytes(),
        )
        .map_err(|e| MuseError::Other(format!("cwd_sessions atomic write failed: {e}")))?;
        Ok(())
    }

    pub fn load(id: &str) -> Result<Self> {
        ensure_dirs()?;
        let id = id.trim();
        // Allow short prefix match
        if id.len() < 36 {
            if let Some(s) = find_by_prefix(id)? {
                return Ok(s);
            }
        }
        let path = sessions_dir().join(format!("{id}.json"));
        let legacy = crate::config::legacy_muse_home()
            .join("sessions")
            .join(format!("{id}.json"));
        // Prefer the **richer** of ~/.meta vs legacy ~/.muse (more tokens wins).
        // Never silently drop a high-cost chat in favour of a thin twin.
        if path.is_file() && legacy.is_file() {
            let prefer_legacy = match (
                fs::metadata(&path).ok().map(|m| m.len()),
                fs::metadata(&legacy).ok().map(|m| m.len()),
            ) {
                (Some(a), Some(b)) => b > a,
                _ => false,
            };
            if prefer_legacy {
                let _ = fs::create_dir_all(sessions_dir());
                let _ = fs::copy(&legacy, &path);
            }
        } else if !path.exists() && legacy.is_file() {
            let _ = fs::create_dir_all(sessions_dir());
            let _ = fs::copy(&legacy, &path);
        }
        if !path.exists() {
            return Err(MuseError::Other(format!("session not found: {id}")));
        }
        let text = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn continue_for_cwd(cwd: &str) -> Result<Self> {
        let key = normalize_cwd(cwd);
        let map_path = muse_home().join("cwd_sessions.json");
        if map_path.exists() {
            let map: serde_json::Map<String, Value> =
                serde_json::from_str(&fs::read_to_string(&map_path)?)?;
            if let Some(Value::String(id)) = map.get(&key) {
                if let Ok(s) = Self::load(id) {
                    return Ok(s);
                }
            }
        }
        // Fallback: most recently updated session with matching cwd
        let mut best: Option<Session> = None;
        for s in list_sessions()? {
            if normalize_cwd(&s.cwd) == key {
                if best
                    .as_ref()
                    .map(|b| s.updated_at > b.updated_at)
                    .unwrap_or(true)
                {
                    best = Some(s);
                }
            }
        }
        best.ok_or_else(|| {
            MuseError::Other(format!(
                "no previous session for cwd {cwd}; start a new one without -c"
            ))
        })
    }

    pub fn push_user(&mut self, content: &str) {
        self.messages.push(SessionMessage {
            role: "user".into(),
            content: content.into(),
            ts: Utc::now(),
        });
        self.updated_at = Utc::now();
    }

    pub fn push_assistant(&mut self, content: &str) {
        self.messages.push(SessionMessage {
            role: "assistant".into(),
            content: content.into(),
            ts: Utc::now(),
        });
        self.updated_at = Utc::now();
    }

    #[allow(dead_code)]
    pub fn summary(&self) -> SessionSummary {
        let preview = self
            .messages
            .iter()
            .find(|m| m.role == "user")
            .map(|m| {
                m.content
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .chars()
                    .take(120)
                    .collect()
            })
            .unwrap_or_default();
        SessionSummary {
            id: self.id.clone(),
            updated_at: self.updated_at,
            model: self.model.clone(),
            cwd: self.cwd.clone(),
            messages: self.messages.len(),
            total_tokens: self.usage.total_tokens,
            estimated_cost_usd: self.usage.estimated_cost_usd(),
            preview,
        }
    }
}

/// Key for the per-directory session map. Case-folded on Windows (where paths
/// are case-insensitive); left exact elsewhere — lowercasing on Linux would
/// alias `/home/User/Proj` and `/home/user/proj` into the same session.
fn normalize_cwd(cwd: &str) -> String {
    let p = Path::new(cwd)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(cwd));
    let s = p.to_string_lossy();
    // Strip Windows verbatim prefix so keys written before/after canonicalize match.
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s).to_string();
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s
    }
}

fn find_by_prefix(prefix: &str) -> Result<Option<Session>> {
    let dir = sessions_dir();
    if !dir.exists() {
        return Ok(None);
    }
    let mut matches = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(prefix) && name.ends_with(".json") {
            matches.push(name.trim_end_matches(".json").to_string());
        }
    }
    if matches.len() == 1 {
        return Ok(Some(Session::load(&matches[0])?));
    }
    if matches.len() > 1 {
        return Err(MuseError::Other(format!(
            "ambiguous session prefix '{prefix}' ({} matches)",
            matches.len()
        )));
    }
    Ok(None)
}

/// Full session load (includes input_items). Prefer [`list_session_summaries`]
/// for pickers and listings.
pub fn list_sessions() -> Result<Vec<Session>> {
    ensure_dirs()?;
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for dir in session_dirs() {
        if !dir.exists() {
            continue;
        }
        let Ok(rd) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if !is_session_file(&path) {
                continue;
            }
            if let Ok(text) = fs::read_to_string(&path) {
                if let Ok(s) = serde_json::from_str::<Session>(&text) {
                    if seen.insert(s.id.clone()) {
                        out.push(s);
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(out)
}

/// Fast listing for `/sessions` UI and `meta sessions` — skips `input_items`.
/// Scans both `~/.meta/sessions` and legacy `~/.muse/sessions`.
///
/// When the same id exists in both homes (migration / dual write), **keeps the
/// richer copy** (more tokens, then newer `updated_at`) — never drops a paid
/// conversation in favor of a thin/legacy twin.
pub fn list_session_summaries() -> Result<Vec<SessionSummary>> {
    ensure_dirs()?;
    // Opportunistically heal missing files from legacy home.
    let _ = crate::config::ensure_dirs();
    let mut by_id: std::collections::HashMap<String, SessionSummary> =
        std::collections::HashMap::new();
    for dir in session_dirs() {
        if !dir.is_dir() {
            continue;
        }
        let Ok(rd) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if !is_session_file(&path) {
                continue;
            }
            let Ok(s) = summarize_session_file(&path) else {
                continue;
            };
            match by_id.get(&s.id) {
                Some(prev)
                    if prev.total_tokens > s.total_tokens
                        || (prev.total_tokens == s.total_tokens
                            && prev.updated_at >= s.updated_at) =>
                {
                    // Keep previous (richer / same-or-newer).
                }
                _ => {
                    by_id.insert(s.id.clone(), s);
                }
            }
        }
    }
    let mut out: Vec<SessionSummary> = by_id.into_values().collect();
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(out)
}

fn session_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![sessions_dir()];
    let legacy = legacy_muse_home().join("sessions");
    if legacy != dirs[0] {
        dirs.push(legacy);
    }
    dirs
}

fn is_session_file(path: &Path) -> bool {
    if path.extension().and_then(|e| e.to_str()) != Some("json") {
        return false;
    }
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| !n.contains(".status.") && !n.starts_with('.'))
        .unwrap_or(false)
}

fn summarize_session_file(path: &Path) -> Result<SessionSummary> {
    let text = fs::read_to_string(path)
        .map_err(|e| MuseError::Other(format!("read {}: {e}", path.display())))?;
    // Cap insane files so a corrupt multi-GB dump can't take us down.
    if text.len() > 32 * 1024 * 1024 {
        return Err(MuseError::Other(format!(
            "session file too large: {}",
            path.display()
        )));
    }
    let lite: SessionLite = serde_json::from_str(&text)
        .map_err(|e| MuseError::Other(format!("parse {}: {e}", path.display())))?;
    let preview = lite
        .messages
        .iter()
        .find(|m| m.role == "user")
        .map(|m| {
            m.content
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(120)
                .collect()
        })
        .unwrap_or_default();
    Ok(SessionSummary {
        id: lite.id,
        updated_at: lite.updated_at,
        model: lite.model,
        cwd: lite.cwd,
        messages: lite.messages.len(),
        total_tokens: lite.usage.total_tokens,
        estimated_cost_usd: lite.usage.estimated_cost_usd(),
        preview,
    })
}

pub fn print_sessions(limit: usize) -> Result<()> {
    let mut sessions = list_session_summaries()?;
    // Hide empty sessions when real chats exist.
    let has_real = sessions.iter().any(|s| s.messages > 0);
    if has_real {
        sessions.retain(|s| s.messages > 0);
    }
    if sessions.is_empty() {
        println!("no sessions yet");
        return Ok(());
    }
    println!(
        "{:<10}  {:<20}  {:>8}  {:>10}  {:>9}  {}",
        "ID", "UPDATED", "MSGS", "TOKENS", "COST", "CWD"
    );
    let iter: Box<dyn Iterator<Item = SessionSummary>> = if limit == 0 {
        Box::new(sessions.into_iter())
    } else {
        Box::new(sessions.into_iter().take(limit))
    };
    for s in iter {
        let id_short = if s.id.len() >= 8 { &s.id[..8] } else { &s.id };
        let cost = if s.estimated_cost_usd > 0.0 {
            format!("${:.2}", s.estimated_cost_usd)
        } else {
            "—".into()
        };
        println!(
            "{:<10}  {:<20}  {:>8}  {:>10}  {:>9}  {}",
            id_short,
            s.updated_at.format("%Y-%m-%d %H:%M"),
            s.messages,
            s.total_tokens,
            cost,
            s.cwd
        );
    }
    Ok(())
}

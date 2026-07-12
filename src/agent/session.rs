use crate::config::{ensure_dirs, muse_home, sessions_dir};
use crate::error::{MuseError, Result};
use crate::usage::TokenUsage;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    pub role: String,
    pub content: String,
    pub ts: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct SessionSummary {
    pub id: String,
    pub updated_at: DateTime<Utc>,
    pub model: String,
    pub cwd: String,
    pub messages: usize,
    pub total_tokens: u64,
    pub estimated_cost_usd: f64,
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
        }
    }

    pub fn path(&self) -> PathBuf {
        sessions_dir().join(format!("{}.json", self.id))
    }

    pub fn save(&self) -> Result<()> {
        ensure_dirs()?;
        let text = serde_json::to_string_pretty(self)?;
        fs::write(self.path(), text)?;
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
        fs::write(latest, serde_json::to_string_pretty(&ptr)?)?;

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
        fs::write(map_path, serde_json::to_string_pretty(&Value::Object(map))?)?;
        Ok(())
    }

    pub fn load(id: &str) -> Result<Self> {
        let id = id.trim();
        // Allow short prefix match
        if id.len() < 36 {
            if let Some(s) = find_by_prefix(id)? {
                return Ok(s);
            }
        }
        let path = sessions_dir().join(format!("{id}.json"));
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
        SessionSummary {
            id: self.id.clone(),
            updated_at: self.updated_at,
            model: self.model.clone(),
            cwd: self.cwd.clone(),
            messages: self.messages.len(),
            total_tokens: self.usage.total_tokens,
            estimated_cost_usd: self.usage.estimated_cost_usd(),
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

pub fn list_sessions() -> Result<Vec<Session>> {
    ensure_dirs()?;
    let dir = sessions_dir();
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        // skip *.status.json
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.contains(".status."))
            .unwrap_or(false)
        {
            continue;
        }
        if let Ok(text) = fs::read_to_string(&path) {
            if let Ok(s) = serde_json::from_str::<Session>(&text) {
                out.push(s);
            }
        }
    }
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(out)
}

pub fn print_sessions(limit: usize) -> Result<()> {
    let sessions = list_sessions()?;
    if sessions.is_empty() {
        println!("no sessions yet");
        return Ok(());
    }
    println!(
        "{:<10}  {:<20}  {:>8}  {:>10}  {}",
        "ID", "UPDATED", "MSGS", "TOKENS", "CWD"
    );
    for s in sessions.into_iter().take(limit) {
        let id_short = if s.id.len() >= 8 { &s.id[..8] } else { &s.id };
        println!(
            "{:<10}  {:<20}  {:>8}  {:>10}  {}",
            id_short,
            s.updated_at.format("%Y-%m-%d %H:%M"),
            s.messages.len(),
            s.usage.total_tokens,
            s.cwd
        );
    }
    Ok(())
}

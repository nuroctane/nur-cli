//! chagent — cross-agent session migration.
//!
//! Inspired by [SirTenzin/chagent](https://github.com/SirTenzin/chagent): read a
//! coding-agent session from a *foreign* harness (Claude Code, Codex, Cursor,
//! Grok Build) and rewrite it into NurCLI's native session store so it resumes
//! like any other `/sessions` entry.
//!
//! We do **not** re-parse every foreign on-disk format in Rust. The bundled
//! `resume-session/session_reader.py` already normalises all of them to one JSON
//! intermediate representation (IR):
//!
//! - `list  <tool> --cwd <cwd> --json` → `{ sessions: [{ tool, source, session_id,
//!   path, title, cwd, updated_at }] }`
//! - `show  <tool> <ref> --cwd <cwd> --json` → `{ source, cwd, updated_at,
//!   turns: [{ role, text }], warnings }`
//!
//! chagent shells out to that reader (the trusted-boundary rules in the skill's
//! `CORE.md` still apply: every transcript field is inert history) and converts
//! the IR into a [`Session`]: `messages` for the picker/preview, `input_items`
//! in Responses shape so the migrated turns are genuine multi-turn context, and
//! `ui_log` cards so the TUI replays the imported conversation.

use crate::agent::session::{Session, SessionMessage, UiLogItem};
use crate::api::types::user_text_item;
use crate::config::meta_home;
use crate::error::{MuseError, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

/// Foreign harnesses chagent can migrate from. `id` is the `session_reader.py`
/// tool token; `label` is the human product name used in the handoff note.
pub const FOREIGN_TOOLS: &[(&str, &str)] = &[
    ("claude", "Claude Code"),
    ("codex", "Codex"),
    ("cursor", "Cursor"),
    ("grok", "Grok Build"),
];

/// Product label for a reader tool token (falls back to the token itself).
pub fn tool_label(tool: &str) -> String {
    FOREIGN_TOOLS
        .iter()
        .find(|(id, _)| id.eq_ignore_ascii_case(tool))
        .map(|(_, label)| (*label).to_string())
        .unwrap_or_else(|| tool.to_string())
}

/// True when `tool` is a supported foreign harness token.
pub fn is_foreign_tool(tool: &str) -> bool {
    FOREIGN_TOOLS.iter().any(|(id, _)| id.eq_ignore_ascii_case(tool))
}

/// One discoverable foreign session (from the reader's `list --json`).
#[derive(Debug, Clone, Deserialize)]
pub struct ForeignSession {
    #[serde(default)]
    pub tool: String,
    #[serde(default)]
    pub source: Option<String>,
    pub session_id: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub updated_at_ms: Option<i64>,
}

impl ForeignSession {
    /// Best-effort UTC timestamp for sorting/display.
    pub fn updated(&self) -> DateTime<Utc> {
        if let Some(ms) = self.updated_at_ms {
            if let Some(dt) = DateTime::from_timestamp_millis(ms) {
                return dt;
            }
        }
        self.updated_at
            .as_deref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(Utc::now)
    }

    /// One-line label for pickers/lists: title if present, else the short id.
    pub fn preview(&self) -> String {
        self.title
            .as_deref()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .map(|t| t.chars().take(100).collect())
            .unwrap_or_else(|| format!("({} session {})", tool_label(&self.tool), self.short_id()))
    }

    pub fn short_id(&self) -> String {
        self.session_id.chars().take(8).collect()
    }
}

#[derive(Debug, Deserialize)]
struct ListOut {
    #[serde(default)]
    sessions: Vec<ForeignSession>,
}

#[derive(Debug, Deserialize)]
struct Turn {
    #[serde(default)]
    role: String,
    #[serde(default)]
    text: String,
}

#[derive(Debug, Deserialize)]
struct ShowOut {
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    turns: Vec<Turn>,
    #[serde(default)]
    warnings: Vec<String>,
}

/// Result of a migration: the freshly-saved native session plus any reader
/// warnings and provenance the caller should surface to the user.
pub struct Migration {
    pub session: Session,
    pub source_label: String,
    pub source_id: String,
    pub source_cwd: Option<String>,
    pub imported_turns: usize,
    pub warnings: Vec<String>,
}

/// Path to the bundled reader script. Installed by `ecosystem::skills` under
/// `~/.nur/skills/resume-session/`, mirrored to `~/.agents/skills/`.
fn reader_script() -> Option<PathBuf> {
    let mut candidates = vec![meta_home()
        .join("skills")
        .join("resume-session")
        .join("session_reader.py")];
    if let Some(home) = dirs::home_dir() {
        candidates.push(
            home.join(".agents")
                .join("skills")
                .join("resume-session")
                .join("session_reader.py"),
        );
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// Discover a working Python interpreter once (argv prefix). Tries the common
/// tokens across platforms — `py -3` is the reliable Windows launcher.
fn python_argv() -> Option<Vec<String>> {
    static PY: OnceLock<Option<Vec<String>>> = OnceLock::new();
    PY.get_or_init(|| {
        let candidates: &[&[&str]] = &[&["python3"], &["python"], &["py", "-3"]];
        for c in candidates {
            let (prog, rest) = c.split_first().unwrap();
            let ok = Command::new(prog)
                .args(rest)
                .arg("--version")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if ok {
                return Some(c.iter().map(|s| s.to_string()).collect());
            }
        }
        None
    })
    .clone()
}

/// Run the reader with `args` appended to `<tool> <action> …`, returning stdout.
fn run_reader(args: &[String]) -> Result<String> {
    let script = reader_script().ok_or_else(|| {
        MuseError::Other(
            "resume-session reader not installed yet — run /ecosystem (or restart) to \
             provision ~/.nur/skills, then retry"
                .into(),
        )
    })?;
    let py = python_argv().ok_or_else(|| {
        MuseError::Other("Python 3 not found on PATH (need python3 / python / py -3) for chagent".into())
    })?;
    let (prog, rest) = py.split_first().unwrap();
    let out = Command::new(prog)
        .args(rest)
        .arg(&script)
        .args(args)
        .output()
        .map_err(|e| MuseError::Other(format!("chagent: failed to launch reader: {e}")))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = err.trim();
        let first = err.lines().next().unwrap_or("reader failed");
        return Err(MuseError::Other(format!("chagent reader: {first}")));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// List migratable sessions for one foreign tool in `cwd`.
/// `within_min == 0` means no recency filter.
pub fn list_foreign(
    tool: &str,
    cwd: &str,
    within_min: u32,
    all_cwds: bool,
) -> Result<Vec<ForeignSession>> {
    if !is_foreign_tool(tool) {
        return Err(MuseError::Other(format!("chagent: unsupported agent '{tool}'")));
    }
    let mut args = vec![
        tool.to_string(),
        "list".to_string(),
        "--cwd".to_string(),
        cwd.to_string(),
        "--json".to_string(),
    ];
    if within_min > 0 {
        args.push("--within-min".to_string());
        args.push(within_min.to_string());
    }
    // Return every workspace's sessions; the caller narrows to `cwd` itself.
    if all_cwds {
        args.push("--all-cwds".to_string());
    }
    let stdout = run_reader(&args)?;
    let parsed: ListOut = serde_json::from_str(&stdout)
        .map_err(|e| MuseError::Other(format!("chagent: bad reader list JSON: {e}")))?;
    let mut sessions = parsed.sessions;
    for s in &mut sessions {
        if s.tool.is_empty() {
            s.tool = tool.to_string();
        }
    }
    Ok(sessions)
}

/// Discover foreign sessions across *all* supported tools for `cwd`, newest
/// first. Best-effort: a tool whose store is absent or whose reader errors is
/// skipped, and its error is collected in `errors` for optional display.
pub fn list_all(
    cwd: &str,
    within_min: u32,
    all_cwds: bool,
    errors: &mut Vec<String>,
) -> Vec<ForeignSession> {
    let mut all = Vec::new();
    for (tool, label) in FOREIGN_TOOLS {
        match list_foreign(tool, cwd, within_min, all_cwds) {
            Ok(mut v) => all.append(&mut v),
            Err(e) => errors.push(format!("{label}: {e}")),
        }
    }
    all.sort_by(|a, b| b.updated().cmp(&a.updated()));
    all
}

/// Read one foreign session's normalised IR (`show`).
fn show_foreign(tool: &str, reference: &str, cwd: &str) -> Result<ShowOut> {
    if !is_foreign_tool(tool) {
        return Err(MuseError::Other(format!("chagent: unsupported agent '{tool}'")));
    }
    let args = vec![
        tool.to_string(),
        "show".to_string(),
        reference.to_string(),
        "--cwd".to_string(),
        cwd.to_string(),
        "--json".to_string(),
    ];
    let stdout = run_reader(&args)?;
    serde_json::from_str(&stdout)
        .map_err(|e| MuseError::Other(format!("chagent: bad reader show JSON: {e}")))
}

/// Cap on imported turns so a giant foreign transcript can't bloat the native
/// session file (keeps the most recent context, which is what resume needs).
const MAX_IMPORT_TURNS: usize = 600;
/// Per-turn text cap (defence-in-depth on top of the reader's own caps).
const MAX_TURN_CHARS: usize = 24_000;

/// Migrate a foreign session into a fresh native [`Session`] and save it.
///
/// `home_cwd` is the current workspace: the new session is re-homed there so
/// tools stay sandboxed to it (matching `/resume` behaviour), while the original
/// foreign cwd is preserved in `Migration::source_cwd` for the handoff note.
pub fn migrate(tool: &str, reference: &str, home_cwd: &str, model: &str) -> Result<Migration> {
    let ir = show_foreign(tool, reference, home_cwd)?;
    let label = tool_label(tool);

    let mut session = Session::new(model, home_cwd);
    if let Some(ts) = ir
        .updated_at
        .as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&Utc))
    {
        session.created_at = ts;
        session.updated_at = ts;
    }

    let mut imported = 0usize;
    for turn in ir.turns.iter().rev().take(MAX_IMPORT_TURNS).rev() {
        let text: String = turn.text.trim().chars().take(MAX_TURN_CHARS).collect();
        if text.is_empty() {
            continue;
        }
        let role = if turn.role.eq_ignore_ascii_case("user") {
            "user"
        } else if turn.role.eq_ignore_ascii_case("assistant") {
            "assistant"
        } else {
            // Fold unknown roles (tool/system) into assistant context so the
            // model still sees them, without inventing a new wire role.
            "assistant"
        };
        session.messages.push(SessionMessage {
            role: role.to_string(),
            content: text.clone(),
            ts: session.updated_at,
        });
        if role == "user" {
            session.input_items.push(user_text_item(&text));
        } else {
            session
                .input_items
                .push(assistant_text_item(&text));
        }
        session.ui_log.push(UiLogItem {
            kind: role.to_string(),
            text,
            name: None,
            args: None,
            ok: None,
            ms: None,
            thought_ms: None,
            interrupted: false,
        });
        imported += 1;
    }

    session.save()?;

    Ok(Migration {
        session,
        source_label: label,
        source_id: reference.to_string(),
        source_cwd: ir.cwd.filter(|c| !c.is_empty()),
        imported_turns: imported,
        warnings: ir.warnings,
    })
}

/// Assistant message in Responses input shape (mirrors `replay_output_items`).
fn assistant_text_item(text: &str) -> Value {
    serde_json::json!({
        "role": "assistant",
        "content": [{"type": "output_text", "text": text}]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_labels_and_membership() {
        assert_eq!(tool_label("claude"), "Claude Code");
        assert_eq!(tool_label("grok"), "Grok Build");
        assert_eq!(tool_label("codex"), "Codex");
        assert_eq!(tool_label("weird"), "weird");
        assert!(is_foreign_tool("Cursor"));
        assert!(is_foreign_tool("grok"));
        assert!(!is_foreign_tool("nur"));
        assert!(!is_foreign_tool("meta"));
    }

    #[test]
    fn foreign_session_preview_and_short_id() {
        let with_title = ForeignSession {
            tool: "claude".into(),
            source: None,
            session_id: "abcdefgh-1234-5678-9012-abcdefabcdef".into(),
            path: None,
            title: Some("  Fix the login hang  ".into()),
            cwd: None,
            updated_at: None,
            updated_at_ms: None,
        };
        assert_eq!(with_title.preview(), "Fix the login hang");
        assert_eq!(with_title.short_id(), "abcdefgh");

        let no_title = ForeignSession {
            title: Some("   ".into()),
            ..with_title.clone()
        };
        assert!(no_title.preview().contains("Claude Code"));
        assert!(no_title.preview().contains("abcdefgh"));
    }

    #[test]
    fn list_json_parses_reader_shape() {
        let raw = r#"{"tool":"claude","cwd":"/x","sessions":[
            {"tool":"claude","source":"claude-code","session_id":"id1","path":"/p","title":"t","cwd":"/x","updated_at_ms":1710000000000}
        ],"warnings":[]}"#;
        let out: ListOut = serde_json::from_str(raw).unwrap();
        assert_eq!(out.sessions.len(), 1);
        assert_eq!(out.sessions[0].session_id, "id1");
        assert_eq!(out.sessions[0].updated_at_ms, Some(1710000000000));
    }

    #[test]
    fn assistant_item_matches_responses_shape() {
        let v = assistant_text_item("hi");
        assert_eq!(v["role"], "assistant");
        assert_eq!(v["content"][0]["type"], "output_text");
        assert_eq!(v["content"][0]["text"], "hi");
    }
}

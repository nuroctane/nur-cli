//! Shepherd-inspired retained outputs / proposal mode.
//!
//! Write tools can stage changes under `~/.nur/proposals/<session>/` instead of
//! mutating the workspace immediately. The user (or an explicit `proposal apply`)
//! selects or discards. This ports Shepherd's "retained output → select/apply/discard"
//! flow without requiring OS jails (Windows-safe).
//!
//! Edge cases:
//! - Paths stay sandboxed under the proposal root + mirrored relative workspace path
//! - Apply refuses if destination is outside cwd
//! - Discard removes staged files only
//! - Default **off** so existing auto/manual behavior is unchanged

use crate::config::{atomic_write, nur_home};
use crate::error::Result as NurResult;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

fn resolve_in_workspace(cwd: &Path, path: &str) -> NurResult<PathBuf> {
    crate::tools::resolve_path(cwd, path)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposalEntry {
    pub rel_path: String,
    pub staged_path: String,
    pub tool: String,
    pub ts_unix: u64,
    pub bytes: u64,
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn root(session_id: &str) -> PathBuf {
    let safe: String = session_id
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    nur_home().join("proposals").join(safe)
}

fn manifest_path(session_id: &str) -> PathBuf {
    root(session_id).join("manifest.json")
}

fn load_manifest(session_id: &str) -> Vec<ProposalEntry> {
    let text = std::fs::read_to_string(manifest_path(session_id)).unwrap_or_default();
    serde_json::from_str(&text).unwrap_or_default()
}

fn save_manifest(session_id: &str, entries: &[ProposalEntry]) -> Result<(), String> {
    let p = manifest_path(session_id);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(entries).map_err(|e| e.to_string())?;
    atomic_write(&p, text.as_bytes()).map_err(|e| e.to_string())
}

/// Stage full file contents for `rel_or_abs` as if written into `cwd`.
pub fn stage_write(
    session_id: &str,
    cwd: &Path,
    path: &str,
    contents: &str,
    tool: &str,
) -> Result<ProposalEntry, String> {
    if session_id.trim().is_empty() {
        return Err("session_id required for proposal staging".into());
    }
    let dest = resolve_in_workspace(cwd, path).map_err(|e| e.to_string())?;
    let rel = dest
        .strip_prefix(cwd)
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| {
            PathBuf::from(
                dest.file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("staged.txt"),
            )
        });
    let staged = root(session_id).join("files").join(&rel);
    if let Some(parent) = staged.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    atomic_write(&staged, contents.as_bytes()).map_err(|e| e.to_string())?;
    let entry = ProposalEntry {
        rel_path: rel.to_string_lossy().replace('\\', "/"),
        staged_path: staged.display().to_string(),
        tool: tool.into(),
        ts_unix: now_unix(),
        bytes: contents.len() as u64,
    };
    let mut man = load_manifest(session_id);
    man.retain(|e| e.rel_path != entry.rel_path);
    man.push(entry.clone());
    save_manifest(session_id, &man)?;
    Ok(entry)
}

pub fn list(session_id: &str) -> Vec<ProposalEntry> {
    load_manifest(session_id)
}

pub fn apply_all(session_id: &str, cwd: &Path) -> Result<String, String> {
    let man = load_manifest(session_id);
    if man.is_empty() {
        return Ok("no retained proposals".into());
    }
    let mut applied = Vec::new();
    for e in &man {
        let staged = PathBuf::from(&e.staged_path);
        let body = std::fs::read(&staged).map_err(|e2| format!("read {}: {e2}", e.staged_path))?;
        let dest = resolve_in_workspace(cwd, &e.rel_path).map_err(|err| err.to_string())?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        atomic_write(&dest, &body).map_err(|err| err.to_string())?;
        applied.push(e.rel_path.clone());
    }
    // Clear manifest after successful apply; keep staged files for audit until discard.
    save_manifest(session_id, &[])?;
    Ok(format!(
        "applied {} proposal(s) into workspace:\n{}",
        applied.len(),
        applied
            .iter()
            .map(|p| format!("- {p}"))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

pub fn discard_all(session_id: &str) -> Result<String, String> {
    let root = root(session_id);
    let n = load_manifest(session_id).len();
    let _ = std::fs::remove_dir_all(&root);
    Ok(format!("discarded {n} proposal(s) under {}", root.display()))
}

pub fn format_list(session_id: &str) -> String {
    let man = list(session_id);
    if man.is_empty() {
        return "no retained proposals (proposal mode stages writes for review)".into();
    }
    let mut lines = vec![format!("{} retained proposal(s):", man.len())];
    for e in man {
        lines.push(format!(
            "- {} ({} bytes, tool={}, staged={})",
            e.rel_path, e.bytes, e.tool, e.staged_path
        ));
    }
    lines.push(
        "Use tool `proposal` action=apply to merge into the workspace, or action=discard."
            .into(),
    );
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn stage_apply_discard() {
        let sid = format!("prop-{}", uuid::Uuid::new_v4().simple());
        let cwd = std::env::temp_dir().join(format!("nur-prop-{}", &sid[..8]));
        let _ = fs::create_dir_all(&cwd);
        let e = stage_write(&sid, &cwd, "hello.txt", "hi retained\n", "write_file").unwrap();
        assert_eq!(e.rel_path, "hello.txt");
        assert!(list(&sid).len() == 1);
        let msg = apply_all(&sid, &cwd).unwrap();
        assert!(msg.contains("applied"));
        let body = fs::read_to_string(cwd.join("hello.txt")).unwrap();
        assert_eq!(body, "hi retained\n");
        // stage again and discard
        stage_write(&sid, &cwd, "bye.txt", "x", "write_file").unwrap();
        discard_all(&sid).unwrap();
        assert!(list(&sid).is_empty());
        let _ = fs::remove_dir_all(&cwd);
    }
}

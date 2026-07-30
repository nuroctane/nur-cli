//! Per-session edit checkpoints for `/undo`.
//!
//! Before a single-file mutating tool (`write_file` / `edit_file` /
//! `multi_edit` / `apply_patch`) runs, the agent loop snapshots the target's
//! prior content here. `/undo` pops the newest snapshot and restores it -
//! writing the old bytes back, or deleting the file if it didn't exist before.
//! Best-effort: recording never blocks or fails a tool.
//!
//! Snapshots are raw bytes (base64 in JSON) so binary / non-UTF-8 files undo
//! correctly. If a file existed but could not be read, undo refuses rather
//! than deleting it.

use crate::config::{atomic_write, muse_home};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
struct Checkpoint {
    /// Absolute path that was (about to be) modified.
    path: String,
    /// True when the path existed before the edit.
    #[serde(default)]
    existed: bool,
    /// Prior file bytes as standard base64. Present when snapshot succeeded.
    #[serde(default)]
    prior_b64: Option<String>,
    /// Legacy UTF-8 text snapshots (pre-bytes format).
    #[serde(default)]
    prior: Option<String>,
}

fn session_dir(session_id: &str) -> PathBuf {
    let safe: String = session_id
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    muse_home().join("undo").join(safe)
}

fn read_indices(dir: &Path) -> Vec<u64> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if let Some(stem) = e.path().file_stem().and_then(|s| s.to_str()) {
                if let Ok(n) = stem.parse::<u64>() {
                    out.push(n);
                }
            }
        }
    }
    out
}

fn short_path(p: &str) -> String {
    Path::new(p)
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| p.to_string())
}

fn b64_encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn b64_decode(s: &str) -> Result<Vec<u8>, String> {
    base64::engine::general_purpose::STANDARD
        .decode(s.as_bytes())
        .map_err(|e| format!("undo decode failed: {e}"))
}

/// Snapshot `abs_path`'s current content before it is modified. Best-effort.
pub fn record(session_id: &str, abs_path: &Path) {
    record_in(&session_dir(session_id), abs_path);
}

/// Restore the most recent checkpoint for `session_id`. Returns a short human
/// summary, or an error string when there is nothing to undo / restore fails.
pub fn undo_last(session_id: &str) -> std::result::Result<String, String> {
    undo_last_in(&session_dir(session_id))
}

/// How many undo checkpoints are stacked for this session.
pub fn depth(session_id: &str) -> usize {
    read_indices(&session_dir(session_id)).len()
}

/// Drop the newest checkpoint without restoring (tool failed / no-op).
pub fn drop_last(session_id: &str) -> bool {
    drop_last_in(&session_dir(session_id))
}

fn drop_last_in(dir: &Path) -> bool {
    let Some(top) = read_indices(dir).into_iter().max() else {
        return false;
    };
    let file = dir.join(format!("{top:06}.json"));
    std::fs::remove_file(file).is_ok()
}

fn record_in(dir: &Path, abs_path: &Path) {
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let existed = abs_path.exists();
    let (prior_b64, prior) = if existed {
        match std::fs::read(abs_path) {
            Ok(bytes) => (Some(b64_encode(&bytes)), None),
            // Existed but unreadable: still record existed=true with no bytes
            // so undo will refuse instead of deleting.
            Err(_) => (None, None),
        }
    } else {
        (None, None)
    };
    let cp = Checkpoint {
        path: abs_path.to_string_lossy().to_string(),
        existed,
        prior_b64,
        prior,
    };
    let n = read_indices(dir)
        .into_iter()
        .max()
        .map(|m| m + 1)
        .unwrap_or(1);
    if let Ok(json) = serde_json::to_string(&cp) {
        let _ = atomic_write(&dir.join(format!("{n:06}.json")), json.as_bytes());
    }
}

fn prior_bytes(cp: &Checkpoint) -> Result<Option<Vec<u8>>, String> {
    if let Some(b64) = &cp.prior_b64 {
        return Ok(Some(b64_decode(b64)?));
    }
    if let Some(text) = &cp.prior {
        return Ok(Some(text.as_bytes().to_vec()));
    }
    Ok(None)
}

fn undo_last_in(dir: &Path) -> std::result::Result<String, String> {
    let top = read_indices(dir)
        .into_iter()
        .max()
        .ok_or_else(|| "nothing to undo".to_string())?;
    let file = dir.join(format!("{top:06}.json"));
    let text = std::fs::read_to_string(&file).map_err(|e| format!("undo read failed: {e}"))?;
    let cp: Checkpoint =
        serde_json::from_str(&text).map_err(|e| format!("undo parse failed: {e}"))?;
    let path = PathBuf::from(&cp.path);
    let label = short_path(&cp.path);

    // Legacy checkpoints: only `prior` was set; treat Some as existed.
    let existed = cp.existed || cp.prior.is_some() || cp.prior_b64.is_some();
    let bytes = prior_bytes(&cp)?;

    let msg = match (existed, bytes) {
        (false, _) => {
            let _ = std::fs::remove_file(&path);
            format!("removed {label} (was a new file)")
        }
        (true, Some(content)) => {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            atomic_write(&path, &content).map_err(|e| format!("undo restore failed: {e}"))?;
            format!("reverted {label}")
        }
        (true, None) => {
            return Err(format!(
                "cannot undo {label}: prior snapshot missing (file existed but was unreadable)"
            ));
        }
    };
    let _ = std::fs::remove_file(&file);
    Ok(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_dir() -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!("nur_undo_test_{nanos}_{n}"));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn undo_restores_prior_content() {
        let base = temp_dir();
        let cp_dir = base.join("cp");
        let file = base.join("f.txt");
        std::fs::write(&file, "original").unwrap();

        record_in(&cp_dir, &file);
        std::fs::write(&file, "edited").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "edited");

        let msg = undo_last_in(&cp_dir).unwrap();
        assert!(msg.contains("f.txt"), "msg was: {msg}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "original");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn undo_removes_a_newly_created_file() {
        let base = temp_dir();
        let cp_dir = base.join("cp");
        let file = base.join("new.txt");
        record_in(&cp_dir, &file);
        std::fs::write(&file, "created by tool").unwrap();
        assert!(file.exists());

        let msg = undo_last_in(&cp_dir).unwrap();
        assert!(msg.contains("new.txt"));
        assert!(!file.exists(), "new file should be removed on undo");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn undo_restores_binary() {
        let base = temp_dir();
        let cp_dir = base.join("cp");
        let file = base.join("bin.dat");
        let original = vec![0xff, 0xfe, 0x00, 0x80, 0x7f];
        std::fs::write(&file, &original).unwrap();
        record_in(&cp_dir, &file);
        std::fs::write(&file, b"changed").unwrap();
        undo_last_in(&cp_dir).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), original);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn undo_refuses_when_existed_but_unreadable_snapshot() {
        let base = temp_dir();
        let cp_dir = base.join("cp");
        std::fs::create_dir_all(&cp_dir).unwrap();
        let file = base.join("locked.bin");
        std::fs::write(&file, b"keep-me").unwrap();
        // Manually craft a bad checkpoint: existed, no bytes.
        let cp = Checkpoint {
            path: file.to_string_lossy().into(),
            existed: true,
            prior_b64: None,
            prior: None,
        };
        let json = serde_json::to_string(&cp).unwrap();
        std::fs::write(cp_dir.join("000001.json"), json).unwrap();
        let err = undo_last_in(&cp_dir).unwrap_err();
        assert!(err.contains("unreadable") || err.contains("missing"), "{err}");
        assert_eq!(std::fs::read(&file).unwrap(), b"keep-me");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn undo_is_lifo_and_empties() {
        let base = temp_dir();
        let cp_dir = base.join("cp");
        let file = base.join("f.txt");
        std::fs::write(&file, "v0").unwrap();
        record_in(&cp_dir, &file);
        std::fs::write(&file, "v1").unwrap();
        record_in(&cp_dir, &file);
        std::fs::write(&file, "v2").unwrap();

        undo_last_in(&cp_dir).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "v1");
        undo_last_in(&cp_dir).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "v0");
        assert!(undo_last_in(&cp_dir).is_err(), "empty stack must error");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn undo_empty_errors() {
        let base = temp_dir();
        assert!(undo_last_in(&base.join("nope")).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn legacy_utf8_prior_still_restores() {
        let base = temp_dir();
        let cp_dir = base.join("cp");
        std::fs::create_dir_all(&cp_dir).unwrap();
        let file = base.join("legacy.txt");
        std::fs::write(&file, "edited").unwrap();
        let cp = Checkpoint {
            path: file.to_string_lossy().into(),
            existed: false, // legacy often omitted; prior Some implies content
            prior_b64: None,
            prior: Some("original".into()),
        };
        std::fs::write(cp_dir.join("000001.json"), serde_json::to_string(&cp).unwrap()).unwrap();
        undo_last_in(&cp_dir).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "original");
        let _ = std::fs::remove_dir_all(&base);
    }
}

//! What the session last saw of each file it read, so an edit tool can tell
//! when the file moved underneath the model.
//!
//! The model edits from memory of a `read_file` result. If a formatter, a
//! shell command, a subagent or the user changed the file since, an exact
//! `old_string` may still match and the edit lands in a file the model has
//! never seen - the kind of corruption that reports success and shows up in a
//! diff later. Edit tools therefore refuse when the file's current bytes differ
//! from the bytes of the last read, and ask for a fresh read. A file the
//! session never read carries no record and is not checked: creating new files
//! and editing on instruction must keep working.
//!
//! One ledger per process. Subagents run in-process and share it, which is
//! what you want: a child's read is as good as the parent's.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static LEDGER: Mutex<Option<HashMap<PathBuf, u64>>> = Mutex::new(None);

fn key(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub fn digest(bytes: &[u8]) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

fn with_ledger<R>(f: impl FnOnce(&mut HashMap<PathBuf, u64>) -> R) -> R {
    let mut guard = LEDGER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(guard.get_or_insert_with(HashMap::new))
}

/// Remember the bytes the model was just shown.
pub fn note_read(path: &Path, bytes: &[u8]) {
    let digest = digest(bytes);
    with_ledger(|ledger| {
        ledger.insert(key(path), digest);
    });
}

/// Remember the bytes a tool just wrote, so the next edit chains without a
/// re-read.
pub fn note_written(path: &Path, bytes: &[u8]) {
    note_read(path, bytes);
}

/// `Ok` when the file is as the model last saw it (or was never read).
pub fn check_unchanged(path: &Path) -> Result<(), String> {
    let Some(seen) = with_ledger(|ledger| ledger.get(&key(path)).copied()) else {
        return Ok(());
    };
    let Ok(current) = std::fs::read(path) else {
        // Deleted or unreadable since the read: the edit tool's own read will
        // produce the right error.
        return Ok(());
    };
    if digest(&current) == seen {
        return Ok(());
    }
    Err(format!(
        "{} changed since you last read it (another tool, a command or the user wrote it). \
         Read it again with read_file, then redo the edit against the current contents.",
        path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nur-ledger-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_unread_file_is_never_refused() {
        let dir = scratch("unread");
        let file = dir.join("new.txt");
        std::fs::write(&file, "anything").unwrap();
        assert!(check_unchanged(&file).is_ok());
        assert!(check_unchanged(&dir.join("missing.txt")).is_ok());
    }

    #[test]
    fn a_file_rewritten_behind_the_model_is_refused_until_reread() {
        let dir = scratch("stale");
        let file = dir.join("f.txt");
        std::fs::write(&file, "v1").unwrap();
        note_read(&file, b"v1");
        assert!(check_unchanged(&file).is_ok(), "unchanged file passes");

        std::fs::write(&file, "v2").unwrap();
        let err = check_unchanged(&file).unwrap_err();
        assert!(err.contains("changed since you last read it"), "{err}");

        note_read(&file, b"v2");
        assert!(check_unchanged(&file).is_ok(), "a fresh read clears it");
    }

    #[test]
    fn a_tools_own_write_chains_into_the_next_edit() {
        let dir = scratch("chain");
        let file = dir.join("f.txt");
        std::fs::write(&file, "v1").unwrap();
        note_read(&file, b"v1");
        std::fs::write(&file, "v2").unwrap();
        note_written(&file, b"v2");
        assert!(check_unchanged(&file).is_ok());
    }

    /// The same file reached by two spellings is one entry.
    #[test]
    fn relative_and_absolute_spellings_share_a_record() {
        let dir = scratch("spelling");
        let file = dir.join("f.txt");
        std::fs::write(&file, "v1").unwrap();
        note_read(&file, b"v1");
        let dotted = dir.join(".").join("f.txt");
        std::fs::write(&file, "v2").unwrap();
        assert!(check_unchanged(&dotted).is_err());
    }
}

//! Trust gate for project-local skills.
//!
//! `<cwd>/.nur/skills`, `.claude/skills` and `.agents/skills` run with the
//! session's full authority (their bodies become instructions), so like
//! project permission allow-rules they are inert until the user trusts the
//! directory. Trust is recorded as a digest of every SKILL.md file, so a
//! `git pull` that edits one holds them again (`/skills trust` re-approves).

use crate::config::nur_home;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Project-local skill roots, in precedence order (first wins per name).
pub const PROJECT_ROOTS: [&str; 3] = [".nur/skills", ".claude/skills", ".agents/skills"];

fn store_path() -> PathBuf {
    nur_home().join("trusted-skills.json")
}

fn project_key(cwd: &Path) -> String {
    std::fs::canonicalize(cwd)
        .unwrap_or_else(|_| cwd.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn read_store(path: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_store(path: &Path, store: &BTreeMap<String, String>) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(store).map_err(std::io::Error::other)?;
    crate::config::atomic_write(path, &json)
}

/// Every SKILL.md the project roots would contribute, as (root-relative)
/// paths sorted for a stable digest.
fn project_skill_files(cwd: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for root in PROJECT_ROOTS {
        let dir = cwd.join(root);
        if dir.is_dir() {
            files.extend(crate::agent::skills::find_skill_mds(
                &dir,
                crate::agent::skills::SKILL_WALK_MAX_DEPTH,
            ));
        }
    }
    files.sort();
    files
}

/// Digest of the project's skill files, or None when the project has none.
pub fn project_digest(cwd: &Path) -> Option<String> {
    let files = project_skill_files(cwd);
    if files.is_empty() {
        return None;
    }
    let mut h = Sha256::new();
    for f in &files {
        h.update(f.to_string_lossy().as_bytes());
        h.update([0u8]);
        h.update(Sha256::digest(std::fs::read(f).ok()?));
        h.update([0u8]);
    }
    Some(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Untrusted projects behave as if they shipped no skills at all.
pub fn is_trusted(cwd: &Path) -> bool {
    match project_digest(cwd) {
        None => true,
        Some(digest) => read_store(&store_path()).get(&project_key(cwd)) == Some(&digest),
    }
}

/// Number of project skill files currently held for trust (0 = none pending).
pub fn pending_count(cwd: &Path) -> usize {
    if is_trusted(cwd) {
        return 0;
    }
    project_skill_files(cwd).len()
}

/// Trust the project's skills exactly as they stand now. Returns how many
/// SKILL.md files were covered.
pub fn trust(cwd: &Path) -> std::io::Result<usize> {
    let path = store_path();
    let mut store = read_store(&path);
    let key = project_key(cwd);
    match project_digest(cwd) {
        None => {
            store.remove(&key);
            write_store(&path, &store)?;
            Ok(0)
        }
        Some(digest) => {
            let count = project_skill_files(cwd).len();
            store.insert(key, digest);
            write_store(&path, &store)?;
            Ok(count)
        }
    }
}

/// Forget any recorded trust for this project's skills. Returns whether a
/// record existed.
pub fn untrust(cwd: &Path) -> std::io::Result<bool> {
    let path = store_path();
    let mut store = read_store(&path);
    let removed = store.remove(&project_key(cwd)).is_some();
    if removed {
        write_store(&path, &store)?;
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_with_skill(dir: &Path, body: &str) {
        let skill = dir.join(".claude/skills/demo");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), body).unwrap();
    }

    #[test]
    fn untrusted_project_holds_its_skills_until_trusted() {
        let tmp = std::env::temp_dir().join(format!("nur-skill-trust-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        assert!(is_trusted(&tmp), "no project skills means nothing to hold");
        assert_eq!(pending_count(&tmp), 0);

        project_with_skill(&tmp, "demo v1");
        // The store lives under the real ~/.nur; use a key nobody trusted.
        let store = store_path();
        let key = project_key(&tmp);
        {
            let mut s = read_store(&store);
            s.remove(&key);
            let _ = write_store(&store, &s);
        }
        assert!(!is_trusted(&tmp));
        assert_eq!(pending_count(&tmp), 1);

        assert_eq!(trust(&tmp).unwrap(), 1);
        assert!(is_trusted(&tmp));

        // An edit to a trusted skill holds the set again.
        project_with_skill(&tmp, "demo v2 - edited");
        assert!(!is_trusted(&tmp));

        let _ = std::fs::remove_dir_all(&tmp);
        let mut s = read_store(&store);
        s.remove(&key);
        let _ = write_store(&store, &s);
    }
}

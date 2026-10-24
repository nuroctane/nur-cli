//! The model's scratch space: `<workspace>/.nur/scratch/`.
//!
//! Agents left alone put throwaway files in `/tmp` or `%TEMP%`, which the OS
//! purges on reboot or by age (and on Linux is often RAM-backed), and which
//! search tools do not index, so a session that resumes after a restart has
//! lost its downloads, experiments and one-off scripts. The workspace's
//! `.nur/` folder already holds nur's own artifacts (frames, media,
//! diagrams); scratch lives beside them, inside the sandbox the file tools
//! enforce, and survives anything short of the user deleting it.
//!
//! The folder ignores itself in git (a `.gitignore` of `*`), so nur never
//! edits the project's own ignore files to keep scratch out of commits.

use std::path::{Path, PathBuf};

/// Environment variable the shell exports with the absolute path.
pub const ENV: &str = "NUR_SCRATCH";

pub fn dir(workspace: &Path) -> PathBuf {
    workspace.join(".nur").join("scratch")
}

/// Create the folder and its self-ignoring `.gitignore` if missing. Best
/// effort: a read-only checkout simply has no scratch folder, which the shell
/// and the file tools then report on their own.
pub fn ensure(workspace: &Path) -> Option<PathBuf> {
    let dir = dir(workspace);
    std::fs::create_dir_all(&dir).ok()?;
    let ignore = dir.join(".gitignore");
    if !ignore.exists() {
        std::fs::write(&ignore, "*\n").ok()?;
    }
    Some(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The folder must ignore itself, or the first scratch file the model
    /// writes turns up in `git status` and, sooner or later, in a commit.
    #[test]
    fn scratch_folder_ignores_itself_and_keeps_existing_rules() {
        let root = std::env::temp_dir().join(format!("nur-scratch-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let dir = ensure(&root).expect("scratch created");
        assert_eq!(dir, root.join(".nur").join("scratch"));
        assert_eq!(
            std::fs::read_to_string(dir.join(".gitignore")).unwrap(),
            "*\n"
        );

        // A user who edited the ignore file keeps their version.
        std::fs::write(dir.join(".gitignore"), "*\n!keep.md\n").unwrap();
        ensure(&root).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join(".gitignore")).unwrap(),
            "*\n!keep.md\n"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

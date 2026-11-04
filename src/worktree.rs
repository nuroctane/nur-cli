//! Local Git worktrees for explicitly isolated child writers.
use crate::error::{NurError, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use tokio_util::sync::CancellationToken;

fn git(cwd: &Path, args: &[&str], cancel: &CancellationToken) -> Result<String> {
    let binary = crate::ecosystem::find_bin("git")
        .ok_or_else(|| NurError::Tool("git is required for worktrees".into()))?;
    crate::ecosystem::run_native_tool_capture(&binary, args, Some(cwd), 60_000, cancel)
        .map_err(|e| NurError::Tool(format!("git worktree: {e}")))
}

fn repository(cwd: &Path, cancel: &CancellationToken) -> Result<PathBuf> {
    let root = git(cwd, &["rev-parse", "--show-toplevel"], cancel)?;
    std::fs::canonicalize(root.trim())
        .map_err(|e| NurError::Tool(format!("worktree repository: {e}")))
}

fn target(root: &Path, name: &str) -> Result<PathBuf> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(NurError::Tool("worktree name must contain only letters, digits, '-' or '_' and be at most 64 characters".into()));
    }
    let hash = Sha256::digest(root.to_string_lossy().as_bytes());
    let key: String = hash
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(crate::config::nur_home()
        .join("worktrees")
        .join(key)
        .join(name))
}

fn managed_parent(path: &Path, create: bool) -> Result<PathBuf> {
    let home = std::fs::canonicalize(crate::config::nur_home())
        .map_err(|e| NurError::Tool(format!("worktree home: {e}")))?;
    let base = home.join("worktrees");
    if create {
        std::fs::create_dir_all(&base)
            .map_err(|e| NurError::Tool(format!("create worktree directory: {e}")))?;
    }
    let base = std::fs::canonicalize(&base)
        .map_err(|e| NurError::Tool(format!("worktree directory: {e}")))?;
    if !base.starts_with(&home) {
        return Err(NurError::Tool(
            "worktree directory escapes Nur's home".into(),
        ));
    }
    let key = path
        .parent()
        .and_then(Path::file_name)
        .ok_or_else(|| NurError::Tool("invalid worktree path".into()))?;
    let parent = base.join(key);
    if create {
        std::fs::create_dir_all(&parent)
            .map_err(|e| NurError::Tool(format!("create worktree directory: {e}")))?;
    }
    let parent = std::fs::canonicalize(&parent)
        .map_err(|e| NurError::Tool(format!("worktree directory: {e}")))?;
    if !parent.starts_with(&base) {
        return Err(NurError::Tool(
            "worktree directory escapes its managed root".into(),
        ));
    }
    Ok(parent)
}

pub fn list(cwd: &Path, cancel: &CancellationToken) -> Result<String> {
    git(cwd, &["worktree", "list", "--porcelain"], cancel)
}

pub fn create(cwd: &Path, name: &str, cancel: &CancellationToken) -> Result<String> {
    let root = repository(cwd, cancel)?;
    let path = target(&root, name)?;
    if std::fs::symlink_metadata(&path).is_ok() {
        return Err(NurError::Tool(format!("worktree {name} already exists")));
    }
    let _parent = managed_parent(&path, true)?;
    let branch = format!(
        "nur/{name}-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..12]
    );
    let path_arg = path.to_string_lossy();
    git(
        &root,
        &["worktree", "add", "-b", &branch, &path_arg, "HEAD"],
        cancel,
    )?;
    Ok(format!("worktree {name}\npath: {}\nbranch: {branch}\nBased on committed HEAD; uncommitted parent changes are not copied.", path.display()))
}

pub fn resolve(cwd: &Path, name: &str, cancel: &CancellationToken) -> Result<PathBuf> {
    let root = repository(cwd, cancel)?;
    let path = target(&root, name)?;
    let canonical_parent = managed_parent(&path, false)?;
    let canonical = std::fs::canonicalize(&path)
        .map_err(|_| NurError::Tool(format!("worktree {name} does not exist")))?;
    if !canonical.starts_with(&canonical_parent) {
        return Err(NurError::Tool(
            "worktree path escapes its managed directory".into(),
        ));
    }
    let listing = list(&root, cancel)?;
    let registered = listing
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .filter_map(|entry| std::fs::canonicalize(entry).ok())
        .any(|entry| entry == canonical);
    if !registered {
        return Err(NurError::Tool(format!(
            "worktree {name} is not registered to this repository"
        )));
    }
    Ok(canonical)
}

/// An OS lease prevents two live child writers from sharing one worktree.
#[derive(Debug)]
pub struct Lease {
    pub path: PathBuf,
    _file: std::fs::File,
}

fn session_leases() -> &'static Mutex<std::collections::HashMap<String, Arc<Lease>>> {
    static LEASES: OnceLock<Mutex<std::collections::HashMap<String, Arc<Lease>>>> = OnceLock::new();
    LEASES.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

pub struct SessionLease {
    session_id: String,
}

impl Drop for SessionLease {
    fn drop(&mut self) {
        if let Ok(mut leases) = session_leases().lock() {
            leases.remove(&self.session_id);
        }
    }
}

pub fn attach(session_id: &str, lease: Option<Arc<Lease>>) -> Result<Option<SessionLease>> {
    let Some(lease) = lease else { return Ok(None) };
    session_leases()
        .lock()
        .map_err(|_| NurError::Tool("worktree lease registry unavailable".into()))?
        .insert(session_id.to_owned(), lease);
    Ok(Some(SessionLease {
        session_id: session_id.to_owned(),
    }))
}

pub fn for_session(session_id: &str) -> Result<Option<Arc<Lease>>> {
    Ok(session_leases()
        .lock()
        .map_err(|_| NurError::Tool("worktree lease registry unavailable".into()))?
        .get(session_id)
        .cloned())
}

pub fn claim(cwd: &Path, name: &str, cancel: &CancellationToken) -> Result<Lease> {
    let path = resolve(cwd, name, cancel)?;
    let parent = path
        .parent()
        .ok_or_else(|| NurError::Tool("invalid worktree path".into()))?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(parent.join(format!(".nur-{name}.lock")))
        .map_err(|e| NurError::Tool(format!("open worktree lease: {e}")))?;
    file.try_lock()
        .map_err(|_| NurError::Tool(format!("worktree is already in use: {name}")))?;
    Ok(Lease { path, _file: file })
}

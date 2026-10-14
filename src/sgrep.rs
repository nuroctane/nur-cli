//! sgrep - local semantic code search from Context.dev
//! (<https://github.com/context-dot-dev/sgrep>, MIT). The engine behind the
//! `sgrep` tool: plain-English queries over the workspace, ranked by static
//! embeddings + BM25 with exact identifiers first, and a piped mode that
//! reranks a focused ripgrep run by intent. Everything runs on the CPU; the
//! only network traffic is a one-time 32-34 MB model download.
//!
//! This module owns discovery, installation state and the model warm-up.
//! Search itself lives in `tools::sgrep`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Upstream repository used for `cargo install --git`.
pub const SGREP_GIT: &str = "https://github.com/context-dot-dev/sgrep";
/// Human install line, shown when the binary is missing.
pub const INSTALL_HINT: &str =
    "cargo install --git https://github.com/context-dot-dev/sgrep --locked  (or: nur ecosystem ensure)";
/// Environment override for the binary location.
pub const BIN_ENV: &str = "NUR_SGREP_BIN";

/// Resolve the `sgrep` binary. `NUR_SGREP_BIN` wins (an empty value or a path
/// that does not exist disables sgrep, which the E2E suite uses to pin the
/// fallback path); otherwise the usual fixed dirs, PATH and `~/.cargo/bin`.
pub fn sgrep_bin() -> Option<String> {
    if let Ok(v) = std::env::var(BIN_ENV) {
        let v = v.trim();
        if v.is_empty() {
            return None;
        }
        return Path::new(v).is_file().then(|| v.to_string());
    }
    crate::ecosystem::find_bin("sgrep")
}

/// Directory to prepend to the child's PATH so sgrep can spawn `rg`, when nur
/// found ripgrep somewhere PATH does not cover.
pub fn rg_path_dir() -> Option<PathBuf> {
    crate::tools::find_rg().and_then(|p| p.parent().map(Path::to_path_buf))
}

/// Model cache sgrep writes to (HF hub layout). Used to decide whether a
/// warm-up download is still ahead.
fn model_cache_dir() -> Option<PathBuf> {
    if let Ok(dir) =
        std::env::var("HF_HUB_CACHE").or_else(|_| std::env::var("HUGGINGFACE_HUB_CACHE"))
    {
        return Some(PathBuf::from(dir));
    }
    if let Ok(home) = std::env::var("HF_HOME") {
        return Some(PathBuf::from(home).join("hub"));
    }
    dirs::home_dir().map(|h| h.join(".cache").join("huggingface").join("hub"))
}

/// True once the code model is on disk, so a first query will not download.
pub fn model_warm() -> bool {
    model_cache_dir().is_some_and(|d| d.join("models--minishlab--potion-code-16M-v2").is_dir())
}

/// One-shot marker: cargo builds are slow, so a failing install is retried at
/// most once per 7 days instead of on every ensure.
pub fn install_attempted() -> bool {
    let marker = crate::config::nur_home()
        .join("cache")
        .join("sgrep-install-attempted");
    match fs::metadata(&marker) {
        Ok(m) => m
            .modified()
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_none_or(|age| age.as_secs() < 7 * 24 * 3600),
        Err(_) => false,
    }
}

pub fn mark_install_attempted() {
    let dir = crate::config::nur_home().join("cache");
    let _ = fs::create_dir_all(&dir);
    let _ = fs::write(dir.join("sgrep-install-attempted"), "1");
}

/// Download the model and parsers ahead of the first real query by searching
/// a one-file scratch corpus. Bounded; failure only means the first query
/// pays the download instead.
pub fn warm_model(bin: &str) -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!("nur-sgrep-warm-{}", std::process::id()));
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::write(
        dir.join("warm.rs"),
        "fn retry_with_backoff() { /* warm-up corpus */ }\n",
    )
    .map_err(|e| e.to_string())?;
    let mut cmd = Command::new(bin);
    cmd.args([
        "retry with exponential backoff",
        ".",
        "-n",
        "1",
        "--no-cache",
        "--json",
    ])
    .current_dir(&dir)
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::piped());
    if let Some(rg_dir) = rg_path_dir() {
        prepend_path(&mut cmd, &rg_dir);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(240);
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_dir_all(&dir);
                return Err("warm-up timed out (model download will happen on first query)".into());
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(200)),
            Err(e) => {
                let _ = fs::remove_dir_all(&dir);
                return Err(e.to_string());
            }
        }
    };
    let _ = fs::remove_dir_all(&dir);
    // Exit 1 is "no results", which is still a successful warm-up.
    if matches!(status.code(), Some(0 | 1)) {
        Ok(())
    } else {
        Err(format!("sgrep exited with {status}"))
    }
}

/// Prepend `dir` to the child's PATH, keeping the inherited value.
pub fn prepend_path(cmd: &mut Command, dir: &Path) {
    let sep = if cfg!(windows) { ";" } else { ":" };
    let inherited = std::env::var("PATH").unwrap_or_default();
    cmd.env("PATH", format!("{}{sep}{inherited}", dir.display()));
}

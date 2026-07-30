//! egaki - terminal image & video generation CLI.
//! Upstream: https://github.com/remorses/egaki
//! Supports `egaki login --provider chatgpt` (ChatGPT sub path).

use crate::ecosystem::{find_bin, run_capture, run_capture_cancelled};
use std::fs;
use std::path::{Path, PathBuf};

pub fn find_egaki() -> Option<String> {
    find_bin("egaki")
}

pub fn doctor_report() -> String {
    let mut lines = Vec::new();
    match find_egaki() {
        Some(p) => {
            lines.push(format!("egaki: {p}"));
            if let Some(v) = run_capture(&p, &["--version"], None, 10_000).ok() {
                lines.push(format!("version: {}", v.lines().next().unwrap_or(&v)));
            }
        }
        None => lines.push(
            "egaki: missing - npm i -g egaki  (or pnpm add -g egaki), then ecosystem ensure"
                .into(),
        ),
    }
    lines.push(
        "Auth: egaki login | egaki login --provider chatgpt  (ChatGPT subscription path)".into(),
    );
    lines.push("Outputs default to <workspace>/.nur/media/".into());
    lines.join("\n")
}

pub fn media_dir(cwd: &Path) -> PathBuf {
    let d = cwd.join(".nur").join("media");
    let _ = fs::create_dir_all(&d);
    d
}

#[allow(dead_code)]
pub fn run_egaki(args: &[&str], cwd: Option<&Path>, timeout_ms: u64) -> Result<String, String> {
    let bin = find_egaki().ok_or_else(|| {
        "egaki not on PATH - npm i -g egaki (supports ChatGPT sub via: egaki login --provider chatgpt)"
            .to_string()
    })?;
    run_capture(&bin, args, cwd, timeout_ms)
}

pub fn run_egaki_cancelled(
    args: &[&str],
    cwd: Option<&Path>,
    timeout_ms: u64,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<String, String> {
    let bin = find_egaki().ok_or_else(|| {
        "egaki not on PATH - npm i -g egaki (supports ChatGPT sub via: egaki login --provider chatgpt)"
            .to_string()
    })?;
    run_capture_cancelled(&bin, args, cwd, timeout_ms, cancel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;

    #[test]
    fn media_dir_creates() {
        let t = temp_dir().join(format!("nur-egaki-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&t);
        let m = media_dir(&t);
        assert!(m.is_dir());
        let _ = fs::remove_dir_all(&t);
    }
}

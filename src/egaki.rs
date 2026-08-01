//! egaki - terminal image, video, and speech generation CLI.
//! Upstream: <https://github.com/remorses/egaki> (npm `egaki` ≥ 0.10).
//!
//! Auth modes (from upstream docs):
//! - BYOK keys via `egaki login --provider <name> --key …`
//! - ChatGPT sub: `egaki login --provider chatgpt` (device auth)
//! - xAI Grok Build sub: `egaki login --provider xai-oauth`
//! - Egaki subscription: `egaki subscribe` then `egaki login --provider egaki --key egaki_…`
//!
//! Credentials: `~/.config/egaki/credentials.json` (+ env vars).

use crate::ecosystem::{find_bin, run_capture, run_capture_cancelled};
use std::fs;
use std::path::{Path, PathBuf};

pub fn find_egaki() -> Option<String> {
    find_bin("egaki")
}

pub fn credentials_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("egaki")
        .join("credentials.json")
}

/// Parse `egaki login --show` for configured / signed-in providers (no secrets).
pub fn login_show_summary() -> Option<String> {
    let bin = find_egaki()?;
    let text = run_capture(&bin, &["login", "--show"], None, 15_000).ok()?;
    Some(text.trim().to_string())
}

pub fn doctor_report() -> String {
    let mut lines = Vec::new();
    match find_egaki() {
        Some(p) => {
            lines.push(format!("egaki: {p}"));
            if let Ok(v) = run_capture(&p, &["--version"], None, 10_000) {
                lines.push(format!(
                    "version: {}",
                    v.lines().next().unwrap_or(&v).trim()
                ));
            }
        }
        None => lines.push(
            "egaki: missing - npm i -g egaki@latest (or pnpm add -g egaki), then ecosystem ensure"
                .into(),
        ),
    }
    let creds = credentials_path();
    if creds.is_file() {
        lines.push(format!("credentials: {}", creds.display()));
    } else {
        lines.push(format!(
            "credentials: not found ({}) - run egaki login",
            creds.display()
        ));
    }
    if let Some(show) = login_show_summary() {
        lines.push(String::new());
        lines.push(show);
    } else {
        lines.push(
            "Auth: egaki login | egaki login --provider chatgpt | --provider xai-oauth | \
             --provider egaki --key egaki_…"
                .into(),
        );
    }
    lines.push(String::new());
    lines.push("Outputs default to <workspace>/.nur/media/".into());
    lines.push("Commands: image · video · speech · transcribe · models · usage · subscribe".into());
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
        "egaki not on PATH - npm i -g egaki@latest (ChatGPT: egaki login --provider chatgpt)"
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
        "egaki not on PATH - npm i -g egaki@latest (ChatGPT: egaki login --provider chatgpt)"
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

    #[test]
    fn credentials_path_under_config_egaki() {
        let p = credentials_path();
        assert!(p.ends_with(Path::new("egaki").join("credentials.json")));
    }
}

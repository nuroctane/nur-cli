//! Headroom - local context compression for tool results.
//!
//! Upstream: <https://github.com/headroomlabs-ai/headroom> (`headroom-ai` on PyPI).
//! Default mode is **inline** compress on tool outputs (config `headroom.enabled`).
//! Uses a small Python helper that calls `from headroom import compress`.

use crate::config::{nur_home, HeadroomConfig};
use crate::ecosystem::{find_bin, run_capture};
use crate::tools::sensitive::body_looks_sensitive;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const COMPRESS_TIMEOUT_MS: u64 = 8_000;
const HELPER_NAME: &str = "headroom_compress.py";
const IMPORT_PROBE_TTL: Duration = Duration::from_secs(60);

/// Tools whose results must never go through Headroom (media / self / binary-ish).
const SKIP_TOOLS: &[&str] = &[
    "look",
    "extract_frames",
    "headroom",
    "egaki",
    "tldraw",
    "excalidraw",
];

static IMPORT_CACHE: Mutex<Option<(Instant, bool)>> = Mutex::new(None);

pub fn find_headroom_bin() -> Option<String> {
    find_bin("headroom")
}

pub fn find_python() -> Option<String> {
    find_bin("python3")
        .or_else(|| find_bin("python"))
        .or_else(|| find_bin("py"))
}

fn with_py_launcher(py: &str, cmd: &mut Command) {
    if Path::new(py)
        .file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("py"))
    {
        cmd.arg("-3");
    }
}

/// True when `from headroom import compress` works (enough for inline mode).
/// Result is cached briefly so doctor / ensure / hot paths do not stall repeatedly.
pub fn python_import_ok() -> bool {
    if let Ok(guard) = IMPORT_CACHE.lock() {
        if let Some((at, ok)) = *guard {
            if at.elapsed() < IMPORT_PROBE_TTL {
                return ok;
            }
        }
    }
    let ok = python_import_ok_uncached();
    if let Ok(mut guard) = IMPORT_CACHE.lock() {
        *guard = Some((Instant::now(), ok));
    }
    ok
}

fn python_import_ok_uncached() -> bool {
    let Some(py) = find_python() else {
        return false;
    };
    let mut cmd = Command::new(&py);
    with_py_launcher(&py, &mut cmd);
    cmd.args(["-c", "from headroom import compress"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Bound the probe: a hung import must not stall doctor forever.
    let Ok(mut child) = cmd.spawn() else {
        return false;
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return st.success(),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => return false,
        }
    }
}

/// Ensure the compress helper exists under `~/.nur/bin/`.
pub fn ensure_helper_script() -> PathBuf {
    let dir = nur_home().join("bin");
    let _ = std::fs::create_dir_all(&dir);
    let dest = dir.join(HELPER_NAME);
    // Refuse to follow a pre-existing symlink at the helper path.
    if dest
        .symlink_metadata()
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        return dest;
    }
    let body = include_str!("../scripts/headroom_compress.py");
    let tmp = dir.join(format!(".{HELPER_NAME}.{}.tmp", std::process::id()));
    if std::fs::write(&tmp, body).is_ok() {
        let _ = std::fs::rename(&tmp, &dest).or_else(|_| {
            let _ = std::fs::copy(&tmp, &dest);
            std::fs::remove_file(&tmp)
        });
    }
    dest
}

pub fn probe_version() -> Option<String> {
    let bin = find_headroom_bin()?;
    run_capture(&bin, &["--version"], None, 10_000)
        .ok()
        .map(|s| s.lines().next().unwrap_or(&s).trim().to_string())
}

pub fn doctor_report() -> String {
    let mut lines = Vec::new();
    match find_headroom_bin() {
        Some(p) => {
            lines.push(format!("headroom CLI: {p}"));
            if let Some(v) = probe_version() {
                lines.push(format!("version: {v}"));
            }
        }
        None => lines.push(
            "headroom CLI: missing (optional - inline compress uses the Python package)".into(),
        ),
    }
    match find_python() {
        Some(p) => lines.push(format!("python: {p}")),
        None => lines.push("python: missing (needed for inline compress helper)".into()),
    }
    lines.push(format!(
        "python import headroom: {}",
        if python_import_ok() { "ok" } else { "missing" }
    ));
    let helper = ensure_helper_script();
    lines.push(format!("helper: {}", helper.display()));
    lines.push(
        "mode: inline compress on tool results when [headroom] enabled=true (default)".into(),
    );
    lines.join("\n")
}

fn looks_secret(body: &str) -> bool {
    body_looks_sensitive(body)
}

pub fn should_compress(cfg: &HeadroomConfig, tool: &str, body: &str) -> bool {
    if !cfg.enabled {
        return false;
    }
    let mode = cfg.mode.trim().to_ascii_lowercase();
    if mode == "off" || mode == "proxy" {
        return false;
    }
    let tool_l = tool.trim().to_ascii_lowercase();
    if SKIP_TOOLS.iter().any(|t| *t == tool_l) {
        return false;
    }
    if body.chars().count() < cfg.min_chars as usize {
        return false;
    }
    if looks_secret(body) {
        return false;
    }
    true
}

fn resolve_model(cfg: &HeadroomConfig, session_model: &str) -> String {
    let cfg_m = cfg.model.trim();
    if !cfg_m.is_empty() {
        return cfg_m.to_string();
    }
    let sm = session_model.trim();
    if !sm.is_empty() {
        return sm.to_string();
    }
    "gpt-4o".into()
}

/// Compress `body` when Headroom is available; otherwise return `None`.
///
/// Uses a temp file + `run_capture` so stdout/stderr are drained under a real
/// timeout (avoids pipe-full deadlocks from the old try_wait loop).
pub fn compress_text(
    cfg: &HeadroomConfig,
    tool: &str,
    body: &str,
    session_model: &str,
) -> Option<String> {
    if !should_compress(cfg, tool, body) {
        return None;
    }
    let py = find_python()?;
    let helper = ensure_helper_script();
    if !helper.is_file() {
        return None;
    }

    let tmp_dir = nur_home().join("cache").join("headroom");
    let _ = std::fs::create_dir_all(&tmp_dir);
    let tmp = tmp_dir.join(format!(
        "in-{}-{}.txt",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ));
    if std::fs::write(&tmp, body.as_bytes()).is_err() {
        return None;
    }

    let model = resolve_model(cfg, session_model);
    let helper_s = helper.to_string_lossy().into_owned();
    let tmp_s = tmp.to_string_lossy().into_owned();
    let mut argv: Vec<String> = Vec::new();
    if Path::new(&py)
        .file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("py"))
    {
        argv.push("-3".into());
    }
    argv.push(helper_s);
    argv.push("--model".into());
    argv.push(model);
    argv.push("--label".into());
    argv.push(tool.to_string());
    argv.push("--file".into());
    argv.push(tmp_s);
    let refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
    let captured = run_capture(&py, &refs, None, COMPRESS_TIMEOUT_MS);
    let _ = std::fs::remove_file(&tmp);

    let out = match captured {
        Ok(s) => s,
        Err(_) => return None,
    };
    let trimmed = out.trim().to_string();
    if trimmed.is_empty() || trimmed.len() >= body.len() {
        return None;
    }
    let saved = body.len().saturating_sub(trimmed.len());
    let header = format!(
        "[headroom compressed - {} -> {} chars, ~{saved} chars saved; \
         original was not spilled - re-run the tool if you need the full body]\n",
        body.chars().count(),
        trimmed.chars().count()
    );
    Some(format!("{header}{trimmed}"))
}

/// Compress then spill - shared by all agent-loop tool-result sites.
pub fn prepare_tool_body(
    headroom: &HeadroomConfig,
    session_id: &str,
    tool: &str,
    body: String,
    ok: bool,
    spill_max_chars: usize,
    session_model: &str,
) -> String {
    let body = if ok {
        match compress_text(headroom, tool, &body, session_model) {
            Some(c) => c,
            None => body,
        }
    } else {
        body
    };
    crate::tools::spill::maybe_spill(session_id, tool, body, spill_max_chars)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HeadroomConfig;

    #[test]
    fn disabled_skips() {
        let cfg = HeadroomConfig {
            enabled: false,
            ..Default::default()
        };
        assert!(!should_compress(&cfg, "bash", &"x".repeat(5000)));
    }

    #[test]
    fn small_body_skips() {
        let cfg = HeadroomConfig::default();
        assert!(!should_compress(&cfg, "bash", "tiny"));
    }

    #[test]
    fn skip_look() {
        let cfg = HeadroomConfig::default();
        assert!(!should_compress(&cfg, "look", &"x".repeat(5000)));
    }

    #[test]
    fn default_enabled() {
        assert!(HeadroomConfig::default().enabled);
        assert_eq!(HeadroomConfig::default().mode, "inline");
    }

    #[test]
    fn secrets_skip_api_key() {
        let cfg = HeadroomConfig::default();
        let body = format!("api_key=sk-{}", "x".repeat(3000));
        assert!(!should_compress(&cfg, "bash", &body));
    }

    #[test]
    fn secrets_skip_bearer() {
        let cfg = HeadroomConfig::default();
        let body = format!(
            "Authorization: Bearer {}\n{}",
            "a".repeat(40),
            "x".repeat(3000)
        );
        assert!(!should_compress(&cfg, "bash", &body));
    }

    #[test]
    fn secrets_skip_pem() {
        let cfg = HeadroomConfig::default();
        let body = format!("-----BEGIN PRIVATE KEY-----\n{}\n", "x".repeat(3000));
        assert!(!should_compress(&cfg, "bash", &body));
    }

    #[test]
    fn resolve_prefers_cfg_model() {
        let cfg = HeadroomConfig {
            model: "claude-opus".into(),
            ..Default::default()
        };
        assert_eq!(resolve_model(&cfg, "gpt-4o"), "claude-opus");
    }
}

//! Managed local models (ported from wizard's Local tier).
//!
//! `nur local up` bundles llama.cpp the way wizard does — fetching a prebuilt
//! `llama-server` for this platform on demand — downloads a GGUF sized to the
//! machine's RAM, starts the server on `127.0.0.1:8080`, and points the existing
//! `llamacpp` provider at it. No API key needed.
//!
//! The pure decision helpers (tier sizing, model registry, release-asset
//! picking, server args, state) are unit-tested; the download/launch path is
//! best-effort and platform-dependent.

use crate::cli::LocalCmd;
use crate::config::muse_home;
use crate::error::{MuseError, Result};
use crate::theme;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

/// The local server's fixed endpoint (matches the `llamacpp` catalog provider).
pub const LOCAL_PORT: u16 = 8080;

/// A built-in model tier: a curated GGUF and the RAM it wants.
pub struct Tier {
    pub name: &'static str,
    pub label: &'static str,
    pub url: &'static str,
    pub file: &'static str,
    /// Minimum system RAM (GB) we'd pick this tier for.
    pub min_ram_gb: u64,
}

/// Curated tiers (official Qwen2.5 GGUF repos — long-lived, no key). Override
/// any of these by passing a direct `.gguf` URL to `nur local up <url>`.
pub const TIERS: &[Tier] = &[
    Tier {
        name: "small",
        label: "Qwen2.5-3B-Instruct Q4_K_M (~2 GB)",
        url: "https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/resolve/main/qwen2.5-3b-instruct-q4_k_m.gguf",
        file: "qwen2.5-3b-instruct-q4_k_m.gguf",
        min_ram_gb: 0,
    },
    Tier {
        name: "medium",
        label: "Qwen2.5-7B-Instruct Q4_K_M (~4.7 GB)",
        url: "https://huggingface.co/Qwen/Qwen2.5-7B-Instruct-GGUF/resolve/main/qwen2.5-7b-instruct-q4_k_m.gguf",
        file: "qwen2.5-7b-instruct-q4_k_m.gguf",
        min_ram_gb: 16,
    },
    Tier {
        name: "large",
        label: "Qwen2.5-14B-Instruct Q4_K_M (~9 GB)",
        url: "https://huggingface.co/Qwen/Qwen2.5-14B-Instruct-GGUF/resolve/main/qwen2.5-14b-instruct-q4_k_m.gguf",
        file: "qwen2.5-14b-instruct-q4_k_m.gguf",
        min_ram_gb: 32,
    },
];

/// Recorded state of the managed server (`~/.nur/local/server.json`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServerState {
    pub pid: u32,
    pub port: u16,
    pub model_file: String,
}

// ── pure helpers (unit-tested) ───────────────────────────────────────────

/// Pick a tier for `ram_gb`: the largest tier whose `min_ram_gb` fits, but never
/// below `small`.
pub fn tier_for_ram(ram_gb: u64) -> &'static Tier {
    let mut chosen = &TIERS[0];
    for t in TIERS {
        if ram_gb >= t.min_ram_gb {
            chosen = t;
        }
    }
    chosen
}

/// Resolve a user selector (`None` → size to RAM · a tier name · a direct URL)
/// into `(url, filename)`.
pub fn resolve_selector(selector: Option<&str>, ram_gb: u64) -> (String, String) {
    match selector {
        None => {
            let t = tier_for_ram(ram_gb);
            (t.url.to_string(), t.file.to_string())
        }
        Some(s) if s.starts_with("http://") || s.starts_with("https://") => {
            let file = s.rsplit('/').next().unwrap_or("model.gguf").to_string();
            (s.to_string(), file)
        }
        Some(s) => {
            let t = TIERS
                .iter()
                .find(|t| t.name.eq_ignore_ascii_case(s))
                .unwrap_or(&TIERS[0]);
            (t.url.to_string(), t.file.to_string())
        }
    }
}

/// Pick the best llama.cpp release asset for `os`/`arch` from a list of asset
/// names. Prefers a CPU build (portable — no CUDA/Vulkan runtime needed).
pub fn pick_release_asset(assets: &[String], os: &str, arch: &str) -> Option<String> {
    let (os_tag, arch_tag): (&[&str], &[&str]) = match os {
        "windows" => (&["win"], &["x64", "amd64"]),
        "macos" => (
            &["macos", "osx"],
            if arch == "aarch64" {
                &["arm64"]
            } else {
                &["x64"]
            },
        ),
        _ => (&["ubuntu", "linux"], &["x64", "amd64"]),
    };
    let matches = |name: &str| -> bool {
        let n = name.to_ascii_lowercase();
        n.ends_with(".zip")
            && os_tag.iter().any(|t| n.contains(t))
            && arch_tag.iter().any(|t| n.contains(t))
    };
    // Prefer a plain CPU build, else any matching bin asset.
    assets
        .iter()
        .find(|a| matches(a) && a.to_ascii_lowercase().contains("cpu"))
        .or_else(|| {
            assets
                .iter()
                .find(|a| matches(a) && a.to_ascii_lowercase().contains("bin"))
        })
        .or_else(|| assets.iter().find(|a| matches(a)))
        .cloned()
}

/// Args for launching llama-server against `gguf` on the fixed local port.
pub fn server_args(gguf: &Path) -> Vec<String> {
    vec![
        "-m".into(),
        gguf.display().to_string(),
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        LOCAL_PORT.to_string(),
        "-c".into(),
        "8192".into(),
    ]
}

pub fn models_dir() -> PathBuf {
    muse_home().join("models")
}
pub fn bin_dir() -> PathBuf {
    muse_home().join("bin")
}
fn state_path() -> PathBuf {
    muse_home().join("local").join("server.json")
}
fn server_exe_name() -> &'static str {
    if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    }
}

// ── runtime (best-effort) ────────────────────────────────────────────────

/// Best-effort total system RAM in GB (shells out per-OS; 8 GB fallback).
fn detect_ram_gb() -> u64 {
    #[cfg(windows)]
    let out = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory",
        ])
        .output();
    #[cfg(target_os = "macos")]
    let out = std::process::Command::new("sysctl")
        .args(["-n", "hw.memsize"])
        .output();
    #[cfg(all(unix, not(target_os = "macos")))]
    let out = std::process::Command::new("sh")
        .args(["-c", "awk '/MemTotal/ {print $2 * 1024}' /proc/meminfo"])
        .output();

    if let Ok(o) = out {
        if let Ok(s) = String::from_utf8(o.stdout) {
            if let Ok(bytes) = s.trim().parse::<u64>() {
                let gb = bytes / (1024 * 1024 * 1024);
                if gb > 0 {
                    return gb;
                }
            }
        }
    }
    8
}

pub async fn run_local(action: &LocalCmd) -> Result<()> {
    match action {
        LocalCmd::Models => {
            print!("{}", models_report());
            Ok(())
        }
        LocalCmd::Status => {
            print!("{}", status_report());
            Ok(())
        }
        LocalCmd::Down => {
            println!("{}", stop_report());
            Ok(())
        }
        LocalCmd::Up { model } => run_up(model.as_deref()).await,
    }
}

/// Plain-text list of the built-in tiers. Shared by the CLI and `/local models`.
pub fn models_report() -> String {
    let mut s = String::from("built-in local model tiers:\n");
    for t in TIERS {
        s.push_str(&format!("  {:<7} {}\n", t.name, t.label));
    }
    s.push_str("or: /local up <direct .gguf url>");
    s
}

/// Managed-local status: server binary · downloaded models · running server.
pub fn status_report() -> String {
    let mut s = String::new();
    match find_llama_server() {
        Some(p) => s.push_str(&format!("llama-server: {}\n", p.display())),
        None => s.push_str("llama-server: not installed (/local up fetches it)\n"),
    }
    let models = list_models();
    if models.is_empty() {
        s.push_str("models: none downloaded\n");
    } else {
        s.push_str("models downloaded:\n");
        for m in models {
            s.push_str(&format!("  {m}\n"));
        }
    }
    match load_state() {
        Some(st) if pid_alive(st.pid) => s.push_str(&format!(
            "server up · pid {} · 127.0.0.1:{} · {}",
            st.pid, st.port, st.model_file
        )),
        _ => s.push_str(&format!(
            "server: not running (once up, /login → `llama.cpp (local)` — 127.0.0.1:{LOCAL_PORT})"
        )),
    }
    s
}

/// Stop the managed server; returns a status message. Shared by CLI + `/local down`.
pub fn stop_report() -> String {
    let Some(st) = load_state() else {
        return "no managed server recorded".to_string();
    };
    let msg = if kill_pid(st.pid) {
        format!("stopped llama-server (pid {})", st.pid)
    } else {
        "server was not running".to_string()
    };
    let _ = std::fs::remove_file(state_path());
    msg
}

async fn run_up(selector: Option<&str>) -> Result<()> {
    let ram = detect_ram_gb();
    let (url, file) = resolve_selector(selector, ram);
    theme::print_info(&format!("system RAM ~{ram} GB · model: {file}"));

    // 1. llama.cpp binary (bundle on demand).
    let server = match find_llama_server() {
        Some(p) => p,
        None => {
            theme::print_info("fetching llama.cpp (llama-server) for this platform…");
            ensure_llama_server().await?
        }
    };

    // 2. GGUF weights.
    std::fs::create_dir_all(models_dir())?;
    let gguf = models_dir().join(&file);
    if gguf.exists() {
        theme::print_info(&format!("model present: {}", gguf.display()));
    } else {
        theme::print_info(&format!("downloading {url}"));
        download_to(&url, &gguf).await?;
        theme::print_ok("model downloaded");
    }

    // 3. Launch + record.
    let pid = spawn_server(&server, &gguf)?;
    save_state(&ServerState {
        pid,
        port: LOCAL_PORT,
        model_file: file.clone(),
    })?;
    theme::print_ok(&format!(
        "llama-server up · pid {pid} · 127.0.0.1:{LOCAL_PORT}"
    ));
    theme::print_info(
        "use it: run `nur`, `/login` → select `llama.cpp (local)` (no key), then chat. `nur local down` stops it.",
    );
    Ok(())
}

fn find_llama_server() -> Option<PathBuf> {
    let local = bin_dir().join(server_exe_name());
    if local.exists() {
        return Some(local);
    }
    // On PATH?
    which_on_path(server_exe_name())
}

fn which_on_path(exe: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let cand = dir.join(exe);
        if cand.exists() {
            return Some(cand);
        }
    }
    None
}

fn list_models() -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(models_dir()) {
        for e in rd.flatten() {
            if let Some(name) = e.file_name().to_str() {
                if name.ends_with(".gguf") {
                    out.push(name.to_string());
                }
            }
        }
    }
    out.sort();
    out
}

/// Fetch the latest llama.cpp prebuilt release, extract llama-server into
/// `~/.nur/bin`, and return its path.
async fn ensure_llama_server() -> Result<PathBuf> {
    let http = reqwest::Client::builder()
        .user_agent(format!("nur-cli/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| MuseError::Other(e.to_string()))?;

    let rel: serde_json::Value = http
        .get("https://api.github.com/repos/ggml-org/llama.cpp/releases/latest")
        .send()
        .await
        .map_err(|e| MuseError::Other(format!("could not query llama.cpp releases: {e}")))?
        .json()
        .await
        .map_err(|e| MuseError::Other(format!("could not parse llama.cpp releases: {e}")))?;

    let assets: Vec<(String, String)> = rel
        .get("assets")
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|a| {
                    Some((
                        a.get("name")?.as_str()?.to_string(),
                        a.get("browser_download_url")?.as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();

    let names: Vec<String> = assets.iter().map(|(n, _)| n.clone()).collect();
    let os = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    let arch = std::env::consts::ARCH;
    let pick = pick_release_asset(&names, os, arch).ok_or_else(|| {
        MuseError::Other(
            "no matching llama.cpp release asset for this platform — install llama-server manually and put it on PATH"
                .into(),
        )
    })?;
    let dl_url = assets
        .iter()
        .find(|(n, _)| n == &pick)
        .map(|(_, u)| u.clone())
        .unwrap();

    std::fs::create_dir_all(bin_dir())?;
    let zip = bin_dir().join(&pick);
    theme::print_info(&format!("downloading {pick}"));
    download_to(&dl_url, &zip).await?;
    extract_zip(&zip, &bin_dir())?;
    let _ = std::fs::remove_file(&zip);

    // Locate llama-server anywhere under bin/ and hoist it to a stable path.
    let found = find_file_recursive(&bin_dir(), server_exe_name())
        .ok_or_else(|| MuseError::Other("llama-server not found in the release archive".into()))?;
    let dest = bin_dir().join(server_exe_name());
    if found != dest {
        let _ = std::fs::copy(&found, &dest);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
    }
    Ok(dest)
}

fn extract_zip(zip: &Path, into: &Path) -> Result<()> {
    let z = zip.display().to_string();
    let d = into.display().to_string();
    #[cfg(windows)]
    let status = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!("Expand-Archive -Force -LiteralPath '{z}' -DestinationPath '{d}'"),
        ])
        .status();
    #[cfg(not(windows))]
    let status = std::process::Command::new("unzip")
        .args(["-o", &z, "-d", &d])
        .status()
        .or_else(|_| {
            std::process::Command::new("tar")
                .args(["-xf", &z, "-C", &d])
                .status()
        });

    match status {
        Ok(s) if s.success() => Ok(()),
        _ => Err(MuseError::Other(
            "could not extract the llama.cpp archive (need PowerShell/unzip/tar)".into(),
        )),
    }
}

fn find_file_recursive(root: &Path, name: &str) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().and_then(|n| n.to_str()) == Some(name) {
                return Some(p);
            }
        }
    }
    None
}

/// Stream a URL to a file (async). Follows HF/GitHub redirects.
async fn download_to(url: &str, dest: &Path) -> Result<()> {
    let http = reqwest::Client::builder()
        .user_agent(format!("nur-cli/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(3600))
        .build()
        .map_err(|e| MuseError::Other(e.to_string()))?;
    let resp = http
        .get(url)
        .send()
        .await
        .map_err(|e| MuseError::Other(format!("download failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(MuseError::Other(format!(
            "download failed: HTTP {} for {url}",
            resp.status().as_u16()
        )));
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = dest.with_extension("part");
    let mut f = tokio::fs::File::create(&tmp)
        .await
        .map_err(|e| MuseError::Other(e.to_string()))?;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| MuseError::Other(format!("download interrupted: {e}")))?;
        f.write_all(&chunk)
            .await
            .map_err(|e| MuseError::Other(e.to_string()))?;
    }
    f.flush()
        .await
        .map_err(|e| MuseError::Other(e.to_string()))?;
    drop(f);
    tokio::fs::rename(&tmp, dest)
        .await
        .map_err(|e| MuseError::Other(e.to_string()))?;
    Ok(())
}

fn spawn_server(server: &Path, gguf: &Path) -> Result<u32> {
    let mut cmd = std::process::Command::new(server);
    cmd.args(server_args(gguf))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let child = cmd
        .spawn()
        .map_err(|e| MuseError::Other(format!("could not start llama-server: {e}")))?;
    Ok(child.id())
}

fn save_state(st: &ServerState) -> Result<()> {
    let p = state_path();
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&p, serde_json::to_string_pretty(st).unwrap_or_default())?;
    Ok(())
}

fn load_state() -> Option<ServerState> {
    let s = std::fs::read_to_string(state_path()).ok()?;
    serde_json::from_str(&s).ok()
}

#[cfg(windows)]
fn pid_alive(pid: u32) -> bool {
    // tasklist filter — cheap and dependency-free.
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
        .unwrap_or(false)
}
#[cfg(not(windows))]
fn pid_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
        || std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
}

#[cfg(windows)]
fn kill_pid(pid: u32) -> bool {
    std::process::Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
#[cfg(not(windows))]
fn kill_pid(pid: u32) -> bool {
    std::process::Command::new("kill")
        .arg(pid.to_string())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_for_ram_scales_with_memory() {
        assert_eq!(tier_for_ram(8).name, "small");
        assert_eq!(tier_for_ram(16).name, "medium");
        assert_eq!(tier_for_ram(24).name, "medium");
        assert_eq!(tier_for_ram(64).name, "large");
        assert_eq!(tier_for_ram(0).name, "small");
    }

    #[test]
    fn resolve_selector_handles_ram_tier_and_url() {
        // None → sized to RAM.
        let (_, file) = resolve_selector(None, 8);
        assert_eq!(file, "qwen2.5-3b-instruct-q4_k_m.gguf");
        // Named tier.
        let (_, file) = resolve_selector(Some("large"), 8);
        assert_eq!(file, "qwen2.5-14b-instruct-q4_k_m.gguf");
        // Direct URL → filename derived from the path.
        let (url, file) = resolve_selector(Some("https://x.test/a/custom-model.gguf"), 8);
        assert_eq!(url, "https://x.test/a/custom-model.gguf");
        assert_eq!(file, "custom-model.gguf");
    }

    #[test]
    fn pick_release_asset_prefers_cpu_build_per_platform() {
        let assets: Vec<String> = [
            "llama-b1-bin-win-cuda-x64.zip",
            "llama-b1-bin-win-cpu-x64.zip",
            "llama-b1-bin-ubuntu-x64.zip",
            "llama-b1-bin-macos-arm64.zip",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            pick_release_asset(&assets, "windows", "x86_64").as_deref(),
            Some("llama-b1-bin-win-cpu-x64.zip")
        );
        assert_eq!(
            pick_release_asset(&assets, "macos", "aarch64").as_deref(),
            Some("llama-b1-bin-macos-arm64.zip")
        );
        assert_eq!(
            pick_release_asset(&assets, "linux", "x86_64").as_deref(),
            Some("llama-b1-bin-ubuntu-x64.zip")
        );
    }

    #[test]
    fn pick_release_asset_none_when_no_match() {
        let assets = vec!["llama-b1-bin-macos-arm64.zip".to_string()];
        assert_eq!(pick_release_asset(&assets, "windows", "x86_64"), None);
    }

    #[test]
    fn server_args_target_the_local_port() {
        let args = server_args(Path::new("/models/m.gguf"));
        assert!(args.contains(&"--port".to_string()));
        assert!(args.contains(&LOCAL_PORT.to_string()));
        assert!(args.contains(&"/models/m.gguf".to_string()));
    }

    #[test]
    fn server_state_round_trips() {
        let st = ServerState {
            pid: 42,
            port: 8080,
            model_file: "m.gguf".into(),
        };
        let json = serde_json::to_string(&st).unwrap();
        let back: ServerState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.pid, 42);
        assert_eq!(back.model_file, "m.gguf");
    }
}

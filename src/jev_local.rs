//! Local System One engines: run typed decisions on this machine, no cloud key.
//!
//! nur's TypeSafe layer normally talks to `api.typesafe.ai`, but the contract it
//! speaks - `state` + typed `choice`/`noul`/`score` questions in, probabilities
//! and confidence out - is exactly what three open local decision engines
//! implement:
//!
//! | backend   | engine                                             | device |
//! |-----------|----------------------------------------------------|--------|
//! | `verdict` | openJev-verdict-2.0 (ModernBERT-base + GLiClass)   | CPU / any GPU |
//! | `nimble`  | Bespoke-Nimble-9B (Qwen3.5-9B LoRA)                | NVIDIA CUDA GPU with native BF16 |
//! | `laya`    | Laya Core ML (Core ML + Neural Engine)             | Apple Silicon, macOS 15+ |
//! | `mock`    | deterministic scorer                               | anywhere |
//!
//! `scripts/jev_local_bridge.py` translates between the two schema families and
//! serves the System One HTTP shape on loopback. The script is embedded in the
//! binary and materialized under `~/.nur/jev/` at start time (the same pattern
//! [`crate::headroom`] uses for its helper), so an installed nur does not need
//! the repository next to it.
//!
//! Everything here is deliberately dependency-free: the health probe is a plain
//! `TcpStream` HTTP GET, so it cannot panic by dropping a tokio runtime inside an
//! async caller (see `typesafe::client::off_runtime`), and a missing Python is an
//! instruction, not a crash.

use crate::config::{nur_home, TypesafeConfig};
use crate::error::{NurError, Result};
use crate::typesafe::client;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Where the materialized bridge and its state live.
pub fn home() -> PathBuf {
    nur_home().join("jev")
}

pub fn bridge_script() -> PathBuf {
    home().join("jev_local_bridge.py")
}

pub fn state_path() -> PathBuf {
    home().join("bridge.json")
}

/// What `nur jev start` recorded about the running bridge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeState {
    pub pid: u32,
    pub port: u16,
    pub backend: String,
    pub endpoint: String,
    pub started_at: String,
}

/// Write the embedded bridge script next to the user's other nur helpers.
///
/// Same safety shape as `headroom::ensure_helper_script`: refuse to follow a
/// pre-existing symlink, write to a temp name, then rename.
pub fn ensure_bridge_script() -> Result<PathBuf> {
    let dir = home();
    std::fs::create_dir_all(&dir)
        .map_err(|e| NurError::Other(format!("cannot create {}: {e}", dir.display())))?;
    let dest = bridge_script();
    if dest
        .symlink_metadata()
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(NurError::Other(format!(
            "refusing to overwrite the symlink at {}",
            dest.display()
        )));
    }
    let body = include_str!("../scripts/jev_local_bridge.py");
    let tmp = dir.join(format!(".jev_local_bridge.{}.tmp", std::process::id()));
    std::fs::write(&tmp, body)
        .map_err(|e| NurError::Other(format!("cannot write the bridge: {e}")))?;
    std::fs::rename(&tmp, &dest).or_else(|_| -> Result<()> {
        std::fs::copy(&tmp, &dest).map_err(|e| {
            NurError::Other(format!(
                "cannot place the bridge at {}: {e}",
                dest.display()
            ))
        })?;
        let _ = std::fs::remove_file(&tmp);
        Ok(())
    })?;
    Ok(dest)
}

/// The interpreter the bridge runs under.
///
/// Reuses OptMem's validating probe rather than a plain PATH lookup: `find_bin`
/// happily returns the Windows Store `python3.exe` alias stub, which exists but
/// exits 9009, so every bridge command would fail with "returned no JSON".
pub fn python() -> Option<String> {
    crate::optmem::python_runner_cached()
}

// --------------------------------------------------------------------------- //
// Minimal loopback HTTP (no runtime, no client library)
// --------------------------------------------------------------------------- //
fn get_local(port: u16, path: &str, timeout: Duration) -> Option<serde_json::Value> {
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().ok()?;
    let mut stream = TcpStream::connect_timeout(&addr, timeout).ok()?;
    stream.set_read_timeout(Some(timeout)).ok()?;
    stream.set_write_timeout(Some(timeout)).ok()?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;
    let mut raw = Vec::new();
    let mut buf = [0u8; 4096];
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                raw.extend_from_slice(&buf[..n]);
                // A local JSON body is small; stop as soon as it parses.
                if let Some(body) = http_body(&raw) {
                    if serde_json::from_str::<serde_json::Value>(body).is_ok() {
                        break;
                    }
                }
            }
            Err(_) => break,
        }
    }
    let body = http_body(&raw)?;
    serde_json::from_str(body).ok()
}

fn http_body(raw: &[u8]) -> Option<&str> {
    let text = std::str::from_utf8(raw).ok()?;
    let (_, body) = text.split_once("\r\n\r\n")?;
    Some(body)
}

/// Is a bridge answering on this port, and what is it running?
pub fn probe_port(port: u16) -> Option<serde_json::Value> {
    get_local(port, "/health", Duration::from_millis(800))
}

/// Is something (bridge or not) listening on this loopback port?
fn tcp_busy(port: u16) -> bool {
    let addr: SocketAddr = match format!("127.0.0.1:{port}").parse() {
        Ok(addr) => addr,
        Err(_) => return false,
    };
    TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok()
}

/// A free loopback port to suggest when the requested one is taken.
fn suggest_port() -> Option<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").ok()?;
    listener.local_addr().ok().map(|a| a.port())
}

/// Which backends this machine can actually run, per the bridge's own probe.
pub fn probe_backends() -> Result<serde_json::Value> {
    let script = ensure_bridge_script()?;
    let py = python().ok_or_else(|| {
        NurError::Other(
            "no usable Python 3 interpreter found (the Windows Store python3 alias stub does not \
             count) - install Python 3 to run a local engine"
                .into(),
        )
    })?;
    // Bounded: `Command::output()` waits forever on a hung interpreter, and this
    // runs on the path of a plain `nur jev status`.
    let text = bridge_run(&py, &script, &["--probe"])?;
    serde_json::from_str(&text).map_err(|_| {
        NurError::Other(format!(
            "bridge probe returned no JSON: {}",
            text.lines().next().unwrap_or("").to_string()
        ))
    })
}

/// The bridge is a local process; it should answer well inside this.
const BRIDGE_TIMEOUT_MS: u64 = 30_000;

/// Run the bridge script under a real deadline. `ecosystem::run_capture` drains
/// both pipes and kills the child at the deadline, which `Command::output()`
/// cannot do.
fn bridge_run(py: &str, script: &std::path::Path, args: &[&str]) -> Result<String> {
    let mut owned: Vec<String> = Vec::new();
    if std::path::Path::new(py)
        .file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("py"))
    {
        owned.push("-3".into());
    }
    owned.push(script.to_string_lossy().to_string());
    owned.extend(args.iter().map(|a| (*a).to_string()));
    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    crate::ecosystem::run_capture(py, &refs, None, BRIDGE_TIMEOUT_MS)
        .map_err(|e| NurError::Other(format!("the bridge did not answer in time: {e}")))
}

/// Run the bridge's own mapping selftest (no model needed).
///
/// A failing selftest surfaces as an error carrying the checks that failed.
pub fn selftest() -> Result<String> {
    let script = ensure_bridge_script()?;
    let py =
        python().ok_or_else(|| NurError::Other("no usable Python 3 interpreter found".into()))?;
    bridge_run(&py, &script, &["--selftest"])
}

/// Read the recorded bridge state, if a bridge was started by `nur jev start`.
pub fn state() -> Option<BridgeState> {
    let raw = std::fs::read_to_string(state_path()).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_state(state: &BridgeState) -> Result<()> {
    let dir = home();
    std::fs::create_dir_all(&dir)
        .map_err(|e| NurError::Other(format!("cannot create {}: {e}", dir.display())))?;
    let text = serde_json::to_string_pretty(state)
        .map_err(|e| NurError::Other(format!("cannot serialize bridge state: {e}")))?;
    std::fs::write(state_path(), text)
        .map_err(|e| NurError::Other(format!("cannot write {}: {e}", state_path().display())))
}

/// Start a local engine and wait until it answers.
///
/// `extra` is forwarded to the bridge as-is (`--verdict-model`, `--nimble-dir`,
/// `--laya-model`, `--device`): the script owns those defaults, and a bad value
/// then fails in the foreground with the bridge's own message.
pub fn start(backend: &str, port: u16, extra: &[String]) -> Result<String> {
    if let Some(existing) = probe_port(port) {
        return Ok(format!(
            "a bridge is already answering on port {port}: {}",
            serde_json::to_string(&existing).unwrap_or_default()
        ));
    }
    // The health probe only recognizes a bridge (/health with a JSON body).
    // Anything else holding the port (a test echo server, another app) would
    // fail to bind in the child while this function records a bogus pid and
    // reports "not answering yet". Refuse up front instead.
    if tcp_busy(port) {
        return Err(NurError::Other(format!(
            "port {port} is already in use by another process (and it is not a Jev \
             bridge) - stop it, or start the bridge on a free port: `nur jev start \
             --port {}`",
            suggest_port().unwrap_or(8788)
        )));
    }
    let pid = spawn_detached(backend, port, extra)?;
    let endpoint = format!("http://127.0.0.1:{port}/v1/systemone");
    // Recorded before waiting on purpose: a bridge that is still loading a model
    // (or that fails to start) must still be stoppable, and `nur jev status` has
    // to name it. The record is refreshed below when health answers.
    write_state(&BridgeState {
        pid,
        port,
        backend: backend.to_string(),
        endpoint: endpoint.clone(),
        started_at: chrono::Utc::now().to_rfc3339(),
    })?;

    // Wait for /health: a cold start loads a model (verdict/nimble can take
    // seconds to tens of seconds).
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(health) = probe_port(port) {
            let report = format!(
                "bridge up · pid {pid} · backend {} · {endpoint}",
                health
                    .get("backend")
                    .and_then(|b| b.as_str())
                    .unwrap_or(backend)
            );
            return Ok(report);
        }
        if Instant::now() >= deadline {
            // Keep the pid in the report: the process may still be loading a
            // model, and killing it here would be worse than saying so.
            let py = python().unwrap_or_else(|| "python".into());
            let script = bridge_script();
            return Ok(format!(
                "started pid {pid} on port {port} as backend `{backend}`, but it is not answering \
                 yet (a cold model load can take longer than 30s). Check with `nur jev status`, or \
                 run the bridge in the foreground to see its output:\n  {py} {} --backend {backend} \
                 --port {port}{extra_hint}",
                script.display(),
                extra_hint = if extra.is_empty() {
                    String::new()
                } else {
                    format!(" {}", extra.join(" "))
                }
            ));
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Turn a `py` launcher into the concrete interpreter it would run.
///
/// `py -3 --version`-style launching works, but it adds a process layer: the
/// launcher stays alive as the parent of the interpreter, so the pid we record is
/// not the pid serving the port. Asking the launcher for `sys.executable` once
/// removes that layer. Returns `None` when `py` is not a launcher (or does not
/// answer), and the caller falls back to `py -3`.
fn resolve_interpreter(py: &str) -> Option<String> {
    let stem = std::path::Path::new(py)
        .file_stem()
        .and_then(|s| s.to_str())?;
    if !stem.eq_ignore_ascii_case("py") {
        return None;
    }
    let out = crate::ecosystem::run_capture(
        py,
        &["-3", "-c", "import sys;sys.stdout.write(sys.executable)"],
        None,
        10_000,
    )
    .ok()?;
    let path = out.trim().to_string();
    if path.is_empty() || !std::path::Path::new(&path).is_file() {
        return None;
    }
    Some(path)
}

/// Spawn the bridge as a detached process, materializing the script first.
///
/// Separate from [`start`] so a caller (or a test) can run a throwaway bridge
/// without touching the recorded state in `~/.nur/jev/bridge.json`.
pub fn spawn_detached(backend: &str, port: u16, extra: &[String]) -> Result<u32> {
    let script = ensure_bridge_script()?;
    let py = python().ok_or_else(|| {
        NurError::Other(
            "no usable Python 3 interpreter found - a local engine needs Python 3.11+ (for \
             `nimble`/`laya` the engine's own install steps also apply)"
                .into(),
        )
    })?;
    // A `py` launcher would be the process we record - and it spawns the real
    // interpreter as its *child*, so `state.pid` would name a process whose death
    // leaves the engine running and holding the port (`nur jev stop` then reported
    // "stopped" while the bridge kept answering). Resolve it first.
    let (py, launcher_args) = match resolve_interpreter(&py) {
        Some(real) => (real, Vec::new()),
        None => (py, vec!["-3".to_string()]),
    };
    let mut cmd = std::process::Command::new(&py);
    crate::headroom::with_py_launcher(&py, &mut cmd);
    cmd.args(&launcher_args)
        .arg(&script)
        .args(["--backend", backend, "--port", &port.to_string()])
        .args(extra)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW: the
        // bridge must outlive this command without holding a console.
        cmd.creation_flags(0x0000_0008 | 0x0000_0200 | 0x0800_0000);
    }
    let child = cmd
        .spawn()
        .map_err(|e| NurError::Other(format!("cannot start the bridge: {e}")))?;
    Ok(child.id())
}

/// Kill a bridge process by pid (used by `nur jev stop` and by tests).
pub fn kill_pid(pid: u32) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("kill")
            .arg(pid.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

/// Stop a bridge started by `nur jev start`.
pub fn stop() -> Result<String> {
    let Some(state) = state() else {
        return Ok("no bridge recorded (nothing to stop)".into());
    };
    let alive = probe_port(state.port).is_some();
    kill_pid(state.pid);
    // Killing the recorded pid is not enough on its own: if that process was a
    // launcher (or a parent of the engine), the engine survives holding the port,
    // and reporting "stopped" would be a lie the user only notices later.
    let mut killed_extra: Vec<u32> = Vec::new();
    if alive {
        for pid in pids_on_port(state.port) {
            if pid != 0 && pid != state.pid {
                kill_pid(pid);
                killed_extra.push(pid);
            }
        }
    }
    // Give the OS a moment to release the socket before declaring failure.
    let deadline = Instant::now() + Duration::from_millis(1_500);
    while probe_port(state.port).is_some() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    if probe_port(state.port).is_some() {
        // Keep the record: a stale pid is more useful to the user than no record.
        return Ok(format!(
            "bridge pid {} killed on port {}, but something still answers there and no process could be identified. Check `nur jev status` and stop it by hand (Windows: Task Manager; Unix: `lsof -ti tcp:{}`).",
            state.pid, state.port, state.port
        ));
    }
    let _ = std::fs::remove_file(state_path());
    Ok(format!(
        "bridge pid {} on port {} stopped{}{}",
        state.pid,
        state.port,
        if alive { "" } else { " (it was already gone)" },
        if killed_extra.is_empty() {
            String::new()
        } else {
            format!(
                " - also killed {} engine process(es) it had left behind: {}",
                killed_extra.len(),
                killed_extra
                    .iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    ))
}

/// Pids listening on a loopback port, best effort (empty when the platform tool
/// is missing - the caller then only reports what it can prove).
fn pids_on_port(port: u16) -> Vec<u32> {
    #[cfg(windows)]
    let (program, args): (&str, Vec<String>) = (
        "powershell",
        vec![
            "-NoProfile".into(),
            "-Command".into(),
            format!(
                "(Get-NetTCPConnection -LocalPort {port} -State Listen                  -ErrorAction SilentlyContinue).OwningProcess"
            ),
        ],
    );
    #[cfg(not(windows))]
    let (program, args): (&str, Vec<String>) = ("lsof", {
        let mut a = vec!["-ti".to_string()];
        a.push(format!("tcp:{port}"));
        a.push("-sTCP:LISTEN".to_string());
        a
    });
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let Ok(out) = crate::ecosystem::run_capture(program, &refs, None, 10_000) else {
        return Vec::new();
    };
    out.split_whitespace()
        .filter_map(|t| t.trim().parse::<u32>().ok())
        .filter(|p| *p != 0)
        .collect()
}

/// Point nur's TypeSafe layer at the local bridge.
///
/// Writes `[typesafe] base_url` so the choice survives restarts; a key is not
/// required for loopback, so nothing else has to change.
pub fn use_port(port: u16) -> Result<String> {
    let endpoint = format!("http://127.0.0.1:{port}/v1/systemone");
    // Pointing at a dead port looks identical to a working one from inside the
    // harness: the layer treats loopback as a valid engine and every judgment
    // quietly degrades. Say so up front.
    let live = probe_port(port);
    let mut cfg = crate::config::load_config()?;
    let previous = cfg.typesafe.base_url.clone();
    cfg.typesafe.base_url = endpoint.clone();
    crate::config::save_config(&cfg)?;
    let mut out = format!(
        "typesafe base_url set to {endpoint}\n  previous: {}\n  a loopback endpoint needs no key, \
         so local judgments now run with no credential and nothing leaving the machine.\n  \
         (alternative without touching config.toml: export NUR_JEV_LOCAL_URL={endpoint})",
        if previous.trim().is_empty() {
            "(hosted default)"
        } else {
            previous.as_str()
        }
    );
    match live {
        Some(health) => out.push_str(&format!(
            "\n  bridge: answering ({})",
            health
                .get("backend")
                .and_then(|b| b.as_str())
                .unwrap_or("unknown backend")
        )),
        None => out.push_str(&format!(
            "\n  warning: nothing is answering on port {port} yet - start one with \
             `nur jev start --backend <verdict|nimble|laya|mock>` (or `nur jev start` for the \
             default), otherwise every judgment will fail and the harness will fall back to its \
             own behavior."
        )),
    }
    Ok(out)
}

/// Clear a local base_url, going back to the hosted endpoint.
pub fn use_hosted() -> Result<String> {
    let mut cfg = crate::config::load_config()?;
    cfg.typesafe.base_url = String::new();
    crate::config::save_config(&cfg)?;
    Ok("typesafe base_url cleared - back to the hosted endpoint (a key is required again)".into())
}

/// Is the configured endpoint local, and does it answer?
pub fn status_lines(cfg: &TypesafeConfig) -> Vec<String> {
    let mut lines = Vec::new();
    let effective = client::effective_base_url(cfg);
    let local = client::is_loopback_endpoint(&effective);
    lines.push(format!(
        "endpoint: {effective}{}",
        if local {
            "  (local engine)"
        } else {
            "  (hosted)"
        }
    ));
    if let Some(key) = client::key_provenance(cfg) {
        lines.push(format!("credential: {key}"));
    } else if local {
        lines.push("credential: none needed (loopback)".into());
    } else {
        lines.push("credential: none - hosted judgments are inactive".into());
    }
    if let Some(state) = state() {
        let health = probe_port(state.port);
        lines.push(format!(
            "bridge: pid {} · backend {} · port {} · {}",
            state.pid,
            state.backend,
            state.port,
            match &health {
                Some(h) => {
                    let stats = h.get("stats").cloned().unwrap_or_default();
                    format!(
                        "answering (requests {}, questions {}, errors {})",
                        stats.get("requests").and_then(|v| v.as_u64()).unwrap_or(0),
                        stats.get("questions").and_then(|v| v.as_u64()).unwrap_or(0),
                        stats.get("errors").and_then(|v| v.as_u64()).unwrap_or(0)
                    )
                }
                None => "not answering (started earlier; model load, or it exited)".to_string(),
            }
        ));
    } else {
        lines.push(
            "bridge: none started by nur (`nur jev start --backend <verdict|nimble|laya|mock>`)"
                .into(),
        );
    }
    match probe_backends() {
        Ok(info) => {
            lines.push("backends on this machine:".into());
            if let Some(map) = info.as_object() {
                for (name, entry) in map {
                    let available = entry
                        .get("available")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let reason = entry.get("reason").and_then(|v| v.as_str()).unwrap_or("");
                    let device = entry.get("device").and_then(|v| v.as_str()).unwrap_or("");
                    lines.push(format!(
                        "  {:<8} {:<10} {}",
                        name,
                        if available { "available" } else { "no" },
                        if available { device } else { reason }
                    ));
                }
            }
        }
        Err(e) => lines.push(format!("backends: {e}")),
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_body_splits_headers_from_json() {
        let raw = b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\r\n{\"status\":\"ok\"}";
        assert_eq!(http_body(raw), Some("{\"status\":\"ok\"}"));
        assert_eq!(http_body(b"garbage"), None);
    }

    /// Live: start a throwaway bridge (backend `mock`, its own port), drive a real
    /// `TypesafeClient` round trip through it with NO key, and clean up.
    ///
    /// ```text
    /// cargo test --bin nur jev_local_bridge_answers_keyless -- --ignored --nocapture
    /// ```
    ///
    /// This is the check that the local-engine path works end to end: script
    /// materialization, process spawn, loopback keyless client, and an answer that
    /// resolves onto the caller's own options.
    #[test]
    #[ignore = "live: spawns the local bridge and makes a real HTTP request"]
    fn jev_local_bridge_answers_keyless() {
        use crate::typesafe::client;
        use crate::typesafe::questions::{Answer, Question};

        if python().is_none() {
            eprintln!("no usable Python interpreter - skipping");
            return;
        }
        // A free port: bind then release.
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
            let port = listener.local_addr().unwrap().port();
            drop(listener);
            port
        };
        let pid = spawn_detached("mock", port, &[]).expect("bridge spawns");
        let endpoint = format!("http://127.0.0.1:{port}/v1/systemone");

        let deadline = Instant::now() + Duration::from_secs(20);
        while probe_port(port).is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(200));
        }
        let health = probe_port(port);
        assert!(
            health.is_some(),
            "the bridge did not answer on {port} within 20s"
        );
        println!("bridge health: {}", serde_json::to_string(&health).unwrap());

        let cfg = TypesafeConfig {
            enabled: true,
            api_key: String::new(), // deliberately keyless
            base_url: endpoint.clone(),
            timeout_ms: 10_000,
            retries: 0,
            max_parallel: 1,
            ..TypesafeConfig::default()
        };
        assert!(client::is_loopback_endpoint(&endpoint));
        let availability = client::client(&cfg);
        assert!(availability.is_ready(), "{:?}", availability.reason());
        let client_handle = availability.client().expect("client");
        assert_eq!(
            client_handle.key(),
            client::LOCAL_KEY_PLACEHOLDER,
            "a local engine carries no credential"
        );

        let judge_state = serde_json::json!({
            "goal": "triage a support message",
            "message": "Our payments are down and the outage is ongoing.",
        });
        let questions = vec![
            (
                "urgent".to_string(),
                Question::noul("Does this message report an outage?"),
            ),
            (
                "team".to_string(),
                Question::choice_of(
                    "Which team should handle this?",
                    &["billing".to_string(), "technical".to_string()],
                ),
            ),
            (
                "severity".to_string(),
                Question::score(
                    "How severe is this?",
                    vec!["low".into(), "medium".into(), "high".into()],
                ),
            ),
        ];
        let batch = client_handle
            .ask(&judge_state, &questions)
            .expect("the local bridge answers");
        println!(
            "answers: {} · errors: {:?} · tokens {} in / {} out",
            batch.answers.len(),
            batch.errors,
            batch.input_tokens,
            batch.output_tokens
        );
        assert_eq!(batch.answers.len(), 3, "three typed answers");
        assert!(batch.errors.is_empty(), "{:?}", batch.errors);

        let urgent = batch.get("urgent").and_then(Answer::noul);
        assert!(urgent.is_some(), "noul answered");
        let options = vec!["billing".to_string(), "technical".to_string()];
        let team = batch.get("team").and_then(|a| a.resolve_verbatim(&options));
        assert!(
            team.is_some(),
            "the choice resolved onto the caller's own options: {:?}",
            batch.get("team").and_then(|a| a.choice())
        );
        let severity = batch.get("severity").and_then(Answer::score);
        assert!(
            severity.is_some_and(|s| (0.0..=2.0).contains(&s)),
            "score stays on the level scale: {severity:?}"
        );

        kill_pid(pid);
        // The state file is untouched: this bridge was never recorded.
        assert!(
            state().map(|s| s.port) != Some(port),
            "a throwaway bridge must not overwrite the recorded one"
        );
    }

    #[test]
    fn a_closed_port_is_not_a_bridge() {
        // Bind then drop, so the port is (almost certainly) free.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        assert!(probe_port(port).is_none());
    }

    #[test]
    fn an_occupied_port_is_busy_but_not_a_bridge() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        // A plain listener is not a bridge (no /health JSON)...
        assert!(probe_port(port).is_none());
        // ...but the port is taken, so `start` must refuse rather than
        // record a bogus pid (regression: an echo server once squatted the
        // requested port and start reported "not answering yet").
        assert!(tcp_busy(port));
        assert!(start("mock", port, &[]).is_err());
        drop(listener);
    }
}

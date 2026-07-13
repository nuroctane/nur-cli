//! External agent-ecosystem integrations provisioned by Meta's one-shot install.
//!
//! Core runtime: Graphify · PLUR · Ruflo
//! Skill packs: Emil design · clone-website · cybersecurity · OpenCode catalog
//! Gateways: Executor MCP · skills CLI · AKM
//! Patterns: DCP-style context pruning (native + docs)

use crate::config::muse_home;
use crate::error::{MuseError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod packs;
mod skills;

pub use skills::install_bundled_skills;

const ECOSYSTEM_MARKER: &str = "ecosystem.json";
/// Bump when new packs/tools are added so old markers re-run ensure.
/// Bump when spawn/install logic changes so markers re-run ensure.
const ECOSYSTEM_SCHEMA: u32 = 3;
/// Re-run ensure at most once per this many seconds unless forced.
const ENSURE_TTL_SECS: u64 = 86_400;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ComponentStatus {
    pub name: String,
    pub available: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EcosystemStatus {
    #[serde(default)]
    pub schema: u32,
    pub graphify: ComponentStatus,
    pub plur: ComponentStatus,
    pub ruflo: ComponentStatus,
    #[serde(default)]
    pub skills_cli: ComponentStatus,
    #[serde(default)]
    pub akm: ComponentStatus,
    #[serde(default)]
    pub executor: ComponentStatus,
    pub skills_installed: Vec<String>,
    #[serde(default)]
    pub packs_installed: Vec<String>,
    pub ensured_at: u64,
    pub node_ok: bool,
    pub notes: Vec<String>,
}

impl EcosystemStatus {
    pub fn summary_line(&self) -> String {
        let bit = |ok: bool, name: &str| {
            if ok {
                format!("{name}✓")
            } else {
                format!("{name}✗")
            }
        };
        format!(
            "ecosystem · {}  {}  {}  {}  {}  · packs {}",
            bit(self.graphify.available, "graphify"),
            bit(self.plur.available, "plur"),
            bit(self.ruflo.available, "ruflo"),
            bit(self.executor.available, "executor"),
            bit(self.skills_cli.available, "skills"),
            if self.packs_installed.is_empty() {
                "…".into()
            } else {
                self.packs_installed.join(",")
            }
        )
    }

    pub fn report(&self) -> String {
        let mut s = String::from("Meta ecosystem (auto-provisioned)\n");
        let comps = [
            &self.graphify,
            &self.plur,
            &self.ruflo,
            &self.skills_cli,
            &self.akm,
            &self.executor,
        ];
        for c in comps {
            if c.name.is_empty() {
                continue;
            }
            let mark = if c.available { "✓" } else { "✗" };
            s.push_str(&format!("  {mark} {:10} {}\n", c.name, c.detail));
            if let Some(v) = &c.version {
                s.push_str(&format!("              version {v}\n"));
            }
            if let Some(p) = &c.path {
                s.push_str(&format!("              {p}\n"));
            }
        }
        s.push_str(&format!(
            "  node: {}\n",
            if self.node_ok {
                "ok"
            } else {
                "missing — install Node.js 20+"
            }
        ));
        if !self.skills_installed.is_empty() {
            s.push_str(&format!(
                "  bundled skills: {}\n",
                self.skills_installed.join(", ")
            ));
        }
        if !self.packs_installed.is_empty() {
            s.push_str(&format!(
                "  skill packs: {}\n",
                self.packs_installed.join(", ")
            ));
        }
        for n in &self.notes {
            s.push_str(&format!("  note: {n}\n"));
        }
        s.push_str(
            "\n  slash: /ecosystem /plur /ruflo /graphify /skills\n\
             tools:  graphify plur ruflo executor skill\n\
             packs:  design · clone-website · cybersecurity · opencode catalog · DCP patterns\n",
        );
        s
    }
}

pub fn ruflo_home() -> PathBuf {
    muse_home().join("ruflo")
}

pub fn ruflo_db_path() -> PathBuf {
    ruflo_home().join("memory.db")
}

pub fn marker_path() -> PathBuf {
    muse_home().join(ECOSYSTEM_MARKER)
}

/// Ensure the full Meta ecosystem is installed and initialised.
/// Safe to call on every launch — skips heavy work when the marker is fresh.
pub fn ensure_ecosystem(force: bool) -> EcosystemStatus {
    if !force {
        if let Some(cached) = load_marker_if_fresh() {
            return cached;
        }
    }

    let mut status = EcosystemStatus {
        schema: ECOSYSTEM_SCHEMA,
        ..Default::default()
    };
    status.ensured_at = now_secs();
    status.node_ok = which("node") || which("node.exe");

    // Bundled Meta skills (pure FS).
    match install_bundled_skills() {
        Ok(names) => status.skills_installed = names,
        Err(e) => status.notes.push(format!("bundled skills: {e}")),
    }

    status.graphify = ensure_graphify();
    status.plur = ensure_plur(status.node_ok);
    status.ruflo = ensure_ruflo(status.node_ok);
    status.skills_cli = packs::ensure_skills_cli(status.node_ok);
    status.akm = packs::ensure_akm(status.node_ok);
    status.executor = packs::ensure_executor(status.node_ok);

    // Third-party skill packs (network; markers skip re-download).
    let (packs_ok, pack_notes) = packs::install_skill_packs(&status.skills_cli);
    status.packs_installed = packs_ok;
    status.notes.extend(pack_notes);

    if status.plur.available {
        seed_default_plur_engrams();
    }

    let _ = save_marker(&status);
    status
}

fn load_marker_if_fresh() -> Option<EcosystemStatus> {
    let path = marker_path();
    let text = fs::read_to_string(path).ok()?;
    let st: EcosystemStatus = serde_json::from_str(&text).ok()?;
    if st.schema < ECOSYSTEM_SCHEMA {
        return None;
    }
    let age = now_secs().saturating_sub(st.ensured_at);
    if age < ENSURE_TTL_SECS
        && st.graphify.available
        && st.plur.available
        && st.ruflo.available
        && st.skills_cli.available
    {
        Some(st)
    } else {
        None
    }
}

fn save_marker(st: &EcosystemStatus) -> Result<()> {
    let _ = fs::create_dir_all(muse_home());
    let text = serde_json::to_string_pretty(st).map_err(|e| MuseError::Config(e.to_string()))?;
    fs::write(marker_path(), text)?;
    Ok(())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ── Graphify ──────────────────────────────────────────────────────────────

fn ensure_graphify() -> ComponentStatus {
    let mut c = ComponentStatus {
        name: "graphify".into(),
        ..Default::default()
    };
    if let Some(bin) = find_bin("graphify") {
        c.available = true;
        c.path = Some(bin.clone());
        c.version = cmd_version(&bin, &["--version"]);
        c.detail = "CLI ready".into();
        // Keep skill registered for agent discovery.
        let _ = run_quiet(&bin, &["install", "--platform", "agents"], None, 120_000);
        return c;
    }
    // Try install via uv.
    if which("uv") || which("uv.exe") {
        let _ = run_quiet("uv", &["tool", "install", "graphifyy"], None, 300_000);
        if let Some(bin) = find_bin("graphify") {
            c.available = true;
            c.path = Some(bin.clone());
            c.version = cmd_version(&bin, &["--version"]);
            c.detail = "installed via uv tool install graphifyy".into();
            let _ = run_quiet(&bin, &["install", "--platform", "agents"], None, 120_000);
            return c;
        }
    }
    c.detail = "not found — install: uv tool install graphifyy".into();
    c
}

// ── PLUR ──────────────────────────────────────────────────────────────────

fn ensure_plur(node_ok: bool) -> ComponentStatus {
    let mut c = ComponentStatus {
        name: "plur".into(),
        ..Default::default()
    };
    if !node_ok {
        c.detail = "needs Node.js 18+".into();
        return c;
    }
    if find_bin("plur").is_none() {
        let _ = run_quiet(
            "npm",
            &["install", "-g", "@plur-ai/cli@latest", "@plur-ai/mcp@latest"],
            None,
            600_000,
        );
    }
    if let Some(bin) = find_bin("plur") {
        c.available = true;
        c.path = Some(bin.clone());
        c.version = cmd_version(&bin, &["--version"]);
        // Touch store (status creates ~/.plur if missing).
        let _ = run_quiet(&bin, &["status", "--json"], None, 60_000);
        let home = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".plur");
        c.detail = format!("store {}", home.display());
    } else {
        c.detail = "not found — npm install -g @plur-ai/cli @plur-ai/mcp".into();
    }
    c
}

fn seed_default_plur_engrams() {
    let Some(bin) = find_bin("plur") else { return };
    // Only seed when the store is empty so we never spam duplicates.
    if let Ok(out) = run_capture(&bin, &["status", "--json"], None, 30_000) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&out) {
            let n = v.get("engram_count").and_then(|x| x.as_u64()).unwrap_or(1);
            if n > 0 {
                return;
            }
        }
    }
    let seeds = [
        "When editing code in meta-cli or similar agents, prefer apply_patch / multi_edit over full file rewrites for multi-hunk changes.",
        "Never commit secrets, API keys, or ~/.muse/auth.json. Keys live only in local auth storage.",
        "Prefer graphify query/path/explain over broad grep when graphify-out/graph.json exists for architecture questions.",
        "PLUR engrams are shared memory — learn corrections and preferences so future sessions remember them.",
        "Ruflo memory is vector memory for patterns and trajectories; use it for cross-session swarm knowledge.",
    ];
    for s in seeds {
        let _ = run_quiet(&bin, &["learn", s, "--quiet"], None, 30_000);
    }
}

// ── Ruflo ─────────────────────────────────────────────────────────────────

fn ensure_ruflo(node_ok: bool) -> ComponentStatus {
    let mut c = ComponentStatus {
        name: "ruflo".into(),
        ..Default::default()
    };
    if !node_ok {
        c.detail = "needs Node.js 20+".into();
        return c;
    }
    if find_bin("ruflo").is_none() {
        // Minimal install (omit optional ML extras) for faster first-run.
        let _ = run_quiet(
            "npm",
            &["install", "-g", "ruflo@latest", "--omit=optional"],
            None,
            600_000,
        );
        if find_bin("ruflo").is_none() {
            let _ = run_quiet("npm", &["install", "-g", "ruflo@latest"], None, 600_000);
        }
    }
    let Some(bin) = find_bin("ruflo") else {
        c.detail = "not found — npm install -g ruflo".into();
        return c;
    };
    c.available = true;
    c.path = Some(bin.clone());
    c.version = cmd_version(&bin, &["--version"]);

    let home = ruflo_home();
    let _ = fs::create_dir_all(&home);
    let db = ruflo_db_path();

    // Initialise memory DB once (global under ~/.muse/ruflo — does not pollute projects).
    if !db.is_file() {
        let path_s = db.to_string_lossy().into_owned();
        let _ = run_quiet(
            &bin,
            &["memory", "init", "-p", &path_s, "--verify"],
            Some(&home),
            180_000,
        );
        // Fallback: init in ruflo home (creates .swarm/memory.db there).
        if !db.is_file() {
            let _ = run_quiet(&bin, &["memory", "init"], Some(&home), 180_000);
            // Prefer explicit path if swarm db appeared under home.
            let alt = home.join(".swarm").join("memory.db");
            if alt.is_file() && !db.is_file() {
                let _ = fs::copy(&alt, &db);
            }
        }
    }
    c.detail = format!("memory {}", db.display());
    c
}

// ── Public helpers for tools ──────────────────────────────────────────────

/// Resolve a CLI to an **absolute** path when possible.
///
/// On Windows we never return a bare name like `"npm"` / `"skills"` — those
/// are `.cmd` shims and `std::process::Command` cannot CreateProcess them
/// without going through `cmd /C`. Returning `…\npm.cmd` (or `where`’s path)
/// makes spawns reliable.
pub fn find_bin(name: &str) -> Option<String> {
    let home = dirs::home_dir()?;
    let mut candidates: Vec<PathBuf> = vec![
        home.join(".local").join("bin").join(format!("{name}.exe")),
        home.join(".local").join("bin").join(format!("{name}.cmd")),
        home.join(".local").join("bin").join(name),
        home.join("AppData")
            .join("Roaming")
            .join("npm")
            .join(format!("{name}.cmd")),
        home.join("AppData")
            .join("Roaming")
            .join("npm")
            .join(format!("{name}.exe")),
        home.join("AppData")
            .join("Roaming")
            .join("npm")
            .join(name),
        PathBuf::from(r"C:\Program Files\nodejs").join(format!("{name}.cmd")),
        PathBuf::from(r"C:\Program Files\nodejs").join(format!("{name}.exe")),
        PathBuf::from(r"C:\Program Files\nodejs").join(name),
    ];

    // npm global prefix (use cmd-safe npm resolution to avoid recursion)
    if let Some(npm) = resolve_where("npm") {
        if let Ok(out) = spawn_program(&npm, &["prefix", "-g"]).output() {
            if out.status.success() {
                let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !p.is_empty() {
                    candidates.push(PathBuf::from(&p).join(format!("{name}.cmd")));
                    candidates.push(PathBuf::from(&p).join(format!("{name}.exe")));
                    candidates.push(PathBuf::from(&p).join(name));
                    candidates.push(PathBuf::from(&p).join("bin").join(name));
                }
            }
        }
    }

    if let Some(uv) = resolve_where("uv").or_else(|| find_file_only("uv")) {
        if let Ok(out) = spawn_program(&uv, &["tool", "dir", "--bin"]).output() {
            if out.status.success() {
                let dir = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !dir.is_empty() {
                    candidates.push(PathBuf::from(&dir).join(format!("{name}.exe")));
                    candidates.push(PathBuf::from(&dir).join(format!("{name}.cmd")));
                    candidates.push(PathBuf::from(&dir).join(name));
                }
            }
        }
    }

    for c in &candidates {
        if c.is_file() {
            return Some(c.to_string_lossy().into_owned());
        }
    }

    // `where` / `which` last — returns absolute paths on modern Windows.
    resolve_where(name)
}

fn find_file_only(name: &str) -> Option<String> {
    let home = dirs::home_dir()?;
    for c in [
        home.join(".local").join("bin").join(format!("{name}.exe")),
        home.join(".cargo").join("bin").join(format!("{name}.exe")),
        home.join(".local").join("bin").join(name),
    ] {
        if c.is_file() {
            return Some(c.to_string_lossy().into_owned());
        }
    }
    None
}

/// First absolute path from `where name` (Windows) or `which -a` (Unix).
fn resolve_where(name: &str) -> Option<String> {
    #[cfg(windows)]
    {
        let out = Command::new("where.exe")
            .arg(name)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        // Prefer .cmd / .exe over extensionless shim scripts.
        let mut first: Option<String> = None;
        for line in text.lines() {
            let p = line.trim();
            if p.is_empty() {
                continue;
            }
            let lower = p.to_ascii_lowercase();
            if lower.ends_with(".cmd") || lower.ends_with(".exe") || lower.ends_with(".bat") {
                return Some(p.to_string());
            }
            if first.is_none() {
                first = Some(p.to_string());
            }
        }
        first
    }
    #[cfg(not(windows))]
    {
        let out = Command::new("which").arg(name).output().ok()?;
        if !out.status.success() {
            return None;
        }
        let p = String::from_utf8_lossy(&out.stdout).lines().next()?.trim().to_string();
        if p.is_empty() {
            None
        } else {
            Some(p)
        }
    }
}

pub fn which(name: &str) -> bool {
    find_bin(name).is_some()
}

/// Build a Command that can actually start npm/skills/executor shims on Windows.
fn spawn_program(bin: &str, args: &[&str]) -> Command {
    #[cfg(windows)]
    {
        let lower = bin.to_ascii_lowercase();
        let needs_cmd = lower.ends_with(".cmd")
            || lower.ends_with(".bat")
            || (!bin.contains('\\') && !bin.contains('/'));
        if needs_cmd {
            let mut c = Command::new("cmd.exe");
            c.arg("/D").arg("/C").arg(bin);
            for a in args {
                c.arg(a);
            }
            return c;
        }
    }
    let mut c = Command::new(bin);
    c.args(args);
    c
}

pub fn run_capture(
    bin: &str,
    args: &[&str],
    cwd: Option<&Path>,
    timeout_ms: u64,
) -> std::result::Result<String, String> {
    // Re-resolve bare names to absolute paths (Windows .cmd safety).
    let resolved = if bin.contains('\\')
        || bin.contains('/')
        || bin.ends_with(".cmd")
        || bin.ends_with(".exe")
    {
        bin.to_string()
    } else {
        find_bin(bin).unwrap_or_else(|| bin.to_string())
    };

    let mut cmd = spawn_program(&resolved, args);
    if let Some(c) = cwd {
        cmd.current_dir(c);
    }
    // Capture output manually to enforce timeout
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    // Ruflo global memory path for any child that respects it.
    if let Ok(db) = ruflo_db_path().into_os_string().into_string() {
        cmd.env("CLAUDE_FLOW_DB_PATH", &db);
        cmd.env("CLAUDE_FLOW_MEMORY_PATH", ruflo_home());
    }
    // Ensure npm global bin is on PATH for child processes.
    if let Some(home) = dirs::home_dir() {
        let npm_bin = home.join("AppData").join("Roaming").join("npm");
        if npm_bin.is_dir() {
            let path = std::env::var_os("PATH").unwrap_or_default();
            let mut paths = std::env::split_paths(&path).collect::<Vec<_>>();
            paths.insert(0, npm_bin);
            if let Ok(joined) = std::env::join_paths(paths) {
                cmd.env("PATH", joined);
            }
        }
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn {resolved}: {e}"))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // Drain pipes on background threads with cap to avoid deadlock
    let out_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(s) = stdout {
            use std::io::Read;
            let mut limited = s.take(2_000_000); // 2MB cap for capture
            let _ = limited.read_to_end(&mut buf);
        }
        buf
    });
    let err_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(s) = stderr {
            use std::io::Read;
            let mut limited = s.take(500_000);
            let _ = limited.read_to_end(&mut buf);
        }
        buf
    });

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms.max(1_000));
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    // Kill child and whole tree on timeout
                    #[cfg(windows)]
                    {
                        let _ = std::process::Command::new("taskkill")
                            .args(["/PID", &child.id().to_string(), "/T", "/F"])
                            .output();
                    }
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "{resolved} timed out after {}ms (killed)",
                        timeout_ms
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(30));
            }
            Err(e) => return Err(format!("wait failed for {resolved}: {e}")),
        }
    };

    let stdout_bytes = out_handle.join().unwrap_or_default();
    let stderr_bytes = err_handle.join().unwrap_or_default();
    let mut out = String::from_utf8_lossy(&stdout_bytes).trim().to_string();
    let err = String::from_utf8_lossy(&stderr_bytes).trim().to_string();
    if !err.is_empty() {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        let filtered: Vec<&str> = err
            .lines()
            .filter(|l| !l.starts_with("[DEBUG]"))
            .collect();
        if !filtered.is_empty() {
            out.push_str(&filtered.join("\n"));
        }
    }
    if !status.success() && out.is_empty() {
        return Err(format!("{resolved} exited with {}", status));
    }
    if !status.success() {
        return Err(out);
    }
    Ok(if out.is_empty() {
        "(no output)".into()
    } else {
        out
    })
}

pub(crate) fn run_quiet(bin: &str, args: &[&str], cwd: Option<&Path>, timeout_ms: u64) -> bool {
    run_capture(bin, args, cwd, timeout_ms).is_ok()
}

fn cmd_version(bin: &str, args: &[&str]) -> Option<String> {
    cmd_version_pub(bin, args)
}

pub(crate) fn cmd_version_pub(bin: &str, args: &[&str]) -> Option<String> {
    run_capture(bin, args, None, 15_000)
        .ok()
        .map(|s| s.lines().next().unwrap_or(&s).trim().to_string())
}

/// PLUR inject for the current task — used to seed the system prompt.
pub fn plur_inject(task: &str) -> Option<String> {
    let bin = find_bin("plur")?;
    // Prefer --fast so cold start does not stall on ONNX download.
    let out = run_capture(
        &bin,
        &["inject", task, "--fast", "--json"],
        None,
        45_000,
    )
    .or_else(|_| run_capture(&bin, &["inject", task, "--fast"], None, 45_000))
    .ok()?;
    if out.trim().is_empty() || out.contains("\"count\":0") {
        return None;
    }
    // Cap prompt injection size.
    let capped: String = out.chars().take(4_000).collect();
    Some(capped)
}

/// Brief status snippet for the TUI banner / /ecosystem command.
/// Reads the on-disk marker only — never blocks on network installs.
pub fn quick_status() -> String {
    if let Ok(text) = fs::read_to_string(marker_path()) {
        if let Ok(st) = serde_json::from_str::<EcosystemStatus>(&text) {
            return st.report();
        }
    }
    "Meta ecosystem not provisioned yet — background ensure is running, or run:\n  meta ecosystem ensure\n".into()
}

/// One-line snapshot for TUI open. Instant; no npm/uv.
pub fn launch_snapshot() -> String {
    if let Ok(text) = fs::read_to_string(marker_path()) {
        if let Ok(st) = serde_json::from_str::<EcosystemStatus>(&text) {
            return st.summary_line();
        }
    }
    "ecosystem · provisioning in background…".into()
}

/// Sleep helper used when we want a soft bound (unused externally).
#[allow(dead_code)]
fn sleep_ms(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

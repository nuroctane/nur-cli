//! External agent-ecosystem integrations provisioned by Meta's one-shot install.
//!
//! Core runtime: Graphify · PLUR · Ruflo
//! Skill packs: Emil design · clone-website · cybersecurity · default plugins
//! (superpowers · fable · mattpocock · addyosmani · builderio) · OpenCode catalog
//! Gateways: Executor MCP · skills CLI · AKM
//! Patterns: DCP-style context pruning (native + docs)

use crate::config::muse_home;
use crate::error::{MuseError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub mod browser_setup;
mod packs;
mod skills;

pub use skills::install_bundled_skills;

/// Absolute path to the newest available `omp` binary (Bun / native / PATH).
pub fn find_omp() -> Option<String> {
    packs::best_omp()
        .map(|(path, _)| path)
        .or_else(|| find_bin("omp"))
}

const ECOSYSTEM_MARKER: &str = "ecosystem.json";
/// Bump when new packs/tools are added so old markers re-run ensure.
/// Bump when spawn/install logic changes so markers re-run ensure.
/// Bump to force `ensure_ecosystem` past a cached marker on upgrade.
/// 10: retire the resume-* skills superseded by `/takeover`.
/// 11: session_reader.py gains `--all-cwds` (takeover lists every workspace).
/// 12: headroom + optmem + egaki + fractal ensure + infinite-headcount pack.
/// 13: omp floor 17.2.0 + auto-upgrade for omp/plur/egaki/ruflo/browser/executor/graphify/headroom.
/// 14: penecho ensure (npm) + auto-config from nur auth + seamless browser launch.
/// 15: terminal-browser (native/WSL + Windows host fallback via agent-browser-cli).
const ECOSYSTEM_SCHEMA: u32 = 15;
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
    #[serde(default)]
    pub omp: ComponentStatus,
    /// GraphJin - governed GraphQL→SQL over live data (detect-only; opt-in).
    #[serde(default)]
    pub graphjin: ComponentStatus,
    #[serde(default)]
    pub browser: ComponentStatus,
    #[serde(default)]
    pub excalidraw: ComponentStatus,
    /// Cua Drivers - computer-use MCP server + CLI (`cua-driver`).
    #[serde(default)]
    pub cua: ComponentStatus,
    /// Akarso - social posting CLI/MCP (`akarso`).
    #[serde(default)]
    pub akarso: ComponentStatus,
    /// Headroom - tool-result context compression.
    #[serde(default)]
    pub headroom: ComponentStatus,
    /// OptMem - permanent memory (~/.optmem).
    #[serde(default)]
    pub optmem: ComponentStatus,
    /// egaki - image/video generation CLI.
    #[serde(default)]
    pub egaki: ComponentStatus,
    /// fractal - hierarchical agent loops (Unix).
    #[serde(default)]
    pub fractal: ComponentStatus,
    /// penecho - canvas beyond the chat box (npm AGPL sidecar).
    #[serde(default)]
    pub penecho: ComponentStatus,
    /// terminal-browser - in-terminal Chromium (native/WSL) or Windows-host fallback.
    #[serde(default)]
    pub terminal_browser: ComponentStatus,
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
            "ecosystem · {}  {}  {}  {}  {}  {}  {}  {}  {}  {}  {}  {}  {}  {}  {}  {}  {}  · packs {}",
            bit(self.graphify.available, "graphify"),
            bit(self.plur.available, "plur"),
            bit(self.ruflo.available, "ruflo"),
            bit(self.executor.available, "executor"),
            bit(self.omp.available, "omp"),
            bit(self.graphjin.available, "graphjin"),
            bit(self.browser.available, "browser"),
            bit(self.terminal_browser.available, "tb"),
            bit(self.excalidraw.available, "excalidraw"),
            bit(self.skills_cli.available, "skills"),
            bit(self.cua.available, "cua"),
            bit(self.akarso.available, "akarso"),
            bit(self.headroom.available, "headroom"),
            bit(self.optmem.available, "optmem"),
            bit(self.egaki.available, "egaki"),
            bit(self.fractal.available, "fractal"),
            bit(self.penecho.available, "penecho"),
            if self.packs_installed.is_empty() {
                "…".into()
            } else {
                self.packs_installed.join(",")
            }
        )
    }

    pub fn report(&self) -> String {
        let mut s = String::from("Nur ecosystem (auto-provisioned on install / open)\n");
        // Fixed names so older ecosystem.json markers (pre-field) still list every slot.
        let comps: [(&str, &ComponentStatus); 18] = [
            ("graphify", &self.graphify),
            ("plur", &self.plur),
            ("ruflo", &self.ruflo),
            ("skills", &self.skills_cli),
            ("akm", &self.akm),
            ("executor", &self.executor),
            ("omp", &self.omp),
            ("graphjin", &self.graphjin),
            ("browser", &self.browser),
            ("terminal_browser", &self.terminal_browser),
            ("excalidraw", &self.excalidraw),
            ("cua", &self.cua),
            ("akarso", &self.akarso),
            ("headroom", &self.headroom),
            ("optmem", &self.optmem),
            ("egaki", &self.egaki),
            ("fractal", &self.fractal),
            ("penecho", &self.penecho),
        ];
        for (fallback_name, c) in comps {
            let name = if c.name.is_empty() {
                fallback_name
            } else {
                c.name.as_str()
            };
            let detail = if c.name.is_empty() && c.detail.is_empty() {
                if c.available {
                    "ready"
                } else {
                    "not provisioned yet - will install on next open / ensure"
                }
            } else if c.detail.is_empty() {
                if c.available {
                    "ready"
                } else {
                    "missing"
                }
            } else {
                c.detail.as_str()
            };
            let mark = if c.available { "✓" } else { "✗" };
            s.push_str(&format!("  {mark} {name:12} {detail}\n"));
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
                "missing - install Node.js 20+"
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
        let (indexed, triggers) = crate::agent::skill_intents::stats();
        s.push_str(&format!(
            "  NL trigger index: {indexed} skills · {triggers} triggers\n"
        ));
        for n in &self.notes {
            s.push_str(&format!("  note: {n}\n"));
        }
        s.push_str(
            "\n  slash: /ecosystem /plur /ruflo /graphify /skills /akarso /openseo /tb\n\
             tools:  graphify plur ruflo akarso executor omp browser terminal_browser excalidraw penecho tldraw skill\n\
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
/// Safe to call on every launch - skips heavy work when the marker is fresh.
pub fn ensure_ecosystem(force: bool) -> EcosystemStatus {
    // Always heal ~/.muse → ~/.nur gaps before creating empty ruflo/skills dirs.
    let _ = crate::config::ensure_dirs();

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
        Ok(names) => {
            status.skills_installed = names;
            // Invalidate skill cache so next TUI gets fresh index (was 263ms cold, now 17ms cached)
            crate::agent::skill_cache::invalidate_cache();
        }
        Err(e) => status.notes.push(format!("bundled skills: {e}")),
    }

    status.graphify = ensure_graphify();
    status.plur = ensure_plur(status.node_ok);
    status.ruflo = ensure_ruflo(status.node_ok);
    status.skills_cli = packs::ensure_skills_cli(status.node_ok);
    status.akm = packs::ensure_akm(status.node_ok);
    status.executor = packs::ensure_executor(status.node_ok);
    status.omp = packs::ensure_omp();
    status.graphjin = packs::ensure_graphjin();
    status.browser = packs::ensure_browser_cli(status.node_ok);
    status.terminal_browser = ensure_terminal_browser(status.node_ok);
    status.excalidraw = ensure_excalidraw(status.node_ok);
    status.penecho = ensure_penecho(status.node_ok);
    status.headroom = ensure_headroom();
    status.optmem = ensure_optmem();
    status.egaki = ensure_egaki(status.node_ok);
    status.fractal = ensure_fractal();
    status.cua = ensure_cua();
    status.akarso = ensure_akarso(status.node_ok);

    // tldraw offline desktop app (official) - best-effort auto-install so `/draw`
    // works out of the box. No-ops when already present; skips quietly offline.
    match crate::tools::tldraw::ensure_installed() {
        Ok(note) => status.notes.push(format!(
            "tldraw offline: {}",
            note.lines().next().unwrap_or("ok")
        )),
        Err(e) => status.notes.push(format!("tldraw offline: {e}")),
    }

    // Third-party skill packs via skills CLI (network; markers skip re-download).
    let (packs_ok, pack_notes) = packs::install_skill_packs(&status.skills_cli);
    status.packs_installed = packs_ok;
    status.notes.extend(pack_notes);

    // Default marketplace plugins: superpowers, fable, mattpocock, addyosmani, builderio.
    // Recursive SKILL.md mirror so /skill-name slash + discovery work out of the box.
    let (plug_ok, plug_notes) = crate::plugins::ensure_default_plugins();
    for id in &plug_ok {
        if !status.packs_installed.iter().any(|p| p == id) {
            status.packs_installed.push(id.clone());
        }
    }
    status.notes.extend(plug_notes);
    // Packs/plugins may have mirrored new skills into ~/.nur/skills - invalidate cache
    crate::agent::skill_cache::invalidate_cache();

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
    // Re-run when a core/new component is missing so schema bumps and new
    // tools (excalidraw, browser, …) land without a manual --force.
    if age < ENSURE_TTL_SECS
        && st.graphify.available
        && st.plur.available
        && st.ruflo.available
        && st.skills_cli.available
        && st.excalidraw.available
        && st.omp.available
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

// ── penecho (canvas beyond chat) ──────────────────────────────────────────

/// Install penecho via npm and auto-write `~/.penecho/config.env` from nur auth
/// so `penecho(action=launch)` just opens the browser — no user diagnosis.
fn ensure_penecho(node_ok: bool) -> ComponentStatus {
    let mut c = ComponentStatus {
        name: "penecho".into(),
        ..Default::default()
    };
    if let Some(bin) = find_bin("penecho") {
        c.available = true;
        c.path = Some(bin.clone());
        c.version = cmd_version(&bin, &["--version"]);
        // Best-effort config so launch never drops into interactive configure.
        match crate::penecho::auto_configure_from_nur(false, crate::penecho::Effort::Medium) {
            Ok((_, msg)) => {
                c.detail = format!("CLI ready · {msg} · penecho tool opens browser canvas");
            }
            Err(e) => {
                c.detail = format!(
                    "CLI ready · config deferred ({e}) · launch will retry /login or CLI mode"
                );
            }
        }
        return c;
    }
    if !node_ok {
        c.detail = "needs Node.js 20.3+ - npm i -g penecho".into();
        return c;
    }
    let npm = find_bin("npm").unwrap_or_else(|| "npm".into());
    match run_capture(&npm, &["install", "-g", "penecho@latest"], None, 300_000) {
        Ok(_) => {}
        Err(e) => {
            c.detail = format!(
                "npm install failed: {}",
                e.chars().take(200).collect::<String>()
            );
            return c;
        }
    }
    if let Some(bin) = find_bin("penecho") {
        c.available = true;
        c.path = Some(bin.clone());
        c.version = cmd_version(&bin, &["--version"]);
        match crate::penecho::auto_configure_from_nur(false, crate::penecho::Effort::Medium) {
            Ok((_, msg)) => {
                c.detail = format!("installed via npm · {msg}");
            }
            Err(e) => {
                c.detail = format!("installed via npm · config deferred ({e})");
            }
        }
        return c;
    }
    if c.detail.is_empty() {
        c.detail = "not found after npm install - try: npm i -g penecho".into();
    }
    c
}

// ── terminal-browser ──────────────────────────────────────────────────────

fn ensure_terminal_browser(node_ok: bool) -> ComponentStatus {
    let mut c = ComponentStatus {
        name: "terminal_browser".into(),
        ..Default::default()
    };
    let _ = node_ok; // host fallback may npm-install agent-browser-cli itself
    let (ok, detail, path, version) = crate::terminal_browser::ensure_runtime();
    c.available = ok;
    c.detail = detail;
    c.path = path;
    c.version = version;
    if c.available && c.version.is_none() {
        if let Some(bin) = c.path.as_deref() {
            if !bin.starts_with("wsl:") && !bin.starts_with("host:") {
                c.version = cmd_version(bin, &["--version"]);
            }
        }
    }
    c
}

// ── Excalidraw CLI ────────────────────────────────────────────────────────

fn ensure_excalidraw(node_ok: bool) -> ComponentStatus {
    let mut c = ComponentStatus {
        name: "excalidraw".into(),
        ..Default::default()
    };
    if let Some(bin) = find_bin("excalidraw").or_else(|| find_bin("excalidraw-cli")) {
        c.available = true;
        c.path = Some(bin.clone());
        c.version = cmd_version(&bin, &["--version"]);
        c.detail = "CLI ready · diagrams via excalidraw tool".into();
        return c;
    }
    if !node_ok {
        c.detail = "needs Node.js 18+ - npm i -g excalidraw-cli".into();
        return c;
    }
    let npm = find_bin("npm").unwrap_or_else(|| "npm".into());
    match run_capture(&npm, &["install", "-g", "excalidraw-cli"], None, 300_000) {
        Ok(_) => {}
        Err(e) => {
            c.detail = format!(
                "npm install failed: {}",
                e.chars().take(200).collect::<String>()
            );
            return c;
        }
    }
    if let Some(bin) = find_bin("excalidraw").or_else(|| find_bin("excalidraw-cli")) {
        c.available = true;
        c.path = Some(bin.clone());
        c.version = cmd_version(&bin, &["--version"]);
        c.detail = "installed via npm i -g excalidraw-cli".into();
        return c;
    }
    if c.detail.is_empty() {
        c.detail = "not found after npm install - try: npm i -g excalidraw-cli".into();
    }
    c
}

// ── Headroom (context compression) ────────────────────────────────────────

fn ensure_headroom() -> ComponentStatus {
    let mut c = ComponentStatus {
        name: "headroom".into(),
        ..Default::default()
    };
    let _ = crate::headroom::ensure_helper_script();
    // Refresh Python package / uv tool when possible so inline compress stays current.
    if let Some(uv) = find_bin("uv") {
        let _ = run_capture(
            &uv,
            &[
                "tool",
                "install",
                "--upgrade",
                "--python",
                "3.13",
                "headroom-ai[all]",
            ],
            None,
            600_000,
        );
    }
    if let Some(bin) = find_bin("headroom") {
        c.available = true;
        c.path = Some(bin.clone());
        c.version = cmd_version(&bin, &["--version"]);
        c.detail = "CLI ready · inline tool-result compress (disable: [headroom] enabled=false)".into();
        return c;
    }
    // Best-effort install via uv / pip
    if let Some(uv) = find_bin("uv") {
        let _ = run_capture(
            &uv,
            &[
                "tool",
                "install",
                "--python",
                "3.13",
                "headroom-ai[all]",
            ],
            None,
            600_000,
        );
    } else if let Some(pip) = find_bin("pip3").or_else(|| find_bin("pip")) {
        let _ = run_capture(
            &pip,
            &["install", "--user", "-U", "headroom-ai[all]"],
            None,
            600_000,
        );
    }
    if let Some(bin) = find_bin("headroom") {
        c.available = true;
        c.path = Some(bin.clone());
        c.version = cmd_version(&bin, &["--version"]);
        c.detail = "installed headroom-ai".into();
        return c;
    }
    // Inline compress only needs `from headroom import compress` - no CLI required.
    if crate::headroom::python_import_ok() {
        c.available = true;
        c.detail =
            "Python package ready · inline compress (no headroom CLI; disable: [headroom] enabled=false)"
                .into();
    } else {
        c.detail =
            "not found - uv tool install --python 3.13 \"headroom-ai[all]\" (inline compress no-ops until installed)"
                .into();
    }
    c
}

// ── OptMem (permanent memory, upstream ~/.optmem) ─────────────────────────

fn ensure_optmem() -> ComponentStatus {
    let mut c = ComponentStatus {
        name: "optmem".into(),
        ..Default::default()
    };
    match crate::optmem::ensure_install() {
        Ok(msg) => {
            c.available = crate::optmem::memo_bin().is_some();
            c.path = crate::optmem::memo_bin().map(|p| p.display().to_string());
            c.detail = msg;
        }
        Err(e) => {
            c.detail = e;
        }
    }
    c
}

// ── egaki (image/video gen) ───────────────────────────────────────────────

fn ensure_egaki(node_ok: bool) -> ComponentStatus {
    let mut c = ComponentStatus {
        name: "egaki".into(),
        ..Default::default()
    };
    if !node_ok {
        c.detail = "needs Node.js - npm i -g egaki@latest".into();
        return c;
    }
    let npm = find_bin("npm").unwrap_or_else(|| "npm".into());
    let _ = run_capture(&npm, &["install", "-g", "egaki@latest"], None, 300_000);
    if let Some(bin) = find_bin("egaki") {
        c.available = true;
        c.path = Some(bin.clone());
        c.version = cmd_version(&bin, &["--version"]);
        c.detail =
            "CLI ready · egaki login --provider chatgpt | xai-oauth | egaki --key …".into();
    } else {
        c.detail = "not found after npm install - try: npm i -g egaki@latest".into();
    }
    c
}

// ── fractal (Unix hierarchical loops) ─────────────────────────────────────

fn ensure_fractal() -> ComponentStatus {
    let mut c = ComponentStatus {
        name: "fractal".into(),
        ..Default::default()
    };
    #[cfg(windows)]
    {
        c.detail = crate::fractal::PLATFORM_UNSUPPORTED.to_string();
        // Still report binary presence for awareness
        if let Some(bin) = crate::fractal::find_on_path("fractal") {
            c.path = Some(bin.display().to_string());
        }
        return c;
    }
    #[cfg(not(windows))]
    {
        if let Some(bin) = crate::fractal::find_on_path("fractal") {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let probe = crate::fractal::probe_at(&cwd);
            c.available = probe.usable;
            c.path = Some(bin.display().to_string());
            c.version = probe.version;
            if probe.usable {
                c.detail = "CLI ready · /fractal · unattended nodes bypass approvals".into();
                let _ = run_capture(&bin.to_string_lossy(), &["install"], None, 60_000);
            } else {
                c.detail = probe
                    .unusable_reason
                    .unwrap_or_else(|| "fractal present but not usable".into());
            }
            return c;
        }
        if let Some(uv) = find_bin("uv") {
            let _ = run_capture(
                &uv,
                &[
                    "tool",
                    "install",
                    "plasma-fractal",
                    "--with-executables-from",
                    "plasma-wiki",
                ],
                None,
                600_000,
            );
        } else if let Some(pipx) = find_bin("pipx") {
            let _ = run_capture(&pipx, &["install", "plasma-fractal"], None, 600_000);
        }
        if let Some(bin) = crate::fractal::find_on_path("fractal") {
            c.available = true;
            c.path = Some(bin.display().to_string());
            c.detail = "installed plasma-fractal".into();
        } else {
            c.detail = "not found - uv tool install plasma-fractal (Unix only)".into();
        }
        c
    }
}

// ── Akarso (social posting CLI/MCP) ─────────────────────────────────────────

/// Akarso - post/schedule/reply across 14 social platforms (`akarso` npm CLI).
/// Best-effort global install so the `akarso` tool + `/akarso` work out of the
/// box; the user still runs `akarso auth login` once to connect. Never blocks
/// ensure on failure (no account required to install the CLI).
fn ensure_akarso(node_ok: bool) -> ComponentStatus {
    let mut c = ComponentStatus {
        name: "akarso".into(),
        ..Default::default()
    };
    if let Some(bin) = find_bin("akarso") {
        c.available = true;
        c.path = Some(bin.clone());
        c.version = cmd_version(&bin, &["--version"]);
        c.detail =
            "CLI ready · social posting via the akarso tool (run `akarso auth login`)".into();
        return c;
    }
    if !node_ok {
        c.detail = "needs Node.js 18+ - npm i -g akarso".into();
        return c;
    }
    let _ = run_quiet("npm", &["install", "-g", "akarso"], None, 600_000);
    if let Some(bin) = find_bin("akarso") {
        c.available = true;
        c.path = Some(bin.clone());
        c.version = cmd_version(&bin, &["--version"]);
        c.detail = "installed via npm i -g akarso · run `akarso auth login`".into();
    } else {
        c.detail = "not found - npm install -g akarso".into();
    }
    c
}

// ── Cua Drivers (computer-use MCP + CLI) ────────────────────────────────────

/// Locate the `cua-driver` binary. Falls back to the Windows installer's fixed
/// LOCALAPPDATA location so we find it right after install, before a new shell
/// picks up the updated User PATH.
fn cua_driver_bin() -> Option<String> {
    if let Some(bin) = find_bin("cua-driver") {
        return Some(bin);
    }
    #[cfg(windows)]
    if let Some(local) = dirs::data_local_dir() {
        let p = local
            .join("Programs")
            .join("Cua")
            .join("cua-driver")
            .join("bin")
            .join("cua-driver.exe");
        if p.is_file() {
            return Some(p.to_string_lossy().to_string());
        }
    }
    None
}

/// Public locator for the `cua-driver` binary (used by the `/cua` command).
pub fn cua_driver_path() -> Option<String> {
    cua_driver_bin()
}

/// Cua Drivers - computer-use MCP server + CLI (`cua-driver`) from trycua/cua.
/// Installed via the vendor's official script. On Windows we pass `-NoAutoStart`
/// so nur never silently registers an **elevated** background daemon: the useful
/// binary lands on PATH, and you start it on demand (`cua-driver serve`) or wire
/// its MCP (`cua-driver mcp-config`). Best-effort - a failure never blocks ensure.
fn ensure_cua() -> ComponentStatus {
    let mut c = ComponentStatus {
        name: "cua".into(),
        ..Default::default()
    };
    if let Some(bin) = cua_driver_bin() {
        c.available = true;
        c.version = cmd_version(&bin, &["--version"]);
        c.path = Some(bin);
        c.detail = "cua-driver ready · computer-use MCP/CLI (no autostart daemon)".into();
        return c;
    }

    #[cfg(windows)]
    let install = run_capture(
        "powershell",
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "& ([scriptblock]::Create((irm https://cua.ai/driver/install.ps1))) -NoAutoStart",
        ],
        None,
        300_000,
    );
    #[cfg(not(windows))]
    let install = run_capture(
        "bash",
        &["-c", "curl -fsSL https://cua.ai/driver/install.sh | bash"],
        None,
        300_000,
    );

    if let Err(e) = install {
        c.detail = format!(
            "cua-driver install failed: {}",
            e.chars().take(200).collect::<String>()
        );
        return c;
    }

    if let Some(bin) = cua_driver_bin() {
        c.available = true;
        c.version = cmd_version(&bin, &["--version"]);
        c.path = Some(bin);
        c.detail = "cua-driver installed · computer-use MCP/CLI (no autostart daemon)".into();
    } else {
        c.detail = "installed but cua-driver not on PATH yet - open a new shell".into();
    }
    c
}

// ── Graphify ──────────────────────────────────────────────────────────────

fn ensure_graphify() -> ComponentStatus {
    let mut c = ComponentStatus {
        name: "graphify".into(),
        ..Default::default()
    };
    // Prefer upgrading via uv when available so ensure tracks latest graphifyy.
    if which("uv") || which("uv.exe") {
        let _ = run_quiet(
            "uv",
            &["tool", "install", "--upgrade", "graphifyy"],
            None,
            300_000,
        );
        if find_bin("graphify").is_none() {
            let _ = run_quiet("uv", &["tool", "install", "graphifyy"], None, 300_000);
        }
    }
    if let Some(bin) = find_bin("graphify") {
        c.available = true;
        c.path = Some(bin.clone());
        c.version = cmd_version(&bin, &["--version"]);
        c.detail = "CLI ready".into();
        let _ = run_quiet(&bin, &["install", "--platform", "agents"], None, 120_000);
        return c;
    }
    c.detail = "not found - install: uv tool install graphifyy".into();
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
    // Refresh to latest on every ensure (marker TTL / schema bump gated).
    let _ = run_quiet(
        "npm",
        &[
            "install",
            "-g",
            "@plur-ai/cli@latest",
            "@plur-ai/mcp@latest",
        ],
        None,
        600_000,
    );
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
        c.detail = "not found - npm install -g @plur-ai/cli @plur-ai/mcp".into();
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
        "When editing code in nur-cli or similar agents, prefer apply_patch / multi_edit over full file rewrites for multi-hunk changes.",
        "Never commit secrets, API keys, or ~/.nur/auth.json. Keys live only in local auth storage.",
        "Prefer graphify query/path/explain over broad grep when graphify-out/graph.json exists for architecture questions.",
        "PLUR engrams are shared memory - learn corrections and preferences so future sessions remember them.",
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
    // Refresh ruflo to latest on ensure.
    let npm = find_bin("npm").unwrap_or_else(|| "npm".into());
    let _ = run_quiet(
        &npm,
        &["install", "-g", "ruflo@latest", "--omit=optional"],
        None,
        600_000,
    );
    if find_bin("ruflo").is_none() {
        let _ = run_quiet(&npm, &["install", "-g", "ruflo@latest"], None, 600_000);
    }
    let Some(bin) = find_bin("ruflo") else {
        c.detail = "not found - npm install -g ruflo".into();
        return c;
    };
    c.available = true;
    c.path = Some(bin.clone());
    c.version = cmd_version(&bin, &["--version"]);

    let home = ruflo_home();
    let _ = fs::create_dir_all(&home);
    let db = ruflo_db_path();

    // Initialise memory DB once (global under ~/.nur/ruflo - does not pollute projects).
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
/// On Windows we never return a bare name like `"npm"` / `"skills"` - those
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
        home.join("AppData").join("Roaming").join("npm").join(name),
        PathBuf::from(r"C:\Program Files\nodejs").join(format!("{name}.cmd")),
        PathBuf::from(r"C:\Program Files\nodejs").join(format!("{name}.exe")),
        PathBuf::from(r"C:\Program Files\nodejs").join(name),
        // Bun global installs (`bun install -g`) - e.g. the omp coding agent.
        home.join(".bun").join("bin").join(format!("{name}.exe")),
        home.join(".bun").join("bin").join(format!("{name}.cmd")),
        home.join(".bun").join("bin").join(name),
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

    // `where` / `which` last - returns absolute paths on modern Windows.
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
        let out = Command::new("where.exe").arg(name).output().ok()?;
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
        let p = String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()?
            .trim()
            .to_string();
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
        use std::os::windows::process::CommandExt;
        let lower = bin.to_ascii_lowercase();
        let needs_cmd = lower.ends_with(".cmd")
            || lower.ends_with(".bat")
            || (!bin.contains('\\') && !bin.contains('/'));
        let mut c = if needs_cmd {
            let mut c = Command::new("cmd.exe");
            c.arg("/D").arg("/C").arg(bin);
            for a in args {
                c.arg(a);
            }
            c
        } else {
            let mut c = Command::new(bin);
            c.args(args);
            c
        };
        // CREATE_NO_WINDOW - keep background/ensure children off nur's console so
        // their cmd/npm `SetConsoleTitle` never clobbers nur's animated moon-phase
        // window title (and no console window flashes on install).
        c.creation_flags(0x0800_0000);
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new(bin);
        c.args(args);
        c
    }
}

pub fn run_capture(
    bin: &str,
    args: &[&str],
    cwd: Option<&Path>,
    timeout_ms: u64,
) -> std::result::Result<String, String> {
    run_capture_inner(bin, args, cwd, timeout_ms, None)
}

/// Capture a child process while honoring the agent turn's cancellation token.
/// Cancellation kills the full process tree so delegated agents cannot keep
/// editing or spending after the user presses Esc.
pub fn run_capture_cancelled(
    bin: &str,
    args: &[&str],
    cwd: Option<&Path>,
    timeout_ms: u64,
    cancel: &tokio_util::sync::CancellationToken,
) -> std::result::Result<String, String> {
    run_capture_inner(bin, args, cwd, timeout_ms, Some(cancel))
}

fn run_capture_inner(
    bin: &str,
    args: &[&str],
    cwd: Option<&Path>,
    timeout_ms: u64,
    cancel: Option<&tokio_util::sync::CancellationToken>,
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
    // Capture output manually to enforce timeout; never inherit TUI stdin.
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
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

    let deadline =
        std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms.max(1_000));
    let mut stopped_early = None;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
                if cancel.is_some_and(|token| token.is_cancelled()) {
                    stopped_early = Some(format!("{resolved} cancelled (process tree killed)"));
                    kill_process_tree(&mut child);
                    break child
                        .wait()
                        .map_err(|error| format!("wait after cancel failed: {error}"))?;
                }
                if std::time::Instant::now() >= deadline {
                    stopped_early = Some(format!(
                        "{resolved} timed out after {timeout_ms}ms (process tree killed)"
                    ));
                    kill_process_tree(&mut child);
                    break child
                        .wait()
                        .map_err(|error| format!("wait after timeout failed: {error}"))?;
                }
                std::thread::sleep(std::time::Duration::from_millis(30));
            }
            Err(e) => return Err(format!("wait failed for {resolved}: {e}")),
        }
    };

    let stdout_bytes = join_capture(out_handle, 2_000);
    let stderr_bytes = join_capture(err_handle, 2_000);
    if let Some(reason) = stopped_early {
        return Err(reason);
    }
    let mut out = String::from_utf8_lossy(&stdout_bytes).trim().to_string();
    let err = String::from_utf8_lossy(&stderr_bytes).trim().to_string();
    if !err.is_empty() {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        let filtered: Vec<&str> = err.lines().filter(|l| !l.starts_with("[DEBUG]")).collect();
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

fn join_capture(handle: std::thread::JoinHandle<Vec<u8>>, timeout_ms: u64) -> Vec<u8> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(handle.join().unwrap_or_default());
    });
    rx.recv_timeout(std::time::Duration::from_millis(timeout_ms))
        .unwrap_or_default()
}

fn kill_process_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .creation_flags(0x0800_0000)
            .output();
    }
    let _ = child.kill();
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

/// PLUR inject for the current task - used to seed the system prompt.
pub fn plur_inject(task: &str) -> Option<String> {
    let bin = find_bin("plur")?;
    // Prefer --fast so cold start does not stall on ONNX download.
    let out = run_capture(&bin, &["inject", task, "--fast", "--json"], None, 45_000)
        .or_else(|_| run_capture(&bin, &["inject", task, "--fast"], None, 45_000))
        .ok()?;
    if out.trim().is_empty() || out.contains("\"count\":0") {
        return None;
    }
    // Cap prompt injection size.
    let capped: String = out.chars().take(4_000).collect();
    Some(capped)
}

/// Status for `/ecosystem` and doctor. Heals the marker when schema is old or
/// a component (e.g. excalidraw) is missing - same path as one-shot install.
pub fn quick_status() -> String {
    // Prefer a live ensure so /ecosystem never lies about a stale marker.
    // Cached when fresh (TTL + all core bits including excalidraw).
    let st = ensure_ecosystem(false);
    st.report()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_capture_kills_the_child_promptly() {
        let cancel = tokio_util::sync::CancellationToken::new();
        cancel.cancel();
        let started = std::time::Instant::now();
        #[cfg(windows)]
        let result = run_capture_cancelled(
            "powershell",
            &["-NoProfile", "-Command", "Start-Sleep -Seconds 30"],
            None,
            60_000,
            &cancel,
        );
        #[cfg(not(windows))]
        let result = run_capture_cancelled("sh", &["-c", "sleep 30"], None, 60_000, &cancel);

        assert!(result.unwrap_err().contains("cancelled"));
        // The property is "cancellation does not wait for the child to finish
        // on its own" - the child sleeps 30s, so anything comfortably under
        // that proves it. The old 5s bound was really measuring PowerShell's
        // cold-start time and failed intermittently whenever the rest of the
        // suite was spawning processes in parallel; a flaky assertion trains
        // people to ignore failures, which costs more than the tighter bound
        // ever bought.
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(15),
            "cancel took {elapsed:?} - should not wait out the child's 30s sleep"
        );
    }
}

//! One-stop self-install — same job as `install.ps1` / `install.sh`, minus the
//! cargo build (this binary *is* the product).
//!
//! Release users: download `nur-windows-*.exe` → run it → full stack lands
//! under `~/.local/bin` + `~/.nur` **before** any TUI. No "open first, packs later".

use crate::config;
use crate::ecosystem;
use crate::error::{MuseError, Result};
use crate::theme;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const BOOTSTRAP_MARKER: &str = "bootstrap.json";
/// Bump when self-install steps change so incomplete installs re-run fully.
const BOOTSTRAP_SCHEMA: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct BootstrapMarker {
    #[serde(default)]
    schema: u32,
    #[serde(default)]
    version: String,
    #[serde(default)]
    binary: String,
    #[serde(default)]
    completed_at: u64,
    #[serde(default)]
    ecosystem_ok: bool,
}

/// Default install directory (`~/.local/bin`) — same as the shell installers.
pub fn install_dir() -> PathBuf {
    install_dir_from(env::var("NUR_INSTALL_DIR").ok().as_deref())
}

/// Split out so the override is testable without mutating process env (tests run
/// in parallel threads and `set_var` would leak across them).
///
/// `NUR_INSTALL_DIR` exists so an end-to-end update rehearsal can point the whole
/// install/replace path at a scratch directory instead of the user's live
/// `~/.local/bin/nur` — proving the download+swap without risking the real tool.
fn install_dir_from(env_override: Option<&str>) -> PathBuf {
    if let Some(dir) = env_override {
        let t = dir.trim();
        if !t.is_empty() {
            return PathBuf::from(t);
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local")
        .join("bin")
}

#[cfg(windows)]
pub fn install_binary_path() -> PathBuf {
    install_dir().join("nur.exe")
}

#[cfg(not(windows))]
pub fn install_binary_path() -> PathBuf {
    install_dir().join("nur")
}

fn marker_path() -> PathBuf {
    config::muse_home().join(BOOTSTRAP_MARKER)
}

/// True when this process was launched as a GitHub Releases artifact
/// (`nur-windows-x86_64.exe`, etc.) rather than the installed `nur` name.
pub fn looks_like_release_artifact() -> bool {
    let Ok(exe) = env::current_exe() else {
        return false;
    };
    let name = exe
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    name.starts_with("nur-windows")
        || name.starts_with("nur-linux")
        || name.starts_with("nur-macos")
        || name.starts_with("nur-darwin")
        || name.contains("nur-windows-x86_64")
        // Legacy pre-rebrand release-asset names.
        || name.starts_with("meta-linux")
        || name.starts_with("meta-macos")
        || name.starts_with("meta-darwin")
}

pub fn is_running_from_install() -> bool {
    let Ok(exe) = env::current_exe() else {
        return false;
    };
    let installed = install_binary_path();
    let (a, b) = match (fs::canonicalize(&exe), fs::canonicalize(&installed)) {
        (Ok(a), Ok(b)) => (a, b),
        _ => return paths_equal_loose(&exe, &installed),
    };
    a == b
}

fn paths_equal_loose(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| p.to_string_lossy().replace('/', "\\").to_ascii_lowercase();
    norm(a) == norm(b)
}

#[allow(dead_code)]
fn bootstrap_complete() -> bool {
    let text = match fs::read_to_string(marker_path()) {
        Ok(t) => t,
        Err(_) => return false,
    };
    let Ok(m) = serde_json::from_str::<BootstrapMarker>(&text) else {
        return false;
    };
    m.schema >= BOOTSTRAP_SCHEMA
        && m.ecosystem_ok
        && install_binary_path().is_file()
        && !m.version.is_empty()
}

/// Interactive TUI launch should run a full one-stop install first when:
/// - user double-clicked a **release artifact** (`nur-windows-*.exe`), or
/// - there is **no** installed `~/.local/bin/meta` yet (first-time cargo binary), or
/// - `META_FORCE_BOOTSTRAP=1`
///
/// Already-installed `nur` on PATH must **never** re-enter one-stop install on
/// every open — that used to rename the running EXE to `meta.old` and brick PATH.
///
/// Skip with `NUR_SKIP_BOOTSTRAP=1` (dev / re-exec after install; legacy
/// `META_SKIP_BOOTSTRAP` still honored). Force anytime: `nur install`.
pub fn should_bootstrap_on_launch() -> bool {
    if env_truthy("NUR_SKIP_BOOTSTRAP") || env_truthy("META_SKIP_BOOTSTRAP") {
        return false;
    }
    if env_truthy("NUR_FORCE_BOOTSTRAP") || env_truthy("META_FORCE_BOOTSTRAP") {
        return true;
    }
    // Downloads folder / release asset: always one-stop.
    if looks_like_release_artifact() {
        return true;
    }
    // Installed binary (or already running from it): open TUI, do not reinstall.
    if is_running_from_install() || install_binary_path().is_file() {
        return false;
    }
    // No install on disk yet (e.g. bare `target/release/nur`) → offer full setup once.
    true
}

fn env_truthy(key: &str) -> bool {
    match env::var(key) {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            !(v.is_empty() || v == "0" || v == "false" || v == "no" || v == "off")
        }
        Err(_) => false,
    }
}

/// Full one-stop install: binary → PATH → prereqs (best-effort) → ecosystem →
/// browser → Orca hook → optional auth from env. Prints progress to stdout.
/// Does **not** open the TUI.
pub fn run_full_install() -> Result<()> {
    let _ = config::ensure_dirs();

    println!();
    theme::print_info("NurCLI — one-stop install");
    theme::print_info("same stack as the one-liner · no TUI until this finishes");
    println!();

    // ── 1. Install this binary ───────────────────────────────────────────
    step("Installing binary to ~/.local/bin…");
    let dest_dir = install_dir();
    fs::create_dir_all(&dest_dir)?;
    let src = env::current_exe().map_err(MuseError::Io)?;
    let dest = install_binary_path();

    if same_file(&src, &dest) {
        theme::print_ok(&format!("Already at {}", dest.display()));
    } else {
        install_binary_safe(&src, &dest)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&dest, fs::Permissions::from_mode(0o755));
        }
        if let Some(hash) = file_sha256(&dest) {
            let record = format!(
                "{hash}  {}",
                dest.file_name().and_then(|s| s.to_str()).unwrap_or("nur")
            );
            let _ = fs::write(dest_dir.join("nur.sha256"), format!("{record}\n"));
            theme::print_ok(&format!("SHA-256 {hash}"));
        }
        theme::print_ok(&format!("Installed {}", dest.display()));
    }
    // Product binary is ONLY `nur`. Remove legacy Meta/Muse names.
    // Also remove same-hash impostors of *this* image under other agent names
    // (e.g. an old install that overwrote claude.exe with meta.exe).
    scrub_legacy_and_impostor_bins(&dest_dir, &dest);

    // Prefer the install dir for everything that follows in this process.
    prepend_path(&dest_dir);

    // ── 2. User PATH ─────────────────────────────────────────────────────
    step("Ensuring ~/.local/bin is on PATH…");
    match ensure_user_path(&dest_dir) {
        Ok(true) => theme::print_ok("Added ~/.local/bin to user PATH (new terminals pick it up)"),
        Ok(false) => theme::print_ok("PATH already includes ~/.local/bin"),
        Err(e) => theme::print_info(&format!("PATH note: {e} (binary still installed)")),
    }

    // ── 3. Prereqs (best-effort, same list as install.ps1 / install.sh) ───
    step("Checking prerequisites (node · bun · uv · rg · ffmpeg)…");
    ensure_prereqs_best_effort();
    // Re-export common install locations for child processes.
    prepend_path(&dest_dir);
    if let Some(home) = dirs::home_dir() {
        prepend_path(&home.join(".bun").join("bin"));
        prepend_path(&home.join(".cargo").join("bin"));
    }
    #[cfg(windows)]
    {
        if let Ok(local) = env::var("LOCALAPPDATA") {
            prepend_path(
                &Path::new(&local)
                    .join("Microsoft")
                    .join("WinGet")
                    .join("Links"),
            );
        }
        if let Ok(pf) = env::var("ProgramFiles") {
            prepend_path(&Path::new(&pf).join("nodejs"));
        }
    }

    // ── 4. Ecosystem (blocking — this is the whole point) ────────────────
    step("Provisioning ecosystem (graphify · plur · ruflo · omp · browser · excalidraw · skills)…");
    theme::print_info("this can take a few minutes the first time — hang tight");
    let st = ecosystem::ensure_ecosystem(true);
    print!("{}", st.report());
    let any_ok = st.graphify.available
        || st.plur.available
        || st.ruflo.available
        || st.browser.available
        || st.excalidraw.available
        || st.skills_cli.available;
    if st.graphify.available && st.plur.available && st.ruflo.available && st.excalidraw.available {
        theme::print_ok("ecosystem ready (incl. excalidraw)");
    } else if st.graphify.available && st.plur.available && st.ruflo.available {
        theme::print_ok("ecosystem core ready");
        if !st.excalidraw.available {
            theme::print_info("excalidraw-cli deferred — Node/npm required for diagrams");
        }
    } else if any_ok {
        theme::print_info("partial ecosystem — missing pieces noted above (Node/uv help)");
    } else {
        theme::print_info(
            "ecosystem packs need Node.js 20+ and uv — install those, then: nur install",
        );
    }

    // ── 5. Browser stage (no TUI) ────────────────────────────────────────
    step("Browser tool setup…");
    match stage_browser_quiet() {
        Ok(msg) => theme::print_ok(&msg),
        Err(e) => theme::print_info(&format!("browser setup deferred: {e}")),
    }

    // ── 6. Orca hook ─────────────────────────────────────────────────────
    step("Orca hook (best-effort)…");
    match crate::ade::install_orca_hook() {
        Ok(()) => {}
        Err(e) => theme::print_info(&format!("Orca hook skipped: {e}")),
    }

    // ── 7. Auth from env (never print the key) ────────────────────────────
    if let Some(key) = env_api_key() {
        step("API key found in environment — saving to ~/.nur/auth.json…");
        match crate::auth::save_api_key(&key) {
            Ok(()) => theme::print_ok("Auth stored under ~/.nur/ (never committed to git)"),
            Err(e) => theme::print_info(&format!("auth save failed: {e}")),
        }
    } else {
        theme::print_info("No API key in env yet — you'll sign in on first open (/login)");
        theme::print_info("Get a key: https://dev.meta.ai/");
    }

    // ── 8. Marker ────────────────────────────────────────────────────────
    // Always mark complete after a full pass — packs are best-effort (need
    // Node/uv). Release artifacts re-run install on every double-click via
    // `looks_like_release_artifact`, not via a sticky failure loop.
    let marker = BootstrapMarker {
        schema: BOOTSTRAP_SCHEMA,
        version: env!("CARGO_PKG_VERSION").into(),
        binary: dest.display().to_string(),
        completed_at: now_secs(),
        ecosystem_ok: true,
    };
    if let Ok(text) = serde_json::to_string_pretty(&marker) {
        let _ = fs::write(marker_path(), text);
    }

    println!();
    theme::print_ok("Done. Full stack is on this machine.");
    theme::print_info(&format!("Binary:  {}", dest.display()));
    theme::print_info("Run:     nur");
    theme::print_info("Auth:    nur auth login   (or /login in the TUI)");
    theme::print_info("Doctor:  nur doctor");
    theme::print_info("Update:  nur update");
    println!();

    Ok(())
}

/// `nur update` — prefer GitHub release binary; fall back to git pull + rebuild
/// when a local checkout exists; last resort reinstalls the running binary.
pub fn run_update() -> Result<()> {
    println!();
    theme::print_info("NurCLI — update");
    theme::print_info("GitHub release · or git pull + cargo build --release");
    println!();

    // 1) GitHub prebuilt (works for release-installed users without a clone).
    match try_install_from_github(true) {
        Ok(UpdateOutcome::Updated { version }) => {
            theme::print_ok(&format!("Updated to v{version} from GitHub Releases"));
            finish_update_stack(&version)?;
            return Ok(());
        }
        Ok(UpdateOutcome::AlreadyCurrent { version }) => {
            theme::print_ok(&format!("Already on latest release (v{version})"));
            // Still refresh ecosystem packs.
            finish_update_stack(&version)?;
            return Ok(());
        }
        Err(e) => {
            theme::print_info(&format!("GitHub release path skipped: {e}"));
        }
    }

    // 2) Local source tree rebuild.
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let mut repo = home.join("laboratory").join("nur-cli");
    if !repo.join("Cargo.toml").is_file() {
        let alt = home.join("Laboratory").join("nur-cli");
        if alt.join("Cargo.toml").is_file() {
            repo = alt;
        }
    }

    if repo.join("Cargo.toml").is_file() {
        step(&format!("Updating checkout {}…", repo.display()));
        let st = Command::new("git")
            .args(["pull", "--ff-only", "origin", "main"])
            .current_dir(&repo)
            .status();
        match st {
            Ok(s) if s.success() => theme::print_ok("git pull ok"),
            Ok(_) => theme::print_info("git pull non-zero — continuing with local tree"),
            Err(e) => theme::print_info(&format!("git pull skipped: {e}")),
        }
        step("Building release…");
        let st = Command::new("cargo")
            .args(["build", "--release", "-q"])
            .current_dir(&repo)
            .status()
            .map_err(|e| MuseError::Other(format!("cargo: {e}")))?;
        if !st.success() {
            let _ = Command::new("cargo")
                .args(["build", "--release"])
                .current_dir(&repo)
                .status();
            return Err(MuseError::Other("cargo build --release failed".into()));
        }
        theme::print_ok("cargo build --release ok");
        #[cfg(windows)]
        let built = repo.join("target").join("release").join("nur.exe");
        #[cfg(not(windows))]
        let built = repo.join("target").join("release").join("nur");
        if !built.is_file() {
            return Err(MuseError::Other(format!(
                "missing built binary at {}",
                built.display()
            )));
        }
        step("Installing built binary…");
        let dest_dir = install_dir();
        fs::create_dir_all(&dest_dir)?;
        let dest = install_binary_path();
        install_binary_safe(&built, &dest)?;
        scrub_legacy_and_impostor_bins(&dest_dir, &dest);
        theme::print_ok(&format!("Installed {}", dest.display()));
        prepend_path(&dest_dir);
        let _ = ensure_user_path(&dest_dir);
        finish_update_stack(env!("CARGO_PKG_VERSION"))?;
        return Ok(());
    }

    // 3) No network + no source — reinstall the currently running binary.
    theme::print_info("No GitHub asset and no local checkout — reinstalling this binary");
    run_full_install()
}

fn finish_update_stack(version: &str) -> Result<()> {
    let dest = install_binary_path();
    step("Provisioning ecosystem…");
    let _ = Command::new(&dest)
        .args(["ecosystem", "ensure", "--force"])
        .status();
    let _ = Command::new(&dest).args(["browser", "setup"]).status();
    let _ = Command::new(&dest).arg("install-hook").status();

    let marker = BootstrapMarker {
        schema: BOOTSTRAP_SCHEMA,
        version: version.into(),
        binary: dest.display().to_string(),
        completed_at: now_secs(),
        ecosystem_ok: true,
    };
    if let Ok(text) = serde_json::to_string_pretty(&marker) {
        let _ = fs::write(marker_path(), text);
    }

    println!();
    theme::print_ok("Update complete.");
    theme::print_info(&format!("Binary:  {}", dest.display()));
    theme::print_info("Run:     nur");
    println!();
    Ok(())
}

// ── Auto-update on launch (GitHub Releases) ───────────────────────────────

const GH_RELEASES_LATEST: &str = "https://api.github.com/repos/nuroctane/nur-cli/releases/latest";

/// Floor between network checks — **not** a cache TTL. The contract is "every
/// time `nur` runs in a shell it looks for a new release"; this only stops a
/// script that loops `nur` in a tight `for` loop from hammering api.github.com
/// (unauthenticated GitHub allows 60 req/h/IP, so 60s ≈ exactly at the limit).
///
/// This used to be 6 hours, which meant the overwhelming majority of launches
/// never touched the network at all — the feature looked broken because it was
/// asleep, not because the plumbing failed.
const AUTO_UPDATE_MIN_INTERVAL_SECS: u64 = 60;

/// Leftover `.nur-update-*` downloads older than this are abandoned garbage: the
/// background thread is killed when a short headless run exits, so a partial
/// body can outlive the process that was writing it.
const UPDATE_TMP_MAX_AGE_SECS: u64 = 60 * 60;
const UPDATE_TMP_PREFIX: &str = ".nur-update-";

/// Per-shell opt-out. Returns the name of the set variable (for reporting) or
/// `None`.
///
/// Honors Claude Code's `DISABLE_AUTOUPDATER`: that variable is injected only
/// inside AI-agent sessions to stop tools from swapping their own binary out
/// from under a running agent, and it is absent from an ordinary terminal — so
/// respecting it costs the user nothing (their real shell still updates every
/// run) while keeping nur from silently changing version mid-session. It stays
/// alongside nur's own kill switches rather than replacing them.
fn auto_update_opt_out_var() -> Option<&'static str> {
    [
        "NUR_SKIP_AUTO_UPDATE",
        "META_SKIP_AUTO_UPDATE",
        "NUR_DISABLE_UPDATES",
        "DISABLE_UPDATES",
        "DISABLE_AUTOUPDATER",
    ]
    .into_iter()
    .find(|k| env_truthy(k))
}

/// Effective floor, overridable per-shell for testing/CI. `0` = check every run.
fn auto_update_min_interval_secs() -> u64 {
    let raw = env::var("NUR_AUTO_UPDATE_TTL_SECS")
        .or_else(|_| env::var("META_AUTO_UPDATE_TTL_SECS"))
        .ok();
    parse_min_interval(raw.as_deref())
}

/// Garbage in the env must never disable the feature or spin the network — fall
/// back to the default floor on anything unparseable.
fn parse_min_interval(raw: Option<&str>) -> u64 {
    match raw {
        Some(v) => v
            .trim()
            .parse::<u64>()
            .unwrap_or(AUTO_UPDATE_MIN_INTERVAL_SECS),
        None => AUTO_UPDATE_MIN_INTERVAL_SECS,
    }
}

/// Pure launch gate so the "does it actually check?" policy is unit-testable.
/// `now < last_check_at` means the wall clock moved backwards (NTP correction,
/// dual-boot); treating that as "due" beats being wedged shut for hours.
fn auto_update_due(last_check_at: u64, now: u64, min_interval: u64) -> bool {
    if last_check_at == 0 || min_interval == 0 || now < last_check_at {
        return true;
    }
    now - last_check_at >= min_interval
}

/// Delete abandoned partial downloads from the install dir. Returns how many
/// were removed. A temp still being written by a *live* sibling process is
/// younger than `max_age_secs`, so an in-flight download is never yanked.
fn cleanup_stale_update_temps(dir: &Path, max_age_secs: u64) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0usize;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with(UPDATE_TMP_PREFIX) {
            continue;
        }
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let age_ok = entry
            .metadata()
            .and_then(|m| m.modified())
            .map(|m| {
                m.elapsed()
                    .map(|d| d.as_secs() >= max_age_secs)
                    // Future mtime (clock skew) — old enough to be junk.
                    .unwrap_or(true)
            })
            // No mtime available: err on the side of cleaning up.
            .unwrap_or(true);
        if age_ok && fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AutoUpdateState {
    #[serde(default)]
    pub last_check_at: u64,
    #[serde(default)]
    pub last_remote_version: String,
    #[serde(default)]
    pub last_result: String,
}

enum UpdateOutcome {
    Updated { version: String },
    AlreadyCurrent { version: String },
}

pub(crate) fn auto_update_state_path() -> PathBuf {
    config::muse_home().join("auto_update.json")
}

pub(crate) fn load_auto_update_state() -> AutoUpdateState {
    fs::read_to_string(auto_update_state_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_auto_update_state(st: &AutoUpdateState) {
    let _ = config::ensure_dirs();
    if let Ok(text) = serde_json::to_string_pretty(st) {
        let _ = fs::write(auto_update_state_path(), text);
    }
}

/// Called on **every** launch — bare TUI, `nur run …`, `nur "prompt"`,
/// gateway, everything. If a newer GitHub release exists, install it in the
/// background so the *next* launch runs it. Returns `true` when the caller
/// should **exit** (reserved for a future re-exec handoff; today always false).
///
/// Strictly non-blocking and soft-failing: the network call happens on a
/// detached OS thread, so a headless invocation is never delayed and can never
/// fail because api.github.com is unreachable. The tradeoff is that a very
/// short headless run may exit before the download lands — that is fine, the
/// check is retried next launch and the partial file is swept up (see
/// `cleanup_stale_update_temps`).
///
/// Off when:
/// - `config.auto_update = false` (caller passes the flag)
/// - `NUR_SKIP_AUTO_UPDATE` / `META_SKIP_AUTO_UPDATE` / `NUR_DISABLE_UPDATES` /
///   `DISABLE_UPDATES` / `DISABLE_AUTOUPDATER` is set (the last is Claude Code's,
///   present only inside agent sessions — see [`auto_update_opt_out_var`])
/// - running a release artifact or an un-installed build (bootstrap owns those)
/// - the last check was under the floor (default 60s, `NUR_AUTO_UPDATE_TTL_SECS`)
///
/// `NUR_AUTO_UPDATE_BLOCKING=1` waits for the check to finish instead of
/// detaching — for scripts and for proving this path end to end.
pub fn maybe_auto_update_on_launch(enabled: bool) -> bool {
    if !enabled {
        return false;
    }
    if auto_update_opt_out_var().is_some() {
        return false;
    }
    // First-run / release EXE: bootstrap already handles install.
    if looks_like_release_artifact() {
        return false;
    }
    // A foreground one-stop install is about to rewrite the same binary — two
    // writers on ~/.local/bin/nur.exe is how you end up with a bricked PATH.
    if should_bootstrap_on_launch() {
        return false;
    }
    // Only auto-update when the product is installed (or we are it).
    if !install_binary_path().is_file() && !is_running_from_install() {
        return false;
    }

    let st = load_auto_update_state();
    // Throttle on the timestamp alone. Keying this on `last_result == "current"`
    // meant a failed check re-downloaded on every single launch — invisible now
    // that the attempt happens on a background thread.
    if !auto_update_due(
        st.last_check_at,
        now_secs(),
        auto_update_min_interval_secs(),
    ) {
        return false;
    }

    // Non-blocking: spawn background thread so startup stays instant.
    // Previously this did a blocking 8s HTTP call to api.github.com on the main thread,
    // adding 1s+ latency to every cold launch when cache stale.
    // Now we return immediately and let background handle it; next launch uses new binary.
    let handle = std::thread::Builder::new()
        .name("nur-auto-update".into())
        .spawn(move || {
            // Sweep first: a previous run that exited mid-download left a partial
            // body in the install dir, and those accumulate forever otherwise.
            cleanup_stale_update_temps(&install_dir(), UPDATE_TMP_MAX_AGE_SECS);
            // Re-load state inside thread to avoid race, use fresh now
            let now_inner = now_secs();
            let mut st_inner = load_auto_update_state();
            match try_install_from_github(false) {
                Ok(UpdateOutcome::Updated { version }) => {
                    st_inner.last_check_at = now_inner;
                    st_inner.last_remote_version = version.clone();
                    st_inner.last_result = "updated".into();
                    save_auto_update_state(&st_inner);
                    let dest = install_binary_path();
                    let marker = BootstrapMarker {
                        schema: BOOTSTRAP_SCHEMA,
                        version: version.clone(),
                        binary: dest.display().to_string(),
                        completed_at: now_inner,
                        ecosystem_ok: true,
                    };
                    if let Ok(text) = serde_json::to_string_pretty(&marker) {
                        let _ = fs::write(marker_path(), text);
                    }
                    // No re-exec and no printing: the TUI owns the alternate
                    // screen by the time this lands, so stdout here would
                    // scribble over the render. The state file carries the
                    // result to the next launch.
                }
                Ok(UpdateOutcome::AlreadyCurrent { version }) => {
                    st_inner.last_check_at = now_inner;
                    st_inner.last_remote_version = version;
                    st_inner.last_result = "current".into();
                    save_auto_update_state(&st_inner);
                }
                Err(e) => {
                    st_inner.last_check_at = now_inner;
                    st_inner.last_result = format!("error: {e}");
                    save_auto_update_state(&st_inner);
                }
            }
        })
        .ok();

    // Opt-in synchronous mode. Default stays detached so `nur run …` in a script
    // pays zero latency; scripts that *want* to be on the newest build (and the
    // end-to-end test for this path) set the env var and wait.
    if env_truthy("NUR_AUTO_UPDATE_BLOCKING") {
        if let Some(h) = handle {
            let _ = h.join();
        }
    }

    false
}

/// `nur update --check` — dry run. Says exactly what the auto-updater would do
/// on this machine, right now, and installs nothing. This exists because the
/// real path is deliberately silent and backgrounded, which makes "is it even
/// working?" unanswerable from a shell otherwise.
pub fn run_update_check() -> Result<()> {
    let local = env!("CARGO_PKG_VERSION");
    println!();
    theme::print_info("nur update --check · dry run (nothing will be installed)");
    println!();

    let rel = fetch_latest_release(20)?;
    let tag = rel
        .get("tag_name")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if tag.is_empty() {
        return Err(MuseError::Other("empty release tag".into()));
    }
    let remote = strip_v_prefix(&tag).to_string();
    let assets = release_assets(&rel);
    let picked = pick_nur_release_asset(&assets);

    println!("  local     v{local}");
    println!("  remote    v{remote}   (tag {tag})");
    println!(
        "  platform  {}-{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    match &picked {
        Some((name, url)) => {
            println!("  asset     {name}");
            println!("  url       {url}");
        }
        None => {
            println!("  asset     (none matched this platform)");
            println!(
                "  found     {}",
                if assets.is_empty() {
                    "(release has no assets)".to_string()
                } else {
                    assets
                        .iter()
                        .map(|(n, _)| n.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            );
        }
    }
    println!("  install   {}", install_binary_path().display());
    println!(
        "  interval  every launch, min {}s between network checks (NUR_AUTO_UPDATE_TTL_SECS)",
        auto_update_min_interval_secs()
    );
    println!();

    let newer = version_is_newer(&remote, local);
    if newer && picked.is_some() {
        theme::print_ok(&format!(
            "decision  WOULD UPDATE  v{local} → v{remote}  (run `nur update` to apply)"
        ));
    } else if newer {
        theme::print_err("decision  newer release exists but no asset matches this platform");
    } else if remote == local {
        theme::print_ok(&format!("decision  up to date (v{local})"));
    } else {
        theme::print_ok(&format!(
            "decision  up to date — local v{local} is ahead of the latest release v{remote}"
        ));
    }

    // Show what the launch gate believes, so a stuck throttle is visible too.
    println!();
    for line in auto_update_report() {
        println!("  {line}");
    }
    println!();
    Ok(())
}

/// State lines for `nur doctor` / `nur update --check`: is the launch check
/// enabled, when did it last run, and what did it decide?
pub fn auto_update_report() -> Vec<String> {
    let mut out = Vec::new();
    let enabled = config::load_config().map(|c| c.auto_update).unwrap_or(true);
    let skipped = auto_update_opt_out_var();
    out.push(match (enabled, skipped) {
        (false, _) => "auto-update  off (config.auto_update = false)".to_string(),
        (true, Some(k)) => format!("auto-update  off for this shell ({k} is set)"),
        (true, None) => format!(
            "auto-update  on · every launch · min {}s between checks",
            auto_update_min_interval_secs()
        ),
    });

    let st = load_auto_update_state();
    if st.last_check_at == 0 {
        out.push("last check   never (no ~/.nur/auto_update.json yet)".to_string());
    } else {
        let ago = now_secs().saturating_sub(st.last_check_at);
        let stamp = chrono::DateTime::from_timestamp(st.last_check_at as i64, 0)
            .map(|dt| {
                dt.with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string()
            })
            .unwrap_or_else(|| st.last_check_at.to_string());
        out.push(format!("last check   {stamp}  ({} ago)", human_secs(ago)));
        out.push(format!(
            "last remote  v{}",
            if st.last_remote_version.is_empty() {
                "(unknown)"
            } else {
                st.last_remote_version.as_str()
            }
        ));
        out.push(format!(
            "last result  {}",
            if st.last_result.is_empty() {
                "(none)"
            } else {
                st.last_result.as_str()
            }
        ));
    }
    out.push(format!(
        "state file   {}",
        auto_update_state_path().display()
    ));
    out
}

fn human_secs(s: u64) -> String {
    if s < 90 {
        format!("{s}s")
    } else if s < 90 * 60 {
        format!("{}m", s / 60)
    } else if s < 48 * 3600 {
        format!("{}h", s / 3600)
    } else {
        format!("{}d", s / 86400)
    }
}

/// GET the latest release JSON. Shared by the installer and the `--check` dry
/// run so both can never disagree about which release "latest" means.
fn fetch_latest_release(timeout_secs: u64) -> Result<serde_json::Value> {
    let http = http_client(timeout_secs)?;
    http.get(GH_RELEASES_LATEST)
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| MuseError::Other(format!("releases API: {e}")))?
        .error_for_status()
        .map_err(|e| MuseError::Other(format!("releases API: {e}")))?
        .json()
        .map_err(|e| MuseError::Other(format!("releases JSON: {e}")))
}

fn http_client(timeout_secs: u64) -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent(format!("nur-cli/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| MuseError::Other(format!("http client: {e}")))
}

fn release_assets(rel: &serde_json::Value) -> Vec<(String, String)> {
    rel.get("assets")
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
        .unwrap_or_default()
}

/// Query GitHub Releases and install a newer binary when available.
/// `force_verbose` prints status lines (used by `nur update`); the launch path
/// passes `false` and stays **completely silent** — it runs on a background
/// thread while ratatui owns the alternate screen (or while a headless run is
/// streaming to stdout), so any print here corrupts someone's output.
fn try_install_from_github(force_verbose: bool) -> Result<UpdateOutcome> {
    let local = env!("CARGO_PKG_VERSION");
    // Launch path gets a short leash: 8s to notice a new release, then give up
    // until the next run. `nur update` is user-initiated and may wait.
    let http = http_client(if force_verbose { 60 } else { 8 })?;

    if force_verbose {
        step("Checking GitHub Releases…");
    }
    let rel: serde_json::Value = fetch_latest_release(if force_verbose { 60 } else { 8 })?;

    let tag = rel
        .get("tag_name")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .trim();
    if tag.is_empty() {
        return Err(MuseError::Other("empty release tag".into()));
    }
    let remote = strip_v_prefix(tag);
    if !version_is_newer(remote, local) {
        return Ok(UpdateOutcome::AlreadyCurrent {
            version: remote.to_string(),
        });
    }

    let assets = release_assets(&rel);

    let (name, url) = pick_nur_release_asset(&assets)
        .ok_or_else(|| MuseError::Other("no matching release asset for this platform".into()))?;

    if force_verbose {
        step(&format!("Downloading {name} (v{remote})…"));
    }

    let dest_dir = install_dir();
    fs::create_dir_all(&dest_dir)?;
    // Unique per process: two shells launching at once must not write the same
    // temp file and hand each other half an EXE.
    let tmp = dest_dir.join(format!(
        "{UPDATE_TMP_PREFIX}{}-{}{}",
        remote,
        std::process::id(),
        if cfg!(windows) { ".exe.tmp" } else { ".tmp" }
    ));
    let bytes = http
        .get(&url)
        .send()
        .map_err(|e| MuseError::Other(format!("download: {e}")))?
        .error_for_status()
        .map_err(|e| MuseError::Other(format!("download: {e}")))?
        .bytes()
        .map_err(|e| MuseError::Other(format!("download body: {e}")))?;
    // Validate **before** anything touches disk, so a partial or bogus body can
    // never reach `install_binary_safe`. `.bytes()` already errors on a
    // truncated body; these two guards catch the cases that still return 200:
    // a rate-limit/HTML error page, or an asset pointing at the wrong file.
    if bytes.len() < 1_000_000 {
        // Guard against HTML error pages / truncated assets (real EXEs are multi-MB).
        return Err(MuseError::Other(format!(
            "downloaded asset too small ({} bytes) — aborting",
            bytes.len()
        )));
    }
    if !looks_like_native_executable(&bytes) {
        return Err(MuseError::Other(
            "downloaded asset is not a native executable — aborting".into(),
        ));
    }
    fs::write(&tmp, &bytes).map_err(MuseError::Io)?;
    // A short write (disk full) would leave a truncated image that passes the
    // guards above purely because they ran on the in-memory buffer.
    match fs::metadata(&tmp) {
        Ok(m) if m.len() as usize == bytes.len() => {}
        other => {
            let _ = fs::remove_file(&tmp);
            return Err(MuseError::Other(format!(
                "download did not land intact ({} of {} bytes) — aborting",
                other.map(|m| m.len()).unwrap_or(0),
                bytes.len()
            )));
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755));
    }

    let dest = install_binary_path();
    // Remove the temp on the failure path too — otherwise a locked target leaks
    // a multi-MB file into ~/.local/bin on every attempt.
    let installed = install_binary_safe(&tmp, &dest);
    let _ = fs::remove_file(&tmp);
    installed?;
    scrub_legacy_and_impostor_bins(&dest_dir, &dest);
    if let Some(hash) = file_sha256(&dest) {
        let record = format!(
            "{hash}  {}",
            dest.file_name().and_then(|s| s.to_str()).unwrap_or("nur")
        );
        let _ = fs::write(dest_dir.join("nur.sha256"), format!("{record}\n"));
    }
    prepend_path(&dest_dir);

    Ok(UpdateOutcome::Updated {
        version: remote.to_string(),
    })
}

/// Cheap "is this actually a binary for this OS" check on the downloaded body.
/// GitHub can answer 200 with an HTML error page or a redirect stub; installing
/// that over `nur.exe` bricks the tool until the user re-runs the installer.
/// Mach-O has four valid magics (fat/thin × endianness), so macOS checks all.
fn looks_like_native_executable(bytes: &[u8]) -> bool {
    if bytes.len() < 4 {
        return false;
    }
    if cfg!(windows) {
        return &bytes[..2] == b"MZ";
    }
    if cfg!(target_os = "macos") {
        const MACHO: [[u8; 4]; 4] = [
            [0xfe, 0xed, 0xfa, 0xce],
            [0xfe, 0xed, 0xfa, 0xcf],
            [0xce, 0xfa, 0xed, 0xfe],
            [0xcf, 0xfa, 0xed, 0xfe],
        ];
        const FAT: [[u8; 4]; 2] = [[0xca, 0xfe, 0xba, 0xbe], [0xbe, 0xba, 0xfe, 0xca]];
        // Release pipeline may also ship .tar.gz for macOS — gzip magic passes.
        return MACHO.iter().chain(FAT.iter()).any(|m| &bytes[..4] == m)
            || bytes[..2] == [0x1f, 0x8b];
    }
    // Linux: ELF, or a gzip tarball of one.
    &bytes[..4] == b"\x7fELF" || bytes[..2] == [0x1f, 0x8b]
}

fn strip_v_prefix(tag: &str) -> &str {
    tag.strip_prefix('v')
        .or_else(|| tag.strip_prefix('V'))
        .unwrap_or(tag)
}

/// True when `remote` is a strictly greater semver than `local` (major.minor.patch).
fn version_is_newer(remote: &str, local: &str) -> bool {
    let parse = |s: &str| -> Option<(u64, u64, u64)> {
        let mut parts = s.split('.');
        let maj = parts.next()?.parse().ok()?;
        let min = parts.next().unwrap_or("0").parse().unwrap_or(0);
        // Take only numeric prefix of patch (ignore -beta etc.)
        let pat_s = parts.next().unwrap_or("0");
        let pat: u64 = pat_s
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .unwrap_or(0);
        Some((maj, min, pat))
    };
    match (parse(remote), parse(local)) {
        (Some(r), Some(l)) => r > l,
        _ => remote != local && !remote.is_empty(),
    }
}

/// Pick the nur-cli release asset for this OS/arch.
#[cfg_attr(test, allow(dead_code))]
fn pick_nur_release_asset(assets: &[(String, String)]) -> Option<(String, String)> {
    let os = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    let arch = std::env::consts::ARCH;
    // Preferred names used by our release pipeline / docs.
    let preferred: Vec<String> = if cfg!(windows) {
        vec![
            format!("nur-windows-{arch}.exe"),
            format!("nur-windows-{arch}"),
            "nur-windows-x86_64.exe".into(),
        ]
    } else if cfg!(target_os = "macos") {
        vec![
            format!("nur-macos-{arch}"),
            format!("nur-darwin-{arch}"),
            format!("nur-macos-{arch}.tar.gz"),
        ]
    } else {
        vec![
            format!("nur-linux-{arch}"),
            format!("nur-linux-{arch}.tar.gz"),
        ]
    };

    for want in &preferred {
        if let Some((n, u)) = assets.iter().find(|(n, _)| n.eq_ignore_ascii_case(want)) {
            return Some((n.clone(), u.clone()));
        }
    }
    // Fuzzy: any asset that contains both os token and arch.
    assets
        .iter()
        .find(|(n, _)| {
            let l = n.to_ascii_lowercase();
            l.contains("nur")
                && (l.contains(os) || (os == "macos" && l.contains("darwin")))
                && l.contains(arch)
        })
        .map(|(n, u)| (n.clone(), u.clone()))
}

/// After installing from a release artifact, re-exec the installed `nur`
/// so the user lands in the real binary (and PATH-friendly name).
pub fn reexec_installed_tui() -> Result<()> {
    let dest = install_binary_path();
    if !dest.is_file() {
        return Err(MuseError::Other(format!(
            "installed binary missing at {}",
            dest.display()
        )));
    }
    theme::print_info("Opening NurCLI…");
    let status = Command::new(&dest)
        .env("NUR_SKIP_BOOTSTRAP", "1")
        .env("META_SKIP_BOOTSTRAP", "1")
        .status()
        .map_err(|e| MuseError::Other(format!("failed to launch {}: {e}", dest.display())))?;
    if status.success() {
        Ok(())
    } else {
        let code = status.code().unwrap_or(1);
        Err(MuseError::Other(format!("nur exited with status {code}")))
    }
}

fn step(msg: &str) {
    theme::print_info(msg);
}

fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn env_api_key() -> Option<String> {
    for k in [
        "NUR_API_KEY",
        "META_API_KEY",
        "MODEL_API_KEY",
        "MUSE_API_KEY",
    ] {
        if let Ok(v) = env::var(k) {
            let t = v.trim().to_string();
            if !t.is_empty() {
                return Some(t);
            }
        }
    }
    None
}

fn prepend_path(dir: &Path) {
    let dir_s = dir.display().to_string();
    let key = if cfg!(windows) { "Path" } else { "PATH" };
    let cur = env::var_os(key).unwrap_or_default();
    let mut paths = env::split_paths(&cur).collect::<Vec<_>>();
    if paths.iter().any(|p| p == dir) {
        return;
    }
    paths.insert(0, dir.to_path_buf());
    if let Ok(joined) = env::join_paths(paths) {
        env::set_var(key, joined);
    } else {
        // Fallback: crude prepend
        let sep = if cfg!(windows) { ";" } else { ":" };
        env::set_var(key, format!("{dir_s}{sep}{}", cur.to_string_lossy()));
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    if let (Ok(x), Ok(y)) = (fs::canonicalize(a), fs::canonicalize(b)) {
        return x == y;
    }
    paths_equal_loose(a, b)
}

/// Install target is **only** `nur` / `nur.exe`. Never write ourselves as
/// `claude`, `codex`, etc. Remove legacy meta/muse names, and delete any
/// *identical copy* of this binary under foreign agent names (historical bug:
/// Meta CLI was copied over real Claude Code).
fn scrub_legacy_and_impostor_bins(dest_dir: &Path, nur_bin: &Path) {
    for legacy in ["muse.exe", "muse", "meta.exe", "meta"] {
        let _ = fs::remove_file(dest_dir.join(legacy));
    }
    let Some(our_hash) = file_sha256(nur_bin) else {
        return;
    };
    // Well-known foreign agent names that must never be our product binary.
    const FOREIGN: &[&str] = &[
        "claude.exe",
        "claude",
        "codex.exe",
        "codex",
        "cursor.exe",
        "cursor",
        "gemini.exe",
        "gemini",
        "grok.exe",
        "grok",
    ];
    for name in FOREIGN {
        let p = dest_dir.join(name);
        if !p.is_file() {
            continue;
        }
        if same_file(nur_bin, &p) {
            let _ = fs::remove_file(&p);
            continue;
        }
        if let Some(h) = file_sha256(&p) {
            if h == our_hash {
                let _ = fs::remove_file(&p);
                theme::print_info(&format!(
                    "removed impostor {name} (was a copy of nur/meta — restore the real tool if you need it)"
                ));
            }
        }
    }
}

fn install_binary_safe(src: &Path, target: &Path) -> Result<()> {
    // Only ever install as nur — never as claude/codex/etc.
    // Never "install over ourselves" — rename/copy would delete the only image
    // and leave PATH pointing at nothing (os error 2 after rename to .old).
    if same_file(src, target) {
        return Ok(());
    }
    if !src.is_file() {
        return Err(MuseError::Other(format!(
            "source binary missing: {}",
            src.display()
        )));
    }
    match fs::copy(src, target) {
        Ok(_) => Ok(()),
        Err(_) => {
            // Locked by a running instance of *target* — swap via rename, but
            // only when source is a different file that still exists after.
            let bak = target.with_extension("old");
            let _ = fs::remove_file(&bak);
            if target.exists() {
                fs::rename(target, &bak).map_err(|e| {
                    MuseError::Other(format!(
                        "could not replace {} (close other nur sessions): {e}",
                        target.display()
                    ))
                })?;
            }
            if !src.is_file() {
                // Catastrophic: restore target if we renamed it.
                if bak.is_file() {
                    let _ = fs::rename(&bak, target);
                }
                return Err(MuseError::Other(format!(
                    "source vanished while installing {} — restored previous binary if possible",
                    target.display()
                )));
            }
            match fs::copy(src, target) {
                Ok(_) => {
                    let _ = fs::remove_file(&bak);
                    Ok(())
                }
                Err(e) => {
                    if bak.is_file() && !target.is_file() {
                        let _ = fs::rename(&bak, target);
                    }
                    Err(MuseError::Other(format!(
                        "could not install {} (is nur still running?): {e}",
                        target.display()
                    )))
                }
            }
        }
    }
}

fn file_sha256(path: &Path) -> Option<String> {
    #[cfg(windows)]
    {
        let out = Command::new("certutil")
            .args(["-hashfile", &path.display().to_string(), "SHA256"])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        // certutil prints: "SHA256 hash of …:" / hex line / "CertUtil: …"
        for line in text.lines() {
            let t = line.trim();
            if t.len() == 64 && t.chars().all(|c| c.is_ascii_hexdigit()) {
                return Some(t.to_ascii_lowercase());
            }
        }
        None
    }
    #[cfg(not(windows))]
    {
        let out = Command::new("sha256sum")
            .arg(path)
            .output()
            .or_else(|_| {
                Command::new("shasum")
                    .args(["-a", "256"])
                    .arg(path)
                    .output()
            })
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        text.split_whitespace()
            .next()
            .map(|s| s.to_ascii_lowercase())
    }
}

fn ensure_user_path(dir: &Path) -> std::result::Result<bool, String> {
    #[cfg(windows)]
    {
        let dir_s = dir.display().to_string();
        // PowerShell User PATH — same mechanism as install.ps1.
        let ps = format!(
            "$bin = '{}'; $p = [Environment]::GetEnvironmentVariable('Path','User'); if (-not $p) {{ $p = '' }}; if ($p -like ('*' + $bin + '*')) {{ exit 2 }}; [Environment]::SetEnvironmentVariable('Path', ($bin + ';' + $p), 'User'); exit 0",
            dir_s.replace('\'', "''")
        );
        let status = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
            .status()
            .map_err(|e| e.to_string())?;
        match status.code() {
            Some(0) => Ok(true),
            Some(2) => Ok(false),
            other => Err(format!("powershell PATH update exited {other:?}")),
        }
    }
    #[cfg(not(windows))]
    {
        let home = dirs::home_dir().ok_or_else(|| "no home dir".to_string())?;
        let line = r#"export PATH="$HOME/.local/bin:$PATH""#;
        for name in [
            ".zprofile",
            ".zshrc",
            ".bash_profile",
            ".bashrc",
            ".profile",
        ] {
            let rc = home.join(name);
            if !rc.is_file() {
                continue;
            }
            let text = fs::read_to_string(&rc).unwrap_or_default();
            if text.contains(".local/bin") {
                return Ok(false);
            }
            use std::io::Write;
            let mut f = fs::OpenOptions::new()
                .append(true)
                .open(&rc)
                .map_err(|e| e.to_string())?;
            writeln!(f, "\n# nur-cli\n{line}").map_err(|e| e.to_string())?;
            return Ok(true);
        }
        // No rc file — create .profile
        let rc = home.join(".profile");
        use std::io::Write;
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&rc)
            .map_err(|e| e.to_string())?;
        writeln!(f, "\n# nur-cli\n{line}").map_err(|e| e.to_string())?;
        Ok(true)
    }
}

fn which(cmd: &str) -> bool {
    ecosystem::find_bin(cmd).is_some()
}

fn ensure_prereqs_best_effort() {
    // node / bun / uv / rg / ffmpeg — mirror install scripts, never fail hard.
    #[cfg(windows)]
    {
        try_winget_or_note(
            "node",
            "OpenJS.NodeJS.LTS",
            "plur · ruflo · executor · browser",
        );
        if !which("bun") && !which("bun.exe") {
            theme::print_info("installing bun…");
            let _ = Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "irm https://bun.sh/install.ps1 | iex",
                ])
                .status();
            if which("bun") || which("bun.exe") {
                theme::print_ok("bun installed");
            } else {
                theme::print_info("bun not on PATH yet - needed for omp");
            }
        } else {
            theme::print_ok("bun already installed");
        }
        if !which("uv") && !which("uv.exe") {
            theme::print_info("installing uv…");
            let _ = Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "irm https://astral.sh/uv/install.ps1 | iex",
                ])
                .status();
            if which("uv") || which("uv.exe") {
                theme::print_ok("uv installed");
            } else {
                theme::print_info("uv not on PATH yet — needed for graphify");
            }
        } else {
            theme::print_ok("uv already installed");
        }
        try_winget_or_note("rg", "BurntSushi.ripgrep.MSVC", "fast grep / glob");
        try_winget_or_note(
            "ffmpeg",
            "Gyan.FFmpeg",
            "extract_frames / design-from-video",
        );
    }
    #[cfg(not(windows))]
    {
        for (cmd, note) in [
            ("node", "plur · ruflo · executor · browser"),
            ("bun", "omp"),
            ("uv", "graphify"),
            ("rg", "fast grep"),
            ("ffmpeg", "extract_frames"),
        ] {
            if which(cmd) {
                theme::print_ok(&format!("{cmd} already installed"));
            } else {
                theme::print_info(&format!("{cmd} missing — needed for: {note}"));
            }
        }
        // uv official installer (non-interactive)
        if !which("uv") {
            theme::print_info("trying official uv installer…");
            let _ = Command::new("sh")
                .args(["-c", "curl -LsSf https://astral.sh/uv/install.sh | sh"])
                .status();
        }
        if !which("bun") {
            theme::print_info("trying official bun installer…");
            let _ = Command::new("sh")
                .args(["-c", "curl -fsSL https://bun.sh/install | bash"])
                .status();
        }
    }
}

#[cfg(windows)]
fn try_winget_or_note(cmd: &str, winget_id: &str, note: &str) {
    if which(cmd) || which(&format!("{cmd}.exe")) {
        theme::print_ok(&format!("{cmd} already installed"));
        return;
    }
    theme::print_info(&format!("installing {cmd} — {note}…"));
    let status = Command::new("winget")
        .args([
            "install",
            "--id",
            winget_id,
            "-e",
            "--silent",
            "--accept-package-agreements",
            "--accept-source-agreements",
        ])
        .status();
    if status.map(|s| s.success()).unwrap_or(false) && (which(cmd) || which(&format!("{cmd}.exe")))
    {
        theme::print_ok(&format!("{cmd} installed"));
    } else {
        theme::print_info(&format!(
            "{cmd} could not be auto-installed — needed for: {note}"
        ));
    }
}

fn stage_browser_quiet() -> Result<String> {
    use ecosystem::browser_setup as bs;
    if ecosystem::find_bin("agent-browser-cli").is_none() {
        let _ = ecosystem::ensure_ecosystem(false);
    }
    let staged = bs::stage_extension_from_cli().or_else(|| {
        let d = bs::staged_extension_dir();
        d.join("manifest.json").is_file().then_some(d)
    });
    let browser = bs::detect_default_browser();
    match staged {
        Some(dir) => Ok(format!(
            "browser · {} · extension staged at {}",
            browser.label(),
            dir.display()
        )),
        None => Ok(format!(
            "browser · {} · extension not staged yet (run nur browser setup after Node is available)",
            browser.label()
        )),
    }
}

#[cfg(test)]
mod auto_update_tests {
    use super::*;

    #[test]
    fn version_is_newer_semver() {
        assert!(version_is_newer("0.18.7", "0.18.6"));
        assert!(version_is_newer("1.0.0", "0.99.9"));
        assert!(!version_is_newer("0.18.6", "0.18.6"));
        assert!(!version_is_newer("0.18.5", "0.18.6"));
        assert!(version_is_newer("0.19.0", "0.18.99"));
    }

    #[test]
    fn strip_v_prefix_works() {
        assert_eq!(strip_v_prefix("v0.18.7"), "0.18.7");
        assert_eq!(strip_v_prefix("0.18.7"), "0.18.7");
    }

    // ── launch gate ───────────────────────────────────────────────────────
    // The bug this whole change exists to kill: a 6h TTL meant a launch almost
    // never checked. These lock in "checks on every run, with a small floor".

    #[test]
    fn first_ever_launch_always_checks() {
        assert!(auto_update_due(0, 1_700_000_000, 60));
    }

    #[test]
    fn checks_again_once_the_floor_elapses() {
        let last = 1_700_000_000u64;
        // Inside the floor: skip. At/after the floor: check.
        assert!(!auto_update_due(last, last + 1, 60));
        assert!(!auto_update_due(last, last + 59, 60));
        assert!(auto_update_due(last, last + 60, 60));
        assert!(auto_update_due(last, last + 61, 60));
        // The old 6h TTL would have skipped all of these.
        assert!(auto_update_due(last, last + 5 * 60, 60));
    }

    #[test]
    fn zero_interval_checks_every_single_launch() {
        let last = 1_700_000_000u64;
        assert!(auto_update_due(last, last, 0));
        assert!(auto_update_due(last, last + 1, 0));
    }

    #[test]
    fn backwards_clock_does_not_wedge_the_gate() {
        // NTP correction / dual boot: `now` behind `last_check_at` must not
        // subtract-underflow into "wait forever".
        assert!(auto_update_due(1_700_000_000, 1_600_000_000, 60));
    }

    #[test]
    fn min_interval_env_parsing() {
        assert_eq!(parse_min_interval(None), AUTO_UPDATE_MIN_INTERVAL_SECS);
        assert_eq!(parse_min_interval(Some("0")), 0);
        assert_eq!(parse_min_interval(Some("900")), 900);
        assert_eq!(parse_min_interval(Some("  120  ")), 120);
        // Garbage must fall back to the default, never to 0 (network hammer)
        // and never to a huge number (feature asleep again).
        assert_eq!(parse_min_interval(Some("")), AUTO_UPDATE_MIN_INTERVAL_SECS);
        assert_eq!(
            parse_min_interval(Some("nonsense")),
            AUTO_UPDATE_MIN_INTERVAL_SECS
        );
        assert_eq!(
            parse_min_interval(Some("-5")),
            AUTO_UPDATE_MIN_INTERVAL_SECS
        );
    }

    #[test]
    fn default_floor_is_small_enough_to_feel_like_every_run() {
        const { assert!(AUTO_UPDATE_MIN_INTERVAL_SECS <= 60) };
    }

    // ── install dir override ──────────────────────────────────────────────

    #[test]
    fn install_dir_env_override() {
        let scratch = if cfg!(windows) {
            "C:\\scratch\\bin"
        } else {
            "/tmp/scratch/bin"
        };
        assert_eq!(install_dir_from(Some(scratch)), PathBuf::from(scratch));
        // Blank / unset falls back to ~/.local/bin.
        let dflt = install_dir_from(None);
        assert!(dflt.ends_with("bin"));
        assert_eq!(install_dir_from(Some("   ")), dflt);
    }

    // ── stale temp cleanup ────────────────────────────────────────────────

    #[test]
    fn cleanup_removes_only_stale_update_temps() {
        let dir = std::env::temp_dir().join(format!("nur-cleanup-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let temps = [".nur-update-0.1.0-123.exe.tmp", ".nur-update-9.9.9-7.tmp"];
        let keep = ["nur.exe", "nur", "nur.sha256", "claude.exe"];
        for f in temps.iter().chain(keep.iter()) {
            fs::write(dir.join(f), b"x").unwrap();
        }

        // max_age far in the future: nothing is old enough yet — an in-flight
        // download by a live sibling process must survive.
        assert_eq!(cleanup_stale_update_temps(&dir, 3600), 0);
        for f in temps.iter().chain(keep.iter()) {
            assert!(dir.join(f).is_file(), "{f} vanished too early");
        }

        // max_age 0: every leftover temp is fair game, real binaries are not.
        assert_eq!(cleanup_stale_update_temps(&dir, 0), temps.len());
        for f in &temps {
            assert!(!dir.join(f).exists(), "{f} should be gone");
        }
        for f in &keep {
            assert!(dir.join(f).is_file(), "{f} must never be touched");
        }

        // Idempotent on an already-clean dir, and safe on a missing one.
        assert_eq!(cleanup_stale_update_temps(&dir, 0), 0);
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(cleanup_stale_update_temps(&dir, 0), 0);
    }

    // ── partial-download guard ────────────────────────────────────────────

    #[test]
    fn rejects_non_executable_payloads() {
        // The realistic failure: GitHub/CDN answers 200 with HTML.
        assert!(!looks_like_native_executable(
            b"<!DOCTYPE html><html>rate limited"
        ));
        assert!(!looks_like_native_executable(b""));
        assert!(!looks_like_native_executable(b"MZ")); // too short to judge
        assert!(!looks_like_native_executable(
            b"{\"message\":\"Not Found\"}"
        ));
    }

    #[test]
    fn accepts_this_platforms_executable_magic() {
        if cfg!(windows) {
            assert!(looks_like_native_executable(b"MZ\x90\x00\x03\x00"));
            assert!(!looks_like_native_executable(b"\x7fELF\x02\x01"));
        } else if cfg!(target_os = "macos") {
            assert!(looks_like_native_executable(&[
                0xcf, 0xfa, 0xed, 0xfe, 0, 0
            ]));
        } else {
            assert!(looks_like_native_executable(b"\x7fELF\x02\x01"));
            assert!(!looks_like_native_executable(b"MZ\x90\x00"));
        }
    }

    #[test]
    fn real_running_binary_passes_the_magic_check() {
        // Belt and braces: whatever this OS calls an executable, our own image
        // must satisfy the guard — otherwise updates would always self-reject.
        let exe = std::env::current_exe().unwrap();
        let head = fs::read(&exe).unwrap();
        assert!(looks_like_native_executable(&head[..64.min(head.len())]));
    }

    #[test]
    fn pick_windows_asset() {
        let assets = vec![
            ("nur-linux-x86_64".into(), "http://l".into()),
            ("nur-windows-x86_64.exe".into(), "http://w".into()),
            ("nur-macos-aarch64".into(), "http://m".into()),
        ];
        let picked = pick_nur_release_asset(&assets);
        assert!(picked.is_some());
        let (name, url) = picked.unwrap();
        if cfg!(windows) {
            assert_eq!(name, "nur-windows-x86_64.exe");
            assert_eq!(url, "http://w");
        } else if cfg!(target_os = "macos") {
            // may be none if arch mismatch on CI; just ensure no panic
            let _ = (name, url);
        } else {
            assert!(name.contains("linux") || name.contains("windows") || name.contains("macos"));
        }
    }
}

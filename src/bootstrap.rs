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
    let norm = |p: &Path| {
        p.to_string_lossy()
            .replace('/', "\\")
            .to_ascii_lowercase()
    };
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
            prepend_path(&Path::new(&local).join("Microsoft").join("WinGet").join("Links"));
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
        || st.skills_cli.available;
    if st.graphify.available && st.plur.available && st.ruflo.available {
        theme::print_ok("ecosystem ready");
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

/// `nur update` — pull the git checkout, rebuild release, reinstall full stack.
pub fn run_update() -> Result<()> {
    println!();
    theme::print_info("NurCLI — update");
    theme::print_info("git pull · cargo build --release · reinstall stack");
    println!();

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
            .args(["build", "--release"])
            .current_dir(&repo)
            .status()
            .map_err(|e| MuseError::Other(format!("cargo: {e}")))?;
        if !st.success() {
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
    } else {
        // No source tree — repair using the running binary (one-stop self-install).
        return run_full_install();
    }

    // Stack repair via the *newly installed* binary (never re-copy this process).
    let dest = install_binary_path();
    step("Provisioning ecosystem…");
    let _ = Command::new(&dest)
        .args(["ecosystem", "ensure", "--force"])
        .status();
    let _ = Command::new(&dest).args(["browser", "setup"]).status();
    let _ = Command::new(&dest).arg("install-hook").status();

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
    theme::print_ok("Update complete.");
    theme::print_info(&format!("Binary:  {}", dest.display()));
    theme::print_info("Run:     nur");
    println!();
    Ok(())
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
        .map_err(|e| {
            MuseError::Other(format!("failed to launch {}: {e}", dest.display()))
        })?;
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
    for k in ["NUR_API_KEY", "META_API_KEY", "MODEL_API_KEY", "MUSE_API_KEY"] {
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
            .or_else(|_| Command::new("shasum").args(["-a", "256"]).arg(path).output())
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
        try_winget_or_note("node", "OpenJS.NodeJS.LTS", "plur · ruflo · executor · browser");
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
                theme::print_info("bun not on PATH yet — needed for omp");
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
        try_winget_or_note("ffmpeg", "Gyan.FFmpeg", "extract_frames / design-from-video");
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
                .args([
                    "-c",
                    "curl -LsSf https://astral.sh/uv/install.sh | sh",
                ])
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
    if status.map(|s| s.success()).unwrap_or(false)
        && (which(cmd) || which(&format!("{cmd}.exe")))
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

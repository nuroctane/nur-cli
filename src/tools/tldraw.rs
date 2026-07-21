//! tldraw offline integration — official desktop app
//! (github.com/tldraw/tldraw-offline) for interactive `.tldraw` files.
//!
//! Actions:
//!   * `status`  — is the app installed?
//!   * `install` — download + run the official platform installer
//!   * `open`    — launch the app on a `.tldraw`/`.tldr` (robust Windows launch)
//!   * `create`  — write a **valid** `.tldraw` document from a shape list, then open
//!   * `run`     — alias of `open`
//!
//! Models must use `create` (or open an existing valid file). Invented JSON via
//! `write_file` is not a tldraw document and will fail to open usefully.

use super::{arg_str, resolve_path, Tool, ToolContext};
use crate::error::{MuseError, Result};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

const RELEASES_API: &str = "https://api.github.com/repos/tldraw/tldraw-offline/releases/latest";

pub struct Tldraw;

/// Actions that only inspect state (approval-free in manual mode).
pub fn is_read_only_action(args: &str) -> bool {
    let v: Value = serde_json::from_str(args).unwrap_or_else(|_| Value::Object(Default::default()));
    let action = v.get("action").and_then(|a| a.as_str()).unwrap_or("status");
    matches!(action, "status" | "detect")
    // open/create/enable_scripts/api mutate desktop or canvas
}

impl Tool for Tldraw {
    fn name(&self) -> &str {
        "tldraw"
    }

    fn description(&self) -> &str {
        "tldraw offline desktop app + interactive boards (document scripts + agent-shapes). \
         action=status: app + local API (port/token) + open docs. \
         action=install: official installer. \
         action=create: static Desktop .tldraw from shapes (contrast-safe, dark theme). \
         action=open/run: open path and AUTO-ENABLE document scripts (script-workspace → applied). \
         action=enable_scripts: re-enable scripts on an already-open board. \
         action=api: run JS against live canvas {code} (optional path= to pick doc). \
         NEVER invent .tldraw JSON with write_file. Interactive demos (ZIP archives with script/) \
         open with scripts enabled automatically."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["status", "detect", "install", "open", "run", "create", "enable_scripts", "api"],
                    "default": "status"
                },
                "path": {
                    "type": "string",
                    "description": "Filename or path. create → Desktop basename. open → absolute/Desktop/workspace."
                },
                "title": {
                    "type": "string",
                    "description": "Document / page title for create"
                },
                "shapes": {
                    "type": "array",
                    "description": "For create: list of boxes {x,y,w,h,text,color?,geo?}",
                    "items": { "type": "object" }
                },
                "code": {
                    "type": "string",
                    "description": "For action=api: JavaScript with top-level await (api / editor helpers)"
                },
                "spec": {
                    "description": "Legacy: ignored if shapes provided"
                }
            }
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action").unwrap_or_else(|_| "status".into());
        match action.as_str() {
            "status" | "detect" => Ok(status_report()),
            "install" => install(),
            "open" | "run" => open_action(args, &ctx.cwd),
            "create" => create_action(args, &ctx.cwd),
            "enable_scripts" => enable_scripts_action(args, &ctx.cwd),
            "api" => api_action(args, &ctx.cwd),
            other => Err(MuseError::Tool(format!(
                "unknown tldraw action '{other}' — use status|install|open|create|enable_scripts|api"
            ))),
        }
    }
}

// ── app detection ──────────────────────────────────────────────────────────

/// Locate an installed tldraw offline executable / bundle for the current OS.
pub fn app_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    match std::env::consts::OS {
        "windows" => {
            let mut roots = vec![
                home.join("AppData").join("Local").join("Programs"),
                PathBuf::from(r"C:\Program Files"),
                PathBuf::from(r"C:\Program Files (x86)"),
            ];
            if let Ok(lad) = std::env::var("LOCALAPPDATA") {
                roots.push(PathBuf::from(lad).join("Programs"));
            }
            for root in roots {
                if let Some(exe) = scan_for_tldraw_exe(&root) {
                    return Some(exe);
                }
            }
            // Prefer main app over Uninstall / elevate stubs.
            None
        }
        "macos" => {
            let candidates = [
                PathBuf::from("/Applications/tldraw offline.app"),
                PathBuf::from("/Applications/tldraw-offline.app"),
                home.join("Applications").join("tldraw offline.app"),
            ];
            candidates.into_iter().find(|p| p.exists())
        }
        _ => {
            let candidates = [
                home.join(".local").join("bin").join("tldraw-offline"),
                PathBuf::from("/usr/bin/tldraw-offline"),
                PathBuf::from("/opt/tldraw-offline/tldraw-offline"),
            ];
            candidates.into_iter().find(|p| p.exists())
        }
    }
}

/// Look one level deep under `root` for the main `*tldraw*.exe` (skip Uninstall).
fn scan_for_tldraw_exe(root: &Path) -> Option<PathBuf> {
    let dir = std::fs::read_dir(root).ok()?;
    let mut best: Option<PathBuf> = None;
    let mut best_size: u64 = 0;
    for entry in dir.flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if !name.contains("tldraw") {
            continue;
        }
        let sub = entry.path();
        if !sub.is_dir() {
            continue;
        }
        if let Ok(inner) = std::fs::read_dir(&sub) {
            for f in inner.flatten() {
                let fname = f.file_name().to_string_lossy().to_lowercase();
                if !fname.ends_with(".exe") || !fname.contains("tldraw") {
                    continue;
                }
                if fname.contains("uninstall") || fname.contains("elevate") {
                    continue;
                }
                let len = f.metadata().map(|m| m.len()).unwrap_or(0);
                // Main Electron app is huge (100MB+); installer stubs are smaller.
                if len > best_size {
                    best_size = len;
                    best = Some(f.path());
                }
            }
        }
    }
    best
}

// ── release resolution + install ───────────────────────────────────────────

fn asset_pattern() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "aarch64") => "tldraw-offline-win-arm64.exe",
        ("windows", _) => "tldraw-offline-win-x64.exe",
        ("macos", _) => "tldraw-offline-mac-universal.dmg",
        ("linux", "aarch64") => "tldraw-offline-linux-arm64.AppImage",
        ("linux", _) => "tldraw-offline-linux-x86_64.AppImage",
        _ => "tldraw-offline-win-x64.exe",
    }
}

fn latest_asset_url() -> Result<(String, String)> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("nur-cli")
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| MuseError::Tool(format!("http client: {e}")))?;
    let body: Value = client
        .get(RELEASES_API)
        .send()
        .map_err(|e| MuseError::Tool(format!("fetch releases: {e}")))?
        .json()
        .map_err(|e| MuseError::Tool(format!("parse releases: {e}")))?;
    let tag = body
        .get("tag_name")
        .and_then(|t| t.as_str())
        .unwrap_or("latest")
        .to_string();
    let want = asset_pattern();
    let url = body
        .get("assets")
        .and_then(|a| a.as_array())
        .and_then(|arr| {
            arr.iter().find_map(|asset| {
                let name = asset.get("name").and_then(|n| n.as_str())?;
                if name == want {
                    asset
                        .get("browser_download_url")
                        .and_then(|u| u.as_str())
                        .map(|s| s.to_string())
                } else {
                    None
                }
            })
        })
        .ok_or_else(|| {
            MuseError::Tool(format!("no release asset '{want}' found for this platform"))
        })?;
    Ok((tag, url))
}

pub fn ensure_installed() -> Result<String> {
    install()
}

pub fn install() -> Result<String> {
    if let Some(app) = app_path() {
        return Ok(format!(
            "tldraw offline already installed: {}\n(use action=open path=… to open a file)",
            app.display()
        ));
    }
    let (tag, url) = latest_asset_url()?;
    let client = reqwest::blocking::Client::builder()
        .user_agent("nur-cli")
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|e| MuseError::Tool(format!("http client: {e}")))?;
    let bytes = client
        .get(&url)
        .send()
        .map_err(|e| MuseError::Tool(format!("download installer: {e}")))?
        .bytes()
        .map_err(|e| MuseError::Tool(format!("read installer body: {e}")))?;

    let fname = url.rsplit('/').next().unwrap_or("tldraw-offline-installer");
    let dl = std::env::temp_dir().join(fname);
    {
        let mut f = std::fs::File::create(&dl)
            .map_err(|e| MuseError::Tool(format!("write installer: {e}")))?;
        f.write_all(&bytes)
            .map_err(|e| MuseError::Tool(format!("save installer: {e}")))?;
    }

    let mut s = format!("downloaded tldraw offline {tag} → {}\n", dl.display());
    match std::env::consts::OS {
        "windows" => {
            // Do NOT use /S first — silent install sometimes leaves a broken
            // session. Prefer interactive so the user completes setup; still
            // try silent as a second launch if needed.
            match std::process::Command::new(&dl).spawn() {
                Ok(_) => s.push_str(
                    "launched the installer window — finish setup, then re-run action=status.\n",
                ),
                Err(e) => s.push_str(&format!(
                    "could not launch installer ({e}) — run manually: {}\n",
                    dl.display()
                )),
            }
        }
        _ => {
            let _ = crate::open_uri::open_path(&dl);
            s.push_str(
                "opened the installer/image — complete installation, then re-run action=status.\n",
            );
        }
    }
    Ok(s)
}

// ── open ───────────────────────────────────────────────────────────────────

fn open_action(args: &Value, cwd: &Path) -> Result<String> {
    let path = arg_str(args, "path")
        .map_err(|_| MuseError::Tool("open requires path= to a .tldraw file".into()))?;
    let abs = resolve_open_path(cwd, &path)?;
    if !abs.is_file() {
        return Err(MuseError::Tool(format!(
            "file not found: {}\n  (create saves boards to Desktop — try opening from there)",
            abs.display()
        )));
    }
    launch_on_file(&abs)
}

/// User's Desktop directory (Windows/macOS/Linux via `dirs`).
pub fn desktop_dir() -> PathBuf {
    dirs::desktop_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Desktop")))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Resolve a path for `open`: absolute as-is; else try Desktop first, then workspace.
fn resolve_open_path(cwd: &Path, path: &str) -> Result<PathBuf> {
    let p = PathBuf::from(path);
    if p.is_absolute() {
        return Ok(p);
    }
    let desktop = desktop_dir().join(path);
    if desktop.is_file() {
        return Ok(desktop);
    }
    // Bare filename on Desktop
    if let Some(name) = p.file_name() {
        let desk_name = desktop_dir().join(name);
        if desk_name.is_file() {
            return Ok(desk_name);
        }
    }
    // Fall back to workspace (sandbox-checked)
    resolve_path(&cwd.to_path_buf(), path)
}

/// Resolve output path for `create` — **always under Desktop**.
///
/// Relative paths and bare names become `Desktop/<basename>.tldraw`.
/// Absolute paths outside Desktop still land on Desktop using the file name
/// so boards stay easy to find.
fn resolve_create_path(args: &Value) -> Result<PathBuf> {
    let desktop = desktop_dir();
    let _ = std::fs::create_dir_all(&desktop);

    let title = arg_str(args, "title").unwrap_or_else(|_| "Board".into());
    let raw = arg_str(args, "path").ok();

    let mut name = match raw.as_deref() {
        Some(p) if !p.trim().is_empty() => {
            let pb = PathBuf::from(p);
            pb.file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| slug_filename(&title))
        }
        _ => slug_filename(&title),
    };
    if name.is_empty() {
        name = "board.tldraw".into();
    }
    let lower = name.to_ascii_lowercase();
    if !(lower.ends_with(".tldraw") || lower.ends_with(".tldr")) {
        name.push_str(".tldraw");
    }

    Ok(desktop.join(name))
}

fn slug_filename(title: &str) -> String {
    let mut s: String = title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else if c.is_whitespace() || c == '-' || c == '_' {
                '-'
            } else {
                '-'
            }
        })
        .collect();
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "board".into()
    } else {
        // Cap length for Windows path comfort
        if s.len() > 48 {
            s.truncate(48);
            s = s.trim_end_matches('-').to_string();
        }
        s
    }
}

/// Shared launcher used by the tool and `/draw <file>`.
///
/// Important Windows behavior (verified):
/// - Launching the official Electron app **with an invalid `.tldraw`** starts
///   processes but often produces **no MainWindowHandle** (invisible app).
/// - `.tldraw` is frequently **not registered** with a file association, so
///   `cmd start file.tldraw` alone is unreliable.
/// - Reliable path: `cmd /c start "" "app.exe" [valid-file]` (or bare app).
pub fn launch_on_file(abs: &Path) -> Result<String> {
    let abs = abs.canonicalize().unwrap_or_else(|_| abs.to_path_buf());
    let notes = validate_or_hint(&abs);
    let file_is_valid = notes.is_empty();

    // Dark canvas before the app reads prefs — white labels stay readable.
    ensure_dark_theme();

    if app_path().is_none() {
        let _ = crate::open_uri::open_path(&abs);
        return Err(MuseError::Tool(
            "tldraw offline is not installed — run action=install first, then open again.\n\
             Or drag the file onto https://www.tldraw.com/ in a browser."
                .into(),
        ));
    }

    let mut methods: Vec<String> = Vec::new();
    let mut opened = false;

    // Only pass the path when the document is valid (ZIP archive OR JSON snapshot).
    let file_arg = if file_is_valid {
        Some(abs.as_path())
    } else {
        None
    };

    // 1) Shell open of the *file* (Invoke-Item / association) — most reliable
    //    for landing on the correct board (single-instance app).
    if file_is_valid {
        match crate::open_uri::open_path(&abs) {
            Ok(()) => {
                methods.push("shell open file".into());
                opened = true;
            }
            Err(e) => methods.push(format!("shell open failed: {e}")),
        }
    }

    // 2) PowerShell Start-Process app+file
    if !opened {
        if let Some(app) = app_path() {
            match spawn_via_powershell_start(&app, file_arg) {
                Ok(()) => {
                    methods.push("powershell Start-Process".into());
                    opened = true;
                }
                Err(e) => methods.push(format!("powershell start failed: {e}")),
            }
        }
    }

    // 3) cmd start / plain spawn
    if !opened {
        if let Some(app) = app_path() {
            if spawn_via_shell_start(&app, file_arg).is_ok()
                || spawn_app_plain(&app, file_arg).is_ok()
            {
                methods.push("cmd/direct spawn".into());
                opened = true;
            }
        }
    }

    if !opened {
        return Err(MuseError::Tool(format!(
            "could not launch tldraw offline for {}\n  tried: {}",
            abs.display(),
            methods.join(" · ")
        )));
    }

    let mut out = format!(
        "opened tldraw offline (dark theme) for {}\n  methods: {}\n",
        abs.display(),
        methods.join(" · ")
    );
    if !notes.is_empty() {
        out.push_str(&notes);
        out.push('\n');
        out.push_str(
            "App launched without that file (invalid docs). \
             Use tldraw(action=create, …) or open a real interactive ZIP .tldraw.\n",
        );
        return Ok(out);
    }

    // Auto-enable document scripts (script-workspace → state applied).
    // Without this, hasScript boards open as not-watching and agent scripts look dead.
    match enable_scripts_for_path(&abs, Duration::from_secs(35)) {
        Ok(report) => {
            out.push_str(&report);
        }
        Err(e) => {
            out.push_str(&format!(
                "scripts: not enabled yet ({e})\n  \
                 Re-run tldraw(action=enable_scripts, path=…) after the window appears, \
                 or wait for localhost:7236 (see %APPDATA%\\tldraw\\server.json).\n"
            ));
        }
    }
    out.push_str("Look for the tldraw offline window (Alt+Tab if needed).\n");
    Ok(out)
}

/// Detach GUI via PowerShell so Electron is not a child of nur's console/job.
fn spawn_via_powershell_start(app: &Path, file: Option<&Path>) -> std::result::Result<(), String> {
    let app_s = app.to_string_lossy().replace('\'', "''");
    let script = match file {
        Some(f) => {
            let f_s = f.to_string_lossy().replace('\'', "''");
            format!(
                "Start-Process -FilePath '{app_s}' -ArgumentList @('{f_s}') -WorkingDirectory (Split-Path -Parent '{app_s}')"
            )
        }
        None => format!(
            "Start-Process -FilePath '{app_s}' -WorkingDirectory (Split-Path -Parent '{app_s}')"
        ),
    };
    let status = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &script,
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("powershell exited {status}"))
    }
}

/// Windows: `cmd /c start "" app [file]` so the GUI is owned by Explorer's
/// desktop session. Empty title arg is required so paths are not mis-parsed.
fn spawn_via_shell_start(app: &Path, file: Option<&Path>) -> std::result::Result<(), String> {
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("cmd.exe");
        cmd.arg("/C").arg("start").arg("").arg(app.as_os_str());
        if let Some(f) = file {
            cmd.arg(f.as_os_str());
        }
        cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        spawn_app_plain(app, file)
    }
}

/// Plain CreateProcess / exec — break away from nur's job so CLI exit cannot
/// kill Electron. Avoid DETACHED_PROCESS alone (can leave MainWindowHandle=0).
fn spawn_app_plain(app: &Path, file: Option<&Path>) -> std::result::Result<(), String> {
    let mut cmd = std::process::Command::new(app);
    if let Some(f) = file {
        cmd.arg(f);
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB);
    }
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

fn validate_or_hint(path: &Path) -> String {
    // Official offline docs can be:
    // 1) ZIP archive (PK..) containing db.sqlite + script/ — interactive boards
    // 2) JSON snapshot with tldrawFileFormatVersion + records — static boards
    if let Ok(mut f) = std::fs::File::open(path) {
        let mut magic = [0u8; 4];
        use std::io::Read;
        if f.read_exact(&mut magic).is_ok() && &magic == b"PK\x03\x04" {
            return String::new(); // valid ZIP .tldraw
        }
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return "warning: cannot read file".into();
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return "warning: not a ZIP .tldraw archive and not JSON — tldraw may refuse it.".into();
    };
    if v.get("tldrawFileFormatVersion").is_some() && v.get("records").is_some() {
        return String::new();
    }
    "warning: this is NOT a valid .tldraw document (missing tldrawFileFormatVersion + records, \
     and not a ZIP archive). Use tldraw(action=create, …) for static boards, or open a real \
     interactive .tldraw (e.g. nn-digits) produced by the offline app."
        .into()
}

// ── create valid .tldraw ───────────────────────────────────────────────────

fn create_action(args: &Value, _cwd: &Path) -> Result<String> {
    // Always Desktop — not workspace (sandbox would block Desktop otherwise).
    ensure_dark_theme();
    let abs = resolve_create_path(args)?;
    if let Some(parent) = abs.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let title = arg_str(args, "title").unwrap_or_else(|_| "Board".into());
    let shapes = args
        .get("shapes")
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();

    if shapes.is_empty() {
        // Persist legacy spec for debugging, but still write an empty valid board.
        if let Some(spec) = args.get("spec") {
            let spec_path = abs.with_extension("tldraw.spec.json");
            let _ = std::fs::write(
                &spec_path,
                serde_json::to_string_pretty(spec).unwrap_or_default(),
            );
        }
    }

    let doc = build_tldraw_document(&title, &shapes);
    let body = serde_json::to_string_pretty(&doc)
        .map_err(|e| MuseError::Tool(format!("serialize tldraw: {e}")))?;
    std::fs::write(&abs, body).map_err(|e| MuseError::Tool(format!("write tldraw: {e}")))?;

    let mut out = format!(
        "wrote valid .tldraw ({} shapes) → Desktop\n  {}\n",
        shapes.len(),
        abs.display()
    );
    match launch_on_file(&abs) {
        Ok(launch) => {
            out.push_str(&launch);
        }
        Err(e) => {
            out.push_str(&format!("open failed: {e}\n"));
            out.push_str(
                "File is on your Desktop — double-click it, or drag onto https://www.tldraw.com/\n",
            );
        }
    }
    Ok(out)
}

/// Build a valid tldraw file for the offline app (v1.11+ / geo schema 11).
///
/// Critical: modern geo shapes use **`richText`**, not plain `text`.
/// Files with `props.text` load as a blank canvas (validation strips shapes).
fn build_tldraw_document(title: &str, shapes: &[Value]) -> Value {
    let mut records: Vec<Value> = vec![
        json!({
            "id": "document:document",
            "typeName": "document",
            "gridSize": 10,
            "name": title,
            "meta": {}
        }),
        json!({
            "id": "page:page",
            "typeName": "page",
            "name": title,
            "index": "a1",
            "meta": {}
        }),
    ];

    let mut min_x = f64::MAX;
    let mut min_y = f64::MAX;
    let mut max_x = f64::MIN;
    let mut max_y = f64::MIN;

    for (i, s) in shapes.iter().enumerate() {
        let id = s
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| {
                if s.starts_with("shape:") {
                    s.to_string()
                } else {
                    format!("shape:{s}")
                }
            })
            .unwrap_or_else(|| format!("shape:box{i}"));
        let x = num(s, "x", 80.0 + (i as f64 % 4.0) * 240.0);
        let y = num(s, "y", 80.0 + (i as f64 / 4.0).floor() * 160.0);
        let w = num(s, "w", 200.0).max(40.0);
        let h = num(s, "h", 100.0).max(40.0);
        let text = s
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let color = s
            .get("color")
            .and_then(|v| v.as_str())
            .map(normalize_color)
            .unwrap_or_else(|| "blue".into());
        // High-contrast under dark theme (pastel solid fills + white text = invisible).
        let (color, label_color, fill) = contrast_style(&color);
        let geo = s.get("geo").and_then(|v| v.as_str()).unwrap_or("rectangle");
        // Must be a valid tldraw IndexKey. "a10" is REJECTED and blanks the
        // whole canvas with ValidationError — use a1..a9, aA..aZ, b1…
        let index = fractional_index(i);

        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x + w);
        max_y = max_y.max(y + h);

        records.push(json!({
            "id": id,
            "typeName": "shape",
            "type": "geo",
            "x": x,
            "y": y,
            "rotation": 0,
            "index": index,
            "parentId": "page:page",
            "isLocked": false,
            "opacity": 1,
            "props": {
                "geo": geo,
                "w": w,
                "h": h,
                "growY": 0,
                "richText": to_rich_text(&text),
                "labelColor": label_color,
                "color": color,
                "fill": fill,
                "dash": "solid",
                "size": "m",
                "font": "sans",
                "align": "middle",
                "verticalAlign": "middle",
                "url": "",
                "scale": 1
            },
            "meta": {}
        }));
    }

    // Session records so the viewport shows the shapes (not a blank far-away camera).
    let (cam_x, cam_y) = if shapes.is_empty() {
        (0.0, 0.0)
    } else {
        (-min_x + 40.0, -min_y + 40.0)
    };
    records.push(json!({
        "id": "camera:page:page",
        "typeName": "camera",
        "x": cam_x,
        "y": cam_y,
        "z": 1,
        "meta": {}
    }));
    records.push(json!({
        "id": "pointer:pointer",
        "typeName": "pointer",
        "x": 0,
        "y": 0,
        "lastActivityTimestamp": 0,
        "meta": {}
    }));
    records.push(json!({
        "id": "instance:instance",
        "typeName": "instance",
        "currentPageId": "page:page",
        "exportBackground": true,
        "isFocusMode": false,
        "isDebugMode": false,
        "isToolLocked": false,
        "isGridMode": false,
        "canMoveCamera": true,
        "isPenMode": false,
        "isReadonly": false,
        "openMenus": [],
        "followingUserId": null,
        "highlightedUserIds": [],
        "brush": null,
        "cursor": { "type": "default", "rotation": 0 },
        "opacityForNextShape": 1,
        "stylesForNextShape": {},
        "meta": {},
        "duplicateProps": null,
        "screenBounds": { "x": 0, "y": 0, "w": 1400, "h": 900 },
        "insets": [false, false, false, false],
        "chatMessage": "",
        "isChatting": false,
        "isFocused": true,
        "devicePixelRatio": 1,
        "isCoarsePointer": false,
        "isHoveringCanvas": null,
        "openDialog": null,
        "isChangingStyle": false,
        "isSnapping": false
    }));
    records.push(json!({
        "id": "instance_page_state:page:page",
        "typeName": "instance_page_state",
        "pageId": "page:page",
        "selectedShapeIds": [],
        "hintingShapeIds": [],
        "erasingShapeIds": [],
        "hoveredShapeId": null,
        "editingShapeId": null,
        "croppingShapeId": null,
        "focusedGroupId": null,
        "meta": {}
    }));

    // Schema versions match tldraw offline 1.11 (geo=11 requires richText).
    json!({
        "tldrawFileFormatVersion": 1,
        "schema": {
            "schemaVersion": 2,
            "sequences": {
                "com.tldraw.store": 5,
                "com.tldraw.asset": 1,
                "com.tldraw.camera": 1,
                "com.tldraw.document": 2,
                "com.tldraw.instance": 26,
                "com.tldraw.instance_page_state": 5,
                "com.tldraw.page": 1,
                "com.tldraw.instance_presence": 6,
                "com.tldraw.pointer": 1,
                "com.tldraw.shape": 4,
                "com.tldraw.user": 1,
                "com.tldraw.asset.bookmark": 2,
                "com.tldraw.asset.image": 6,
                "com.tldraw.asset.video": 5,
                "com.tldraw.shape.arrow": 8,
                "com.tldraw.shape.bookmark": 2,
                "com.tldraw.shape.draw": 5,
                "com.tldraw.shape.embed": 4,
                "com.tldraw.shape.frame": 1,
                "com.tldraw.shape.geo": 11,
                "com.tldraw.shape.group": 0,
                "com.tldraw.shape.highlight": 4,
                "com.tldraw.shape.image": 5,
                "com.tldraw.shape.line": 5,
                "com.tldraw.shape.note": 13,
                "com.tldraw.shape.text": 4,
                "com.tldraw.shape.video": 4,
                "com.tldraw.binding.arrow": 1
            }
        },
        "records": records
    })
}

/// TipTap/ProseMirror doc used by modern tldraw (`toRichText` equivalent).
fn to_rich_text(text: &str) -> Value {
    if text.is_empty() {
        return json!({
            "type": "doc",
            "content": [{ "type": "paragraph" }]
        });
    }
    let content: Vec<Value> = text
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                json!({ "type": "paragraph" })
            } else {
                json!({
                    "type": "paragraph",
                    "content": [{ "type": "text", "text": line }]
                })
            }
        })
        .collect();
    json!({ "type": "doc", "content": content })
}

/// Generate a tldraw-valid fractional index key for shape ordering.
///
/// Plain `a{n}` breaks at 10: `a10` is not a legal IndexKey and the offline
/// app shows a full-screen error ("Something went wrong") with a blank canvas.
fn fractional_index(i: usize) -> String {
    // a1..a9, aA..aZ, b1..b9, bA..bZ, …
    let major = i / 35; // 9 digits + 26 letters
    let minor = i % 35;
    let head = (b'a' + major as u8) as char;
    let tail = if minor < 9 {
        (b'1' + minor as u8) as char
    } else {
        (b'A' + (minor - 9) as u8) as char
    };
    format!("{head}{tail}")
}

fn num(v: &Value, key: &str, default: f64) -> f64 {
    v.get(key)
        .and_then(|x| x.as_f64().or_else(|| x.as_i64().map(|i| i as f64)))
        .unwrap_or(default)
}

fn normalize_color(c: &str) -> String {
    match c.to_ascii_lowercase().as_str() {
        "gray" | "grey" => "grey".into(),
        "gold" | "yellow" => "yellow".into(),
        "purple" | "violet" => "violet".into(),
        "brown" => "orange".into(), // closest built-in
        "teal" | "cyan" => "light-blue".into(),
        "pink" => "light-red".into(),
        other => other.to_string(),
    }
}

/// Map a requested color to (stroke/fill color, labelColor, fill) that stays readable.
///
/// tldraw **solid** fills are pastel tints of `color`. White labels on those pastels
/// vanish in light mode. We force dark theme on open (see `ensure_dark_theme`) and
/// still pair labels for dark-canvas readability.
fn contrast_style(color: &str) -> (String, String, &'static str) {
    let c = normalize_color(color);
    // Near-white / yellow / light-* : keep light fill, force black text.
    // Everything else: saturated stroke + solid fill + white text (dark theme).
    match c.as_str() {
        "white" => ("grey".into(), "black".into(), "solid"),
        "yellow" | "light-blue" | "light-green" | "light-red" | "light-violet" => {
            (c, "black".into(), "solid")
        }
        // Mid greys: black label is safer on pastel solid
        "grey" => ("grey".into(), "black".into(), "solid"),
        // Strong accents — white label under dark theme
        "black" | "blue" | "green" | "red" | "orange" | "violet" => (c, "white".into(), "solid"),
        other => (other.to_string(), "white".into(), "solid"),
    }
}

/// Force tldraw offline user preference to **dark** so solid fills + white labels
/// stay readable. Best-effort; never fails create/open.
pub fn ensure_dark_theme() {
    let Some(cfg_path) = tldraw_config_path() else {
        return;
    };
    if let Some(parent) = cfg_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut root: Value = std::fs::read_to_string(&cfg_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| {
            json!({
                "version": "2.0.0",
                "userPreferences": {},
                "featureFlags": {}
            })
        });
    if !root.is_object() {
        root = json!({
            "version": "2.0.0",
            "userPreferences": {},
            "featureFlags": {}
        });
    }
    let prefs = root
        .as_object_mut()
        .unwrap()
        .entry("userPreferences")
        .or_insert_with(|| json!({}));
    if let Some(obj) = prefs.as_object_mut() {
        obj.insert("theme".into(), json!("dark"));
        // Keep a visible board background when exporting / sharing
        obj.entry("exportBackground").or_insert(json!(true));
    }
    if let Ok(body) = serde_json::to_string_pretty(&root) {
        let _ = std::fs::write(&cfg_path, body);
    }
}

fn tldraw_config_path() -> Option<PathBuf> {
    // Official offline app stores prefs here (Windows Roaming\tldraw).
    if let Ok(appdata) = std::env::var("APPDATA") {
        return Some(PathBuf::from(appdata).join("tldraw").join("config.json"));
    }
    dirs::config_dir().map(|c| c.join("tldraw").join("config.json"))
}

// ── local canvas API (localhost + server.json) ─────────────────────────────

#[derive(Debug, Clone)]
struct ServerInfo {
    port: u16,
    token: String,
}

fn server_json_path() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        return PathBuf::from(appdata).join("tldraw").join("server.json");
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("tldraw")
        .join("server.json")
}

fn read_server_info() -> Option<ServerInfo> {
    let p = server_json_path();
    let text = std::fs::read_to_string(p).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let port = v.get("port")?.as_u64()? as u16;
    let token = v.get("token")?.as_str()?.to_string();
    if token.is_empty() {
        return None;
    }
    Some(ServerInfo { port, token })
}

fn wait_for_server(timeout: Duration) -> Option<ServerInfo> {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if let Some(s) = read_server_info() {
            // Probe /api/search lightly
            if canvas_api_post(&s, "/api/search", &json!({"code": "return 1"})).is_ok() {
                return Some(s);
            }
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    None
}

fn canvas_api_request(
    server: &ServerInfo,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> Result<Value> {
    let url = format!("http://127.0.0.1:{}{path}", server.port);
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| MuseError::Tool(format!("http client: {e}")))?;
    let mut req = match method {
        "GET" => client.get(&url),
        _ => client.post(&url),
    };
    req = req.header("authorization", format!("Bearer {}", server.token));
    if let Some(b) = body {
        req = req
            .header("content-type", "application/json")
            .body(serde_json::to_string(b).unwrap_or_else(|_| "{}".into()));
    }
    let resp = req
        .send()
        .map_err(|e| MuseError::Tool(format!("canvas API {method} {path}: {e}")))?;
    let status = resp.status();
    let text = resp
        .text()
        .map_err(|e| MuseError::Tool(format!("canvas API body: {e}")))?;
    if !status.is_success() {
        return Err(MuseError::Tool(format!(
            "canvas API {status}: {}",
            text.chars().take(300).collect::<String>()
        )));
    }
    serde_json::from_str(&text).map_err(|e| MuseError::Tool(format!("canvas API json: {e}")))
}

fn canvas_api_post(server: &ServerInfo, path: &str, body: &Value) -> Result<Value> {
    canvas_api_request(server, "POST", path, Some(body))
}

fn canvas_api_get(server: &ServerInfo, path: &str) -> Result<Value> {
    canvas_api_request(server, "GET", path, None)
}

fn path_matches_doc(doc: &Value, abs: &Path) -> bool {
    let abs_s = abs
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();
    let name = abs
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    for key in ["filePath", "name", "displayPath"] {
        if let Some(v) = doc.get(key).and_then(|x| x.as_str()) {
            let vv = v.replace('/', "\\").to_ascii_lowercase();
            if vv == abs_s || vv.ends_with(&abs_s) || vv.contains(&name) {
                return true;
            }
        }
    }
    false
}

/// Pick API doc id. Prefer short `documentId` (stable for /api/doc/…), fall back to full `id`.
fn find_doc_id(server: &ServerInfo, abs: &Path) -> Result<String> {
    let resp = canvas_api_post(
        server,
        "/api/search",
        &json!({"code": "return await api.getDocs()"}),
    )?;
    let docs = resp.get("result").cloned().unwrap_or(resp.clone());
    let arr = docs
        .as_array()
        .ok_or_else(|| MuseError::Tool(format!("getDocs unexpected: {docs}")))?;

    let pick = |d: &Value| -> Option<String> {
        d.get("documentId")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
            .or_else(|| d.get("id").and_then(|x| x.as_str()).map(|s| s.to_string()))
    };

    if let Some(d) = arr.iter().find(|d| path_matches_doc(d, abs)) {
        if let Some(id) = pick(d) {
            return Ok(id);
        }
    }
    if let Some(d) = arr
        .iter()
        .find(|d| d.get("focusOrder").and_then(|x| x.as_i64()) == Some(0))
    {
        if let Some(id) = pick(d) {
            return Ok(id);
        }
    }
    if let Some(d) = arr
        .iter()
        .find(|d| d.get("hasScript").and_then(|x| x.as_bool()) == Some(true))
    {
        if let Some(id) = pick(d) {
            return Ok(id);
        }
    }
    arr.first()
        .and_then(pick)
        .ok_or_else(|| MuseError::Tool("no open documents on canvas API".into()))
}

/// Open script-workspace so document scripts are watched + applied.
/// Returns a human report. Critical for interactive boards (nn-digits, agent-shapes).
pub fn enable_scripts_for_path(abs: &Path, timeout: Duration) -> Result<String> {
    let server = wait_for_server(timeout).ok_or_else(|| {
        MuseError::Tool(
            "canvas API not up — is tldraw offline running? (expect %APPDATA%\\tldraw\\server.json)"
                .into(),
        )
    })?;
    let doc_id = find_doc_id(&server, abs)?;
    // Status before
    let before =
        canvas_api_get(&server, &format!("/api/doc/{doc_id}/script-status")).unwrap_or(json!({}));
    let before_state = before
        .pointer("/result/state")
        .and_then(|x| x.as_str())
        .unwrap_or("?");

    let ws = canvas_api_post(
        &server,
        &format!("/api/doc/{doc_id}/script-workspace"),
        &json!({}),
    )?;
    let is_default = ws
        .pointer("/result/isDefaultScript")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);
    let main_js = ws
        .pointer("/result/mainJsPath")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    let has_script_manifest = ws.pointer("/result/manifest/sha256").is_some();

    // Poll until applied / watching
    let mut final_state = "unknown".to_string();
    let mut watching = false;
    for _ in 0..25 {
        if let Ok(st) = canvas_api_get(&server, &format!("/api/doc/{doc_id}/script-status")) {
            final_state = st
                .pointer("/result/state")
                .and_then(|x| x.as_str())
                .unwrap_or("?")
                .to_string();
            watching = st
                .pointer("/result/watching")
                .and_then(|x| x.as_bool())
                .unwrap_or(false);
            if final_state == "applied"
                || final_state == "error"
                || (watching && final_state != "not-watching")
            {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(400));
    }

    let mut out = format!(
        "scripts: enabled for doc {doc_id}\n  \
         before: {before_state} → after: {final_state} (watching={watching})\n  \
         main.js: {main_js}\n  \
         defaultScript={is_default} manifest={has_script_manifest}\n  \
         api: http://127.0.0.1:{}/  (token in server.json)\n",
        server.port
    );
    if final_state == "applied" {
        out.push_str("  ✓ document scripts are applied and live\n");
    } else if final_state == "error" {
        out.push_str("  ✗ script apply error — check .script-workspace/error.log\n");
    } else {
        out.push_str("  … scripts may still be loading; call enable_scripts again if needed\n");
    }
    Ok(out)
}

fn enable_scripts_action(args: &Value, cwd: &Path) -> Result<String> {
    let path = arg_str(args, "path").unwrap_or_default();
    let abs = if path.is_empty() {
        // Focused / first open doc — still need a path hint for matching; use Desktop empty
        // and let find_doc_id fall through to focusOrder 0
        desktop_dir()
    } else {
        resolve_open_path(cwd, &path)?
    };
    ensure_dark_theme();
    // If app not up, try open first
    if read_server_info().is_none() && !path.is_empty() && abs.is_file() {
        let _ = launch_on_file(&abs);
        return Ok("open+enable_scripts already ran via open\n".into());
    }
    if path.is_empty() {
        let server = wait_for_server(Duration::from_secs(10))
            .ok_or_else(|| MuseError::Tool("canvas API down — open a board first".into()))?;
        let docs = canvas_api_post(
            &server,
            "/api/search",
            &json!({"code": "return await api.getDocs()"}),
        )?;
        let arr = docs
            .get("result")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();
        let id = arr
            .iter()
            .find(|d| d.get("focusOrder").and_then(|x| x.as_i64()) == Some(0))
            .or_else(|| arr.first())
            .and_then(|d| d.get("id"))
            .and_then(|x| x.as_str())
            .ok_or_else(|| MuseError::Tool("no open docs".into()))?;
        // Fake path for report — call workspace directly
        let dummy = PathBuf::from(id);
        return enable_scripts_for_path(&dummy, Duration::from_secs(20));
    }
    enable_scripts_for_path(&abs, Duration::from_secs(25))
}

fn api_action(args: &Value, cwd: &Path) -> Result<String> {
    let code = arg_str(args, "code")
        .map_err(|_| MuseError::Tool("api requires code= JavaScript with await api.…".into()))?;
    let server = wait_for_server(Duration::from_secs(15))
        .ok_or_else(|| MuseError::Tool("canvas API not running — open a .tldraw first".into()))?;
    // optional path just to ensure scripts if given
    if let Ok(path) = arg_str(args, "path") {
        if !path.is_empty() {
            if let Ok(abs) = resolve_open_path(cwd, &path) {
                let _ = enable_scripts_for_path(&abs, Duration::from_secs(10));
            }
        }
    }
    let resp = canvas_api_post(&server, "/api/search", &json!({ "code": code }))?;
    Ok(format!(
        "canvas api ok\n{}\n",
        serde_json::to_string_pretty(&resp).unwrap_or_else(|_| resp.to_string())
    ))
}

fn status_report() -> String {
    let mut s = String::new();
    match app_path() {
        Some(app) => {
            s.push_str(&format!("tldraw offline: INSTALLED\n  {}\n", app.display()));
            s.push_str(&format!(
                "output dir (create): {}\n",
                desktop_dir().display()
            ));
            s.push_str(
                "open:    tldraw(action=open, path=board.tldraw)  # auto-enables document scripts\n",
            );
            s.push_str(
                "create:  tldraw(action=create, title=…, shapes=[…]) → Desktop (static board)\n",
            );
            s.push_str(
                "scripts: tldraw(action=enable_scripts, path=…) · api: action=api code=\"return await api.getDocs()\"\n",
            );
        }
        None => {
            s.push_str("tldraw offline: NOT INSTALLED\n");
            s.push_str("install: tldraw(action=install) — github.com/tldraw/tldraw-offline\n");
        }
    }
    match read_server_info() {
        Some(si) => {
            s.push_str(&format!(
                "canvas API: UP  http://127.0.0.1:{}  (server.json present)\n",
                si.port
            ));
            if let Ok(docs) = canvas_api_post(
                &si,
                "/api/search",
                &json!({"code": "return await api.getDocs()"}),
            ) {
                if let Some(arr) = docs.get("result").and_then(|x| x.as_array()) {
                    s.push_str(&format!("open docs: {}\n", arr.len()));
                    for d in arr.iter().take(6) {
                        let name = d.get("name").and_then(|x| x.as_str()).unwrap_or("?");
                        let hs = d
                            .get("hasScript")
                            .and_then(|x| x.as_bool())
                            .unwrap_or(false);
                        s.push_str(&format!("  - {name}  hasScript={hs}\n"));
                    }
                }
            }
        }
        None => s.push_str("canvas API: down (open a board to start localhost server)\n"),
    }
    match latest_asset_url() {
        Ok((tag, _)) => s.push_str(&format!("latest release: {tag}\n")),
        Err(_) => s.push_str("latest release: (offline / unknown)\n"),
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_document_has_format_version_and_shapes() {
        let shapes =
            vec![json!({"x": 10, "y": 20, "w": 100, "h": 50, "text": "A", "color": "blue"})];
        let doc = build_tldraw_document("Test", &shapes);
        assert_eq!(doc["tldrawFileFormatVersion"], 1);
        let records = doc["records"].as_array().unwrap();
        assert!(records.iter().any(|r| r["typeName"] == "document"));
        assert!(records.iter().any(|r| r["typeName"] == "page"));
        assert!(records.iter().any(|r| r["typeName"] == "camera"));
        let shape = records
            .iter()
            .find(|r| r["typeName"] == "shape")
            .expect("shape");
        // Modern geo schema: richText, not props.text
        assert!(shape["props"].get("text").is_none());
        assert_eq!(
            shape["props"]["richText"]["content"][0]["content"][0]["text"],
            "A"
        );
        assert_eq!(doc["schema"]["sequences"]["com.tldraw.shape.geo"], 11);
    }

    #[test]
    fn to_rich_text_splits_lines() {
        let rt = to_rich_text("Hello\nWorld");
        assert_eq!(rt["type"], "doc");
        assert_eq!(rt["content"].as_array().unwrap().len(), 2);
        assert_eq!(rt["content"][1]["content"][0]["text"], "World");
    }

    #[test]
    fn fractional_index_never_emits_a10() {
        let idxs: Vec<String> = (0..40).map(fractional_index).collect();
        assert_eq!(idxs[0], "a1");
        assert_eq!(idxs[8], "a9");
        assert_eq!(idxs[9], "aA");
        assert!(!idxs.iter().any(|s| s == "a10" || s == "a11"));
        // all unique
        let mut sorted = idxs.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), idxs.len());
    }

    #[test]
    fn validate_detects_fake_schema() {
        let dir = std::env::temp_dir().join(format!("nur-tldraw-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("fake.tldraw");
        std::fs::write(&p, r#"{"schemaVersion":30,"store":{}}"#).unwrap();
        let hint = validate_or_hint(&p);
        assert!(hint.contains("NOT a valid"), "{hint}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn validate_accepts_zip_magic() {
        let dir = std::env::temp_dir().join(format!("nur-tldraw-zip-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("board.tldraw");
        // Minimal ZIP local-file header magic
        std::fs::write(&p, b"PK\x03\x04dummy").unwrap();
        let hint = validate_or_hint(&p);
        assert!(hint.is_empty(), "ZIP .tldraw must be valid, got: {hint}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn create_path_lands_on_desktop() {
        let args = json!({"title": "Car Meet Parking", "path": "subdir/foo.tldraw"});
        let p = resolve_create_path(&args).unwrap();
        assert_eq!(p.parent().unwrap(), desktop_dir());
        assert_eq!(p.file_name().unwrap(), "foo.tldraw");
    }

    #[test]
    fn create_path_from_title_slug() {
        let args = json!({"title": "My Cool Board!"});
        let p = resolve_create_path(&args).unwrap();
        assert_eq!(p.parent().unwrap(), desktop_dir());
        let name = p.file_name().unwrap().to_string_lossy();
        assert!(name.ends_with(".tldraw"), "{name}");
        assert!(name.contains("my-cool-board"), "{name}");
    }

    #[test]
    fn contrast_style_never_white_on_white() {
        let (c, label, _) = contrast_style("white");
        assert_ne!(c, "white");
        assert_eq!(label, "black");
        let (_, label_blue, _) = contrast_style("blue");
        assert_eq!(label_blue, "white");
        let (_, label_yellow, _) = contrast_style("yellow");
        assert_eq!(label_yellow, "black");
    }

    #[test]
    fn build_document_applies_contrast_labels() {
        let shapes = vec![
            json!({"text": "A", "color": "blue"}),
            json!({"text": "B", "color": "yellow"}),
        ];
        let doc = build_tldraw_document("T", &shapes);
        let shapes: Vec<_> = doc["records"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["typeName"] == "shape")
            .collect();
        assert_eq!(shapes[0]["props"]["labelColor"], "white");
        assert_eq!(shapes[1]["props"]["labelColor"], "black");
    }
}

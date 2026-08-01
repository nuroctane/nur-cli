//! Default-browser detection + one-time setup for the `browser` tool.
//!
//! agent-browser-cli drives a real Chromium browser through the
//! `tmwd_cdp_bridge` MV3 extension. To make that usable immediately after
//! install — targeting whatever browser the user actually uses (Arc, Chrome,
//! Edge, Brave, …) — we: (1) detect the default browser, (2) stage the
//! extension files to a stable path so nothing has to be downloaded, and
//! (3) open the browser's extensions page for the single unavoidable
//! "load unpacked" step (a Chromium security boundary we can't script away).

use std::path::{Path, PathBuf};

/// A recognized Chromium-family browser we can target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserKind {
    Arc,
    Chrome,
    Edge,
    Brave,
    Vivaldi,
    Opera,
    Chromium,
    /// Default fell to a non-Chromium browser (Firefox/Safari) — the extension
    /// bridge won't work there; we still tell the user what we found.
    Other,
}

impl BrowserKind {
    pub fn label(self) -> &'static str {
        match self {
            BrowserKind::Arc => "Arc",
            BrowserKind::Chrome => "Chrome",
            BrowserKind::Edge => "Edge",
            BrowserKind::Brave => "Brave",
            BrowserKind::Vivaldi => "Vivaldi",
            BrowserKind::Opera => "Opera",
            BrowserKind::Chromium => "Chromium",
            BrowserKind::Other => "your default browser",
        }
    }

    /// Chromium browsers all honour `chrome://extensions`; Arc also routes it.
    pub fn extensions_url(self) -> &'static str {
        "chrome://extensions"
    }

    pub fn is_chromium(self) -> bool {
        !matches!(self, BrowserKind::Other)
    }
}

/// Map a Windows UserChoice ProgId (or a launcher/exe hint on other OSes) to a
/// browser kind. Pure so it can be unit-tested without touching the registry.
pub fn kind_from_hint(hint: &str) -> BrowserKind {
    let h = hint.to_ascii_lowercase();
    // Arc ships as "TheBrowserCompany.Arc" / "Company.Arc…" ProgIds and `arc`.
    if h.contains("arc") || h.contains("thebrowsercompany") {
        BrowserKind::Arc
    } else if h.contains("brave") {
        BrowserKind::Brave
    } else if h.contains("edge") || h.contains("msedge") {
        BrowserKind::Edge
    } else if h.contains("vivaldi") {
        BrowserKind::Vivaldi
    } else if h.contains("opera") {
        BrowserKind::Opera
    } else if h.contains("chromium") {
        BrowserKind::Chromium
    } else if h.contains("chrome") || h.contains("google") {
        BrowserKind::Chrome
    } else {
        BrowserKind::Other
    }
}

/// Best-effort default-browser detection across platforms.
#[cfg(windows)]
pub fn detect_default_browser() -> BrowserKind {
    // HKCU\…\UrlAssociations\https\UserChoice\ProgId is the source of truth for
    // the user's chosen default; read it via `reg query` to avoid a winreg dep.
    let out = std::process::Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice",
            "/v",
            "ProgId",
        ])
        .output();
    if let Ok(o) = out {
        let text = String::from_utf8_lossy(&o.stdout);
        if let Some(line) = text.lines().find(|l| l.contains("ProgId")) {
            if let Some(progid) = line.split_whitespace().last() {
                let kind = kind_from_hint(progid);
                if kind != BrowserKind::Other {
                    return kind;
                }
            }
        }
    }
    // Arc doesn't always register as the https handler even when preferred —
    // fall back to detecting an Arc install directly.
    if arc_installed_windows() {
        return BrowserKind::Arc;
    }
    BrowserKind::Other
}

#[cfg(windows)]
fn arc_installed_windows() -> bool {
    if let Some(local) = dirs::data_local_dir() {
        // Arc installs under %LOCALAPPDATA%\Arc on Windows.
        if local.join("Arc").is_dir() {
            return true;
        }
    }
    false
}

#[cfg(target_os = "macos")]
pub fn detect_default_browser() -> BrowserKind {
    // Arc is the common case for this project; prefer a direct app-presence
    // check, then fall back to LaunchServices via `defaultbrowser` if present.
    if Path::new("/Applications/Arc.app").exists() {
        return BrowserKind::Arc;
    }
    for (path, kind) in [
        ("/Applications/Google Chrome.app", BrowserKind::Chrome),
        ("/Applications/Microsoft Edge.app", BrowserKind::Edge),
        ("/Applications/Brave Browser.app", BrowserKind::Brave),
        ("/Applications/Vivaldi.app", BrowserKind::Vivaldi),
        ("/Applications/Chromium.app", BrowserKind::Chromium),
    ] {
        if Path::new(path).exists() {
            return kind;
        }
    }
    BrowserKind::Other
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn detect_default_browser() -> BrowserKind {
    // xdg-settings reports the default browser as a .desktop file name.
    let out = std::process::Command::new("xdg-settings")
        .args(["get", "default-web-browser"])
        .output();
    if let Ok(o) = out {
        let name = String::from_utf8_lossy(&o.stdout);
        let kind = kind_from_hint(name.trim());
        if kind != BrowserKind::Other {
            return kind;
        }
    }
    BrowserKind::Other
}

/// Where we stage the unpacked extension so the user (or a launch flag) can
/// load it without downloading anything.
pub fn staged_extension_dir() -> PathBuf {
    super::nur_home()
        .join("browser-extension")
        .join("tmwd_cdp_bridge")
}

/// Copy the `tmwd_cdp_bridge` extension out of the installed agent-browser-cli
/// npm package into a stable staging dir. Returns the staged path on success.
///
/// The CLI ships the extension under `assets/tmwd_cdp_bridge` in its package;
/// we locate the package via the resolved CLI binary and copy the assets.
pub fn stage_extension_from_cli() -> Option<PathBuf> {
    let bin = super::find_bin("agent-browser-cli")?;
    let src = locate_extension_assets(Path::new(&bin))?;
    let dst = staged_extension_dir();
    if let Some(parent) = dst.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // Fresh copy each time so extension updates propagate.
    let _ = std::fs::remove_dir_all(&dst);
    copy_dir(&src, &dst).ok()?;
    if dst.join("manifest.json").is_file() {
        Some(dst)
    } else {
        None
    }
}

/// Walk up from the CLI binary/shim looking for the packaged
/// `assets/tmwd_cdp_bridge` directory (npm global layouts vary by platform).
fn locate_extension_assets(bin: &Path) -> Option<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut cur = bin.parent();
    while let Some(dir) = cur {
        roots.push(dir.to_path_buf());
        // npm global: <prefix>/node_modules/@sleepinsummer/agent-browser-cli
        roots.push(
            dir.join("node_modules")
                .join("@sleepinsummer")
                .join("agent-browser-cli"),
        );
        cur = dir.parent();
        if roots.len() > 40 {
            break;
        }
    }
    for r in roots {
        let cand = r.join("assets").join("tmwd_cdp_bridge");
        if cand.join("manifest.json").is_file() {
            return Some(cand);
        }
    }
    None
}

fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// A short, honest summary of the browser setup state for `nur doctor` / the
/// `browser` tool `status` action.
pub fn setup_summary() -> String {
    let browser = detect_default_browser();
    let staged = staged_extension_dir();
    let have_ext = staged.join("manifest.json").is_file();
    let mut s = format!("default browser: {}\n", browser.label());
    if have_ext {
        s.push_str(&format!("extension staged: {}\n", staged.display()));
    } else {
        s.push_str("extension: not staged yet — run `nur ecosystem ensure`\n");
    }
    if browser.is_chromium() {
        s.push_str(&format!(
            "one-time load: open {} in {} → enable Developer mode → \
             Load unpacked → pick the staged folder above",
            browser.extensions_url(),
            browser.label()
        ));
    } else {
        s.push_str(
            "note: your default browser isn't Chromium — the bridge needs \
             Arc / Chrome / Edge / Brave / Chromium",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progid_maps_to_browser() {
        assert_eq!(kind_from_hint("TheBrowserCompany.Arc"), BrowserKind::Arc);
        assert_eq!(kind_from_hint("ArcHTML.abc123"), BrowserKind::Arc);
        assert_eq!(kind_from_hint("ChromeHTML"), BrowserKind::Chrome);
        assert_eq!(kind_from_hint("MSEdgeHTM"), BrowserKind::Edge);
        assert_eq!(kind_from_hint("BraveHTML"), BrowserKind::Brave);
        assert_eq!(kind_from_hint("FirefoxURL"), BrowserKind::Other);
    }

    #[test]
    fn chromium_family_reports_extensions_url() {
        assert!(BrowserKind::Arc.is_chromium());
        assert!(BrowserKind::Chrome.is_chromium());
        assert!(!BrowserKind::Other.is_chromium());
        assert_eq!(BrowserKind::Arc.extensions_url(), "chrome://extensions");
    }
}

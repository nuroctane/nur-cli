//! terminal-browser integration - https://terminal-browser.com/
//!
//! Upstream ships Apple Silicon macOS builds today (Linux WIP). On Windows we
//! still wire a full tool surface:
//! 1. Native `terminal-browser` / `.exe` when present (future releases)
//! 2. WSL invocation when the binary is installed inside WSL
//! 3. Windows-host fallback via `agent-browser-cli` (same open/snapshot/click
//!    workflow against the user's real Chrome) so agents stay productive

use crate::ecosystem::{find_bin, run_capture, run_capture_cancelled};
use std::path::Path;
use std::process::Command;

pub const BIN: &str = "terminal-browser";
pub const INSTALL_HINT: &str =
    "curl -fsSL https://terminal-browser.sh/install | bash  (Apple Silicon macOS; Linux WIP)";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Runtime {
    /// Official / native CLI on PATH or known install roots.
    Native(String),
    /// Binary lives in WSL; invoke through `wsl.exe`.
    Wsl,
    /// Windows (or any host without the real CLI): agent-browser-cli bridge.
    Host(String),
}

/// Prefer native → WSL → agent-browser-cli host fallback.
pub fn resolve_runtime() -> Option<Runtime> {
    if let Some(bin) = find_native_bin() {
        return Some(Runtime::Native(bin));
    }
    if wsl_has_terminal_browser() {
        return Some(Runtime::Wsl);
    }
    if let Some(bin) = find_bin("agent-browser-cli") {
        return Some(Runtime::Host(bin));
    }
    None
}

#[allow(dead_code)]
pub fn find_terminal_browser() -> Option<String> {
    match resolve_runtime()? {
        Runtime::Native(p) => Some(p),
        Runtime::Wsl => Some("wsl:terminal-browser".into()),
        Runtime::Host(p) => Some(format!("host:{p}")),
    }
}

fn find_native_bin() -> Option<String> {
    if let Some(bin) = find_bin(BIN) {
        return Some(bin);
    }
    #[cfg(windows)]
    {
        for name in [
            "terminal-browser.exe",
            "terminal-browser.cmd",
            "terminal-browser.ps1",
        ] {
            if let Some(bin) = find_bin(name) {
                return Some(bin);
            }
        }
    }
    if let Some(home) = dirs::home_dir() {
        let candidates = [
            home.join(".local").join("bin").join(BIN),
            home.join(".local").join("bin").join("terminal-browser.exe"),
            home.join(".local")
                .join("share")
                .join("terminal-browser")
                .join("app")
                .join("bin")
                .join(BIN),
            home.join("AppData")
                .join("Local")
                .join("terminal-browser")
                .join("bin")
                .join("terminal-browser.exe"),
        ];
        for p in candidates {
            if p.is_file() {
                return Some(p.to_string_lossy().into_owned());
            }
        }
    }
    None
}

fn wsl_available() -> bool {
    #[cfg(windows)]
    {
        Command::new("wsl.exe")
            .args(["-e", "true"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn wsl_has_terminal_browser() -> bool {
    #[cfg(windows)]
    {
        if !wsl_available() {
            return false;
        }
        Command::new("wsl.exe")
            .args([
                "-e",
                "bash",
                "-lc",
                "command -v terminal-browser >/dev/null 2>&1",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub fn doctor_report() -> String {
    let mut lines = Vec::new();
    lines.push("terminal-browser · https://terminal-browser.com/".into());
    match resolve_runtime() {
        Some(Runtime::Native(p)) => {
            lines.push(format!("runtime: native"));
            lines.push(format!("binary: {p}"));
            if let Ok(v) = run_capture(&p, &["--version"], None, 10_000) {
                let first = v.lines().next().unwrap_or(v.trim());
                if !first.is_empty() {
                    lines.push(format!("version: {first}"));
                }
            }
            lines.push(
                "Full in-terminal Chromium. Prefer open split=right, then action -- snapshot."
                    .into(),
            );
        }
        Some(Runtime::Wsl) => {
            lines.push("runtime: wsl".into());
            lines.push("binary: wsl -e terminal-browser".into());
            if let Ok(v) = run_wsl(&["--version"], None, 15_000, None) {
                let first = v.lines().next().unwrap_or(v.trim());
                if !first.is_empty() {
                    lines.push(format!("version: {first}"));
                }
            }
            lines.push(
                "Using terminal-browser inside WSL. Kitty-graphics terminals in WSL \
                 (WezTerm, Ghostty, kitty over SSH) get the real in-terminal browser."
                    .into(),
            );
        }
        Some(Runtime::Host(p)) => {
            lines.push("runtime: windows-host (agent-browser-cli fallback)".into());
            lines.push(format!("bridge: {p}"));
            lines.push(
                "Upstream terminal-browser is macOS (Apple Silicon) today; Linux is WIP. \
                 On Windows, Nur maps the same tool API onto your real Chrome via \
                 agent-browser-cli so open/ls/snapshot/click/fill still work. \
                 Run `nur browser setup` once to load the extension."
                    .into(),
            );
        }
        None => {
            lines.push("runtime: unavailable".into());
            #[cfg(windows)]
            {
                lines.push(
                    "Windows: install agent-browser-cli (`nur ecosystem ensure` / \
                     `nur browser setup`) for the host fallback, or install \
                     terminal-browser inside WSL when a Linux build is available."
                        .into(),
                );
                if wsl_available() {
                    lines.push("WSL: detected (no terminal-browser inside yet)".into());
                } else {
                    lines.push("WSL: not detected".into());
                }
            }
            #[cfg(not(windows))]
            {
                lines.push(format!("Install: {INSTALL_HINT}"));
            }
        }
    }
    lines.push(
        "Distinct from the `browser` tool name - same Chrome bridge is reused only as \
         the Windows-host fallback backend."
            .into(),
    );
    lines.join("\n")
}

pub fn run_tb_cancelled(
    args: &[&str],
    cwd: Option<&Path>,
    timeout_ms: u64,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<String, String> {
    match resolve_runtime() {
        Some(Runtime::Native(bin)) => run_capture_cancelled(&bin, args, cwd, timeout_ms, cancel),
        Some(Runtime::Wsl) => run_wsl(args, cwd, timeout_ms, Some(cancel)),
        Some(Runtime::Host(_)) => run_host(args, cwd, timeout_ms, Some(cancel)),
        None => Err(missing_msg()),
    }
}

fn missing_msg() -> String {
    #[cfg(windows)]
    {
        format!(
            "terminal-browser runtime missing. On Windows: `nur ecosystem ensure` \
             (installs agent-browser-cli host fallback) or install terminal-browser \
             in WSL when Linux builds ship. Native: {INSTALL_HINT}"
        )
    }
    #[cfg(not(windows))]
    {
        format!("terminal-browser not found. {INSTALL_HINT}")
    }
}

fn run_wsl(
    args: &[&str],
    cwd: Option<&Path>,
    timeout_ms: u64,
    cancel: Option<&tokio_util::sync::CancellationToken>,
) -> Result<String, String> {
    // Quote for bash -lc so paths with spaces survive.
    let mut cmd = String::from("terminal-browser");
    for a in args {
        cmd.push(' ');
        cmd.push_str(&bash_single_quote(a));
    }
    if let Some(dir) = cwd {
        let wsl_dir = windows_path_to_wsl(dir).unwrap_or_else(|| dir.to_string_lossy().into());
        cmd = format!("cd {} && {cmd}", bash_single_quote(&wsl_dir));
    }
    let wsl_args = ["-e", "bash", "-lc", cmd.as_str()];
    if let Some(token) = cancel {
        run_capture_cancelled("wsl.exe", &wsl_args, None, timeout_ms, token)
    } else {
        run_capture("wsl.exe", &wsl_args, None, timeout_ms)
    }
}

fn bash_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r#"'"'"'"#))
}

/// Best-effort `C:\Users\…` → `/mnt/c/Users/…` for WSL cwd.
fn windows_path_to_wsl(path: &Path) -> Option<String> {
    let s = path.to_string_lossy();
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' {
        let drive = (bytes[0] as char).to_ascii_lowercase();
        if drive.is_ascii_alphabetic() {
            let rest = s[2..].replace('\\', "/");
            return Some(format!("/mnt/{drive}{rest}"));
        }
    }
    None
}

/// Map terminal-browser CLI argv onto agent-browser-cli for Windows host mode.
fn run_host(
    args: &[&str],
    cwd: Option<&Path>,
    timeout_ms: u64,
    cancel: Option<&tokio_util::sync::CancellationToken>,
) -> Result<String, String> {
    let bin = find_bin("agent-browser-cli").ok_or_else(|| {
        "agent-browser-cli missing for Windows-host fallback - run `nur ecosystem ensure` \
         or `nur browser setup`"
            .to_string()
    })?;

    let Some((head, rest)) = args.split_first() else {
        return Err("no arguments".into());
    };

    match *head {
        "help" | "--help" | "-h" => Ok(
            "terminal_browser (windows-host)\n\
             open [url] [--split right]  → opens in real Chrome (+ optional Windows Terminal split)\n\
             ls [--json]                 → agent-browser-cli tabs\n\
             setup                       → nur browser setup / doctor\n\
             action -- <cmd>             → snapshot|click|fill|… via agent-browser-cli\n\
             Upstream in-terminal Chromium needs macOS (or future Linux/Windows builds)."
                .into(),
        ),
        "--version" | "version" => Ok(format!(
            "terminal-browser windows-host via {bin}\n\
             (upstream binary not installed; using agent-browser-cli)"
        )),
        "setup" => {
            let setup = crate::ecosystem::browser_setup::setup_summary();
            let live = run_cli(&bin, &["doctor"], cwd, timeout_ms, cancel)
                .unwrap_or_else(|e| format!("(doctor unavailable: {e})"));
            Ok(format!(
                "windows-host setup\n{setup}\n\nbridge doctor:\n{live}\n\n\
                 Tip: chrome://extensions → Load unpacked the staged tmwd_cdp_bridge."
            ))
        }
        "ls" => {
            let argv = ["tabs"];
            let _ = rest.iter().any(|a| *a == "--json");
            run_cli(&bin, &argv, cwd, timeout_ms, cancel).map(|out| {
                format!(
                    "{out}\n\n[windows-host] tab ids above come from agent-browser-cli \
                     (not terminal-browser ls)."
                )
            })
        }
        "open" => host_open(rest, cwd, timeout_ms, cancel),
        "action" => host_action(rest, cwd, timeout_ms, cancel),
        other => Err(format!(
            "windows-host does not implement `{other}` - use open|ls|setup|action|help"
        )),
    }
}

fn host_open(
    args: &[&str],
    cwd: Option<&Path>,
    timeout_ms: u64,
    cancel: Option<&tokio_util::sync::CancellationToken>,
) -> Result<String, String> {
    let bin =
        find_bin("agent-browser-cli").ok_or_else(|| String::from("agent-browser-cli missing"))?;
    let mut url = None;
    let mut split = Some("right");
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "--split" => {
                i += 1;
                if i < args.len() {
                    if args[i] == "none" {
                        split = None;
                    } else {
                        split = Some(args[i]);
                    }
                }
            }
            "--size" => {
                i += 1; // ignore size in host mode
            }
            flag if flag.starts_with('-') => {}
            other => {
                if url.is_none() {
                    url = Some(other);
                }
            }
        }
        i += 1;
    }

    let mut notes = Vec::new();
    if let Some(dir) = split {
        if let Some(msg) = try_windows_terminal_split(dir, url) {
            notes.push(msg);
        } else {
            notes.push(format!(
                "split={dir}: Windows Terminal split skipped (wt not available or failed) - \
                 opening in Chrome instead"
            ));
        }
    }

    let mut argv = vec!["open".to_string()];
    if let Some(u) = url {
        argv.push(u.to_string());
    }
    let refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
    match run_cli(&bin, &refs, cwd, timeout_ms, cancel) {
        Ok(out) => {
            notes.push(out);
            notes.push(
                "[windows-host] opened via agent-browser-cli. \
                 Full kitty-graphics in-terminal chrome needs upstream terminal-browser."
                    .into(),
            );
            Ok(notes.join("\n"))
        }
        Err(e) => {
            // Bridge failed - fall back to the OS handler once (no double-open
            // on the happy path).
            if let Some(u) = url {
                if crate::open_uri::open(u).is_ok() {
                    notes.push(format!(
                        "[windows-host] agent-browser-cli open failed ({e}); opened via system handler"
                    ));
                    return Ok(notes.join("\n"));
                }
            }
            Err(e)
        }
    }
}

fn host_action(
    args: &[&str],
    cwd: Option<&Path>,
    timeout_ms: u64,
    cancel: Option<&tokio_util::sync::CancellationToken>,
) -> Result<String, String> {
    let bin =
        find_bin("agent-browser-cli").ok_or_else(|| String::from("agent-browser-cli missing"))?;
    // Strip selectors + `--`, keep agent-browser passthrough.
    let mut passthrough = Vec::new();
    let mut i = 0;
    let mut seen_sep = false;
    while i < args.len() {
        if !seen_sep {
            match args[i] {
                "--" => seen_sep = true,
                "--browser" | "--tab" | "--target" => i += 1, // skip value
                "--follow" => {}
                other if other.starts_with('-') => {}
                other => passthrough.push(other),
            }
        } else {
            passthrough.push(args[i]);
        }
        i += 1;
    }
    if passthrough.is_empty() {
        return Err(
            "action requires a command after -- (e.g. snapshot, click @e1, fill @e2 text)".into(),
        );
    }

    let mapped = map_agent_cmd_to_cli(&passthrough)?;
    let refs: Vec<&str> = mapped.iter().map(|s| s.as_str()).collect();
    run_cli(&bin, &refs, cwd, timeout_ms, cancel)
}

/// Translate vercel agent-browser verbs onto sleepinginsummer agent-browser-cli.
fn map_agent_cmd_to_cli(cmd: &[&str]) -> Result<Vec<String>, String> {
    let Some((head, rest)) = cmd.split_first() else {
        return Err("empty action command".into());
    };
    let mut out = Vec::new();
    match *head {
        "snapshot" => out.push("snapshot".into()),
        "screenshot" => {
            out.push("screenshot".into());
            for a in rest {
                if *a == "--full" || *a == "--full-page" {
                    out.push("--full-page".into());
                }
            }
        }
        "click" | "fill" | "open" | "close" | "tabs" | "scan" | "tabtree" | "status" | "doctor" => {
            out.push((*head).into());
            out.extend(rest.iter().map(|s| (*s).to_string()));
        }
        "type" => {
            // agent-browser type <sel> <text> → fill
            out.push("fill".into());
            out.extend(rest.iter().map(|s| (*s).to_string()));
        }
        "press" | "key" => {
            out.push("send-keys".into());
            if let Some(k) = rest.first() {
                out.push((*k).to_string());
            }
            if rest.len() > 1 {
                out.push("--target".into());
                out.push(rest[1].to_string());
            }
        }
        "eval" | "evaluate" => {
            out.push("exec".into());
            out.extend(rest.iter().map(|s| (*s).to_string()));
        }
        "tab" => {
            // tab list → tabs; tab new → open; tab close → close
            match rest.first().copied() {
                Some("list") | None => out.push("tabs".into()),
                Some("new") => {
                    out.push("open".into());
                    if let Some(url) = rest.get(1) {
                        out.push((*url).to_string());
                    }
                }
                Some("close") => out.push("close".into()),
                Some(other) => {
                    return Err(format!(
                        "windows-host: unsupported tab subcommand `{other}`"
                    ));
                }
            }
        }
        "get" | "is" | "wait" | "hover" | "select" | "check" | "uncheck" | "scroll" | "network"
        | "console" => {
            // Pass through best-effort; CLI may reject unknown verbs.
            out.push((*head).into());
            out.extend(rest.iter().map(|s| (*s).to_string()));
        }
        "launch" | "install" | "connect" | "disconnect" => {
            return Err(format!(
                "{head} is not available - Windows-host drives your existing Chrome"
            ));
        }
        other => {
            out.push(other.into());
            out.extend(rest.iter().map(|s| (*s).to_string()));
        }
    }
    Ok(out)
}

fn run_cli(
    bin: &str,
    args: &[&str],
    cwd: Option<&Path>,
    timeout_ms: u64,
    cancel: Option<&tokio_util::sync::CancellationToken>,
) -> Result<String, String> {
    if let Some(token) = cancel {
        run_capture_cancelled(bin, args, cwd, timeout_ms, token)
    } else {
        run_capture(bin, args, cwd, timeout_ms)
    }
}

/// Best-effort `wt` split so open still feels side-by-side on Windows Terminal.
///
/// Never interpolates the target URL into a shell `-Command` string (that was
/// injectable via `'` in model-supplied URLs). The pane is a static hint only;
/// Chrome/agent-browser-cli performs the real navigation.
fn try_windows_terminal_split(direction: &str, url: Option<&str>) -> Option<String> {
    #[cfg(windows)]
    {
        let _ = url; // intentionally unused in the pane command
        let dir_flag = match direction {
            "right" => "-H", // horizontal split → pane to the right in wt
            "left" => "-H",
            "down" | "up" => "-V",
            _ => "-H",
        };
        let args = [
            "-w",
            "0",
            "sp",
            dir_flag,
            "powershell",
            "-NoProfile",
            "-Command",
            "Write-Host 'terminal_browser pane (page opens in Chrome)'; Start-Sleep -Seconds 30",
        ];
        match run_capture("wt.exe", &args, None, 8_000) {
            Ok(_) => Some(format!(
                "Windows Terminal: opened a {direction} split pane (preview)"
            )),
            Err(_) => None,
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (direction, url);
        None
    }
}

/// Best-effort native install via the published installer script.
pub fn try_install_native() -> Option<String> {
    if find_native_bin().is_some() {
        return None;
    }
    #[cfg(unix)]
    {
        return run_capture(
            "bash",
            &[
                "-lc",
                "curl -fsSL https://terminal-browser.sh/install | bash",
            ],
            None,
            600_000,
        )
        .ok();
    }
    #[cfg(windows)]
    {
        if wsl_available() && !wsl_has_terminal_browser() {
            return run_capture(
                "wsl.exe",
                &[
                    "-e",
                    "bash",
                    "-lc",
                    "curl -fsSL https://terminal-browser.sh/install | bash",
                ],
                None,
                300_000,
            )
            .ok();
        }
        None
    }
    #[cfg(all(not(unix), not(windows)))]
    {
        None
    }
}

/// Best-effort provision for ecosystem ensure.
pub fn ensure_runtime() -> (bool, String, Option<String>, Option<String>) {
    // Refresh host bridge first so Windows always has a usable fallback.
    #[cfg(windows)]
    {
        let node_ok = crate::ecosystem::which("node") || crate::ecosystem::which("node.exe");
        if node_ok && find_bin("agent-browser-cli").is_none() {
            let npm = find_bin("npm").unwrap_or_else(|| "npm".into());
            let _ = run_capture(
                &npm,
                &["install", "-g", "@sleepinsummer/agent-browser-cli@latest"],
                None,
                300_000,
            );
        }
        // Soft-try WSL install when Linux channel exists (installer may still refuse).
        if wsl_available() && !wsl_has_terminal_browser() {
            let _ = run_capture(
                "wsl.exe",
                &[
                    "-e",
                    "bash",
                    "-lc",
                    "curl -fsSL https://terminal-browser.sh/install | bash",
                ],
                None,
                300_000,
            );
        }
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        if find_native_bin().is_none() {
            let _ = run_capture(
                "bash",
                &[
                    "-lc",
                    "curl -fsSL https://terminal-browser.sh/install | bash",
                ],
                None,
                600_000,
            );
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if find_native_bin().is_none() {
            let _ = run_capture(
                "bash",
                &[
                    "-lc",
                    "curl -fsSL https://terminal-browser.sh/install | bash",
                ],
                None,
                600_000,
            );
        }
    }

    match resolve_runtime() {
        Some(Runtime::Native(p)) => (
            true,
            "native CLI ready · in-terminal Chromium".into(),
            Some(p),
            None,
        ),
        Some(Runtime::Wsl) => (
            true,
            "WSL terminal-browser ready · in-terminal Chromium inside WSL".into(),
            Some("wsl:terminal-browser".into()),
            None,
        ),
        Some(Runtime::Host(p)) => (
            true,
            "windows-host ready · agent-browser-cli fallback (same open/snapshot/click API)".into(),
            Some(p),
            None,
        ),
        None => (
            false,
            "not found - Windows: nur ecosystem ensure (host fallback); macOS: curl install".into(),
            None,
            None,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_agent_cmds() {
        assert_eq!(
            map_agent_cmd_to_cli(&["snapshot"]).unwrap(),
            vec!["snapshot"]
        );
        assert_eq!(
            map_agent_cmd_to_cli(&["click", "@e1"]).unwrap(),
            vec!["click", "@e1"]
        );
        assert_eq!(
            map_agent_cmd_to_cli(&["type", "@e1", "hi"]).unwrap(),
            vec!["fill", "@e1", "hi"]
        );
        assert_eq!(
            map_agent_cmd_to_cli(&["eval", "1+1"]).unwrap(),
            vec!["exec", "1+1"]
        );
        assert!(map_agent_cmd_to_cli(&["launch"]).is_err());
    }

    #[test]
    fn wsl_path_translation() {
        let p = Path::new(r"C:\Users\david\proj");
        assert_eq!(
            windows_path_to_wsl(p).as_deref(),
            Some("/mnt/c/Users/david/proj")
        );
    }

    #[test]
    fn bash_quote_escapes() {
        assert_eq!(bash_single_quote("a'b"), r#"'a'"'"'b'"#);
    }
}

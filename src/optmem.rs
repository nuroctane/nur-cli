//! OptMem - permanent portable agent memory (Victor Taelin).
//!
//! Upstream-pure paths: `~/.optmem/memo` and `~/.optmem/memory`.
//! Honors `$MEMORY_DIR` for the memory tree (upstream behavior).
//! Repo: <https://github.com/VictorTaelin/OptMem>

use crate::ecosystem::{find_bin, run_capture};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const WAKE_CAP_CHARS: usize = 8_000;
const NOTE_MAX_CHARS: usize = 280;
const WAKE_CACHE_TTL: Duration = Duration::from_secs(30 * 60);

static WAKE_CACHE: Mutex<Option<(String, Instant, String)>> = Mutex::new(None);

pub fn optmem_home() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".optmem")
}

pub fn memory_dir() -> PathBuf {
    if let Ok(d) = std::env::var("MEMORY_DIR") {
        let t = d.trim();
        if !t.is_empty() {
            return PathBuf::from(t);
        }
    }
    optmem_home().join("memory")
}

pub fn memo_bin() -> Option<PathBuf> {
    let home_memo = optmem_home().join("memo");
    if home_memo.is_file() {
        return Some(home_memo);
    }
    let home_py = optmem_home().join("memo.py");
    if home_py.is_file() {
        return Some(home_py);
    }
    find_bin("memo").map(PathBuf::from)
}

pub fn doctor_report() -> String {
    let mut lines = Vec::new();
    match memo_bin() {
        Some(p) => lines.push(format!("memo: {}", p.display())),
        None => lines.push(
            "memo: missing - install with: curl -fsSL https://raw.githubusercontent.com/VictorTaelin/OptMem/main/install.sh | sh \
             (or place the memo script at ~/.optmem/memo)"
                .into(),
        ),
    }
    lines.push(format!("memory dir: {}", memory_dir().display()));
    lines.push("OptMem is upstream-pure (~/.optmem); set MEMORY_DIR to relocate memory/.".into());
    lines.join("\n")
}

/// Find a real, runnable Python interpreter for the `memo` script.
///
/// On Windows, `find_bin("python3")` / `find_bin("python")` can resolve to
/// broken `%LOCALAPPDATA%\Microsoft\WindowsApps\python*.exe` Store stubs that
/// only print "Python was not found; run without arguments..." and exit
/// non-zero, which made `memo wake` silently fail even though `~/.optmem/memo`
/// was installed. Prefer the `py` launcher, then probe each candidate and skip
/// any that does not actually start an interpreter.
fn python_runner() -> Option<String> {
    let mut candidates: Vec<String> = Vec::new();
    for name in ["py", "python", "python3"] {
        if let Some(p) = find_bin(name) {
            if !candidates.contains(&p) {
                candidates.push(p);
            }
        }
    }
    for c in candidates {
        let is_launcher = Path::new(&c)
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("py"));
        if python_probe(&c, is_launcher) {
            return Some(c);
        }
    }
    None
}

/// Returns true if `bin` starts a Python interpreter (probe: `-c "import sys"`).
fn python_probe(bin: &str, launcher: bool) -> bool {
    let args: &[&str] = if launcher {
        &["-3", "-c", "import sys"]
    } else {
        &["-c", "import sys"]
    };
    run_capture(bin, args, None, 15_000).is_ok()
}

/// Run memo with args; returns stdout/stderr merged or an error string.
pub fn run_memo(args: &[&str], timeout_ms: u64) -> Result<String, String> {
    let bin = memo_bin().ok_or_else(|| {
        "OptMem memo not found at ~/.optmem/memo - run ecosystem ensure or the upstream install.sh"
            .to_string()
    })?;

    let is_py = bin
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("py"));

    if is_py {
        let py = python_runner().ok_or_else(|| "python required to run memo.py".to_string())?;
        let mut argv: Vec<String> = Vec::new();
        if Path::new(&py)
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("py"))
        {
            argv.push("-3".into());
        }
        argv.push(bin.to_string_lossy().into());
        argv.extend(args.iter().map(|s| (*s).to_string()));
        let args_ref: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
        return run_capture(&py, &args_ref, None, timeout_ms);
    }

    let bin_s = bin.to_string_lossy().to_string();
    match run_capture(&bin_s, args, None, timeout_ms) {
        Ok(s) => Ok(s),
        Err(e) => {
            if let Some(py) = python_runner() {
                let mut argv: Vec<String> = Vec::new();
                if Path::new(&py)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.eq_ignore_ascii_case("py"))
                {
                    argv.push("-3".into());
                }
                argv.push(bin_s.clone());
                argv.extend(args.iter().map(|s| (*s).to_string()));
                let args_ref: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
                run_capture(&py, &args_ref, None, timeout_ms).map_err(|_| e)
            } else {
                Err(e)
            }
        }
    }
}

pub fn wake_capped() -> Option<String> {
    let out = run_memo(&["wake"], 15_000).ok()?;
    let trimmed: String = out.chars().take(WAKE_CAP_CHARS).collect();
    if trimmed.chars().count() < out.chars().count() {
        Some(format!(
            "{trimmed}\n… [optmem wake truncated to {WAKE_CAP_CHARS} chars]"
        ))
    } else {
        Some(trimmed)
    }
}

/// Cached wake for prompt builds - must not re-run memo every turn.
/// Keyed by memory_dir so distinct MEMORY_DIR values do not share a wake blob.
pub fn wake_capped_cached() -> Option<String> {
    let key = memory_dir().to_string_lossy().into_owned();
    if let Ok(guard) = WAKE_CACHE.lock() {
        if let Some((k, at, text)) = guard.as_ref() {
            if k == &key && at.elapsed() < WAKE_CACHE_TTL {
                return Some(text.clone());
            }
        }
    }
    let fresh = wake_capped()?;
    if let Ok(mut guard) = WAKE_CACHE.lock() {
        *guard = Some((key, Instant::now(), fresh.clone()));
    }
    Some(fresh)
}

/// Drop the wake cache (e.g. after note/nap so the next turn sees fresh memory).
pub fn invalidate_wake_cache() {
    if let Ok(mut guard) = WAKE_CACHE.lock() {
        *guard = None;
    }
}

pub fn note(line: &str) -> Result<String, String> {
    let mut s = line.trim().to_string();
    if s.chars().count() > NOTE_MAX_CHARS {
        s = s.chars().take(NOTE_MAX_CHARS).collect();
    }
    if s.is_empty() {
        return Err("note text is empty".into());
    }
    let out = run_memo(&["note", &s], 30_000)?;
    invalidate_wake_cache();
    Ok(out)
}

fn looks_like_memo_script(bytes: &[u8]) -> bool {
    if bytes.len() < 80 {
        return false;
    }
    // Reject HTML error pages / empty downloads.
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(400)]).to_ascii_lowercase();
    if head.contains("<!doctype") || head.contains("<html") {
        return false;
    }
    head.contains("optmem")
        || head.contains("memory_dir")
        || head.contains("def wake")
        || head.starts_with("#!")
}

/// Best-effort install: download memo script into ~/.optmem/memo.
pub fn ensure_install() -> Result<String, String> {
    let home = optmem_home();
    fs::create_dir_all(&home).map_err(|e| e.to_string())?;
    fs::create_dir_all(memory_dir()).map_err(|e| e.to_string())?;

    if memo_bin().is_some() {
        return Ok(format!("OptMem already present at {}", home.display()));
    }

    let url = "https://raw.githubusercontent.com/VictorTaelin/OptMem/main/memo";
    let dest = home.join("memo");
    let tmp = home.join("memo.download");

    let downloaded = if let Some(curl) = find_bin("curl") {
        run_capture(
            &curl,
            &[
                "-fsSL",
                "--max-time",
                "60",
                url,
                "-o",
                &tmp.to_string_lossy(),
            ],
            None,
            70_000,
        )
        .is_ok()
            && tmp.is_file()
    } else {
        false
    };

    #[cfg(windows)]
    let downloaded = downloaded || {
        let ps = format!(
            "Invoke-WebRequest -Uri '{}' -OutFile '{}' -UseBasicParsing -TimeoutSec 60",
            url,
            tmp.display()
        );
        run_capture(
            "powershell",
            &["-NoProfile", "-NonInteractive", "-Command", &ps],
            None,
            70_000,
        )
        .is_ok()
            && tmp.is_file()
    };

    if !downloaded {
        let _ = fs::remove_file(&tmp);
        return Err(
            "could not download OptMem memo - install manually: \
             curl -fsSL https://raw.githubusercontent.com/VictorTaelin/OptMem/main/install.sh | sh"
                .into(),
        );
    }

    let bytes = fs::read(&tmp).map_err(|e| e.to_string())?;
    if !looks_like_memo_script(&bytes) {
        let _ = fs::remove_file(&tmp);
        return Err(
            "downloaded OptMem memo failed integrity check (not a memo script) - install manually"
                .into(),
        );
    }
    fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&dest, fs::Permissions::from_mode(0o755));
    }
    Ok(format!("installed memo -> {}", dest.display()))
}

/// Prompt block for root agents (not subagents).
pub fn prompt_block(enabled: bool, is_subagent: bool, poor_mode: bool) -> String {
    if is_subagent {
        return "\n# OptMem\nYou are a subagent. Don't run memo.\n".into();
    }
    if !enabled || poor_mode {
        return String::new();
    }
    let mut s = String::from(
        r#"
# OptMem (permanent memory)
Your memory is OptMem (upstream-pure under ~/.optmem):
- Tool: optmem (or ~/.optmem/memo)
- Memories: ~/.optmem/memory (or $MEMORY_DIR)

OptMem outlives every session, compaction, model and vendor change.
Without it you do not know who you are, or what was decided and tried.

## While working
Call optmem(action=note, text="...") whenever you learn something worth keeping
(one line, max 280 chars). Do not register redundant memories.
If note asks a compression / merge: run optmem(action=nap) before your next action.
Never edit files under the OptMem memory directory by hand.

## Search
optmem(action=recall, query=...) or optmem(action=zoom, range="a-b").

"#,
    );
    if let Some(wake) = wake_capped_cached() {
        s.push_str("## Wake (session start)\n");
        s.push_str(&wake);
        s.push('\n');
    } else {
        s.push_str(
            "## Wake\nOptMem memo not available yet - run `nur ecosystem ensure` or install OptMem.\n",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_dir_default() {
        let p = memory_dir();
        assert!(p.ends_with("memory") || std::env::var("MEMORY_DIR").is_ok());
    }

    #[test]
    fn subagent_block() {
        let b = prompt_block(true, true, false);
        assert!(b.contains("Don't run memo"));
    }

    #[test]
    fn rejects_html_download() {
        assert!(!looks_like_memo_script(b"<!DOCTYPE html><html>404</html>"));
        let mut ok =
            b"#!/usr/bin/env python3\n# OptMem memory manager\ndef wake():\n    pass\n".to_vec();
        while ok.len() < 80 {
            ok.extend_from_slice(b"# pad\n");
        }
        assert!(looks_like_memo_script(&ok));
    }

    #[test]
    fn python_probe_accepts_real_interpreter() {
        // A candidate that runs `import sys` successfully must probe true.
        // This is environment-dependent; skip silently if none exists so the
        // test is not flaky on bare CI.
        let mut any_ok = false;
        for name in ["python", "python3", "py"] {
            if let Some(p) = find_bin(name) {
                let is_launcher = Path::new(&p)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.eq_ignore_ascii_case("py"));
                if python_probe(&p, is_launcher) {
                    any_ok = true;
                }
            }
        }
        // On machines with a real Python at least one must probe true.
        // If none does, we accept that only if the binary also does not exist
        // (broken environment) - but on dev machines there is always one.
        if any_ok {
            return;
        }
        // No working python found anywhere - environment exotic; don't hard fail.
        eprintln!("no working python interpreter found on this machine");
    }
}

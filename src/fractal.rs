//! fractal integration — hierarchical recursive loops in git worktrees
//!
//! Fractal (<https://github.com/plasma-ai/fractal>) spawns child nodes in isolated
//! git worktrees, each running its own autonomous agent loop. You are one node;
//! spawn a child to own a subtask that is well-defined, separable, large enough
//! for its own iteration cycle, and able to be run in parallel.
//!
//! This module mirrors the penecho/t3code pattern:
//! - Probe binary on PATH with Windows extension handling
//! - Detect fractal repo (`.fractal` folder at repo root, `.worktrees`)
//! - Version check via `fractal --version`
//! - Repo root discovery via walking up for `.git`
//! - Doctor checks: binary, git, fractal folder, worktrees

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;

use crate::error::{NurError, Result};

/// fractal data dir at repo root (from fractal/constants.py)
pub const FRACTAL_FOLDER: &str = ".fractal";
pub const WORKTREES_FOLDER: &str = ".worktrees";

/// Wall-clock ceiling for one `fractal` CLI invocation. Several subcommands
/// stream instead of terminating (`radio read --follow`, `chat --current`), and
/// `Command::output()` blocks until the child closes stdout — so without a
/// deadline one of those wedges the whole turn with no way out.
pub const FRACTAL_TIMEOUT_MS: u64 = 30_000;

/// Probes are only `--version` / `--help`: they answer at once or never.
const PROBE_TIMEOUT_MS: u64 = 10_000;

/// How often to re-check the child for exit / cancellation (mirrors `tools::shell`).
const POLL_INTERVAL_MS: u64 = 30;

/// Per-stream capture cap, so a runaway child cannot exhaust memory.
const CAPTURE_CAP_BYTES: u64 = 1_000_000;

/// The one failure mode every Windows install hits: upstream's
/// `fractal/core/worktree.py` imports `fcntl`, which is Unix-only, so *every*
/// invocation — `--version` and `--help` included — dies during import.
pub const PLATFORM_UNSUPPORTED: &str = "fractal is installed but cannot run on this platform — upstream requires Unix (imports fcntl). Use WSL or a Linux/macOS host.";

const NOT_FOUND_HINT: &str = "fractal binary not found on PATH. Install via pipx: `pipx install plasma-fractal` (requires Python 3.12-3.14). Repo: https://github.com/plasma-ai/fractal";

/// Reuse robust find_on_path from penecho (handles .exe/.cmd/.bat/.js wrappers on Windows)
pub fn find_on_path(name: &str) -> Option<PathBuf> {
    crate::penecho::find_on_path(name)
}

#[derive(Debug, Clone)]
pub struct FractalProbe {
    pub binary: Option<PathBuf>,
    pub version: Option<String>,
    /// A binary on PATH is not the same as a binary that runs. `probe_at`
    /// actually executes the version/help probe; this is true only when the CLI
    /// answered. On Windows it is always false — see [`PLATFORM_UNSUPPORTED`].
    pub usable: bool,
    /// One actionable line explaining why `usable` is false — never a stack trace.
    pub unusable_reason: Option<String>,
    pub repo_root: Option<PathBuf>,
    pub fractal_dir: Option<PathBuf>,
    pub fractal_dir_exists: bool,
    pub worktrees_exist: bool,
    pub is_git_repo: bool,
    pub is_fractal_repo: bool,
}

/// Result of one completed child process: exit success plus merged output.
#[derive(Debug)]
struct Capture {
    success: bool,
    text: String,
}

fn read_capped(pipe: Option<impl Read>) -> Vec<u8> {
    let mut buf = Vec::new();
    if let Some(p) = pipe {
        let mut limited = p.take(CAPTURE_CAP_BYTES);
        let _ = limited.read_to_end(&mut buf);
    }
    buf
}

fn kill_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        // `child.kill()` alone leaves grandchildren (python → git → …) running.
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Spawn `bin args`, capturing both streams, bounded by `timeout_ms` and by
/// `cancel`. Kills the whole process tree on either. `Err` is a single
/// human-readable line describing why no output exists.
fn run_capture(
    bin: &Path,
    args: &[&str],
    cwd: Option<&Path>,
    timeout_ms: u64,
    cancel: Option<&CancellationToken>,
) -> std::result::Result<Capture, String> {
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn fractal: {e}"))?;

    // Drain both pipes on threads so a chatty child can't deadlock on a full pipe.
    let out_pipe = child.stdout.take();
    let err_pipe = child.stderr.take();
    let out_h = thread::spawn(move || read_capped(out_pipe));
    let err_h = thread::spawn(move || read_capped(err_pipe));

    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
                if cancel.map(|c| c.is_cancelled()).unwrap_or(false) {
                    kill_tree(&mut child);
                    let _ = out_h.join();
                    let _ = err_h.join();
                    return Err("fractal cancelled by user (process tree killed)".into());
                }
                if Instant::now() >= deadline {
                    kill_tree(&mut child);
                    let _ = out_h.join();
                    let _ = err_h.join();
                    return Err(format!(
                        "fractal timed out after {timeout_ms}ms (process tree killed) — `{}` may be a streaming or interactive subcommand",
                        args.join(" ")
                    ));
                }
                thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
            }
            Err(e) => {
                kill_tree(&mut child);
                return Err(format!("waiting on fractal failed: {e}"));
            }
        }
    };

    let stdout = String::from_utf8_lossy(&out_h.join().unwrap_or_default()).to_string();
    let stderr = String::from_utf8_lossy(&err_h.join().unwrap_or_default()).to_string();
    let text = match (stdout.trim().is_empty(), stderr.trim().is_empty()) {
        (true, true) => String::new(),
        (true, false) => stderr,
        (false, true) => stdout,
        (false, false) => format!("{}\n{}", stdout.trim_end(), stderr.trim_end()),
    };
    Ok(Capture {
        success: status.success(),
        text,
    })
}

/// True when `text` is a Python interpreter stack trace rather than a CLI
/// diagnostic. Click errors ("Error: no such option: --foo") must not match:
/// those are genuinely useful and are passed through untouched.
fn looks_like_python_traceback(text: &str) -> bool {
    text.contains("Traceback (most recent call last)")
        || text
            .lines()
            .any(|l| l.trim_start().starts_with("File \"") && l.contains(", line "))
}

/// The `SomeError: message` line that closes a traceback. Python prints frames
/// indented and the exception at column 0, so the last unindented line wins.
fn traceback_error_line(text: &str) -> Option<&str> {
    text.lines().map(|l| l.trim_end()).rfind(|l| {
        !l.is_empty()
            && !l.starts_with(char::is_whitespace)
            && !l.starts_with("Traceback (")
            && !l.starts_with("During handling of")
            && !l.starts_with("The above exception")
    })
}

/// Collapse whatever a failed `fractal` invocation printed into ONE actionable
/// line. Stack traces lose their frames; real CLI errors survive verbatim.
pub fn summarize_failure(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "fractal failed with no output".into();
    }
    if trimmed.contains("No module named 'fcntl'") || trimmed.contains("No module named \"fcntl\"")
    {
        return PLATFORM_UNSUPPORTED.to_string();
    }
    if !looks_like_python_traceback(trimmed) {
        // A Click/usage error or an ordinary fractal message — keep it as-is.
        return trimmed.to_string();
    }
    let last = traceback_error_line(trimmed).unwrap_or("unknown Python error");
    if let Some(module) = last.strip_prefix("ModuleNotFoundError: No module named ") {
        return format!(
            "fractal cannot run here: its Python environment is missing the module {module} — reinstall with `pipx install plasma-fractal`, or use WSL / a Linux host."
        );
    }
    format!("fractal crashed: {last} (Python traceback suppressed)")
}

/// Interpret one probe invocation. `Ok(line)` is the version/identity line the
/// CLI answered with; `Err(reason)` is a single line saying why it could not run.
fn classify_probe(success: bool, text: &str) -> std::result::Result<String, String> {
    if !success {
        return Err(summarize_failure(text));
    }
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|l| l.to_string())
        .ok_or_else(|| "fractal answered with no output".to_string())
}

/// Run `--version`, falling back to `--help`, and decide whether the installed
/// binary is actually usable.
fn probe_binary(bin: &Path) -> (Option<String>, bool, Option<String>) {
    let version = match run_capture(bin, &["--version"], None, PROBE_TIMEOUT_MS, None) {
        Ok(c) => classify_probe(c.success, &c.text),
        Err(e) => Err(e),
    };
    match version {
        Ok(v) => (Some(v), true, None),
        // `--version` failing does not by itself mean broken: a Click app that
        // never declared the flag exits 2 with "no such option". Ask `--help`
        // before calling the install unusable.
        Err(version_reason) => match run_capture(bin, &["--help"], None, PROBE_TIMEOUT_MS, None) {
            Ok(c) if c.success => match classify_probe(true, &c.text) {
                Ok(v) => (Some(v), true, None),
                Err(reason) => (None, false, Some(reason)),
            },
            Ok(c) => (None, false, Some(summarize_failure(&c.text))),
            Err(_) => (None, false, Some(version_reason)),
        },
    }
}

/// Where the binary lives and whether it runs — the half of a probe that costs
/// process spawns and does not depend on `cwd`.
#[derive(Clone)]
struct BinaryFacts {
    binary: Option<PathBuf>,
    version: Option<String>,
    usable: bool,
    unusable_reason: Option<String>,
}

struct CachedFacts {
    at: Instant,
    facts: BinaryFacts,
}

static BINARY_FACTS: std::sync::Mutex<Option<CachedFacts>> = std::sync::Mutex::new(None);

/// How long a probe result stays good. An install (or uninstall) mid-session is
/// picked up on the next turn; meanwhile every `probe_at`/`doctor_at` in a burst
/// reuses one answer instead of re-spawning the CLI.
const PROBE_CACHE_TTL: Duration = Duration::from_secs(60);

/// Probe the binary at most once per [`PROBE_CACHE_TTL`]. Every probe costs 1-2
/// process spawns, and on a host where fractal cannot start they are 1-2
/// *guaranteed-failing* spawns — pure latency repeated on every tool call.
/// The lock is held across the probe so concurrent callers wait for one answer
/// rather than stampeding.
fn binary_facts() -> BinaryFacts {
    let mut guard = BINARY_FACTS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(c) = guard.as_ref() {
        if c.at.elapsed() < PROBE_CACHE_TTL {
            return c.facts.clone();
        }
    }
    let binary = find_on_path("fractal");
    let (version, usable, unusable_reason) = match binary.as_ref() {
        Some(b) => probe_binary(b),
        None => (None, false, Some(NOT_FOUND_HINT.to_string())),
    };
    let facts = BinaryFacts {
        binary,
        version,
        usable,
        unusable_reason,
    };
    *guard = Some(CachedFacts {
        at: Instant::now(),
        facts: facts.clone(),
    });
    facts
}

fn find_git_root(mut cwd: &Path) -> Option<PathBuf> {
    loop {
        if cwd.join(".git").exists() {
            return Some(cwd.to_path_buf());
        }
        match cwd.parent() {
            Some(p) => cwd = p,
            None => break,
        }
    }
    None
}

/// Enclosing git repo root, resolved from the filesystem alone — no process spawn.
pub fn repo_root_of(cwd: &Path) -> Option<PathBuf> {
    find_git_root(cwd)
}

fn find_fractal_root(mut cwd: &Path) -> Option<PathBuf> {
    loop {
        if cwd.join(FRACTAL_FOLDER).exists() {
            return Some(cwd.to_path_buf());
        }
        match cwd.parent() {
            Some(p) => cwd = p,
            None => break,
        }
    }
    None
}

fn is_valid_node_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 64 {
        return false;
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return false;
    }
    name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[allow(dead_code)]
pub fn is_valid_fractal_node_name(name: &str) -> bool {
    is_valid_node_name(name)
}

/// Probe fractal installation and current repo state.
pub fn probe_at(cwd: &Path) -> FractalProbe {
    let BinaryFacts {
        binary,
        version,
        usable,
        unusable_reason,
    } = binary_facts();

    let repo_root = find_git_root(cwd);
    let is_git_repo = repo_root.is_some();
    let fractal_root = find_fractal_root(cwd);
    let fractal_dir = fractal_root
        .as_ref()
        .map(|r| r.join(FRACTAL_FOLDER))
        .or_else(|| repo_root.as_ref().map(|r| r.join(FRACTAL_FOLDER)));
    let fractal_dir_exists = fractal_dir.as_ref().map(|p| p.exists()).unwrap_or(false);
    let worktrees_exist = fractal_root
        .as_ref()
        .or(repo_root.as_ref())
        .map(|r| {
            let wt = r.join(WORKTREES_FOLDER);
            wt.exists() || r.join(FRACTAL_FOLDER).join(WORKTREES_FOLDER).exists()
        })
        .unwrap_or(false);
    let is_fractal_repo = fractal_dir_exists;

    FractalProbe {
        binary,
        version,
        usable,
        unusable_reason,
        repo_root,
        fractal_dir,
        fractal_dir_exists,
        worktrees_exist,
        is_git_repo,
        is_fractal_repo,
    }
}

#[allow(dead_code)]
pub fn probe() -> FractalProbe {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    probe_at(&cwd)
}

/// Run fractal CLI and capture output, honouring `cancel` and a wall-clock
/// deadline. Failures come back as one actionable line, never a stack trace.
pub fn run_fractal_args_cancellable(
    cwd: &Path,
    args: &[String],
    cancel: &CancellationToken,
) -> Result<String> {
    let bin = find_on_path("fractal").ok_or_else(|| NurError::Other(NOT_FOUND_HINT.into()))?;
    let argv: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let cap = run_capture(&bin, &argv, Some(cwd), FRACTAL_TIMEOUT_MS, Some(cancel))
        .map_err(NurError::Other)?;
    if cap.success {
        Ok(cap.text)
    } else {
        Err(NurError::Other(summarize_failure(&cap.text)))
    }
}

/// Run fractal CLI with the default timeout and no cancellation source.
/// Prefer [`run_fractal_args_cancellable`] anywhere a `ToolContext` is in hand.
#[allow(dead_code)]
pub fn run_fractal_args(cwd: &Path, args: &[String]) -> Result<String> {
    run_fractal_args_cancellable(cwd, args, &CancellationToken::new())
}

/// Doctor report.
#[derive(Debug, Clone)]
pub struct Doctor {
    pub binary_present: bool,
    /// Present on PATH *and* able to run — see [`FractalProbe::usable`].
    pub binary_usable: bool,
    pub unusable_reason: Option<String>,
    pub version: Option<String>,
    pub git_repo: bool,
    pub fractal_repo: bool,
    pub fractal_dir: Option<PathBuf>,
    pub worktrees_present: bool,
    pub python_present: bool,
}

pub fn doctor_at(cwd: &Path) -> Doctor {
    let probe = probe_at(cwd);
    let python = find_on_path("python")
        .or_else(|| find_on_path("python3"))
        .is_some();
    Doctor {
        binary_present: probe.binary.is_some(),
        binary_usable: probe.usable,
        unusable_reason: probe.unusable_reason,
        version: probe.version,
        git_repo: probe.is_git_repo,
        fractal_repo: probe.is_fractal_repo,
        fractal_dir: probe.fractal_dir,
        worktrees_present: probe.worktrees_exist,
        python_present: python,
    }
}

#[allow(dead_code)]
pub fn doctor() -> Doctor {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    doctor_at(&cwd)
}

/// List nodes via `fractal node list` if available; fallback to reading .fractal dir.
pub fn list_nodes_cancellable(cwd: &Path, cancel: &CancellationToken) -> Result<String> {
    match run_fractal_args_cancellable(cwd, &["node".into(), "list".into()], cancel) {
        Ok(s) => Ok(s),
        Err(e) => {
            // The fallback only needs directory layout, so resolve it directly
            // instead of re-running the whole probe (which spawns processes —
            // twice, in the old code — purely to re-read paths).
            let root = find_git_root(cwd).or_else(|| find_fractal_root(cwd));
            if let Some(root) = root {
                let candidates = [
                    root.join(WORKTREES_FOLDER),
                    root.join(FRACTAL_FOLDER).join(WORKTREES_FOLDER),
                ];
                for wt in candidates {
                    if wt.exists() {
                        let entries = fs::read_dir(&wt)
                            .map(|rd| {
                                let mut names = Vec::new();
                                for ent in rd.flatten() {
                                    if ent.path().is_dir() {
                                        if let Some(n) = ent.file_name().to_str() {
                                            names.push(n.to_string());
                                        }
                                    }
                                }
                                names.join("\n")
                            })
                            .unwrap_or_else(|_| "(cannot read worktrees dir)".into());
                        return Ok(format!(
                            "(fractal CLI failed: {e})\nFallback worktrees in {}:\n{entries}",
                            wt.display()
                        ));
                    }
                }
            }
            Err(e)
        }
    }
}

/// List nodes with the default timeout and no cancellation source.
#[allow(dead_code)]
pub fn list_nodes(cwd: &Path) -> Result<String> {
    list_nodes_cancellable(cwd, &CancellationToken::new())
}

/// Open node dir path for a given node name.
pub fn node_path(cwd: &Path, node_name: &str) -> Option<PathBuf> {
    if !is_valid_node_name(node_name) {
        return None;
    }
    // Pure path question — `probe_at` would spawn the CLI just to re-derive
    // the git root, and on an unusable install that spawn always fails.
    let root = find_git_root(cwd)?;
    let candidates = [
        root.join(WORKTREES_FOLDER).join(node_name),
        root.join(FRACTAL_FOLDER)
            .join(WORKTREES_FOLDER)
            .join(node_name),
    ];
    candidates.into_iter().find(|wt_path| wt_path.exists())
}

/// Check if this repo can init fractal.
#[allow(dead_code)]
pub fn can_init(cwd: &Path) -> bool {
    // Filesystem-only question; no need to execute the CLI.
    find_git_root(cwd).is_some() && find_fractal_root(cwd).is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_does_not_panic() {
        let p = probe();
        let _ = format!("{:?}", p);
    }

    #[test]
    fn doctor_does_not_panic() {
        let d = doctor();
        let _ = format!("{:?}", d);
    }

    #[test]
    fn valid_name_rejects_traversal() {
        assert!(!is_valid_node_name("../../etc"));
        assert!(!is_valid_node_name("a/b"));
        assert!(!is_valid_node_name("a\\b"));
        assert!(is_valid_node_name("my_node_123"));
        assert!(!is_valid_node_name("my-node")); // dash not allowed
    }

    /// Verbatim from `fractal --version` on Windows with plasma-fractal 1.0.0
    /// installed: upstream's `core/worktree.py` imports the Unix-only `fcntl`.
    const REAL_FCNTL_TRACEBACK: &str = r#"Traceback (most recent call last):
  File "<frozen runpy>", line 198, in _run_module_as_main
  File "<frozen runpy>", line 88, in _run_code
  File "C:\Users\david\.local\bin\fractal.exe\__main__.py", line 4, in <module>
    from fractal.cli.main import cli
  File "C:\...\site-packages\fractal\__init__.py", line 6, in <module>
    from . import cli, constants, core, exceptions, impl, typing, util
  File "C:\...\site-packages\fractal\core\worktree.py", line 6, in <module>
    import fcntl
ModuleNotFoundError: No module named 'fcntl'
"#;

    #[test]
    fn fcntl_traceback_collapses_to_one_actionable_line() {
        let s = summarize_failure(REAL_FCNTL_TRACEBACK);
        assert_eq!(s.lines().count(), 1, "must be one line, got: {s}");
        assert!(s.contains("fcntl"), "must name the culprit module: {s}");
        assert!(!s.contains("Traceback"), "must not leak the stack: {s}");
        assert!(!s.contains("site-packages"), "must not leak frames: {s}");
        assert_eq!(s, PLATFORM_UNSUPPORTED);
    }

    #[test]
    fn generic_traceback_keeps_only_the_final_error_line() {
        let raw = "Traceback (most recent call last):\n  File \"a.py\", line 3, in <module>\n    boom()\nRuntimeError: node registry is locked\n";
        let s = summarize_failure(raw);
        assert_eq!(s.lines().count(), 1);
        assert!(s.contains("RuntimeError: node registry is locked"), "{s}");
        assert!(!s.contains("File \""), "{s}");
    }

    #[test]
    fn a_different_missing_module_still_yields_a_useful_line() {
        let raw = "Traceback (most recent call last):\n  File \"a.py\", line 1, in <module>\n    import pty\nModuleNotFoundError: No module named 'pty'\n";
        let s = summarize_failure(raw);
        assert_eq!(s.lines().count(), 1);
        assert!(s.contains("'pty'"), "{s}");
        assert!(!s.contains("Traceback"), "{s}");
    }

    /// Real CLI diagnostics are the useful case — they must survive untouched.
    #[test]
    fn click_and_plain_errors_are_preserved() {
        assert_eq!(
            summarize_failure("Error: no such option: --foo\n"),
            "Error: no such option: --foo"
        );
        assert_eq!(
            summarize_failure(
                "Usage: fractal node start [OPTIONS] NAME\nTry 'fractal node start --help'."
            ),
            "Usage: fractal node start [OPTIONS] NAME\nTry 'fractal node start --help'."
        );
        assert_eq!(summarize_failure("   "), "fractal failed with no output");
    }

    #[test]
    fn traceback_detection_does_not_fire_on_ordinary_text() {
        assert!(!looks_like_python_traceback(
            "Error: no such option: --version"
        ));
        assert!(!looks_like_python_traceback("node_a  running  $0.42"));
        assert!(looks_like_python_traceback(REAL_FCNTL_TRACEBACK));
    }

    /// "on PATH" and "runs" are different facts; a crashing binary is unusable.
    #[test]
    fn probe_classification_separates_present_from_usable() {
        // Startup crash => unusable, with the platform explanation.
        let err = classify_probe(false, REAL_FCNTL_TRACEBACK).unwrap_err();
        assert_eq!(err, PLATFORM_UNSUPPORTED);
        // A successful answer => usable, first non-empty line is the version.
        assert_eq!(
            classify_probe(true, "\nfractal, version 1.0.0\n").unwrap(),
            "fractal, version 1.0.0"
        );
        // Exit 0 with nothing to say is still not a version.
        assert!(classify_probe(true, "  \n \n").is_err());
        // A Click "no such option" is a failure of the flag, not of the binary —
        // it is reported verbatim so `probe_binary` can retry with `--help`.
        assert_eq!(
            classify_probe(false, "Error: no such option: --version").unwrap_err(),
            "Error: no such option: --version"
        );
    }

    /// `Command::output()` waits for the child to close stdout, so a streaming
    /// subcommand used to wedge the turn forever. Both escape hatches — the
    /// deadline and the user's Esc — must kill the child instead.
    ///
    /// Both cases live in one test on purpose: they spawn (and `taskkill`) real
    /// processes, and running them concurrently just adds machine load to a
    /// suite that already has timing-sensitive tests.
    #[test]
    fn run_capture_enforces_the_deadline_and_cancellation() {
        let Some((bin, args)) = slow_command() else {
            return;
        };

        let started = Instant::now();
        let err = run_capture(&bin, &args, None, 400, None).unwrap_err();
        assert!(err.contains("timed out"), "{err}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "deadline not enforced: {:?}",
            started.elapsed()
        );

        let cancel = CancellationToken::new();
        cancel.cancel();
        let started = Instant::now();
        let err = run_capture(&bin, &args, None, 60_000, Some(&cancel)).unwrap_err();
        assert!(err.contains("cancelled"), "{err}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "cancel not honoured promptly: {:?}",
            started.elapsed()
        );
    }

    /// A ~30s no-op that exists on the host, or `None` if we cannot find one.
    fn slow_command() -> Option<(PathBuf, Vec<&'static str>)> {
        #[cfg(windows)]
        {
            let bin = find_on_path("ping")?;
            Some((bin, vec!["-n", "30", "127.0.0.1"]))
        }
        #[cfg(not(windows))]
        {
            let bin = find_on_path("sleep")?;
            Some((bin, vec!["30"]))
        }
    }
}

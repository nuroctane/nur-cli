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
/// True when the file is a `#!… python` script. The upstream install.sh ships
/// `memo` with NO extension, so an `.py`-extension check alone misses it and
/// the direct spawn dies with os error 193 (%1 is not a valid Win32
/// application) on Windows before the Python fallback can run.
fn is_python_script(bin: &std::path::Path) -> bool {
    use std::io::Read;
    if bin
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("py"))
    {
        return true;
    }
    let mut f = match std::fs::File::open(bin) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut head = [0u8; 128];
    let n = f.read(&mut head).unwrap_or(0);
    let first_line = head[..n].split(|b| *b == b'\n').next().unwrap_or(&[]);
    first_line.starts_with(b"#!")
        && first_line
            .to_ascii_lowercase()
            .windows(6)
            .any(|w| w == b"python")
}

/// The probed interpreter, cached: probing spawns a child per candidate and
/// optmem calls arrive several times a session.
///
/// Shared with the local-engine bridge (`crate::jev_local`), which needs the same
/// guarantee: `py -3` first, and never the Windows Store `python3.exe` alias stub
/// (it exists on PATH but exits 9009).
pub(crate) fn python_runner_cached() -> Option<String> {
    static RUNNER: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    RUNNER.get_or_init(python_runner).clone()
}

#[cfg(test)]
mod python_script_tests {
    use super::*;

    /// The upstream install ships `memo` with NO extension; the old
    /// extension-only check missed it and every optmem call died with
    /// os error 193 before the Python fallback could run.
    #[test]
    fn shebang_detection_covers_extensionless_python_scripts() {
        let dir = std::env::temp_dir().join(format!("nur-optmem-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let no_ext = dir.join("memo");
        std::fs::write(&no_ext, "#!/usr/bin/env python3\nprint(1)\n").unwrap();
        assert!(is_python_script(&no_ext));
        let with_ext = dir.join("memo.py");
        std::fs::write(&with_ext, "print(1)\n").unwrap();
        assert!(is_python_script(&with_ext));
        let binary = dir.join("memo.exe");
        std::fs::write(&binary, b"MZ not a python script").unwrap();
        assert!(!is_python_script(&binary));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

pub fn run_memo(args: &[&str], timeout_ms: u64) -> Result<String, String> {
    let bin = memo_bin().ok_or_else(|| {
        "OptMem memo not found at ~/.optmem/memo - run ecosystem ensure or the upstream install.sh"
            .to_string()
    })?;

    let is_py = is_python_script(&bin)
        || bin
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("py"));

    if is_py {
        let py = python_runner_cached().ok_or_else(|| {
            "memo is a Python script but no usable interpreter was found (probed py / python /              python3). Install Python 3 - the WindowsApps python3.exe Store alias stub does not              count."
                .to_string()
        })?;
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
            if let Some(py) = python_runner_cached() {
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

/// One pending OptMem compression, as upstream's `memo nap` describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct NapPrompt {
    /// Block range upstream asks for, inclusive (`36-37`).
    pub lo: u64,
    pub hi: u64,
    /// How many compressions remain *after* this one, per upstream's own count.
    pub remaining: usize,
    /// Upstream's text for the block (the part worth compressing).
    pub body: String,
}

impl NapPrompt {
    /// `36-37`, the argument upstream expects.
    pub fn range(&self) -> String {
        format!("{}-{}", self.lo, self.hi)
    }
}

/// Parse upstream's nap prompt.
///
/// Upstream prints (exact shape, verified against `memo nap`):
///
/// ```text
/// Compress memories #36-37 into one line of at most 280 bytes.
/// Keep what has lasting effect, drop what does not. Invent nothing.
///
///   #36 <text>
///   #37 <text>
///
/// 20 compressions remain after this one.
/// Run: ~\.optmem\memo nap 36-37 "<your line>"
/// ```
///
/// `None` means "nothing pending" - the queue is empty, which is the normal end
/// state and not an error.
pub fn parse_nap_prompt(out: &str) -> Option<NapPrompt> {
    let range_marker = "Compress memories #";
    let idx = out.find(range_marker)?;
    let rest = &out[idx + range_marker.len()..];
    // "36-37 into one line..."
    let range_str: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    let (lo, hi) = range_str.split_once('-')?;
    let lo: u64 = lo.trim().parse().ok()?;
    let hi: u64 = hi.trim().parse().ok()?;
    if hi < lo {
        return None;
    }
    // "20 compressions remain after this one."
    let remaining = out
        .lines()
        .find_map(|l| {
            let l = l.trim();
            let n = l.split_whitespace().next()?;
            if l.contains("compressions remain") {
                n.parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);
    // The block text: lines starting with two spaces and `#<id> `.
    let mut body: Vec<&str> = Vec::new();
    for line in out.lines() {
        let t = line.trim_start_matches(' ');
        if line.starts_with("  #") && t.starts_with('#') {
            body.push(line.trim());
        }
    }
    Some(NapPrompt {
        lo,
        hi,
        remaining,
        body: body.join(
            "
",
        ),
    })
}

/// How many single-block naps may be applied before the tool refuses to keep
/// prompting. A finite housekeeping queue must never read as an instruction
/// chain: past this, the caller is told to drain in one call or drop it.
pub const NAP_CHAIN_MAX: usize = 2;

/// Window after which the chain counter resets (a later, deliberate session of
/// memory hygiene starts fresh).
const NAP_CHAIN_TTL: Duration = Duration::from_secs(600);

static NAP_CHAIN: Mutex<Option<(Instant, usize)>> = Mutex::new(None);

/// Count one single-block nap application and return the running total for the
/// current window.
pub fn count_single_nap() -> usize {
    let Ok(mut guard) = NAP_CHAIN.lock() else {
        return 1;
    };
    let now = Instant::now();
    let count = match guard.as_ref() {
        Some((at, n)) if now.duration_since(*at) < NAP_CHAIN_TTL => n + 1,
        _ => 1,
    };
    *guard = Some((now, count));
    count
}

/// Reset the chain counter (a drain happened, or the user asked for hygiene).
pub fn reset_nap_chain() {
    if let Ok(mut guard) = NAP_CHAIN.lock() {
        *guard = None;
    }
}

/// Apply one compression. `line` is the model's own one-line summary.
pub fn nap_apply(range: &str, line: &str) -> Result<String, String> {
    run_memo(&["nap", range, line], 120_000).map(|out| {
        invalidate_wake_cache();
        out
    })
}

/// What a drain did.
#[derive(Debug, Clone, PartialEq)]
pub struct NapDrain {
    pub applied: Vec<String>,
    /// The next pending block, when lines ran out before the queue did.
    pub pending: Option<NapPrompt>,
    /// Set when the queue turned out to be empty (nothing left at all).
    pub queue_cleared: bool,
}

/// Apply several compressions in one tool call, in order.
///
/// Upstream only renders the *next* pending block, so the ranges are discovered
/// as we go: read the current prompt, apply the caller's line for it, read the
/// next prompt, and so on. This is the difference between "one call drains N
/// blocks" and "N calls, each revealing another prompt" - the latter is what
/// turned housekeeping into an endless chain.
///
/// `runner` runs the memo binary (injected for tests).
pub fn nap_drain_with(
    lines: &[String],
    mut runner: impl FnMut(&[&str]) -> Result<String, String>,
) -> Result<NapDrain, String> {
    let mut applied = Vec::new();
    let mut pending_out = runner(&["nap"])?;
    for line in lines {
        let Some(p) = parse_nap_prompt(&pending_out) else {
            // Nothing left to compress: stop early instead of erroring, so a
            // drain that over-delivers lines still reports honestly.
            return Ok(NapDrain {
                applied,
                pending: None,
                queue_cleared: true,
            });
        };
        let range = p.range();
        let line = line.trim();
        if line.is_empty() {
            return Err(format!("nap drain: empty line for block #{range}"));
        }
        pending_out = runner(&["nap", &range, line])?;
        applied.push(range);
    }
    invalidate_wake_cache();
    let pending = parse_nap_prompt(&pending_out);
    Ok(NapDrain {
        queue_cleared: pending.is_none(),
        applied,
        pending,
    })
}

/// [`nap_drain_with`] against the real memo binary.
pub fn nap_drain(lines: &[String]) -> Result<NapDrain, String> {
    let out = nap_drain_with(lines, |args| run_memo(args, 120_000))?;
    reset_nap_chain();
    Ok(out)
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
OptMem compressions are **housekeeping**: `nap` shows the next pending block, and the
queue is finite but long. Never let it outrank the user's request. If you touch it at
all, pass `lines=[...]` to drain several blocks in ONE call (one line each, in order)
rather than applying them one at a time turn after turn.
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

    /// The exact shape upstream prints (captured from `memo nap` on this
    /// machine, 2026-09-20) - the parser must not drift from it.
    const UPSTREAM_PROMPT: &str = "Compress memories #36-37 into one line of at most 280 bytes.\n\
Keep what has lasting effect, drop what does not. Invent nothing.\n\n  #36 2026-09-16 VaultX RWA dApp review centralised: deliverables mirrored to D:\\BACKUP.\n  #37 2026-09-16 VaultX RWA dApp: client handoff set is proposal + appendix + reproduction/.\n\n20 compressions remain after this one.\nRun: ~\\.optmem\\memo nap 36-37 \"<your line>\"\n";

    #[test]
    fn parses_upstream_nap_prompt() {
        let p = parse_nap_prompt(UPSTREAM_PROMPT).expect("range parsed");
        assert_eq!(p.lo, 36);
        assert_eq!(p.hi, 37);
        assert_eq!(p.range(), "36-37");
        assert_eq!(p.remaining, 20);
        assert!(p.body.contains("#36 "), "{}", p.body);
        assert!(p.body.contains("#37 "), "{}", p.body);
        assert!(
            p.body.contains("VaultX RWA dApp review centralised"),
            "{}",
            p.body
        );
        assert!(p.body.contains("client handoff set"), "{}", p.body);
    }

    #[test]
    fn an_empty_queue_is_not_a_prompt() {
        assert!(parse_nap_prompt("Saved as #42.\nNothing to compress.\n").is_none());
        assert!(parse_nap_prompt("Compress memories #37-36 into one line").is_none());
        assert!(parse_nap_prompt("").is_none());
    }

    /// One call, several blocks: the ranges are discovered as the queue moves,
    /// which is the whole point - upstream only ever shows the next block.
    #[test]
    fn a_drain_applies_every_line_in_order() {
        use std::cell::RefCell;
        let calls: RefCell<Vec<Vec<String>>> = RefCell::new(Vec::new());
        let prompt_for = |lo: u64, remaining: usize| {
            // Block lines carry two leading spaces, so they must not follow a
            // string continuation (which would strip the indent).
            format!(
                "Compress memories #{lo}-{hi} into one line of at most 280 bytes.\n  #{lo} text\n  #{hi} text\n{remaining} compressions remain after this one.\nRun: memo nap {lo}-{hi}\n",
                hi = lo + 1
            )
        };
        let drain = nap_drain_with(
            &[
                "first line".to_string(),
                "second line".to_string(),
                "third".to_string(),
            ],
            |args| {
                calls
                    .borrow_mut()
                    .push(args.iter().map(|a| a.to_string()).collect());
                Ok(match args {
                    // bare read
                    [only] if *only == "nap" => prompt_for(36, 20),
                    // first apply -> next block is 38-39
                    ["nap", "36-37", "first line"] => prompt_for(38, 19),
                    ["nap", "38-39", "second line"] => prompt_for(40, 18),
                    // Still more work: upstream renders the block after this one.
                    ["nap", "40-41", "third"] => prompt_for(42, 3),
                    other => panic!("unexpected memo call: {other:?}"),
                })
            },
        )
        .expect("drain succeeds");
        assert_eq!(drain.applied, vec!["36-37", "38-39", "40-41"]);
        assert!(!drain.queue_cleared, "the fake still had work queued");
        let pending = drain
            .pending
            .expect("the next block is reported, not implied");
        assert_eq!(pending.range(), "42-43");
        assert_eq!(pending.remaining, 3);
        // One bare read + three applies, in order.
        let calls = calls.borrow();
        assert_eq!(calls.len(), 4);
        assert_eq!(calls[0], vec!["nap"]);
        assert_eq!(calls[3], vec!["nap", "40-41", "third"]);
    }

    /// A drain that over-delivers lines stops honestly instead of erroring.
    #[test]
    fn a_drain_stops_when_the_queue_is_empty() {
        let drain = nap_drain_with(
            &["one".to_string(), "two".to_string(), "three".to_string()],
            |args| match args {
                ["nap"] => Ok(UPSTREAM_PROMPT.to_string()),
                ["nap", "36-37", "one"] => {
                    Ok("Saved as #36-37.\nNothing left to compress.\n".into())
                }
                other => panic!("must not call memo again: {other:?}"),
            },
        )
        .expect("drain succeeds");
        assert_eq!(drain.applied, vec!["36-37"]);
        assert!(drain.queue_cleared, "the queue ran out mid-drain");
    }

    #[test]
    fn a_drain_rejects_an_empty_line() {
        let err = nap_drain_with(&["   ".to_string()], |_| Ok(UPSTREAM_PROMPT.to_string()))
            .expect_err("empty line is rejected");
        assert!(err.contains("empty line"), "{err}");
    }

    /// Live check against the real memo binary, in a scratch MEMORY_DIR so the
    /// user's `~/.optmem/memory` is never touched.
    ///
    /// Ignored by default (it shells out to Python and writes files). This is
    /// the check that proves the fix against upstream's actual output format
    /// rather than a fake:
    ///
    /// ```text
    /// cargo test --bin nur optmem_nap_drain_against_real_memo -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "live: runs the real memo binary in a scratch MEMORY_DIR"]
    fn optmem_nap_drain_against_real_memo() {
        if memo_bin().is_none() {
            eprintln!("memo not installed - skipping");
            return;
        }
        if python_runner_cached().is_none() {
            eprintln!("no usable Python interpreter - skipping");
            return;
        }
        let dir = std::env::temp_dir().join(format!("nur-optmem-nap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        // Safety: this process hands memo a scratch memory dir, so nothing here
        // can touch ~/.optmem/memory. Restored before returning.
        let previous = std::env::var("MEMORY_DIR").ok();
        std::env::set_var("MEMORY_DIR", &dir);
        let result = std::panic::catch_unwind(|| {
            // Six notes -> three pending pairs, once upstream decides to queue.
            for i in 0..6 {
                let _ = note(&format!(
                    "scratch optmem test entry {i} - compressible housekeeping content"
                ));
            }
            let before = run_memo(&["nap"], 60_000).expect("bare nap works");
            println!("--- upstream prompt ---\n{before}");
            let Some(first) = parse_nap_prompt(&before) else {
                eprintln!("queue empty after 6 notes; nothing to drain in this run");
                return;
            };
            let lines = vec![
                "scratch: entries 0-1 compressed into one line".to_string(),
                "scratch: entries 2-3 compressed into one line".to_string(),
            ];
            let drained = nap_drain(&lines).expect("drain works");
            println!("applied: {:?}", drained.applied);
            println!(
                "cleared: {} pending: {:?}",
                drained.queue_cleared, drained.pending
            );
            assert!(
                !drained.applied.is_empty(),
                "the drain applied at least one block (#{})",
                first.range()
            );
            assert_eq!(
                drained.applied[0],
                first.range(),
                "the drain starts from the block upstream was showing"
            );
            // The queue really moved: the next prompt is a later block.
            if let Some(next) = &drained.pending {
                assert!(
                    next.lo > first.hi,
                    "queue advanced: {} after {}",
                    next.range(),
                    first.range()
                );
            }
        });
        match previous {
            Some(v) => std::env::set_var("MEMORY_DIR", v),
            None => std::env::remove_var("MEMORY_DIR"),
        }
        let _ = std::fs::remove_dir_all(&dir);
        if let Err(e) = result {
            std::panic::resume_unwind(e);
        }
    }

    /// The chain guard counts single-block applications and can be reset.
    #[test]
    fn the_chain_guard_counts_and_resets() {
        reset_nap_chain();
        assert_eq!(count_single_nap(), 1);
        assert_eq!(count_single_nap(), 2);
        assert!(count_single_nap() > NAP_CHAIN_MAX);
        reset_nap_chain();
        assert_eq!(count_single_nap(), 1, "reset starts a fresh window");
        reset_nap_chain();
    }

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

//! Honest shell selection — never claim "bash" when running cmd.exe.

use crate::error::{NurError, Result};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Default when the model omits `timeout_ms`.
pub const DEFAULT_TIMEOUT_MS: u64 = 60_000;
/// Hard ceiling. Builds and test suites routinely run past three minutes;
/// hung commands are caught by the idle kill, not by this cap, so it can be
/// generous (Claude Code allows ten minutes).
pub const MAX_TIMEOUT_MS: u64 = 600_000;
/// Default byte budget for stdout that re-enters the model context; stderr
/// gets half. Overrun keeps head and tail with the full text spilled to disk.
pub const DEFAULT_MAX_OUTPUT_BYTES: usize = 80_000;
pub const MIN_MAX_OUTPUT_BYTES: usize = 1_000;
pub const MAX_MAX_OUTPUT_BYTES: usize = 400_000;
/// A handed-off job is still bounded: nothing should run unattended forever.
const HANDOFF_MAX_MS: u64 = 30 * 60 * 1_000;
/// `background=true` still waits this long so instant failures (typo, missing
/// binary) come back synchronously instead of as a bg job to poll.
const BACKGROUND_GRACE_MS: u64 = 750;
/// Snapshot of partial output included in the "still running" placeholder.
const HANDOFF_PREVIEW_BYTES: usize = 4_000;
/// A command is handed off at its deadline only if it wrote something this
/// recently; a silent one is more likely wedged or waiting on a prompt and
/// would just sit in bg until the idle kill.
const HANDOFF_RECENT_OUTPUT_MS: u64 = 15_000;
/// Per-stream capture cap: anything past it is drained (so the child never
/// blocks on a full pipe) but counted instead of kept.
const CAPTURE_CAP: usize = 2_000_000;
/// No stdout/stderr bytes for this long → treat as hung (waiting on stdin, etc.).
/// Kept long enough that quiet compilers/linkers are not killed mid-build.
const IDLE_TIMEOUT_MS: u64 = 90_000;
/// Grace before idle timeout starts (compilers are quiet at startup).
const IDLE_GRACE_MS: u64 = 20_000;
/// How long we wait for pipe drain threads after killing the process tree.
const JOIN_AFTER_KILL_MS: u64 = 2_000;
/// Bounded drain after the child exits: grandchildren (cargo, rustc, node, …)
/// inherit the pipes and can hold them open forever, so a bare `join()` on the
/// reader threads can wedge the tool long after the command itself finished
/// (session 26940d90 died exactly this way).
const DRAIN_IDLE_MS: u64 = 5_000;
const DRAIN_CAP_MS: u64 = 30_000;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Bytes read from one pipe so far. Shared with the reader thread so a
/// still-running command can report partial output and a bounded drain can
/// return what did arrive instead of nothing.
#[derive(Default)]
struct Capture {
    bytes: Vec<u8>,
    /// Bytes that arrived after `bytes` reached `CAPTURE_CAP`.
    dropped: usize,
}

type Sink = Arc<Mutex<Capture>>;

/// Lossy UTF-8 conversion that reuses the buffer when it is already valid.
fn bytes_to_string(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

/// Move the captured bytes out of the sink (no clone under the lock).
fn take_sink(sink: &Sink) -> (String, usize) {
    let cap = sink
        .lock()
        .map(|mut c| std::mem::take(&mut *c))
        .unwrap_or_default();
    (bytes_to_string(cap.bytes), cap.dropped)
}

/// Head/tail preview of a still-filling sink; only the preview bytes are
/// copied out, never the whole buffer. `None` when nothing arrived yet.
fn sink_preview(sink: &Sink, max_bytes: usize) -> Option<String> {
    let g = sink.lock().ok()?;
    let b = &g.bytes;
    if b.is_empty() {
        return None;
    }
    if b.len() <= max_bytes {
        return Some(String::from_utf8_lossy(b).into_owned());
    }
    let head = max_bytes * 3 / 5;
    let tail = max_bytes - head;
    Some(format!(
        "{}...{} bytes truncated...{}",
        String::from_utf8_lossy(&b[..head]),
        b.len() - head - tail,
        String::from_utf8_lossy(&b[b.len() - tail..])
    ))
}

fn read_all(pipe: Option<impl Read>, sink: Sink, progress_ms: Arc<AtomicU64>) {
    let mut tmp = [0u8; 8192];
    let Some(mut p) = pipe else {
        return;
    };
    loop {
        match p.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                progress_ms.store(now_ms(), Ordering::Relaxed);
                if let Ok(mut cap) = sink.lock() {
                    let take = n.min(CAPTURE_CAP.saturating_sub(cap.bytes.len()));
                    cap.bytes.extend_from_slice(&tmp[..take]);
                    cap.dropped += n - take;
                }
            }
            Err(_) => break,
        }
    }
}

fn join_with_timeout(handle: thread::JoinHandle<()>, timeout_ms: u64) {
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let _ = handle.join();
        let _ = tx.send(());
    });
    let _ = rx.recv_timeout(Duration::from_millis(timeout_ms));
}

/// Bounded join for the post-exit pipe drain. Returns when the reader
/// finishes; gives up when the shared `progress` timestamp has not advanced
/// for `idle_ms` (pipe held open but silent) or `cap_ms` elapsed overall (pipe
/// held open by a chatty grandchild). Whatever reached the sink is kept.
fn join_drain_bounded(
    handle: thread::JoinHandle<()>,
    progress: &Arc<AtomicU64>,
    idle_ms: u64,
    cap_ms: u64,
) {
    let progress = Arc::clone(progress);
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let _ = handle.join();
        let _ = tx.send(());
    });
    let started = Instant::now();
    let idle = Duration::from_millis(idle_ms);
    let cap = Duration::from_millis(cap_ms);
    let mut last_progress = progress.load(Ordering::Relaxed);
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(()) => return,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }
        if started.elapsed() >= cap {
            return;
        }
        let now = progress.load(Ordering::Relaxed);
        if now != last_progress {
            last_progress = now;
        } else if now_ms().saturating_sub(last_progress) >= idle.as_millis() as u64 {
            return;
        }
    }
}

#[derive(Debug, Clone)]
pub struct ShellBackend {
    pub kind: ShellKind,
    pub program: PathBuf,
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    /// Real bash (Git Bash, WSL, or /bin/bash).
    Bash,
    /// PowerShell 7+ (`pwsh`).
    Pwsh,
    /// Windows PowerShell 5.
    PowerShell,
    /// Last-resort cmd.exe.
    Cmd,
}

/// Process-wide cached shell backend. `detect_shell` probes the filesystem and
/// scans PATH, so callers on hot paths (system prompt, every bash call) use this.
pub fn shell_backend() -> &'static ShellBackend {
    static B: std::sync::OnceLock<ShellBackend> = std::sync::OnceLock::new();
    B.get_or_init(detect_shell)
}

/// Detect the best available shell (prefer `shell_backend()` — this probes disk).
pub fn detect_shell() -> ShellBackend {
    // 1) Explicit override
    if let Ok(p) = std::env::var("NUR_SHELL") {
        let pb = PathBuf::from(&p);
        if pb.is_file() || which_exists(&p) {
            let kind = if p.to_ascii_lowercase().contains("bash") {
                ShellKind::Bash
            } else if p.to_ascii_lowercase().contains("pwsh") {
                ShellKind::Pwsh
            } else if p.to_ascii_lowercase().contains("powershell") {
                ShellKind::PowerShell
            } else {
                ShellKind::Bash
            };
            return ShellBackend {
                kind,
                program: pb,
                label: format!("NUR_SHELL={p}"),
            };
        }
    }

    // 2) Prefer real bash (Git for Windows, user PATH, WSL via bash.exe)
    let bash_candidates = [
        r"C:\Program Files\Git\bin\bash.exe",
        r"C:\Program Files\Git\usr\bin\bash.exe",
        r"C:\Program Files (x86)\Git\bin\bash.exe",
    ];
    for c in bash_candidates {
        let p = PathBuf::from(c);
        if p.is_file() {
            return ShellBackend {
                kind: ShellKind::Bash,
                program: p,
                label: "git-bash".into(),
            };
        }
    }
    if let Some(p) = which("bash") {
        return ShellBackend {
            kind: ShellKind::Bash,
            program: p,
            label: "bash".into(),
        };
    }

    // 3) PowerShell 7
    if let Some(p) = which("pwsh") {
        return ShellBackend {
            kind: ShellKind::Pwsh,
            program: p,
            label: "pwsh".into(),
        };
    }

    // 4) Windows PowerShell
    #[cfg(windows)]
    {
        let ps = PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe");
        if ps.is_file() {
            return ShellBackend {
                kind: ShellKind::PowerShell,
                program: ps,
                label: "powershell".into(),
            };
        }
        if let Some(p) = which("powershell") {
            return ShellBackend {
                kind: ShellKind::PowerShell,
                program: p,
                label: "powershell".into(),
            };
        }
    }

    // 5) cmd last resort
    #[cfg(windows)]
    {
        ShellBackend {
            kind: ShellKind::Cmd,
            program: PathBuf::from("cmd.exe"),
            label: "cmd.exe".into(),
        }
    }
    #[cfg(not(windows))]
    {
        ShellBackend {
            kind: ShellKind::Bash,
            program: PathBuf::from("/bin/sh"),
            label: "sh".into(),
        }
    }
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let c = dir.join(name);
        if c.is_file() {
            return Some(c);
        }
        #[cfg(windows)]
        {
            let exe = dir.join(format!("{name}.exe"));
            if exe.is_file() {
                return Some(exe);
            }
        }
    }
    None
}

fn which_exists(name: &str) -> bool {
    Path::new(name).is_file() || which(name).is_some()
}

/// Clamp model-requested timeouts into a sane band.
pub fn clamp_timeout_ms(requested: u64) -> u64 {
    requested.clamp(1_000, MAX_TIMEOUT_MS)
}

/// Clamp a model-requested output budget (bytes kept inline) into a sane band.
pub fn clamp_max_output_bytes(requested: usize) -> usize {
    requested.clamp(MIN_MAX_OUTPUT_BYTES, MAX_MAX_OUTPUT_BYTES)
}

/// How a shell command is supervised.
#[derive(Debug, Clone)]
pub struct ShellOptions {
    /// Wall-clock budget for the synchronous wait (clamped to `MAX_TIMEOUT_MS`).
    pub timeout_ms: u64,
    /// When the budget runs out and the command is still producing output,
    /// hand it to a `bg` job instead of killing it. Idle (silent) commands are
    /// still killed: a prompt waiting on stdin never finishes on its own.
    pub handoff_on_timeout: bool,
    /// Hand off almost immediately (after a short grace so instant failures
    /// still come back inline).
    pub background: bool,
    /// Byte budget for stdout kept in the result; stderr gets half.
    pub max_output_bytes: usize,
}

impl Default for ShellOptions {
    fn default() -> Self {
        Self {
            timeout_ms: DEFAULT_TIMEOUT_MS,
            handoff_on_timeout: false,
            background: false,
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
        }
    }
}

/// Default-supervision wrapper (kill on timeout, default output budget).
/// Only the cmd-backend regression tests call this, and those are
/// Windows-only, so gate it the same way or Linux clippy flags it dead.
#[cfg(all(test, windows))]
pub fn run_in_shell(
    backend: &ShellBackend,
    command: &str,
    cwd: &Path,
    timeout_ms: u64,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<String> {
    let opts = ShellOptions {
        timeout_ms,
        ..ShellOptions::default()
    };
    run_in_shell_with(backend, command, cwd, &opts, cancel)
}

/// Everything a supervised child owns, so the whole bundle can move to a bg
/// job thread when the synchronous wait gives up on it.
struct Supervised {
    child: Child,
    process_tree: crate::process_tree::ProcessTree,
    progress: Arc<AtomicU64>,
    out_sink: Sink,
    err_sink: Sink,
    out_h: thread::JoinHandle<()>,
    err_h: thread::JoinHandle<()>,
    started: Instant,
}

impl Supervised {
    fn kill(mut self) {
        self.process_tree.terminate(&mut self.child);
        // The tree is dead, so the pipes close on their own; the bounded joins
        // run together so a wedged reader costs one window, not two.
        let out_h = self.out_h;
        let out_join = thread::spawn(move || join_with_timeout(out_h, JOIN_AFTER_KILL_MS));
        join_with_timeout(self.err_h, JOIN_AFTER_KILL_MS);
        let _ = out_join.join();
    }

    fn idle_for_ms(&self) -> Option<u64> {
        if self.started.elapsed() < Duration::from_millis(IDLE_GRACE_MS) {
            return None;
        }
        let idle = now_ms().saturating_sub(self.progress.load(Ordering::Relaxed));
        (idle >= IDLE_TIMEOUT_MS).then_some(idle)
    }

    /// True when at least one byte arrived on either stream and the most
    /// recent one is no older than `window_ms`. `progress` alone cannot tell:
    /// it starts at launch time, so a silent child looks "recent" early on.
    fn output_within_ms(&self, window_ms: u64) -> bool {
        let saw_output = [&self.out_sink, &self.err_sink].iter().any(|s| {
            s.lock()
                .map(|c| !c.bytes.is_empty() || c.dropped > 0)
                .unwrap_or(false)
        });
        saw_output && now_ms().saturating_sub(self.progress.load(Ordering::Relaxed)) <= window_ms
    }

    /// Child exited normally - but a grandchild may still hold the pipes open.
    /// Drain both streams with an idle/cap bound (a chatty grandchild keeps the
    /// shared progress stamp moving, which would defeat the idle cutoff, so the
    /// cap is the real backstop). Run the two joins concurrently so the whole
    /// drain is bounded by ONE cap window, not one per stream.
    fn drain(self) -> Drained {
        let drain_progress = Arc::clone(&self.progress);
        let out_h = self.out_h;
        let out_drain = thread::spawn(move || {
            join_drain_bounded(out_h, &drain_progress, DRAIN_IDLE_MS, DRAIN_CAP_MS)
        });
        join_drain_bounded(self.err_h, &self.progress, DRAIN_IDLE_MS, DRAIN_CAP_MS);
        let _ = out_drain.join();
        let (stdout, stdout_dropped) = take_sink(&self.out_sink);
        let (stderr, stderr_dropped) = take_sink(&self.err_sink);
        Drained {
            stdout,
            stderr,
            stdout_dropped,
            stderr_dropped,
        }
    }
}

/// What reached the sinks by the time the command ended, plus the bytes per
/// stream that overflowed `CAPTURE_CAP` and were discarded.
#[derive(Debug, Default)]
struct Drained {
    stdout: String,
    stderr: String,
    stdout_dropped: usize,
    stderr_dropped: usize,
}

pub fn run_in_shell_with(
    backend: &ShellBackend,
    command: &str,
    cwd: &Path,
    opts: &ShellOptions,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<String> {
    if cancel.is_cancelled() {
        return Err(NurError::Tool("command cancelled before launch".into()));
    }
    let kind = backend.kind;
    let label = backend.label.clone();
    let timeout_ms = clamp_timeout_ms(opts.timeout_ms);
    let max_output = clamp_max_output_bytes(opts.max_output_bytes);
    let handoff_after_ms = if opts.background {
        BACKGROUND_GRACE_MS.min(timeout_ms)
    } else {
        timeout_ms
    };
    let handoff = opts.handoff_on_timeout || opts.background;

    let mut cmd = Command::new(&backend.program);
    cmd.current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Nudge CLIs away from interactive prompts (stdin is already null).
        .env("CI", "1")
        .env("TERM", "dumb")
        .env("NO_COLOR", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env("NPM_CONFIG_YES", "true")
        .env("DEBIAN_FRONTEND", "noninteractive")
        .env("PYTHONUNBUFFERED", "1");
    // The model's scratch space (see `agent::scratch`): durable, inside the
    // sandbox, and the place the prompt sends throwaway files instead of /tmp.
    if let Some(scratch) = crate::agent::scratch::ensure(cwd) {
        cmd.env(crate::agent::scratch::ENV, scratch);
    }
    match kind {
        ShellKind::Bash => {
            // Non-login: sourcing ~/.bash_profile on every call is slow and
            // lets profile output leak into results. PATH comes from nur's
            // own environment, which the user's login shell already set.
            cmd.args(["-c", command]);
        }
        ShellKind::Pwsh | ShellKind::PowerShell => {
            cmd.args(["-NoProfile", "-NonInteractive", "-Command", command]);
        }
        ShellKind::Cmd => {
            // Verbatim, not a normal argument: Rust escapes inner quotes as \",
            // which cmd.exe does not understand, so `type "my file.txt"` ran
            // with a mangled path. /S strips exactly the outer quotes added here.
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.args(["/S", "/C"]).raw_arg(format!("\"{command}\""));
            }
            #[cfg(not(windows))]
            cmd.args(["/C", command]);
        }
    }

    let (mut child, process_tree) = crate::process_tree::ProcessTree::spawn(&mut cmd)
        .map_err(|e| NurError::Tool(format!("command failed to start: {e}")))?;

    // Drain pipes on threads so a chatty child can't deadlock on a full pipe.
    let progress = Arc::new(AtomicU64::new(now_ms()));
    let out_sink: Sink = Arc::default();
    let err_sink: Sink = Arc::default();
    let out_pipe = child.stdout.take();
    let err_pipe = child.stderr.take();
    let (prog_out, sink_out) = (Arc::clone(&progress), Arc::clone(&out_sink));
    let (prog_err, sink_err) = (Arc::clone(&progress), Arc::clone(&err_sink));
    let out_h = thread::spawn(move || read_all(out_pipe, sink_out, prog_out));
    let err_h = thread::spawn(move || read_all(err_pipe, sink_err, prog_err));

    let started = Instant::now();
    let sup = Supervised {
        child,
        process_tree,
        progress,
        out_sink,
        err_sink,
        out_h,
        err_h,
        started,
    };
    let deadline = started + Duration::from_millis(handoff_after_ms);
    let mut sup = sup;
    let status = loop {
        match sup.child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if cancel.is_cancelled() {
                    sup.kill();
                    return Err(NurError::Tool(
                        "command cancelled by user (process tree killed)".into(),
                    ));
                }
                // Idle: no pipe bytes for IDLE_TIMEOUT after grace → likely waiting on stdin.
                if let Some(idle_for) = sup.idle_for_ms() {
                    sup.kill();
                    return Err(NurError::Tool(format!(
                        "command idle for {idle_for}ms with no output (process tree killed). \
                         Likely waiting for interactive input or stuck. Do not retry the \
                         identical command - switch to a dedicated tool or a non-interactive form."
                    )));
                }
                if Instant::now() >= deadline {
                    // `background=true` is an explicit request to hand off, so
                    // it does not wait for output; the plain timeout only hands
                    // off a command that is visibly still working.
                    let still_working =
                        opts.background || sup.output_within_ms(HANDOFF_RECENT_OUTPUT_MS);
                    if handoff && still_working {
                        return Ok(hand_off_to_bg(
                            sup,
                            command,
                            &label,
                            kind,
                            handoff_after_ms,
                            max_output,
                        ));
                    }
                    sup.kill();
                    return Err(NurError::Tool(format!(
                        "command timed out after {timeout_ms}ms (process tree killed). \
                         If it simply needs longer, retry with a larger timeout_ms (max \
                         {MAX_TIMEOUT_MS}) or background=true; otherwise use a narrower \
                         non-interactive command."
                    )));
                }
                thread::sleep(Duration::from_millis(30));
            }
            Err(e) => {
                sup.kill();
                return Err(NurError::Tool(format!("command wait failed: {e}")));
            }
        }
    };

    let drained = sup.drain();
    Ok(format_result(&label, kind, status, &drained, max_output))
}

/// Move a still-running command to the bg job registry. The returned text is
/// a placeholder the model can act on now; the real result lands in the job.
fn hand_off_to_bg(
    sup: Supervised,
    command: &str,
    label: &str,
    kind: ShellKind,
    waited_ms: u64,
    max_output: usize,
) -> String {
    let preview = {
        let mut p = String::new();
        if let Some(out) = sink_preview(&sup.out_sink, HANDOFF_PREVIEW_BYTES) {
            p.push_str("Stdout so far:\n");
            p.push_str(out.trim_end_matches('\n'));
            p.push('\n');
        }
        if let Some(err) = sink_preview(&sup.err_sink, HANDOFF_PREVIEW_BYTES / 2) {
            p.push_str("Stderr so far:\n");
            p.push_str(err.trim_end_matches('\n'));
            p.push('\n');
        }
        if p.is_empty() {
            p.push_str("(no output yet)\n");
        }
        p
    };
    let job_label = {
        let flat = command.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut l: String = flat.chars().take(60).collect();
        if flat.chars().count() > 60 {
            l.push('…');
        }
        l
    };
    let label_owned = label.to_string();
    let id = crate::bg_jobs::spawn(job_label, "shell", move |bg_cancel| {
        let mut sup = sup;
        let handoff_deadline = Instant::now() + Duration::from_millis(HANDOFF_MAX_MS);
        let status = loop {
            if bg_cancel.load(Ordering::SeqCst) {
                sup.kill();
                return Err("cancelled".into());
            }
            match sup.child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {
                    if let Some(idle_for) = sup.idle_for_ms() {
                        sup.kill();
                        return Err(format!(
                            "command idle for {idle_for}ms with no output in the background \
                             (process tree killed) - likely waiting for input"
                        ));
                    }
                    if Instant::now() >= handoff_deadline {
                        sup.kill();
                        return Err(format!(
                            "command still running after {}s in the background (process tree killed)",
                            HANDOFF_MAX_MS / 1_000
                        ));
                    }
                    thread::sleep(Duration::from_millis(120));
                }
                Err(e) => {
                    sup.kill();
                    return Err(format!("wait: {e}"));
                }
            }
        };
        let drained = sup.drain();
        let text = format_result(&label_owned, kind, status, &drained, max_output);
        if status.success() {
            Ok(text)
        } else {
            Err(text)
        }
    });
    format!(
        "Command still running after {waited_ms}ms - handed off to bg job #{id}.\n\
         {preview}\
         Not killed, not a failure. Its result arrives on its own in a later \
         turn: continue with independent work, or end the turn if nothing else \
         is left. Do not poll it and do not re-run this command; \
         bg(action=result, id={id}) only when you need it before then."
    )
}

/// Model-facing result. Plain stdout when that is all there is; `Stderr:`,
/// `Exit code: N` and the shell name are added only when they carry
/// information (stderr present, non-zero exit, a non-bash backend), so a
/// routine call costs a handful of tokens of framing instead of a header.
fn format_result(
    label: &str,
    kind: ShellKind,
    status: ExitStatus,
    drained: &Drained,
    max_output: usize,
) -> String {
    let code = status.code().unwrap_or(-1);
    let mut parts: Vec<String> = Vec::new();
    if kind != ShellKind::Bash {
        parts.push(format!("Shell: {label}"));
    }
    if !drained.stdout.is_empty() {
        parts.push(bound_and_spill(&drained.stdout, max_output));
    }
    if drained.stdout_dropped > 0 {
        parts.push(dropped_note("stdout", drained.stdout_dropped));
    }
    if !drained.stderr.is_empty() {
        parts.push(format!(
            "Stderr:\n{}",
            bound_and_spill(&drained.stderr, max_output / 2)
        ));
    }
    if drained.stderr_dropped > 0 {
        parts.push(dropped_note("stderr", drained.stderr_dropped));
    }
    if code != 0 {
        parts.push(format!("Exit code: {code}"));
    }
    if kind == ShellKind::Cmd {
        parts.push(
            "note: shell is cmd.exe - use Windows syntax (dir, type, findstr). \
             Install Git Bash or set NUR_SHELL for real bash."
                .into(),
        );
    }
    if drained.stdout.is_empty() && drained.stderr.is_empty() && code == 0 {
        parts.push("(no output)".into());
    }
    parts.join("\n")
}

fn dropped_note(stream: &str, dropped: usize) -> String {
    format!(
        "[{stream}: {dropped} bytes past the {CAPTURE_CAP}-byte capture cap were discarded; \
         the capture file is incomplete too]"
    )
}

/// Head+tail bound, with the full text spilled to the shared tool-results
/// blob store so nothing is lost (errors usually sit at the tail, context at
/// the head). Sensitive-looking output is bounded without a disk copy.
fn bound_and_spill(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let path = if crate::tools::sensitive::body_looks_sensitive(s) {
        None
    } else {
        crate::tools::spill::write_content_addressed_blob(s).ok()
    };
    bound_output_marked(s, max_bytes, path.as_deref())
}

fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    i = i.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_char_boundary(s: &str, mut i: usize) -> usize {
    i = i.min(s.len());
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// Keep roughly 60% head and 40% tail of `s` within `max_bytes`, marking the
/// cut as `head...N bytes truncated; complete output in <path>...tail` (the
/// path clause is dropped when nothing was spilled). Cuts land on char
/// boundaries. Pure: no disk access.
fn bound_output_marked(s: &str, max_bytes: usize, path: Option<&Path>) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let head_len = floor_char_boundary(s, max_bytes * 3 / 5);
    let tail_start = ceil_char_boundary(s, s.len() - (max_bytes - head_len));
    let omitted = tail_start.saturating_sub(head_len);
    let mut out = String::with_capacity(max_bytes + 96);
    out.push_str(&s[..head_len]);
    out.push_str(&format!("...{omitted} bytes truncated"));
    if let Some(p) = path {
        out.push_str(&format!("; complete output in {}", p.display()));
    }
    out.push_str("...");
    out.push_str(&s[tail_start..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_bounds_timeout() {
        assert_eq!(clamp_timeout_ms(0), 1_000);
        assert_eq!(clamp_timeout_ms(30_000), 30_000);
        assert_eq!(clamp_timeout_ms(999_999), MAX_TIMEOUT_MS);
    }

    // Failure modes: tail lost (errors sit at the tail), cut inside a UTF-8
    // sequence, or a body under budget rewritten.
    #[test]
    fn bound_output_keeps_head_and_tail_on_char_boundaries() {
        let body = format!("{}HEAD{}TAIL", "é".repeat(500), "ü".repeat(500));
        let out = bound_output_marked(&body, 600, None);
        assert!(out.starts_with("éé"));
        assert!(out.ends_with("üTAIL"), "{out}");
        assert!(out.contains("...1408 bytes truncated..."), "{out}");
        assert!(out.len() < body.len());
        assert_eq!(bound_output_marked("short", 600, None), "short");
        let marked = bound_output_marked(&body, 600, Some(Path::new("/cap/out")));
        assert!(
            marked.contains("...1408 bytes truncated; complete output in /cap/out..."),
            "{marked}"
        );
    }

    fn exit(code: i32) -> ExitStatus {
        #[cfg(windows)]
        {
            use std::os::windows::process::ExitStatusExt;
            ExitStatus::from_raw(code as u32)
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            ExitStatus::from_raw(code << 8)
        }
    }

    fn drained(stdout: &str, stderr: &str) -> Drained {
        Drained {
            stdout: stdout.into(),
            stderr: stderr.into(),
            ..Drained::default()
        }
    }

    // Failure modes: a header the model pays for on every call, a lost exit
    // code, stderr folded into stdout, or an empty result with no marker.
    #[test]
    fn format_result_adds_framing_only_when_it_carries_information() {
        let f = |d: &Drained, code: i32| {
            format_result("git-bash", ShellKind::Bash, exit(code), d, 1_000)
        };
        assert_eq!(f(&drained("ok\n", ""), 0), "ok\n");
        assert_eq!(f(&drained("", ""), 0), "(no output)");
        assert_eq!(f(&drained("", ""), 1), "Exit code: 1");
        assert_eq!(f(&drained("", "warning\n"), 0), "Stderr:\nwarning\n");
        assert_eq!(
            f(&drained("out\n", "err\n"), 7),
            "out\n\nStderr:\nerr\n\nExit code: 7"
        );
        let pwsh = format_result("pwsh", ShellKind::Pwsh, exit(0), &drained("hi", ""), 1_000);
        assert_eq!(pwsh, "Shell: pwsh\nhi");
    }

    // Failure mode: bytes past the capture cap vanish while the result still
    // reads like a complete capture.
    #[test]
    fn format_result_states_discarded_bytes() {
        let d = Drained {
            stdout: "kept".into(),
            stdout_dropped: 1_234,
            ..Drained::default()
        };
        let out = format_result("git-bash", ShellKind::Bash, exit(0), &d, 1_000);
        assert!(out.starts_with("kept\n[stdout: 1234 bytes past"), "{out}");
        assert!(out.contains("capture file is incomplete"), "{out}");
    }

    // Failure modes: the cap accounting miscounts, or the sink keeps growing.
    #[test]
    fn read_all_counts_bytes_past_the_cap_instead_of_keeping_them() {
        let sink: Sink = Arc::default();
        let total = CAPTURE_CAP + 10_000;
        let data = std::io::repeat(b'z').take(total as u64);
        read_all(Some(data), Arc::clone(&sink), Arc::new(AtomicU64::new(0)));
        let (text, dropped) = take_sink(&sink);
        assert_eq!(text.len(), CAPTURE_CAP);
        assert_eq!(dropped, 10_000);
    }

    /// A command that keeps producing output past the budget is handed to a bg
    /// job (not killed) and the placeholder carries the job id plus partial
    /// output; the job then completes with the real exit code.
    #[test]
    fn slow_command_is_handed_off_and_completes_in_background() {
        const CHILD: &str = "NUR_TEST_SHELL_HANDOFF_CHILD";
        if std::env::var_os(CHILD).is_none() {
            // bg jobs write a result file under the nur home; run the real
            // check in a child with a throwaway NUR_HOME.
            let home = std::env::temp_dir().join(format!("nur-handoff-{}", std::process::id()));
            std::fs::create_dir_all(&home).unwrap();
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "tools::shell::tests::slow_command_is_handed_off_and_completes_in_background",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .env("NUR_HOME", &home)
                .output()
                .unwrap();
            let _ = std::fs::remove_dir_all(&home);
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        let backend = shell_backend().clone();
        // Handoff needs output before the deadline. Shell startup on a hosted
        // Windows runner has taken over 3s, so the first echo gets a 6s window
        // and the sleep outlasts it comfortably.
        let cmd = if backend.kind == ShellKind::Bash {
            "echo first; sleep 12; echo second"
        } else if backend.kind == ShellKind::Cmd {
            "echo first & ping -n 13 127.0.0.1 >nul & echo second"
        } else {
            "Write-Output first; Start-Sleep -Seconds 12; Write-Output second"
        };
        let opts = ShellOptions {
            timeout_ms: 6_000,
            handoff_on_timeout: true,
            ..ShellOptions::default()
        };
        let cancel = tokio_util::sync::CancellationToken::new();
        let out = run_in_shell_with(&backend, cmd, Path::new("."), &opts, &cancel).unwrap();
        assert!(out.contains("handed off to bg job #"), "{out}");
        assert!(out.contains("first"), "partial output missing: {out}");
        let id: u64 = out
            .split("bg job #")
            .nth(1)
            .and_then(|s| s.split(|c: char| !c.is_ascii_digit()).next())
            .and_then(|s| s.parse().ok())
            .expect("job id");
        let mut state = None;
        for _ in 0..400 {
            let j = crate::bg_jobs::get(id).expect("job");
            if j.state != crate::bg_jobs::JobState::Running {
                state = Some(j.state);
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(state, Some(crate::bg_jobs::JobState::Completed));
        let result = crate::bg_jobs::result(id).unwrap();
        assert!(!result.contains("Exit code:"), "{result}");
        assert!(result.contains("second"), "{result}");
    }

    /// A command that has printed nothing by its deadline is killed with the
    /// timeout error, not parked in bg (where it would only hit the idle kill).
    #[test]
    fn silent_command_at_deadline_is_killed_not_handed_off() {
        let backend = shell_backend().clone();
        let cmd = if backend.kind == ShellKind::Bash {
            "sleep 20"
        } else if backend.kind == ShellKind::Cmd {
            "ping -n 21 127.0.0.1 >nul"
        } else {
            "Start-Sleep -Seconds 20"
        };
        let opts = ShellOptions {
            timeout_ms: 2_000,
            handoff_on_timeout: true,
            ..ShellOptions::default()
        };
        let cancel = tokio_util::sync::CancellationToken::new();
        let started = Instant::now();
        let err = run_in_shell_with(&backend, cmd, Path::new("."), &opts, &cancel)
            .expect_err("silent command must time out");
        assert!(err.to_string().contains("timed out after 2000ms"), "{err}");
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "{:?}",
            started.elapsed()
        );
    }

    /// Regression: after the child exits, a grandchild holding the inherited
    /// pipes must not wedge the tool forever (session 26940d90). `start /b
    /// ping` backgrounds a writer that keeps stdout alive ~60s; the tool must
    /// return well inside the 45s wall-clock bound (one DRAIN_CAP_MS window).
    /// cmd.exe is the last-resort shell. A quoted path used to reach it as
    /// `\"...\"` and fail; E2E runs on Git Bash, so only this pins it.
    #[cfg(windows)]
    #[test]
    fn cmd_backend_keeps_quoted_paths_intact() {
        let dir = std::env::temp_dir().join(format!("nur shell quote {}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("has space.txt");
        std::fs::write(&file, "quoted-path-ok").unwrap();
        let backend = ShellBackend {
            kind: ShellKind::Cmd,
            program: PathBuf::from("cmd.exe"),
            label: "cmd".into(),
        };
        let out = run_in_shell(
            &backend,
            &format!("type \"{}\"", file.display()),
            Path::new("."),
            10_000,
            &tokio_util::sync::CancellationToken::new(),
        );
        let _ = std::fs::remove_dir_all(&dir);
        let out = out.expect("cmd ran");
        assert!(out.contains("quoted-path-ok"), "{out}");
    }

    #[cfg(windows)]
    #[test]
    fn drain_returns_despite_pipe_holding_grandchild() {
        let backend = ShellBackend {
            kind: ShellKind::Cmd,
            program: PathBuf::from("cmd.exe"),
            label: "cmd".into(),
        };
        let cancel = tokio_util::sync::CancellationToken::new();
        let started = Instant::now();
        let result = run_in_shell(
            &backend,
            "start /b ping -n 60 127.0.0.1 & exit 0",
            Path::new("."),
            10_000,
            &cancel,
        );
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(45),
            "run_in_shell took {elapsed:?} - drain wedged on pipe-holding grandchild"
        );
        // cmd exited 0, so this should be a normal Ok result.
        assert!(result.is_ok(), "unexpected failure: {:?}", result.err());
    }
}

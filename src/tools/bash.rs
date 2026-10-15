use super::sandbox;
use super::shell::{
    clamp_max_output_bytes, clamp_timeout_ms, run_in_shell_with, shell_backend, ShellOptions,
    DEFAULT_MAX_OUTPUT_BYTES, DEFAULT_TIMEOUT_MS, MAX_MAX_OUTPUT_BYTES, MAX_TIMEOUT_MS,
};
use super::{arg_str, arg_u64, Tool, ToolContext};
use crate::error::{NurError, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Why a command was locked out. A deadline timeout is retryable with a longer
/// `timeout_ms`; an idle kill or a user cancel is not helped by more time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FailureKind {
    Timeout,
    Idle,
    Cancelled,
}

#[derive(Clone, Copy, Debug)]
struct Failure {
    at: Instant,
    kind: FailureKind,
    /// The `timeout_ms` the failed attempt ran with.
    timeout_ms: u64,
}

/// Recent failing bash fingerprints (cwd + normalized command). Only timeout /
/// idle / cancel / hang-kill are tracked — ordinary non-zero exits can be retried
/// with a different approach without a 15-minute lockout.
fn recent_failures() -> &'static Mutex<HashMap<String, Failure>> {
    static RECENT_FAILURES: OnceLock<Mutex<HashMap<String, Failure>>> = OnceLock::new();
    RECENT_FAILURES.get_or_init(|| Mutex::new(HashMap::new()))
}
const FAILURE_TTL: Duration = Duration::from_secs(15 * 60);
const MAX_TRACKED_FAILURES: usize = 64;

pub struct Bash;

impl Tool for Bash {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        // Honest description — name kept as `bash` for model familiarity.
        "Run a non-interactive shell command in the workspace cwd (not a full OS sandbox; \
         non-login shell, pipelines and && chains allowed). \
         On Windows prefers Git Bash, then pwsh/PowerShell, then cmd.exe (reported in output). \
         Default timeout 60s (max 600s). A command still producing output at the timeout is \
         handed to a bg job (result says 'still running' with the job id; fetch it later with \
         bg(action=result, id=N)); background=true hands off right away for builds/test suites. \
         Idle commands with no output are killed; the whole process tree dies with the command. \
         Output keeps head+tail within max_output_length (full text spilled to disk). \
         Not for search or file reads: a grep/rg/findstr/Select-String command is refused \
         (use sgrep/grep/glob; list_dir/read_file for contents). \
         A command that hit its deadline may be retried once with a larger timeout_ms or \
         background=true; identical idle/cancelled commands are refused for 15 minutes."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {"type": "string"},
                "timeout_ms": {
                    "type": "integer",
                    "description": format!(
                        "Timeout in ms (default {DEFAULT_TIMEOUT_MS}, hard max {MAX_TIMEOUT_MS}). \
                         At the deadline a still-running command is handed to bg, not killed."
                    )
                },
                "background": {
                    "type": "boolean",
                    "description": "Hand off to a bg job almost immediately (long builds, test suites, servers that log). Default false."
                },
                "max_output_length": {
                    "type": "integer",
                    "description": format!(
                        "Max bytes of stdout kept inline (default {DEFAULT_MAX_OUTPUT_BYTES} or the configured tool_result_max_chars if lower, max {MAX_MAX_OUTPUT_BYTES}); stderr gets half. Head and tail are kept, the full text is spilled."
                    )
                }
            },
            "required": ["command"]
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        if sandbox::is_dangerous_workspace(&ctx.cwd) {
            return Err(NurError::Tool(
                "refused: workspace is filesystem root - start nur from a project directory \
                 (or --cwd) before running shell commands"
                    .into(),
            ));
        }

        let command = arg_str(args, "command")?;
        let timeout_ms =
            clamp_timeout_ms(arg_u64(args, "timeout_ms").unwrap_or(DEFAULT_TIMEOUT_MS));
        let background = args
            .get("background")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let config = crate::config::load_config().ok();
        let max_output_bytes = match arg_u64(args, "max_output_length") {
            Some(n) => clamp_max_output_bytes(n as usize),
            None => {
                default_max_output_bytes(config.as_ref().map(|c| c.tool_result_max_chars as usize))
            }
        };
        let handoff_on_timeout = config
            .as_ref()
            .map(|c| c.shell_timeout_handoff)
            .unwrap_or(true);
        let opts = ShellOptions {
            timeout_ms,
            handoff_on_timeout,
            background,
            max_output_bytes,
        };

        if let Some(reason) = check_destructive(&command, &ctx.cwd) {
            return Err(NurError::Tool(reason));
        }
        if let Some(reason) = check_hang_prone(&command) {
            return Err(NurError::Tool(reason));
        }
        if let Some(reason) = refuse_recent_failure(&ctx.cwd, &command, timeout_ms, background) {
            return Err(NurError::Tool(reason));
        }
        let sgrep_first = config.as_ref().map(|c| c.tools.sgrep_first).unwrap_or(true);
        if sgrep_first {
            if let Some(reason) = refuse_shell_search(&command) {
                return Err(NurError::Tool(reason));
            }
        }

        match run_in_shell_with(shell_backend(), &command, &ctx.cwd, &opts, &ctx.cancel) {
            // A bg handoff comes back as Ok and is not a failure to lock out.
            Ok(out) => Ok(out),
            Err(e) => {
                // Only lock out hang/timeout/cancel — not ordinary tool refusals
                // from this Err path that are already terminal.
                if let Some(kind) = failure_kind(&e.to_string()) {
                    note_failure(&ctx.cwd, &command, kind, timeout_ms);
                }
                Err(e)
            }
        }
    }
}

/// Inline budget when the model does not ask for one. The shell result is the
/// only bounding a bash result gets (the generic tool-result spill skips it),
/// so the default must honor the user's `tool_result_max_chars` cap; an
/// explicit `max_output_length` is the model's own choice and is honored.
fn default_max_output_bytes(inline_cap: Option<usize>) -> usize {
    match inline_cap {
        Some(cap) if cap > 0 => clamp_max_output_bytes(cap.min(DEFAULT_MAX_OUTPUT_BYTES)),
        _ => DEFAULT_MAX_OUTPUT_BYTES,
    }
}

fn fingerprint(cwd: &Path, cmd: &str) -> String {
    let cwd_key = cwd
        .canonicalize()
        .unwrap_or_else(|_| cwd.to_path_buf())
        .to_string_lossy()
        .to_ascii_lowercase();
    let cmd_key = cmd.split_whitespace().collect::<Vec<_>>().join(" ");
    format!("{cwd_key}\0{cmd_key}")
}

/// Classify a shell error by the message `run_in_shell_with` produced; other
/// errors (refusals, spawn failures, non-zero exits) are not locked out.
fn failure_kind(msg: &str) -> Option<FailureKind> {
    if msg.contains("timed out") {
        Some(FailureKind::Timeout)
    } else if msg.contains("idle for") {
        Some(FailureKind::Idle)
    } else if msg.contains("cancelled") {
        Some(FailureKind::Cancelled)
    } else {
        None
    }
}

fn note_failure(cwd: &Path, cmd: &str, kind: FailureKind, timeout_ms: u64) {
    let key = fingerprint(cwd, cmd);
    if key.ends_with('\0') {
        return;
    }
    if let Ok(mut map) = recent_failures().lock() {
        map.retain(|_, f| f.at.elapsed() < FAILURE_TTL);
        if map.len() >= MAX_TRACKED_FAILURES {
            if let Some(old) = map.iter().min_by_key(|(_, f)| f.at).map(|(k, _)| k.clone()) {
                map.remove(&old);
            }
        }
        // A retry that fails again must not lower the recorded timeout.
        let timeout_ms = map
            .get(&key)
            .map_or(timeout_ms, |prev| prev.timeout_ms.max(timeout_ms));
        map.insert(
            key,
            Failure {
                at: Instant::now(),
                kind,
                timeout_ms,
            },
        );
    }
}

/// `None` when the command may run. A deadline timeout is retryable with a
/// strictly larger `timeout_ms` or as a background job, because the first
/// run proved only that the budget was too small; idle kills and cancels are
/// refused until the TTL passes.
fn refuse_recent_failure(
    cwd: &Path,
    cmd: &str,
    timeout_ms: u64,
    background: bool,
) -> Option<String> {
    let key = fingerprint(cwd, cmd);
    let Ok(mut map) = recent_failures().lock() else {
        return None;
    };
    map.retain(|_, f| f.at.elapsed() < FAILURE_TTL);
    let prev = *map.get(&key)?;
    let shown = cmd.split_whitespace().collect::<Vec<_>>().join(" ");
    match prev.kind {
        FailureKind::Timeout if background || timeout_ms > prev.timeout_ms => None,
        FailureKind::Timeout => Some(format!(
            "refused: identical bash command already timed out after {}ms in this cwd: `{shown}`. \
             Retry only with a larger timeout_ms (max {MAX_TIMEOUT_MS}) or background=true, \
             or change the command.",
            prev.timeout_ms
        )),
        FailureKind::Idle => Some(format!(
            "refused: identical bash command already hung with no output in this cwd: `{shown}`. \
             More time will not help. Use list_dir/read_file/grep/glob, change the command, \
             or explain the blocker."
        )),
        FailureKind::Cancelled => Some(format!(
            "refused: identical bash command was cancelled by the user in this cwd: `{shown}`. \
             Do not retry it; ask before running it again or change the command."
        )),
    }
}

/// A shell command whose point is searching the workspace is refused: the
/// `sgrep`/`grep`/`glob` tools are the search path (sandboxed, .gitignore,
/// never hang, and sgrep ranks by intent). That is the command when a
/// searcher (`grep`, `rg`, `ag`, `ack`, `findstr`, `Select-String`) starts the
/// line, or when it filters a bare file read (`cat x | grep y`). A searcher
/// filtering another command's output (`git log | grep fix`) is not a
/// workspace search and runs; so does `echo "grep this"`.
fn refuse_shell_search(cmd: &str) -> Option<String> {
    const SEARCHERS: [&str; 7] = ["grep", "rg", "ag", "ack", "findstr", "select-string", "sls"];
    const READERS: [&str; 5] = ["cat", "type", "get-content", "gc", "head"];
    let segments = command_segments(cmd);
    let base_of = |seg: &str| -> Option<String> {
        let s = strip_env_assigns(seg);
        let first = s.split_whitespace().next()?;
        let base = first
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(first)
            .to_ascii_lowercase();
        Some(base.trim_end_matches(".exe").to_string())
    };
    // `cd src && rg foo` is still a search; skip the setup segments.
    const SETUP: [&str; 5] = ["cd", "pushd", "export", "set", "source"];
    let start = segments
        .iter()
        .position(|seg| base_of(seg).is_some_and(|b| !SETUP.contains(&b.as_str())))?;
    let first = base_of(segments[start])?;
    let searcher = if SEARCHERS.contains(&first.as_str()) {
        first
    } else if READERS.contains(&first.as_str()) {
        segments
            .get(start + 1)
            .and_then(|s| base_of(s))
            .filter(|b| SEARCHERS.contains(&b.as_str()))?
    } else {
        return None;
    };
    Some(format!(
        "refused: `{searcher}` as a shell command - search goes through the tools. \
         sgrep query=\"<what the code does>\" ranks passages by intent (exact identifier hits \
         first); grep pattern=<regex> for an identifier or string you already know \
         (fixed_strings=true for literal text, glob/type to narrow); glob for file names. \
         Filtering another command's output (`git log | grep fix`) still runs."
    ))
}

fn command_segments(cmd: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = cmd.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'|' | b';' | b'\n' => {
                out.push(&cmd[start..i]);
                start = i + 1;
                i += 1;
            }
            b'&' if i + 1 < bytes.len() && bytes[i + 1] == b'&' => {
                out.push(&cmd[start..i]);
                start = i + 2;
                i += 2;
            }
            _ => i += 1,
        }
    }
    out.push(&cmd[start..]);
    out
}

fn strip_env_assigns(seg: &str) -> &str {
    let mut s = seg.trim();
    while let Some((key, rest)) = s.split_once('=') {
        if key.is_empty() || key.contains(' ') || key.contains('/') || key.contains('\\') {
            break;
        }
        // VAR=value rest…  (value may be quoted lightly; take next token boundary)
        let rest = rest.trim_start();
        if let Some(stripped) = rest.strip_prefix('"') {
            if let Some(end) = stripped.find('"') {
                s = stripped[end + 1..].trim_start();
                continue;
            }
        }
        if let Some(stripped) = rest.strip_prefix('\'') {
            if let Some(end) = stripped.find('\'') {
                s = stripped[end + 1..].trim_start();
                continue;
            }
        }
        let mut parts = rest.splitn(2, char::is_whitespace);
        let _val = parts.next();
        s = parts.next().unwrap_or("").trim_start();
        if s.is_empty() {
            break;
        }
    }
    s
}

/// Commands that almost never finish under a non-interactive agent (stdin null).
/// Matches segment *starts* so `echo "npm run dev"` is allowed.
fn check_hang_prone(cmd: &str) -> Option<String> {
    for seg in command_segments(cmd) {
        let s = strip_env_assigns(seg).to_ascii_lowercase();
        let s = s.trim();
        if s.is_empty() {
            continue;
        }

        let starters: &[(&str, &str)] = &[
            ("cargo watch", "use a one-shot `cargo check` / `cargo test`"),
            (
                "npm run dev",
                "dev servers hang forever - use build/test instead",
            ),
            (
                "npm start",
                "start/dev servers hang forever under the agent",
            ),
            (
                "pnpm dev",
                "dev servers hang forever - use build/test instead",
            ),
            (
                "pnpm start",
                "start/dev servers hang forever under the agent",
            ),
            (
                "yarn dev",
                "dev servers hang forever - use build/test instead",
            ),
            (
                "yarn start",
                "start/dev servers hang forever under the agent",
            ),
            ("next dev", "dev servers hang forever - use `next build`"),
            (
                "npx vite",
                "dev servers hang forever - use a one-shot build",
            ),
            (
                "pnpm vite",
                "dev servers hang forever - use a one-shot build",
            ),
            (
                "yarn vite",
                "dev servers hang forever - use a one-shot build",
            ),
            ("vite ", "dev servers hang forever - use a one-shot build"),
            ("webpack-dev-server", "dev servers hang forever"),
            ("webpack serve", "dev servers hang forever"),
            ("flask run", "dev servers hang forever"),
            ("tail -f", "follow/watch modes hang - use a bounded read"),
            ("get-content -wait", "follow/watch modes hang"),
            ("read -p", "interactive prompts hang (stdin is null)"),
            ("read -n", "interactive prompts hang (stdin is null)"),
            ("cmd /c pause", "interactive pause hangs"),
            ("pause", "interactive pause hangs"),
            ("get-credential", "interactive prompts hang"),
            ("while true", "infinite loops hang - use a bounded command"),
            ("while :", "infinite loops hang - use a bounded command"),
            (
                "python -m http.server",
                "servers hang forever under the agent",
            ),
            (
                "python3 -m http.server",
                "servers hang forever under the agent",
            ),
            ("npx serve", "servers hang forever under the agent"),
            ("live-server", "servers hang forever under the agent"),
        ];
        for (pat, hint) in starters {
            if s == *pat || s.starts_with(pat) {
                return Some(format!(
                    "refused hang-prone command starting with '{pat}' ({hint}). \
                     Prefer dedicated tools or a one-shot non-interactive command."
                ));
            }
        }
        if s == "vite" {
            return Some(
                "refused hang-prone `vite` (dev server). Prefer a one-shot build/test command."
                    .into(),
            );
        }
        // uvicorn … --reload
        if (s.starts_with("uvicorn ") || s.starts_with("uvicorn\t")) && s.contains("--reload") {
            return Some(
                "refused hang-prone uvicorn --reload (dev server). Prefer a one-shot check.".into(),
            );
        }
        if let Some(rest) = s.strip_prefix("npm run ") {
            let script = rest.split_whitespace().next().unwrap_or("");
            if matches!(
                script,
                "dev" | "start" | "watch" | "serve" | "develop" | "preview"
            ) {
                return Some(format!(
                    "refused hang-prone `npm run {script}` - use a one-shot build/test script instead"
                ));
            }
        }
    }
    None
}

fn check_destructive(cmd: &str, cwd: &std::path::Path) -> Option<String> {
    let lower = cmd.to_lowercase();
    let _ = cwd;

    // Signatures, not commands: these survive any segment parser, so they
    // stay textual.
    if lower.contains("encodedcommand")
        || lower.contains("-enc ")
        || lower.contains("frombase64string")
    {
        return Some(format!(
            "refused: encoded PowerShell obscures intent: {cmd}"
        ));
    }
    if lower.contains(":(){") && lower.contains("|:&") {
        return Some("refused: fork bomb detected".into());
    }

    let parts = crate::agent::permissions::shell_parts(cmd);
    // Substitution or a file redirect breaks the segment view; sweep the raw
    // text as a backstop for those, then still check the visible segments.
    if parts.opaque {
        const OPAQUE_BACKSTOP: &[&str] = &[
            "rm -rf /",
            "rm -fr /",
            "rm -rf ~",
            "rm -fr ~",
            "rm -rf $home",
            "rm -rf %userprofile%",
            "rm -rf ..",
            ">/dev/sd",
            ">/dev/nvme",
            ">/dev/hd",
            "format c:",
            "cipher /w:",
            r"\\?\c:",
        ];
        for d in OPAQUE_BACKSTOP {
            if lower.contains(d) {
                return Some(format!("refused destructive pattern '{d}' in: {cmd}"));
            }
        }
    }
    for seg in &parts.commands {
        if let Some(reason) = destructive_segment(seg) {
            return Some(format!("{reason}: {cmd}"));
        }
    }
    None
}

/// Words of one simple command, quotes stripped (the denylist reads the
/// command the shell would run, not the string that hides it). Backslashes
/// stay literal outside double quotes so Windows device paths (`\\?\c:\`)
/// survive parsing.
fn shell_words(seg: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut chars = seg.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some('"') => match c {
                '"' => quote = None,
                '\\' => {
                    if let Some(&n) = chars.peek() {
                        if matches!(n, '"' | '\\') {
                            cur.push(chars.next().unwrap());
                            continue;
                        }
                    }
                    cur.push(c);
                }
                _ => cur.push(c),
            },
            Some('\'') => {
                if c == '\'' {
                    quote = None;
                } else {
                    cur.push(c);
                }
            }
            // quote is only ever Some('"') or Some('\''); keep the wildcard
            // for the compiler.
            Some(_) => cur.push(c),
            None => match c {
                '\'' | '"' => quote = Some(c),
                c if c.is_whitespace() => {
                    if !cur.is_empty() {
                        out.push(std::mem::take(&mut cur));
                    }
                }
                _ => cur.push(c),
            },
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// A target whose recursive deletion is catastrophic: filesystem root, the
/// user's home (however spelled), a drive root, or a parent-directory climb.
fn catastrophic_target(word: &str) -> bool {
    let w = word.trim_matches(['"', '\'']);
    let w = w.strip_prefix(r"\\?\").unwrap_or(w);
    let mut t = w.trim_end_matches(['/', '\\']);
    // `rm -rf /*` and `rm -rf ~/*` spell the same thing as their prefix.
    if let Some(stripped) = t.strip_suffix('*') {
        t = stripped.trim_end_matches(['/', '\\']);
    }
    if t.is_empty() {
        return !matches!(w, "" | "*"); // "/" and friends collapsed to nothing
    }
    let l = t.to_ascii_lowercase();
    if matches!(
        l.as_str(),
        "/" | "~"
            | ".."
            | "$home"
            | "${home}"
            | "$userprofile"
            | "${userprofile}"
            | "%userprofile%"
            | "%home%"
            | "$env:userprofile"
            | "$env:home"
            | "/root"
    ) {
        return true;
    }
    // A pure parent-climb (`..`, `../..`, `..\..\..`) escapes the workspace.
    if t.contains("..") && t.split(['/', '\\']).all(|s| s == "..") {
        return true;
    }
    // Drive root: `c:` / `c:\` / `d:/` …
    let b = l.as_bytes();
    if b.len() == 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return true;
    }
    // The literal home path, with either slash style.
    if let Some(home) = crate::config::home_dir() {
        let h = home.to_string_lossy().replace('/', "\\");
        let h = h.trim_end_matches('\\').to_ascii_lowercase();
        let t_norm = t.replace('/', "\\");
        if !h.is_empty() && t_norm.trim_end_matches('\\').to_ascii_lowercase() == h {
            return true;
        }
    }
    false
}

/// True when a short/long flag cluster carries the recursive bit: `-r`,
/// `-rf`, `-fr`, `-Rf`, `--recursive`, and PowerShell prefix spellings of
/// `-Recurse` (`-r` … `-recurse`). Short clusters read as unix (`-force`
/// without an `r` is not recursive); longer ones as PowerShell prefixes.
fn has_recursive_flag(args: &[String]) -> bool {
    args.iter().any(|a| {
        let l = a.to_ascii_lowercase();
        if let Some(rest) = l.strip_prefix("--") {
            return rest == "recursive";
        }
        if let Some(rest) = l.strip_prefix('-') {
            if rest.is_empty() || !rest.chars().all(|c| c.is_ascii_alphabetic()) {
                return false;
            }
            if rest.len() <= 3 {
                return rest.contains('r');
            }
            return "recurse".starts_with(rest);
        }
        false
    })
}

/// A cmd-style switch (`/s`, `/q`, `/fs:ntfs`) vs a unix absolute path (`/`,
/// `/usr`). Only single-letter-ish slash words count as switches.
fn is_cmd_switch(word: &str) -> bool {
    let Some(rest) = word.strip_prefix('/') else {
        return false;
    };
    !rest.is_empty() && rest.len() <= 3 && rest.chars().all(|c| c.is_ascii_alphanumeric())
}

/// One simple command, parsed: program + words, matched against the
/// catastrophic-target rules. Returns the refusal reason when destructive.
fn destructive_segment(seg: &str) -> Option<String> {
    let words = shell_words(seg);
    if words.is_empty() {
        return None;
    }
    let program = crate::agent::permissions::program(seg)?;
    let prog_at = words
        .iter()
        .position(|w| !(w.contains('=') && !w.starts_with('=') && !w.starts_with('-')))?;
    let args: Vec<String> = words[prog_at + 1..].to_vec();
    // Non-flag words are candidate targets. For rm/Remove-Item only `-`
    // introduces a flag, so `/` and `/*` stay visible as targets; for the
    // cmd builtins `/s`-style switches are filtered out too.
    let dash_targets: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    let cmd_targets: Vec<&String> = args
        .iter()
        .filter(|a| !a.starts_with('-') && !is_cmd_switch(a))
        .collect();

    // A device write hides as a plain argument or a shell redirect token.
    for (i, a) in args.iter().enumerate() {
        let l = a.to_ascii_lowercase();
        let dev = l
            .strip_prefix("of=")
            .or_else(|| l.strip_prefix("of ="))
            .map(str::trim_start);
        let redirect_target = (a == ">" || a == "1>").then(|| args.get(i + 1)).flatten();
        let inline_redirect = l
            .strip_prefix('>')
            .filter(|s| !s.is_empty() && !s.starts_with('&') && !s.starts_with('>'));
        for cand in [dev, redirect_target.map(String::as_str), inline_redirect]
            .into_iter()
            .flatten()
        {
            let c = cand.to_ascii_lowercase();
            if c.starts_with("/dev/sd")
                || c.starts_with("/dev/nvme")
                || c.starts_with("/dev/hd")
                || c.starts_with("/dev/mmcblk")
            {
                return Some(format!("refused raw disk write to {cand}"));
            }
        }
    }

    match program.as_str() {
        // Elevation wrappers: the payload follows the wrapper's flags.
        "sudo" | "doas" | "pkexec" | "gsudo" | "runas" => {
            let mut rest = &args[..];
            while let Some(first) = rest.first() {
                if first.starts_with('-') {
                    rest = if matches!(first.as_str(), "-u" | "-g" | "-h" | "-p") && rest.len() > 1
                    {
                        &rest[2..]
                    } else {
                        &rest[1..]
                    };
                } else {
                    break;
                }
            }
            if rest.is_empty() {
                return None;
            }
            destructive_segment(&rest.join(" "))
        }
        // `-Command <script>` arrives as one quoted word; parse it again.
        "powershell" | "pwsh" => {
            for a in &args {
                if a.contains(' ') {
                    if let Some(r) = destructive_segment(a) {
                        return Some(r);
                    }
                }
            }
            None
        }
        // `cmd /c <line>` re-parses the words after the switch.
        "cmd" => {
            if let Some(at) = args
                .iter()
                .position(|a| a.eq_ignore_ascii_case("/c") || a.eq_ignore_ascii_case("/k"))
            {
                let inner = args[at + 1..].join(" ");
                if !inner.is_empty() {
                    return destructive_segment(&inner);
                }
            }
            None
        }
        // `rm` on unix; Remove-Item's alias in PowerShell.
        "rm" | "remove-item" | "ri" => {
            if has_recursive_flag(&args) && dash_targets.iter().any(|t| catastrophic_target(t)) {
                let t = dash_targets
                    .iter()
                    .find(|t| catastrophic_target(t))
                    .map(|s| s.as_str())
                    .unwrap_or("");
                return Some(format!("refused recursive delete of {t}"));
            }
            None
        }
        "rmdir" | "rd" => {
            let recursive =
                args.iter().any(|a| a.eq_ignore_ascii_case("/s")) || has_recursive_flag(&args);
            if recursive && cmd_targets.iter().any(|t| catastrophic_target(t)) {
                return Some("refused recursive rmdir of a root/home path".into());
            }
            // Unix `rmdir` only removes empty dirs; no recurse, no block.
            None
        }
        "del" | "erase" => {
            let recursive = args.iter().any(|a| a.eq_ignore_ascii_case("/s"));
            if recursive && cmd_targets.iter().any(|t| catastrophic_target(t)) {
                return Some("refused recursive del of a root/home path".into());
            }
            None
        }
        "format" => {
            if args.iter().any(|a| {
                let b = a.as_bytes();
                b.len() == 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
            }) {
                return Some("refused format of a drive".into());
            }
            None
        }
        "cipher" => {
            if args
                .iter()
                .any(|a| a.to_ascii_lowercase().starts_with("/w"))
            {
                return Some("refused cipher /w (wipes free space)".into());
            }
            None
        }
        "dd" => None, // device writes caught by the of=/redirect scan above
        "shutdown" | "halt" | "poweroff" | "reboot" => Some(format!(
            "refused {program}: it stops the machine mid-session"
        )),
        p if p == "mkfs" || p.starts_with("mkfs.") || p == "mke2fs" => {
            Some("refused filesystem creation on a device".into())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn refuses_dev_servers_but_allows_echo() {
        assert!(check_hang_prone("npm run dev").is_some());
        assert!(check_hang_prone("cargo watch -x check").is_some());
        assert!(check_hang_prone("cargo check").is_none());
        assert!(check_hang_prone("npm test").is_none());
        assert!(
            check_hang_prone(r#"echo "npm run dev""#).is_none(),
            "echo of hang phrase must not refuse"
        );
        assert!(check_hang_prone("FOO=1 npm run dev").is_some());
    }

    /// Failure modes the denylist exists for: recursive deletion of root or
    /// home in any flag order or quoting, drive wipes, disk/device writes,
    /// and mid-session machine shutdowns - without refusing ordinary project
    /// cleanup like `rm -rf target`.
    #[test]
    fn destructive_table() {
        let cwd = PathBuf::from("C:/proj");
        for bad in [
            "rm -rf /",
            "rm -rf /*",
            "rm -fr /",
            "sudo rm -rf /",
            "rm -r /",
            "rm --recursive --force /",
            "rm -rf ~",
            "rm -fr ~",
            "rm -rf \"$HOME\"",
            "rm -rf ${HOME}",
            "rm -rf %USERPROFILE%",
            "rm -rf $env:USERPROFILE",
            "rm ~ -rf",
            "rm -rf ..",
            "rm -rf ../..",
            "Remove-Item -Recurse -Force C:\\",
            "remove-item -r -force $home",
            "Remove-Item -recurse $env:USERPROFILE",
            "ri -Recurse ~",
            "rmdir /s /q C:\\",
            "rd /s %USERPROFILE%",
            "del /f /s /q c:",
            "del /s %USERPROFILE%",
            "format c:",
            "format D: /q",
            "cipher /w:C:\\",
            "mkfs.ext4 /dev/sda1",
            "mkfs -t ext4 /dev/sda",
            "dd if=/dev/zero of=/dev/sda",
            "dd if=img of=/dev/nvme0n1",
            "echo x > /dev/sda",
            "cat img.bin > /dev/mmcblk0",
            "shutdown -h now",
            "shutdown /s",
            "shutdown",
            "halt -f",
            "poweroff",
            "reboot",
            "powershell -enc SQBFAFgA",
            "powershell -EncodedCommand SQBFAFgA",
            ":(){ :|:& };:",
            "rm -rf \\\\?\\c:\\",
            "sudo -u root rm -rf ~",
            "powershell -Command \"Remove-Item -Recurse -Force C:\\\"",
            "cmd /c rmdir /s /q C:\\",
            "pwsh -c \"rm -r $HOME\"",
        ] {
            assert!(
                check_destructive(bad, &cwd).is_some(),
                "should refuse: {bad}"
            );
        }
        for ok in [
            "rm -rf target",
            "rm -rf node_modules",
            "rm -rf ./dist",
            "rm -f src/main.rs",
            "rm target/debug/old.exe",
            "Remove-Item -Force build.log",
            "rmdir empty_dir",
            "del /q build.log",
            "dd if=/dev/sda of=disk.img bs=4M count=1",
            "dd if=input.bin of=output.bin",
            "echo rm -rf /",
            "echo \"Remove-Item -Recurse -Force C:\\\"",
            "git clean -fdx",
            "cargo clean",
            "cat README.md | grep format",
        ] {
            assert!(check_destructive(ok, &cwd).is_none(), "should allow: {ok}");
        }
    }

    #[test]
    fn failure_fingerprint_is_cwd_scoped() {
        let a = PathBuf::from("C:/proj-a");
        let b = PathBuf::from("C:/proj-b");
        let cmd = "unique-nur-bash-fail-test-abc";
        note_failure(&a, cmd, FailureKind::Idle, 60_000);
        assert!(refuse_recent_failure(&a, cmd, 60_000, false).is_some());
        assert!(refuse_recent_failure(&b, cmd, 60_000, false).is_none());
    }

    /// Failure modes: a timed-out command is refused even when the model asks
    /// for more time (the old behaviour onno hit), or an identical retry with
    /// the same budget slips through and burns another full timeout.
    #[test]
    fn timeout_lockout_lifts_for_a_larger_budget_or_background() {
        let cwd = PathBuf::from("C:/proj-timeout");
        let cmd = "unique-nur-bash-timeout-test-xyz";
        note_failure(&cwd, cmd, FailureKind::Timeout, 60_000);
        let same = refuse_recent_failure(&cwd, cmd, 60_000, false).expect("same budget refused");
        assert!(
            same.contains("60000ms") && same.contains("timeout_ms"),
            "{same}"
        );
        assert!(refuse_recent_failure(&cwd, cmd, 59_000, false).is_some());
        assert!(refuse_recent_failure(&cwd, cmd, 120_000, false).is_none());
        assert!(refuse_recent_failure(&cwd, cmd, 60_000, true).is_none());

        // The retry fails too: the recorded budget rises, a third attempt at
        // 120s is refused, 180s is allowed.
        note_failure(&cwd, cmd, FailureKind::Timeout, 120_000);
        assert!(refuse_recent_failure(&cwd, cmd, 120_000, false).is_some());
        assert!(refuse_recent_failure(&cwd, cmd, 180_000, false).is_none());
    }

    #[test]
    fn idle_and_cancel_lockouts_ignore_the_budget() {
        let cwd = PathBuf::from("C:/proj-idle");
        let idle = "unique-nur-bash-idle-test-xyz";
        note_failure(&cwd, idle, FailureKind::Idle, 60_000);
        let msg = refuse_recent_failure(&cwd, idle, 600_000, true).expect("idle refused");
        assert!(msg.contains("no output"), "{msg}");

        let cancelled = "unique-nur-bash-cancel-test-xyz";
        note_failure(&cwd, cancelled, FailureKind::Cancelled, 60_000);
        let msg = refuse_recent_failure(&cwd, cancelled, 600_000, true).expect("cancel refused");
        assert!(msg.contains("cancelled by the user"), "{msg}");
    }

    #[test]
    fn only_hang_class_errors_are_locked_out() {
        assert_eq!(
            failure_kind("command timed out after 60000ms (process tree killed)."),
            Some(FailureKind::Timeout)
        );
        assert_eq!(
            failure_kind("command idle for 30000ms with no output"),
            Some(FailureKind::Idle)
        );
        assert_eq!(
            failure_kind("command cancelled by user"),
            Some(FailureKind::Cancelled)
        );
        assert_eq!(failure_kind("exit code 1\nstderr: boom"), None);
        assert_eq!(failure_kind("failed to spawn shell: not found"), None);
    }

    // Failure modes: a searcher named inside an argument (`echo "grep this"`,
    // `git grep`) refused; a legitimate filter of another command's output
    // (`git log | grep`, `cargo test 2>&1 | grep FAILED`) refused; a workspace
    // search behind an env assignment, a full Windows path, a PowerShell
    // cmdlet or a bare file read allowed through.
    #[test]
    fn shell_search_refusal_targets_workspace_searches_only() {
        assert!(refuse_shell_search("echo \"grep this\"").is_none());
        assert!(refuse_shell_search("git grep -n foo").is_none());
        assert!(refuse_shell_search("git log --oneline | grep fix").is_none());
        assert!(refuse_shell_search("cargo test 2>&1 | grep FAILED").is_none());
        assert!(refuse_shell_search("cargo test").is_none());
        assert!(refuse_shell_search("rg -n foo src").is_some());
        assert!(refuse_shell_search("cd src && rg -n foo").is_some());
        assert!(refuse_shell_search("LC_ALL=C grep -r foo .").is_some());
        assert!(refuse_shell_search("cat a.txt | grep foo").is_some());
        let win = refuse_shell_search("C:\\tools\\rg.exe foo src").unwrap();
        assert!(win.starts_with("refused: `rg`"), "{win}");
        assert!(refuse_shell_search("Select-String -Path *.rs -Pattern foo").is_some());
        assert!(refuse_shell_search("sls foo *.rs").is_some());
    }
}

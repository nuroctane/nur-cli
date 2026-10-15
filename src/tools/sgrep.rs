//! `sgrep` tool: semantic code search over the workspace, backed by the
//! Context.dev `sgrep` binary (<https://github.com/context-dot-dev/sgrep>,
//! MIT). Two modes:
//!
//! - **discover** (`query` only): sgrep indexes the search root, ranks every
//!   chunk by static embeddings + BM25 with exact identifier hits first, and
//!   returns the top `count` chunks.
//! - **rerank** (`query` + `pattern`): ripgrep finds the candidate lines for
//!   the regex (with three lines of context) and sgrep ranks only those
//!   passages by the query, so a broad grep comes back as the few blocks that
//!   matter instead of hundreds of lines.
//!
//! When the binary is missing the tool degrades to the ripgrep/lexical path of
//! `grep` and says so, so a model that leads with `sgrep` still gets an answer.

use super::search_util::{display_path, find_rg, normalize_user_glob};
use super::{arg_str, arg_u64, resolve_path, Tool, ToolContext};
use crate::error::{NurError, Result};
use crate::sgrep::{prepend_path, rg_path_dir, sgrep_bin, INSTALL_HINT};
use serde::Deserialize;
use serde_json::Value;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub struct Sgrep;

const DEFAULT_COUNT: u64 = 5;
const MAX_COUNT: u64 = 25;
/// Lines of context rg attaches around each match in rerank mode; the
/// context is what gives sgrep a passage to rank instead of one line.
const RERANK_CONTEXT: u32 = 3;
/// Most rg JSON fed to sgrep. Past this the pattern was too broad for a
/// rerank to mean much, so the run is cut short and marked.
const RERANK_INPUT_BUDGET: usize = 8 * 1024 * 1024;
/// Wall clock for one sgrep run. The first run on a repository embeds every
/// chunk (later runs hit the index) and the very first run of all downloads
/// a 32 MB model, so this is deliberately generous; cancellation is immediate.
const SGREP_BUDGET: Duration = Duration::from_secs(120);
/// A chunk longer than this is cut in the result; the model can read_file.
const MAX_CHUNK_BYTES: usize = 4000;

impl Tool for Sgrep {
    fn name(&self) -> &str {
        "sgrep"
    }

    fn description(&self) -> &str {
        "Semantic code search (Context.dev sgrep, local, no API). Ask in plain English what you \
         are looking for - 'where are retries with backoff', 'code that validates the session \
         token' - and get the top-ranked code chunks (path:start-end then the text), exact \
         identifier hits first. Pass pattern to rerank a ripgrep run instead: rg finds the \
         candidate lines for the regex, sgrep returns only the passages that match the query's \
         intent. Prefer this over grep when you know what the code does but not what it is \
         called, or when a grep would return more than a screen. Use grep for an exact \
         identifier or regex, glob for file names."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "What you are looking for, in plain English (identifiers welcome)"},
                "path": {"type": "string", "description": "Directory or file to search (default cwd)"},
                "pattern": {"type": "string", "description": "Optional ripgrep regex. When set, only lines matching it (plus context) are ranked by the query"},
                "glob": {"type": "string", "description": "With pattern: only files matching this glob, e.g. *.rs, src/**/*.ts"},
                "count": {"type": "integer", "default": DEFAULT_COUNT, "description": format!("How many chunks to return (max {MAX_COUNT})")},
                "model": {"type": "string", "enum": ["auto", "code", "text"], "default": "auto", "description": "Embedding model: text for prose corpora, code for source, auto picks"}
            },
            "required": ["query"]
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        execute_with(sgrep_bin(), args, ctx)
    }
}

/// The tool body with the binary already resolved, so the fallback path can
/// be exercised without touching the environment.
fn execute_with(bin: Option<String>, args: &Value, ctx: &ToolContext) -> Result<String> {
    let query = arg_str(args, "query")?;
    let query = query.trim().to_string();
    if query.is_empty() {
        return Err(NurError::Tool("sgrep: query is empty".into()));
    }
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let opt_str = |name: &str| {
        args.get(name)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let pattern = opt_str("pattern");
    let glob = opt_str("glob").map(|g| normalize_user_glob(&g));
    let count = arg_u64(args, "count")
        .unwrap_or(DEFAULT_COUNT)
        .clamp(1, MAX_COUNT);
    let model = match opt_str("model").as_deref() {
        Some("code") => "code",
        Some("text") => "text",
        _ => "auto",
    };
    let root = resolve_path(&ctx.cwd, path)?;

    let Some(bin) = bin else {
        return lexical_fallback(
            args,
            ctx,
            &query,
            pattern.as_deref(),
            format!("sgrep not installed - searched with grep instead. Install: {INSTALL_HINT}"),
        );
    };

    let req = SemanticRequest {
        query: &query,
        root: &root,
        pattern: pattern.as_deref(),
        glob: glob.as_deref(),
        file_type: None,
        fixed_strings: false,
        case_insensitive: false,
        count,
        model,
    };
    match semantic(&bin, &req, ctx) {
        Semantic::Chunks(out) => Ok(out),
        Semantic::NoMatches => Ok("no matches".into()),
        Semantic::Cancelled => Ok("[search cancelled]".into()),
        Semantic::RgError(e) => Err(NurError::Tool(format!("sgrep: {e}"))),
        Semantic::Unreadable(e) => Err(NurError::Tool(format!("sgrep: unreadable output: {e}"))),
        Semantic::Unavailable(reason) => lexical_fallback(
            args,
            ctx,
            &query,
            pattern.as_deref(),
            format!("{reason} - searched with grep instead"),
        ),
    }
}

/// One semantic search, with the binary already resolved. `grep` hands
/// itself over to this when a call was really a job for sgrep.
pub(crate) struct SemanticRequest<'a> {
    pub query: &'a str,
    /// Resolved search root (directory or file).
    pub root: &'a Path,
    /// Rerank mode when set: ripgrep gathers the candidates for this regex.
    pub pattern: Option<&'a str>,
    pub glob: Option<&'a str>,
    pub file_type: Option<&'a str>,
    pub fixed_strings: bool,
    pub case_insensitive: bool,
    pub count: u64,
    pub model: &'a str,
}

/// What one sgrep run produced. Callers that have their own answer (grep)
/// keep it on anything but `Chunks`; the sgrep tool maps the rest to its
/// lexical fallback or an error.
pub(crate) enum Semantic {
    /// Rendered chunks with any notes attached.
    Chunks(String),
    NoMatches,
    Cancelled,
    /// ripgrep could not gather the rerank candidates (bad regex, missing rg).
    RgError(String),
    /// sgrep exited 0 with output that is not its JSON.
    Unreadable(String),
    /// sgrep did not run to completion; the reason is phrased for a note.
    Unavailable(String),
}

pub(crate) fn semantic(bin: &str, req: &SemanticRequest<'_>, ctx: &ToolContext) -> Semantic {
    let (dir, target) = search_target(req.root);
    let prefix = display_prefix(&ctx.cwd, &dir);

    let (rg_input, rg_note) = match req.pattern {
        Some(pat) => match rg_json(ctx, &dir, &target, pat, req) {
            Ok(Some(feed)) => (Some(feed.bytes), feed.note),
            Ok(None) => return Semantic::NoMatches,
            Err(e) if e == "search cancelled" => return Semantic::Cancelled,
            Err(e) => return Semantic::RgError(e),
        },
        None => (None, None),
    };

    let mut cmd = Command::new(bin);
    cmd.current_dir(&dir)
        .arg(req.query)
        .arg(&target)
        .arg("-n")
        .arg(req.count.to_string())
        .arg("--json")
        .arg("--model")
        .arg(req.model);
    if rg_input.is_some() {
        cmd.arg("--stdin");
    }
    if let Some(rg_dir) = rg_path_dir() {
        prepend_path(&mut cmd, &rg_dir);
    }
    let run = match run_sgrep(cmd, rg_input.as_deref(), ctx) {
        Ok(r) => r,
        Err(e) => return Semantic::Unavailable(format!("sgrep could not run ({e})")),
    };
    match run.exit {
        Exit::Code(0) => {}
        Exit::Code(1) => return Semantic::NoMatches,
        Exit::Cancelled => return Semantic::Cancelled,
        Exit::Timeout => {
            return Semantic::Unavailable(format!(
                "sgrep gave up after {}s (first run indexes the repository; retry later)",
                SGREP_BUDGET.as_secs()
            ))
        }
        Exit::Code(_) => {
            let reason = first_line(&run.stderr).unwrap_or("unknown error");
            return Semantic::Unavailable(format!("sgrep failed: {reason}"));
        }
    }
    let chunks = match parse_chunks(&run.stdout) {
        Ok(c) => c,
        Err(e) => return Semantic::Unreadable(e.to_string()),
    };
    if chunks.is_empty() {
        return Semantic::NoMatches;
    }
    let mut out = render(&chunks, &prefix);
    for note in [rg_note, run.note].into_iter().flatten() {
        out.push_str(&format!("\n[{note}]"));
    }
    Semantic::Chunks(out)
}

/// One result chunk as sgrep prints it with `--json`.
#[derive(Debug, Deserialize, PartialEq)]
pub(crate) struct Chunk {
    pub path: String,
    pub start: usize,
    pub end: usize,
    pub content: String,
}

pub(crate) fn parse_chunks(stdout: &str) -> std::result::Result<Vec<Chunk>, serde_json::Error> {
    let s = stdout.trim();
    if s.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(s)
}

/// `path:start-end` then the chunk text, chunks separated by a blank line.
/// Paths come out of sgrep relative to the search root with OS separators;
/// `prefix` puts them back relative to the cwd.
pub(crate) fn render(chunks: &[Chunk], prefix: &str) -> String {
    let mut out = String::new();
    for (i, c) in chunks.iter().enumerate() {
        if i > 0 {
            out.push_str("\n\n");
        }
        let path = c.path.replace('\\', "/");
        let path = path.strip_prefix("./").unwrap_or(&path);
        out.push_str(&format!("{prefix}{path}:{}-{}\n", c.start, c.end));
        out.push_str(&clip_chunk(c.content.trim_end_matches(['\n', '\r'])));
    }
    out
}

fn clip_chunk(content: &str) -> std::borrow::Cow<'_, str> {
    if content.len() <= MAX_CHUNK_BYTES {
        return std::borrow::Cow::Borrowed(content);
    }
    let mut cut = MAX_CHUNK_BYTES;
    while !content.is_char_boundary(cut) {
        cut -= 1;
    }
    // Cut at the last full line inside the budget so the tail is readable.
    let cut = content[..cut].rfind('\n').unwrap_or(cut);
    std::borrow::Cow::Owned(format!(
        "{}\n[... {} more bytes - read_file for the rest]",
        &content[..cut],
        content.len() - cut
    ))
}

/// `(process cwd, path argument)`: sgrep runs inside the search root so the
/// rg JSON paths it reads and the chunk paths it prints are root-relative.
fn search_target(root: &Path) -> (PathBuf, PathBuf) {
    if root.is_file() {
        let dir = root.parent().unwrap_or(root).to_path_buf();
        let name = root.file_name().map(PathBuf::from).unwrap_or_default();
        (dir, name)
    } else {
        (root.to_path_buf(), PathBuf::from("."))
    }
}

fn display_prefix(cwd: &Path, dir: &Path) -> String {
    let s = display_path(cwd, dir);
    if s.is_empty() || s == "." {
        String::new()
    } else {
        format!("{}/", s.trim_end_matches('/'))
    }
}

/// ripgrep's JSON for sgrep to rank, plus anything the model should know
/// about how it was gathered (cut at the budget, unreadable entries).
struct RgFeed {
    bytes: Vec<u8>,
    note: Option<String>,
}

/// Run ripgrep in `dir` with `--json -C 3` and return its output for sgrep to
/// rank. `Ok(None)` when nothing matched. No `--max-columns`: sgrep checks
/// each passage against the file on disk and rejects altered lines.
fn rg_json(
    ctx: &ToolContext,
    dir: &Path,
    target: &Path,
    pattern: &str,
    req: &SemanticRequest<'_>,
) -> std::result::Result<Option<RgFeed>, String> {
    let rg = find_rg().ok_or_else(|| "ripgrep is required for pattern reranking".to_string())?;
    let mut cmd = Command::new(rg);
    cmd.current_dir(dir)
        .arg("--no-config")
        .arg("--no-messages")
        .arg("--json")
        .arg("--hidden")
        .arg("--max-filesize=1M")
        .arg("-C")
        .arg(RERANK_CONTEXT.to_string());
    for d in super::search_util::HARD_EXCLUDES {
        cmd.arg(format!("--glob=!**/{d}/"));
    }
    if let Some(g) = req.glob {
        cmd.arg("--glob").arg(g);
    }
    if let Some(t) = req.file_type {
        cmd.arg("--type").arg(t);
    }
    if req.fixed_strings {
        cmd.arg("-F");
    }
    if req.case_insensitive {
        cmd.arg("-i");
    }
    cmd.arg("-e").arg(pattern).arg("--").arg(target);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("cannot start ripgrep: {e}"))?;
    let mut stdout = child.stdout.take().ok_or("ripgrep stdout")?;
    let mut stderr = child.stderr.take().ok_or("ripgrep stderr")?;
    let err_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = (&mut stderr).take(64 * 1024).read_to_string(&mut s);
        let _ = std::io::copy(&mut stderr, &mut std::io::sink());
        s
    });
    let out_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut stdout)
            .take(RERANK_INPUT_BUDGET as u64 + 1)
            .read_to_end(&mut buf);
        // Keep the pipe moving once the budget is full: a blocked writer would
        // hold rg (and this call) until the wall-clock budget killed it.
        let _ = std::io::copy(&mut stdout, &mut std::io::sink());
        buf
    });
    let outcome = wait_bounded(&mut child, ctx, SGREP_BUDGET);
    let mut bytes = out_thread.join().unwrap_or_default();
    let stderr = err_thread.join().unwrap_or_default();
    let mut note = None;
    match outcome {
        Exit::Cancelled => return Err("search cancelled".into()),
        Exit::Timeout => return Err("ripgrep timed out - narrow the pattern or path".into()),
        Exit::Code(0) => {}
        Exit::Code(1) => return Ok(None),
        // Exit 2 means some entry errored; `--no-messages` already silences
        // per-file read errors. With matches in hand that is a footnote, and
        // with none it is an error only when rg named a real problem.
        Exit::Code(_) if bytes.is_empty() => {
            return match first_line(&stderr) {
                Some(s) => Err(format!("ripgrep: {s}")),
                None => Ok(None),
            };
        }
        Exit::Code(_) => {
            note = Some(
                first_line(&stderr)
                    .map(|s| format!("ripgrep: {s}"))
                    .unwrap_or_else(|| {
                        "some entries could not be read (ripgrep exit code 2)".into()
                    }),
            );
        }
    }
    if bytes.len() > RERANK_INPUT_BUDGET {
        // Cut on an event boundary so sgrep still parses it; rank what fits.
        bytes.truncate(RERANK_INPUT_BUDGET);
        if let Some(nl) = bytes.iter().rposition(|b| *b == b'\n') {
            bytes.truncate(nl + 1);
        }
        note = Some(format!(
            "pattern matched more than the {} MiB rerank budget - ranked what fit; narrow the pattern or path",
            RERANK_INPUT_BUDGET / (1024 * 1024)
        ));
    }
    Ok(Some(RgFeed { bytes, note }))
}

enum Exit {
    Code(i32),
    Cancelled,
    Timeout,
}

struct SgrepRun {
    exit: Exit,
    stdout: String,
    stderr: String,
    note: Option<String>,
}

/// Spawn sgrep, feed `stdin` if given, and collect its output while honoring
/// the cancel token and the wall-clock budget. Hidden window on Windows:
/// this runs inside the TUI.
fn run_sgrep(
    mut cmd: Command,
    stdin: Option<&[u8]>,
    ctx: &ToolContext,
) -> std::result::Result<SgrepRun, String> {
    cmd.stdin(if stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let writer = stdin.map(|bytes| {
        let mut pipe = child.stdin.take();
        let bytes = bytes.to_vec();
        std::thread::spawn(move || {
            if let Some(mut p) = pipe.take() {
                let _ = p.write_all(&bytes);
            }
        })
    });
    let mut stdout = child.stdout.take().ok_or("sgrep stdout")?;
    let mut stderr = child.stderr.take().ok_or("sgrep stderr")?;
    let out_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let err_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = (&mut stderr).take(64 * 1024).read_to_string(&mut s);
        let _ = std::io::copy(&mut stderr, &mut std::io::sink());
        s
    });
    let exit = wait_bounded(&mut child, ctx, SGREP_BUDGET);
    if let Some(w) = writer {
        let _ = w.join();
    }
    let stdout = out_thread.join().unwrap_or_default();
    let stderr = err_thread.join().unwrap_or_default();
    let note = stderr
        .lines()
        .map(str::trim)
        .find(|l| l.to_ascii_lowercase().contains("download"))
        .map(|_| "sgrep downloaded its model on this first run".to_string());
    Ok(SgrepRun {
        exit,
        stdout,
        stderr,
        note,
    })
}

/// Poll the child until it exits, the turn is cancelled or `budget` elapses;
/// the child is killed on either early stop.
fn wait_bounded(child: &mut Child, ctx: &ToolContext, budget: Duration) -> Exit {
    let deadline = Instant::now() + budget;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Exit::Code(status.code().unwrap_or(2)),
            Ok(None) => {}
            Err(_) => return Exit::Code(2),
        }
        if ctx.cancel.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Exit::Cancelled;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Exit::Timeout;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}

fn first_line(s: &str) -> Option<&str> {
    s.lines()
        .map(|l| l.trim().trim_start_matches("sgrep:").trim())
        .find(|l| !l.is_empty())
}

/// Without sgrep: run `grep` with the regex, or with the query's words as
/// alternatives, and lead the result with why.
fn lexical_fallback(
    args: &Value,
    ctx: &ToolContext,
    query: &str,
    pattern: Option<&str>,
    note: String,
) -> Result<String> {
    let mut grep_args = serde_json::Map::new();
    if let Some(p) = args.get("path") {
        grep_args.insert("path".into(), p.clone());
    }
    if let Some(g) = args.get("glob") {
        grep_args.insert("glob".into(), g.clone());
    }
    let count = arg_u64(args, "count")
        .unwrap_or(DEFAULT_COUNT)
        .clamp(1, MAX_COUNT);
    match pattern {
        Some(p) => {
            grep_args.insert("pattern".into(), Value::String(p.to_string()));
            grep_args.insert("context".into(), Value::from(RERANK_CONTEXT));
            grep_args.insert("max_matches".into(), Value::from(count * 4));
        }
        None => {
            let Some(p) = query_regex(query) else {
                return Ok(format!(
                    "no matches\n[{note}]\n[query has no searchable words]"
                ));
            };
            grep_args.insert("pattern".into(), Value::String(p));
            grep_args.insert("case_insensitive".into(), Value::Bool(true));
            grep_args.insert("max_matches".into(), Value::from(count * 6));
        }
    }
    let text = super::grep::grep_lexical(&Value::Object(grep_args), ctx)?;
    Ok(format!("[{note}]\n{text}"))
}

/// Words of the query worth grepping for (identifiers and nouns, not glue),
/// as an alternation; `None` when nothing is left.
pub(crate) fn query_regex(query: &str) -> Option<String> {
    const STOP: &[&str] = &[
        "a", "an", "and", "are", "as", "at", "be", "been", "but", "by", "can", "code", "do",
        "does", "find", "for", "from", "function", "had", "has", "have", "how", "i", "if", "in",
        "into", "is", "it", "its", "me", "not", "of", "on", "or", "our", "should", "show", "so",
        "that", "the", "their", "them", "there", "these", "they", "this", "to", "use", "using",
        "was", "we", "what", "when", "where", "which", "who", "will", "with", "would", "you",
        "your",
    ];
    let mut words: Vec<String> = query
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|w| w.len() >= 3)
        .filter(|w| !STOP.contains(&w.to_ascii_lowercase().as_str()))
        .map(regex::escape)
        .collect();
    words.sort();
    words.dedup();
    if words.is_empty() {
        return None;
    }
    Some(words.join("|"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_chunks_render_with_prefix_and_forward_slashes() {
        let out = r#"[{"path":"src\\api\\client.rs","start":10,"end":14,"content":"fn retry() {\n    loop {}\n}\n"},{"path":"./lib/x.py","start":1,"end":1,"content":"x = 1"}]"#;
        let chunks = parse_chunks(out).unwrap();
        assert_eq!(chunks.len(), 2);
        let text = render(&chunks, "crate/");
        assert_eq!(
            text,
            "crate/src/api/client.rs:10-14\nfn retry() {\n    loop {}\n}\n\ncrate/lib/x.py:1-1\nx = 1"
        );
    }

    #[test]
    fn empty_or_blank_output_is_no_chunks_and_garbage_is_an_error() {
        assert!(parse_chunks("").unwrap().is_empty());
        assert!(parse_chunks("  \n").unwrap().is_empty());
        assert!(parse_chunks("sgrep: boom").is_err());
    }

    #[test]
    fn oversized_chunk_is_cut_on_a_line_with_a_marker() {
        let body = "line\n".repeat(MAX_CHUNK_BYTES / 5 + 50);
        let cut = clip_chunk(&body);
        assert!(cut.len() < body.len());
        assert!(cut.ends_with("more bytes - read_file for the rest]"));
        assert!(
            cut.contains("line\n[..."),
            "cut must land after a full line"
        );
    }

    #[test]
    fn query_regex_keeps_identifiers_and_drops_glue() {
        assert_eq!(
            query_regex("where is the retry_with_backoff for the HTTP client").unwrap(),
            "HTTP|client|retry_with_backoff"
        );
        assert_eq!(query_regex("what does it do").as_deref(), None);
        assert_eq!(query_regex("foo.bar(baz)").unwrap(), "bar|baz|foo");
    }

    #[test]
    fn missing_binary_falls_back_to_grep_with_a_note() {
        let dir = std::env::temp_dir().join(format!("nur-sgrep-fb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("a.rs"),
            "fn retry_with_backoff() {}\nfn other() {}\n",
        )
        .unwrap();
        let ctx = ToolContext::default_for_test(&dir);
        let out = execute_with(
            None,
            &serde_json::json!({"query": "retry with backoff logic"}),
            &ctx,
        )
        .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(out.starts_with("[sgrep not installed"), "{out}");
        assert!(out.contains("a.rs:1:"), "{out}");
        assert!(!out.contains("other()"), "{out}");
    }
}

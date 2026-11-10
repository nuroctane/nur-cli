//! `ast_grep` tool: structural code search and rewrite, backed by the
//! ast-grep binary (<https://ast-grep.github.io>, MIT, tree-sitter).
//!
//! A pattern is ordinary code with `$VAR` for one AST node and `$$$` for zero
//! or more, so `console.log($A)` matches every one-argument call across
//! formatting variants and never a string or comment that merely spells it.
//!
//! Actions:
//! - `search`: find a shape (`pattern`, or `kind` alone).
//! - `scan`: run YAML rules (`rule` file, `inline_rule` text, or the
//!   project's `sgconfig.yml`).
//! - `rewrite`: preview a replacement; never writes.
//! - `apply`: apply a `pattern` + `rewrite`, or a rule's `fix`. The edit is
//!   computed from ast-grep's JSON preview and written through nur's own
//!   commit path (proposal mode, file ledger, `/undo`, post-edit LSP), so
//!   ast-grep's `--update-all` never runs. That also sidesteps ast-grep's
//!   one trap: `--json` with `--update-all` writes nothing.
//! - `explain`: show how ast-grep parsed a pattern (node kinds), for writing
//!   `kind`/rules or finding why a pattern matched nothing.

use super::search_util::display_path;
use super::{arg_u64, resolve_path, Tool, ToolContext};
use crate::ast_grep::{ast_grep_bin, INSTALL_HINT};
use crate::error::{NurError, Result};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub struct AstGrep;

const DEFAULT_COUNT: u64 = 20;
const MAX_COUNT: u64 = 100;
const MAX_CONTEXT: u64 = 5;
/// Worker threads per run.
const THREADS: usize = 4;
/// ast-grep is parallel tree-sitter; a minute covers very large trees.
const BUDGET: Duration = Duration::from_secs(60);
/// Most JSON read from one run (a pattern this broad needs narrowing anyway).
const STDOUT_CAP: u64 = 32 * 1024 * 1024;
/// One match's text past this is cut; the model can read_file.
const MAX_MATCH_BYTES: usize = 1500;
/// `apply` refuses past these, so one call cannot rewrite a whole tree.
const APPLY_MAX_MATCHES: usize = 200;
const APPLY_MAX_FILES: usize = 50;

/// The action a call names, aliases folded in; `None` for an unknown one.
fn action_of(name: Option<&str>) -> Option<Action> {
    Some(
        match name
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("search")
        {
            "search" | "find" | "run" => Action::Search,
            "scan" | "lint" => Action::Scan,
            "rewrite" | "preview" => Action::Rewrite,
            "apply" => Action::Apply,
            "explain" | "debug" | "ast" => Action::Explain,
            _ => return None,
        },
    )
}

/// Everything but `apply` reads. An unknown action is not read-only: it
/// fails in the tool, and failing closed costs nothing.
pub fn is_read_only_action(args_json: &str) -> bool {
    let v = serde_json::from_str::<Value>(args_json).unwrap_or(Value::Null);
    matches!(
        action_of(v.get("action").and_then(Value::as_str)),
        Some(Action::Search | Action::Scan | Action::Rewrite | Action::Explain)
    )
}

/// Whether a call writes (for the edit-tool secret screen and the
/// protected-directory checks).
pub fn writes(args: &Value) -> bool {
    action_of(args.get("action").and_then(Value::as_str)) == Some(Action::Apply)
}

impl Tool for AstGrep {
    fn name(&self) -> &str {
        "ast_grep"
    }

    fn description(&self) -> &str {
        "Structural code search and rewrite (ast-grep, tree-sitter, local). Use when the target \
         is a code shape, not text or intent: every call shaped like X, every function with Y \
         parameters, every console.log to replace. Write the shape as real code: $VAR matches one \
         node, $$$ zero or more (`fetch($URL, $$$)`, `fn $F($$$) -> Result<$T> { $$$ }`); a \
         pattern must parse as one complete node, so a fragment (an object key, a method inside \
         a class) goes in inline_rule as `pattern: {context: <full code>, selector: <kind>}`. \
         $$$ARGS captures the separators too: reuse it whole in the rewrite. \
         action=search finds it (path:line:col, the lines, the $VAR bindings); scan runs YAML lint \
         rules (rule file, inline_rule, or the project's sgconfig.yml); rewrite previews a \
         replacement and writes nothing; apply writes the same change across files (undoable, \
         one /undo per file); explain shows how a pattern parsed, its node kinds, when it \
         matched nothing. Prefer it over grep when formatting, strings or comments would fool a \
         regex, and over sgrep when you know the shape, not just what the code does."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["search", "scan", "rewrite", "apply", "explain"],
                    "default": "search",
                    "description": "search: find a code shape. scan: run YAML rules. rewrite: preview a replacement, no writes. apply: write a pattern+rewrite or a rule's fix. explain: show the parsed pattern."
                },
                "pattern": {"type": "string", "description": "AST pattern as real code; $VAR for one node, $$$ for zero or more, $$$ARGS to name them. Required for rewrite and explain, and for search/apply unless kind or a rule is given."},
                "rewrite": {"type": "string", "description": "Replacement using the pattern's metavariables, e.g. logger.info($A). For rewrite and apply."},
                "lang": {"type": "string", "description": "Pattern language: rust, ts, tsx, js, python, go, java, c, cpp, ... Omitted: inferred per file from its extension. Required for explain."},
                "kind": {"type": "string", "description": "Search: tree-sitter node kind to match, e.g. function_item (use explain to see kinds)."},
                "path": {"type": "string", "description": "Directory or file (default cwd). .gitignore is honoured."},
                "glob": {"type": "string", "description": "Only matching paths; prefix ! to exclude, e.g. src/**/*.ts or !**/tests/**"},
                "rule": {"type": "string", "description": "Scan/apply: path to a YAML rule file."},
                "inline_rule": {"type": "string", "description": "Scan/apply: YAML rule text (id, language, rule, optional message, severity, fix)."},
                "context": {"type": "integer", "default": 0, "description": format!("Search: lines of context around each match (max {MAX_CONTEXT}).")},
                "count": {"type": "integer", "default": DEFAULT_COUNT, "description": format!("How many matches to show (max {MAX_COUNT}).")}
            }
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        execute_with(ast_grep_bin(), args, ctx, commit_real)
    }
}

/// The tool body with the binary already resolved, so the missing-binary
/// path is testable without touching the environment.
fn execute_with(
    bin: Option<String>,
    args: &Value,
    ctx: &ToolContext,
    commit: Commit,
) -> Result<String> {
    let req = Request::parse(args, ctx)?;
    let Some(bin) = bin else {
        return Ok(format!(
            "[ast-grep is not installed. Install: {INSTALL_HINT}]\n\
             Meanwhile: grep pattern=<regex> for exact text, sgrep query=\"<what the code does>\" \
             for intent. Neither matches syntax, so check the hits by reading them."
        ));
    };
    match req.action {
        Action::Search => search(&bin, &req, ctx),
        Action::Scan => scan(&bin, &req, ctx),
        Action::Rewrite => rewrite_preview(&bin, &req, ctx),
        Action::Apply => apply(&bin, &req, ctx, commit),
        Action::Explain => explain(&bin, &req, ctx),
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Action {
    Search,
    Scan,
    Rewrite,
    Apply,
    Explain,
}

struct Request {
    action: Action,
    pattern: Option<String>,
    rewrite: Option<String>,
    lang: Option<String>,
    kind: Option<String>,
    globs: Vec<String>,
    rule: Option<PathBuf>,
    inline_rule: Option<String>,
    context: u64,
    count: usize,
    /// Process cwd for ast-grep and the path argument inside it.
    dir: PathBuf,
    target: PathBuf,
    /// Prefix that puts ast-grep's root-relative paths back relative to cwd.
    prefix: String,
    /// The `path` argument as given (protected-directory checks).
    path_arg: String,
}

impl Request {
    fn parse(args: &Value, ctx: &ToolContext) -> Result<Self> {
        let opt = |name: &str| {
            args.get(name)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        let action = match action_of(opt("action").as_deref()) {
            Some(a) => a,
            None => {
                return Err(NurError::Tool(format!(
                    "ast_grep: unknown action '{}' - use search, scan, rewrite, apply or explain",
                    opt("action").unwrap_or_default()
                )))
            }
        };
        let globs = match args.get("glob") {
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect(),
            _ => opt("glob").into_iter().collect(),
        };
        let root = resolve_path(
            &ctx.cwd,
            args.get("path").and_then(Value::as_str).unwrap_or("."),
        )?;
        let rule = match opt("rule") {
            Some(r) => Some(resolve_path(&ctx.cwd, &r)?),
            None => None,
        };
        let (dir, target) = if root.is_file() {
            let dir = root.parent().unwrap_or(&root).to_path_buf();
            let name = root.file_name().map(PathBuf::from).unwrap_or_default();
            (dir, name)
        } else {
            (root.clone(), PathBuf::from("."))
        };
        let shown = display_path(&ctx.cwd, &dir);
        let prefix = if shown.is_empty() || shown == "." {
            String::new()
        } else {
            format!("{}/", shown.trim_end_matches('/'))
        };
        let req = Self {
            action,
            pattern: args
                .get("pattern")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string),
            rewrite: args
                .get("rewrite")
                .and_then(Value::as_str)
                .map(str::to_string),
            lang: opt("lang"),
            kind: opt("kind"),
            globs,
            rule,
            inline_rule: args
                .get("inline_rule")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string),
            context: arg_u64(args, "context").unwrap_or(0).min(MAX_CONTEXT),
            count: arg_u64(args, "count")
                .unwrap_or(DEFAULT_COUNT)
                .clamp(1, MAX_COUNT) as usize,
            dir,
            target,
            prefix,
            path_arg: args
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or(".")
                .to_string(),
        };
        req.validate()?;
        Ok(req)
    }

    fn has_rule(&self) -> bool {
        self.rule.is_some() || self.inline_rule.is_some()
    }

    fn validate(&self) -> Result<()> {
        let need = |what: &str| {
            Err(NurError::Tool(format!(
                "ast_grep {}: {what}",
                self.action_name()
            )))
        };
        match self.action {
            Action::Search if self.pattern.is_none() && self.kind.is_none() => {
                need("needs pattern (or kind)")
            }
            Action::Rewrite if self.pattern.is_none() || self.rewrite.is_none() => {
                need("needs pattern and rewrite")
            }
            Action::Apply if self.has_rule() && self.pattern.is_some() => {
                need("takes pattern+rewrite or a rule, not both")
            }
            Action::Apply if self.pattern.is_some() && self.rewrite.is_none() => {
                need("needs rewrite with a pattern (or a rule, or neither for the project's sgconfig.yml rules)")
            }
            Action::Explain if self.pattern.is_none() || self.lang.is_none() => {
                need("needs pattern and lang")
            }
            Action::Scan if self.rule.is_some() && self.inline_rule.is_some() => {
                need("takes rule or inline_rule, not both")
            }
            _ => Ok(()),
        }
    }

    fn action_name(&self) -> &'static str {
        match self.action {
            Action::Search => "search",
            Action::Scan => "scan",
            Action::Rewrite => "rewrite",
            Action::Apply => "apply",
            Action::Explain => "explain",
        }
    }

    /// `ast-grep run` with the pattern, language and filters; `rewrite` adds
    /// `-r`. Never `--update-all`.
    fn run_args(&self, with_rewrite: bool) -> Vec<String> {
        let mut a = vec!["run".to_string()];
        if let Some(p) = &self.pattern {
            a.extend(["--pattern".into(), p.clone()]);
        }
        if let Some(k) = &self.kind {
            a.extend(["--kind".into(), k.clone()]);
        }
        if with_rewrite {
            if let Some(r) = &self.rewrite {
                a.extend(["--rewrite".into(), r.clone()]);
            }
        }
        if let Some(l) = &self.lang {
            a.extend(["--lang".into(), l.clone()]);
        }
        if self.context > 0 && self.action == Action::Search {
            a.extend(["--context".into(), self.context.to_string()]);
        }
        self.common(&mut a);
        a
    }

    fn scan_args(&self) -> Vec<String> {
        let mut a = vec!["scan".to_string()];
        if let Some(r) = &self.rule {
            a.extend(["--rule".into(), r.to_string_lossy().into_owned()]);
        }
        if let Some(r) = &self.inline_rule {
            a.extend(["--inline-rules".into(), r.clone()]);
        }
        self.common(&mut a);
        a
    }

    fn common(&self, a: &mut Vec<String>) {
        for g in &self.globs {
            a.extend(["--globs".into(), g.clone()]);
        }
        // Native custom-language libraries run code; never load one a cloned
        // repository's sgconfig.yml names.
        a.extend([
            "--custom-languages".into(),
            "ignore".into(),
            "--color".into(),
            "never".into(),
            // Bounded: several searches may run at once (codemode fans them
            // out), and the owner's machine stays usable.
            "--threads".into(),
            THREADS.to_string(),
            "--json=stream".into(),
            self.target.to_string_lossy().into_owned(),
        ]);
    }
}

// ── one bounded run ──────────────────────────────────────────────────────

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(
    bin: &str,
    dir: &Path,
    args: &[String],
    stdin: Option<&str>,
    ctx: &ToolContext,
) -> Result<Run> {
    let mut cmd = Command::new(bin);
    cmd.current_dir(dir)
        .args(args)
        .stdin(if stdin.is_some() {
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
    let mut child = cmd
        .spawn()
        .map_err(|e| NurError::Tool(format!("ast-grep could not start ({e})")))?;
    if let Some(text) = stdin {
        if let Some(mut pipe) = child.stdin.take() {
            let _ = pipe.write_all(text.as_bytes());
        }
    }
    let mut out = child
        .stdout
        .take()
        .ok_or_else(|| NurError::Tool("ast-grep stdout".into()))?;
    let mut err = child
        .stderr
        .take()
        .ok_or_else(|| NurError::Tool("ast-grep stderr".into()))?;
    let out_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut out).take(STDOUT_CAP).read_to_end(&mut buf);
        let _ = std::io::copy(&mut out, &mut std::io::sink());
        String::from_utf8_lossy(&buf).into_owned()
    });
    let err_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = (&mut err).take(64 * 1024).read_to_string(&mut s);
        let _ = std::io::copy(&mut err, &mut std::io::sink());
        s
    });
    let deadline = Instant::now() + BUDGET;
    let code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code().unwrap_or(2),
            Ok(None) => {}
            Err(e) => return Err(NurError::Tool(format!("ast-grep wait: {e}"))),
        }
        if ctx.cancel.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(NurError::Tool("ast_grep cancelled".into()));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(NurError::Tool(format!(
                "ast-grep gave up after {}s - narrow path, glob or lang",
                BUDGET.as_secs()
            )));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    Ok(Run {
        code,
        stdout: out_thread.join().unwrap_or_default(),
        stderr: err_thread.join().unwrap_or_default(),
    })
}

/// The lines of stderr worth showing (ast-grep's `Error:`/`Help:` block,
/// without its playground links).
fn stderr_summary(stderr: &str) -> String {
    stderr
        .lines()
        .map(str::trim)
        .filter(|l| {
            !l.is_empty() && !l.starts_with("See also:") && !l.starts_with("For more information")
        })
        .take(4)
        .collect::<Vec<_>>()
        .join(" ")
}

/// The pattern did not parse as code in its language.
fn pattern_broken(stderr: &str) -> bool {
    stderr.contains("ERROR node")
}

fn broken_pattern_hint(req: &Request) -> String {
    format!(
        "[the pattern did not parse as {} code: write it as a complete expression or statement \
         (balanced brackets, `$VAR` in place of a node){}; explain shows the parse]",
        req.lang.as_deref().unwrap_or("the file's language"),
        if req.lang.is_none() {
            ", and set lang"
        } else {
            ""
        }
    )
}

/// Run and parse matches; an error when ast-grep refused the request.
fn matches_of(
    bin: &str,
    req: &Request,
    args: &[String],
    ctx: &ToolContext,
) -> Result<(Vec<Match>, Run)> {
    let run = run(bin, &req.dir, args, None, ctx)?;
    // run: 0 matches, 1 none. scan: 0, or 1 when an error-severity rule hit.
    if run.code > 1
        || (run.code != 0 && run.stdout.trim().is_empty() && !run.stderr.trim().is_empty())
    {
        let msg = stderr_summary(&run.stderr);
        let hint = if msg.contains("No ast-grep project configuration") {
            " - this project has no sgconfig.yml; pass rule=<file.yml> or inline_rule=<yaml>"
        } else if msg.contains("--lang") {
            " - lang takes a tree-sitter language name: rust, ts, tsx, js, python, go, java, c, cpp, ..."
        } else {
            ""
        };
        return Err(NurError::Tool(format!(
            "ast-grep: {}{hint}",
            if msg.is_empty() {
                format!("exit {}", run.code)
            } else {
                msg
            }
        )));
    }
    let matches = parse_matches(&run.stdout)
        .map_err(|e| NurError::Tool(format!("ast-grep: unreadable output ({e})")))?;
    if run.stdout.len() as u64 >= STDOUT_CAP {
        return Err(NurError::Tool(format!(
            "ast_grep: the pattern matched more than {} MB of results - narrow it with path, \
             glob or lang",
            STDOUT_CAP / (1024 * 1024)
        )));
    }
    Ok((matches, run))
}

// ── JSON shape (recorded from ast-grep 0.51) ─────────────────────────────

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct Match {
    pub file: String,
    pub text: String,
    pub range: Range,
    #[serde(default)]
    pub lines: String,
    #[serde(default, rename = "charCount")]
    pub char_count: Option<CharCount>,
    #[serde(default)]
    pub replacement: Option<String>,
    #[serde(default, rename = "replacementOffsets")]
    pub replacement_offsets: Option<Offsets>,
    #[serde(default, rename = "metaVariables")]
    pub meta: Option<MetaVariables>,
    #[serde(default, rename = "ruleId")]
    pub rule_id: Option<String>,
    #[serde(default)]
    pub severity: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct Range {
    #[serde(rename = "byteOffset")]
    pub byte_offset: Offsets,
    pub start: Pos,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq)]
pub(crate) struct Offsets {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Deserialize, Clone, Copy)]
pub(crate) struct Pos {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Deserialize, Clone, Copy)]
pub(crate) struct CharCount {
    pub leading: usize,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub(crate) struct MetaVariables {
    #[serde(default)]
    pub single: BTreeMap<String, Bound>,
    #[serde(default)]
    pub multi: BTreeMap<String, Vec<Bound>>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct Bound {
    pub text: String,
}

/// ast-grep's matches: a JSON array (`--json=compact`), or one object per
/// line (`--json=stream`, what the tool runs). In a stream cut at the read
/// cap the last, partial line is dropped and the complete ones stay.
pub(crate) fn parse_matches(stdout: &str) -> std::result::Result<Vec<Match>, serde_json::Error> {
    let s = stdout.trim();
    if s.is_empty() {
        return Ok(Vec::new());
    }
    if s.starts_with('[') {
        return serde_json::from_str(s);
    }
    let lines: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
    let mut out = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        match serde_json::from_str::<Match>(line) {
            Ok(m) => out.push(m),
            Err(_) if i + 1 == lines.len() && !out.is_empty() => break,
            Err(e) => return Err(e),
        }
    }
    Ok(out)
}

// ── rendering ────────────────────────────────────────────────────────────

fn shown_path(prefix: &str, file: &str) -> String {
    let p = file.replace('\\', "/");
    let p = p.strip_prefix("./").unwrap_or(&p).to_string();
    format!("{prefix}{p}")
}

fn clip(text: &str) -> String {
    if text.len() <= MAX_MATCH_BYTES {
        return text.to_string();
    }
    let mut cut = MAX_MATCH_BYTES;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    let cut = text[..cut].rfind('\n').unwrap_or(cut);
    format!(
        "{}\n[... {} more bytes - read_file for the rest]",
        &text[..cut],
        text.len() - cut
    )
}

/// `$A = x · $$$ARGS = a, b`: what each metavariable caught, short.
fn bindings(meta: &Option<MetaVariables>) -> Option<String> {
    let meta = meta.as_ref()?;
    let short = |s: &str| {
        let one = s.split_whitespace().collect::<Vec<_>>().join(" ");
        if one.chars().count() > 60 {
            format!("{}…", one.chars().take(59).collect::<String>())
        } else {
            one
        }
    };
    let mut parts: Vec<String> = meta
        .single
        .iter()
        .filter(|(name, _)| !name.starts_with('_'))
        .map(|(name, b)| format!("${name} = {}", short(&b.text)))
        .collect();
    for (name, list) in &meta.multi {
        if name.starts_with('_') {
            continue;
        }
        // ast-grep lists separators (`,`) as nodes too; keep the real ones.
        let items: Vec<String> = list
            .iter()
            .map(|b| b.text.trim())
            .filter(|t| !t.is_empty() && !t.chars().all(|c| matches!(c, ',' | ';')))
            .map(short)
            .collect();
        parts.push(format!(
            "$$${name} = {}",
            if items.is_empty() {
                "(none)".into()
            } else {
                items.join(", ")
            }
        ));
    }
    if parts.is_empty() || parts.len() > 8 {
        return None;
    }
    Some(parts.join(" · "))
}

/// The match's full lines, each numbered (1-based) so an edit can target them.
fn numbered_lines(m: &Match) -> String {
    let body = if m.lines.is_empty() {
        &m.text
    } else {
        &m.lines
    };
    // `charCount.leading` is the characters of context before the match in
    // `lines`; the lines it spans come before the match's own first line.
    let before = m
        .char_count
        .map(|c| {
            body.chars()
                .take(c.leading)
                .filter(|ch| *ch == '\n')
                .count()
        })
        .unwrap_or(0);
    let first = (m.range.start.line + 1).saturating_sub(before);
    clip(body)
        .lines()
        .enumerate()
        .map(|(i, l)| {
            if l.starts_with("[... ") {
                l.to_string()
            } else {
                format!("{:>5}| {}", first + i, l.trim_end_matches('\r'))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn files_in(matches: &[Match]) -> usize {
    let mut f: Vec<&str> = matches.iter().map(|m| m.file.as_str()).collect();
    f.sort_unstable();
    f.dedup();
    f.len()
}

fn plural(n: usize, one: &str) -> String {
    let many = match one {
        "match" => "matches",
        "checkpoint" => "checkpoints",
        _ => "",
    };
    match (n, many) {
        (1, _) => format!("1 {one}"),
        (_, "") => format!("{n} {one}s"),
        _ => format!("{n} {many}"),
    }
}

fn more_note(total: usize, shown: usize) -> Option<String> {
    (total > shown).then(|| {
        format!(
            "[{} more not shown - narrow with path, glob or lang, or raise count (max {MAX_COUNT})]",
            total - shown
        )
    })
}

pub(crate) fn render_search(matches: &[Match], prefix: &str, count: usize) -> String {
    let mut out = format!(
        "{} in {}\n",
        plural(matches.len(), "match"),
        plural(files_in(matches), "file")
    );
    for m in matches.iter().take(count) {
        out.push('\n');
        out.push_str(&format!(
            "{}:{}:{}\n{}\n",
            shown_path(prefix, &m.file),
            m.range.start.line + 1,
            m.range.start.column + 1,
            numbered_lines(m)
        ));
        if let Some(b) = bindings(&m.meta) {
            out.push_str(&format!("  {b}\n"));
        }
    }
    if let Some(n) = more_note(matches.len(), count) {
        out.push('\n');
        out.push_str(&n);
    }
    out.trim_end().to_string()
}

fn diff_lines(old: &str, new: &str) -> String {
    let mut s = String::new();
    for l in clip(old).lines() {
        s.push_str(&format!("  - {}\n", l.trim_end_matches('\r')));
    }
    for l in clip(new).lines() {
        s.push_str(&format!("  + {}\n", l.trim_end_matches('\r')));
    }
    if new.is_empty() {
        s.push_str("  + (removed)\n");
    }
    s
}

pub(crate) fn render_rewrite(matches: &[Match], prefix: &str, count: usize) -> String {
    let mut out = format!(
        "rewrite preview: {} in {} - nothing written\n",
        plural(matches.len(), "replacement"),
        plural(files_in(matches), "file")
    );
    for m in matches.iter().take(count) {
        out.push('\n');
        out.push_str(&format!(
            "{}:{}:{}\n",
            shown_path(prefix, &m.file),
            m.range.start.line + 1,
            m.range.start.column + 1
        ));
        out.push_str(&diff_lines(&m.text, m.replacement.as_deref().unwrap_or("")));
    }
    if let Some(n) = more_note(matches.len(), count) {
        out.push('\n');
        out.push_str(&n);
        out.push('\n');
    }
    out.push_str("\nApply with action=apply and the same pattern, rewrite, lang, path and glob.");
    out
}

pub(crate) fn render_scan(matches: &[Match], prefix: &str, count: usize) -> String {
    let rank = |s: &Option<String>| match s.as_deref() {
        Some("error") => 0,
        Some("warning") => 1,
        Some("info") => 2,
        Some("hint") => 3,
        _ => 4,
    };
    let mut sorted: Vec<&Match> = matches.iter().collect();
    sorted.sort_by_key(|m| (rank(&m.severity), m.file.clone(), m.range.byte_offset.start));
    let mut per_rule: BTreeMap<&str, usize> = BTreeMap::new();
    for m in matches {
        *per_rule
            .entry(m.rule_id.as_deref().unwrap_or("?"))
            .or_default() += 1;
    }
    let tally = per_rule
        .iter()
        .map(|(r, n)| format!("{r} {n}"))
        .collect::<Vec<_>>()
        .join(", ");
    let fixable = matches.iter().filter(|m| m.replacement.is_some()).count();
    let mut out = format!(
        "{} in {} ({tally})\n",
        plural(matches.len(), "finding"),
        plural(files_in(matches), "file")
    );
    for m in sorted.into_iter().take(count) {
        out.push('\n');
        out.push_str(&format!(
            "{}:{}:{}  {}  {}{}\n{}\n",
            shown_path(prefix, &m.file),
            m.range.start.line + 1,
            m.range.start.column + 1,
            m.severity.as_deref().unwrap_or("hint"),
            m.rule_id.as_deref().unwrap_or("?"),
            m.message
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(|s| format!("  {s}"))
                .unwrap_or_default(),
            numbered_lines(m)
        ));
        if let Some(note) = m.note.as_deref().filter(|s| !s.is_empty()) {
            out.push_str(&format!("  note: {}\n", note.trim()));
        }
        if let Some(fix) = &m.replacement {
            out.push_str(&format!(
                "  fix: {}\n",
                clip(fix).replace('\n', "\n       ")
            ));
        }
    }
    if let Some(n) = more_note(matches.len(), count) {
        out.push('\n');
        out.push_str(&n);
        out.push('\n');
    }
    if fixable > 0 {
        out.push_str(&format!(
            "\n{} carry a fix: action=apply with the same rule applies them.",
            plural(fixable, "finding")
        ));
    }
    out.trim_end().to_string()
}

// ── actions ──────────────────────────────────────────────────────────────

fn search(bin: &str, req: &Request, ctx: &ToolContext) -> Result<String> {
    let (matches, run) = matches_of(bin, req, &req.run_args(false), ctx)?;
    if matches.is_empty() {
        let mut out = String::from("no matches");
        if pattern_broken(&run.stderr) {
            out.push('\n');
            out.push_str(&broken_pattern_hint(req));
        } else if req.lang.is_none() {
            out.push_str(
                "\n[lang was inferred per file; set lang if the pattern is for one language]",
            );
        }
        return Ok(out);
    }
    Ok(render_search(&matches, &req.prefix, req.count))
}

fn scan(bin: &str, req: &Request, ctx: &ToolContext) -> Result<String> {
    let (matches, _) = matches_of(bin, req, &req.scan_args(), ctx)?;
    if matches.is_empty() {
        return Ok("no findings".into());
    }
    Ok(render_scan(&matches, &req.prefix, req.count))
}

fn rewrite_preview(bin: &str, req: &Request, ctx: &ToolContext) -> Result<String> {
    let (matches, run) = matches_of(bin, req, &req.run_args(true), ctx)?;
    if matches.is_empty() {
        let mut out = String::from("no matches - nothing to rewrite");
        if pattern_broken(&run.stderr) {
            out.push('\n');
            out.push_str(&broken_pattern_hint(req));
        }
        return Ok(out);
    }
    Ok(render_rewrite(&matches, &req.prefix, req.count))
}

fn explain(bin: &str, req: &Request, ctx: &ToolContext) -> Result<String> {
    let mut args = vec![
        "run".to_string(),
        "--pattern".into(),
        req.pattern.clone().unwrap_or_default(),
        "--lang".into(),
        req.lang.clone().unwrap_or_default(),
        "--debug-query=ast".into(),
        "--color".into(),
        "never".into(),
        "--stdin".into(),
    ];
    if let Some(k) = &req.kind {
        args.extend(["--kind".into(), k.clone()]);
    }
    let run = run(bin, &req.dir, &args, Some(""), ctx)?;
    let tree: Vec<&str> = run
        .stderr
        .lines()
        .skip_while(|l| !l.starts_with("Debug AST"))
        .skip(1)
        .take_while(|l| !l.trim().is_empty())
        .collect();
    if tree.is_empty() {
        let msg = stderr_summary(&run.stderr);
        return Err(NurError::Tool(format!(
            "ast-grep explain: {}",
            if msg.is_empty() {
                format!("exit {}", run.code)
            } else {
                msg
            }
        )));
    }
    let mut out = format!(
        "pattern parsed as {} (named nodes; `metavariable` is a $VAR):\n{}",
        req.lang.as_deref().unwrap_or("?"),
        tree.join("\n")
    );
    if pattern_broken(&run.stderr) || tree.iter().any(|l| l.trim_start().starts_with("ERROR")) {
        out.push('\n');
        out.push_str(&broken_pattern_hint(req));
    }
    out.push_str("\n[kind=<node kind> matches every node of that kind; rules can combine them]");
    Ok(out)
}

/// One planned replacement inside one file. `start..end` is what gets
/// replaced (`replacementOffsets`, wider than the match when a rule's fix
/// uses `expandStart`/`expandEnd`); `check_start..check_end` is the match
/// itself, which must still read `old` or the file changed since the preview.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Edit {
    pub start: usize,
    pub end: usize,
    pub check_start: usize,
    pub check_end: usize,
    pub old: String,
    pub new: String,
}

#[cfg(test)]
impl Edit {
    /// A replacement exactly over its match (patterns and plain fixes).
    pub fn exact(start: usize, end: usize, old: &str, new: &str) -> Self {
        Self {
            start,
            end,
            check_start: start,
            check_end: end,
            old: old.into(),
            new: new.into(),
        }
    }
}

/// What a splice did.
#[derive(Debug, PartialEq)]
pub(crate) struct Spliced {
    pub content: String,
    pub applied: usize,
    /// Matches inside another match's replacement: a second apply reaches
    /// them once the outer one is rewritten.
    pub nested: usize,
}

/// Apply `edits` to `content`. Every edit's match must still hold the text
/// ast-grep saw; nested or overlapping edits keep the outer, earlier one and
/// are counted. A multi-line replacement takes the file's CRLF line endings.
pub(crate) fn splice(content: &str, edits: &[Edit]) -> std::result::Result<Spliced, String> {
    let at_boundary = |i: usize| i <= content.len() && content.is_char_boundary(i);
    let mut sorted: Vec<&Edit> = edits.iter().collect();
    sorted.sort_by_key(|e| (e.start, std::cmp::Reverse(e.end)));
    let mut kept: Vec<&Edit> = Vec::new();
    let mut nested = 0;
    for e in sorted {
        if kept.last().is_some_and(|k| e.start < k.end) {
            nested += 1;
            continue;
        }
        let ok = e.start <= e.check_start
            && e.check_start <= e.check_end
            && e.check_end <= e.end
            && [e.start, e.end, e.check_start, e.check_end]
                .into_iter()
                .all(at_boundary)
            && content[e.check_start..e.check_end] == e.old;
        if !ok {
            return Err("changed since the preview".into());
        }
        kept.push(e);
    }
    let crlf = content.contains("\r\n");
    let mut out = String::with_capacity(content.len());
    let mut at = 0;
    for e in &kept {
        out.push_str(&content[at..e.start]);
        if crlf && e.new.contains('\n') {
            out.push_str(&e.new.replace("\r\n", "\n").replace('\n', "\r\n"));
        } else {
            out.push_str(&e.new);
        }
        at = e.end;
    }
    out.push_str(&content[at..]);
    Ok(Spliced {
        content: out,
        applied: kept.len(),
        nested,
    })
}

/// Why an `apply` leaves a file alone: git internals always (a hook or
/// `core.fsmonitor` there runs code), CI workflows, editor tasks and nur's
/// own rules unless the call's `path` names that directory (which makes the
/// call ask for approval every time, as the edit tools do).
fn protected(shown: &str, path_arg: &str) -> Option<String> {
    let parts = |p: &str| -> Vec<String> {
        p.replace('\\', "/")
            .to_ascii_lowercase()
            .split('/')
            .filter(|c| !c.is_empty() && *c != ".")
            .map(str::to_string)
            .collect()
    };
    let file = parts(shown);
    let named = parts(path_arg);
    if file.iter().any(|c| c == ".git") {
        return Some("git internals are never rewritten".into());
    }
    [".github", ".nur", ".vscode"]
        .into_iter()
        .find(|d| file.iter().any(|c| c == d) && !named.iter().any(|c| c == d))
        .map(|d| format!("{d} is rewritten only when path names it"))
}

/// How one file's new content lands.
enum Wrote {
    /// Written; the post-edit LSP note.
    Written(String),
    /// Proposal mode staged it instead.
    Staged(String),
}

type Commit = fn(&ToolContext, &Path, &str) -> Result<Wrote>;

/// nur's own commit path: an `/undo` checkpoint, proposal mode, the file
/// ledger and post-edit diagnostics, exactly as the edit tools get them.
fn commit_real(ctx: &ToolContext, full: &Path, updated: &str) -> Result<Wrote> {
    let proposal = crate::config::proposal_mode_on();
    if !proposal {
        super::undo::record(&ctx.session_id, full);
    }
    let path_arg = full.to_string_lossy().into_owned();
    match super::commit_file_write(ctx, &path_arg, full, updated, "ast_grep") {
        Ok(c) => match c.staged {
            Some(msg) => Ok(Wrote::Staged(msg)),
            None => {
                super::undo::seal(&ctx.session_id, full);
                Ok(Wrote::Written(c.lsp_note))
            }
        },
        Err(e) => {
            if !proposal {
                let _ = super::undo::drop_last(&ctx.session_id);
            }
            Err(e)
        }
    }
}

fn apply(bin: &str, req: &Request, ctx: &ToolContext, commit: Commit) -> Result<String> {
    // No pattern: a rule's fix, or the project's own sgconfig.yml rules.
    let args = if req.pattern.is_none() {
        req.scan_args()
    } else {
        req.run_args(true)
    };
    let (matches, run) = matches_of(bin, req, &args, ctx)?;
    let fixes: Vec<&Match> = matches.iter().filter(|m| m.replacement.is_some()).collect();
    if fixes.is_empty() {
        let mut out = String::from(if matches.is_empty() {
            "no matches - nothing written"
        } else {
            "the rule matched but carries no fix - nothing written"
        });
        if pattern_broken(&run.stderr) {
            out.push('\n');
            out.push_str(&broken_pattern_hint(req));
        }
        return Ok(out);
    }
    let mut by_file: BTreeMap<String, Vec<Edit>> = BTreeMap::new();
    for m in &fixes {
        let span = m.replacement_offsets.unwrap_or(m.range.byte_offset);
        by_file.entry(m.file.clone()).or_default().push(Edit {
            start: span.start,
            end: span.end,
            check_start: m.range.byte_offset.start,
            check_end: m.range.byte_offset.end,
            old: m.text.clone(),
            new: m.replacement.clone().unwrap_or_default(),
        });
    }
    if fixes.len() > APPLY_MAX_MATCHES || by_file.len() > APPLY_MAX_FILES {
        return Err(NurError::Tool(format!(
            "ast_grep apply: {} in {} is more than one call applies (max {APPLY_MAX_MATCHES} \
             replacements, {APPLY_MAX_FILES} files) - nothing written. Narrow with path or glob \
             and apply in parts; rewrite previews each part.",
            plural(fixes.len(), "replacement"),
            plural(by_file.len(), "file")
        )));
    }

    let mut done: Vec<(String, usize)> = Vec::new();
    let mut staged: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut lsp_notes = String::new();
    let mut nested = 0;
    for (file, edits) in &by_file {
        let shown = shown_path(&req.prefix, file);
        let full = match resolve_path(&req.dir, file) {
            Ok(p) => p,
            Err(e) => {
                skipped.push(format!("{shown} ({e})"));
                continue;
            }
        };
        if let Some(why) = protected(&shown, &req.path_arg) {
            skipped.push(format!("{shown} ({why})"));
            continue;
        }
        let content = match std::fs::read(&full).map(String::from_utf8) {
            Ok(Ok(s)) => s,
            Ok(Err(_)) => {
                skipped.push(format!("{shown} (not UTF-8)"));
                continue;
            }
            Err(e) => {
                skipped.push(format!("{shown} ({e})"));
                continue;
            }
        };
        let (updated, n) = match splice(&content, edits) {
            Ok(r) => {
                nested += r.nested;
                (r.content, r.applied)
            }
            Err(why) => {
                skipped.push(format!("{shown} ({why})"));
                continue;
            }
        };
        if updated == content {
            continue;
        }
        match commit(ctx, &full, &updated) {
            Ok(Wrote::Staged(msg)) => staged.push(msg),
            Ok(Wrote::Written(note)) => {
                lsp_notes.push_str(&note);
                done.push((shown, n));
            }
            Err(e) => skipped.push(format!("{shown} ({e})")),
        }
    }

    let total: usize = done.iter().map(|(_, n)| n).sum();
    let mut out = if done.is_empty() && staged.is_empty() {
        String::from("nothing written")
    } else if done.is_empty() {
        format!("{} staged (proposal mode)", plural(staged.len(), "file"))
    } else {
        format!(
            "applied {} in {}",
            plural(total, "replacement"),
            plural(done.len(), "file")
        )
    };
    out.push('\n');
    for (f, n) in &done {
        out.push_str(&format!("  {f}  {n}\n"));
    }
    for s in &staged {
        out.push_str(&format!("  {s}\n"));
    }
    if !skipped.is_empty() {
        out.push_str(&format!(
            "skipped {}: {}\n",
            plural(skipped.len(), "file"),
            skipped.join("; ")
        ));
    }
    if nested > 0 {
        out.push_str(&format!(
            "{} inside another match left for now: apply again to rewrite {}\n",
            if nested == 1 {
                "1 nested match".to_string()
            } else {
                format!("{nested} nested matches")
            },
            if nested == 1 { "it" } else { "them" }
        ));
    }
    if !done.is_empty() {
        out.push_str(&format!(
            "undo: /undo restores one file per step ({} recorded)\n",
            plural(done.len(), "checkpoint")
        ));
    }
    out.push_str(&lsp_notes);
    Ok(out.trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests write plainly: no undo checkpoint under the real home, no
    /// language server.
    fn commit_plain(_: &ToolContext, full: &Path, updated: &str) -> Result<Wrote> {
        std::fs::write(full, updated).map_err(|e| NurError::Tool(e.to_string()))?;
        Ok(Wrote::Written(String::new()))
    }

    /// Recorded from `ast-grep run -p 'console.log($A)' -r 'logger.info($A)'
    /// -l ts --json=compact` (0.51.1) over a file with one one-line and one
    /// multi-line call.
    const REWRITE_FIXTURE: &str = r#"[{"text":"console.log(\"hi\")","range":{"byteOffset":{"start":53,"end":70},"start":{"line":1,"column":0},"end":{"line":1,"column":17}},"file":"src\\a.ts","lines":"console.log(\"hi\");","charCount":{"leading":0,"trailing":1},"replacement":"logger.info(\"hi\")","replacementOffsets":{"start":53,"end":70},"language":"TypeScript","metaVariables":{"single":{"A":{"text":"\"hi\"","range":{"byteOffset":{"start":65,"end":69},"start":{"line":1,"column":12},"end":{"line":1,"column":16}}}},"multi":{},"transformed":{}}},{"text":"console.log(\n  add(1, 2),\n)","range":{"byteOffset":{"start":72,"end":99},"start":{"line":2,"column":0},"end":{"line":4,"column":1}},"file":"src\\a.ts","lines":"console.log(\n  add(1, 2),\n);","charCount":{"leading":0,"trailing":1},"replacement":"logger.info(add(1, 2))","replacementOffsets":{"start":72,"end":99},"language":"TypeScript","metaVariables":{"single":{"A":{"text":"add(1, 2)","range":{"byteOffset":{"start":87,"end":96},"start":{"line":3,"column":2},"end":{"line":3,"column":11}}}},"multi":{},"transformed":{}}}]"#;
    const SOURCE: &str = "function add(a: number, b: number) { return a + b; }\nconsole.log(\"hi\");\nconsole.log(\n  add(1, 2),\n);\nconst s = \"console.log(x)\"; // console.log(y)\n";

    #[test]
    fn recorded_json_parses_and_renders_with_numbered_lines_and_bindings() {
        let m = parse_matches(REWRITE_FIXTURE).unwrap();
        assert_eq!(m.len(), 2);
        let out = render_search(&m, "", 20);
        assert!(out.starts_with("2 matches in 1 file\n"), "{out}");
        assert!(
            out.contains("src/a.ts:2:1\n    2| console.log(\"hi\");\n  $A = \"hi\""),
            "{out}"
        );
        assert!(
            out.contains("src/a.ts:3:1\n    3| console.log(\n    4|   add(1, 2),\n    5| );"),
            "{out}"
        );
        assert!(parse_matches("").unwrap().is_empty());
        assert!(parse_matches("Error: boom").is_err());
    }

    // Failure mode: context lines before the match shifted every line number.
    #[test]
    fn context_lines_number_from_the_first_context_line() {
        let ctx = r#"[{"text":"foo(1, 2)","range":{"byteOffset":{"start":40,"end":49},"start":{"line":2,"column":4},"end":{"line":2,"column":13}},"file":"m.rs","lines":"a\nb\n    foo(1, 2);\nc","charCount":{"leading":8,"trailing":4}}]"#;
        let out = render_search(&parse_matches(ctx).unwrap(), "", 5);
        assert!(
            out.contains("    1| a\n    2| b\n    3|     foo(1, 2);\n    4| c"),
            "{out}"
        );
    }

    // Failure mode: `$$$ARGS` listed the `,` separators as arguments.
    #[test]
    fn multi_bindings_drop_separator_nodes() {
        let meta = MetaVariables {
            single: BTreeMap::new(),
            multi: BTreeMap::from([(
                "ARGS".to_string(),
                vec![
                    Bound {
                        text: "add(1, 2)".into(),
                    },
                    Bound { text: ",".into() },
                    Bound { text: "x".into() },
                ],
            )]),
        };
        assert_eq!(bindings(&Some(meta)).unwrap(), "$$$ARGS = add(1, 2), x");
    }

    #[test]
    fn rewrite_preview_shows_old_and_new_and_says_nothing_was_written() {
        let out = render_rewrite(&parse_matches(REWRITE_FIXTURE).unwrap(), "web/", 20);
        assert!(
            out.starts_with("rewrite preview: 2 replacements in 1 file - nothing written"),
            "{out}"
        );
        assert!(
            out.contains("web/src/a.ts:2:1\n  - console.log(\"hi\")\n  + logger.info(\"hi\")"),
            "{out}"
        );
        assert!(out.contains("  + logger.info(add(1, 2))"), "{out}");
    }

    // The apply engine against the recorded offsets: the real calls change,
    // the string and the comment that spell the same call do not.
    #[test]
    fn splice_applies_recorded_offsets_and_leaves_strings_and_comments() {
        let m = parse_matches(REWRITE_FIXTURE).unwrap();
        let edits: Vec<Edit> = m
            .iter()
            .map(|m| {
                let span = m.replacement_offsets.unwrap();
                Edit::exact(
                    span.start,
                    span.end,
                    &m.text,
                    m.replacement.as_deref().unwrap(),
                )
            })
            .collect();
        let r = splice(SOURCE, &edits).unwrap();
        let out = r.content;
        assert_eq!((r.applied, r.nested), (2, 0));
        assert!(
            out.contains("logger.info(\"hi\");\nlogger.info(add(1, 2));\n"),
            "{out}"
        );
        assert!(
            out.contains("\"console.log(x)\"; // console.log(y)"),
            "{out}"
        );
        // Applying the same plan again finds the text gone: refused, no double edit.
        assert!(splice(&out, &edits).is_err());
    }

    // Failure modes: a nested match (`f(f(x))`) was spliced twice and
    // corrupted the file; a file edited after the preview was overwritten.
    #[test]
    fn splice_keeps_the_outer_match_and_refuses_a_changed_file() {
        let src = "f(f(x))";
        let edits = vec![
            Edit::exact(2, 6, "f(x)", "g(x)"),
            Edit::exact(0, 7, "f(f(x))", "g(f(x))"),
        ];
        let r = splice(src, &edits).unwrap();
        assert_eq!((r.content.as_str(), r.applied, r.nested), ("g(f(x))", 1, 1));
        let stale = vec![Edit::exact(0, 4, "f(y)", "z")];
        assert!(splice("f(x)", &stale).is_err());
        let past_end = vec![Edit::exact(0, 99, "f", "z")];
        assert!(splice("f(x)", &past_end).is_err());
    }

    // Failure mode (ast-grep 0.51.1, a fix with `expandEnd: {regex: ","}`):
    // the replacement span is wider than the match, and checking the wider
    // span against the match text skipped every such file as "changed".
    #[test]
    fn splice_handles_a_fix_wider_than_its_match_and_keeps_crlf() {
        let src = "f(a: 1, b: 2)";
        let widened = Edit {
            start: 2,
            end: 8,
            check_start: 2,
            check_end: 6,
            old: "a: 1".into(),
            new: String::new(),
        };
        assert_eq!(splice(src, &[widened]).unwrap().content, "f(b: 2)");
        let crlf = "fn a() {\r\n    x\r\n}\r\n";
        let multi = Edit::exact(14, 15, "x", "y;\nz");
        assert_eq!(
            splice(crlf, &[multi]).unwrap().content,
            "fn a() {\r\n    y;\r\nz\r\n}\r\n"
        );
    }

    #[test]
    fn scan_render_sorts_by_severity_and_offers_the_fix() {
        let json = r#"[
          {"text":"a","range":{"byteOffset":{"start":0,"end":1},"start":{"line":0,"column":0}},"file":"x.ts","lines":"a","ruleId":"style","severity":"hint","message":"m1"},
          {"text":"b","range":{"byteOffset":{"start":2,"end":3},"start":{"line":1,"column":0}},"file":"x.ts","lines":"b","ruleId":"bug","severity":"error","message":"m2","replacement":"B"}
        ]"#;
        let out = render_scan(&parse_matches(json).unwrap(), "", 10);
        assert!(
            out.starts_with("2 findings in 1 file (bug 1, style 1)"),
            "{out}"
        );
        assert!(
            out.find("bug").unwrap() < out.find("style  m1").unwrap(),
            "{out}"
        );
        assert!(out.contains("  fix: B"), "{out}");
        assert!(
            out.ends_with("1 finding carry a fix: action=apply with the same rule applies them."),
            "{out}"
        );
    }

    // Failure modes: an apply over `path=.` rewrote a CI workflow or a git
    // hook; a call that named .github on purpose was refused anyway.
    #[test]
    fn apply_leaves_git_and_unnamed_protected_dirs_alone() {
        assert!(protected("src/a.ts", ".").is_none());
        assert!(protected(".git/hooks/pre-commit", ".").is_some());
        assert!(protected(".git/hooks/pre-commit", ".git").is_some());
        assert!(protected(".github/workflows/ci.yml", ".").is_some());
        assert!(protected(".github/workflows/ci.yml", ".github").is_none());
        assert!(protected("web\\.vscode\\tasks.json", "web").is_some());
    }

    // Failure mode: a stream cut at the read cap made the whole call fail as
    // unreadable instead of keeping the complete matches.
    #[test]
    fn stream_output_parses_per_line_and_drops_a_cut_tail() {
        let one = r#"{"text":"a","range":{"byteOffset":{"start":0,"end":1},"start":{"line":0,"column":0}},"file":"x.ts"}"#;
        let stream = format!("{one}\n{one}\n{{\"text\":\"cut");
        assert_eq!(parse_matches(&stream).unwrap().len(), 2);
        assert!(parse_matches("{\"text\":").is_err());
    }

    #[test]
    fn read_only_actions_are_everything_but_apply() {
        for a in [
            "search", "scan", "rewrite", "explain", "find", "lint", "preview", "ast",
        ] {
            assert!(
                is_read_only_action(&format!(r#"{{"action":"{a}"}}"#)),
                "{a}"
            );
        }
        assert!(is_read_only_action("{}"), "default action is search");
        assert!(!is_read_only_action(r#"{"action":"apply"}"#));
        assert!(!is_read_only_action(r#"{"action":"bogus"}"#));
    }

    // Failure mode: a missing binary errored the call (or silently answered a
    // shape query with text matches) instead of saying what to do.
    #[test]
    fn missing_binary_answers_with_the_install_line_and_the_alternatives() {
        let dir = std::env::temp_dir().join(format!("nur-astgrep-missing-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = ToolContext::default_for_test(&dir);
        let out = execute_with(
            None,
            &serde_json::json!({"pattern": "foo($A)"}),
            &ctx,
            commit_plain,
        )
        .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            out.starts_with("[ast-grep is not installed. Install: npm i -g @ast-grep/cli"),
            "{out}"
        );
        assert!(
            out.contains("grep pattern=") && out.contains("sgrep query="),
            "{out}"
        );
    }

    #[test]
    fn requests_missing_their_inputs_are_refused_before_running() {
        let dir = std::env::temp_dir().join(format!("nur-astgrep-args-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = ToolContext::default_for_test(&dir);
        let refused =
            |args: Value| execute_with(Some("ast-grep".into()), &args, &ctx, commit_plain).is_err();
        assert!(refused(serde_json::json!({"action": "search"})));
        assert!(refused(
            serde_json::json!({"action": "rewrite", "pattern": "f($A)"})
        ));
        assert!(refused(
            serde_json::json!({"action": "apply", "pattern": "f($A)"})
        ));
        assert!(refused(
            serde_json::json!({"action": "explain", "pattern": "f($A)"})
        ));
        assert!(refused(serde_json::json!({"action": "nope"})));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Failure mode: explain searched the tree (slow, noisy) instead of
    // parsing only the pattern, or lost the hint on a broken pattern.
    #[test]
    fn real_binary_explain_shows_the_parse_and_flags_a_broken_pattern() {
        let Some(bin) = ast_grep_bin() else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("nur-astgrep-ex-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = ToolContext::default_for_test(&dir);
        let ok = execute_with(
            Some(bin.clone()),
            &serde_json::json!({"action": "explain", "pattern": "foo($A, $B)", "lang": "rust"}),
            &ctx,
            commit_plain,
        )
        .unwrap();
        let broken = execute_with(
            Some(bin),
            &serde_json::json!({"action": "explain", "pattern": "foo(", "lang": "ts"}),
            &ctx,
            commit_plain,
        )
        .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            ok.contains("call_expression") && ok.contains("metavariable"),
            "{ok}"
        );
        assert!(!ok.contains("did not parse"), "{ok}");
        assert!(
            broken.contains("ERROR") && broken.contains("did not parse"),
            "{broken}"
        );
    }

    // The real binary, when installed (skipped otherwise): preview writes
    // nothing, apply writes once through the commit path, a second apply
    // finds nothing.
    #[test]
    fn real_binary_preview_then_apply_round_trip() {
        let Some(bin) = ast_grep_bin() else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("nur-astgrep-rt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src").join("a.ts"), SOURCE).unwrap();
        let ctx = ToolContext::default_for_test(&dir);
        let args = |action: &str| serde_json::json!({"action": action, "pattern": "console.log($A)", "rewrite": "logger.info($A)", "lang": "ts"});
        let preview =
            execute_with(Some(bin.clone()), &args("rewrite"), &ctx, commit_plain).unwrap();
        assert!(preview.contains("2 replacements"), "{preview}");
        assert_eq!(
            std::fs::read_to_string(dir.join("src/a.ts")).unwrap(),
            SOURCE
        );
        let applied = execute_with(Some(bin.clone()), &args("apply"), &ctx, commit_plain).unwrap();
        let after = std::fs::read_to_string(dir.join("src/a.ts")).unwrap();
        let again = execute_with(Some(bin), &args("apply"), &ctx, commit_plain).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            applied.starts_with("applied 2 replacements in 1 file"),
            "{applied}"
        );
        assert!(
            after.contains("logger.info(add(1, 2));") && after.contains("// console.log(y)"),
            "{after}"
        );
        assert!(again.starts_with("no matches"), "{again}");
    }
}

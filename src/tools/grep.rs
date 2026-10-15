use super::search_util::{
    clip_line, display_path, fallback_file_filter, normalize_user_glob, rg_grep, walk_builder,
    GrepOptions, RgOutcome, MAX_FILE_BYTES, SEARCH_BUDGET,
};
use super::sgrep::{semantic, Semantic, SemanticRequest};
use super::{arg_str, arg_u64, resolve_path, Tool, ToolContext};
use crate::error::{NurError, Result};
use regex::RegexBuilder;
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::time::Instant;

pub struct Grep;

const DEFAULT_MAX_MATCHES: usize = 50;
const MAX_MAX_MATCHES: usize = 500;
const MAX_CONTEXT: usize = 10;

impl Tool for Grep {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Exact search: file contents by regex, for an identifier, string or pattern you already \
         know. Anything else is sgrep's job (semantic, plain-English, ranked chunks): a pattern \
         written as a description is answered by sgrep, and a search that overflows the default \
         cap comes back ranked by sgrep instead of as a first page. Ripgrep when available; \
         respects .gitignore; skips node_modules/target/etc. at any depth; hidden files \
         included. Output is path:line:text, sorted by path. fixed_strings=true for literal text \
         (no escaping needed), glob/type to narrow files, context for surrounding lines. Set \
         max_matches yourself (up to 500) when you want the exhaustive list."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Regex (Rust/ripgrep syntax), or literal text when fixed_strings=true"},
                "path": {"type": "string", "description": "Directory or file to search (default cwd)"},
                "case_insensitive": {"type": "boolean", "default": false},
                "fixed_strings": {"type": "boolean", "default": false, "description": "Match the pattern literally (for code with ( ) [ ] . * etc.)"},
                "glob": {"type": "string", "description": "Only files matching this glob, relative to path, e.g. *.rs, src/**/*.ts, !*.test.js"},
                "type": {"type": "string", "description": "ripgrep file type: rust, py, js, ts, go, md, json, …"},
                "context": {"type": "integer", "default": 0, "description": format!("Lines of context before and after each match (max {MAX_CONTEXT})")},
                "multiline": {"type": "boolean", "default": false, "description": "Let the pattern span lines (ripgrep only)"},
                "max_matches": {"type": "integer", "default": DEFAULT_MAX_MATCHES, "description": format!("Cap on matching lines (max {MAX_MAX_MATCHES})")}
            },
            "required": ["pattern"]
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let call = GrepCall::parse(args, ctx)?;
        let Some(bin) = sgrep_route() else {
            return Ok(call.finish(grep_only(&call, ctx)?));
        };

        // A description is not a regex: grep would match it literally and
        // find nothing, so the call is answered by sgrep outright.
        if !call.opts.fixed_strings && reads_like_a_phrase(&call.pattern) {
            match semantic(&bin, &call.semantic_request(None), ctx) {
                Semantic::Chunks(out) => {
                    return Ok(format!("[{}]\n{out}", phrase_header(&call.pattern)));
                }
                Semantic::Cancelled => return Ok("[search cancelled]".into()),
                _ => {}
            }
        }

        let grepped = grep_only(&call, ctx)?;
        // Blowing through the default cap means the model did not think
        // about volume; sgrep ranks the matches instead of handing back the
        // first page. An explicit max_matches is the model's own choice.
        if grepped.truncated && !call.explicit_cap {
            match semantic(&bin, &call.semantic_request(Some(&call.pattern)), ctx) {
                Semantic::Chunks(out) => {
                    return Ok(format!("[{}]\n{out}", overflow_header(&call)));
                }
                Semantic::Cancelled => return Ok("[search cancelled]".into()),
                _ => {}
            }
        }
        Ok(call.finish(grepped))
    }
}

/// How many passages a routed call brings back: more than sgrep's default
/// five, since the model asked grep for a list, well under a page.
const ROUTED_COUNT: u64 = 8;

/// The sgrep binary when semantic routing applies: `[tools] sgrep_first`
/// (default on) and a binary on this machine. The probe runs once per
/// process because `ecosystem::find_bin` ends in runtime launches (npm/uv)
/// when nothing is found, which must not happen on every grep call.
fn sgrep_route() -> Option<String> {
    static BIN: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    let enabled = crate::config::load_config()
        .map(|c| c.tools.sgrep_first)
        .unwrap_or(true);
    if !enabled {
        return None;
    }
    BIN.get_or_init(crate::sgrep::sgrep_bin).clone()
}

fn phrase_header(pattern: &str) -> String {
    format!(
        "grep: {pattern:?} reads as a description, which grep matches literally - answered by \
         sgrep query={pattern:?}. grep is for an exact identifier, string or regex; sgrep for \
         what the code does"
    )
}

fn overflow_header(call: &GrepCall) -> String {
    format!(
        "grep: more than {} lines matched {:?} - sgrep ranked them by intent and the top {} \
         passages follow. For the exhaustive list pass max_matches (up to {}) or narrow with \
         glob/path; for a sharper ranking call sgrep query=\"<what you want>\" pattern={:?}",
        call.opts.max_matches, call.pattern, ROUTED_COUNT, MAX_MAX_MATCHES, call.pattern
    )
}

/// One parsed grep call.
struct GrepCall {
    pattern: String,
    root: std::path::PathBuf,
    opts: GrepOptions,
    /// The model set `max_matches` itself.
    explicit_cap: bool,
}

/// What the lexical search produced, before trailers.
struct Grepped {
    text: String,
    truncated: bool,
    notes: Vec<String>,
}

impl GrepCall {
    fn parse(args: &Value, ctx: &ToolContext) -> Result<Self> {
        let pattern = arg_str(args, "pattern")?;
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let flag = |name: &str| args.get(name).and_then(|v| v.as_bool()).unwrap_or(false);
        let opt_str = |name: &str| {
            args.get(name)
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        let cap = arg_u64(args, "max_matches");
        let opts = GrepOptions {
            case_insensitive: flag("case_insensitive"),
            fixed_strings: flag("fixed_strings"),
            glob: opt_str("glob").map(|g| normalize_user_glob(&g)),
            file_type: opt_str("type"),
            context: (arg_u64(args, "context").unwrap_or(0) as usize).min(MAX_CONTEXT),
            multiline: flag("multiline"),
            max_matches: (cap.unwrap_or(DEFAULT_MAX_MATCHES as u64) as usize)
                .clamp(1, MAX_MAX_MATCHES),
        };
        let root = resolve_path(&ctx.cwd, path)?;
        Ok(GrepCall {
            pattern,
            root,
            opts,
            explicit_cap: cap.is_some(),
        })
    }

    fn semantic_request<'a>(&'a self, pattern: Option<&'a str>) -> SemanticRequest<'a> {
        SemanticRequest {
            query: &self.pattern,
            root: &self.root,
            pattern,
            glob: self.opts.glob.as_deref(),
            file_type: self.opts.file_type.as_deref(),
            fixed_strings: self.opts.fixed_strings,
            case_insensitive: self.opts.case_insensitive,
            count: ROUTED_COUNT,
            model: "auto",
        }
    }

    fn finish(&self, g: Grepped) -> String {
        let mut out = g.text;
        if g.truncated {
            out.push_str(&format!(
                "\n[truncated at {} matches - narrow the pattern/path/glob or raise max_matches]",
                self.opts.max_matches
            ));
        }
        for n in g.notes {
            out.push_str(&format!("\n[{n}]"));
        }
        out
    }
}

/// The lexical search alone, no semantic routing: what `sgrep` falls back
/// to when its binary is missing, so the two tools cannot hand a call back
/// and forth.
pub(crate) fn grep_lexical(args: &Value, ctx: &ToolContext) -> Result<String> {
    let call = GrepCall::parse(args, ctx)?;
    let g = grep_only(&call, ctx)?;
    Ok(call.finish(g))
}

fn grep_only(call: &GrepCall, ctx: &ToolContext) -> Result<Grepped> {
    let GrepCall {
        pattern,
        root,
        opts,
        ..
    } = call;
    let none = |notes: Vec<String>| Grepped {
        text: "no matches".into(),
        truncated: false,
        notes,
    };

    // Fast path: system ripgrep (what Claude Code / Cursor / Codex rely on).
    match rg_grep(ctx, pattern, root, opts) {
        RgOutcome::Hits {
            text,
            truncated,
            note,
        } => {
            let notes: Vec<String> = note.into_iter().collect();
            if text.is_empty() {
                return Ok(none(notes));
            }
            return Ok(Grepped {
                text,
                truncated,
                notes,
            });
        }
        RgOutcome::Error(msg) => {
            return Err(NurError::Tool(format!("grep: {msg}")));
        }
        RgOutcome::Unavailable => {}
    }

    // Fallback: parallel ignore walk + line scan, hard excludes + budget.
    let re = RegexBuilder::new(&if opts.fixed_strings {
        regex::escape(pattern)
    } else {
        pattern.clone()
    })
    .case_insensitive(opts.case_insensitive)
    .build()
    .map_err(|e| NurError::Tool(format!("invalid regex: {e}")))?;
    let filter = fallback_file_filter(root, opts.glob.as_deref(), opts.file_type.as_deref())
        .map_err(NurError::Tool)?;

    let start = Instant::now();
    let mut matches = Vec::new();
    let mut count = 0usize;
    let mut truncated = false;
    let mut notes: Vec<String> = Vec::new();
    if opts.multiline {
        notes.push("multiline needs ripgrep; searched line by line".into());
    }

    if root.is_file() {
        let shown = display_path(&ctx.cwd, root);
        truncated = search_file(
            root,
            &shown,
            &re,
            opts.context,
            &mut matches,
            &mut count,
            opts.max_matches,
        );
    } else {
        let mut b = walk_builder(root, Some(MAX_FILE_BYTES));
        if let Some(ov) = filter {
            b.overrides(ov);
        }
        for entry in b.build().flatten() {
            if ctx.cancel.is_cancelled() {
                notes.push("search cancelled".into());
                break;
            }
            if start.elapsed() > SEARCH_BUDGET {
                notes.push(format!(
                    "search stopped after {}ms budget - narrow path or install ripgrep",
                    SEARCH_BUDGET.as_millis()
                ));
                break;
            }
            if truncated {
                break;
            }
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            let p = entry.path();
            let shown = display_path(&ctx.cwd, p);
            truncated |= search_file(
                p,
                &shown,
                &re,
                opts.context,
                &mut matches,
                &mut count,
                opts.max_matches,
            );
        }
    }

    if matches.is_empty() {
        return Ok(none(notes));
    }
    Ok(Grepped {
        text: matches.join("\n"),
        truncated,
        notes,
    })
}

/// Three or more words and no regex syntax: "validate the session token"
/// rather than `fn\s+main` or `TODO`.
fn reads_like_a_phrase(pattern: &str) -> bool {
    pattern.split_whitespace().count() >= 3
        && !pattern.chars().any(|c| {
            matches!(
                c,
                '\\' | '[' | ']' | '(' | ')' | '|' | '*' | '+' | '?' | '^' | '$' | '{' | '}' | '.'
            )
        })
}

/// Scan one file, printing it as `shown`. `count` is the running number of
/// match lines across files (context lines and separators do not count).
/// Returns `true` when the match cap stopped the scan with matches still
/// unreported.
fn search_file(
    path: &Path,
    shown: &str,
    re: &regex::Regex,
    context: usize,
    matches: &mut Vec<String>,
    count: &mut usize,
    max: usize,
) -> bool {
    // Skip huge / binary-ish files quickly
    if let Ok(meta) = fs::metadata(path) {
        if meta.len() > MAX_FILE_BYTES {
            return false;
        }
    }
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(_) => return false,
    };
    // NUL byte ⇒ binary
    if bytes.contains(&0) {
        return false;
    }
    let content = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let lines: Vec<&str> = content.lines().collect();
    let mut last_emitted: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        if !re.is_match(line) {
            continue;
        }
        if *count >= max {
            return true;
        }
        *count += 1;
        if context > 0 {
            let from = i.saturating_sub(context);
            if let Some(prev) = last_emitted {
                if from > prev + 1 {
                    matches.push("--".into());
                }
            }
            for (j, before) in lines.iter().enumerate().take(i).skip(from) {
                if last_emitted.is_none_or(|p| j > p) {
                    matches.push(format!("{shown}-{}-{}", j + 1, clip_line(before)));
                }
            }
        }
        matches.push(format!("{shown}:{}:{}", i + 1, clip_line(line)));
        last_emitted = Some(i);
        if context > 0 {
            let to = (i + context + 1).min(lines.len());
            for (j, after) in lines.iter().enumerate().take(to).skip(i + 1) {
                if re.is_match(after) {
                    break;
                }
                matches.push(format!("{shown}-{}-{}", j + 1, clip_line(after)));
                last_emitted = Some(j);
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_file(name: &str, body: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("nur-grep-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        p
    }

    // Failure modes: the cap silently drops matches with no signal, context
    // lines count as matches (a `12:30:45` in a context line used to), two
    // adjacent blocks get a stray separator, a long line is emitted whole.
    #[test]
    fn fallback_scan_caps_matches_and_keeps_context_blocks() {
        let long = "x".repeat(2000);
        let p = tmp_file(
            "ctx.txt",
            &format!("at 12:30:45\nhit1\nb\nc\nd\nhit2 {long}\ne\nhit3\n"),
        );
        let re = regex::Regex::new("hit").unwrap();
        let mut out = Vec::new();
        let mut count = 0;
        let truncated = search_file(&p, "ctx.txt", &re, 1, &mut out, &mut count, 2);
        assert!(truncated, "third match must flag truncation");
        assert_eq!(count, 2);
        let joined = out.join("\n");
        assert!(joined.contains("ctx.txt-1-at 12:30:45"), "{joined}");
        assert!(joined.contains("ctx.txt:2:hit1"), "{joined}");
        assert!(
            joined.contains("--"),
            "blocks 1-3 and 5-7 are separate: {joined}"
        );
        assert!(!joined.contains("hit3"));
        assert!(joined.contains("more bytes]"), "{joined}");
        assert!(out.iter().all(|l| l.len() < 600), "{joined}");
        let _ = std::fs::remove_file(p);
    }

    // Failure modes of the sgrep hand-off: a regex with spaces (`impl .* for`)
    // routed as prose, a two-word literal routed, a model's own max_matches
    // mistaken for the default cap (its exhaustive list would be replaced by
    // eight passages), an out-of-range cap losing the "explicit" mark.
    #[test]
    fn phrase_and_explicit_cap_detection() {
        assert!(reads_like_a_phrase("validate the session token"));
        assert!(!reads_like_a_phrase("impl .* for"));
        assert!(!reads_like_a_phrase("fn\\s+main"));
        assert!(!reads_like_a_phrase("session token"));
        assert!(!reads_like_a_phrase("TODO"));

        let dir = std::env::temp_dir();
        let ctx = ToolContext::default_for_test(&dir);
        let by_default = GrepCall::parse(&serde_json::json!({"pattern": "x"}), &ctx).unwrap();
        assert!(!by_default.explicit_cap);
        assert_eq!(by_default.opts.max_matches, DEFAULT_MAX_MATCHES);
        let chosen = GrepCall::parse(
            &serde_json::json!({"pattern": "x", "max_matches": 50}),
            &ctx,
        )
        .unwrap();
        assert!(
            chosen.explicit_cap,
            "same number as the default, still a choice"
        );
        let huge = GrepCall::parse(
            &serde_json::json!({"pattern": "x", "max_matches": 9999}),
            &ctx,
        )
        .unwrap();
        assert!(huge.explicit_cap);
        assert_eq!(huge.opts.max_matches, MAX_MAX_MATCHES);
    }
}

use super::search_util::{
    clip_line, display_path, fallback_file_filter, normalize_user_glob, rg_grep, walk_builder,
    GrepOptions, RgOutcome, MAX_FILE_BYTES, SEARCH_BUDGET,
};
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
        "Search file contents with a regex (ripgrep when available; respects .gitignore; skips \
         node_modules/target/etc. at any depth; hidden files included). Output is path:line:text, \
         sorted by path. fixed_strings=true for literal text (no escaping needed), glob/type to \
         narrow files, context for surrounding lines. A '[truncated …]' trailer means more matches \
         exist - narrow the pattern or path, or raise max_matches."
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
        let opts = GrepOptions {
            case_insensitive: flag("case_insensitive"),
            fixed_strings: flag("fixed_strings"),
            glob: opt_str("glob").map(|g| normalize_user_glob(&g)),
            file_type: opt_str("type"),
            context: (arg_u64(args, "context").unwrap_or(0) as usize).min(MAX_CONTEXT),
            multiline: flag("multiline"),
            max_matches: (arg_u64(args, "max_matches").unwrap_or(DEFAULT_MAX_MATCHES as u64)
                as usize)
                .clamp(1, MAX_MAX_MATCHES),
        };

        let root = resolve_path(&ctx.cwd, path)?;

        // Fast path: system ripgrep (what Claude Code / Cursor / Codex rely on).
        match rg_grep(ctx, &pattern, &root, &opts) {
            RgOutcome::Hits {
                text,
                truncated,
                note,
            } => {
                let notes: Vec<String> = note.into_iter().collect();
                if text.is_empty() {
                    return Ok(finish("no matches".into(), false, 0, Some(notes)));
                }
                return Ok(finish(text, truncated, opts.max_matches, Some(notes)));
            }
            RgOutcome::Error(msg) => {
                return Err(NurError::Tool(format!("grep: {msg}")));
            }
            RgOutcome::Unavailable => {}
        }

        // Fallback: parallel ignore walk + line scan, hard excludes + budget.
        let re = RegexBuilder::new(&if opts.fixed_strings {
            regex::escape(&pattern)
        } else {
            pattern.clone()
        })
        .case_insensitive(opts.case_insensitive)
        .build()
        .map_err(|e| NurError::Tool(format!("invalid regex: {e}")))?;
        let filter = fallback_file_filter(&root, opts.glob.as_deref(), opts.file_type.as_deref())
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
            let shown = display_path(&ctx.cwd, &root);
            truncated = search_file(
                &root,
                &shown,
                &re,
                opts.context,
                &mut matches,
                &mut count,
                opts.max_matches,
            );
        } else {
            let mut b = walk_builder(&root, Some(MAX_FILE_BYTES));
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
            return Ok(finish("no matches".into(), false, 0, Some(notes)));
        }
        Ok(finish(
            matches.join("\n"),
            truncated,
            opts.max_matches,
            Some(notes),
        ))
    }
}

fn finish(text: String, truncated: bool, max: usize, notes: Option<Vec<String>>) -> String {
    let mut out = text;
    if truncated {
        out.push_str(&format!(
            "\n[truncated at {max} matches - narrow the pattern/path/glob or raise max_matches]"
        ));
    }
    for n in notes.unwrap_or_default() {
        out.push_str(&format!("\n[{n}]"));
    }
    out
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
}

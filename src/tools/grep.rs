use super::search_util::{
    fallback_file_filter, is_hard_excluded, rg_grep, walk_builder, GrepOptions, RgOutcome,
    SEARCH_BUDGET,
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
                "glob": {"type": "string", "description": "Only files matching this glob, e.g. *.rs, src/**/*.ts, !*.test.js"},
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
            glob: opt_str("glob"),
            file_type: opt_str("type"),
            context: (arg_u64(args, "context").unwrap_or(0) as usize).min(MAX_CONTEXT),
            multiline: flag("multiline"),
            max_matches: (arg_u64(args, "max_matches").unwrap_or(DEFAULT_MAX_MATCHES as u64)
                as usize)
                .clamp(1, MAX_MAX_MATCHES),
        };

        let root = resolve_path(&ctx.cwd, path)?;

        // Fast path: system ripgrep (what Claude Code / Cursor / Codex rely on).
        match rg_grep(&ctx.cwd, &pattern, &root, &opts) {
            RgOutcome::Hits { text, truncated } => {
                if text.is_empty() {
                    return Ok("no matches".into());
                }
                return Ok(finish(text, truncated, opts.max_matches, None));
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
        let mut truncated = false;
        let mut notes: Vec<String> = Vec::new();
        if opts.multiline {
            notes.push("multiline needs ripgrep; searched line by line".into());
        }

        if root.is_file() {
            truncated = search_file(&root, &re, opts.context, &mut matches, opts.max_matches);
        } else {
            let mut b = walk_builder(&root);
            if let Some(ov) = filter {
                b.overrides(ov);
            }
            for entry in b.build().flatten() {
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
                let p = entry.path();
                if !p.is_file() || is_hard_excluded(p) {
                    continue;
                }
                truncated |= search_file(p, &re, opts.context, &mut matches, opts.max_matches);
            }
        }

        if matches.is_empty() {
            let mut out = String::from("no matches");
            for n in notes {
                out.push_str(&format!("\n[{n}]"));
            }
            return Ok(out);
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

/// Scan one file. Returns `true` when the match cap stopped the scan with
/// matches still unreported.
fn search_file(
    path: &Path,
    re: &regex::Regex,
    context: usize,
    matches: &mut Vec<String>,
    max: usize,
) -> bool {
    // Skip huge / binary-ish files quickly
    if let Ok(meta) = fs::metadata(path) {
        if meta.len() > super::search_util::MAX_FILE_BYTES {
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
    let shown = path.to_string_lossy().replace('\\', "/");
    let lines: Vec<&str> = content.lines().collect();
    let mut total = matches_count(matches);
    let mut last_emitted: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        if !re.is_match(line) {
            continue;
        }
        if total >= max {
            return true;
        }
        total += 1;
        if context > 0 {
            let from = i.saturating_sub(context);
            if let Some(prev) = last_emitted {
                if from > prev + 1 {
                    matches.push("--".into());
                }
            }
            for j in from..i {
                if last_emitted.map_or(true, |p| j > p) {
                    matches.push(format!("{shown}-{}-{}", j + 1, lines[j]));
                }
            }
        }
        matches.push(format!("{shown}:{}:{line}", i + 1));
        last_emitted = Some(i);
        if context > 0 {
            let to = (i + context + 1).min(lines.len());
            for j in (i + 1)..to {
                if re.is_match(lines[j]) {
                    break;
                }
                matches.push(format!("{shown}-{}-{}", j + 1, lines[j]));
                last_emitted = Some(j);
            }
        }
    }
    false
}

/// Matching lines only (context lines and separators do not count toward the cap).
fn matches_count(lines: &[String]) -> usize {
    lines
        .iter()
        .filter(|l| super::search_util::is_rg_match_line(l))
        .count()
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
    // lines count as matches, two adjacent blocks get a stray separator.
    #[test]
    fn fallback_scan_caps_matches_and_keeps_context_blocks() {
        let p = tmp_file("ctx.txt", "a\nhit1\nb\nc\nd\nhit2\ne\nhit3\n");
        let re = regex::Regex::new("hit").unwrap();
        let mut out = Vec::new();
        let truncated = search_file(&p, &re, 1, &mut out, 2);
        assert!(truncated, "third match must flag truncation");
        assert_eq!(matches_count(&out), 2);
        let joined = out.join("\n");
        assert!(joined.contains("-1-a"), "{joined}");
        assert!(joined.contains(":2:hit1"), "{joined}");
        assert!(
            joined.contains("--"),
            "blocks 1-3 and 5-7 are separate: {joined}"
        );
        assert!(!joined.contains("hit3"));
        let _ = std::fs::remove_file(p);
    }
}

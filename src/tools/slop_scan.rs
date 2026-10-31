//! `slop_scan`: the mechanical half of the `codebase-slop` audit.
//!
//! Counting is the tool's job, judging is the model's: every finding is a
//! candidate to read before calling it slop. Signals:
//!
//! - **god files**: line and top-level-definition counts against the repo's own
//!   distribution (p95, with an absolute floor);
//! - **duplication**: normalized 8-line windows (whitespace collapsed, blank and
//!   comment-only lines dropped) hashed and matched across files, merged into
//!   ranges (the method PMD CPD and jscpd use, written here);
//! - **change coupling** and **shotgun surgery** from `git log`: file pairs in
//!   different directories that change together in most of their commits, and
//!   commits that touch many files across many directories (the idea is from
//!   code-maat, which is GPL-3.0, so only the method is used);
//! - **layer-first folders**: three or more siblings named after layers;
//! - **pass-through wrappers**: one-expression bodies forwarding their own
//!   parameters, by pattern for Rust, TypeScript/JavaScript and Python.
//!
//! Bounded by file count, bytes and the shared search time budget; repo text
//! in the report has control characters stripped and is clipped.

use super::{arg_str, arg_u64, Tool, ToolContext};
use crate::error::{NurError, Result};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub struct SlopScan;

const CODE_EXTENSIONS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "py", "go", "java", "kt", "swift", "rb", "php",
    "cs", "c", "cc", "cpp", "h", "hpp", "scala", "vue", "svelte",
];
const LAYER_NAMES: &[&str] = &[
    "controllers",
    "services",
    "models",
    "repositories",
    "handlers",
    "utils",
    "helpers",
    "dto",
    "dtos",
];
const DEFAULT_MAX_FILES: usize = 4_000;
const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
const WINDOW: usize = 8;
const GOD_FILE_FLOOR: usize = 500;
const TOP: usize = 10;
const COUPLING_MIN_SHARED: usize = 5;
const COUPLING_RATIO: f64 = 0.6;

impl Tool for SlopScan {
    fn name(&self) -> &str {
        "slop_scan"
    }

    fn description(&self) -> &str {
        "Read-only structural scan for codebase-slop candidates: god files, duplicated blocks \
         across files, change coupling and shotgun-surgery commits from git history, layer-first \
         folders, pass-through wrappers. Returns candidates with file:line evidence for you to \
         judge: read the code before calling anything slop. Pairs with the codebase-slop skill."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Subdirectory to scan (default: the workspace)"},
                "signals": {
                    "type": "array",
                    "items": {"type": "string", "enum": ["god_files", "duplication", "coupling", "layers", "pass_through"]},
                    "description": "Which signals to compute (default: all)"
                },
                "max_files": {"type": "integer", "description": "Files to read at most (default 4000)"}
            }
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let root = match arg_str(args, "path") {
            Ok(p) if !p.trim().is_empty() => super::resolve_path(&ctx.cwd, p.trim())?,
            _ => ctx.cwd.clone(),
        };
        if !root.is_dir() {
            return Err(NurError::Tool(format!(
                "not a directory: {}",
                root.display()
            )));
        }
        let wanted: HashSet<String> = args
            .get("signals")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();
        const SIGNALS: &[&str] = &[
            "god_files",
            "duplication",
            "coupling",
            "layers",
            "pass_through",
        ];
        if let Some(unknown) = wanted.iter().find(|s| !SIGNALS.contains(&s.as_str())) {
            return Err(NurError::Tool(format!(
                "unknown signal `{unknown}`; use {}",
                SIGNALS.join(", ")
            )));
        }
        let on = |signal: &str| wanted.is_empty() || wanted.contains(signal);
        let max_files = arg_u64(args, "max_files")
            .map(|n| n as usize)
            .unwrap_or(DEFAULT_MAX_FILES)
            .clamp(1, 20_000);
        let files = read_files(&root, max_files, ctx);
        let mut out = format!(
            "slop_scan · {} files under {} · candidates only, read the code before acting\n",
            files.files.len(),
            super::search_util::display_path(&ctx.cwd, &root)
        );
        if files.truncated {
            out.push_str("(stopped at the file, byte or time limit; narrow `path` to see more)\n");
        }
        if on("god_files") {
            out.push_str(&god_files(&files.files));
        }
        if on("duplication") {
            out.push_str(&duplication(&files.files, &ctx.cancel));
        }
        if on("coupling") {
            out.push_str(&coupling(&root));
        }
        if on("layers") {
            out.push_str(&layers(&root, &ctx.cwd));
        }
        if on("pass_through") {
            out.push_str(&pass_through(&files.files));
        }
        Ok(out)
    }
}

struct SourceFile {
    /// Path relative to the scan root, `/`-separated.
    rel: String,
    text: String,
}

struct Files {
    files: Vec<SourceFile>,
    truncated: bool,
}

fn read_files(root: &Path, max_files: usize, ctx: &ToolContext) -> Files {
    let started = Instant::now();
    let mut files = Vec::new();
    let mut bytes = 0u64;
    let mut truncated = false;
    for entry in super::search_util::walk_builder(root, Some(super::search_util::MAX_FILE_BYTES))
        .build()
        .flatten()
    {
        if ctx.cancel.is_cancelled() || started.elapsed() > super::search_util::SEARCH_BUDGET {
            truncated = true;
            break;
        }
        let path = entry.path();
        let is_code = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| CODE_EXTENSIONS.contains(&e));
        if !is_code || !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        if files.len() >= max_files || bytes > MAX_TOTAL_BYTES {
            truncated = true;
            break;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        bytes += text.len() as u64;
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        files.push(SourceFile { rel, text });
    }
    Files { files, truncated }
}

/// Strip control characters and clip, for repo text quoted in the report.
fn clean(text: &str, max: usize) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > max {
        format!("{}…", flat.chars().take(max).collect::<String>())
    } else {
        flat
    }
}

fn top_level_definitions(text: &str) -> usize {
    const STARTS: &[&str] = &[
        "fn ",
        "pub fn ",
        "pub(crate) fn ",
        "async fn ",
        "pub async fn ",
        "struct ",
        "pub struct ",
        "enum ",
        "pub enum ",
        "impl ",
        "trait ",
        "pub trait ",
        "def ",
        "class ",
        "async def ",
        "function ",
        "export function ",
        "export default function ",
        "export class ",
        "func ",
        "type ",
        "export type ",
        "interface ",
        "export interface ",
        "const ",
        "export const ",
    ];
    text.lines()
        .filter(|line| STARTS.iter().any(|s| line.starts_with(s)))
        .count()
}

fn god_files(files: &[SourceFile]) -> String {
    let mut sizes: Vec<(usize, &SourceFile)> =
        files.iter().map(|f| (f.text.lines().count(), f)).collect();
    if sizes.is_empty() {
        return String::new();
    }
    let mut counts: Vec<usize> = sizes.iter().map(|(n, _)| *n).collect();
    counts.sort_unstable();
    let p95 = counts[(counts.len() * 95 / 100).min(counts.len() - 1)];
    let threshold = p95.max(GOD_FILE_FLOOR);
    sizes.retain(|(n, _)| *n >= threshold);
    sizes.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.rel.cmp(&b.1.rel)));
    let mut out = format!("\n## God files (≥ {threshold} lines; repo p95 is {p95})\n");
    if sizes.is_empty() {
        out.push_str("none\n");
    }
    for (lines, file) in sizes.iter().take(TOP) {
        out.push_str(&format!(
            "- {} · {lines} lines · {} top-level definitions\n",
            file.rel,
            top_level_definitions(&file.text)
        ));
    }
    out
}

/// Code lines worth comparing: normalized text and original 1-based number.
fn normalized_lines(text: &str) -> Vec<(String, usize)> {
    text.lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let t = line.trim();
            let comment = ["//", "#", "/*", "*", "--", "\"\"\""]
                .iter()
                .any(|c| t.starts_with(c));
            let norm = t.split_whitespace().collect::<Vec<_>>().join(" ");
            (norm.len() > 2 && !comment).then_some((norm, i + 1))
        })
        .collect()
}

fn duplication(files: &[SourceFile], cancel: &tokio_util::sync::CancellationToken) -> String {
    let lines: Vec<Vec<(String, usize)>> =
        files.iter().map(|f| normalized_lines(&f.text)).collect();
    let mut windows: HashMap<u64, Vec<(usize, usize)>> = HashMap::new();
    for (fi, file_lines) in lines.iter().enumerate() {
        if cancel.is_cancelled() {
            return String::from("\n## Duplicated blocks\ncancelled\n");
        }
        for start in 0..file_lines.len().saturating_sub(WINDOW - 1) {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            for (text, _) in &file_lines[start..start + WINDOW] {
                text.hash(&mut h);
            }
            windows.entry(h.finish()).or_default().push((fi, start));
        }
    }
    // Matching window pairs in different files, then runs of consecutive pairs.
    let mut pairs: BTreeMap<(usize, usize), Vec<(usize, usize)>> = BTreeMap::new();
    for spots in windows.values() {
        if spots.len() < 2 || spots.len() > 20 {
            continue; // boilerplate repeated everywhere is not a copy to merge
        }
        for (i, a) in spots.iter().enumerate() {
            for b in &spots[i + 1..] {
                if a.0 != b.0 {
                    let (x, y) = if a.0 < b.0 { (*a, *b) } else { (*b, *a) };
                    pairs.entry((x.0, y.0)).or_default().push((x.1, y.1));
                }
            }
        }
    }
    let mut ranges: Vec<(usize, String)> = Vec::new();
    for ((fa, fb), mut starts) in pairs {
        starts.sort_unstable();
        let mut run: Option<(usize, usize, usize)> = None; // (a, b, windows)
        let mut flush = |run: (usize, usize, usize)| {
            let (a, b, n) = run;
            let len = n + WINDOW - 1;
            let la = &lines[fa];
            let lb = &lines[fb];
            ranges.push((
                len,
                format!(
                    "- {}:{}-{} ≈ {}:{}-{} ({len} code lines)",
                    files[fa].rel,
                    la[a].1,
                    la[(a + len - 1).min(la.len() - 1)].1,
                    files[fb].rel,
                    lb[b].1,
                    lb[(b + len - 1).min(lb.len() - 1)].1,
                ),
            ));
        };
        for (a, b) in starts {
            run = match run {
                Some((ra, rb, n)) if a == ra + n && b == rb + n => Some((ra, rb, n + 1)),
                Some(done) => {
                    flush(done);
                    Some((a, b, 1))
                }
                None => Some((a, b, 1)),
            };
        }
        if let Some(done) = run {
            flush(done);
        }
    }
    ranges.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut out = format!("\n## Duplicated blocks (≥ {WINDOW} matching code lines)\n");
    if ranges.is_empty() {
        out.push_str("none\n");
    }
    for (_, line) in ranges.iter().take(TOP) {
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn git_log(root: &Path) -> Option<String> {
    // Commit headers start with a NUL, which no path can hold (a path may
    // start with `@`), and paths come unquoted so non-ASCII names survive.
    let out = std::process::Command::new("git")
        .args([
            "-c",
            "core.quotepath=off",
            "log",
            "--no-merges",
            "--name-only",
            "--pretty=format:%x00%h %s",
            // Paths relative to the scan root, like every other section.
            "--relative",
            "-n",
            "500",
            "--",
            ".",
        ])
        .current_dir(root)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn parent_dir(path: &str) -> &str {
    path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or(".")
}

fn coupling(root: &Path) -> String {
    let mut out = String::from("\n## Change coupling (git history)\n");
    let Some(log) = git_log(root) else {
        out.push_str("no git history here\n");
        return out;
    };
    let mut commits: Vec<(String, Vec<String>)> = Vec::new();
    for line in log.lines() {
        if let Some(head) = line.strip_prefix('\0') {
            commits.push((head.to_string(), Vec::new()));
        } else if let Some((_, files)) = commits.last_mut() {
            let path = line.trim();
            let is_code = path
                .rsplit_once('.')
                .is_some_and(|(_, ext)| CODE_EXTENSIONS.contains(&ext));
            if is_code {
                files.push(path.to_string());
            }
        }
    }
    let mut per_file: HashMap<&str, usize> = HashMap::new();
    let mut together: HashMap<(&str, &str), usize> = HashMap::new();
    for (_, files) in &commits {
        if files.len() > 30 {
            continue; // a sweeping change says nothing about two files
        }
        for f in files {
            *per_file.entry(f.as_str()).or_default() += 1;
        }
        for (i, a) in files.iter().enumerate() {
            for b in &files[i + 1..] {
                if parent_dir(a) != parent_dir(b) {
                    let key = if a < b {
                        (a.as_str(), b.as_str())
                    } else {
                        (b.as_str(), a.as_str())
                    };
                    *together.entry(key).or_default() += 1;
                }
            }
        }
    }
    let mut coupled: Vec<(f64, usize, String)> = together
        .into_iter()
        .filter_map(|((a, b), shared)| {
            let fewer = per_file[a].min(per_file[b]).max(1);
            let ratio = shared as f64 / fewer as f64;
            (shared >= COUPLING_MIN_SHARED && ratio >= COUPLING_RATIO).then(|| {
                (
                    ratio,
                    shared,
                    format!(
                        "- {a} ↔ {b} · changed together in {shared} commits ({:.0}% of the rarer file's)",
                        ratio * 100.0
                    ),
                )
            })
        })
        .collect();
    coupled.sort_by(|x, y| {
        y.1.cmp(&x.1)
            .then(y.0.partial_cmp(&x.0).unwrap_or(std::cmp::Ordering::Equal))
            .then(x.2.cmp(&y.2))
    });
    if coupled.is_empty() {
        out.push_str("no file pairs across directories change together that often\n");
    }
    for (_, _, line) in coupled.iter().take(TOP) {
        out.push_str(line);
        out.push('\n');
    }
    let shotgun: Vec<String> = commits
        .iter()
        .filter(|(_, files)| {
            let dirs: HashSet<&str> = files.iter().map(|f| parent_dir(f)).collect();
            files.len() >= 6 && dirs.len() >= 4 && files.len() <= 30
        })
        .take(5)
        .map(|(head, files)| format!("- {} · {} files", clean(head, 90), files.len()))
        .collect();
    out.push_str("\n## Shotgun-surgery commits (6+ code files in 4+ directories)\n");
    if shotgun.is_empty() {
        out.push_str("none in the last 500 commits\n");
    }
    for line in shotgun {
        out.push_str(&line);
        out.push('\n');
    }
    out
}

fn layers(root: &Path, cwd: &Path) -> String {
    let mut found: Vec<String> = Vec::new();
    let mut dirs: Vec<PathBuf> = vec![root.to_path_buf()];
    let mut seen = 0;
    while let Some(dir) = dirs.pop() {
        seen += 1;
        if seen > 5_000 {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut layer_children = Vec::new();
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || super::search_util::HARD_EXCLUDES.contains(&name.as_str()) {
                continue;
            }
            if LAYER_NAMES.contains(&name.to_ascii_lowercase().as_str()) {
                layer_children.push(name);
            }
            dirs.push(entry.path());
        }
        if layer_children.len() >= 3 {
            layer_children.sort();
            found.push(format!(
                "- {}/ holds {}",
                super::search_util::display_path(cwd, &dir),
                layer_children.join(", ")
            ));
        }
    }
    let mut out = String::from("\n## Layer-first folders (3+ layer-named siblings)\n");
    if found.is_empty() {
        out.push_str("none\n");
    }
    found.sort();
    for line in found.iter().take(TOP) {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Parameter names from a parameter list, types and defaults dropped.
fn param_names(params: &str) -> Vec<String> {
    params
        .split(',')
        .map(|p| {
            p.split([':', '='])
                .next()
                .unwrap_or("")
                .trim()
                .trim_start_matches("mut ")
                .trim_start_matches('&')
                .trim_start_matches('*')
                .to_string()
        })
        .filter(|p| !p.is_empty() && p != "self" && p != "&self" && p != "&mut self")
        .collect()
}

fn arg_names(args: &str) -> Vec<String> {
    args.split(',')
        .map(|a| {
            a.trim()
                .trim_start_matches("&mut ")
                .trim_start_matches('&')
                .trim_start_matches('*')
                .to_string()
        })
        .filter(|a| !a.is_empty())
        .collect()
}

fn pass_through(files: &[SourceFile]) -> String {
    use std::sync::OnceLock;
    static PATTERNS: OnceLock<Vec<regex::Regex>> = OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| {
        [
            // Rust: fn name(params) -> T { path(args) }
            r"(?m)^\s*(?:pub(?:\([a-z]+\))?\s+)?fn\s+(\w+)\s*(?:<[^>]*>)?\(([^)]*)\)[^{;]*\{\s*(?:return\s+)?[\w:.]+\(([^()]*)\)\s*;?\s*\}",
            // JS/TS: function name(params) { return f(args); }
            r"(?m)^\s*(?:export\s+)?(?:async\s+)?function\s+(\w+)\s*\(([^)]*)\)[^{]*\{\s*return\s+(?:await\s+)?[\w.]+\(([^()]*)\);?\s*\}",
            // JS/TS: const name = (params) => f(args)
            r"(?m)^\s*(?:export\s+)?const\s+(\w+)\s*=\s*(?:async\s*)?\(([^)]*)\)\s*(?::[^=]+)?=>\s*(?:await\s+)?[\w.]+\(([^()]*)\);?\s*$",
            // Python: def name(params):\n    return f(args)
            r"(?m)^\s*(?:async\s+)?def\s+(\w+)\s*\(([^)]*)\)[^:]*:\s*\n\s+return\s+(?:await\s+)?[\w.]+\(([^()]*)\)\s*$",
        ]
        .iter()
        .map(|p| regex::Regex::new(p).expect("pass-through pattern"))
        .collect()
    });
    let mut found = Vec::new();
    for file in files {
        for pattern in patterns {
            for caps in pattern.captures_iter(&file.text) {
                let params = param_names(&caps[2]);
                if params.is_empty() || params != arg_names(&caps[3]) {
                    continue;
                }
                let line = file.text[..caps.get(0).map_or(0, |m| m.start())]
                    .matches('\n')
                    .count()
                    + 1;
                found.push(format!("- {}:{line} `{}`", file.rel, clean(&caps[1], 60)));
            }
        }
    }
    let mut out =
        String::from("\n## Pass-through wrapper candidates (body only forwards its parameters)\n");
    if found.is_empty() {
        out.push_str("none\n");
    }
    for line in found.iter().take(TOP * 2) {
        out.push_str(line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(rel: &str, text: &str) -> SourceFile {
        SourceFile {
            rel: rel.into(),
            text: text.into(),
        }
    }

    // Failure modes: a block copied between files is missed or reported in
    // pieces; blank lines or comments break a match; a big file is not flagged
    // against a small repo; a real wrapper is missed or a narrowing one flagged.

    #[test]
    fn a_copied_block_is_one_range_across_files() {
        let block: String = (0..12)
            .map(|i| format!("let value_{i} = compute({i});\n"))
            .collect();
        let a = format!("fn a() {{\n{block}}}\n");
        let b = format!("// header\n\nfn b() {{\n// note\n{block}}}\n");
        let report = duplication(
            &[file("a.rs", &a), file("b.rs", &b)],
            &tokio_util::sync::CancellationToken::new(),
        );
        assert!(
            report.contains("a.rs:2-13 ≈ b.rs:5-16 (12 code lines)"),
            "{report}"
        );
    }

    #[test]
    fn a_file_far_above_the_repo_is_a_god_file() {
        let small = file("small.rs", "fn x() {}\n");
        let big = file("big.rs", &"fn y() {}\n".repeat(900));
        let report = god_files(&[small, big]);
        assert!(
            report.contains("big.rs · 900 lines · 900 top-level definitions"),
            "{report}"
        );
        assert!(!report.contains("small.rs"));
    }

    #[test]
    fn forwarding_wrappers_are_candidates_and_narrowing_ones_are_not() {
        let rust = "pub fn load(path: &Path, opts: Opts) -> Result<Data> {\n    inner::load(path, opts)\n}\n\
                    fn narrow(path: &Path) -> Data {\n    inner::load(path, Opts::default())\n}\n";
        let py = "def fetch(url, timeout):\n    return client.fetch(url, timeout)\n";
        let report = pass_through(&[file("lib.rs", rust), file("api.py", py)]);
        assert!(report.contains("lib.rs:1 `load`"), "{report}");
        assert!(report.contains("api.py:1 `fetch`"), "{report}");
        assert!(!report.contains("narrow"), "{report}");
    }
}

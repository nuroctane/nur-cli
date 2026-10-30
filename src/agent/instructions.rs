//! Standing instructions: the user's, then every project file from the git
//! root down to the working directory.
//!
//! What this replaced read one file from the working directory only, let a
//! `NUR.md` hide a team's `AGENTS.md`, and cut at 20,000 characters without a
//! word (nur-cli's own AGENTS.md lost its last two sections). Now:
//!
//! - **Chain.** For each directory from the git root to the cwd: `AGENTS.md`
//!   (else `CLAUDE.md`), then `NUR.md` as a nur-only layer on top.
//! - **Budget, never silent.** `[instructions] max_chars` across the chain;
//!   what does not fit is spilled (see [`Section::overflow`]) and the prompt
//!   says where the rest is.
//! - **Imports.** A line that is exactly `@path` outside a code fence is
//!   replaced by that file. A project file may import only inside its
//!   repository, a user file only under the home directory; credential-shaped
//!   paths or content are refused with a one-line notice.
//! - **Nested on read.** A directory below the cwd with its own AGENTS.md is
//!   delivered once, in the result of the first tool that touches it
//!   ([`nested_for_tool`]), so the cached system prompt never changes.
//! - Hidden Unicode (tag characters, bidi controls) is stripped before any of
//!   it reaches a model.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Base file per directory, first found wins; `NUR.md` layers on top.
const BASE_FILES: &[&str] = &["AGENTS.md", "CLAUDE.md"];
const LAYER_FILE: &str = "NUR.md";
/// The nur home's own file, first found wins (as before).
const USER_FILES: &[&str] = &["NUR.md", "AGENTS.md"];
const MAX_IMPORT_DEPTH: usize = 5;
const MAX_FILE_BYTES: u64 = 1_000_000;
/// Directories whose AGENTS.md is never delivered on read.
const SKIPPED_DIRS: &[&str] = &["node_modules", "vendor", "target", ".git", "third_party"];

/// One instruction file as it reaches the prompt.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    /// `AGENTS.md`, `pkg/app/AGENTS.md`, `~/.nur/NUR.md`: what the heading shows.
    pub label: String,
    pub path: PathBuf,
    /// What fits in the budget (all of it when [`Self::overflow`] is `None`).
    pub text: String,
    pub overflow: Option<Overflow>,
}

/// The part of a section past the budget.
#[derive(Debug, Clone, PartialEq)]
pub struct Overflow {
    /// The whole file (imports expanded), for the context store.
    pub full: String,
    /// 1-based line of the file where the shown text stops.
    pub next_line: usize,
    pub remaining_chars: usize,
}

/// Everything loaded for one prompt.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Instructions {
    pub user: Vec<Section>,
    pub project: Vec<Section>,
    /// Refused imports, stripped characters: shown by `/context`.
    pub notices: Vec<String>,
}

impl Instructions {
    pub fn sections(&self) -> impl Iterator<Item = &Section> {
        self.user.iter().chain(self.project.iter())
    }
}

/// Load the chain for `cwd` with `max_chars` across all sections.
pub fn load(cwd: &Path, max_chars: usize) -> Instructions {
    let home = crate::config::nur_home();
    load_with(cwd, &home, crate::config::home_dir().as_deref(), max_chars)
}

fn load_with(cwd: &Path, nur_home: &Path, home: Option<&Path>, max_chars: usize) -> Instructions {
    let mut notices = Vec::new();
    let mut user = Vec::new();
    // In the nur home itself its file is the project's.
    if nur_home != cwd {
        let scope = home.unwrap_or(nur_home);
        if let Some(path) = USER_FILES
            .iter()
            .map(|name| nur_home.join(name))
            .find(|p| p.is_file())
        {
            let label = format!("~/.nur/{}", file_name(&path));
            if let Some(section) = read_section(&path, &label, scope, &mut notices) {
                user.push(section);
            }
        }
    }
    let root = crate::fractal::repo_root_of(cwd).unwrap_or_else(|| cwd.to_path_buf());
    let mut project = Vec::new();
    for dir in dirs_from(&root, cwd) {
        for path in dir_files(&dir) {
            let label = relative_label(&root, &path);
            if let Some(section) = read_section(&path, &label, &root, &mut notices) {
                project.push(section);
            }
        }
    }
    let mut loaded = Instructions {
        user,
        project,
        notices,
    };
    apply_budget(&mut loaded, max_chars);
    loaded
}

/// The instruction files of one directory: its base file, then `NUR.md`.
fn dir_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Some(base) = BASE_FILES
        .iter()
        .map(|name| dir.join(name))
        .find(|p| p.is_file())
    {
        files.push(base);
    }
    let layer = dir.join(LAYER_FILE);
    if layer.is_file() {
        files.push(layer);
    }
    files
}

/// `root`, then each directory down to `cwd`. Just `cwd` when it is not
/// under `root`.
fn dirs_from(root: &Path, cwd: &Path) -> Vec<PathBuf> {
    let Ok(rest) = cwd.strip_prefix(root) else {
        return vec![cwd.to_path_buf()];
    };
    let mut dirs = vec![root.to_path_buf()];
    let mut current = root.to_path_buf();
    for component in rest.components() {
        if let Component::Normal(part) = component {
            current.push(part);
            dirs.push(current.clone());
        }
    }
    dirs
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn relative_label(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|rel| rel.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.display().to_string())
}

/// Read one file with its imports expanded and hidden Unicode stripped.
/// `None` for a missing, unreadable or empty file.
fn read_section(
    path: &Path,
    label: &str,
    scope: &Path,
    notices: &mut Vec<String>,
) -> Option<Section> {
    let raw = read_capped(path)?;
    let mut visited = HashSet::new();
    if let Ok(canonical) = path.canonicalize() {
        visited.insert(canonical);
    }
    let expanded = expand_imports(&raw, path, scope, 0, &mut visited, notices);
    let (clean, removed) = strip_hidden(&expanded);
    if removed > 0 {
        notices.push(format!(
            "{label}: removed {removed} hidden character{} (tag or bidi controls)",
            if removed == 1 { "" } else { "s" }
        ));
    }
    let text = clean.trim().to_string();
    (!text.is_empty()).then(|| Section {
        label: label.to_string(),
        path: path.to_path_buf(),
        text,
        overflow: None,
    })
}

fn read_capped(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// Replace `@path` lines (outside code fences) with the files they name.
fn expand_imports(
    text: &str,
    from: &Path,
    scope: &Path,
    depth: usize,
    visited: &mut HashSet<PathBuf>,
    notices: &mut Vec<String>,
) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_fence = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
        }
        let target = trimmed
            .strip_prefix('@')
            .filter(|t| !in_fence && !t.is_empty() && !t.contains(char::is_whitespace));
        let Some(target) = target else {
            out.push_str(line);
            out.push('\n');
            continue;
        };
        match resolve_import(target, from, scope, depth, visited) {
            Ok((path, body)) => {
                let nested = expand_imports(&body, &path, scope, depth + 1, visited, notices);
                out.push_str(nested.trim_end());
                out.push('\n');
            }
            Err(reason) => {
                notices.push(format!(
                    "{}: import @{target} refused ({reason})",
                    from.display()
                ));
                out.push_str(&format!("[import @{target} refused: {reason}]\n"));
            }
        }
    }
    out
}

fn resolve_import(
    target: &str,
    from: &Path,
    scope: &Path,
    depth: usize,
    visited: &mut HashSet<PathBuf>,
) -> Result<(PathBuf, String), &'static str> {
    if depth >= MAX_IMPORT_DEPTH {
        return Err("imports nest too deep");
    }
    let path = if let Some(rest) = target.strip_prefix("~/") {
        crate::config::home_dir()
            .ok_or("no home directory")?
            .join(rest)
    } else {
        let base = from.parent().unwrap_or(Path::new("."));
        base.join(target)
    };
    let canonical = path.canonicalize().map_err(|_| "file not found")?;
    let scope = scope.canonicalize().map_err(|_| "file not found")?;
    let Ok(inside) = canonical.strip_prefix(&scope) else {
        return Err("outside the allowed directory");
    };
    if credential_path(inside) {
        return Err("credential path");
    }
    if !visited.insert(canonical.clone()) {
        return Err("imported twice (cycle)");
    }
    let body = read_capped(&canonical).ok_or("unreadable or over 1 MB")?;
    if crate::tools::sensitive::body_looks_sensitive(&body) {
        return Err("content looks like a credential");
    }
    Ok((canonical, body))
}

/// Paths (relative to the import scope) that hold credentials, whatever a
/// file imports them as.
fn credential_path(path: &Path) -> bool {
    path.components().any(|c| {
        let part = c.as_os_str().to_string_lossy().to_ascii_lowercase();
        matches!(
            part.as_str(),
            ".ssh" | ".aws" | ".gnupg" | ".azure" | ".kube" | ".docker" | "gcloud"
        ) || part == ".env"
            || part.starts_with(".env.")
            || part.contains("credential")
            || part.contains("secret")
            || part.ends_with(".pem")
            || part.ends_with(".key")
    })
}

/// Drop Unicode tag characters and bidi controls, which can hide
/// instructions from a human reviewer while a model still reads them.
fn strip_hidden(text: &str) -> (String, usize) {
    let mut removed = 0;
    let clean = text
        .chars()
        .filter(|c| {
            let hidden = matches!(
                *c as u32,
                0xE0000..=0xE007F | 0x202A..=0x202E | 0x2066..=0x2069 | 0x200E | 0x200F | 0x061C
            );
            removed += hidden as usize;
            !hidden
        })
        .collect();
    (clean, removed)
}

/// Fit the chain in `max_chars`: sections in order, the first that does not
/// fit is cut at a line boundary and every later one is spilled whole.
fn apply_budget(loaded: &mut Instructions, max_chars: usize) {
    let mut remaining = max_chars;
    for section in loaded.user.iter_mut().chain(loaded.project.iter_mut()) {
        let len = section.text.chars().count();
        if len <= remaining {
            remaining -= len;
            continue;
        }
        let full = std::mem::take(&mut section.text);
        let (head, next_line) = head_at_line(&full, remaining);
        let shown = head.chars().count();
        section.text = head;
        section.overflow = Some(Overflow {
            next_line,
            remaining_chars: len - shown,
            full,
        });
        remaining = 0;
    }
}

/// The longest run of whole lines within `max_chars`, and the 1-based line
/// where the rest starts.
fn head_at_line(text: &str, max_chars: usize) -> (String, usize) {
    let mut head = String::new();
    let mut used = 0;
    for (index, line) in text.split_inclusive('\n').enumerate() {
        let n = line.chars().count();
        if used + n > max_chars {
            return (head.trim_end().to_string(), index + 1);
        }
        head.push_str(line);
        used += n;
    }
    (head.trim_end().to_string(), text.lines().count() + 1)
}

/// The prompt text for one section: heading, what fits, and where the rest is.
/// `session_id` lets the overflow ride in the context store; without one (or
/// when the store refuses it) the pointer names the file and line.
pub fn render_section(kind: &str, section: &Section, session_id: &str) -> String {
    let mut out = format!("\n# {kind} instructions ({})\n", section.label);
    if !section.text.is_empty() {
        out.push_str(&section.text);
        out.push('\n');
    }
    if let Some(overflow) = &section.overflow {
        out.push_str(&overflow_pointer(section, overflow, session_id));
        out.push('\n');
    }
    out
}

fn overflow_pointer(section: &Section, overflow: &Overflow, session_id: &str) -> String {
    let var = format!("instructions_{}", section.label.replace(['/', '~'], "_"));
    let stored = !session_id.is_empty()
        && (crate::agent::context_store::get(session_id, &var)
            .is_some_and(|v| v.char_count == overflow.full.chars().count())
            || crate::agent::context_store::register(
                session_id,
                &var,
                &overflow.full,
                "instructions",
                "instructions",
            )
            .is_ok());
    let n = overflow.remaining_chars;
    if stored {
        format!(
            "[{} continues for {n} more characters: read context variable `{var}` (tool \
             `context`, action=peek, from line {}) before editing files]",
            section.label, overflow.next_line
        )
    } else {
        format!(
            "[{} continues for {n} more characters: read {} from line {} before editing files]",
            section.label,
            section.path.display(),
            overflow.next_line
        )
    }
}

/// Per session, the directories whose instructions were already delivered.
fn delivered() -> &'static Mutex<HashMap<String, HashSet<PathBuf>>> {
    static D: OnceLock<Mutex<HashMap<String, HashSet<PathBuf>>>> = OnceLock::new();
    D.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Tools whose `path` argument (or patch headers) point at workspace files.
fn touched_paths(tool: &str, args: &str) -> Vec<String> {
    let Ok(args) = serde_json::from_str::<serde_json::Value>(args) else {
        return Vec::new();
    };
    match tool {
        "read_file" | "edit_file" | "write_file" | "multi_edit" | "hashline_edit" | "list_dir" => {
            args.get("path")
                .and_then(|p| p.as_str())
                .map(|p| vec![p.to_string()])
                .unwrap_or_default()
        }
        "apply_patch" => args
            .get("patch")
            .or_else(|| args.get("input"))
            .and_then(|p| p.as_str())
            .map(|patch| {
                patch
                    .lines()
                    .filter_map(|line| {
                        ["*** Update File: ", "*** Add File: ", "*** Delete File: "]
                            .iter()
                            .find_map(|prefix| line.strip_prefix(prefix))
                    })
                    .map(|p| p.trim().to_string())
                    .collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Instructions from directories below `cwd` that `tool` just touched, not yet
/// delivered this session, formatted to append to its result. `None` when
/// there are none.
pub fn nested_for_tool(
    cwd: &Path,
    session_id: &str,
    tool: &str,
    args: &str,
    max_chars: usize,
) -> Option<String> {
    let mut out = String::new();
    let root = crate::fractal::repo_root_of(cwd).unwrap_or_else(|| cwd.to_path_buf());
    for target in touched_paths(tool, args) {
        let path = cwd.join(&target);
        let dir = if path.is_dir() {
            path
        } else {
            path.parent()?.to_path_buf()
        };
        let Ok(rest) = dir.strip_prefix(cwd) else {
            continue;
        };
        if rest.components().any(|c| {
            SKIPPED_DIRS.contains(&c.as_os_str().to_string_lossy().as_ref())
                || matches!(c, Component::ParentDir)
        }) {
            continue;
        }
        // Directories strictly below cwd, outermost first.
        for dir in dirs_from(cwd, &dir).into_iter().skip(1) {
            let fresh = delivered()
                .lock()
                .map(|mut d| {
                    d.entry(session_id.to_string())
                        .or_default()
                        .insert(dir.clone())
                })
                .unwrap_or(false);
            if !fresh {
                continue;
            }
            for file in dir_files(&dir) {
                let label = relative_label(&root, &file);
                let mut notices = Vec::new();
                let Some(mut section) = read_section(&file, &label, &root, &mut notices) else {
                    continue;
                };
                let mut one = Instructions {
                    user: Vec::new(),
                    project: vec![section.clone()],
                    notices: Vec::new(),
                };
                apply_budget(&mut one, max_chars);
                section = one.project.remove(0);
                out.push_str(&format!(
                    "\n\n[nur instructions: {label}]\n{}",
                    section.text
                ));
                if let Some(overflow) = &section.overflow {
                    out.push('\n');
                    out.push_str(&overflow_pointer(&section, overflow, session_id));
                }
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tree(PathBuf);
    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn tree(files: &[(&str, &str)]) -> Tree {
        let root = std::env::temp_dir().join(format!("nur-instr-{}", uuid::Uuid::new_v4()));
        for (rel, body) in files {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        Tree(root)
    }

    fn labels(loaded: &Instructions) -> Vec<String> {
        loaded.sections().map(|s| s.label.clone()).collect()
    }

    // Failure modes: a subdirectory loses the root file; NUR.md hides
    // AGENTS.md; a long file is cut with no word; an import reaches outside
    // the repo or into credentials; an import cycle loops; hidden Unicode
    // reaches the model; a nested file is delivered twice or for an ignored
    // directory.

    #[test]
    fn the_chain_runs_from_the_repo_root_to_cwd_with_nur_md_layered() {
        let t = tree(&[
            (".git/HEAD", "ref: refs/heads/main\n"),
            ("AGENTS.md", "ROOT-RULES"),
            ("NUR.md", "ROOT-NUR-LAYER"),
            ("pkg/app/CLAUDE.md", "APP-RULES"),
        ]);
        let home = t.0.join("home/.nur");
        let loaded = load_with(&t.0.join("pkg/app"), &home, None, 48_000);
        assert_eq!(
            labels(&loaded),
            ["AGENTS.md", "NUR.md", "pkg/app/CLAUDE.md"]
        );
        assert!(loaded.sections().all(|s| s.overflow.is_none()));
    }

    #[test]
    fn what_does_not_fit_is_spilled_and_pointed_to_never_cut_silently() {
        let body: String = (0..100).map(|i| format!("rule line {i}\n")).collect();
        let t = tree(&[
            (".git/HEAD", "x"),
            ("AGENTS.md", &body),
            ("sub/AGENTS.md", "SECOND"),
        ]);
        let loaded = load_with(&t.0.join("sub"), &t.0.join("home"), None, 300);
        let first = &loaded.project[0];
        let overflow = first.overflow.as_ref().expect("first file overflows");
        assert!(first
            .text
            .ends_with(&format!("rule line {}", overflow.next_line - 2)));
        assert_eq!(overflow.full.trim(), body.trim());
        assert_eq!(loaded.project[1].text, "");
        assert!(loaded.project[1].overflow.is_some());
        let rendered = render_section("Project", first, "");
        assert!(rendered.contains(&format!("from line {}", overflow.next_line)));
    }

    #[test]
    fn imports_stay_inside_the_repo_and_away_from_credentials() {
        let t = tree(&[
            ("repo/.git/HEAD", "x"),
            (
                "repo/AGENTS.md",
                "top\n@docs/build.md\n```\n@not/an/import.md\n```\n@../outside.md\n@self.md\n",
            ),
            ("repo/docs/build.md", "BUILD-RULES\n@../AGENTS.md"),
            ("repo/self.md", "api_key=sk-1234567890abcdefghij"),
            ("outside.md", "OUTSIDE"),
        ]);
        let loaded = load_with(&t.0.join("repo"), &t.0.join("home"), None, 48_000);
        let text = &loaded.project[0].text;
        assert!(text.contains("BUILD-RULES"));
        assert!(
            text.contains("@not/an/import.md"),
            "fenced lines are left alone"
        );
        assert!(!text.contains("OUTSIDE"));
        assert!(!text.contains("sk-1234567890"));
        assert!(text.contains("[import @../AGENTS.md refused: imported twice (cycle)]"));
        assert_eq!(loaded.notices.len(), 3, "{:?}", loaded.notices);
    }

    #[test]
    fn hidden_unicode_is_stripped_and_reported() {
        let t = tree(&[
            (".git/HEAD", "x"),
            ("AGENTS.md", "safe\u{E0041}\u{202E}text"),
        ]);
        let loaded = load_with(&t.0, &t.0.join("home"), None, 48_000);
        assert_eq!(loaded.project[0].text, "safetext");
        assert_eq!(loaded.notices.len(), 1);
    }

    #[test]
    fn nested_instructions_arrive_once_on_the_first_touch() {
        let t = tree(&[
            (".git/HEAD", "x"),
            ("AGENTS.md", "ROOT"),
            ("src/ui/AGENTS.md", "UI-RULES"),
            ("src/ui/view.rs", "fn main() {}"),
            ("node_modules/pkg/AGENTS.md", "IGNORED"),
        ]);
        let sid = format!("test-{}", uuid::Uuid::new_v4());
        let args = r#"{"path":"src/ui/view.rs"}"#;
        let first = nested_for_tool(&t.0, &sid, "read_file", args, 48_000).unwrap();
        assert!(first.contains("[nur instructions: src/ui/AGENTS.md]\nUI-RULES"));
        assert_eq!(nested_for_tool(&t.0, &sid, "edit_file", args, 48_000), None);
        let ignored = r#"{"path":"node_modules/pkg/index.js"}"#;
        assert_eq!(
            nested_for_tool(&t.0, &sid, "read_file", ignored, 48_000),
            None
        );
        delivered().lock().unwrap().remove(&sid);
    }
}

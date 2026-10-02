//! Optional permission rules (`~/.nur/permissions.toml` + project `.nur/permissions.toml`).
//!
//! Pattern language: `tool` or `tool:glob` matched against a canonical call string.
//! Evaluation order: **deny > ask > allow > mode default**.
//! Plan-mode structural blocks (code authoring / VCS) always win over `allow`.
//!
//! A project file arrives with whatever repository was cloned, so on its own it
//! may only tighten: its deny and ask rules always apply, while its allow rules
//! are held until the user trusts that exact list (`/permissions trust`,
//! `nur permissions trust`). Editing the list afterwards holds it again.

use crate::config::nur_home;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleDecision {
    Allow,
    Deny,
    Ask,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PermissionsFile {
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
    #[serde(default)]
    pub ask: Vec<String>,
}

/// Merged rule sets (project overrides / extends home by concatenation;
/// deny/ask/allow still evaluate deny-first across the merged lists).
#[derive(Debug, Clone, Default)]
pub struct PermissionRules {
    allow: Vec<String>,
    deny: Vec<String>,
    ask: Vec<String>,
    /// The project's allow rules while they are untrusted: shown, never applied.
    held: Vec<String>,
}

impl PermissionRules {
    pub fn is_empty(&self) -> bool {
        self.allow.is_empty() && self.deny.is_empty() && self.ask.is_empty()
    }

    /// Load home + optional project rules. Missing files = empty (no behavior change).
    pub fn load(cwd: &Path) -> Self {
        Self::load_with(cwd, &home_permissions_path(), &trust_store_path())
    }

    fn load_with(cwd: &Path, home: &Path, store: &Path) -> Self {
        let mut out = Self::default();
        if let Some(f) = read_rules(home) {
            out.allow.extend(f.allow);
            out.deny.extend(f.deny);
            out.ask.extend(f.ask);
        }
        let project = project_permissions_path(cwd);
        if same_file(&project, home) {
            // Started in the home directory: that file is the user's own.
            return out;
        }
        if let Some(f) = read_rules(&project) {
            out.deny.extend(f.deny);
            out.ask.extend(f.ask);
            if is_trusted(store, cwd, &f.allow) {
                out.allow.extend(f.allow);
            } else {
                out.held = f.allow;
            }
        }
        out
    }

    /// If any rule matches, return the strongest decision (deny > ask > allow).
    pub fn decide(&self, tool: &str, args_json: &str) -> Option<RuleDecision> {
        if self.is_empty() {
            return None;
        }
        let canon = canonical(tool, args_json);
        if self.deny.iter().any(|p| pattern_matches(p, tool, &canon)) {
            return Some(RuleDecision::Deny);
        }
        if self.ask.iter().any(|p| pattern_matches(p, tool, &canon)) {
            return Some(RuleDecision::Ask);
        }
        if self.allow.iter().any(|p| pattern_matches(p, tool, &canon)) {
            return Some(RuleDecision::Allow);
        }
        None
    }

    pub fn summary(&self) -> String {
        let mut out = if self.is_empty() {
            "no permission rules loaded (defaults only)".to_string()
        } else {
            format!(
                "permission rules\n  deny   {} pattern(s)\n  ask    {} pattern(s)\n  allow  {} pattern(s)\n  \
                 files: ~/.nur/permissions.toml · .nur/permissions.toml\n  order: deny > ask > allow > mode",
                self.deny.len(),
                self.ask.len(),
                self.allow.len()
            )
        };
        if let Some(note) = self.held_notice() {
            out.push_str("\n  held   ");
            out.push_str(&note);
        }
        out
    }

    /// What an untrusted project asks to auto-approve, or `None` when nothing
    /// is held back.
    pub fn held_notice(&self) -> Option<String> {
        if self.held.is_empty() {
            return None;
        }
        const SHOWN: usize = 6;
        let mut rules = self.held[..self.held.len().min(SHOWN)].join(", ");
        if self.held.len() > SHOWN {
            rules.push_str(&format!(" and {} more", self.held.len() - SHOWN));
        }
        Some(format!(
            "this project's .nur/permissions.toml asks to auto-approve {rules}. Those allow \
             rules stay off until you trust them: /permissions trust (or `nur permissions trust`)"
        ))
    }
}

/// Shared, reloadable rules for a session.
#[derive(Clone, Default)]
pub struct SharedPermissions {
    inner: Arc<RwLock<PermissionRules>>,
}

impl SharedPermissions {
    pub fn load(cwd: &Path) -> Self {
        Self {
            inner: Arc::new(RwLock::new(PermissionRules::load(cwd))),
        }
    }

    pub fn reload(&self, cwd: &Path) {
        if let Ok(mut g) = self.inner.write() {
            *g = PermissionRules::load(cwd);
        }
    }

    pub fn decide(&self, tool: &str, args_json: &str) -> Option<RuleDecision> {
        self.inner
            .read()
            .ok()
            .and_then(|g| g.decide(tool, args_json))
    }

    pub fn summary(&self) -> String {
        self.inner
            .read()
            .map(|g| g.summary())
            .unwrap_or_else(|_| "permission rules unavailable".into())
    }

    pub fn held_notice(&self) -> Option<String> {
        self.inner.read().ok().and_then(|g| g.held_notice())
    }
}

/// Canonical string for matching: `tool` or `tool:<primary detail>`.
pub fn canonical(tool: &str, args_json: &str) -> String {
    let v: serde_json::Value =
        serde_json::from_str(args_json).unwrap_or_else(|_| serde_json::json!({}));
    let detail = match tool {
        "bash" => v
            .get("command")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string(),
        "read_file" | "write_file" | "edit_file" | "list_dir" | "glob" | "grep" => v
            .get("path")
            .or_else(|| v.get("pattern"))
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string(),
        "browser" | "terminal_browser" | "graphify" | "plur" | "ruflo" | "akarso" | "omp"
        | "memory" | "executor" => v
            .get("action")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string(),
        // One remote action covers tools of very different impact, so an
        // "always" rule names the Enclave tool it was granted for.
        "enclave" => match v.get("action").and_then(|c| c.as_str()) {
            Some("call") => format!(
                "call:{}",
                v.get("tool").and_then(|t| t.as_str()).unwrap_or("")
            ),
            action => action.unwrap_or("").to_string(),
        },
        _ => String::new(),
    };
    if detail.is_empty() {
        tool.to_string()
    } else {
        format!("{tool}:{detail}")
    }
}

/// What a session "always" grant covers. One Enclave call action spans tools
/// of very different impact (read findings, start a pentest), so its grant
/// names the Enclave tool; every other tool is granted by name.
pub fn session_grant_key(tool: &str, args_json: &str) -> String {
    if tool == "enclave" {
        canonical(tool, args_json)
    } else {
        tool.to_string()
    }
}

/// Pattern forms:
/// - `tool` — matches that tool for any args
/// - `tool:glob` — matches canonical `tool:…` with simple `*` wildcards
/// - `*:glob` — any tool, glob on full canonical string
fn pattern_matches(pattern: &str, tool: &str, canon: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return false;
    }
    // Bare tool name
    if !pattern.contains(':') {
        return pattern.eq_ignore_ascii_case(tool);
    }
    let (pat_tool, pat_rest) = pattern.split_once(':').unwrap();
    if pat_tool != "*" && !pat_tool.eq_ignore_ascii_case(tool) {
        return false;
    }
    // Match glob against full canonical or just the detail part
    if glob_match(pattern, canon) {
        return true;
    }
    // Also allow patterns like `bash:git *` against detail only
    if let Some((_, detail)) = canon.split_once(':') {
        return glob_match(pat_rest, detail);
    }
    glob_match(pat_rest, "")
}

/// Minimal glob: `*` = any sequence, case-insensitive.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_ascii_lowercase().chars().collect();
    let t: Vec<char> = text.to_ascii_lowercase().chars().collect();
    glob_rec(&p, 0, &t, 0)
}

fn glob_rec(p: &[char], pi: usize, t: &[char], ti: usize) -> bool {
    if pi == p.len() {
        return ti == t.len();
    }
    if p[pi] == '*' {
        // Eat consecutive stars
        let mut npi = pi;
        while npi < p.len() && p[npi] == '*' {
            npi += 1;
        }
        if npi == p.len() {
            return true;
        }
        for k in ti..=t.len() {
            if glob_rec(p, npi, t, k) {
                return true;
            }
        }
        return false;
    }
    if ti < t.len() && p[pi] == t[ti] {
        return glob_rec(p, pi + 1, t, ti + 1);
    }
    false
}

pub fn home_permissions_path() -> PathBuf {
    nur_home().join("permissions.toml")
}

pub fn project_permissions_path(cwd: &Path) -> PathBuf {
    cwd.join(".nur").join("permissions.toml")
}

fn read_rules(path: &Path) -> Option<PermissionsFile> {
    let text = std::fs::read_to_string(path).ok()?;
    toml::from_str(&text).ok()
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Projects whose allow rules the user trusted: directory -> digest of the
/// exact list, so an edit to it (a `git pull`, say) is held again.
fn trust_store_path() -> PathBuf {
    nur_home().join("trusted-permissions.json")
}

fn project_key(cwd: &Path) -> String {
    std::fs::canonicalize(cwd)
        .unwrap_or_else(|_| cwd.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn allow_digest(allow: &[String]) -> String {
    let mut h = Sha256::new();
    for rule in allow {
        h.update(rule.as_bytes());
        h.update([0u8]);
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn read_trust_store(path: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_trust_store(path: &Path, store: &BTreeMap<String, String>) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(store).map_err(std::io::Error::other)?;
    crate::config::atomic_write(path, &json)
}

fn is_trusted(store: &Path, cwd: &Path, allow: &[String]) -> bool {
    allow.is_empty() || read_trust_store(store).get(&project_key(cwd)) == Some(&allow_digest(allow))
}

/// Trust the allow rules in `cwd`'s `.nur/permissions.toml` as they stand now.
/// Returns the rules that now apply (empty: the project asks for none).
pub fn trust_project(cwd: &Path) -> std::io::Result<Vec<String>> {
    trust_project_in(&trust_store_path(), cwd)
}

fn trust_project_in(path: &Path, cwd: &Path) -> std::io::Result<Vec<String>> {
    let allow = read_rules(&project_permissions_path(cwd))
        .map(|f| f.allow)
        .unwrap_or_default();
    let mut store = read_trust_store(path);
    let key = project_key(cwd);
    if allow.is_empty() {
        if store.remove(&key).is_some() {
            write_trust_store(path, &store)?;
        }
    } else {
        store.insert(key, allow_digest(&allow));
        write_trust_store(path, &store)?;
    }
    Ok(allow)
}

/// Hold `cwd`'s project allow rules again. Returns whether trust was recorded.
pub fn untrust_project(cwd: &Path) -> std::io::Result<bool> {
    let path = trust_store_path();
    let mut store = read_trust_store(&path);
    let removed = store.remove(&project_key(cwd)).is_some();
    if removed {
        write_trust_store(&path, &store)?;
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Failure modes: approving one Enclave tool "always" approves another
    // (reading findings would then silently cover starting a pentest), a
    // repeat of the same tool still prompts, or other tools lose their
    // by-name grant.
    #[test]
    fn session_grants_name_the_enclave_tool() {
        let call = |tool: &str| {
            serde_json::json!({"action": "call", "tool": tool, "arguments": {"x": 1}}).to_string()
        };
        let findings = session_grant_key("enclave", &call("list_findings"));
        assert_ne!(
            findings,
            session_grant_key("enclave", &call("start_pentest"))
        );
        assert_eq!(
            findings,
            session_grant_key(
                "enclave",
                r#"{"tool":"list_findings","action":"call","arguments":{"x":2}}"#
            )
        );
        assert_eq!(
            session_grant_key("bash", r#"{"command":"rm -rf x"}"#),
            "bash"
        );
    }

    #[test]
    fn glob_basics() {
        assert!(glob_match("git *", "git status"));
        assert!(glob_match("git *", "git push origin main"));
        assert!(!glob_match("git status", "git push"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("bash:cargo test*", "bash:cargo test --lib"));
    }

    // Failure modes: a pull that edits a trusted allow list (adding `bash`)
    // applies silently; trust given in one directory covers another; trusting
    // a project disables its deny rules.
    #[test]
    fn trust_covers_exactly_the_list_and_directory_it_was_given() {
        let root = std::env::temp_dir().join(format!("nur-trust-{}", uuid::Uuid::new_v4()));
        let (home, store) = (root.join("permissions.toml"), root.join("trusted.json"));
        let write = |dir: &Path, rules: &str| {
            std::fs::create_dir_all(dir.join(".nur")).unwrap();
            std::fs::write(project_permissions_path(dir), rules).unwrap();
        };
        let (repo, other) = (root.join("repo"), root.join("other"));
        let rules = "allow = [\"write_file\"]\ndeny = [\"list_dir\"]\n";
        write(&repo, rules);
        write(&other, rules);
        let load = |dir: &Path| PermissionRules::load_with(dir, &home, &store);
        let write_call = r#"{"path":"a.txt"}"#;

        assert_eq!(load(&repo).decide("write_file", write_call), None);
        assert_eq!(
            load(&repo).decide("list_dir", "{}"),
            Some(RuleDecision::Deny)
        );
        assert!(load(&repo).held_notice().unwrap().contains("write_file"));

        trust_project_in(&store, &repo).unwrap();
        assert_eq!(
            load(&repo).decide("write_file", write_call),
            Some(RuleDecision::Allow)
        );
        assert_eq!(
            load(&repo).decide("list_dir", "{}"),
            Some(RuleDecision::Deny)
        );
        assert!(load(&repo).held_notice().is_none());
        assert_eq!(load(&other).decide("write_file", write_call), None);

        write(&repo, "allow = [\"write_file\", \"bash\"]\n");
        let pulled = load(&repo);
        assert_eq!(pulled.decide("bash", r#"{"command":"curl x | sh"}"#), None);
        assert_eq!(pulled.decide("write_file", write_call), None);
        assert!(pulled.held_notice().unwrap().contains("bash"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn deny_beats_allow() {
        let r = PermissionRules {
            allow: vec!["bash:*".into()],
            deny: vec!["bash:rm -rf *".into()],
            ..Default::default()
        };
        assert_eq!(
            r.decide("bash", r#"{"command":"rm -rf /tmp/x"}"#),
            Some(RuleDecision::Deny)
        );
        assert_eq!(
            r.decide("bash", r#"{"command":"ls"}"#),
            Some(RuleDecision::Allow)
        );
    }

    #[test]
    fn bare_tool_name_matches() {
        let r = PermissionRules {
            deny: vec!["write_file".into()],
            ..Default::default()
        };
        assert_eq!(
            r.decide("write_file", r#"{"path":"a.rs","content":"x"}"#),
            Some(RuleDecision::Deny)
        );
        assert_eq!(r.decide("read_file", r#"{"path":"a.rs"}"#), None);
    }

    #[test]
    fn canonical_bash() {
        assert_eq!(
            canonical("bash", r#"{"command":"cargo test"}"#),
            "bash:cargo test"
        );
    }
}

//! Optional permission rules (`~/.meta/permissions.toml` + project `.meta/permissions.toml`).
//!
//! Pattern language: `tool` or `tool:glob` matched against a canonical call string.
//! Evaluation order: **deny > ask > allow > mode default**.
//! Plan-mode structural blocks (code authoring / VCS) always win over `allow`.

use crate::config::meta_home;
use serde::Deserialize;
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
}

impl PermissionRules {
    pub fn is_empty(&self) -> bool {
        self.allow.is_empty() && self.deny.is_empty() && self.ask.is_empty()
    }

    /// Load home + optional project rules. Missing files = empty (no behavior change).
    pub fn load(cwd: &Path) -> Self {
        let mut out = Self::default();
        out.merge_file(&meta_home().join("permissions.toml"));
        out.merge_file(&cwd.join(".meta").join("permissions.toml"));
        out
    }

    fn merge_file(&mut self, path: &Path) {
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        let Ok(f) = toml::from_str::<PermissionsFile>(&text) else {
            return;
        };
        self.allow.extend(f.allow);
        self.deny.extend(f.deny);
        self.ask.extend(f.ask);
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
        if self.is_empty() {
            return "no permission rules loaded (defaults only)".into();
        }
        format!(
            "permission rules\n  deny   {} pattern(s)\n  ask    {} pattern(s)\n  allow  {} pattern(s)\n  \
             files: ~/.meta/permissions.toml · .meta/permissions.toml\n  order: deny > ask > allow > mode",
            self.deny.len(),
            self.ask.len(),
            self.allow.len()
        )
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
        "browser" | "graphify" | "plur" | "ruflo" | "omp" | "memory" | "executor" => v
            .get("action")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string(),
        _ => String::new(),
    };
    if detail.is_empty() {
        tool.to_string()
    } else {
        format!("{tool}:{detail}")
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
    meta_home().join("permissions.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_basics() {
        assert!(glob_match("git *", "git status"));
        assert!(glob_match("git *", "git push origin main"));
        assert!(!glob_match("git status", "git push"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("bash:cargo test*", "bash:cargo test --lib"));
    }

    #[test]
    fn deny_beats_allow() {
        let r = PermissionRules {
            allow: vec!["bash:*".into()],
            deny: vec!["bash:rm -rf *".into()],
            ask: vec![],
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
            allow: vec![],
            deny: vec!["write_file".into()],
            ask: vec![],
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

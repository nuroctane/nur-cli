//! Pinned versions and commits for everything the ecosystem installs.
//!
//! `lock.json` is resolved once per nur release (`.nur/resolve_lock.py`) and
//! embedded in the binary. Every `ensure_*` component installs the pinned
//! version instead of `@latest` / HEAD, so a compromised or broken upstream
//! release never auto-installs on the next launch. Bumping a pin is a
//! deliberate repo change, reviewed like any other dependency bump.

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Debug, Clone, Deserialize)]
pub struct GitPin {
    /// `owner/repo` for packs, full clone url for plugins.
    pub repo: String,
    pub commit: String,
}

#[derive(Debug, Deserialize)]
struct Lock {
    schema: u32,
    resolved_at: String,
    npm: HashMap<String, String>,
    pypi: HashMap<String, String>,
    packs: HashMap<String, GitPin>,
    plugins: HashMap<String, GitPin>,
    git: HashMap<String, GitPin>,
}

fn lock() -> &'static Lock {
    static LOCK: OnceLock<Lock> = OnceLock::new();
    LOCK.get_or_init(|| {
        let parsed: Lock = serde_json::from_str(include_str!("lock.json"))
            .expect("ecosystem lock.json must parse");
        assert_eq!(parsed.schema, 1, "unsupported ecosystem lock schema");
        parsed
    })
}

/// Release the lock was resolved for (reporting only).
pub fn resolved_at() -> &'static str {
    &lock().resolved_at
}

/// `npm install -g` argument for a pinned package: `name@version`. A package
/// the lock does not know fails visibly (`name@MISSING-PIN`) instead of
/// silently tracking `@latest` - add the pin, do not bypass it.
pub fn npm_spec(package: &str) -> String {
    match lock().npm.get(package) {
        Some(v) => format!("{package}@{v}"),
        None => format!("{package}@MISSING-PIN"),
    }
}

/// `uv tool install` argument: `name==version` (same MISSING-PIN contract).
pub fn pypi_spec(package: &str) -> String {
    match lock().pypi.get(package) {
        Some(v) => format!("{package}=={v}"),
        None => format!("{package}==MISSING-PIN"),
    }
}

/// Pinned commit for a curated skill pack (keyed by its SKILL_PACKS label).
pub fn pack_pin(label: &str) -> Option<GitPin> {
    lock().packs.get(label).cloned()
}

/// Pinned commit for a default plugin (keyed by catalog id).
pub fn plugin_pin(id: &str) -> Option<GitPin> {
    lock().plugins.get(id).cloned()
}

/// Pinned commit for a `cargo install --git` component (e.g. dogwood).
pub fn git_pin(name: &str) -> Option<GitPin> {
    lock().git.get(name).cloned()
}

/// Every label the lock must cover, so a forgotten pin is a test failure,
/// not a silent `@latest`.
#[cfg(test)]
pub const LOCKED_NPM: &[&str] = &[
    "skills",
    "akm-cli",
    "executor",
    "graphjin",
    "@oh-my-pi/pi-coding-agent",
    "@sleepinsummer/agent-browser-cli",
    "penecho",
    "excalidraw-cli",
    "egaki",
    "akarso",
    "@plur-ai/cli",
    "@plur-ai/mcp",
    "ruflo",
];

#[cfg(test)]
pub const LOCKED_PYPI: &[&str] = &["headroom-ai", "plasma-fractal", "graphifyy"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_covers_every_component_with_sane_values() {
        // Failure modes: a component silently falls back to @latest because
        // its pin was never recorded; a placeholder pin ships to users.
        for pkg in LOCKED_NPM {
            let spec = npm_spec(pkg);
            assert!(
                spec.starts_with(&format!("{pkg}@")) && !spec.contains("MISSING-PIN"),
                "npm pin missing for {pkg}: {spec}"
            );
            let v = spec.rsplit('@').next().unwrap();
            assert!(
                v.chars().next().is_some_and(|c| c.is_ascii_digit()),
                "npm pin for {pkg} is not a version: {v}"
            );
        }
        for pkg in LOCKED_PYPI {
            let spec = pypi_spec(pkg);
            assert!(
                !spec.contains("MISSING-PIN"),
                "pypi pin missing for {pkg}: {spec}"
            );
        }
        for (label, pin) in &lock().packs {
            assert_eq!(pin.commit.len(), 40, "pack {label} commit not a sha");
            assert!(pin.repo.contains('/'), "pack {label} repo malformed");
        }
        for (id, pin) in &lock().plugins {
            assert_eq!(pin.commit.len(), 40, "plugin {id} commit not a sha");
        }
    }
}

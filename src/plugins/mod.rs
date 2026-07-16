//! NurCLI plugin marketplace.
//!
//! Plugins install under `~/.nur/plugins/<id>/` (git clone) and are tracked in
//! `~/.nur/plugins/registry.json`. Enabled plugins contribute `SKILL.md` trees
//! that the agent discovers on the next turn (see [`crate::agent::skills`]).

mod catalog;
mod registry;

pub use catalog::{by_id, catalog};
pub use registry::{
    install_plugin, is_enabled, is_installed, list_installed, plugins_home, set_enabled,
    uninstall_plugin, Registry,
};

use catalog::PluginEntry as Entry;

/// One row for the `/plugins` picker (catalog + live install state).
#[derive(Debug, Clone)]
pub struct PluginRow {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    #[allow(dead_code)]
    pub source: String,
    pub installed: bool,
    pub enabled: bool,
}

impl PluginRow {
    pub fn status_badge(&self) -> &'static str {
        match (self.installed, self.enabled) {
            (true, true) => "● installed",
            (true, false) => "○ disabled",
            (false, _) => "· available",
        }
    }

    pub fn action_hint(&self) -> &'static str {
        match (self.installed, self.enabled) {
            (false, _) => "↵ install",
            (true, true) => "↵ disable",
            (true, false) => "↵ enable",
        }
    }
}

/// Build picker rows: full catalog with install/enable state from disk.
pub fn marketplace_rows() -> Vec<PluginRow> {
    let reg = Registry::load();
    catalog()
        .iter()
        .map(|p| row_for(p, &reg))
        .collect()
}

fn row_for(p: &Entry, reg: &Registry) -> PluginRow {
    let _ = reg; // registry is still loaded by marketplace_rows for batch consistency
    let installed = is_installed(p.id);
    let enabled = installed && is_enabled(p.id);
    PluginRow {
        id: p.id.to_string(),
        name: p.name.to_string(),
        description: p.description.to_string(),
        category: p.category.to_string(),
        source: p.source_url.to_string(),
        installed,
        enabled,
    }
}

/// Paths of enabled plugins that should feed skill discovery.
pub fn enabled_skill_roots() -> Vec<std::path::PathBuf> {
    let home = plugins_home();
    let reg = Registry::load();
    let mut out = Vec::new();
    if !home.is_dir() {
        return out;
    }
    let Ok(entries) = std::fs::read_dir(&home) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') || name == "cache" {
            continue;
        }
        // Skip if explicitly disabled in registry.
        if let Some(rec) = reg.plugins.get(name) {
            if !rec.enabled {
                continue;
            }
        }
        // Prefer nested skills/ directory; also allow plugin root as skill root.
        let skills = path.join("skills");
        if skills.is_dir() {
            out.push(skills);
        }
        out.push(path);
    }
    out
}

/// One-line marketplace status for `/doctor` / notes.
pub fn quick_status() -> String {
    let rows = marketplace_rows();
    let on_disk = list_installed();
    let installed = on_disk.len().max(rows.iter().filter(|r| r.installed).count());
    let enabled = on_disk
        .iter()
        .filter(|p| p.enabled)
        .count()
        .max(rows.iter().filter(|r| r.enabled).count());
    format!(
        "plugins  {enabled} enabled · {installed} installed · {} in catalog  (~/.nur/plugins)\n  /plugins  open marketplace picker",
        rows.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_core_plugins() {
        let ids: Vec<_> = catalog().iter().map(|p| p.id).collect();
        assert!(ids.contains(&"superpowers"));
        assert!(ids.contains(&"vercel"));
        assert!(ids.contains(&"firecrawl"));
        assert!(ids.contains(&"fable"));
        assert!(ids.len() >= 8);
    }

    #[test]
    fn by_id_works() {
        assert!(by_id("superpowers").is_some());
        assert!(by_id("nope-not-a-plugin").is_none());
    }
}

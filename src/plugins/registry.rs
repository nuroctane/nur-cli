//! On-disk plugin registry + git install into `~/.nur/plugins/`.

use super::catalog::{by_id, PluginEntry};
use crate::config::nur_home;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPlugin {
    pub id: String,
    pub source: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub installed_at: String,
    #[serde(default)]
    pub path: String,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Registry {
    #[serde(default)]
    pub plugins: BTreeMap<String, InstalledPlugin>,
}

impl Registry {
    pub fn load() -> Self {
        let path = registry_path();
        let Ok(raw) = fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_json::from_str(&raw).unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = registry_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let raw = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&path, raw).map_err(|e| e.to_string())
    }
}

pub fn plugins_home() -> PathBuf {
    nur_home().join("plugins")
}

fn registry_path() -> PathBuf {
    plugins_home().join("registry.json")
}

pub fn is_installed(id: &str) -> bool {
    plugins_home().join(id).is_dir() || Registry::load().plugins.contains_key(id)
}

pub fn is_enabled(id: &str) -> bool {
    Registry::load()
        .plugins
        .get(id)
        .map(|p| p.enabled)
        .unwrap_or_else(|| plugins_home().join(id).is_dir())
}

pub fn list_installed() -> Vec<InstalledPlugin> {
    let mut v: Vec<_> = Registry::load().plugins.values().cloned().collect();
    // Also surface bare plugin dirs that never got a registry row.
    if let Ok(rd) = fs::read_dir(plugins_home()) {
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            if !ft.is_dir() {
                continue;
            }
            let id = e.file_name().to_string_lossy().to_string();
            if id.starts_with('.') {
                continue;
            }
            if v.iter().any(|p| p.id == id) {
                continue;
            }
            v.push(InstalledPlugin {
                id: id.clone(),
                source: String::new(),
                enabled: true,
                installed_at: String::new(),
                path: e.path().display().to_string(),
            });
        }
    }
    v.sort_by(|a, b| a.id.cmp(&b.id));
    v
}

pub fn set_enabled(id: &str, enabled: bool) -> Result<(), String> {
    let id = id.trim();
    let mut reg = Registry::load();
    if let Some(p) = reg.plugins.get_mut(id) {
        p.enabled = enabled;
        reg.save()?;
        return Ok(());
    }
    // Bare directory without registry row — create a record.
    let dir = plugins_home().join(id);
    if !dir.is_dir() {
        return Err(format!("plugin '{id}' is not installed"));
    }
    let source = by_id(id)
        .map(|p| p.source_url.to_string())
        .unwrap_or_default();
    reg.plugins.insert(
        id.to_string(),
        InstalledPlugin {
            id: id.to_string(),
            source,
            enabled,
            installed_at: chrono::Utc::now().to_rfc3339(),
            path: dir.display().to_string(),
        },
    );
    reg.save()
}

/// Install (or update) a catalog plugin by id. Clones into `~/.nur/plugins/<id>`.
pub fn install_plugin(id: &str) -> Result<String, String> {
    let entry = by_id(id).ok_or_else(|| {
        format!("unknown plugin '{id}' — open /plugins to browse the catalog")
    })?;
    install_entry(entry)
}

pub fn install_entry(entry: &PluginEntry) -> Result<String, String> {
    let home = plugins_home();
    fs::create_dir_all(&home).map_err(|e| format!("create plugins home: {e}"))?;

    // Ensure git is available.
    let git_ok = Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !git_ok {
        return Err("git not found on PATH — install Git to use the plugin marketplace".into());
    }

    let dest = home.join(entry.id);
    if dest.is_dir() {
        // Update existing checkout.
        let pull = Command::new("git")
            .args(["-C", &dest.display().to_string(), "pull", "--ff-only"])
            .output();
        match pull {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                // Non-fatal: keep existing tree if pull fails (dirty / no remote).
                tracing::warn!("plugin pull {}: {err}", entry.id);
            }
            Err(e) => tracing::warn!("plugin pull {}: {e}", entry.id),
        }
    } else {
        // Shallow clone into a temp dir then rename (atomic-ish).
        let tmp = home.join(format!(".tmp-{}", entry.id));
        let _ = fs::remove_dir_all(&tmp);
        let out = Command::new("git")
            .args([
                "clone",
                "--depth",
                "1",
                entry.source_url,
                &tmp.display().to_string(),
            ])
            .output()
            .map_err(|e| format!("git clone failed to start: {e}"))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let _ = fs::remove_dir_all(&tmp);
            return Err(format!("git clone failed: {err}"));
        }

        // Optional path_in_repo: promote that subdirectory to the plugin root.
        let final_src = if let Some(sub) = entry.path_in_repo {
            let sub_path = tmp.join(sub);
            if !sub_path.is_dir() {
                let _ = fs::remove_dir_all(&tmp);
                return Err(format!(
                    "plugin {} missing path_in_repo '{sub}' after clone",
                    entry.id
                ));
            }
            // Move sub to dest via temp promote.
            let promote = home.join(format!(".promote-{}", entry.id));
            let _ = fs::remove_dir_all(&promote);
            fs::rename(&sub_path, &promote).map_err(|e| e.to_string())?;
            let _ = fs::remove_dir_all(&tmp);
            promote
        } else {
            tmp
        };

        if dest.exists() {
            let _ = fs::remove_dir_all(&dest);
        }
        fs::rename(&final_src, &dest).map_err(|e| format!("install move failed: {e}"))?;
    }

    // Count skills for the success message.
    let skill_n = count_skills(&dest);

    let mut reg = Registry::load();
    reg.plugins.insert(
        entry.id.to_string(),
        InstalledPlugin {
            id: entry.id.to_string(),
            source: entry.source_url.to_string(),
            enabled: true,
            installed_at: chrono::Utc::now().to_rfc3339(),
            path: dest.display().to_string(),
        },
    );
    reg.save()?;

    // Mirror skills into ~/.nur/skills/<name> for maximum discovery compatibility.
    mirror_skills_to_nur_home(&dest)?;

    Ok(format!(
        "installed {} → {} ({} skill packs, enabled)",
        entry.name,
        dest.display(),
        skill_n
    ))
}

pub fn uninstall_plugin(id: &str) -> Result<String, String> {
    let id = id.trim();
    let dir = plugins_home().join(id);
    if dir.is_dir() {
        fs::remove_dir_all(&dir).map_err(|e| format!("remove plugin dir: {e}"))?;
    }
    let mut reg = Registry::load();
    reg.plugins.remove(id);
    reg.save()?;
    Ok(format!("uninstalled plugin '{id}'"))
}

fn count_skills(root: &Path) -> usize {
    let mut n = 0;
    // skills/*/SKILL.md
    let skills = root.join("skills");
    if skills.is_dir() {
        if let Ok(rd) = fs::read_dir(&skills) {
            for e in rd.flatten() {
                if e.path().join("SKILL.md").is_file() {
                    n += 1;
                }
            }
        }
    }
    // root-level */SKILL.md (plugin packs that put skills at top level)
    if let Ok(rd) = fs::read_dir(root) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() && p.join("SKILL.md").is_file() {
                n += 1;
            }
        }
    }
    // single SKILL.md at root
    if root.join("SKILL.md").is_file() {
        n += 1;
    }
    n
}

/// Copy/symlink discovered skill dirs into `~/.nur/skills/` so existing
/// discovery keeps working even without plugins path (belt + suspenders).
fn mirror_skills_to_nur_home(plugin_root: &Path) -> Result<(), String> {
    let dest_root = nur_home().join("skills");
    fs::create_dir_all(&dest_root).map_err(|e| e.to_string())?;

    let mut sources: Vec<PathBuf> = Vec::new();
    let nested = plugin_root.join("skills");
    if nested.is_dir() {
        if let Ok(rd) = fs::read_dir(&nested) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() && p.join("SKILL.md").is_file() {
                    sources.push(p);
                }
            }
        }
    }
    if let Ok(rd) = fs::read_dir(plugin_root) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() && p.join("SKILL.md").is_file() {
                // Avoid double-counting nested skills/ parent as a skill.
                if p.file_name().and_then(|n| n.to_str()) == Some("skills") {
                    continue;
                }
                sources.push(p);
            }
        }
    }
    if plugin_root.join("SKILL.md").is_file() {
        // Single-skill plugin: use plugin folder name.
        sources.push(plugin_root.to_path_buf());
    }

    for src in sources {
        let name = src
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("skill");
        let dest = dest_root.join(name);
        // Full tree (SKILL.md + references/ etc.) so multi-file skills stay complete.
        let _ = fs::remove_dir_all(&dest);
        copy_dir_recursive(&src, &dest)?;
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let rd = fs::read_dir(src).map_err(|e| e.to_string())?;
    for e in rd.flatten() {
        let from = e.path();
        let to = dest.join(e.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if from.is_file() {
            fs::copy(&from, &to).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

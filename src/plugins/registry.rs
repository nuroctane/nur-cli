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
    /// The commit the clone was at when installed or last updated, so two
    /// machines can tell whether they run the same plugin code.
    #[serde(default)]
    pub commit: String,
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
                commit: String::new(),
            });
        }
    }
    v.sort_by(|a, b| a.id.cmp(&b.id));
    v
}

pub fn set_enabled(id: &str, enabled: bool) -> Result<(), String> {
    let id = checked_id(id)?;
    let mut reg = Registry::load();
    if let Some(p) = reg.plugins.get_mut(id) {
        p.enabled = enabled;
        let (source, commit) = (p.source.clone(), p.commit.clone());
        reg.save()?;
        sync_mirror(id, enabled, &source, &commit)?;
        crate::agent::skill_cache::invalidate_cache();
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
            commit: head_commit(&dir),
        },
    );
    reg.save()?;
    let source = reg.plugins[id].source.clone();
    let commit = reg.plugins[id].commit.clone();
    sync_mirror(id, enabled, &source, &commit)?;
    crate::agent::skill_cache::invalidate_cache();
    Ok(())
}

/// Mirror an enabled plugin's skills into `~/.nur/skills`, or take a
/// disabled one's mirrored copies back out (they are a global skill root, so
/// leaving them would keep a disabled plugin's skills live).
fn sync_mirror(id: &str, enabled: bool, source: &str, commit: &str) -> Result<(), String> {
    let dir = plugins_home().join(id);
    if enabled {
        if dir.is_dir() {
            mirror_skills_to_nur_home(id, source, commit, &dir)?;
        }
    } else {
        unmirror_skills(id, dir.is_dir().then_some(dir.as_path()));
    }
    Ok(())
}

/// A plugin id names one directory under the plugins home; anything that
/// could reach outside it (`..`, a separator) is refused.
fn checked_id(id: &str) -> Result<&str, String> {
    let id = id.trim();
    let plain = !id.is_empty() && !id.starts_with('.') && !id.contains(['/', '\\', ':']);
    if plain {
        Ok(id)
    } else {
        Err(format!("'{id}' is not a plugin id"))
    }
}

/// `HEAD` of a plugin checkout, or empty when it is not a git checkout
/// (a `path_in_repo` plugin keeps only the promoted subdirectory). Without
/// its own `.git`, git would walk up and report an enclosing repository.
fn head_commit(dir: &Path) -> String {
    if !dir.join(".git").exists() {
        return String::new();
    }
    Command::new("git")
        .args(["-C", &dir.display().to_string(), "rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Install (or update) a catalog plugin by id. Clones into `~/.nur/plugins/<id>`.
pub fn install_plugin(id: &str) -> Result<String, String> {
    let entry = by_id(id)
        .ok_or_else(|| format!("unknown plugin '{id}' — open /plugins to browse the catalog"))?;
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
    // Commit of a fresh clone, read before a `path_in_repo` promote drops `.git`.
    let mut cloned_commit = String::new();
    // Default plugins carry a lockfile pin: the clone tracks that exact
    // commit, never a moved HEAD. Catalog plugins without a pin (user-chosen
    // installs) keep the latest-HEAD behavior.
    let pin = crate::ecosystem::lock::plugin_pin(entry.id);
    if dest.is_dir() {
        // Shallow clones often cannot ff-only pull; fetch + reset to origin
        // (or to the pinned commit when the lockfile has one).
        let target = pin.as_ref().map(|p| p.commit.as_str()).unwrap_or("HEAD");
        let fetch = Command::new("git")
            .args([
                "-C",
                &dest.display().to_string(),
                "fetch",
                "--depth",
                "1",
                "origin",
                target,
            ])
            .output();
        match fetch {
            Ok(o) if o.status.success() => {
                let _ = Command::new("git")
                    .args([
                        "-C",
                        &dest.display().to_string(),
                        "reset",
                        "--hard",
                        "FETCH_HEAD",
                    ])
                    .output();
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                tracing::warn!("plugin fetch {}: {err}", entry.id);
            }
            Err(e) => tracing::warn!("plugin fetch {}: {e}", entry.id),
        }
    } else {
        // Shallow clone into a temp dir then rename (atomic-ish). With a pin,
        // fetch that exact commit and verify HEAD matches before the rename.
        let tmp = home.join(format!(".tmp-{}", entry.id));
        let _ = fs::remove_dir_all(&tmp);
        let out = if let Some(p) = &pin {
            Command::new("git")
                .args(["init", "-q"])
                .arg(&tmp)
                .output()
                .and_then(|o| {
                    if o.status.success() {
                        Command::new("git")
                            .args([
                                "-C",
                                &tmp.display().to_string(),
                                "remote",
                                "add",
                                "origin",
                                entry.source_url,
                            ])
                            .output()
                    } else {
                        Ok(o)
                    }
                })
                .and_then(|o| {
                    if o.status.success() {
                        Command::new("git")
                            .args([
                                "-C",
                                &tmp.display().to_string(),
                                "fetch",
                                "--depth",
                                "1",
                                "origin",
                                &p.commit,
                            ])
                            .output()
                    } else {
                        Ok(o)
                    }
                })
                .and_then(|o| {
                    if o.status.success() {
                        Command::new("git")
                            .args([
                                "-C",
                                &tmp.display().to_string(),
                                "reset",
                                "-q",
                                "--hard",
                                &p.commit,
                            ])
                            .output()
                    } else {
                        Ok(o)
                    }
                })
                .map_err(|e| format!("git clone failed to start: {e}"))?
        } else {
            Command::new("git")
                .args([
                    "clone",
                    "--depth",
                    "1",
                    entry.source_url,
                    &tmp.display().to_string(),
                ])
                .output()
                .map_err(|e| format!("git clone failed to start: {e}"))?
        };
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let _ = fs::remove_dir_all(&tmp);
            return Err(format!("git clone failed: {err}"));
        }
        if let Some(p) = &pin {
            let head = Command::new("git")
                .args(["-C", &tmp.display().to_string(), "rev-parse", "HEAD"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default();
            if head != p.commit {
                let _ = fs::remove_dir_all(&tmp);
                return Err(format!(
                    "plugin {} pinned checkout drifted ({head} != {})",
                    entry.id, p.commit
                ));
            }
        }

        cloned_commit = head_commit(&tmp);

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
    // An update (every ecosystem ensure re-installs the defaults) keeps the
    // user's choice: a disabled plugin stays disabled.
    let enabled = reg.plugins.get(entry.id).map(|p| p.enabled).unwrap_or(true);
    // An update of a promoted (`.git`-less) plugin keeps the commit it
    // recorded at install rather than forgetting it.
    let recorded = reg
        .plugins
        .get(entry.id)
        .map(|p| p.commit.clone())
        .unwrap_or_default();
    let commit = match head_commit(&dest) {
        head if !head.is_empty() => head,
        _ if !cloned_commit.is_empty() => cloned_commit,
        _ => pin.as_ref().map(|p| p.commit.clone()).unwrap_or(recorded),
    };
    reg.plugins.insert(
        entry.id.to_string(),
        InstalledPlugin {
            id: entry.id.to_string(),
            source: entry.source_url.to_string(),
            enabled,
            installed_at: chrono::Utc::now().to_rfc3339(),
            path: dest.display().to_string(),
            commit: commit.clone(),
        },
    );
    reg.save()?;

    // Mirror skills into ~/.nur/skills/<name> for maximum discovery compatibility.
    let kept = if enabled {
        mirror_skills_to_nur_home(entry.id, entry.source_url, &commit, &dest)?
    } else {
        unmirror_skills(entry.id, Some(&dest));
        Vec::new()
    };
    let at = if commit.len() >= 7 {
        format!(" @ {}", &commit[..7])
    } else {
        String::new()
    };
    let mut msg = format!(
        "installed {}{at} → {} ({} skill packs, {})",
        entry.name,
        dest.display(),
        skill_n,
        if enabled { "enabled" } else { "disabled" }
    );
    for note in kept {
        msg.push_str(&format!("\n  {note}"));
    }
    Ok(msg)
}

pub fn uninstall_plugin(id: &str) -> Result<String, String> {
    let id = checked_id(id)?;
    let dir = plugins_home().join(id);
    // Its mirrored skills go too, or they stay live after the plugin is gone.
    unmirror_skills(id, dir.is_dir().then_some(dir.as_path()));
    if dir.is_dir() {
        fs::remove_dir_all(&dir).map_err(|e| format!("remove plugin dir: {e}"))?;
    }
    let mut reg = Registry::load();
    reg.plugins.remove(id);
    reg.save()?;
    crate::agent::skill_cache::invalidate_cache();
    Ok(format!("uninstalled plugin '{id}'"))
}

/// Plugins that should be present after a normal ecosystem ensure / install.
/// Superpowers + Fable + the "real engineering" default set (mattpocock,
/// addyosmani, builderio). Re-pulls existing clones on each ensure.
pub const DEFAULT_PLUGINS: &[&str] = &[
    "superpowers",
    "fable",
    "mattpocock",
    "addyosmani",
    "builderio",
];

/// Install the default plugin set. Returns (ok_ids, notes). Network-bound;
/// failures are non-fatal notes so offline machines still boot.
pub fn ensure_default_plugins() -> (Vec<String>, Vec<String>) {
    let mut ok = Vec::new();
    let mut notes = Vec::new();
    for id in DEFAULT_PLUGINS {
        // Always go through install_entry so an existing clone `git pull`s.
        match install_plugin(id) {
            Ok(msg) => {
                ok.push((*id).to_string());
                notes.push(msg);
            }
            Err(e) => notes.push(format!("{id}: {e}")),
        }
    }
    (ok, notes)
}

fn count_skills(root: &Path) -> usize {
    // Nested packs (mattpocock engineering/*, google ads/*, NVIDIA, …) need a walk.
    crate::agent::skills::find_skill_mds(root, 5).len()
}

/// Written into every skill directory mirrored into `~/.nur/skills`: which
/// plugin put it there. A directory without it is the user's own (or a copy
/// from before stamping, adopted only when its SKILL.md is identical).
const PROVENANCE_FILE: &str = ".nur-provenance.json";

#[derive(Debug, Serialize, Deserialize)]
struct Provenance {
    plugin: String,
    source: String,
    #[serde(default)]
    commit: String,
    installed_at: String,
}

fn read_provenance(dir: &Path) -> Option<Provenance> {
    serde_json::from_str(&fs::read_to_string(dir.join(PROVENANCE_FILE)).ok()?).ok()
}

/// The `name` in a skill directory's SKILL.md front matter.
fn skill_md_name(dir: &Path) -> Option<String> {
    let text = fs::read_to_string(dir.join("SKILL.md")).ok()?;
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    lines
        .take_while(|l| l.trim() != "---")
        .find_map(|l| l.strip_prefix("name:"))
        .map(|v| v.trim().trim_matches(['"', '\'']).to_string())
}

/// An unstamped mirror from before provenance existed: the same skill by its
/// SKILL.md `name` (an older copy differs in its body, so matching bytes
/// alone would strand it), or byte-identical when it names none. It may
/// also be a copy the user edited, so it is moved aside, never deleted.
fn same_skill(src: &Path, dest: &Path) -> bool {
    match (skill_md_name(src), skill_md_name(dest)) {
        (Some(a), Some(b)) => a == b,
        (None, None) => {
            let read = |d: &Path| fs::read(d.join("SKILL.md")).ok();
            read(src).is_some() && read(src) == read(dest)
        }
        _ => false,
    }
}

/// Move an unstamped skill directory out of the skill roots, to
/// `~/.nur/skills-replaced/<name>-<time>`, and say where it went.
fn set_aside(dest: &Path) -> Result<String, String> {
    let name = dest.file_name().and_then(|n| n.to_str()).unwrap_or("skill");
    let aside_root = nur_home().join("skills-replaced");
    fs::create_dir_all(&aside_root).map_err(|e| e.to_string())?;
    let stamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
    let aside = aside_root.join(format!("{name}-{stamp}"));
    fs::rename(dest, &aside).map_err(|e| e.to_string())?;
    Ok(format!(
        "moved your earlier ~/.nur/skills/{name} to {}",
        aside.display()
    ))
}

/// Most kept-skill notes one install reports.
const MAX_KEPT_NOTES: usize = 5;

/// The skill directories inside a plugin checkout (nested packs included).
fn plugin_skill_dirs(plugin_root: &Path) -> Vec<PathBuf> {
    // Recursive: supports skills/<category>/<name>/SKILL.md layouts.
    let mut sources: Vec<PathBuf> = Vec::new();
    for skill_md in crate::agent::skills::find_skill_mds(plugin_root, 5) {
        if let Some(parent) = skill_md.parent() {
            // Single-skill plugin with SKILL.md at plugin root: keep plugin id as name.
            if parent == plugin_root {
                sources.push(plugin_root.to_path_buf());
            } else {
                sources.push(parent.to_path_buf());
            }
        }
    }
    // Dedupe paths
    sources.sort();
    sources.dedup();
    sources
}

/// Copy a plugin's skill dirs into `~/.nur/skills/` so existing discovery
/// keeps working without the plugins path. A directory is replaced only when
/// this plugin owns it; one the user wrote, or another plugin's, is kept and
/// named in the returned notes.
fn mirror_skills_to_nur_home(
    plugin_id: &str,
    source: &str,
    commit: &str,
    plugin_root: &Path,
) -> Result<Vec<String>, String> {
    let dest_root = nur_home().join("skills");
    fs::create_dir_all(&dest_root).map_err(|e| e.to_string())?;
    let mut kept = Vec::new();
    let bundled = crate::ecosystem::bundled_skill_names();
    for src in plugin_skill_dirs(plugin_root) {
        let name = src.file_name().and_then(|n| n.to_str()).unwrap_or("skill");
        let dest = dest_root.join(name);
        if bundled.iter().any(|b| b == name) {
            kept.push(format!(
                "kept ~/.nur/skills/{name}: nur ships a skill of that name"
            ));
            continue;
        }
        if dest.exists() {
            let cleared = match read_provenance(&dest) {
                Some(p) if p.plugin == plugin_id => {
                    fs::remove_dir_all(&dest).map_err(|e| e.to_string())
                }
                Some(p) => {
                    kept.push(format!(
                        "kept ~/.nur/skills/{name}: it belongs to plugin `{}`",
                        p.plugin
                    ));
                    continue;
                }
                None if same_skill(&src, &dest) => set_aside(&dest).map(|note| kept.push(note)),
                None => {
                    kept.push(format!(
                        "kept ~/.nur/skills/{name}: a different skill uses that folder"
                    ));
                    continue;
                }
            };
            // A locked file (Windows) skips this skill, not the install.
            if let Err(error) = cleared {
                kept.push(format!(
                    "kept ~/.nur/skills/{name}: could not replace it ({error})"
                ));
                continue;
            }
        }
        // Full tree (SKILL.md + references/ etc.) so multi-file skills stay complete.
        copy_dir_recursive(&src, &dest)?;
        let stamp = Provenance {
            plugin: plugin_id.to_string(),
            source: source.to_string(),
            commit: commit.to_string(),
            installed_at: chrono::Utc::now().to_rfc3339(),
        };
        let raw = serde_json::to_string_pretty(&stamp).map_err(|e| e.to_string())?;
        fs::write(dest.join(PROVENANCE_FILE), raw).map_err(|e| e.to_string())?;
    }
    crate::agent::skill_cache::invalidate_cache();
    if kept.len() > MAX_KEPT_NOTES {
        let more = kept.len() - MAX_KEPT_NOTES;
        kept.truncate(MAX_KEPT_NOTES);
        kept.push(format!("... and {more} more kept"));
    }
    Ok(kept)
}

/// Remove the skill dirs `plugin_id` mirrored into `~/.nur/skills`: stamped
/// ones, plus unstamped copies identical to the plugin's (from before
/// stamping) when the checkout is still there to compare. Returns how many.
fn unmirror_skills(plugin_id: &str, plugin_root: Option<&Path>) -> usize {
    let dest_root = nur_home().join("skills");
    // nur's own skills are never a plugin's to take away.
    let bundled = crate::ecosystem::bundled_skill_names();
    let is_bundled = |dir: &Path| {
        dir.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| bundled.iter().any(|b| b == n))
    };
    let mut removed = 0;
    if let Ok(entries) = fs::read_dir(&dest_root) {
        for entry in entries.flatten() {
            let dir = entry.path();
            if !is_bundled(&dir)
                && read_provenance(&dir).is_some_and(|p| p.plugin == plugin_id)
                && fs::remove_dir_all(&dir).is_ok()
            {
                removed += 1;
            }
        }
    }
    if let Some(root) = plugin_root {
        for src in plugin_skill_dirs(root) {
            let Some(name) = src.file_name() else {
                continue;
            };
            let dest = dest_root.join(name);
            if !is_bundled(&dest)
                && read_provenance(&dest).is_none()
                && same_skill(&src, &dest)
                && set_aside(&dest).is_ok()
            {
                removed += 1;
            }
        }
    }
    crate::agent::skill_cache::invalidate_cache();
    removed
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

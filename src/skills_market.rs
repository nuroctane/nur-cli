//! Skills marketplace: discover and install third-party skills from a signed
//! index, each pinned to its author's repository at a fixed commit.
//!
//! - **Index.** `nur-market-index.json` is built per release by
//!   `scripts/build_market_index.py` (sources in `skills/market-sources.json`,
//!   aidentity's public-domain registry among them) and signed with the same
//!   minisign key as the binaries. nur never queries a registry at runtime: it
//!   downloads the index for its own version, verifies it, and caches it
//!   (`~/.nur/cache/market-index.json`). `NUR_SKILL_MARKET_INDEX` reads a
//!   local index instead (mirrors, tests).
//! - **Install** is a user action only (`nur skills add`, `/skills add`): the
//!   model can search but never install, so a page it reads cannot talk it
//!   into loading new instructions. Each file comes from
//!   `raw.githubusercontent.com/<repo>/<commit>/<path>/<file>`
//!   (`NUR_SKILL_RAW_BASE` overrides), must match the index's SHA-256, and
//!   the whole skill is scanned (`agent::skill_scan`) before it is renamed
//!   into `~/.nur/skills/<name>`. A `.nur-market.json` stamp records where it
//!   came from; `remove` deletes only stamped skills, `verify` re-hashes them.

use crate::agent::skill_scan;
use crate::config::nur_home;
use crate::error::{NurError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const INDEX_ASSET: &str = "nur-market-index.json";
const RELEASE_DOWNLOADS: &str = "https://github.com/nuroctane/nur-cli/releases/download";
const RAW_BASE: &str = "https://raw.githubusercontent.com";
const STAMP: &str = ".nur-market.json";
const CACHE_TTL_SECS: u64 = 24 * 60 * 60;
const MAX_INDEX_BYTES: usize = 16 * 1024 * 1024;
const MAX_FILE_BYTES: usize = 1024 * 1024;
const MAX_SKILL_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    /// `owner/skill-name`, unique in the index.
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub author: String,
    pub repo: String,
    pub commit: String,
    /// Directory of the skill inside `repo`.
    #[serde(default)]
    pub path: String,
    /// File (relative to `path`) -> SHA-256 hex.
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    /// False for link-only entries (no distributable license).
    #[serde(default)]
    pub installable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Index {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub entries: Vec<Entry>,
    /// Set when the index did not come from the signed release asset.
    #[serde(skip)]
    pub unsigned: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Stamp {
    id: String,
    repo: String,
    commit: String,
    license: String,
    installed_at: String,
    files: BTreeMap<String, String>,
}

/// The signed bytes and their signature, re-verified on every read: a cache
/// file is as writable as any other.
#[derive(Debug, Serialize, Deserialize)]
struct CachedIndex {
    fetched_at: u64,
    version: String,
    raw: String,
    sig: String,
}

/// After a failed download, how long `load_index` answers with that failure
/// instead of trying again (offline, every search would wait on the network).
const RETRY_AFTER_FAILURE: std::time::Duration = std::time::Duration::from_secs(300);

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn skills_root() -> PathBuf {
    nur_home().join("skills")
}

fn cache_path() -> PathBuf {
    nur_home().join("cache").join("market-index.json")
}

/// The index: a local one when `NUR_SKILL_MARKET_INDEX` names it, else the
/// cached copy for this version (a day old at most), else this release's
/// signed asset, verified and cached.
pub fn load_index() -> Result<Index> {
    if let Some(path) = std::env::var_os("NUR_SKILL_MARKET_INDEX").filter(|p| !p.is_empty()) {
        let path = Path::new(&path);
        let raw = std::fs::read_to_string(path)
            .map_err(|e| NurError::Other(format!("market index {}: {e}", path.display())))?;
        let mut index: Index = serde_json::from_str(&raw)
            .map_err(|e| NurError::Other(format!("market index is not valid: {e}")))?;
        index.unsigned = Some(format!(
            "unsigned local index {} (NUR_SKILL_MARKET_INDEX)",
            path.display()
        ));
        return Ok(index);
    }
    let version = env!("CARGO_PKG_VERSION");
    if let Some(cached) = std::fs::read_to_string(cache_path())
        .ok()
        .and_then(|raw| serde_json::from_str::<CachedIndex>(&raw).ok())
        .filter(|c| {
            c.version == version && now_secs().saturating_sub(c.fetched_at) < CACHE_TTL_SECS
        })
    {
        if let Ok(index) = verified(cached.raw.as_bytes(), &cached.sig) {
            return Ok(index);
        }
    }
    static LAST_FAILURE: std::sync::Mutex<Option<(std::time::Instant, String)>> =
        std::sync::Mutex::new(None);
    if let Some((at, error)) = LAST_FAILURE.lock().ok().and_then(|g| g.clone()) {
        if at.elapsed() < RETRY_AFTER_FAILURE {
            return Err(NurError::Other(error));
        }
    }
    let (raw, sig) = match download_index(version) {
        Ok(fetched) => fetched,
        Err(error) => {
            if let Ok(mut last) = LAST_FAILURE.lock() {
                *last = Some((std::time::Instant::now(), error.to_string()));
            }
            return Err(error);
        }
    };
    let index = verified(&raw, &sig)?;
    let cached = CachedIndex {
        fetched_at: now_secs(),
        version: version.to_string(),
        raw: String::from_utf8_lossy(&raw).into_owned(),
        sig,
    };
    if let Ok(raw) = serde_json::to_vec(&cached) {
        let _ = crate::cache_io::publish(&cache_path(), &raw);
    }
    Ok(index)
}

/// The index in `raw`, once its signature checks out against the pinned key.
fn verified(raw: &[u8], sig: &str) -> Result<Index> {
    crate::bootstrap::verify_minisig_text(raw, sig)?;
    serde_json::from_slice(raw)
        .map_err(|e| NurError::Other(format!("market index is not valid: {e}")))
}

/// This version's index bytes and signature, unverified.
fn download_index(version: &str) -> Result<(Vec<u8>, String)> {
    let http = crate::bootstrap::http_client(30)?;
    let url = format!("{RELEASE_DOWNLOADS}/v{version}/{INDEX_ASSET}");
    let get = |url: &str| -> Result<Vec<u8>> {
        let bytes = http
            .get(url)
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.bytes())
            .map_err(|e| {
                NurError::Other(format!(
                    "the skills market index for v{version} is not available ({e})"
                ))
            })?;
        if bytes.len() > MAX_INDEX_BYTES {
            return Err(NurError::Other("market index is implausibly large".into()));
        }
        Ok(bytes.to_vec())
    };
    let bytes = get(&url)?;
    let sig = String::from_utf8(get(&format!("{url}.minisig"))?)
        .map_err(|_| NurError::Other("market index signature is not text".into()))?;
    Ok((bytes, sig))
}

/// An id or repo that is safe to show as part of a command: `owner/name`
/// characters only, so a hostile index cannot hand over a shell line.
fn plain_ref(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 120
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '@'))
}

/// Entries matching `query` (id, name, description, category), best first.
pub fn search<'a>(index: &'a Index, query: &str, limit: usize) -> Vec<&'a Entry> {
    let words: Vec<String> = query
        .split_whitespace()
        .map(|w| w.to_ascii_lowercase())
        .collect();
    let mut scored: Vec<(usize, &Entry)> = index
        .entries
        .iter()
        .filter_map(|e| {
            let name = e.name.to_ascii_lowercase();
            let hay = format!(
                "{} {} {} {}",
                e.id.to_ascii_lowercase(),
                name,
                e.description.to_ascii_lowercase(),
                e.category.to_ascii_lowercase()
            );
            if words.is_empty() || !words.iter().all(|w| hay.contains(w)) {
                return None;
            }
            let score = words.iter().filter(|w| name.contains(w.as_str())).count() * 10
                + usize::from(name == query.to_ascii_lowercase()) * 100;
            Some((score, e))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.id.cmp(&b.1.id)));
    scored.into_iter().take(limit).map(|(_, e)| e).collect()
}

/// One result line: id, license, repo, and how to get it.
/// Index text as one clean line: every field comes from third parties and
/// reaches a model through `skill search`.
fn clean(text: &str, max: usize) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .filter(|c| !matches!(*c as u32, 0xE0000..=0xE007F | 0x202A..=0x202E | 0x2066..=0x2069 | 0x200B..=0x200F | 0x2060 | 0xFEFF))
        .collect();
    flat.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max)
        .collect()
}

/// One result line: id, license, repo, description and, with `hint`, how to
/// get it (`nur skills add <id>`; the TUI shows its own command instead).
pub fn describe(entry: &Entry, hint: bool) -> String {
    let license = if entry.license.is_empty() {
        "no license".to_string()
    } else {
        clean(&entry.license, 40)
    };
    let how = match (hint, entry.installable) {
        (true, true) if plain_ref(&entry.id) => format!(" · nur skills add {}", entry.id),
        (_, false) if plain_ref(&entry.repo) => {
            format!(" · link only: https://github.com/{}", entry.repo)
        }
        (_, false) => " · link only".to_string(),
        _ => String::new(),
    };
    format!(
        "{} · {license} · {}@{} · {}{how}",
        clean(&entry.id, 80),
        clean(&entry.repo, 80),
        clean(&entry.commit, 7),
        clean(&entry.description, 160),
    )
}

fn resolve<'a>(index: &'a Index, wanted: &str) -> Result<&'a Entry> {
    if let Some(e) = index.entries.iter().find(|e| e.id == wanted) {
        return Ok(e);
    }
    let by_name: Vec<&Entry> = index.entries.iter().filter(|e| e.name == wanted).collect();
    match by_name.as_slice() {
        [one] => Ok(one),
        [] => Err(NurError::Other(format!(
            "no skill `{wanted}` in the market index (nur skills search <words>)"
        ))),
        many => Err(NurError::Other(format!(
            "`{wanted}` is ambiguous: {}",
            many.iter()
                .map(|e| e.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

fn safe_name(name: &str) -> Result<String> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    ok.then(|| name.to_string())
        .ok_or_else(|| NurError::Other(format!("`{name}` is not a usable skill name")))
}

/// A file path the index may list: relative, plain characters only (a file
/// name reaches the model through `list_dir`, so it is held to the same bar
/// as the contents), and no second SKILL.md, which would register a skill
/// of its own under any name.
fn safe_relative(file: &str) -> bool {
    let path = Path::new(file);
    !file.is_empty()
        && !path.is_absolute()
        && file
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
        && path
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
        && (file == "SKILL.md" || !file.ends_with("SKILL.md"))
}

/// Set the front matter `name` of `SKILL.md` text to `name` (skills register
/// under that name, not their folder's), adding it when missing.
fn with_name(text: &str, name: &str) -> String {
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"));
    let Some(rest) = rest else {
        return format!("---\nname: {name}\n---\n{text}");
    };
    let mut out = String::from("---\n");
    let mut named = false;
    let mut lines = rest.split_inclusive('\n');
    for line in lines.by_ref() {
        if line.trim_end() == "---" {
            if !named {
                out.push_str(&format!("name: {name}\n"));
            }
            out.push_str(line);
            break;
        }
        if line.starts_with("name:") {
            out.push_str(&format!("name: {name}\n"));
            named = true;
        } else {
            out.push_str(line);
        }
    }
    out.extend(lines);
    out
}

fn read_stamp(dir: &Path) -> Option<Stamp> {
    serde_json::from_str(&std::fs::read_to_string(dir.join(STAMP)).ok()?).ok()
}

/// Install `wanted` (an id, or a name unique in the index) as `alias` or its
/// own name. Refuses a name that is taken by a skill the market did not
/// install unless `replace`.
pub fn install(wanted: &str, alias: Option<&str>, replace: bool) -> Result<String> {
    let index = load_index()?;
    install_from(&index, wanted, alias, replace)
}

fn install_from(index: &Index, wanted: &str, alias: Option<&str>, replace: bool) -> Result<String> {
    let entry = resolve(index, wanted)?;
    if !entry.installable {
        return Err(NurError::Other(format!(
            "{} is link only ({}): https://github.com/{}",
            entry.id,
            if entry.license.is_empty() {
                "no distributable license"
            } else {
                &entry.license
            },
            entry.repo
        )));
    }
    if entry.files.is_empty() || !entry.files.contains_key("SKILL.md") {
        return Err(NurError::Other(format!("{} lists no SKILL.md", entry.id)));
    }
    let name = safe_name(alias.unwrap_or(&entry.name))?;
    let root = skills_root();
    let dest = root.join(&name);
    if dest.exists() && !replace {
        match read_stamp(&dest) {
            Some(stamp) if stamp.id == entry.id => {}
            _ => {
                return Err(NurError::Other(format!(
                    "a skill named `{name}` is already installed; add with --as <name> or --replace"
                )))
            }
        }
    }
    let http = crate::bootstrap::http_client(30)?;
    let base = std::env::var("NUR_SKILL_RAW_BASE").unwrap_or_else(|_| RAW_BASE.to_string());
    std::fs::create_dir_all(&root).map_err(|e| NurError::Other(e.to_string()))?;
    let tmp = root.join(format!(".market-{}", uuid::Uuid::new_v4().simple()));
    let result = (|| -> Result<Vec<skill_scan::Flag>> {
        let mut total = 0usize;
        for (file, expected) in &entry.files {
            if !safe_relative(file) {
                return Err(NurError::Other(format!(
                    "index lists an unsafe path `{file}`"
                )));
            }
            let prefix = if entry.path.is_empty() {
                String::new()
            } else {
                format!("{}/", entry.path.trim_matches('/'))
            };
            let url = format!(
                "{}/{}/{}/{prefix}{file}",
                base.trim_end_matches('/'),
                entry.repo,
                entry.commit
            );
            let bytes = http
                .get(&url)
                .send()
                .and_then(|r| r.error_for_status())
                .and_then(|r| r.bytes())
                .map_err(|e| NurError::Other(format!("fetch {file}: {e}")))?;
            total += bytes.len();
            if bytes.len() > MAX_FILE_BYTES || total > MAX_SKILL_BYTES {
                return Err(NurError::Other(format!(
                    "{file} is larger than a skill file may be"
                )));
            }
            let actual = crate::bootstrap::sha256_hex(&bytes);
            if !actual.eq_ignore_ascii_case(expected) {
                return Err(NurError::Other(format!(
                    "{file} does not match the index (sha256 {actual} != {expected})"
                )));
            }
            let path = tmp.join(file);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| NurError::Other(e.to_string()))?;
            }
            std::fs::write(&path, &bytes).map_err(|e| NurError::Other(e.to_string()))?;
        }
        // The skill registers under the name chosen here, never one its
        // front matter claims (`name: commit` would shadow another skill).
        let skill_md = tmp.join("SKILL.md");
        let text = std::fs::read_to_string(&skill_md)
            .map_err(|_| NurError::Other(format!("{}: SKILL.md is not UTF-8 text", entry.id)))?;
        let renamed = with_name(&text, &name);
        let mut files = entry.files.clone();
        if renamed != text {
            std::fs::write(&skill_md, &renamed).map_err(|e| NurError::Other(e.to_string()))?;
            files.insert(
                "SKILL.md".into(),
                crate::bootstrap::sha256_hex(renamed.as_bytes()),
            );
        }
        let flags = skill_scan::scan_dir(&tmp);
        if skill_scan::blocks(&flags) {
            return Err(NurError::Other(format!(
                "refused {}: {}",
                entry.id,
                flags
                    .iter()
                    .filter(|f| f.severity == skill_scan::Severity::Block)
                    .map(|f| f.describe())
                    .collect::<Vec<_>>()
                    .join("; ")
            )));
        }
        let stamp = Stamp {
            id: entry.id.clone(),
            repo: entry.repo.clone(),
            commit: entry.commit.clone(),
            license: entry.license.clone(),
            installed_at: chrono::Utc::now().to_rfc3339(),
            files,
        };
        let raw =
            serde_json::to_string_pretty(&stamp).map_err(|e| NurError::Other(e.to_string()))?;
        std::fs::write(tmp.join(STAMP), raw).map_err(|e| NurError::Other(e.to_string()))?;
        // Swap through a side name so a failed move puts the old skill back
        // instead of leaving neither.
        let old = root.join(format!(".market-old-{}", uuid::Uuid::new_v4().simple()));
        let had_old = dest.exists();
        if had_old {
            std::fs::rename(&dest, &old).map_err(|e| NurError::Other(e.to_string()))?;
        }
        if let Err(error) = std::fs::rename(&tmp, &dest) {
            if had_old {
                let _ = std::fs::rename(&old, &dest);
            }
            return Err(NurError::Other(error.to_string()));
        }
        if had_old {
            let _ = std::fs::remove_dir_all(&old);
        }
        Ok(flags)
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    let flags = result?;
    crate::agent::skill_cache::invalidate_cache();
    let mut msg = format!(
        "installed {} as /{name} from {}@{} ({})",
        entry.id,
        entry.repo,
        entry.commit.chars().take(7).collect::<String>(),
        if entry.license.is_empty() {
            "no license"
        } else {
            &entry.license
        }
    );
    for flag in &flags {
        msg.push_str(&format!("\n  review: {}", flag.describe()));
    }
    Ok(msg)
}

/// Remove a market-installed skill. A skill without the market stamp is the
/// user's own (or a plugin's) and is never touched.
pub fn remove(name: &str) -> Result<String> {
    let name = safe_name(name)?;
    let dir = skills_root().join(&name);
    let stamp = read_stamp(&dir)
        .ok_or_else(|| NurError::Other(format!("`{name}` was not installed from the market")))?;
    std::fs::remove_dir_all(&dir).map_err(|e| NurError::Other(e.to_string()))?;
    crate::agent::skill_cache::invalidate_cache();
    Ok(format!("removed /{name} ({})", stamp.id))
}

/// Every file under `dir`, relative and `/`-separated.
fn files_under(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(rel) = path.strip_prefix(dir) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out
}

/// Re-hash every market-installed skill against its stamp. The flag is true
/// when any skill changed since it was installed.
pub fn verify() -> (String, bool) {
    let mut lines = Vec::new();
    let Ok(entries) = std::fs::read_dir(skills_root()) else {
        return ("no skills installed".into(), false);
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        let Some(stamp) = read_stamp(&dir) else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().to_string();
        let mut changed: Vec<String> = stamp
            .files
            .iter()
            .filter(|(file, expected)| {
                std::fs::read(dir.join(file))
                    .map(|b| !crate::bootstrap::sha256_hex(&b).eq_ignore_ascii_case(expected))
                    .unwrap_or(true)
            })
            .map(|(file, _)| file.clone())
            .collect();
        // A file the install did not write is a change too.
        changed.extend(
            files_under(&dir)
                .into_iter()
                .filter(|f| f != STAMP && !stamp.files.contains_key(f))
                .map(|f| format!("{f} (added)")),
        );
        if changed.is_empty() {
            lines.push(format!("ok        /{name} ({})", stamp.id));
        } else {
            lines.push(format!(
                "CHANGED   /{name} ({}): {}",
                stamp.id,
                changed.join(", ")
            ));
        }
    }
    if lines.is_empty() {
        ("no market-installed skills".into(), false)
    } else {
        lines.sort();
        let changed = lines.iter().any(|l| l.starts_with("CHANGED"));
        (lines.join("\n"), changed)
    }
}

/// Where a skill directory came from, for `skill read`: `repo@commit (license)`.
pub fn provenance_line(skill_dir: &Path) -> Option<String> {
    let stamp = read_stamp(skill_dir)?;
    Some(format!(
        "source: {}@{} ({}), installed from the skills market",
        stamp.repo,
        stamp.commit.chars().take(7).collect::<String>(),
        if stamp.license.is_empty() {
            "no license"
        } else {
            &stamp.license
        }
    ))
}

/// The scanner's verdict for a local skill directory, and whether it blocks.
pub fn scan_report(dir: &Path) -> Result<(String, bool)> {
    if !dir.is_dir() {
        return Err(NurError::Other(format!(
            "{} is not a directory",
            dir.display()
        )));
    }
    let flags = skill_scan::scan_dir(dir);
    if flags.is_empty() {
        return Ok((format!("{}: clean", dir.display()), false));
    }
    let mut out = format!(
        "{}: {}\n",
        dir.display(),
        if skill_scan::blocks(&flags) {
            "blocked"
        } else {
            "review"
        }
    );
    for flag in &flags {
        out.push_str(&format!("  {}\n", flag.describe()));
    }
    Ok((out, skill_scan::blocks(&flags)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, name: &str, installable: bool) -> Entry {
        Entry {
            id: id.into(),
            name: name.into(),
            description: format!("{name} helper"),
            category: "data".into(),
            license: "MIT".into(),
            author: String::new(),
            repo: "owner/repo".into(),
            commit: "abc1234def".into(),
            path: format!("skills/{name}"),
            files: BTreeMap::from([("SKILL.md".to_string(), "00".to_string())]),
            installable,
        }
    }

    // Failure modes: an ambiguous name installs the wrong skill; a link-only
    // entry installs; an index path escapes the skill directory.

    #[test]
    fn names_resolve_only_when_unique() {
        let index = Index {
            version: 1,
            entries: vec![
                entry("a/postgres", "postgres", true),
                entry("b/postgres", "postgres", true),
                entry("a/redis", "redis", true),
            ],
            unsigned: None,
        };
        assert_eq!(resolve(&index, "redis").unwrap().id, "a/redis");
        assert!(resolve(&index, "postgres").is_err());
        assert_eq!(resolve(&index, "b/postgres").unwrap().id, "b/postgres");
        assert_eq!(search(&index, "redis helper", 5)[0].id, "a/redis");
    }

    #[test]
    fn link_only_entries_and_escaping_paths_never_install() {
        let index = Index {
            version: 1,
            entries: vec![entry("a/closed", "closed", false)],
            unsigned: None,
        };
        assert!(install_from(&index, "a/closed", None, false)
            .unwrap_err()
            .to_string()
            .contains("link only"));
        assert!(!safe_relative("../escape.md"));
        assert!(!safe_relative("/abs.md"));
        assert!(safe_relative("references/rls.md"));
        // A second SKILL.md registers a skill of its own; odd characters in a
        // file name reach the model through list_dir.
        assert!(!safe_relative("sub/SKILL.md"));
        assert!(!safe_relative("notes\u{202e}dm.txt"));
    }

    // Failure mode: a market skill registers under a name it was not
    // installed as, shadowing another skill.
    #[test]
    fn front_matter_name_follows_the_install_name() {
        let renamed = with_name("---\nname: commit\ndescription: d\n---\nbody\n", "tidy");
        assert_eq!(renamed, "---\nname: tidy\ndescription: d\n---\nbody\n");
        let added = with_name("---\ndescription: d\n---\nbody\n", "tidy");
        assert_eq!(added, "---\ndescription: d\nname: tidy\n---\nbody\n");
        assert_eq!(with_name("body\n", "tidy"), "---\nname: tidy\n---\nbody\n");
    }
}

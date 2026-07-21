//! Fast skill index cache — avoids re-scanning 700+ SKILL.md files on every TUI launch.
//!
//! Cold scan (718 files) is ~3.6s on Windows; the cached JSON parse is ~12ms.
//! Cache lives at `~/.nur/cache/skills-index.json` and is invalidated when:
//! - version mismatch
//! - TTL expired (24h)
//! - a skill install/mirror calls [`invalidate_cache`]
//!
//! Deliberately *not* invalidated by an mtime/file-count walk: that walk is the
//! expensive part of the cold scan, so hand-editing a SKILL.md under a global
//! root is picked up on the next TTL roll rather than immediately. Skills under
//! the cwd are always scanned fresh, so project-local editing stays live.

use crate::config::nur_home;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::skills::{find_skill_mds, Skill, SKILL_WALK_MAX_DEPTH};

const CACHE_VERSION: u32 = 1;
const CACHE_TTL_SECS: u64 = 24 * 60 * 60; // 24h

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedSkill {
    name: String,
    description: String,
    path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SkillCacheFile {
    version: u32,
    generated_at: u64,
    file_count: usize,
    skills: Vec<CachedSkill>,
}

fn cache_path() -> PathBuf {
    nur_home().join("cache").join("skills-index.json")
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Global roots that are safe to cache (not cwd-specific).
fn global_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    roots.push(crate::config::meta_home().join("skills"));
    roots.extend(crate::plugins::enabled_skill_roots());
    roots.push(crate::config::legacy_muse_home().join("skills"));
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".agents").join("skills"));
    }
    roots
}

fn global_roots_existing() -> Vec<PathBuf> {
    global_roots().into_iter().filter(|p| p.is_dir()).collect()
}

/// Cwd-specific roots — always scanned fresh.
fn cwd_roots(cwd: &Path) -> Vec<PathBuf> {
    vec![
        cwd.join(".nur").join("skills"),
        cwd.join(".meta").join("skills"),
        cwd.join(".claude").join("skills"),
        cwd.join(".agents").join("skills"),
    ]
    .into_iter()
    .filter(|p| p.is_dir())
    .collect()
}

/// Quick count of SKILL.md files under global roots (metadata walk, no content read).
fn quick_count_global() -> usize {
    let mut count = 0;
    for root in global_roots_existing() {
        count += find_skill_mds(&root, SKILL_WALK_MAX_DEPTH).len();
    }
    count
}

/// Try to load cache if fresh. Returns None when stale/missing.
/// Fast path: only checks version + TTL (24h). No filesystem walk for instant TUI.
/// Explicit invalidation via `invalidate_cache()` is called after skill installs.
pub fn try_load_cache() -> Option<Vec<Skill>> {
    let path = cache_path();
    let text = std::fs::read_to_string(&path).ok()?;
    let cf: SkillCacheFile = serde_json::from_str(&text).ok()?;
    if cf.version != CACHE_VERSION {
        return None;
    }
    let now = now_secs();
    let age = now.saturating_sub(cf.generated_at);
    if age > CACHE_TTL_SECS {
        return None;
    }
    // A zero count means the cache was written before any root existed; rescan
    // rather than serving an empty skill set for the rest of the TTL.
    if cf.file_count == 0 {
        return None;
    }

    let mut out = Vec::with_capacity(cf.skills.len());
    for cs in cf.skills {
        out.push(Skill {
            name: cs.name,
            description: cs.description,
            body: String::new(),
            path: PathBuf::from(cs.path),
        });
    }
    Some(out)
}

/// Save global skills to cache.
pub fn save_cache(skills: &[Skill]) {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let cached: Vec<CachedSkill> = skills
        .iter()
        .map(|s| CachedSkill {
            name: s.name.clone(),
            description: s.description.clone(),
            path: s.path.display().to_string(),
        })
        .collect();

    // file_count is total SKILL.md files on disk (not deduped) for quick invalidation
    let file_count = quick_count_global();
    let cf = SkillCacheFile {
        version: CACHE_VERSION,
        generated_at: now_secs(),
        file_count,
        skills: cached,
    };
    if let Ok(text) = serde_json::to_string(&cf) {
        let _ = std::fs::write(&path, text);
    }
}

/// Load skills with cache for global roots + fresh scan for cwd roots.
pub fn load_skills_cached(cwd: &Path) -> Vec<Skill> {
    let mut global_skills = Vec::new();
    let mut from_cache = false;

    if let Some(cached) = try_load_cache() {
        global_skills = cached;
        from_cache = true;
    }

    if !from_cache {
        let mut seen = HashSet::new();
        for root in global_roots_existing() {
            for md in find_skill_mds(&root, SKILL_WALK_MAX_DEPTH) {
                if let Some(skill) = super::skills::parse_skill(&md) {
                    if seen.insert(skill.name.clone()) {
                        global_skills.push(skill);
                    }
                }
            }
        }
        global_skills.sort_by(|a, b| a.name.cmp(&b.name));
        save_cache(&global_skills);
    }

    let mut cwd_skills = Vec::new();
    for root in cwd_roots(cwd) {
        for md in find_skill_mds(&root, SKILL_WALK_MAX_DEPTH) {
            if let Some(skill) = super::skills::parse_skill(&md) {
                cwd_skills.push(skill);
            }
        }
    }

    let mut out = global_skills;
    for cs in cwd_skills {
        if !out.iter().any(|s| s.name == cs.name) {
            out.push(cs);
        } else {
            out.retain(|s| s.name != cs.name);
            out.push(cs);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn invalidate_cache() {
    let _ = std::fs::remove_file(cache_path());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn bench_cached_vs_cold() {
        let cwd = std::env::current_dir().unwrap();
        // clear cache first to measure cold
        let _ = std::fs::remove_file(cache_path());
        let start = Instant::now();
        let cold = load_skills_cached(&cwd);
        let cold_elapsed = start.elapsed();
        println!("COLD load {} skills in {:?}", cold.len(), cold_elapsed);

        let start2 = Instant::now();
        let cached = load_skills_cached(&cwd);
        let cached_elapsed = start2.elapsed();
        println!(
            "CACHED load {} skills in {:?}",
            cached.len(),
            cached_elapsed
        );

        assert!(cached.len() > 0);
        // cached should be significantly faster than cold (at least 2x faster, ideally 5x)
        // cold was ~1-2s with read, cached should be <100ms
        assert!(
            cached_elapsed.as_millis() < 200,
            "cached should be <200ms, got {:?}",
            cached_elapsed
        );
    }
}

//! Durable, incremental skill metadata snapshots. TUI discovery runs on a
//! worker; activation always reads skill bodies from their original files.
use super::skills::{find_skill_mds_excluding, parse_skill_metadata, Skill, SKILL_WALK_MAX_DEPTH};
use crate::config::nur_home;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const CACHE_VERSION: u32 = 2;
const CACHE_TTL_SECS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Stamp {
    length: u64,
    modified: u64,
}
fn stamp(path: &Path) -> Option<Stamp> {
    let m = fs::metadata(path).ok()?;
    Some(Stamp {
        length: m.len(),
        modified: m
            .modified()
            .ok()?
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_nanos() as u64,
    })
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Root {
    path: PathBuf,
    stamp: Option<Stamp>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedSkill {
    name: String,
    description: String,
    path: PathBuf,
    #[serde(default)]
    stamp: Option<Stamp>,
}
impl CachedSkill {
    fn skill(&self) -> Skill {
        Skill {
            name: self.name.clone(),
            description: self.description.clone(),
            path: self.path.clone(),
            body: String::new(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SkillCacheFile {
    version: u32,
    generated_at: u64,
    #[serde(default)]
    generation: String,
    #[serde(default)]
    roots: Vec<Root>,
    file_count: usize,
    skills: Vec<CachedSkill>,
    // Retain shadowed metadata: removal of a winner restores its fallback.
    #[serde(default)]
    files: Vec<CachedSkill>,
}
type Snapshot = (Option<Stamp>, Arc<SkillCacheFile>);
static SNAPSHOTS: OnceLock<Mutex<HashMap<PathBuf, Snapshot>>> = OnceLock::new();
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
fn cache_path(home: &Path) -> PathBuf {
    home.join("cache/skills-index.json")
}
fn generation(home: &Path) -> String {
    fs::read_to_string(home.join("cache/skills-generation")).unwrap_or_default()
}
fn global_roots() -> Vec<PathBuf> {
    let mut roots = vec![
        nur_home().join("skills"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("skills"),
    ];
    roots.extend(crate::plugins::enabled_skill_roots());
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".agents/skills"));
    }
    roots
}
fn root_snapshot(roots: &[PathBuf]) -> Vec<Root> {
    roots
        .iter()
        .map(|path| Root {
            path: path.clone(),
            stamp: stamp(path),
        })
        .collect()
}
fn cwd_roots(cwd: &Path) -> Vec<PathBuf> {
    [".nur/skills", ".claude/skills", ".agents/skills"]
        .iter()
        .map(|p| cwd.join(p))
        .collect()
}
fn read_snapshot(home: &Path) -> Option<Arc<SkillCacheFile>> {
    let path = cache_path(home);
    let current_stamp = stamp(&path);
    let memory = SNAPSHOTS.get_or_init(Default::default);
    if let Ok(guard) = memory.lock() {
        if let Some((saved, data)) = guard.get(&path) {
            if saved == &current_stamp {
                return Some(data.clone());
            }
        }
    }
    let data: SkillCacheFile = serde_json::from_slice(&fs::read(&path).ok()?).ok()?;
    let data = Arc::new(data);
    if let Ok(mut guard) = memory.lock() {
        guard.insert(path, (current_stamp, data.clone()));
    }
    Some(data)
}
fn fresh(data: &SkillCacheFile, generation: &str, roots: &[Root]) -> bool {
    data.version == CACHE_VERSION
        && data.generation == generation
        && data.roots == roots
        && now_secs().saturating_sub(data.generated_at) < CACHE_TTL_SECS
}
/// No recursive walk. Structural changes invalidate immediately; external
/// nested edits reconcile at the existing 24-hour TTL.
pub fn is_current() -> bool {
    let home = nur_home();
    read_snapshot(&home)
        .is_some_and(|d| fresh(&d, &generation(&home), &root_snapshot(&global_roots())))
}
/// Stale data is only for the palette. Execution waits for fresh discovery on
/// its worker, preserving enablement and current skill availability.
pub fn cached_palette() -> Vec<(String, String)> {
    read_snapshot(&nur_home())
        .map(|data| {
            data.skills
                .iter()
                .map(|s| (s.name.clone(), s.description.chars().take(72).collect()))
                .collect()
        })
        .unwrap_or_default()
}
fn rebuild(home: &Path, roots: &[PathBuf], previous: Option<&SkillCacheFile>) -> SkillCacheFile {
    let previous: HashMap<_, _> = previous
        .into_iter()
        .flat_map(|p| p.files.iter())
        .map(|s| (s.path.clone(), s))
        .collect();
    let generation = generation(home);
    let root_state = root_snapshot(roots);
    let mut files = Vec::new();
    let mut skills = Vec::new();
    let mut names = HashSet::new();
    let mut paths = HashSet::new();
    let mut scanned = Vec::new();
    let mut file_count = 0;
    for root in roots {
        let discovered = find_skill_mds_excluding(root, SKILL_WALK_MAX_DEPTH, &scanned);
        crate::startup::mark("skills_root_walked");
        for path in discovered {
            if !paths.insert(path.clone()) {
                continue;
            }
            file_count += 1;
            let current_stamp = stamp(&path);
            let entry = previous
                .get(&path)
                .filter(|old| current_stamp.is_some() && old.stamp == current_stamp)
                .map(|old| (*old).clone())
                .or_else(|| {
                    parse_skill_metadata(&path).map(|s| CachedSkill {
                        name: s.name,
                        description: s.description,
                        path: s.path,
                        stamp: current_stamp,
                    })
                });
            if let Some(entry) = entry {
                if names.insert(entry.name.clone()) {
                    skills.push(entry.clone());
                }
                files.push(entry);
            }
        }
        scanned.push(root.clone());
        crate::startup::mark("skills_root_indexed");
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    SkillCacheFile {
        version: CACHE_VERSION,
        generated_at: now_secs(),
        generation,
        roots: root_state,
        file_count,
        skills,
        files,
    }
}
fn builder_lock(home: &Path) -> std::io::Result<File> {
    fs::create_dir_all(home.join("cache"))?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(home.join("cache/skills-index.lock"))?;
    // OS leases release on crash. Only builders lock; readers retain snapshots.
    file.lock()?;
    Ok(file)
}
fn load_global_at(home: &Path, roots: &[PathBuf]) -> Arc<SkillCacheFile> {
    let current = read_snapshot(home);
    if let Some(data) = current
        .as_ref()
        .filter(|d| fresh(d, &generation(home), &root_snapshot(roots)))
    {
        return data.clone();
    }
    let lock = builder_lock(home).ok();
    let current = read_snapshot(home);
    // Another process may have completed the same work while we waited.
    if let Some(data) = current
        .as_ref()
        .filter(|d| fresh(d, &generation(home), &root_snapshot(roots)))
    {
        return data.clone();
    }
    let mut data = rebuild(home, roots, current.as_deref());
    loop {
        if data.generation != generation(home) || data.roots != root_snapshot(roots) {
            data = rebuild(home, roots, Some(&data));
            continue;
        }
        if lock.is_some() {
            if let Ok(bytes) = serde_json::to_vec(&data) {
                let _ = crate::cache_io::publish(&cache_path(home), &bytes);
            }
        }
        if data.generation != generation(home) || data.roots != root_snapshot(roots) {
            data = rebuild(home, roots, Some(&data));
            continue;
        }
        let data = Arc::new(data);
        if let Ok(mut memory) = SNAPSHOTS.get_or_init(Default::default).lock() {
            memory.insert(cache_path(home), (stamp(&cache_path(home)), data.clone()));
        }
        return data;
    }
}
/// Fresh execution snapshot plus current project overrides. Interactive callers
/// use a worker. Global metadata stays in memory across calls.
pub fn load_skills_cached(cwd: &Path) -> Vec<Skill> {
    let global = load_global_at(&nur_home(), &global_roots());
    let mut out: HashMap<String, Skill> = global
        .skills
        .iter()
        .map(|s| (s.name.clone(), s.skill()))
        .collect();
    for root in cwd_roots(cwd) {
        for path in find_skill_mds_excluding(&root, SKILL_WALK_MAX_DEPTH, &[]) {
            if let Some(skill) = parse_skill_metadata(&path) {
                out.insert(skill.name.clone(), skill);
            }
        }
    }
    let mut out: Vec<_> = out.into_values().collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}
fn invalidate_at(home: &Path) {
    // Never discard a complete snapshot; invalidation cannot wait behind a scan.
    let _ = crate::cache_io::publish(
        &home.join("cache/skills-generation"),
        uuid::Uuid::new_v4().to_string().as_bytes(),
    );
}
pub fn invalidate_cache() {
    invalidate_at(&nur_home());
}
/// Seed metadata before install/update completion instead of on the next launch.
pub fn warm_global() {
    let _ = load_global_at(&nur_home(), &global_roots());
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("nur-skill-cache-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }
        fn skill(&self, relative: &str, name: &str, description: &str) -> PathBuf {
            let path = self.0.join(relative).join("SKILL.md");
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(
                &path,
                format!("---\nname: {name}\ndescription: {description}\n---\nComplete body\n"),
            )
            .unwrap();
            path
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    // Failure: invalidation discards the usable palette; malformed JSON bricks discovery.
    #[test]
    fn invalidation_retains_snapshot_and_corruption_recovers() {
        let f = Fixture::new();
        f.skill("skills/a", "one", "first");
        let roots = vec![f.0.join("skills")];
        let old = load_global_at(&f.0, &roots);
        invalidate_at(&f.0);
        assert_eq!(read_snapshot(&f.0).unwrap().generation, old.generation);
        let new = load_global_at(&f.0, &roots);
        assert_ne!(new.generation, old.generation);
        fs::write(cache_path(&f.0), "{broken").unwrap();
        assert_eq!(load_global_at(&f.0, &roots).skills[0].name, "one");
    }

    // Failure: removing a winner loses its fallback; unchanged metadata masks edits.
    #[test]
    fn incremental_rebuild_restores_fallback_and_changed_metadata() {
        let f = Fixture::new();
        let primary = f.skill("primary/a", "same", "primary");
        f.skill("secondary/a", "same", "fallback");
        let roots = vec![f.0.join("primary"), f.0.join("secondary")];
        assert_eq!(
            load_global_at(&f.0, &roots).skills[0].description,
            "primary"
        );
        fs::remove_file(primary).unwrap();
        f.skill("secondary/a", "same", "changed fallback description");
        invalidate_at(&f.0);
        let data = load_global_at(&f.0, &roots);
        assert_eq!(data.skills[0].description, "changed fallback description");
        assert_eq!(data.file_count, 1);
    }

    // Failure: pruning overlap loses sibling skills or changes root precedence.
    #[test]
    fn overlapping_roots_preserve_siblings_and_precedence() {
        let f = Fixture::new();
        f.skill("plugin/skills/a", "same", "nested wins");
        f.skill("plugin/other/b", "same", "other");
        f.skill("plugin/other/c", "sibling", "retained");
        let roots = vec![f.0.join("plugin/skills"), f.0.join("plugin")];
        let data = load_global_at(&f.0, &roots);
        assert_eq!(data.file_count, 3);
        assert_eq!(data.skills.len(), 2);
        assert_eq!(
            data.skills
                .iter()
                .find(|s| s.name == "same")
                .unwrap()
                .description,
            "nested wins"
        );
    }

    // Failure: a builder's OS lock also blocks snapshot readers.
    #[test]
    fn snapshot_readers_do_not_wait_for_builder() {
        let f = Fixture::new();
        f.skill("skills/a", "one", "description");
        load_global_at(&f.0, &[f.0.join("skills")]);
        let _lock = builder_lock(&f.0).unwrap();
        invalidate_at(&f.0);
        assert_eq!(read_snapshot(&f.0).unwrap().skills[0].name, "one");
    }

    // Failure: empty or expired snapshots never recover when roots appear,
    // and disabled roots keep their formerly winning metadata.
    #[test]
    fn empty_expired_and_changed_root_sets_reconcile() {
        let f = Fixture::new();
        let roots = vec![f.0.join("primary"), f.0.join("fallback")];
        assert!(load_global_at(&f.0, &roots).skills.is_empty());
        f.skill("primary/a", "same", "primary");
        f.skill("fallback/a", "same", "fallback");
        let data = load_global_at(&f.0, &roots);
        assert_eq!(data.skills[0].description, "primary");
        let mut expired = (*data).clone();
        expired.generated_at = 0;
        crate::cache_io::publish(&cache_path(&f.0), &serde_json::to_vec(&expired).unwrap())
            .unwrap();
        assert!(load_global_at(&f.0, &roots).generated_at > 0);
        assert_eq!(
            load_global_at(&f.0, &roots[1..]).skills[0].description,
            "fallback"
        );
    }

    // Failure: faster directory typing loses junctions, hidden/junk exclusion,
    // depth limits, or a SKILL.md whose declared name differs from its folder.
    #[test]
    fn walker_keeps_depth_and_linked_skills() {
        let f = Fixture::new();
        let expected = f.skill("root/visible", "declared-name", "description");
        f.skill("root/.hidden", "hidden", "excluded");
        f.skill("root/node_modules/pkg", "dependency", "excluded");
        f.skill("root/too/deep", "deep", "outside depth one");
        let shallow = find_skill_mds_excluding(&f.0.join("root"), 1, &[]);
        assert_eq!(shallow, vec![expected.clone()]);
        assert_eq!(
            parse_skill_metadata(&expected).unwrap().name,
            "declared-name"
        );
        let target = f.skill("external/linked", "linked-name", "linked description");
        let link = f.0.join("root/link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(target.parent().unwrap(), &link).unwrap();
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let result = std::process::Command::new("cmd.exe")
                .args(["/C", "mklink", "/J"])
                .arg(link.to_string_lossy().replace('/', "\\"))
                .arg(
                    target
                        .parent()
                        .unwrap()
                        .to_string_lossy()
                        .replace('/', "\\"),
                )
                .creation_flags(0x08000000)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "junction creation failed: {} {}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
        }
        let paths = find_skill_mds_excluding(&f.0.join("root"), 1, &[]);
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&link.join("SKILL.md")));
    }

    // Failure: independent processes publish torn JSON or rebuild over each other.
    #[test]
    fn concurrent_processes_publish_complete_generation() {
        const ENV: &str = "NUR_CACHE_TEST_CHILD";
        if let Some(path) = std::env::var_os(ENV) {
            let root = PathBuf::from(path);
            let data = load_global_at(&root, &[root.join("skills")]);
            assert_eq!(data.skills.len(), 30);
            return;
        }
        let f = Fixture::new();
        for i in 0..30 {
            f.skill(&format!("skills/s{i}"), &format!("s{i}"), "description");
        }
        let exe = std::env::current_exe().unwrap();
        let mut children: Vec<_> = (0..3).map(|_| std::process::Command::new(&exe)
            .args(["--exact", "agent::skill_cache::tests::concurrent_processes_publish_complete_generation"])
            .env(ENV, &f.0).stdout(std::process::Stdio::null()).spawn().unwrap()).collect();
        for child in &mut children {
            assert!(child.wait().unwrap().success());
        }
        let data: SkillCacheFile =
            serde_json::from_slice(&fs::read(cache_path(&f.0)).unwrap()).unwrap();
        assert_eq!(data.file_count, 30);
        assert_eq!(data.skills.len(), 30);
    }
}

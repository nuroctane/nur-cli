//! Install the first-party skill pack into `~/.nur/skills/` so the agent
//! discovers it on first launch (also mirrored to `~/.agents/skills`).
//!
//! The pack itself is not compiled into the binary. `skills/bundled.json` (the
//! manifest, a few KB) is, so a binary knows which packs it ships; the bodies
//! come from the `nur-skills.zip` release asset of the running version,
//! verified against the minisign key pinned in `bootstrap.rs`. Debug builds
//! and `NUR_SKILL_PACK` read a checkout (or a zip) instead, which is also how
//! the E2E harness feeds a pack without the network.

use crate::config::nur_home;
use crate::error::{NurError, Result};
use serde::Deserialize;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const MANIFEST_JSON: &str = include_str!("../../skills/bundled.json");
const PACK_ASSET: &str = "nur-skills.zip";
const RELEASE_DOWNLOADS: &str = "https://github.com/nuroctane/nur-cli/releases/download";
/// Compressed pack is ~300 KB; anything near this is not our asset.
const MAX_PACK_BYTES: usize = 32 * 1024 * 1024;
const STAMP_FILE: &str = ".bundled-pack.json";

#[derive(Deserialize)]
struct Manifest {
    packs: Vec<Pack>,
}

#[derive(Deserialize, Clone)]
struct Pack {
    name: String,
    /// Repository directory the files come from (checkout sources only).
    source: String,
    files: Vec<String>,
}

#[derive(Deserialize, serde::Serialize, Default)]
struct Stamp {
    version: String,
    source: String,
    installed_at: u64,
}

/// Skills we used to ship and now retire on upgrade. Without this sweep an
/// existing install keeps discovering (and NL-activating) them forever, since
/// the installer only ever writes files. Superseded by `/takeover`.
const RETIRED_SKILLS: &[&str] = &[
    "resume-claude",
    "resume-codex",
    "resume-cursor",
    "resume-grok",
    "resume-nur",
];

/// Remove retired skill dirs, plus the SKILL.md that used to make the
/// `resume-session` reader a user-facing skill (the reader itself stays -
/// `chagent` shells out to it).
fn retire_stale_skills(root: &Path) {
    for name in RETIRED_SKILLS {
        let _ = fs::remove_dir_all(root.join(name));
    }
    let _ = fs::remove_file(root.join("resume-session").join("SKILL.md"));
}

fn manifest() -> Result<Vec<Pack>> {
    let m: Manifest = serde_json::from_str(MANIFEST_JSON)?;
    Ok(m.packs)
}

/// Where the pack bodies came from, for the stamp and `ecosystem status`.
enum PackSource {
    /// A directory laid out like the repository (`<source>/<file>` per pack).
    Checkout(PathBuf),
    /// Verified `nur-skills.zip` bytes.
    Zip(Vec<u8>),
}

impl PackSource {
    fn read(&self, pack: &Pack, file: &str) -> Result<Vec<u8>> {
        match self {
            PackSource::Checkout(root) => {
                let path = root.join(&pack.source).join(file);
                fs::read(&path).map_err(|e| NurError::Other(format!("{}: {e}", path.display())))
            }
            PackSource::Zip(bytes) => {
                let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))
                    .map_err(|e| NurError::Other(format!("{PACK_ASSET}: {e}")))?;
                let entry = format!("{}/{file}", pack.name);
                let mut f = zip
                    .by_name(&entry)
                    .map_err(|e| NurError::Other(format!("{PACK_ASSET} has no {entry}: {e}")))?;
                let mut out = Vec::with_capacity(f.size() as usize);
                f.read_to_end(&mut out)?;
                Ok(out)
            }
        }
    }
}

/// Install (or refresh) the first-party pack. Returns the pack names.
///
/// Fast path: the stamp says this version already installed and every file is
/// present. Otherwise the pack is fetched, written under `~/.nur/skills`, and
/// mirrored. A fetch failure is an error for the caller's notes; whatever an
/// earlier version installed stays usable in the meantime.
pub fn install_bundled_skills() -> Result<Vec<String>> {
    let root = nur_home().join("skills");
    fs::create_dir_all(&root)?;
    retire_stale_skills(&root);
    let packs = manifest()?;
    let names: Vec<String> = packs.iter().map(|p| p.name.clone()).collect();
    let version = env!("CARGO_PKG_VERSION");

    if !super::ecosystem_force_pub() && stamp_matches(&root, version) && all_present(&root, &packs)
    {
        return Ok(names);
    }

    let (source, origin) = locate_pack(version)?;
    write_packs(&root, &packs, &source)?;

    // Also mirror into ~/.agents/skills for cross-framework discovery.
    if let Some(home) = crate::config::home_dir() {
        let agents = home.join(".agents").join("skills");
        if fs::create_dir_all(&agents).is_ok() {
            retire_stale_skills(&agents);
            let _ = write_packs(&agents, &packs, &source);
        }
    }

    let stamp = Stamp {
        version: version.to_string(),
        source: origin.to_string(),
        installed_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    };
    fs::write(
        root.join(STAMP_FILE),
        serde_json::to_string_pretty(&stamp).unwrap_or_default(),
    )?;
    Ok(names)
}

fn stamp_matches(root: &Path, version: &str) -> bool {
    fs::read_to_string(root.join(STAMP_FILE))
        .ok()
        .and_then(|t| serde_json::from_str::<Stamp>(&t).ok())
        .is_some_and(|s| s.version == version)
}

fn all_present(root: &Path, packs: &[Pack]) -> bool {
    packs
        .iter()
        .all(|p| p.files.iter().all(|f| root.join(&p.name).join(f).is_file()))
}

/// Write every pack as `<root>/<name>/<file>`, staging each pack in a temp dir
/// so a failure mid-pack never leaves a half-written skill in place.
fn write_packs(root: &Path, packs: &[Pack], source: &PackSource) -> Result<()> {
    for pack in packs {
        let dest = root.join(&pack.name);
        let tmp = root.join(format!(".{}.tmp-{}", pack.name, std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let staged = stage_pack(pack, source, &tmp).and_then(|()| {
            // Replace wholesale so a manifest that dropped a file drops it here too.
            let _ = fs::remove_dir_all(&dest);
            fs::rename(&tmp, &dest).map_err(|e| {
                NurError::Other(format!("{}: install move failed: {e}", dest.display()))
            })
        });
        if staged.is_err() {
            let _ = fs::remove_dir_all(&tmp);
        }
        staged?;
    }
    Ok(())
}

fn stage_pack(pack: &Pack, source: &PackSource, tmp: &Path) -> Result<()> {
    fs::create_dir_all(tmp)?;
    for file in &pack.files {
        let body = source.read(pack, file)?;
        let path = tmp.join(file);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, body)?;
    }
    Ok(())
}

/// Pick where this version's pack bodies come from, in priority order:
/// `NUR_SKILL_PACK` (a zip or a checkout root), the checkout a debug build was
/// compiled in, then the signed release asset.
fn locate_pack(version: &str) -> Result<(PackSource, &'static str)> {
    if let Some(path) = std::env::var_os("NUR_SKILL_PACK").filter(|v| !v.is_empty()) {
        let path = PathBuf::from(path);
        // The override skips the signature check by design (tests and local
        // packs), so a shipped binary says so every time it is used.
        #[cfg(not(any(test, debug_assertions)))]
        crate::theme::print_info(&format!(
            "NUR_SKILL_PACK set: installing unsigned skills from {}",
            path.display()
        ));
        if path.is_dir() {
            return Ok((PackSource::Checkout(path), "NUR_SKILL_PACK"));
        }
        let bytes = fs::read(&path)
            .map_err(|e| NurError::Other(format!("NUR_SKILL_PACK {}: {e}", path.display())))?;
        return Ok((PackSource::Zip(bytes), "NUR_SKILL_PACK"));
    }
    // A shipped binary must not read whatever path it happened to be compiled
    // in (on user machines it never exists); debug builds and tests may.
    #[cfg(any(test, debug_assertions))]
    {
        let checkout = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        if checkout.join("skills").join("bundled.json").is_file() {
            return Ok((PackSource::Checkout(checkout), "checkout"));
        }
    }
    download_release_pack(version).map(|bytes| (PackSource::Zip(bytes), "release"))
}

/// Fetch `nur-skills.zip` for exactly `version` and verify it the way the
/// updater verifies a binary: pinned minisign key first (fail closed), then
/// the published sha256 when present.
fn download_release_pack(version: &str) -> Result<Vec<u8>> {
    let http = crate::bootstrap::http_client(30)?;
    let base = format!("{RELEASE_DOWNLOADS}/v{version}/{PACK_ASSET}");
    let get = |url: &str, what: &str| -> Result<Vec<u8>> {
        let bytes = http
            .get(url)
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.bytes())
            .map_err(|e| NurError::Other(format!("{what} download: {e}")))?;
        if bytes.len() > MAX_PACK_BYTES {
            return Err(NurError::Other(format!(
                "{what} is {} bytes - not a skill pack",
                bytes.len()
            )));
        }
        Ok(bytes.to_vec())
    };
    let bytes = get(&base, PACK_ASSET)?;
    if bytes.len() < 4 || &bytes[..2] != b"PK" {
        return Err(NurError::Other(format!(
            "{PACK_ASSET} is not a zip archive"
        )));
    }
    let sig = get(&format!("{base}.minisig"), "skill pack signature")?;
    let sig = String::from_utf8(sig)
        .map_err(|_| NurError::Other("skill pack signature is not text".into()))?;
    crate::bootstrap::verify_minisig_text(&bytes, &sig)?;
    if let Ok(record) = get(&format!("{base}.sha256"), "skill pack checksum") {
        let record = String::from_utf8_lossy(&record);
        let expected = crate::bootstrap::parse_sha256_record(&record).ok_or_else(|| {
            NurError::Other(format!("{PACK_ASSET}.sha256 is not a sha256 record"))
        })?;
        let actual = crate::bootstrap::sha256_hex(&bytes);
        if actual != expected {
            return Err(NurError::Other(format!(
                "{PACK_ASSET} does not match its published sha256 ({actual} != {expected})"
            )));
        }
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!("nur_skillpack_test_{nanos}_{n}"));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn pack(name: &str, files: &[&str]) -> Pack {
        Pack {
            name: name.into(),
            source: format!("skills/{name}"),
            files: files.iter().map(|f| f.to_string()).collect(),
        }
    }

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write;
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut cursor);
            for (name, body) in entries {
                w.start_file(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                w.write_all(body).unwrap();
            }
            w.finish().unwrap();
        }
        cursor.into_inner()
    }

    #[test]
    fn manifest_is_well_formed_and_every_source_exists() {
        let packs = manifest().expect("bundled.json parses");
        assert!(packs.len() >= 50, "manifest lost packs: {}", packs.len());
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut seen = std::collections::HashSet::new();
        for p in &packs {
            assert!(seen.insert(p.name.clone()), "duplicate pack {}", p.name);
            assert!(!p.files.is_empty(), "{} has no files", p.name);
            for f in &p.files {
                assert!(
                    !f.contains("..") && !f.starts_with('/'),
                    "{}: unsafe path {f}",
                    p.name
                );
                assert!(
                    root.join(&p.source).join(f).is_file(),
                    "{}: missing {}/{f}",
                    p.name,
                    p.source
                );
            }
        }
    }

    #[test]
    fn a_pack_missing_from_the_zip_fails_without_touching_the_installed_copy() {
        let root = temp_root().join("skills");
        fs::create_dir_all(root.join("a")).unwrap();
        fs::write(root.join("a/SKILL.md"), "old a").unwrap();
        fs::create_dir_all(root.join("b")).unwrap();
        fs::write(root.join("b/SKILL.md"), "old b").unwrap();

        let packs = [pack("a", &["SKILL.md"]), pack("b", &["SKILL.md", "ref.md"])];
        let good = zip_of(&[("a/SKILL.md", b"new a"), ("b/SKILL.md", b"new b")]);
        let err = write_packs(&root, &packs, &PackSource::Zip(good)).unwrap_err();
        assert!(err.to_string().contains("b/ref.md"), "{err}");
        // `a` was complete and landed; `b` was incomplete and the old copy stays.
        assert_eq!(
            fs::read_to_string(root.join("a/SKILL.md")).unwrap(),
            "new a"
        );
        assert_eq!(
            fs::read_to_string(root.join("b/SKILL.md")).unwrap(),
            "old b"
        );
        assert!(!root.join("b/ref.md").exists());
        assert!(
            fs::read_dir(&root).unwrap().all(|e| !e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp-")),
            "staging dir leaked"
        );
    }

    #[test]
    fn a_stale_file_disappears_when_the_manifest_drops_it() {
        let root = temp_root().join("skills");
        fs::create_dir_all(root.join("a/references")).unwrap();
        fs::write(root.join("a/SKILL.md"), "old").unwrap();
        fs::write(root.join("a/references/gone.md"), "stale").unwrap();
        let packs = [pack("a", &["SKILL.md"])];
        write_packs(
            &root,
            &packs,
            &PackSource::Zip(zip_of(&[("a/SKILL.md", b"new")])),
        )
        .unwrap();
        assert_eq!(fs::read_to_string(root.join("a/SKILL.md")).unwrap(), "new");
        assert!(!root.join("a/references/gone.md").exists());
    }

    #[test]
    fn stamp_fast_path_needs_both_the_version_and_every_file() {
        let root = temp_root();
        let packs = [pack("a", &["SKILL.md", "ref.md"])];
        assert!(!stamp_matches(&root, "1.0.0"), "no stamp yet");
        fs::write(
            root.join(STAMP_FILE),
            serde_json::to_string(&Stamp {
                version: "1.0.0".into(),
                ..Default::default()
            })
            .unwrap(),
        )
        .unwrap();
        assert!(stamp_matches(&root, "1.0.0"));
        assert!(!stamp_matches(&root, "1.0.1"), "a new binary reinstalls");
        assert!(!all_present(&root, &packs), "stamp alone is not enough");
        fs::create_dir_all(root.join("a")).unwrap();
        fs::write(root.join("a/SKILL.md"), "x").unwrap();
        assert!(
            !all_present(&root, &packs),
            "one deleted file forces a reinstall"
        );
        fs::write(root.join("a/ref.md"), "y").unwrap();
        assert!(all_present(&root, &packs));
    }

    #[test]
    fn checkout_source_reads_from_the_manifest_directory_not_the_pack_name() {
        let dir = temp_root();
        let src = dir.join("skills/nur/excalidraw");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("SKILL.md"), "body").unwrap();
        let p = Pack {
            name: "excalidraw".into(),
            source: "skills/nur/excalidraw".into(),
            files: vec!["SKILL.md".into()],
        };
        let source = PackSource::Checkout(dir.clone());
        assert_eq!(source.read(&p, "SKILL.md").unwrap(), b"body");
    }
}

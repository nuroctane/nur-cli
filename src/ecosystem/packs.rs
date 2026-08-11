//! Third-party skill packs + companion CLIs installed into Meta's skill roots.
//!
//! Packs are provisioned once during `ecosystem ensure` / one-shot install so the
//! agent has design, clone-website, cybersecurity, and OpenCode catalog knowledge
//! without any manual `npx skills add` steps.

use super::{find_bin, run_capture, run_quiet, which, ComponentStatus};
use crate::config::nur_home;
use std::fs;
use std::path::PathBuf;

/// Skill sources installed via the `skills` CLI (vercel-labs/skills).
const SKILL_PACKS: &[(&str, &str)] = &[
    // Emil Kowalski - design engineering / animation taste
    ("emilkowalski/skills", "design"),
    // Website reverse-engineering skill (clone-website)
    ("JCodesMore/ai-website-cloner-template", "clone-website"),
    // 817 cybersecurity skills (MITRE/NIST mapped)
    ("mukul975/Anthropic-Cybersecurity-Skills", "cybersecurity"),
    // Also land core engineering packs into ~/.agents via skills CLI when available
    // (plugins path below is primary; this dual-writes for Agent Skills compat).
    ("mattpocock/skills", "mattpocock"),
    ("addyosmani/agent-skills", "addyosmani"),
    ("BuilderIO/skills", "builderio"),
    // Marketing + software factory (factory overnight prefers fractal)
    (
        "MikeFishbeinAtherial/infinite-headcount",
        "infinite-headcount",
    ),
    // STEP-first CAD/robotics/fabrication workflows and their local scripts.
    ("earthtojake/text-to-cad", "text-to-cad"),
    // Portable Android/iOS/cloud-phone operating harness for mobilerun-core.
    ("droidrun/mobile-harness", "mobile-harness"),
];

pub fn ensure_skills_cli(node_ok: bool) -> ComponentStatus {
    let mut c = ComponentStatus {
        name: "skills".into(),
        ..Default::default()
    };
    if !node_ok {
        c.detail = "needs Node.js".into();
        return c;
    }
    if find_bin("skills").is_none() {
        let _ = run_quiet("npm", &["install", "-g", "skills@latest"], None, 300_000);
    }
    if let Some(bin) = find_bin("skills") {
        c.available = true;
        c.path = Some(bin.clone());
        c.version = super::cmd_version_pub(&bin, &["--version"]);
        c.detail = "open agent skills CLI ready".into();
    } else {
        c.detail = "not found — npm i -g skills".into();
    }
    c
}

pub fn ensure_akm(node_ok: bool) -> ComponentStatus {
    let mut c = ComponentStatus {
        name: "akm".into(),
        ..Default::default()
    };
    if !node_ok {
        c.detail = "needs Node.js".into();
        return c;
    }
    // akm-cli ships a bun wrapper on Windows; also try running via node.
    if find_bin("akm").is_none() {
        let _ = run_quiet("npm", &["install", "-g", "akm-cli@latest"], None, 300_000);
    }
    // Prefer bun if present (akm's native runtime).
    if !which("bun") && !which("bun.exe") {
        // Optional: install bun silently (best-effort).
        let _ = run_quiet(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "irm https://bun.sh/install.ps1 | iex",
            ],
            None,
            180_000,
        );
    }
    if let Some(bin) = find_bin("akm") {
        // Probe with node fallback if bun wrapper fails.
        let ok = run_quiet(&bin, &["--version"], None, 15_000)
            || run_via_node_akm(&["--version"]).is_ok();
        c.available = ok;
        c.path = Some(bin);
        c.detail = if ok {
            "agent knowledge manager ready".into()
        } else {
            "installed but needs bun runtime (https://bun.sh)".into()
        };
    } else {
        c.detail = "not found — npm i -g akm-cli".into();
    }
    c
}

fn run_via_node_akm(args: &[&str]) -> Result<String, String> {
    // npm global: .../node_modules/akm-cli/dist/cli.js
    let home = dirs::home_dir().ok_or("no home")?;
    let candidates = [
        home.join("AppData")
            .join("Roaming")
            .join("npm")
            .join("node_modules")
            .join("akm-cli")
            .join("dist")
            .join("cli.js"),
        PathBuf::from("/usr/local/lib/node_modules/akm-cli/dist/cli.js"),
        home.join(".npm-global")
            .join("lib")
            .join("node_modules")
            .join("akm-cli")
            .join("dist")
            .join("cli.js"),
    ];
    for p in candidates {
        if p.is_file() {
            let mut full = vec![p.to_string_lossy().to_string()];
            full.extend(args.iter().map(|s| s.to_string()));
            let refs: Vec<&str> = full.iter().map(|s| s.as_str()).collect();
            return run_capture("node", &refs, None, 60_000);
        }
    }
    Err("akm-cli js entry not found".into())
}

pub fn ensure_executor(node_ok: bool) -> ComponentStatus {
    let mut c = ComponentStatus {
        name: "executor".into(),
        ..Default::default()
    };
    if !node_ok {
        c.detail = "needs Node.js 20+".into();
        return c;
    }

    // Always refresh to @latest on ensure (schema bumps force this).
    let npm = find_bin("npm").unwrap_or_else(|| "npm".into());
    match run_capture(&npm, &["install", "-g", "executor@latest"], None, 300_000) {
        Ok(_) => {}
        Err(e) => {
            c.detail = format!(
                "npm install failed: {}",
                e.chars().take(200).collect::<String>()
            );
            // Still try to locate a partial install.
        }
    }

    if let Some(bin) = find_bin("executor") {
        c.available = true;
        c.path = Some(bin.clone());
        c.version = super::cmd_version_pub(&bin, &["--version"]);
        // Durable local service (best-effort — non-fatal if service already running).
        let _ = run_quiet(&bin, &["install"], None, 90_000);
        c.detail = "MCP gateway ready (executor · local :4788/mcp)".into();
    } else if c.detail.is_empty() {
        c.detail = "not found after npm install - try: npm i -g executor".into();
    }
    c
}

/// GraphJin — governed GraphQL→SQL over live databases (`graphjin` tool).
///
/// **Detect-only, deliberately.** Every other component here is auto-installed
/// because it is useful the moment it exists. GraphJin is not: without a
/// `config/` pointing at a real database it does nothing, and it is a far
/// heavier install than a memory CLI. Pulling it onto every machine that runs
/// `nur ecosystem ensure` would be presumptuous. So we report presence and how
/// to get it, and let the user opt in.
pub fn ensure_graphjin() -> ComponentStatus {
    let mut c = ComponentStatus {
        name: "graphjin".into(),
        ..Default::default()
    };
    match find_bin("graphjin") {
        Some(bin) => {
            c.available = true;
            c.version = super::cmd_version_pub(&bin, &["version"]);
            c.path = Some(bin);
            c.detail =
                "governed data surface ready — point GRAPHJIN_CONFIG_PATH at a config".into();
        }
        None => {
            c.detail =
                "optional — npm i -g graphjin (needed only for the `graphjin` data tool)".into();
        }
    }
    c
}

/// Oh My Pi (omp.sh) - the coding-agent backend the `omp` tool delegates to.
/// (headless `omp -p` runs; we deliberately skip its IDE/ACP surface).
/// Ships as `@oh-my-pi/pi-coding-agent` (Bun) and as a native installer under
/// `%LOCALAPPDATA%\omp\`. Auto-upgrades when below the feature floor.
pub fn ensure_omp() -> ComponentStatus {
    let mut c = ComponentStatus {
        name: "omp".into(),
        ..Default::default()
    };

    let current = best_omp();
    let needs_upgrade = match &current {
        None => true,
        Some((_, ver)) => !omp_meets_feature_floor(ver),
    };
    if needs_upgrade {
        upgrade_omp(&mut c);
    }

    match best_omp() {
        Some((bin, version)) => {
            c.path = Some(bin);
            c.version = Some(version.clone());
            if omp_meets_feature_floor(&version) {
                c.available = true;
                c.detail = format!(
                    "coding-agent backend ready (omp {version}; economy routing + metered delegation)"
                );
            } else {
                c.detail = format!(
                    "omp {version} is too old; need >= {}.{}.{} - bun i -g @oh-my-pi/pi-coding-agent@latest \
                     (Windows: irm https://omp.sh/install.ps1 | iex)",
                    OMP_FEATURE_FLOOR.0, OMP_FEATURE_FLOOR.1, OMP_FEATURE_FLOOR.2
                );
            }
        }
        None => {
            if c.detail.is_empty() {
                c.detail =
                    "not found after install - try: bun i -g @oh-my-pi/pi-coding-agent@latest \
                     (or irm https://omp.sh/install.ps1 | iex)"
                        .into();
            }
        }
    }
    c
}

const OMP_FEATURE_FLOOR: (u64, u64, u64) = (17, 2, 0);
const BUN_OMP_FLOOR: (u64, u64, u64) = (1, 3, 14);

fn upgrade_omp(c: &mut ComponentStatus) {
    if let Some(bun) = find_bin("bun") {
        let bun_version = super::cmd_version_pub(&bun, &["--version"]);
        if bun_version.as_deref().is_some_and(bun_meets_omp_floor) {
            if let Err(e) = run_capture(
                &bun,
                &["install", "-g", "@oh-my-pi/pi-coding-agent@latest"],
                None,
                300_000,
            ) {
                c.detail = format!(
                    "bun install failed: {}",
                    e.chars().take(160).collect::<String>()
                );
            }
        } else if c.detail.is_empty() {
            c.detail = format!(
                "needs Bun >= 1.3.14; found {}",
                bun_version.as_deref().unwrap_or("an unreadable version")
            );
        }
    }

    #[cfg(windows)]
    {
        // Official installer refreshes installers; often lands via Bun now.
        let ps = "irm https://omp.sh/install.ps1 | iex";
        let _ = run_capture(
            "powershell",
            &["-NoProfile", "-NonInteractive", "-Command", ps],
            None,
            300_000,
        );
        // Stale native `%LOCALAPPDATA%\omp\omp.exe` can still win PATH over Bun.
        // Self-update it when below the feature floor.
        if let Some(home) = dirs::home_dir() {
            let local = home
                .join("AppData")
                .join("Local")
                .join("omp")
                .join("omp.exe");
            if local.is_file() {
                let path = local.to_string_lossy().into_owned();
                let ver = super::cmd_version_pub(&path, &["--version"]).unwrap_or_default();
                if !omp_meets_feature_floor(&ver) {
                    let _ = run_capture(&path, &["update", "--force"], None, 300_000);
                }
            }
        }
    }

    #[cfg(not(windows))]
    {
        if best_omp()
            .as_ref()
            .is_none_or(|(_, v)| !omp_meets_feature_floor(v))
        {
            let _ = run_capture(
                "bash",
                &["-lc", "curl -fsSL https://omp.sh/install.sh | bash"],
                None,
                300_000,
            );
        }
    }

    if find_bin("bun").is_none() && best_omp().is_none() && c.detail.is_empty() {
        c.detail =
            "needs Bun >= 1.3.14 (bun.sh), or run: irm https://omp.sh/install.ps1 | iex".into();
    }
}

/// Prefer the newest omp among Bun global, official Local install, and PATH.
pub(crate) fn best_omp() -> Option<(String, String)> {
    use std::collections::HashSet;
    use std::process::Command;

    let mut seen = HashSet::new();
    let mut best: Option<(String, String, (u64, u64, u64))> = None;

    let mut consider = |path: String| {
        let key = path.to_ascii_lowercase();
        if !seen.insert(key) {
            return;
        }
        if !std::path::Path::new(&path).is_file() {
            return;
        }
        let Some(ver) = super::cmd_version_pub(&path, &["--version"]) else {
            return;
        };
        let Some(trip) = semver_triplet(&ver) else {
            return;
        };
        match &best {
            None => best = Some((path, ver, trip)),
            Some((_, _, cur)) if trip > *cur => best = Some((path, ver, trip)),
            _ => {}
        }
    };

    if let Some(home) = dirs::home_dir() {
        for p in [
            home.join(".bun").join("bin").join("omp.exe"),
            home.join(".bun").join("bin").join("omp"),
            home.join("AppData")
                .join("Local")
                .join("omp")
                .join("omp.exe"),
            home.join(".local").join("bin").join("omp.exe"),
            home.join(".local").join("bin").join("omp"),
        ] {
            if p.is_file() {
                consider(p.to_string_lossy().into_owned());
            }
        }
    }
    if let Some(p) = find_bin("omp") {
        consider(p);
    }
    #[cfg(windows)]
    {
        if let Ok(out) = Command::new("where.exe").arg("omp").output() {
            if out.status.success() {
                for line in String::from_utf8_lossy(&out.stdout).lines() {
                    let p = line.trim();
                    if !p.is_empty() {
                        consider(p.to_string());
                    }
                }
            }
        }
    }

    best.map(|(path, ver, _)| (path, ver))
}

fn bun_meets_omp_floor(version: &str) -> bool {
    semver_triplet(version).is_some_and(|version| version >= BUN_OMP_FLOOR)
}

fn omp_meets_feature_floor(version: &str) -> bool {
    semver_triplet(version).is_some_and(|version| version >= OMP_FEATURE_FLOOR)
}

fn semver_triplet(version: &str) -> Option<(u64, u64, u64)> {
    let numeric = version
        .trim()
        .trim_start_matches("omp/")
        .trim_start_matches('v');
    let parsed = numeric
        .split('.')
        .take(3)
        .map(|part| {
            part.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<u64>()
                .ok()
        })
        .collect::<Option<Vec<_>>>()?;
    (parsed.len() == 3).then(|| (parsed[0], parsed[1], parsed[2]))
}

/// agent-browser-cli — real-Chrome perception & control bridge for the
/// `browser` tool (github.com/sleepinginsummer/agent-browser-cli).
/// npm-installed; its Chrome MV3 extension must be loaded once by the user.
/// `install-skill` drops its SOP into ~/.agents/skills, which Meta discovers.
pub fn ensure_browser_cli(node_ok: bool) -> ComponentStatus {
    let mut c = ComponentStatus {
        name: "browser".into(),
        ..Default::default()
    };
    if !node_ok {
        c.detail = "needs Node.js 20+".into();
        return c;
    }

    let npm = find_bin("npm").unwrap_or_else(|| "npm".into());
    match run_capture(
        &npm,
        &["install", "-g", "@sleepinsummer/agent-browser-cli@latest"],
        None,
        300_000,
    ) {
        Ok(_) => {}
        Err(e) => {
            c.detail = format!(
                "npm install failed: {}",
                e.chars().take(200).collect::<String>()
            );
        }
    }

    if let Some(bin) = find_bin("agent-browser-cli") {
        c.available = true;
        c.path = Some(bin.clone());
        c.version = super::cmd_version_pub(&bin, &["--version"]);
        // Drop the usage SOP into ~/.agents/skills (best-effort).
        let _ = run_quiet(&bin, &["install-skill"], None, 60_000);
        // Stage the extension out of the npm package so nothing must be
        // downloaded, and target whatever browser the user actually uses.
        let staged = super::browser_setup::stage_extension_from_cli().is_some();
        let browser = super::browser_setup::detect_default_browser();
        c.detail = if staged {
            format!(
                "real-Chrome bridge ready · default {} · extension staged (load once: `nur browser setup`)",
                browser.label()
            )
        } else {
            format!(
                "real-Chrome bridge ready · default {} · run `nur browser setup` to finish",
                browser.label()
            )
        };
    } else if c.detail.is_empty() {
        c.detail =
            "not found after npm install - try: npm i -g @sleepinsummer/agent-browser-cli".into();
    }
    c
}

/// Install curated skill packs into ~/.agents/skills (Meta discovers this).
pub fn install_skill_packs(skills_cli: &ComponentStatus) -> (Vec<String>, Vec<String>) {
    let mut ok = Vec::new();
    let mut notes = Vec::new();

    // Always write thin catalog skills (even if network fails).
    if let Err(e) = write_catalog_skills() {
        notes.push(format!("catalog skills: {e}"));
    } else {
        ok.push("catalogs".into());
    }

    if !skills_cli.available {
        notes.push("skills CLI missing — pack install deferred".into());
        return (ok, notes);
    }
    let Some(bin) = find_bin("skills") else {
        return (ok, notes);
    };

    for (source, label) in SKILL_PACKS {
        // Skip re-install if a marker file says we already have this pack.
        let marker = pack_marker(label);
        if marker.is_file() {
            ok.push((*label).into());
            continue;
        }
        // skills add <source> -g -a agents -y --copy
        // Design + cyber: install all skills in the repo.
        // Clone-website: full-depth search for nested SKILL.md under .claude/skills.
        let args = skill_pack_install_args(source, label);
        match run_capture(&bin, &args, None, 600_000) {
            Ok(out) => {
                let _ = fs::create_dir_all(marker.parent().unwrap());
                let _ = fs::write(
                    &marker,
                    format!(
                        "source={source}\ninstalled_at={}\n{}\n",
                        chrono_now(),
                        out.chars().take(500).collect::<String>()
                    ),
                );
                // Mirror complete skill directories into ~/.nur/skills so scripts,
                // platform guides, and progressive references remain usable.
                mirror_agents_to_nur();
                ok.push((*label).into());
            }
            Err(e) => {
                notes.push(format!("{label}: {e}"));
                // Still mark attempted to avoid hammering on every launch.
                let _ = fs::create_dir_all(marker.parent().unwrap());
                let _ = fs::write(&marker, format!("attempted_error={e}\n"));
            }
        }
    }

    (ok, notes)
}

fn skill_pack_install_args<'a>(source: &'a str, label: &'a str) -> Vec<&'a str> {
    let mut args = vec!["add", source, "-g", "-a", "agents", "-y", "--copy"];
    if label == "clone-website" {
        args.extend(["--full-depth", "-s", "clone-website"]);
    } else {
        // Do not use `--all`: skills CLI defines it as both `--skill '*'` and
        // `--agent '*'`, which would install into dozens of unrelated agents.
        args.extend(["-s", "*"]);
    }
    args
}

fn pack_marker(label: &str) -> PathBuf {
    nur_home().join("skill-packs").join(format!("{label}.ok"))
}

fn chrono_now() -> String {
    // Avoid adding chrono dep here — use system time display.
    use std::time::{SystemTime, UNIX_EPOCH};
    let s = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    s.to_string()
}

fn mirror_agents_to_nur() {
    let Some(home) = dirs::home_dir() else { return };
    let agents = home.join(".agents").join("skills");
    let nur = nur_home().join("skills");
    if !agents.is_dir() {
        return;
    }
    let _ = fs::create_dir_all(&nur);
    // Recursive: cyber is flat, mattpocock is skills/<cat>/<name>/SKILL.md when
    // installed under agents as nested trees.
    for skill_md in crate::agent::skills::find_skill_mds(&agents, 5) {
        let Some(src_dir) = skill_md.parent() else {
            continue;
        };
        let name = src_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("skill");
        let dest = nur.join(name);
        // Never overwrite a primary ~/.nur copy, but fill every missing file.
        // Root-level harnesses such as mobile-harness depend on sibling trees
        // (`platforms/`, `core/`, `apps/`), while CAD skills ship executable
        // `scripts/`; copying only SKILL.md makes both integrations unusable.
        let _ = mirror_missing_tree(src_dir, &dest);
    }
    crate::agent::skill_cache::invalidate_cache();
}

fn mirror_missing_tree(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        let name = entry.file_name();
        let name_text = name.to_string_lossy();
        if file_type.is_dir()
            && matches!(
                name_text.as_ref(),
                ".git" | ".venv" | "node_modules" | "target" | "__pycache__"
            )
        {
            continue;
        }
        let target = destination.join(&name);
        if file_type.is_dir() {
            mirror_missing_tree(&entry.path(), &target)?;
        } else if file_type.is_file() && !target.exists() {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Catalog / index skills that point the agent at large packs without loading
/// 817 full playbooks into every prompt.
fn write_catalog_skills() -> Result<(), String> {
    let root = nur_home().join("skills");
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;

    let catalogs: &[(&str, &str)] = &[
        ("opencode-awesome", OPENCODE_AWESOME_SKILL),
        ("design-eng", DESIGN_ENG_ROUTER),
        ("clone-website-meta", CLONE_WEBSITE_ROUTER),
        ("cybersecurity", CYBER_ROUTER),
        ("context-pruning", DCP_ROUTER),
        ("executor-gateway", EXECUTOR_ROUTER),
        ("akm-manager", AKM_ROUTER),
    ];

    for (name, body) in catalogs {
        let dir = root.join(name);
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        fs::write(dir.join("SKILL.md"), body).map_err(|e| e.to_string())?;
    }

    // Dual-write to ~/.agents/skills
    if let Some(home) = dirs::home_dir() {
        let agents = home.join(".agents").join("skills");
        let _ = fs::create_dir_all(&agents);
        for (name, body) in catalogs {
            let dir = agents.join(name);
            let _ = fs::create_dir_all(&dir);
            let _ = fs::write(dir.join("SKILL.md"), body);
        }
    }
    Ok(())
}

// ── Catalog skill bodies ──────────────────────────────────────────────────

const OPENCODE_AWESOME_SKILL: &str = r#"---
name: opencode-awesome
description: "Curated OpenCode ecosystem index (plugins, agents, themes). Use when the user asks for OpenCode plugins, multi-agent harnesses, or context tools."
---

# Awesome OpenCode catalog

Meta ships a pointer to the curated list at
https://github.com/awesome-opencode/awesome-opencode

## High-value picks for Meta users

| Plugin / project | Why it matters |
|------------------|----------------|
| **Dynamic Context Pruning (DCP)** | Token savings via compress/dedupe — Meta also has native auto-compact |
| **Oh My Opencode / Slim** | Multi-agent orchestration patterns |
| **Agent Memory / Honcho / Lemma** | Persistent memory (Meta already has PLUR + Ruflo) |
| **FlowDeck / GoopSpec** | Spec-driven multi-phase workflows |
| **Safety Net / EnvSitter** | Destructive-command guards |

## How to use in Meta

- Meta is not OpenCode — do not try to `opencode plugin install`.
- Steal **patterns**: multi-agent topology, compress-before-continue, safety hooks.
- For actual skills, use Meta's `skill` tool / `/skills` — packs are pre-installed.
- Full list: web_fetch the awesome-opencode README when you need the latest plugins.
"#;

const DESIGN_ENG_ROUTER: &str = r#"---
name: design-eng
description: "Emil Kowalski design-engineering & animation skills. Use for UI polish, motion review, easing/duration decisions, and avoiding animation slop."
---

# Design engineering (Emil Kowalski)

Installed from https://github.com/emilkowalski/skills via Meta ecosystem ensure.

## Skills (load with skill tool when needed)

- **emil-design-eng** — core philosophy, easing tables, review format (Before/After/Why table)
- **review-animations** — strict animation review
- **improve-animations** — codebase audit → prioritized plans in `plans/`
- **animation-vocabulary** — precise motion language for prompts
- **apple-design** — Apple WWDC motion principles for the web

## When to activate

UI work, component polish, motion bugs, "make it feel premium", shadcn/radix animations.

## Quick rules (always-on taste)

- Never animate keyboard-triggered actions used 100×/day
- Prefer `ease-out` custom curves; never `ease-in` for UI entry
- UI animations < 300ms; buttons get `:active { scale(0.97) }`
- Never `scale(0)` — start at ≥0.95 + opacity
- `transition: transform/opacity` only — not `all`, not layout props
"#;

const CLONE_WEBSITE_ROUTER: &str = r#"---
name: clone-website-meta
description: "Pixel-perfect website reverse-engineering pipeline. Use when the user wants to clone, replicate, or rebuild a live site into Next.js."
---

# Clone website

Source: https://github.com/JCodesMore/ai-website-cloner-template

## Activation

User says: clone this site, reverse-engineer URL, pixel-perfect rebuild, copy this page.

## Prerequisites

1. Prefer a project scaffolded from the template (Next.js 16 + shadcn + Tailwind v4).
   If missing: `npx create-next-app` or clone the template into a new dir.
2. Browser automation (Playwright/Chrome MCP) — without it, use web_fetch + screenshots best-effort.
3. Full skill: `skill(action=read, name=clone-website)` if installed under skills dirs.

## Pipeline summary

1. Recon — screenshots, design tokens, interaction sweep (scroll before click)
2. Foundation — fonts, globals.css tokens, icons, asset download
3. Spec files in `docs/research/components/*.spec.md` (mandatory before build)
4. Parallel section builders (small scopes, exact getComputedStyle values)
5. Assembly + visual QA

## Meta tooling

- web_fetch / bash for downloads
- multi_edit / apply_patch for components
- agent(subagent_type=general) for parallel sections
- Never phishing/impersonation — lawful use only
"#;

const CYBER_ROUTER: &str = r#"---
name: cybersecurity
description: "Router into 817 Anthropic-Cybersecurity-Skills (MITRE ATT&CK, NIST CSF, ATLAS, D3FEND, AI RMF, F3). Use for security investigations, DFIR, red/blue team playbooks."
---

# Cybersecurity skills library

Source: https://github.com/mukul975/Anthropic-Cybersecurity-Skills (Apache-2.0, community).

**Authorized & lawful use only.** Offensive skills are for systems you own or have written permission to test.

## How Meta uses this pack

- Full skill bodies live under `~/.agents/skills/` (and mirrors) after ecosystem ensure.
- Do **not** load all 817 into context. Progressive disclosure:
  1. Match the user task to a skill **name** via list/grep of skill dirs or index.
  2. `skill(action=read, name=<kebab-name>)` for the full playbook.
  3. Execute workflow steps with bash/read tools; map findings to ATT&CK IDs.

## Domains (29)

Cloud · Threat Hunting · Threat Intel · Network · Web App · DFIR · Malware · IAM · SOC · Red Team · Containers · OT/ICS · API · IR · Vuln Mgmt · Pentest · DevSecOps · Zero Trust · Endpoint · Crypto · Phishing · AI Security · Mobile · Ransomware · Compliance · Supply Chain · Deception · Hardware/Firmware

## Example matches

| User ask | Skill to load |
|----------|----------------|
| memory dump credential theft | performing-memory-forensics-with-volatility3 |
| S3 public buckets | auditing-aws-s3-bucket-permissions |
| prompt injection | detecting-ai-model-prompt-injection-attacks |
| kerberoasting | detecting-kerberoasting-attacks |

Index: https://raw.githubusercontent.com/mukul975/Anthropic-Cybersecurity-Skills/main/index.json
"#;

const DCP_ROUTER: &str = r#"---
name: context-pruning
description: "Dynamic context pruning patterns (OpenCode DCP / Sleev). Meta has native auto-compact; use these rules to manage long sessions."
---

# Context pruning (DCP-inspired)

Upstream: https://github.com/Opencode-DCP/opencode-dynamic-context-pruning  
Successor focus: https://sleev.ai (`npm i -g sleev`)

OpenCode's DCP plugin is **OpenCode-specific**. Meta implements the same goals natively:

## Meta native behavior

- Auto-compact when context pressure is high (~55% of window, once per turn)
- Manual `/compact` slash command
- Tool results are capped; prefer re-query over replaying huge dumps

## Practices for long sessions

1. After a milestone, summarize and drop raw tool blobs (user can `/compact`)
2. Prefer graphify/plur recall over re-grepping the whole repo
3. Don't re-read files already summarized unless editing
4. Parallel read-only tools only — mutating tools stay sequential
5. If using OpenCode elsewhere: `opencode plugin @tarquinen/opencode-dcp@latest --global`

## Compress modes (conceptual)

- **range** — compress a span of turns into one summary
- **dedupe** — identical tool+args keep latest output only
- **purge errors** — drop large error inputs after N turns
"#;

const EXECUTOR_ROUTER: &str = r#"---
name: executor-gateway
description: "Executor MCP gateway — one catalog for OpenAPI/GraphQL/MCP integrations shared across agents. Use for external APIs, multi-agent tool routing, policies."
---

# Executor (executor.sh)

Docs: https://executor.sh/docs  
CLI: `npm i -g executor` (Meta auto-installs)

## What it is

Local (or cloud) MCP gateway: configure integrations once, every agent gets the same tools with shared auth + policies.

## Meta integration

- Tool: `executor` (status / tools search / call / sources)
- Service: `executor install` starts durable local daemon
- MCP HTTP: `http://127.0.0.1:4788/mcp` (stdio: `executor mcp`)
- Prefer Meta's native tools for repo work; use Executor for **external SaaS/APIs**

## Common commands

```
executor tools sources
executor tools search "send email"
executor call <namespace> <tool> '<json>'
executor web          # UI at :4788
```
"#;

const AKM_ROUTER: &str = r#"---
name: akm-manager
description: "AKM (Agent Knowledge Management) — package manager for skills/commands/tools across Claude/OpenCode/Cursor."
---

# AKM CLI

npm: `akm-cli` · binary `akm`  
Meta auto-installs; may need [Bun](https://bun.sh) on Windows.

## Use

- Discover / install / update skill packages across agents
- Complements Meta's `skills` CLI and built-in skill loader
- Prefer Meta `skill` tool for day-to-day; use AKM when managing multi-agent skill libraries

```
akm --help
akm list
akm install <package>
```
"#;

#[cfg(test)]
mod tests {
    use super::{
        bun_meets_omp_floor, mirror_missing_tree, omp_meets_feature_floor, skill_pack_install_args,
        SKILL_PACKS,
    };

    #[test]
    fn omp_bun_floor_is_enforced() {
        assert!(!bun_meets_omp_floor("1.3.13"));
        assert!(bun_meets_omp_floor("1.3.14"));
        assert!(bun_meets_omp_floor("2.0.0"));
        assert!(!bun_meets_omp_floor("unknown"));
    }

    #[test]
    fn omp_feature_floor_is_enforced() {
        assert!(!omp_meets_feature_floor("omp/16.3.5"));
        assert!(!omp_meets_feature_floor("omp/17.1.4"));
        assert!(omp_meets_feature_floor("omp/17.2.0"));
        assert!(omp_meets_feature_floor("omp/18.0.0"));
    }

    #[test]
    fn default_packs_include_cad_and_mobile_harness() {
        assert!(SKILL_PACKS.contains(&("earthtojake/text-to-cad", "text-to-cad")));
        assert!(SKILL_PACKS.contains(&("droidrun/mobile-harness", "mobile-harness")));
    }

    #[test]
    fn pack_install_targets_only_the_agents_root() {
        let args = skill_pack_install_args("earthtojake/text-to-cad", "text-to-cad");
        assert!(args.windows(2).any(|pair| pair == ["-a", "agents"]));
        assert!(args.windows(2).any(|pair| pair == ["-s", "*"]));
        assert!(!args.contains(&"--all"));
    }

    #[test]
    fn mirror_preserves_complete_resource_tree_without_overwriting_primary_skill() {
        let root = std::env::temp_dir().join(format!(
            "nur-skill-mirror-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let source = root.join("source");
        let destination = root.join("destination");
        std::fs::create_dir_all(source.join("platforms").join("android")).unwrap();
        std::fs::create_dir_all(source.join("scripts")).unwrap();
        std::fs::create_dir_all(&destination).unwrap();
        std::fs::write(source.join("SKILL.md"), "upstream").unwrap();
        std::fs::write(
            source.join("platforms").join("android").join("GUIDE.md"),
            "android guide",
        )
        .unwrap();
        std::fs::write(source.join("scripts").join("inspect.py"), "print('ok')").unwrap();
        std::fs::write(destination.join("SKILL.md"), "primary").unwrap();

        mirror_missing_tree(&source, &destination).unwrap();

        assert_eq!(
            std::fs::read_to_string(destination.join("SKILL.md")).unwrap(),
            "primary"
        );
        assert!(destination
            .join("platforms")
            .join("android")
            .join("GUIDE.md")
            .is_file());
        assert!(destination.join("scripts").join("inspect.py").is_file());
        let _ = std::fs::remove_dir_all(root);
    }
}

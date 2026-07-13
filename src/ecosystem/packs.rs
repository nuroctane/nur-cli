//! Third-party skill packs + companion CLIs installed into Meta's skill roots.
//!
//! Packs are provisioned once during `ecosystem ensure` / one-shot install so the
//! agent has design, clone-website, cybersecurity, and OpenCode catalog knowledge
//! without any manual `npx skills add` steps.

use super::{find_bin, run_capture, run_quiet, which, ComponentStatus};
use crate::config::muse_home;
use std::fs;
use std::path::PathBuf;

/// Skill sources installed via the `skills` CLI (vercel-labs/skills).
const SKILL_PACKS: &[(&str, &str)] = &[
    // Emil Kowalski — design engineering / animation taste
    ("emilkowalski/skills", "design"),
    // Website reverse-engineering skill (clone-website)
    ("JCodesMore/ai-website-cloner-template", "clone-website"),
    // 817 cybersecurity skills (MITRE/NIST mapped)
    ("mukul975/Anthropic-Cybersecurity-Skills", "cybersecurity"),
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

    // Install if missing (use resolved npm path — bare "npm" fails on Windows).
    if find_bin("executor").is_none() {
        let npm = find_bin("npm").unwrap_or_else(|| "npm".into());
        match run_capture(
            &npm,
            &["install", "-g", "executor@latest"],
            None,
            300_000,
        ) {
            Ok(_) => {}
            Err(e) => {
                c.detail = format!("npm install failed: {}", e.chars().take(200).collect::<String>());
                // Still try to locate a partial install.
            }
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
        c.detail = "not found after npm install — try: npm i -g executor".into();
    }
    c
}

/// Oh My Pi (omp.sh) — the coding-agent *backend* the `omp` tool delegates to
/// (headless `omp -p` runs; we deliberately skip its IDE/ACP surface).
/// Ships on npm as @oh-my-pi/pi-coding-agent but runs on Bun, so install via
/// bun when present; otherwise report how to get it without failing ensure.
pub fn ensure_omp() -> ComponentStatus {
    let mut c = ComponentStatus {
        name: "omp".into(),
        ..Default::default()
    };

    if find_bin("omp").is_none() {
        if let Some(bun) = find_bin("bun") {
            match run_capture(
                &bun,
                &["install", "-g", "@oh-my-pi/pi-coding-agent"],
                None,
                300_000,
            ) {
                Ok(_) => {}
                Err(e) => {
                    c.detail = format!(
                        "bun install failed: {}",
                        e.chars().take(200).collect::<String>()
                    );
                }
            }
        } else {
            c.detail =
                "needs Bun (bun.sh) — or: irm https://omp.sh/install.ps1 | iex".into();
            return c;
        }
    }

    if let Some(bin) = find_bin("omp") {
        c.available = true;
        c.path = Some(bin.clone());
        c.version = super::cmd_version_pub(&bin, &["--version"]);
        c.detail = "coding-agent backend ready (omp.sh · `omp` tool)".into();
    } else if c.detail.is_empty() {
        c.detail = "not found after install — try: bun i -g @oh-my-pi/pi-coding-agent".into();
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
        let args: Vec<&str> = if *label == "clone-website" {
            vec![
                "add",
                *source,
                "-g",
                "-a",
                "agents",
                "-y",
                "--copy",
                "--full-depth",
                "-s",
                "clone-website",
            ]
        } else {
            vec![
                "add", *source, "-g", "-a", "agents", "-y", "--copy", "--all",
            ]
        };
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
                // Mirror into ~/.meta/skills for dual discovery
                mirror_agents_to_muse();
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

fn pack_marker(label: &str) -> PathBuf {
    muse_home().join("skill-packs").join(format!("{label}.ok"))
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

fn mirror_agents_to_muse() {
    let Some(home) = dirs::home_dir() else { return };
    let agents = home.join(".agents").join("skills");
    let muse = muse_home().join("skills");
    let Ok(entries) = fs::read_dir(&agents) else {
        return;
    };
    let _ = fs::create_dir_all(&muse);
    for e in entries.flatten() {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        let name = e.file_name();
        let dest = muse.join(&name);
        if dest.exists() {
            continue;
        }
        // Best-effort copy of SKILL.md only (avoid huge tree copies for cyber).
        let src_skill = p.join("SKILL.md");
        if src_skill.is_file() {
            let _ = fs::create_dir_all(&dest);
            let _ = fs::copy(&src_skill, dest.join("SKILL.md"));
        }
    }
}

/// Catalog / index skills that point the agent at large packs without loading
/// 817 full playbooks into every prompt.
fn write_catalog_skills() -> Result<(), String> {
    let root = muse_home().join("skills");
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

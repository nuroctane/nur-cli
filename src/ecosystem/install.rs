//! `nur ecosystem install <name>`: install one ecosystem component with a
//! plan-first contract - print exactly what will run and where it lands, and
//! only execute with `--yes`. Everything is pinned by `lock.json`.

use super::{find_bin, lock, packs, run_capture};

/// What an install would do, in plain terms the user can audit.
pub(crate) struct InstallPlan {
    pub name: String,
    /// Human summary lines: source, pinned version/commit, target.
    pub plan: Vec<String>,
}

enum Action {
    /// argv; first element resolved via find_bin at run time when bare.
    Cmd(Vec<String>),
    Pack {
        label: String,
        url: String,
        commit: String,
    },
    Plugin {
        id: String,
    },
}

fn npm_plan(name: &str, package: &str, note: &str) -> (InstallPlan, Action) {
    let spec = lock::npm_spec(package);
    (
        InstallPlan {
            name: name.into(),
            plan: vec![
                format!("npm package: {spec} (pinned in src/ecosystem/lock.json)"),
                format!("runs: npm install -g {spec}"),
                "lands in: your global npm bin directory".into(),
                note.into(),
            ],
        },
        Action::Cmd(vec!["npm".into(), "install".into(), "-g".into(), spec]),
    )
}

fn uv_plan(name: &str, package: &str, extra: &[&str], note: &str) -> (InstallPlan, Action) {
    let spec = lock::pypi_spec(package);
    let mut argv = vec![
        "uv".to_string(),
        "tool".into(),
        "install".into(),
        spec.clone(),
    ];
    argv.extend(extra.iter().map(|s| s.to_string()));
    (
        InstallPlan {
            name: name.into(),
            plan: vec![
                format!("pypi package: {spec} (pinned in src/ecosystem/lock.json)"),
                format!("runs: {}", argv.join(" ")),
                "lands in: the uv tools directory".into(),
                note.into(),
            ],
        },
        Action::Cmd(argv),
    )
}

fn cargo_git_plan(
    name: &str,
    lock_name: &str,
    crate_name: &str,
    note: &str,
) -> (InstallPlan, Action) {
    let pin = lock::git_pin(lock_name);
    let repo = pin
        .as_ref()
        .map(|p| p.repo.clone())
        .unwrap_or_else(|| format!("https://github.com/{lock_name}"));
    let mut argv = vec![
        "cargo".to_string(),
        "install".into(),
        "--git".into(),
        repo.clone(),
    ];
    let mut pin_line = "commit: unpinned (no lockfile entry)".to_string();
    if let Some(p) = &pin {
        argv.push("--rev".into());
        argv.push(p.commit.clone());
        pin_line = format!("commit: {} (pinned in src/ecosystem/lock.json)", p.commit);
    }
    argv.push(crate_name.into());
    (
        InstallPlan {
            name: name.into(),
            plan: vec![
                format!("git repo: {repo}"),
                pin_line,
                format!("runs: {}", argv.join(" ")),
                "lands in: ~/.cargo/bin".into(),
                note.into(),
            ],
        },
        Action::Cmd(argv),
    )
}

/// Build the plan for a known component, or explain what names are valid.
fn plan_install(name: &str) -> Result<(InstallPlan, Action), String> {
    let key = name.trim().to_ascii_lowercase();
    let (plan, action) = match key.as_str() {
        "penecho" => npm_plan(
            "penecho",
            "penecho",
            "an approved penecho tool launch finishes config setup",
        ),
        "egaki" => npm_plan("egaki", "egaki", "image/video generation CLI"),
        "akarso" => npm_plan("akarso", "akarso", "run `akarso auth login` afterwards"),
        "executor" => npm_plan("executor", "executor", "MCP gateway on :4788"),
        "graphjin" => npm_plan("graphjin", "graphjin", "governed GraphQL over SQL"),
        "ruflo" => npm_plan("ruflo", "ruflo", "agent memory/store"),
        "plur" => npm_plan(
            "plur",
            "@plur-ai/cli",
            "pair-programming memory (cli + mcp share a version)",
        ),
        "skills" => npm_plan("skills", "skills", "open agent skills CLI"),
        "akm" => npm_plan("akm", "akm-cli", "agent knowledge manager"),
        "omp" => npm_plan(
            "omp",
            "@oh-my-pi/pi-coding-agent",
            "delegated coding agent (bun or npm)",
        ),
        "agent-browser" | "browser-cli" => npm_plan(
            "agent-browser-cli",
            "@sleepinsummer/agent-browser-cli",
            "real-Chrome bridge; finish with `nur browser setup`",
        ),
        "excalidraw" => npm_plan(
            "excalidraw",
            "excalidraw-cli",
            "diagrams via the excalidraw tool",
        ),
        "headroom" => uv_plan(
            "headroom",
            "headroom-ai",
            &["--python", "3.13"],
            "tool-result compression (extras [all] not included in direct installs)",
        ),
        "fractal" => uv_plan(
            "fractal",
            "plasma-fractal",
            &["--with-executables-from", "plasma-wiki"],
            "Unix-only deepen harness",
        ),
        "graphify" => uv_plan("graphify", "graphifyy", &[], "codebase graphs"),
        "dogwood" => cargo_git_plan(
            "dogwood",
            "dogwood",
            "amzn-dogwood-cli",
            "runtime verification CLI",
        ),
        "sgrep" => cargo_git_plan("sgrep", "sgrep", "sgrep", "local semantic code search"),
        other => {
            // Skill packs (by label) and default plugins (by catalog id).
            if let Some(pin) = lock::pack_pin(other) {
                let url = format!("https://github.com/{}", pin.repo);
                (
                    InstallPlan {
                        name: other.into(),
                        plan: vec![
                            format!("skill pack: {} (commit {})", pin.repo, pin.commit),
                            format!("runs: git clone of {url} at the pinned commit"),
                            "lands in: ~/.agents/skills (+ mirror into ~/.nur/skills)".into(),
                        ],
                    },
                    Action::Pack {
                        label: other.into(),
                        url,
                        commit: pin.commit.clone(),
                    },
                )
            } else if crate::plugins::by_id(other).is_some() {
                let pin_note = lock::plugin_pin(other)
                    .map(|p| format!("commit: {} (pinned)", p.commit))
                    .unwrap_or_else(|| "commit: unpinned catalog entry (tracks HEAD)".into());
                (
                    InstallPlan {
                        name: other.into(),
                        plan: vec![
                            format!("plugin: {other}"),
                            pin_note,
                            "lands in: ~/.nur/plugins (+ skill mirror into ~/.nur/skills)".into(),
                        ],
                    },
                    Action::Plugin { id: other.into() },
                )
            } else {
                return Err(format!(
                    "unknown ecosystem component '{name}'. Known: penecho, egaki, akarso, executor, \
                     graphjin, ruflo, plur, skills, akm, omp, agent-browser, excalidraw, headroom, \
                     fractal, graphify, dogwood, sgrep, skill packs ({}), plugins in /plugins",
                    crate::ecosystem::packs::pack_labels().join(", ")
                ));
            }
        }
    };
    Ok((plan, action))
}

/// Print the plan; execute only when `yes` was passed.
pub(crate) fn run_install(name: &str, yes: bool) -> Result<String, String> {
    let (plan, action) = plan_install(name)?;
    let mut out = format!(
        "install plan for {} (lockfile {})\n",
        plan.name,
        lock::resolved_at()
    );
    for line in &plan.plan {
        out.push_str(&format!("  {line}\n"));
    }
    if !yes {
        out.push_str("\nNothing was installed. Re-run with --yes to proceed.\n");
        return Ok(out);
    }
    match action {
        Action::Cmd(argv) => {
            let bin = find_bin(&argv[0]).unwrap_or_else(|| argv[0].clone());
            let refs: Vec<&str> = argv[1..].iter().map(|s| s.as_str()).collect();
            run_capture(&bin, &refs, None, 900_000).map_err(|e| format!("install failed: {e}"))?;
            out.push_str("\ninstalled.\n");
        }
        Action::Pack { label, url, commit } => {
            let n = packs::install_pack_pinned(&label, &url, &commit)?;
            out.push_str(&format!("\ninstalled {n} skills from {label}.\n"));
        }
        Action::Plugin { id } => {
            let msg = crate::plugins::install_plugin(&id)?;
            out.push_str(&format!("\n{msg}\n"));
        }
    }
    Ok(out)
}

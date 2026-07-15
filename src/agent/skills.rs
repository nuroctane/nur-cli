//! Load agent skills (SKILL.md) — Claude Code-compatible shape.
//!
//! Large skills are listed by name/description only in the system prompt.
//! Natural-language intents (e.g. "think like fable", "debug systematically",
//! "TDD this") auto-activate matching skills for that turn by injecting their
//! full body — no slash command required.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub body: String,
    pub path: PathBuf,
}

/// Result of matching the user message to an installed skill.
#[derive(Debug, Clone)]
pub struct SkillActivation {
    /// Skill id (folder name).
    pub skill_name: String,
    /// Short status chip label, e.g. `fable-method` or `tdd`.
    pub label: String,
    /// Full system-prompt section (header + body).
    pub section: String,
}

/// One natural-language → skill rule. First matching rule whose skill is
/// installed wins (rules are ordered most-specific first).
struct IntentRule {
    /// Prefer first installed name in this list.
    skill_names: &'static [&'static str],
    /// Substrings matched against normalized user text.
    phrases: &'static [&'static str],
    /// Status / docs label.
    label: &'static str,
    /// Short human reason shown in the activation header.
    why: &'static str,
}

/// Built-in NL routes for high-signal workflow skills.
/// Keep phrases specific enough that casual chat does not false-fire.
const INTENT_RULES: &[IntentRule] = &[
    // ── Fable family ─────────────────────────────────────────────────────
    IntentRule {
        skill_names: &["fable-judge"],
        phrases: &[
            "/fable-judge",
            "fable-judge",
            "fable judge",
            "judge this like fable",
            "judge it like fable",
            "fable-style judge",
            "fable style judge",
            "verify like fable",
            "prove it like fable",
        ],
        label: "fable-judge",
        why: "adversarial Fable verification of finished work",
    },
    IntentRule {
        skill_names: &["fable-loop"],
        phrases: &[
            "/fable-loop",
            "fable-loop",
            "fable loop",
            "run the fable loop",
            "fable workflow",
        ],
        label: "fable-loop",
        why: "orchestrated Fable multi-step loop",
    },
    IntentRule {
        skill_names: &["fable-method"],
        phrases: &[
            "/fable-method",
            "fable-method",
            "think like fable",
            "think as fable",
            "how fable would",
            "how fable did",
            "how fable does",
            "the way fable",
            "way fable would",
            "like fable would",
            "as fable would",
            "do it how fable",
            "do this how fable",
            "do it like fable",
            "do this like fable",
            "approach this like fable",
            "approach like fable",
            "use the fable method",
            "use fable method",
            "fable method",
            "fable style",
            "fable's way",
            "fables way",
            "fable way",
            "fable would do",
            "would fable",
            "as fable did",
            "like fable did",
            "be like fable",
            "channel fable",
            "use fable",
            "with fable",
            "via fable",
            "per fable",
        ],
        label: "fable-method",
        why: "Fable think → act → prove problem-solving loop",
    },
    // ── Superpowers process skills ───────────────────────────────────────
    IntentRule {
        skill_names: &["systematic-debugging"],
        phrases: &[
            "systematic debugging",
            "debug systematically",
            "systematically debug",
            "find the root cause",
            "root cause first",
            "no fixes without root cause",
            "debug properly",
            "proper debugging",
        ],
        label: "systematic-debugging",
        why: "root-cause-first debugging before any fix",
    },
    IntentRule {
        skill_names: &["test-driven-development"],
        phrases: &[
            "test-driven",
            "test driven",
            "tdd this",
            "tdd the",
            "write tests first",
            "tests first",
            "red green refactor",
            "red-green-refactor",
            "do it tdd",
            "use tdd",
        ],
        label: "tdd",
        why: "test-driven development (red → green → refactor)",
    },
    IntentRule {
        skill_names: &["brainstorming"],
        phrases: &[
            "brainstorm this",
            "brainstorm the",
            "let's brainstorm",
            "lets brainstorm",
            "brainstorm with me",
            "help me brainstorm",
        ],
        label: "brainstorming",
        why: "structured brainstorming before implementation",
    },
    IntentRule {
        skill_names: &["writing-plans"],
        phrases: &[
            "write a plan",
            "write the plan",
            "draft a plan",
            "make a plan first",
            "plan first then",
            "writing-plans",
            "/writing-plans",
        ],
        label: "writing-plans",
        why: "write an implementation plan before coding",
    },
    IntentRule {
        skill_names: &["executing-plans"],
        phrases: &[
            "execute the plan",
            "execute this plan",
            "run the plan",
            "implement the plan",
            "executing-plans",
            "/executing-plans",
        ],
        label: "executing-plans",
        why: "execute an existing implementation plan",
    },
    IntentRule {
        skill_names: &["verification-before-completion", "verification"],
        phrases: &[
            "verify before claiming",
            "verify before you claim",
            "verify before completion",
            "don't claim done until",
            "do not claim done until",
            "check your work thoroughly",
            "verify the work",
            "verification-before-completion",
        ],
        label: "verify-before-done",
        why: "re-run claimed checks before any success claim",
    },
    IntentRule {
        skill_names: &["requesting-code-review"],
        phrases: &[
            "request a code review",
            "request code review",
            "code review this",
            "review this pr",
            "review my changes",
            "requesting-code-review",
        ],
        label: "code-review-request",
        why: "structured code-review request workflow",
    },
    IntentRule {
        skill_names: &["receiving-code-review"],
        phrases: &[
            "apply the code review",
            "apply review feedback",
            "receiving-code-review",
            "handle review comments",
            "address the review",
        ],
        label: "code-review-receive",
        why: "apply code-review feedback rigorously",
    },
    IntentRule {
        skill_names: &["dispatching-parallel-agents"],
        phrases: &[
            "dispatch parallel agents",
            "parallel agents",
            "fan out agents",
            "spawn parallel agents",
            "dispatching-parallel-agents",
        ],
        label: "parallel-agents",
        why: "dispatch independent work to parallel agents",
    },
    IntentRule {
        skill_names: &["subagent-driven-development"],
        phrases: &[
            "subagent-driven",
            "subagent driven",
            "subagent development",
            "drive with subagents",
        ],
        label: "subagent-driven",
        why: "subagent-driven development workflow",
    },
    IntentRule {
        skill_names: &["finishing-a-development-branch"],
        phrases: &[
            "finish this branch",
            "finish the branch",
            "finishing-a-development-branch",
            "ready to merge",
            "wrap up this branch",
        ],
        label: "finish-branch",
        why: "finish a development branch (merge/PR/cleanup options)",
    },
    IntentRule {
        skill_names: &["using-git-worktrees"],
        phrases: &[
            "use a worktree",
            "use git worktree",
            "using-git-worktrees",
            "isolate in a worktree",
            "create a worktree",
        ],
        label: "git-worktrees",
        why: "isolated git worktree for feature work",
    },
    IntentRule {
        skill_names: &["writing-skills"],
        phrases: &[
            "write a skill",
            "create a skill",
            "author a skill",
            "writing-skills",
            "new agent skill",
        ],
        label: "writing-skills",
        why: "author or improve an agent skill",
    },
    // ── Design / clone / diagrams ────────────────────────────────────────
    IntentRule {
        skill_names: &["design-eng", "emil-design-eng"],
        phrases: &[
            "design-eng",
            "design eng",
            "emil design",
            "emil-style",
            "emil style",
            "polish the ui",
            "polish this ui",
            "ui polish",
            "motion polish",
            "animation polish",
            "make it feel great",
        ],
        label: "design-eng",
        why: "design-engineering / UI polish (Emil-style)",
    },
    IntentRule {
        skill_names: &["clone-website-meta", "clone-website"],
        phrases: &[
            "clone this website",
            "clone the website",
            "clone this site",
            "pixel-perfect clone",
            "pixel perfect clone",
            "clone-website-meta",
            "replicate this site",
            "rebuild this site from",
        ],
        label: "clone-website",
        why: "pixel-perfect website reverse-engineering pipeline",
    },
    IntentRule {
        skill_names: &["excalidraw"],
        phrases: &[
            "excalidraw",
            "draw a diagram",
            "draw an architecture",
            "hand-drawn diagram",
            "hand drawn diagram",
            "architecture diagram as excalidraw",
        ],
        label: "excalidraw",
        why: "hand-drawn Excalidraw diagram workflow",
    },
    IntentRule {
        skill_names: &["improve-animations"],
        phrases: &[
            "improve the animations",
            "improve animations",
            "audit the motion",
            "animation audit",
            "improve-animations",
        ],
        label: "improve-animations",
        why: "animation / motion audit and improvement plan",
    },
    // ── Resume foreign agents ────────────────────────────────────────────
    IntentRule {
        skill_names: &["resume-claude"],
        phrases: &[
            "resume claude",
            "resume from claude",
            "continue from claude",
            "pick up claude",
            "claude's session",
            "claude session",
        ],
        label: "resume-claude",
        why: "resume a Claude Code session",
    },
    IntentRule {
        skill_names: &["resume-grok"],
        phrases: &[
            "resume grok",
            "resume from grok",
            "continue from grok",
            "pick up grok",
            "grok's session",
            "grok session",
            "grok build session",
        ],
        label: "resume-grok",
        why: "resume a Grok Build session",
    },
    IntentRule {
        skill_names: &["resume-codex"],
        phrases: &[
            "resume codex",
            "resume from codex",
            "continue from codex",
            "pick up codex",
            "codex session",
        ],
        label: "resume-codex",
        why: "resume a Codex session",
    },
    IntentRule {
        skill_names: &["resume-cursor"],
        phrases: &[
            "resume cursor",
            "resume from cursor",
            "continue from cursor",
            "pick up cursor",
            "cursor session",
        ],
        label: "resume-cursor",
        why: "resume a Cursor session",
    },
    IntentRule {
        skill_names: &["resume-nur", "resume-meta"],
        phrases: &[
            "resume nur",
            "resume from nur",
            "continue from nur",
            "resume my nur session",
            "continue my nur session",
            "resume meta session",
            "continue meta session",
        ],
        label: "resume-nur",
        why: "resume a prior NurCLI session",
    },
    // ── Knowledge / security routers ─────────────────────────────────────
    IntentRule {
        skill_names: &["graphify"],
        phrases: &[
            "use graphify",
            "build the knowledge graph",
            "update the knowledge graph",
            "graphify this",
            "query the graph",
        ],
        label: "graphify",
        why: "code knowledge graph (graphify) workflow",
    },
    IntentRule {
        skill_names: &["cybersecurity"],
        phrases: &[
            "cybersecurity skill",
            "security playbook",
            "use cybersecurity",
            "mitre att&ck",
            "mitre attack",
        ],
        label: "cybersecurity",
        why: "cybersecurity skill router (load specific playbooks, never all)",
    },
    IntentRule {
        skill_names: &["using-superpowers"],
        phrases: &[
            "use superpowers",
            "using-superpowers",
            "superpowers workflow",
            "invoke superpowers",
        ],
        label: "superpowers",
        why: "Superpowers skill-first process discipline",
    },
];

/// Normalize user text for phrase matching (lowercase, collapse punctuation).
pub fn normalize_intent_text(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '/' || c == '-' {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn find_installed<'a>(skills: &'a [Skill], names: &[&str]) -> Option<&'a Skill> {
    for n in names {
        if let Some(sk) = skills.iter().find(|s| s.name.eq_ignore_ascii_case(n)) {
            return Some(sk);
        }
    }
    None
}

/// Match user text to the first installed skill rule.
pub fn detect_skill_activation<'a>(
    user_text: &str,
    skills: &'a [Skill],
) -> Option<(&'a Skill, &'static IntentRule)> {
    let t = normalize_intent_text(user_text);
    if t.is_empty() {
        return None;
    }

    for rule in INTENT_RULES {
        if !rule.phrases.iter().any(|p| t.contains(p)) {
            continue;
        }
        if let Some(sk) = find_installed(skills, rule.skill_names) {
            return Some((sk, rule));
        }
    }

    // Loose Fable fallback: "fable" + a method-ish word, only if method installed.
    if t.contains("fable") {
        let methodish = ["think", "approach", "method", "workflow", "style", "way", "loop", "judge"];
        if methodish.iter().any(|w| t.contains(w)) {
            let names: &[&str] = if t.contains("judge") {
                &["fable-judge", "fable-method"]
            } else if t.contains("loop") {
                &["fable-loop", "fable-method"]
            } else {
                &["fable-method"]
            };
            if let Some(sk) = find_installed(skills, names) {
                // Reuse the matching rule for label/why when possible.
                let rule = INTENT_RULES
                    .iter()
                    .find(|r| r.skill_names.contains(&sk.name.as_str()))
                    .unwrap_or(&INTENT_RULES[2]); // fable-method rule
                return Some((sk, rule));
            }
        }
    }

    None
}

/// Build activation section + metadata when a NL intent matches an installed skill.
pub fn skill_activation(user_text: &str, skills: &[Skill]) -> Option<SkillActivation> {
    let (sk, rule) = detect_skill_activation(user_text, skills)?;
    let body = read_skill_body(sk);
    let body: String = body.chars().take(40_000).collect();

    let mut section = format!(
        "\n# SKILL ACTIVATED (natural language — mandatory)\n\
         The user's wording matched **{label}** ({why}).\n\
         This is **not** optional flavor. For this entire turn you MUST follow the skill \
         below literally. Slash commands are never required — activation already happened.\n\
         Do **not** freestyle a shorter path. Load sibling `references/` under the skill \
         directory when the skill points there.\n\n\
         ## Active skill: {name} (`{path}`)\n\n{body}\n",
        label = rule.label,
        why = rule.why,
        name = sk.name,
        path = sk.path.display(),
        body = body,
    );

    // Keep section usable even if format! above is huge.
    let _ = &mut section;

    Some(SkillActivation {
        skill_name: sk.name.clone(),
        label: rule.label.to_string(),
        section,
    })
}

/// Back-compat alias used by older call sites / tests.
pub fn fable_activation_section(user_text: &str, skills: &[Skill]) -> Option<String> {
    skill_activation(user_text, skills).map(|a| a.section)
}

fn read_skill_body(sk: &Skill) -> String {
    std::fs::read_to_string(&sk.path)
        .ok()
        .map(|t| {
            if t.starts_with("---") {
                if let Some(end) = t[3..].find("---") {
                    return t[end + 6..].trim().to_string();
                }
            }
            t
        })
        .unwrap_or_else(|| sk.body.clone())
}

/// Discover skills from (first match wins per name):
/// - `$NUR_HOME/skills` (or `~/.nur/skills`) — primary
/// - enabled marketplace plugins under `~/.nur/plugins/` (skills/ + pack roots)
/// - legacy `~/.muse/skills`
/// - `~/.agents/skills` (Agent Skills / graphify install --platform agents)
/// - `<cwd>/.meta/skills` · `<cwd>/.muse/skills` · `<cwd>/.agents/skills` · `<cwd>/.nur/skills`
pub fn load_skills(cwd: &Path) -> Vec<Skill> {
    let mut out = Vec::new();
    let mut dirs = Vec::new();
    // Honor NUR_HOME / META_HOME / MUSE_HOME via meta_home() — not a hard-coded path.
    dirs.push(crate::config::meta_home().join("skills"));
    // Marketplace plugins (enabled only) — after core home skills so user overrides win.
    dirs.extend(crate::plugins::enabled_skill_roots());
    dirs.push(crate::config::legacy_muse_home().join("skills"));
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".agents").join("skills"));
    }
    dirs.push(cwd.join(".nur").join("skills"));
    dirs.push(cwd.join(".meta").join("skills"));
    dirs.push(cwd.join(".muse").join("skills")); // legacy workspace
    dirs.push(cwd.join(".agents").join("skills"));

    for root in dirs {
        if !root.is_dir() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            let skill_md = if p.is_dir() {
                p.join("SKILL.md")
            } else if p
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.eq_ignore_ascii_case("SKILL.md"))
                .unwrap_or(false)
            {
                p.clone()
            } else {
                continue;
            };
            if !skill_md.is_file() {
                continue;
            }
            if let Some(skill) = parse_skill(&skill_md) {
                // dedupe by name
                if !out.iter().any(|s: &Skill| s.name == skill.name) {
                    out.push(skill);
                }
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn parse_skill(path: &Path) -> Option<Skill> {
    let text = std::fs::read_to_string(path).ok()?;
    let name = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("skill")
        .to_string();

    // Optional YAML frontmatter
    let (description, body) = if text.starts_with("---") {
        if let Some(end) = text[3..].find("---") {
            let fm = &text[3..end + 3];
            let body = text[end + 6..].trim().to_string();
            let desc = fm
                .lines()
                .find_map(|l| {
                    l.strip_prefix("description:")
                        .map(|s| s.trim().trim_matches('"').to_string())
                })
                .unwrap_or_else(|| first_line(&body));
            (desc, body)
        } else {
            (first_line(&text), text.clone())
        }
    } else {
        (first_line(&text), text.clone())
    };

    let body: String = body.chars().take(12_000).collect();
    Some(Skill {
        name,
        description,
        body,
        path: path.to_path_buf(),
    })
}

fn first_line(s: &str) -> String {
    s.lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("skill")
        .trim()
        .trim_start_matches('#')
        .trim()
        .chars()
        .take(200)
        .collect()
}

/// Compact catalog for the system prompt + optional full body for named skills.
pub fn skills_prompt_section(skills: &[Skill]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut s = String::from("\n# Installed skills\n");
    s.push_str(
        "Use these when the user task matches a skill's description or natural-language cues. \
         Prefer `skill(action=read, name=…)` (or the path below) for full instructions — \
         slash commands are optional, never required.\n\
         When the harness injects a **SKILL ACTIVATED** block for this turn, that skill is \
         mandatory for the whole turn — follow it literally.\n",
    );

    // Document which NL routes are available for installed skills only.
    let installed: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
    let mut nl_lines = Vec::new();
    for rule in INTENT_RULES {
        if find_installed(skills, rule.skill_names).is_some() {
            // Show 2–3 example phrases
            let examples: Vec<&str> = rule.phrases.iter().copied().take(3).collect();
            nl_lines.push(format!(
                "- **{}** → `{}` — say: *{}*",
                rule.label,
                rule.skill_names[0],
                examples.join("*, *")
            ));
        }
    }
    if !nl_lines.is_empty() {
        s.push_str("\n## Natural-language auto-activation\n");
        s.push_str(
            "These phrases (and close variants) auto-inject the skill body for the turn:\n",
        );
        for line in nl_lines {
            s.push_str(&line);
            s.push('\n');
        }
        let _ = installed; // silence if unused in some builds
    }

    for sk in skills {
        s.push_str(&format!(
            "- **{}**: {} (`{}`)\n",
            sk.name,
            sk.description,
            sk.path.display()
        ));
    }
    // Inline small skills fully
    for sk in skills.iter().filter(|s| s.body.len() < 2500).take(6) {
        s.push_str(&format!(
            "\n## Skill: {}\n{}\n",
            sk.name,
            sk.body.chars().take(2500).collect::<String>()
        ));
    }
    s
}

#[cfg(test)]
mod intent_tests {
    use super::*;

    fn fake_skill(name: &str) -> Skill {
        Skill {
            name: name.into(),
            description: "test".into(),
            body: "body".into(),
            path: PathBuf::from(format!("/tmp/{name}/SKILL.md")),
        }
    }

    #[test]
    fn detects_natural_language_fable() {
        let skills = vec![
            fake_skill("fable-method"),
            fake_skill("fable-loop"),
            fake_skill("fable-judge"),
        ];
        let (sk, rule) = detect_skill_activation(
            "please think like fable and fix the login hang",
            &skills,
        )
        .unwrap();
        assert_eq!(sk.name, "fable-method");
        assert_eq!(rule.label, "fable-method");

        let (sk, _) = detect_skill_activation("do it how fable would do it", &skills).unwrap();
        assert_eq!(sk.name, "fable-method");

        let (sk, _) = detect_skill_activation("run the fable loop on this", &skills).unwrap();
        assert_eq!(sk.name, "fable-loop");

        let (sk, _) = detect_skill_activation("fable judge this work", &skills).unwrap();
        assert_eq!(sk.name, "fable-judge");

        assert!(detect_skill_activation("fix the typo in readme", &skills).is_none());
    }

    #[test]
    fn detects_tdd_and_debug_and_resume() {
        let skills = vec![
            fake_skill("test-driven-development"),
            fake_skill("systematic-debugging"),
            fake_skill("resume-claude"),
            fake_skill("design-eng"),
        ];
        let (sk, rule) =
            detect_skill_activation("please TDD this auth module", &skills).unwrap();
        assert_eq!(sk.name, "test-driven-development");
        assert_eq!(rule.label, "tdd");

        let (sk, _) =
            detect_skill_activation("debug systematically — find the root cause", &skills)
                .unwrap();
        assert_eq!(sk.name, "systematic-debugging");

        let (sk, _) =
            detect_skill_activation("pick up claude's session and finish it", &skills).unwrap();
        assert_eq!(sk.name, "resume-claude");

        let (sk, _) = detect_skill_activation("polish the UI like emil", &skills).unwrap();
        assert_eq!(sk.name, "design-eng");
    }

    #[test]
    fn no_match_when_skill_not_installed() {
        let skills = vec![fake_skill("unrelated")];
        assert!(detect_skill_activation("think like fable", &skills).is_none());
        assert!(detect_skill_activation("tdd this", &skills).is_none());
    }
}

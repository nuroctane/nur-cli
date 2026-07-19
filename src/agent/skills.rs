//! Load agent skills (SKILL.md) — Claude Code-compatible shape.
//!
//! Skills are **on-demand only** — never dumped into every system prompt.
//! Activation paths (every provider):
//! 1. Built-in `INTENT_RULES` phrase routes (high-signal workflows)
//! 2. Installed skill **name** mentioned in the user message (accidental discovery)
//! 3. Soft **description** intent match when the query clearly maps to one skill
//! 4. Slash `/skill-name` (sticky or one-shot) from the TUI
//!
//! When activated, the full skill body is injected for that turn only.

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
    /// Short status chip label, e.g. `fable-method` or `tdd`.
    pub label: String,
    /// Full system-prompt section (header + body).
    pub section: String,
}

/// One natural-language → skill rule. First matching rule whose skill is
/// installed wins (rules are ordered most-specific first).
pub(crate) struct IntentRule {
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
            "fable would do",
            "as fable did",
            "like fable did",
            "be like fable",
            "channel fable",
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
    // ── External pointer skills (short body; load live docs) ─────────────
    IntentRule {
        skill_names: &["toolcraft"],
        phrases: &[
            "toolcraft",
            "/toolcraft",
            "tool craft",
            "design app scaffold",
            "design-app scaffold",
            "craft tooling",
            "use toolcraft",
        ],
        label: "toolcraft",
        why: "Toolcraft design-app scaffold (pointer — fetch live docs)",
    },
    IntentRule {
        skill_names: &["site-cli"],
        phrases: &[
            "site cli",
            "site-cli",
            "/site-cli",
            "watch network requests",
            "watching network requests",
            "record network requests",
            "recording network requests",
            "network requests into a har",
            "har file",
            "har files",
            "save as har",
            "derive a client",
            "derive a cli",
            "build a site cli",
            "uber eats cli",
            "reverse-engineer the api",
            "reverse engineer the api",
        ],
        label: "site-cli",
        why: "HAR capture -> derived HTTP client/CLI (no browser every time)",
    },
    IntentRule {
        skill_names: &["adhd"],
        phrases: &[
            "/adhd",
            "adhd mode",
            "i have adhd",
            "adhd-friendly",
            "adhd friendly",
        ],
        label: "adhd",
        why: "ADHD-friendly output shape (action-first, no fluff)",
    },
    IntentRule {
        skill_names: &["fable-domain"],
        phrases: &[
            "/fable-domain",
            "fable-domain",
            "fable domain",
            "make a skill for",
            "add a domain to the fable method",
            "fable domain adapter",
        ],
        label: "fable-domain",
        why: "generate a Fable domain skill bundle",
    },
    IntentRule {
        skill_names: &["tech-spec"],
        phrases: &[
            "/tech-spec",
            "tech-spec",
            "tech spec",
            "write a tech spec",
            "call-stack architecture handoff",
            "architecture handoff",
        ],
        label: "tech-spec",
        why: "typed call-stack architecture handoff",
    },
    IntentRule {
        skill_names: &["context-pruning"],
        phrases: &[
            "/context-pruning",
            "context-pruning",
            "context pruning",
            "prune context",
            "dcp patterns",
        ],
        label: "context-pruning",
        why: "dynamic context pruning patterns",
    },
    IntentRule {
        skill_names: &["nextjs"],
        phrases: &[
            "/nextjs",
            "next.js app router",
            "nextjs app router",
            "next.js expert",
        ],
        label: "nextjs",
        why: "Next.js App Router guidance",
    },
    IntentRule {
        skill_names: &["shadcn"],
        phrases: &[
            "/shadcn",
            "shadcn/ui",
            "shadcn ui",
            "add shadcn component",
        ],
        label: "shadcn",
        why: "shadcn/ui component guidance",
    },
    IntentRule {
        skill_names: &["ai-sdk"],
        phrases: &[
            "/ai-sdk",
            "vercel ai sdk",
            "ai sdk stream",
            "use the ai sdk",
        ],
        label: "ai-sdk",
        why: "Vercel AI SDK guidance",
    },
    IntentRule {
        skill_names: &["vercel-cli"],
        phrases: &[
            "/vercel-cli",
            "vercel cli deploy",
            "use vercel cli",
        ],
        label: "vercel-cli",
        why: "Vercel CLI guidance",
    },
    IntentRule {
        skill_names: &["herdr"],
        phrases: &[
            "/herdr",
            "herdr workspace",
            "control herdr",
        ],
        label: "herdr",
        why: "control herdr from inside it",
    },
    IntentRule {
        skill_names: &["resume-session"],
        phrases: &[
            "/resume-session",
            "resume-session",
            "resume this session skill",
        ],
        label: "resume-session",
        why: "resume session handoff skill",
    },
    IntentRule {
        skill_names: &["akm-manager"],
        phrases: &[
            "/akm-manager",
            "akm-manager",
            "akm install",
            "akm list skills",
        ],
        label: "akm-manager",
        why: "AKM skill package manager",
    },
    IntentRule {
        skill_names: &["opencode-awesome"],
        phrases: &[
            "/opencode-awesome",
            "opencode-awesome",
            "opencode plugins",
            "opencode ecosystem",
        ],
        label: "opencode-awesome",
        why: "OpenCode ecosystem index",
    },
    IntentRule {
        skill_names: &["scan"],
        phrases: &[
            // /scan is a built-in command; NL phrases only
            "foglamp scan",
            "codebase scan map",
            "publish a codebase scan",
        ],
        label: "scan",
        why: "shareable foglamp codebase scan",
    },
    IntentRule {
        skill_names: &["improve-animations"],
        phrases: &[
            "/improve-animations",
            "improve the animations",
            "improve animations",
            "audit the motion",
            "motion audit",
        ],
        label: "improve-animations",
        why: "animation/motion audit and plan",
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

/// True if `needle` occurs in `haystack` as a whole-token run — bounded by a
/// space or a string end on each side. Both are expected already normalized by
/// [`normalize_intent_text`] (lowercase, single-spaced, tokens of alnum/`/`/`-`),
/// and every `needle` (rule phrase) is ASCII, so byte-boundary checks are safe.
///
/// This is the fix for short phrases matching *inside* a longer word — e.g.
/// `excalidraw` must not fire on `excalidrawings`, and `use fable` must not
/// fire on `use fables`.
fn phrase_matches(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let hb = haystack.as_bytes();
    let mut from = 0;
    while let Some(rel) = haystack[from..].find(needle) {
        let start = from + rel;
        let end = start + needle.len();
        let before_ok = start == 0 || hb[start - 1] == b' ';
        let after_ok = end == haystack.len() || hb[end] == b' ';
        if before_ok && after_ok {
            return true;
        }
        // needle[0] is ASCII, so `start + 1` stays on a char boundary.
        from = start + 1;
        if from >= haystack.len() {
            break;
        }
    }
    false
}

fn find_installed<'a>(skills: &'a [Skill], names: &[&str]) -> Option<&'a Skill> {
    for n in names {
        if let Some(sk) = skills.iter().find(|s| s.name.eq_ignore_ascii_case(n)) {
            return Some(sk);
        }
    }
    None
}

/// Synthetic rule used when activation came from name/description discovery
/// rather than a built-in `INTENT_RULES` phrase.
const DISCOVERY_RULE: IntentRule = IntentRule {
    skill_names: &[],
    phrases: &[],
    label: "skill",
    why: "matched installed skill from your wording (name or description intent)",
};

/// Match user text to the first installed skill rule, then fall back to
/// installed-skill name/description discovery so any pack can activate from
/// natural language without being pre-loaded into the system prompt.
pub fn detect_skill_activation<'a>(
    user_text: &str,
    skills: &'a [Skill],
) -> Option<(&'a Skill, &'static IntentRule)> {
    let t = normalize_intent_text(user_text);
    if t.is_empty() {
        return None;
    }

    for rule in INTENT_RULES {
        if !rule.phrases.iter().any(|p| phrase_matches(&t, p)) {
            continue;
        }
        if let Some(sk) = find_installed(skills, rule.skill_names) {
            return Some((sk, rule));
        }
    }

    // Loose Fable fallback for phrasings the explicit list misses. Require a
    // strong method cue **directly adjacent** to the `fable` token (a bigram),
    // so questions that merely mention fable ("how does fable's loop differ
    // from opus") don't hijack the turn.
    if phrase_matches(&t, "fable") {
        const STRONG: &[&str] = &["judge", "loop", "think", "approach", "method", "workflow"];
        let cue = STRONG.iter().copied().find(|w| {
            phrase_matches(&t, &format!("fable {w}")) || phrase_matches(&t, &format!("{w} fable"))
        });
        if let Some(w) = cue {
            let names: &[&str] = match w {
                "judge" => &["fable-judge", "fable-method"],
                "loop" => &["fable-loop", "fable-method"],
                _ => &["fable-method"],
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

    // Accidental / free-form discovery against *installed* skills only.
    // Prefer longest name match so `fable-method` wins over a short sibling.
    if let Some(sk) = discover_by_skill_name(&t, skills) {
        return Some((sk, &DISCOVERY_RULE));
    }
    if let Some(sk) = discover_by_description_intent(&t, skills) {
        return Some((sk, &DISCOVERY_RULE));
    }

    None
}

/// True when the user message mentions this skill's name as whole tokens
/// (hyphens and spaces interchangeable: `grill-me` ↔ `grill me`).
fn skill_name_mentioned(user_norm: &str, skill_name: &str) -> bool {
    let name = skill_name.trim();
    if name.is_empty() {
        return false;
    }
    let with_hyphen = normalize_intent_text(name);
    let with_spaces = normalize_intent_text(&name.replace('-', " "));
    // Very short names (≤3) are noisy ("ai", "sre") — require a skill cue.
    let short = with_spaces.chars().count() <= 3;
    let mentioned = (!with_hyphen.is_empty() && phrase_matches(user_norm, &with_hyphen))
        || (!with_spaces.is_empty()
            && with_spaces != with_hyphen
            && phrase_matches(user_norm, &with_spaces));
    if !mentioned {
        return false;
    }
    if short {
        return phrase_matches(user_norm, "skill")
            || phrase_matches(user_norm, "use")
            || phrase_matches(user_norm, "run")
            || phrase_matches(user_norm, "with")
            || phrase_matches(user_norm, "via")
            || phrase_matches(user_norm, "mode")
            || user_norm.starts_with(&with_spaces)
            || user_norm.starts_with(&format!("/{with_spaces}"));
    }
    true
}

fn discover_by_skill_name<'a>(user_norm: &str, skills: &'a [Skill]) -> Option<&'a Skill> {
    let mut best: Option<(&'a Skill, usize)> = None;
    for sk in skills {
        if !skill_name_mentioned(user_norm, &sk.name) {
            continue;
        }
        let score = sk.name.chars().count();
        match best {
            Some((_, best_score)) if score <= best_score => {}
            _ => best = Some((sk, score)),
        }
    }
    best.map(|(sk, _)| sk)
}

/// Stopwords ignored when scoring description overlap (common English + agent fluff).
const DESC_STOP: &[&str] = &[
    "a", "an", "the", "and", "or", "to", "of", "for", "in", "on", "at", "by", "with",
    "from", "this", "that", "these", "those", "is", "are", "was", "were", "be", "been",
    "being", "have", "has", "had", "do", "does", "did", "will", "would", "can", "could",
    "should", "may", "might", "must", "use", "using", "used", "when", "where", "what",
    "which", "who", "how", "why", "into", "over", "under", "about", "after", "before",
    "your", "you", "their", "them", "its", "it", "as", "if", "then", "than", "also",
    "just", "only", "not", "no", "yes", "any", "all", "each", "other", "more", "most",
    "some", "such", "via", "per", "between", "through", "during", "without", "within",
    "skill", "skills", "agent", "agents", "help", "please", "like", "make", "need",
    "needs", "want", "wants", "get", "set", "run", "work", "works", "working",
];

fn significant_tokens(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| w.trim().to_ascii_lowercase())
        .filter(|w| w.chars().count() >= 4)
        .filter(|w| !DESC_STOP.contains(&w.as_str()))
        .collect()
}

/// Soft intent match: user query shares enough distinctive description tokens
/// with exactly one installed skill (or a clear winner). Avoids dumping the
/// catalog while still letting "accidental" intent find the right pack.
fn discover_by_description_intent<'a>(user_norm: &str, skills: &'a [Skill]) -> Option<&'a Skill> {
    let user_tokens = significant_tokens(user_norm);
    if user_tokens.len() < 3 {
        return None;
    }
    let mut scored: Vec<(&'a Skill, usize)> = Vec::new();
    for sk in skills {
        let desc_norm = normalize_intent_text(&sk.description);
        if desc_norm.is_empty() {
            continue;
        }
        let desc_tokens = significant_tokens(&desc_norm);
        if desc_tokens.len() < 3 {
            continue;
        }
        let hits = user_tokens
            .iter()
            .filter(|t| desc_tokens.iter().any(|d| d == *t))
            .count();
        // Need a solid multi-token overlap so casual chat does not false-fire.
        if hits >= 3 {
            scored.push((sk, hits));
        }
    }
    if scored.is_empty() {
        return None;
    }
    scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.name.cmp(&b.0.name)));
    let best = scored[0].1;
    // Unique winner (or clear margin) only — ties mean ambiguous intent.
    let contenders: Vec<_> = scored.into_iter().filter(|(_, h)| *h == best).collect();
    if contenders.len() == 1 {
        return Some(contenders[0].0);
    }
    None
}

/// Build activation section + metadata when a NL intent matches an installed skill.
pub fn skill_activation(user_text: &str, skills: &[Skill]) -> Option<SkillActivation> {
    let (sk, rule) = detect_skill_activation(user_text, skills)?;
    let body = read_skill_body(sk);
    let body: String = body.chars().take(40_000).collect();

    // Discovery fallback uses a generic label — surface the real skill name in UI.
    let label = if rule.skill_names.is_empty() {
        sk.name.as_str()
    } else {
        rule.label
    };

    let mut section = format!(
        "\n# SKILL ACTIVATED (natural language — mandatory)\n\
         The user's wording matched **{label}** ({why}).\n\
         This is **not** optional flavor. For this entire turn you MUST follow the skill \
         below literally. Slash commands are never required — activation already happened.\n\
         Do **not** freestyle a shorter path. Load sibling `references/` under the skill \
         directory when the skill points there.\n\n\
         ## Active skill: {name} (`{path}`)\n\n{body}\n",
        label = label,
        why = rule.why,
        name = sk.name,
        path = sk.path.display(),
        body = body,
    );

    // Keep section usable even if format! above is huge.
    let _ = &mut section;

    Some(SkillActivation {
        label: rule.label.to_string(),
        section,
    })
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

/// Look up an installed skill by its exact `name` (case-insensitive).
/// Powers slash invocation of any skill: `/adhd`, `/scan`, etc.
pub fn skill_by_name(cwd: &Path, name: &str) -> Option<Skill> {
    let want = name.trim().trim_start_matches('/').to_ascii_lowercase();
    load_skills(cwd)
        .into_iter()
        .find(|s| s.name.eq_ignore_ascii_case(&want))
}

/// Build the mandatory activation section for a skill invoked explicitly
/// (e.g. via a slash command), mirroring `skill_activation` but without the
/// natural-language matching.
pub fn slash_activation_section(sk: &Skill) -> String {
    let body: String = read_skill_body(sk).chars().take(40_000).collect();
    format!(
        "\n# SKILL ACTIVATED (invoked — mandatory)\n\
         The user invoked **/{name}**. This is **not** optional flavor. For this \
         entire turn (and until told otherwise for sticky skills) you MUST follow \
         the skill below literally. Load sibling `references/` under the skill \
         directory when the skill points there.\n\n\
         ## Active skill: {name} (`{path}`)\n\n{body}\n",
        name = sk.name,
        path = sk.path.display(),
        body = body,
    )
}

/// Discover skills from (first match wins per name):
/// - `$NUR_HOME/skills` (or `~/.nur/skills`) — primary
/// - enabled marketplace plugins under `~/.nur/plugins/` (skills/ + pack roots)
/// - legacy `~/.muse/skills`
/// - `~/.agents/skills` (Agent Skills / graphify install --platform agents)
/// - `<cwd>/.meta/skills` · `<cwd>/.muse/skills` · `<cwd>/.agents/skills` · `<cwd>/.nur/skills`
/// Max directory depth when walking for nested SKILL.md (category/pack layouts).
const SKILL_WALK_MAX_DEPTH: usize = 5;

/// Collect every `SKILL.md` under `root` up to `max_depth` directory levels.
/// Skips obvious junk (`.git`, `node_modules`, `target`, …).
pub fn find_skill_mds(root: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn walk(dir: &Path, depth: usize, max_depth: usize, out: &mut Vec<PathBuf>) {
        if depth > max_depth {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            let name = p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            if name.starts_with('.')
                || name.eq_ignore_ascii_case("node_modules")
                || name.eq_ignore_ascii_case("target")
                || name.eq_ignore_ascii_case("dist")
                || name.eq_ignore_ascii_case("build")
                || name.eq_ignore_ascii_case("__pycache__")
            {
                continue;
            }
            if p.is_file() && name.eq_ignore_ascii_case("SKILL.md") {
                out.push(p);
                continue;
            }
            if p.is_dir() {
                // Prefer skill dirs that contain SKILL.md at this level (still walk
                // siblings/categories for nested pack layouts).
                walk(&p, depth + 1, max_depth, out);
            }
        }
    }
    if root.is_dir() {
        walk(root, 0, max_depth, &mut out);
    } else if root.is_file()
        && root
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.eq_ignore_ascii_case("SKILL.md"))
            .unwrap_or(false)
    {
        out.push(root.to_path_buf());
    }
    out
}

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
        for skill_md in find_skill_mds(&root, SKILL_WALK_MAX_DEPTH) {
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
    let folder_name = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("skill")
        .to_string();

    // Optional YAML frontmatter
    let (name, description, body) = if text.starts_with("---") {
        if let Some(end) = text[3..].find("---") {
            let fm = &text[3..end + 3];
            let body = text[end + 6..].trim().to_string();
            let fm_name = fm.lines().find_map(|l| {
                let rest = l.strip_prefix("name:")?;
                let s = rest.trim().trim_matches('"').trim();
                let s = s.strip_prefix("'").unwrap_or(s);
                let s = s.strip_suffix("'").unwrap_or(s).trim();
                if s.is_empty() {
                    None
                } else {
                    Some(s.to_string())
                }
            });
            let desc = fm
                .lines()
                .find_map(|l| {
                    l.strip_prefix("description:")
                        .map(|s| s.trim().trim_matches('"').to_string())
                })
                .unwrap_or_else(|| first_line(&body));
            (fm_name.unwrap_or(folder_name), desc, body)
        } else {
            (folder_name, first_line(&text), text.clone())
        }
    } else {
        (folder_name, first_line(&text), text.clone())
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

/// Human-readable catalog for `/skills` and `skill(action=list)` — **not**
/// injected into the model system prompt (that burned tokens on large installs).
pub fn skills_prompt_section(skills: &[Skill]) -> String {
    if skills.is_empty() {
        return String::from(
            "No skills installed. Add packs under ~/.nur/skills or: nur plugins install <name>\n",
        );
    }
    let mut s = format!(
        "Installed skills ({}) — on-demand only (NL intent, /name, or skill tool):\n",
        skills.len()
    );
    for sk in skills {
        s.push_str(&format!("- **{}**: {}\n", sk.name, sk.description));
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

        let skills = vec![fake_skill("toolcraft")];
        let (sk, rule) = detect_skill_activation("use toolcraft for this scaffold", &skills).unwrap();
        assert_eq!(sk.name, "toolcraft");
        assert_eq!(rule.label, "toolcraft");
    }

    #[test]
    fn no_match_when_skill_not_installed() {
        let skills = vec![fake_skill("unrelated")];
        assert!(detect_skill_activation("think like fable", &skills).is_none());
        assert!(detect_skill_activation("tdd this", &skills).is_none());
    }

    #[test]
    fn substring_mentions_do_not_false_fire() {
        // A phrase must match whole words, never inside a longer token.
        let skills = vec![fake_skill("excalidraw")];
        assert!(detect_skill_activation("these excalidrawings look great", &skills).is_none());
    }

    #[test]
    fn broad_fable_mentions_do_not_activate() {
        // Merely mentioning fable in a question/comparison must not hijack the turn.
        let skills = vec![
            fake_skill("fable-method"),
            fake_skill("fable-loop"),
            fake_skill("fable-judge"),
        ];
        for q in [
            "compare it with fable and opus",
            "per fable 5 release notes",
            "route this via fable api",
            "how does fable's loop differ from opus",
            "would fable be better than opus here",
            "use fables of aesop as examples",
        ] {
            assert!(
                detect_skill_activation(q, &skills).is_none(),
                "should NOT activate on: {q}"
            );
        }
    }

    #[test]
    fn directive_fable_still_activates() {
        // Clear directives must keep working after the false-positive cleanup.
        let skills = vec![
            fake_skill("fable-method"),
            fake_skill("fable-loop"),
            fake_skill("fable-judge"),
        ];
        let cases = [
            ("use the fable method here", "fable-method"),
            ("channel fable on this refactor", "fable-method"),
            ("run the fable loop on this", "fable-loop"),
            ("take the fable approach here", "fable-method"),
            ("please fable judge this work", "fable-judge"),
        ];
        for (q, want) in cases {
            let (sk, _) = detect_skill_activation(q, &skills)
                .unwrap_or_else(|| panic!("expected activation for: {q}"));
            assert_eq!(sk.name, want, "for: {q}");
        }
    }

    #[test]
    fn discovers_installed_skill_by_name_without_builtin_rule() {
        // Any installed skill name in the user message activates — no catalog needed.
        let skills = vec![Skill {
            name: "grill-me".into(),
            description: "interview the user with hard questions".into(),
            body: "ask tough questions".into(),
            path: PathBuf::from("/tmp/grill-me/SKILL.md"),
        }];
        let (sk, _) = detect_skill_activation("please run grill-me on this design", &skills)
            .expect("name mention should activate");
        assert_eq!(sk.name, "grill-me");

        let (sk, _) = detect_skill_activation("use grill me for the API review", &skills)
            .expect("hyphen↔space name form");
        assert_eq!(sk.name, "grill-me");
    }

    #[test]
    fn longer_skill_name_wins_over_shorter_prefix() {
        let skills = vec![
            fake_skill("fable"),
            fake_skill("fable-method"),
        ];
        // INTENT_RULES hit fable-method first for "fable method"; name discovery
        // alone should prefer the longer token when both names appear.
        let (sk, _) =
            detect_skill_activation("please load fable-method now", &skills).unwrap();
        assert_eq!(sk.name, "fable-method");
    }

    #[test]
    fn discovers_by_unique_description_intent() {
        let skills = vec![
            Skill {
                name: "pixel-clone".into(),
                description: "pixel perfect website reverse engineering pipeline for cloning sites"
                    .into(),
                body: "clone".into(),
                path: PathBuf::from("/tmp/pixel-clone/SKILL.md"),
            },
            Skill {
                name: "other".into(),
                description: "unrelated helper for database migrations".into(),
                body: "db".into(),
                path: PathBuf::from("/tmp/other/SKILL.md"),
            },
        ];
        let (sk, _) = detect_skill_activation(
            "need a pixel perfect website reverse engineering pipeline please",
            &skills,
        )
        .expect("description intent");
        assert_eq!(sk.name, "pixel-clone");
    }

    #[test]
    fn description_intent_ignores_ambiguous_ties() {
        let skills = vec![
            Skill {
                name: "a".into(),
                description: "pixel perfect website reverse engineering pipeline".into(),
                body: "x".into(),
                path: PathBuf::from("/tmp/a/SKILL.md"),
            },
            Skill {
                name: "b".into(),
                description: "pixel perfect website reverse engineering pipeline".into(),
                body: "y".into(),
                path: PathBuf::from("/tmp/b/SKILL.md"),
            },
        ];
        assert!(detect_skill_activation(
            "pixel perfect website reverse engineering pipeline please",
            &skills,
        )
        .is_none());
    }


    #[test]
    fn finds_nested_skill_mds() {
        let root = std::env::temp_dir().join(format!("nur-skill-walk-{}", std::process::id()));
        let nested = root.join("skills").join("engineering").join("grill-me");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(
            nested.join("SKILL.md"),
            "---
name: grill-me
description: test
---

body
",
        )
        .unwrap();
        let found = find_skill_mds(&root, 5);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(found.len(), 1, "expected nested SKILL.md, got {found:?}");
    }
}

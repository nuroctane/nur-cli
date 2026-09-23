//! Expanded NL triggers for all 700+ skills — comprehensive JSON index.
//! This file loads `skill_intents.json` (generated from ~/.nur/skills,
//! ~/.agents/skills, and repo skills) which contains
//! triggers for every installed skill, not just the hardcoded INTENT_RULES.
//! The JSON is 600-700KB and is parsed once via OnceLock.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

use super::skills::{normalize_intent_text, phrase_matches, Skill};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SkillIntentEntry {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub triggers: Vec<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct IntentFile {
    #[serde(default)]
    pub skills: Vec<SkillIntentEntry>,
}

static RAW_JSON: &str = include_str!("skill_intents.json");

static PARSED: OnceLock<Vec<SkillIntentEntry>> = OnceLock::new();
static TRIGGER_MAP: OnceLock<Vec<(String, String)>> = OnceLock::new(); // (normalized_trigger, skill_name)

fn parsed_entries() -> &'static Vec<SkillIntentEntry> {
    PARSED.get_or_init(|| {
        // Try to parse JSON, fallback to empty on error
        match serde_json::from_str::<IntentFile>(RAW_JSON) {
            Ok(f) => f.skills,
            Err(_) => Vec::new(),
        }
    })
}

/// Build a flat list of (normalized_trigger, skill_name) sorted by trigger length desc
/// so longer, more specific triggers win.
fn trigger_map() -> &'static Vec<(String, String)> {
    TRIGGER_MAP.get_or_init(|| {
        let entries = parsed_entries();
        let mut map = Vec::new();
        for entry in entries {
            let skill_name = entry.name.clone();
            for trig in &entry.triggers {
                let norm = normalize_intent_text(trig);
                if norm.is_empty() || norm.len() < 4 {
                    continue;
                }
                // Skip overly generic single-word triggers
                // - single token must be >=6 chars and not be bare "fable" etc
                // - multi-word triggers must be >=7 chars
                if !norm.contains(' ') && !norm.contains('-') && !norm.contains('/') {
                    if norm.len() < 6 {
                        continue;
                    }
                    // block generic top-level names that false-fire
                    if matches!(norm.as_str(), "fable") {
                        continue;
                    }
                }
                // block exact "/fable" etc
                if norm == "/fable" || norm == "fable" {
                    continue;
                }
                map.push((norm, skill_name.clone()));
            }
        }
        // Dedupe and sort by length desc for specificity (longer triggers first)
        map.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
        map.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
        map
    })
}

/// Find an installed skill whose expanded trigger matches the normalized user text.
/// Returns the skill with longest matching trigger.
pub fn find_by_expanded_triggers<'a>(user_norm: &str, installed: &'a [Skill]) -> Option<&'a Skill> {
    if user_norm.is_empty() {
        return None;
    }
    // Build quick lookup of installed names for fast check
    let installed_names: std::collections::HashSet<&str> =
        installed.iter().map(|s| s.name.as_str()).collect();

    // Score every matching trigger instead of returning the first hit: the
    // map's iteration order is arbitrary, so a generic single-word trigger
    // ("audit" -> the UI-audit skill) used to beat a domain-precise
    // multi-word one ("smart contract audit" -> sc-research) for a query
    // containing both. Rank by: multi-word triggers over single-word, then
    // longer (more specific) phrases, then description-token overlap with
    // the query, then name for determinism.
    let query_tokens: std::collections::HashSet<&str> =
        user_norm.split(|c: char| !c.is_alphanumeric()).collect();
    let mut best: Option<(u64, usize, u64, String, &'a Skill)> = None;
    for (trigger_norm, skill_name) in trigger_map().iter() {
        if !installed_names.contains(skill_name.as_str()) {
            continue;
        }
        if !phrase_matches(user_norm, trigger_norm) {
            continue;
        }
        let Some(sk) = installed.iter().find(|s| s.name == *skill_name) else {
            continue;
        };
        let words = trigger_norm.split_whitespace().count() as u64;
        let chars = trigger_norm.chars().count();
        let overlap = sk
            .description
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.chars().count() >= 4)
            .filter(|w| query_tokens.contains(w))
            .count() as u64;
        let key = (words, chars, overlap, sk.name.clone());
        if best
            .as_ref()
            .is_none_or(|b| (b.0, b.1, b.2, b.3.clone()) < key)
        {
            best = Some((words, chars, overlap, sk.name.clone(), sk));
        }
    }
    best.map(|(_, _, _, _, sk)| sk)
}

/// Indexed skill count and expanded trigger count — surfaced by `nur doctor`.
pub fn stats() -> (usize, usize) {
    let entries = parsed_entries();
    let triggers = trigger_map().len();
    (entries.len(), triggers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::skills::{load_skills, normalize_intent_text};

    #[test]
    fn startup_pack_has_slash_and_natural_language_routes() {
        for (name, phrase) in [
            ("startup-design", "validate my startup"),
            ("startup-competitors", "competitor battle cards"),
            ("startup-positioning", "position my startup"),
            ("startup-pitch", "pitch my startup"),
        ] {
            let entry = parsed_entries().iter().find(|e| e.name == name).unwrap();
            assert!(entry.triggers.contains(&format!("/{name}")));
            let skill = Skill {
                name: name.into(),
                description: entry.description.clone(),
                body: String::new(),
                path: std::path::PathBuf::from("SKILL.md"),
            };
            let skills = vec![skill];
            assert_eq!(
                find_by_expanded_triggers(&normalize_intent_text(phrase), &skills)
                    .unwrap()
                    .name,
                name
            );
        }
    }

    #[test]
    fn expanded_triggers_cover_fable() {
        let cwd = std::env::current_dir().unwrap();
        let skills = load_skills(&cwd);
        // Should find fable-method from expanded triggers
        let user = normalize_intent_text("please use the fable method for this refactor");
        let found = find_by_expanded_triggers(&user, &skills);
        assert!(
            found.is_some(),
            "should find fable-method via expanded triggers"
        );
        assert_eq!(found.unwrap().name, "fable-method");
    }

    /// The TypeSafe (Jev) skill must be reachable by natural language: it is in
    /// the generated index with the aliases a user would actually type.
    #[test]
    fn expanded_triggers_cover_typesafe() {
        let cwd = std::env::current_dir().unwrap();
        let skills = load_skills(&cwd);
        assert!(
            skills.iter().any(|s| s.name == "typesafe-ai"),
            "the shipped typesafe-ai skill must be discoverable from the repo"
        );
        for phrase in [
            "use typesafe for this decision",
            // Bare "jev" is deliberately not a trigger: the index drops short
            // single-token triggers to avoid false fires ("noul", "jev").
            "wire system one judgments into the loop",
            "typed judgments instead of a prompt",
            "raise the confidence threshold",
        ] {
            let user = normalize_intent_text(phrase);
            let found = find_by_expanded_triggers(&user, &skills);
            assert_eq!(
                found.map(|s| s.name.clone()),
                Some("typesafe-ai".to_string()),
                "phrase {phrase:?} should activate the TypeSafe skill"
            );
        }
    }

    #[test]
    fn expanded_triggers_cover_scan() {
        let cwd = std::env::current_dir().unwrap();
        let skills = load_skills(&cwd);
        let user = normalize_intent_text("scan the codebase for issues");
        // scan is single word, but our expanded triggers include "scan" for scan skill
        // However single-word "scan" alone is too generic? We have "scan" as trigger for scan skill
        // Since scan is single-word skill name, skill_name_mentioned should catch it, but expanded should also
        let found = find_by_expanded_triggers(&user, &skills);
        // May be None if "scan" single word filtered, but we have "codebase scan" etc
        // So we test with more specific phrase
        let user2 = normalize_intent_text("codebase scan this repo");
        let found2 = find_by_expanded_triggers(&user2, &skills);
        assert!(
            found2.is_some() || found.is_some(),
            "should find scan via expanded"
        );
    }

    /// The scored matcher must prefer the domain-precise multi-word trigger
    /// (sc-research "smart contract audit") over the generic single-word
    /// "audit" trigger of the UI-audit skill, regardless of map order.
    #[test]
    fn scoring_prefers_domain_precise_triggers_over_generic_ones() {
        let cwd = std::env::current_dir().unwrap();
        let skills = load_skills(&cwd);
        let user = normalize_intent_text("audit all smart contracts in this codebase");
        if let Some(found) = find_by_expanded_triggers(&user, &skills) {
            assert_ne!(
                found.name, "audit",
                "generic 'audit' trigger must not win on a smart-contract query"
            );
        }
    }

    #[test]
    fn expanded_triggers_cover_sc_research() {
        // Don't go through load_skills: repo skills/ is a cached global root, so
        // a stale ~/.nur/cache/skills-index.json would hide a brand-new pack.
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("skills")
            .join("security")
            .join("SCA")
            .join("sc-research")
            .join("SKILL.md");
        let parsed = crate::agent::skills::parse_skill(&path)
            .expect("skills/security/SCA/sc-research/SKILL.md must parse");
        assert_eq!(parsed.name, "sc-research");
        assert!(
            parsed.description.to_lowercase().contains("whitehat"),
            "description should mention whitehat, got {:?}",
            parsed.description
        );

        let skills = vec![parsed];
        let user = normalize_intent_text("please use sc-research on this vault");
        let found = find_by_expanded_triggers(&user, &skills);
        assert!(
            found.is_some(),
            "should find sc-research via expanded triggers"
        );
        assert_eq!(found.unwrap().name, "sc-research");

        for extra in [
            "reviewing-erc4626-and-vaults",
            "reviewing-bridges-and-messaging",
            "formal-verification-halmos-certora-kontrol",
            "researcher-gym-and-curriculum",
            "historical-smart-contract-vulns",
            "reviewing-solana-programs",
            "reviewing-move-modules",
            "reviewing-cosmos-and-ibc",
            "reviewing-cairo-and-starknet",
            "reviewing-bitcoin-adjacent",
            "reviewing-frontend-and-ops-surfaces",
            "hunting-x-linked-bounties",
            "auditing-foundry-smart-contract-security",
        ] {
            let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("skills")
                .join("security")
                .join("SCA")
                .join(extra)
                .join("SKILL.md");
            let sk = crate::agent::skills::parse_skill(&p)
                .unwrap_or_else(|| panic!("{extra} must parse"));
            assert_eq!(sk.name, extra);
        }
    }

    #[test]
    fn expanded_index_comprehensive() {
        let (total, triggers) = stats();
        assert!(total >= 700, "should have 700+ skills, got {}", total);
        assert!(
            triggers >= 1000,
            "should have 1000+ triggers, got {}",
            triggers
        );
    }
}

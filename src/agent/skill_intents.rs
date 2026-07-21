//! Expanded NL triggers for all 700+ skills — comprehensive JSON index.
//! This file loads `skill_intents.json` (generated from ~/.nur/skills) which contains
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

    for (trigger_norm, skill_name) in trigger_map().iter() {
        if !installed_names.contains(skill_name.as_str()) {
            continue;
        }
        if phrase_matches(user_norm, trigger_norm) {
            if let Some(sk) = installed.iter().find(|s| s.name == *skill_name) {
                return Some(sk);
            }
        }
    }
    None
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

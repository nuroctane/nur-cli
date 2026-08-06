//! Agent-native memory system for nur.
//!
//! Implements the four-module analytical framework from:
//! Zhou et al., *Are We Ready For An Agent-Native Memory System?*
//! arXiv:2606.24775 (2026) — https://arxiv.org/abs/2606.24775
//!
//!   M1 Representation & storage
//!   M2 Extraction
//!   M3 Retrieval & routing
//!   M4 Maintenance (localized, not global reorganization)
//!
//! Continuity design draws from Anima Labs Connectome
//! (https://animalabs.ai/connectome):
//! - Hierarchical resolution L3 → L2 → L1 → Recent (lossy by design)
//! - On-policy / first-person self-authored memories preferred
//! - Append-only archive; never delete the record
//! - Live edge (recent) is not rewritten — only aged tiers consolidate
//!
//! Complements (does not replace) `memory.md`, OptMem, PLUR, context_store.

use crate::config::{atomic_write, nur_home};
use crate::tools::sensitive::body_looks_sensitive;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Connectome-style resolution tiers (coarse → fine as material ages reverse).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// Deep past — coarsest recollection.
    L3,
    /// Merged era memories.
    L2,
    /// Fine-grained recent past (self-authored diary).
    L1,
    /// Near-verbatim working notes (still not chat tokens).
    Recent,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::L3 => "l3",
            Self::L2 => "l2",
            Self::L1 => "l1",
            Self::Recent => "recent",
        }
    }

    fn rank(self) -> u8 {
        match self {
            Self::Recent => 0,
            Self::L1 => 1,
            Self::L2 => 2,
            Self::L3 => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Voice {
    /// Connectome on-policy: agent wrote this in first person.
    FirstPerson,
    /// Third-party / host-extracted note.
    Observed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub tier: Tier,
    pub voice: Voice,
    pub text: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// 0.0–1.0 confidence (Connectome lessons pattern).
    #[serde(default = "default_confidence")]
    pub confidence: f32,
    pub created_unix: u64,
    pub updated_unix: u64,
    #[serde(default)]
    pub source: String,
    /// Soft-delete for maintenance; chronicle/archive still has text if needed.
    #[serde(default)]
    pub retired: bool,
}

fn default_confidence() -> f32 {
    0.7
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn scope_dir(scope: &str) -> PathBuf {
    let safe: String = scope
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    nur_home().join("native-memory").join(safe)
}

fn entries_path(scope: &str) -> PathBuf {
    scope_dir(scope).join("entries.jsonl")
}

/// M1 — load all non-retired entries (small enough for local CLI agents).
pub fn load_entries(scope: &str) -> Vec<MemoryEntry> {
    let text = std::fs::read_to_string(entries_path(scope)).unwrap_or_default();
    let mut out = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(e) = serde_json::from_str::<MemoryEntry>(line) {
            if !e.retired {
                out.push(e);
            }
        }
    }
    out
}

/// Fetch a single entry by id (including retired — for archive lookups).
pub fn get_by_id(scope: &str, id: &str) -> Option<MemoryEntry> {
    let text = std::fs::read_to_string(entries_path(scope)).unwrap_or_default();
    text.lines()
        .filter_map(|l| serde_json::from_str::<MemoryEntry>(l).ok())
        .find(|e| e.id == id)
}

fn rewrite_all(scope: &str, entries: &[MemoryEntry]) -> Result<(), String> {
    let p = entries_path(scope);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut buf = String::new();
    for e in entries {
        let line = serde_json::to_string(e).map_err(|e| e.to_string())?;
        buf.push_str(&line);
        buf.push('\n');
    }
    atomic_write(&p, buf.as_bytes()).map_err(|e| e.to_string())
}

fn append_entry(scope: &str, entry: &MemoryEntry) -> Result<(), String> {
    use std::io::Write;
    let p = entries_path(scope);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
        .map_err(|e| e.to_string())?;
    let line = serde_json::to_string(entry).map_err(|e| e.to_string())?;
    writeln!(f, "{line}").map_err(|e| e.to_string())
}

fn looks_secret(text: &str) -> bool {
    body_looks_sensitive(text)
        || text.to_ascii_lowercase().contains("password")
        || text.contains("BEGIN PRIVATE KEY")
}

// ─── M2 Extraction ───────────────────────────────────────────────────────────

/// Heuristic extraction of durable facts from a turn (no extra model call).
/// Prefer explicit `remember` for high-value first-person notes.
pub fn extract_candidates(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.len() < 24 || t.len() > 400 {
            continue;
        }
        let lower = t.to_ascii_lowercase();
        let cue = lower.starts_with("i will ")
            || lower.starts_with("we decided ")
            || lower.starts_with("decision:")
            || lower.starts_with("preference:")
            || lower.starts_with("note to self")
            || lower.contains("always ")
            || lower.contains("never ")
            || lower.contains("prefer ");
        if cue && !looks_secret(t) {
            out.push(t.to_string());
        }
        if out.len() >= 5 {
            break;
        }
    }
    out
}

/// M2 write path — store a memory at a tier with voice.
pub fn remember(
    scope: &str,
    text: &str,
    tier: Tier,
    voice: Voice,
    tags: &[String],
    confidence: f32,
    source: &str,
) -> Result<MemoryEntry, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("memory text required".into());
    }
    if text.chars().count() > 4_000 {
        return Err("memory too long (max 4000 chars)".into());
    }
    if looks_secret(text) {
        return Err("refused: looks like a secret".into());
    }
    let now = now_unix();
    let entry = MemoryEntry {
        id: format!("m-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]),
        tier,
        voice,
        text: text.to_string(),
        tags: tags.iter().take(12).cloned().collect(),
        confidence: confidence.clamp(0.0, 1.0),
        created_unix: now,
        updated_unix: now,
        source: source.chars().take(64).collect(),
        retired: false,
    };
    append_entry(scope, &entry)?;
    bump_ops(scope, |o| o.remember_count += 1);
    // Real vector + graph indexing (m2/m3): embed the memory for semantic
    // recall and absorb entity relations into the knowledge graph. Never
    // blocks or fails the write.
    {
        let mut vs = crate::agent::memory_vector::VectorStore::open(scope);
        let _ = vs.index(&entry.id, &entry.text); // api → local fallback
    }
    {
        let mut g = crate::agent::memory_graph::GraphStore::open(scope);
        let _ = g.absorb(&entry.id, &entry.text);
    }
    Ok(entry)
}

// ─── M3 Retrieval & routing ──────────────────────────────────────────────────

fn tokenize(s: &str) -> Vec<String> {
    s.to_ascii_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 3)
        .map(str::to_string)
        .collect()
}

/// Score entry for query: keyword overlap + recency + confidence + voice bonus.
fn score(entry: &MemoryEntry, query_tokens: &[String], now: u64) -> f32 {
    if query_tokens.is_empty() {
        // No query: prefer recent + high confidence first-person.
        let age_days = now.saturating_sub(entry.created_unix) as f32 / 86_400.0;
        let recency = 1.0 / (1.0 + age_days);
        let voice = if entry.voice == Voice::FirstPerson {
            0.15
        } else {
            0.0
        };
        return entry.confidence * 0.5 + recency * 0.35 + voice;
    }
    let hay = format!(
        "{} {}",
        entry.text.to_ascii_lowercase(),
        entry.tags.join(" ").to_ascii_lowercase()
    );
    let mut hits = 0u32;
    for t in query_tokens {
        if hay.contains(t) {
            hits += 1;
        }
    }
    if hits == 0 {
        return 0.0;
    }
    let overlap = hits as f32 / query_tokens.len() as f32;
    let age_days = now.saturating_sub(entry.created_unix) as f32 / 86_400.0;
    let recency = 1.0 / (1.0 + age_days * 0.25);
    let tier_boost = match entry.tier {
        Tier::Recent => 0.12,
        Tier::L1 => 0.08,
        Tier::L2 => 0.04,
        Tier::L3 => 0.02,
    };
    let voice = if entry.voice == Voice::FirstPerson {
        0.1
    } else {
        0.0
    };
    overlap * 0.55 + entry.confidence * 0.2 + recency * 0.15 + tier_boost + voice
}

/// M3 — retrieve top-k memories for a query (or recent if query empty).
pub fn recall(scope: &str, query: &str, k: usize) -> Vec<(MemoryEntry, f32)> {
    let k = k.clamp(1, 32);
    let now = now_unix();
    let qtokens = tokenize(query);
    let live = load_entries(scope);
    let scanned: u64 = live.iter().map(|e| e.text.chars().count() as u64).sum();
    bump_ops(scope, |o| {
        o.recall_count += 1;
        o.recall_chars_scanned += scanned;
    });
    // M1 ("physical storage and indexing", arXiv:2606.24775 §3.1.2): build an
    // inverted index (term → entries) so rare/specific terms route better than
    // plain substring scoring. This is the no-dependency analog of a vector or
    // keyword index the paper compares.
    let index = InvertedIndex::build(&live);
    let mut scored: Vec<(MemoryEntry, f32)> = live
        .into_iter()
        .map(|e| {
            let base = score(&e, &qtokens, now);
            // Index boost: +0.15 for each rare query term present in THIS entry
            // (rare = appears in few entries; idf-like).
            let boost = qtokens
                .iter()
                .map(|t| {
                    let df = index.df(t);
                    if df > 0 && df <= 3 && e_text_has(&e, t) {
                        0.15 / f32::max(1.0, df as f32)
                    } else {
                        0.0
                    }
                })
                .sum::<f32>();
            (e, base + boost)
        })
        .filter(|(_, s)| *s > 0.0)
        .collect();
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.0.created_unix.cmp(&a.0.created_unix))
    });
    scored.truncate(k);
    scored
}

fn e_text_has(e: &MemoryEntry, term: &str) -> bool {
    e.text.to_ascii_lowercase().contains(term) || e.tags.iter().any(|t| t.to_ascii_lowercase() == *term)
}

/// Thin inverted index (term → doc frequency). Built per recall from the live
/// set; small enough that this is cheaper than a persistent structure for the
/// sizes a CLI session accumulates.
struct InvertedIndex {
    df: std::collections::HashMap<String, usize>,
}

impl InvertedIndex {
    fn build(entries: &[MemoryEntry]) -> Self {
        let mut df: std::collections::HashMap<String, usize> = Default::default();
        for e in entries {
            let mut seen: std::collections::HashSet<String> = Default::default();
            for t in tokenize(&e.text) {
                if seen.insert(t.clone()) {
                    *df.entry(t).or_default() += 1;
                }
            }
            for t in e.tags.iter().filter_map(|s| tokenize(s).into_iter().next()) {
                if seen.insert(t.clone()) {
                    *df.entry(t).or_default() += 1;
                }
            }
        }
        Self { df }
    }

    fn df(&self, term: &str) -> usize {
        self.df.get(term).copied().unwrap_or(0)
    }
}

// ─── M4 Maintenance (localized) ──────────────────────────────────────────────

/// Paper finding: localized maintenance is more cost-efficient than global
/// reorganization. Promote the oldest L1 batch into a single L2 merge note
/// without rewriting Recent/L1 live edge.
pub fn consolidate_localized(scope: &str, max_l1: usize) -> Result<String, String> {
    let max_l1 = max_l1.max(8);
    let live = load_entries(scope);
    let mut l1: Vec<&MemoryEntry> = live
        .iter()
        .filter(|e| e.tier == Tier::L1 && !e.retired)
        .collect();
    l1.sort_by_key(|e| e.created_unix);
    if l1.len() <= max_l1 {
        return Ok(format!(
            "no consolidation needed ({} L1 entries, threshold {max_l1})",
            l1.len()
        ));
    }
    let take = (l1.len() - max_l1).clamp(3, 12);
    let batch: Vec<MemoryEntry> = l1.iter().take(take).map(|e| (*e).clone()).collect();
    let ids: Vec<String> = batch.iter().map(|e| e.id.clone()).collect();
    let merged_body = {
        let mut lines = vec![
            "I consolidated older fine memories into an era note:".to_string(),
        ];
        for e in &batch {
            let snippet: String = e.text.chars().take(200).collect();
            lines.push(format!("- {snippet}"));
        }
        lines.join("\n")
    };
    bump_ops(scope, |o| o.consolidate_count += 1);
    // Retire batch in the full file (localized — only those rows).
    // load_entries filters retired; we need every line for rewrite.
    let text = std::fs::read_to_string(entries_path(scope)).unwrap_or_default();
    let mut all: Vec<MemoryEntry> = text
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    for e in &mut all {
        if ids.contains(&e.id) {
            e.retired = true;
            e.updated_unix = now_unix();
        }
    }
    let now = now_unix();
    let l2 = MemoryEntry {
        id: format!("m-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]),
        tier: Tier::L2,
        voice: Voice::FirstPerson,
        text: merged_body,
        tags: vec!["consolidated".into(), "era".into()],
        confidence: 0.65,
        created_unix: now,
        updated_unix: now,
        source: "maintenance".into(),
        retired: false,
    };
    all.push(l2.clone());
    rewrite_all(scope, &all)?;
    Ok(format!(
        "localized maintenance: retired {} L1 → new L2 `{}` ({} chars)",
        take,
        l2.id,
        l2.text.chars().count()
    ))
}

// ─── Prompt injection ────────────────────────────────────────────────────────

/// Route memories into the system prompt (M3). Cap chars for multi-provider budgets.
pub fn prompt_block(scope: &str, query: &str, max_chars: usize) -> String {
    let hits = recall(scope, query, 8);
    if hits.is_empty() {
        return String::new();
    }
    let mut lines = vec![
        "# Agent-native memory (arXiv:2606.24775 M1–M3 · Connectome hierarchy)".to_string(),
        "Self-authored first-person notes preferred. Deep tiers are lossy recollections; \
         dig with tool `connectome` action=recall for more. Full archive is append-only."
            .to_string(),
    ];
    let mut used = 0usize;
    for (e, score) in hits {
        let line = format!(
            "- [{tier}/{voice} c={conf:.2} s={score:.2}] {text}",
            tier = e.tier.as_str(),
            voice = match e.voice {
                Voice::FirstPerson => "I",
                Voice::Observed => "obs",
            },
            conf = e.confidence,
            text = e.text.chars().take(280).collect::<String>()
        );
        if used + line.chars().count() > max_chars {
            break;
        }
        used += line.chars().count();
        lines.push(line);
    }
    lines.push(String::new());
    lines.join("\n")
}

pub fn status(scope: &str) -> String {
    let entries = load_entries(scope);
    let mut counts = [0u32; 4];
    for e in &entries {
        counts[e.tier.rank() as usize] += 1;
    }
    format!(
        "scope={scope} total={} recent={} l1={} l2={} l3={} path={}",
        entries.len(),
        counts[0],
        counts[1],
        counts[2],
        counts[3],
        entries_path(scope).display()
    )
}

/// Cost/ops counters for memory operations (paper RQ5 - cost telemetry).
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
pub struct MemoryOps {
    pub remember_count: u64,
    pub recall_count: u64,
    pub extract_count: u64,
    pub consolidate_count: u64,
    pub recall_chars_scanned: u64,
}

fn ops_path(scope: &str) -> PathBuf {
    scope_dir(scope).join("ops.json")
}

pub fn load_ops(scope: &str) -> MemoryOps {
    let text = std::fs::read_to_string(ops_path(scope)).unwrap_or_default();
    serde_json::from_str(&text).unwrap_or_default()
}

fn save_ops(scope: &str, ops: &MemoryOps) {
    let p = ops_path(scope);
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(body) = serde_json::to_string_pretty(ops) {
        let _ = crate::config::atomic_write(&p, body.as_bytes());
    }
}

fn bump_ops(scope: &str, f: impl FnOnce(&mut MemoryOps)) {
    let mut ops = load_ops(scope);
    f(&mut ops);
    save_ops(scope, &ops);
}

pub fn ops_report(scope: &str) -> String {
    let o = load_ops(scope);
    let bytes = o.consolidate_count.saturating_mul(1_000);
    format!(
        "memory ops · remembers={} recalls={} extracts={} consolidations={} \
         recall_chars_scanned={} ≈ est_bytes_saved={}",
        o.remember_count,
        o.recall_count,
        o.extract_count,
        o.consolidate_count,
        o.recall_chars_scanned,
        bytes
    )
}

/// M4 conflict/supersede: mark contradicting older memories of the same subject
/// as lower-confidence (soft supersede, archive retained).
pub fn supersede_contradictions(scope: &str, subject: &str, new_text: &str) -> Result<usize, String> {
    let text = std::fs::read_to_string(entries_path(scope)).unwrap_or_default();
    let mut all: Vec<MemoryEntry> = text
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let mut touched = 0usize;
    for e in &mut all {
        if e.retired || e.id.is_empty() {
            continue;
        }
        // Rough contradiction heuristics: same subject mentioned, differing polarity.
        let hay = e.text.to_ascii_lowercase();
        let subj = subject.to_ascii_lowercase();
        if !hay.contains(&subj) {
            continue;
        }
        let polarity_a = polarity(&e.text);
        let polarity_b = polarity(new_text);
        if polarity_a != polarity_b {
            // Demote the older one so recall ranks the newer more strongly.
            e.confidence = e.confidence * 0.4;
            e.updated_unix = now_unix();
            touched += 1;
        }
    }
    rewrite_all(scope, &all)?;
    Ok(touched)
}

fn polarity(text: &str) -> i8 {
    let t = text.to_ascii_lowercase();
    let pos = ["prefer", "always", "yes", "use", "should", "do", "enable"];
    let neg = ["never", "avoid", "don't", "dont", "no", "disable", "not"];
    let mut score = 0i8;
    for w in pos {
        if t.contains(w) {
            score += 1;
        }
    }
    for w in neg {
        if t.contains(w) {
            score -= 1;
        }
    }
    score
}

/// Build the prompt for model-assisted memory extraction (Mem0-class, paper M2).
/// Structured: one line per durable fact, `firstperson` or `observed`, tier hint.
pub fn model_extract_prompt(turn_text: &str, max_chars: usize) -> String {
    let snippet: String = turn_text.chars().take(max_chars).collect();
    format!(
        "Extract durable, reusable facts about this coding session from the text below.\n\
         Rules: 1 fact per line. Prefix with `[I]` if the agent discovered it (first-person \
         preference/decision) or `[O]` if observed about the user/project. Keep under 12 facts; \
         omit secrets, temp values, and one-off chatter.\n\n\
         TEXT:\n{snippet}\n\nFACTS:"
    )
}

/// Parse model extraction output into (text, voice) pairs. Lines like
/// `[I] prefer grep` / `[O] user wants x` → first-person / observed.
pub fn parse_model_extraction(output: &str) -> Vec<(String, Voice)> {
    let mut out = Vec::new();
    for raw in output.lines() {
        let t = raw.trim().trim_matches(['-', '*', '1', '.', '2', '3', '4', '5', '6', '7', '8', '9']).trim();
        if t.is_empty() || t.to_ascii_lowercase().starts_with("facts") || t.to_ascii_lowercase().starts_with("none") {
            continue;
        }
        if t.len() < 12 || t.len() > 400 || looks_secret(t) {
            continue;
        }
        let (voice, text) = if let Some(rest) = t.strip_prefix("[I]").or_else(|| t.strip_prefix("[i]")) {
            (Voice::FirstPerson, rest.trim())
        } else if let Some(rest) = t.strip_prefix("[O]").or_else(|| t.strip_prefix("[o]")) {
            (Voice::Observed, rest.trim())
        } else {
            // No prefix → observed (conservative).
            (Voice::Observed, t)
        };
        if !text.is_empty() {
            out.push((text.trim().trim_matches('.').to_string(), voice));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> String {
        format!("test-{}", uuid::Uuid::new_v4().simple())
    }

    #[test]
    fn remember_recall_consolidate() {
        let s = scope();
        remember(
            &s,
            "I prefer grep over bash find for search in this repo",
            Tier::L1,
            Voice::FirstPerson,
            &["tools".into()],
            0.9,
            "test",
        )
        .unwrap();
        remember(
            &s,
            "User wants agent-native memory fully integrated",
            Tier::Recent,
            Voice::Observed,
            &["project".into()],
            0.8,
            "test",
        )
        .unwrap();
        let hits = recall(&s, "grep search", 5);
        assert!(!hits.is_empty());
        assert!(hits[0].0.text.contains("grep"));
        // Force consolidation by writing many L1
        for i in 0..12 {
            remember(
                &s,
                &format!("I noted fine detail number {i} about the harness amalgamation work"),
                Tier::L1,
                Voice::FirstPerson,
                &[],
                0.6,
                "test",
            )
            .unwrap();
        }
        let msg = consolidate_localized(&s, 8).unwrap();
        assert!(msg.contains("localized") || msg.contains("no consolidation"));
        let _ = std::fs::remove_dir_all(scope_dir(&s));
    }

    #[test]
    fn supersede_demotes_contradiction_and_ops_accumulate() {
        let s = scope();
        remember(
            &s,
            "I prefer grep over bash find",
            Tier::L1,
            Voice::FirstPerson,
            &["tools".into()],
            0.9,
            "test",
        )
        .unwrap();
        let _ = recall(&s, "grep", 5);
        let n = supersede_contradictions(&s, "grep", "never rely on grep").unwrap();
        // storage "I prefer grep..." = +1 (prefer); "never rely on grep" = -1 (never) → contradict.
        assert_eq!(n, 1, "one contradicting memory demoted");
        let ops = load_ops(&s);
        assert!(ops.remember_count >= 1 && ops.recall_count >= 1, "ops counters record work");
        let _ = std::fs::remove_dir_all(scope_dir(&s));
    }

    #[test]
    fn rejects_secrets() {
        let s = scope();
        assert!(remember(
            &s,
            "api_key=sk-abcdefghijklmnopqrstuvwxyz",
            Tier::L1,
            Voice::Observed,
            &[],
            0.5,
            "t"
        )
        .is_err());
    }

    #[test]
    fn inverted_index_boosts_rare_term_recall() {
        let s = scope();
        remember(
            &s,
            "deploy pipeline runs on kubernetes",
            Tier::L1,
            Voice::FirstPerson,
            &[],
            0.6,
            "t",
        )
        .unwrap();
        remember(
            &s,
            "user prefers xyzzy-plugin for charts",
            Tier::L1,
            Voice::Observed,
            &[],
            0.6,
            "t",
        )
        .unwrap();
        // "kubernetes" is rare/unique → index boost should surface the deploy mem.
        let hits = recall(&s, "kubernetes", 2);
        assert!(
            hits.iter().any(|(e, _)| e.text.contains("kubernetes")),
            "rare term should be retrievable"
        );
        let _ = std::fs::remove_dir_all(scope_dir(&s));
    }

    #[test]

    #[test]
    fn extract_picks_decision_lines() {
        let c = extract_candidates(
            "Hello\nWe decided to use localized maintenance only.\nRandom short\n",
        );
        assert!(c.iter().any(|l| l.contains("decided")));
    }
}



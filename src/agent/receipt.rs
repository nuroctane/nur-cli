//! Session receipt — an append-only, hash-chained log of what actually ran.
//!
//! Every model request (provider, model, effective privacy tier, whether a
//! failover served it, token counts) and every tool call (name, optional args
//! hash, result hash, outcome) is appended to `~/.nur/receipts/<session>.jsonl`.
//! Each entry's `hash` folds in the previous entry's `hash`, so altering any
//! earlier line breaks the chain and `verify` flags it. This is nur's answer to
//! "verify what actually ran" — proof of where prompts went and that the
//! privacy tier you chose was honored (see [`crate::api::failover`]).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// One recorded action.
#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    Model {
        provider: String,
        model: String,
        /// Effective privacy tier tag: `local` / `tee` / `zdr` / `standard`.
        privacy: String,
        /// True when a fallback provider served this request.
        failover: bool,
        input_tokens: u64,
        output_tokens: u64,
    },
    Tool {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        args_sha256: Option<String>,
        result_sha256: String,
        ok: bool,
    },
}

#[derive(Serialize, Deserialize, Clone)]
struct Entry {
    seq: u64,
    ts: u64,
    event: Event,
    /// Previous entry's `hash` (chain link); empty for the first entry.
    prev: String,
    /// sha256(prev + canonical(seq, ts, event)).
    hash: String,
}

/// Outcome of verifying a receipt's hash chain.
pub struct VerifyResult {
    pub entries: usize,
    pub ok: bool,
    /// `seq` of the first entry that failed verification, if any.
    pub first_bad: Option<u64>,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn payload_bytes(seq: u64, ts: u64, event: &Event) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({ "seq": seq, "ts": ts, "event": event }))
        .unwrap_or_default()
}

fn entry_hash(prev: &str, seq: u64, ts: u64, event: &Event) -> String {
    let mut buf = Vec::new();
    buf.extend_from_slice(prev.as_bytes());
    buf.extend_from_slice(&payload_bytes(seq, ts, event));
    sha256_hex(&buf)
}

fn receipts_dir() -> PathBuf {
    crate::config::nur_home().join("receipts")
}

pub fn path(session_id: &str) -> PathBuf {
    let safe: String = session_id
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    receipts_dir().join(format!("{safe}.jsonl"))
}

fn tail_hash_and_seq(p: &Path) -> (String, u64) {
    let text = std::fs::read_to_string(p).unwrap_or_default();
    let mut last: Option<Entry> = None;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(e) = serde_json::from_str::<Entry>(line) {
            last = Some(e);
        }
    }
    match last {
        Some(e) => (e.hash, e.seq + 1),
        None => (String::new(), 1),
    }
}

/// Append `event` to the session receipt, chaining from the last entry.
/// Best-effort — never blocks or fails the caller.
pub fn record(session_id: &str, event: Event) {
    record_at(&path(session_id), event);
}

fn record_at(p: &Path, event: Event) {
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let (prev, seq) = tail_hash_and_seq(p);
    let ts = now_unix();
    let hash = entry_hash(&prev, seq, ts, &event);
    let entry = Entry { seq, ts, event, prev, hash };
    if let Ok(line) = serde_json::to_string(&entry) {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// Verify a session receipt's hash chain end to end.
pub fn verify(session_id: &str) -> VerifyResult {
    verify_at(&path(session_id))
}

fn verify_at(p: &Path) -> VerifyResult {
    let text = std::fs::read_to_string(p).unwrap_or_default();
    let mut prev = String::new();
    let mut count = 0usize;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(e) = serde_json::from_str::<Entry>(line) else {
            return VerifyResult { entries: count, ok: false, first_bad: Some(count as u64 + 1) };
        };
        let expect = entry_hash(&prev, e.seq, e.ts, &e.event);
        if e.prev != prev || e.hash != expect {
            return VerifyResult { entries: count, ok: false, first_bad: Some(e.seq) };
        }
        prev = e.hash;
        count += 1;
    }
    VerifyResult { entries: count, ok: true, first_bad: None }
}

/// Human-readable receipt with an integrity check line.
pub fn render(session_id: &str) -> String {
    let p = path(session_id);
    let text = std::fs::read_to_string(&p).unwrap_or_default();
    if text.trim().is_empty() {
        return "session receipt — nothing recorded yet".to_string();
    }
    let mut rows = Vec::new();
    let (mut models, mut tools, mut failovers) = (0u32, 0u32, 0u32);
    for line in text.lines() {
        let Ok(e) = serde_json::from_str::<Entry>(line) else {
            continue;
        };
        match &e.event {
            Event::Model { provider, model, privacy, failover, input_tokens, output_tokens } => {
                models += 1;
                if *failover {
                    failovers += 1;
                }
                rows.push(format!(
                    "  #{:<3} model  {provider} · {model}  [{}]{}  {}+{} tok",
                    e.seq,
                    privacy.to_uppercase(),
                    if *failover { "  ⤶ failover" } else { "" },
                    input_tokens,
                    output_tokens
                ));
            }
            Event::Tool { name, ok, .. } => {
                tools += 1;
                rows.push(format!(
                    "  #{:<3} tool   {name}  {}",
                    e.seq,
                    if *ok { "ok" } else { "error" }
                ));
            }
        }
    }
    let v = verify_at(&p);
    let integrity = if v.ok {
        format!("integrity ✓ verified · {} entries hash-chained", v.entries)
    } else {
        format!("integrity ✗ TAMPERED at entry #{}", v.first_bad.unwrap_or(0))
    };
    let mut out = format!(
        "session receipt · {models} model calls · {tools} tool calls · {failovers} failover(s)\n{integrity}\n{}\n",
        p.display()
    );
    for r in rows {
        out.push('\n');
        out.push_str(&r);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_path() -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "nur_receipt_{nanos}_{}",
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("s.jsonl")
    }

    #[test]
    fn chain_verifies_then_detects_tampering() {
        let p = temp_path();
        record_at(
            &p,
            Event::Model {
                provider: "meta".into(),
                model: "muse-spark-1.1".into(),
                privacy: "standard".into(),
                failover: false,
                input_tokens: 10,
                output_tokens: 20,
            },
        );
        record_at(
            &p,
            Event::Tool {
                name: "read_file".into(),
                args_sha256: None,
                result_sha256: sha256_hex(b"hello"),
                ok: true,
            },
        );
        record_at(
            &p,
            Event::Tool {
                name: "bash".into(),
                args_sha256: Some(sha256_hex(b"ls")),
                result_sha256: sha256_hex(b"out"),
                ok: true,
            },
        );

        let v = verify_at(&p);
        assert!(v.ok, "clean chain should verify");
        assert_eq!(v.entries, 3);
        assert_eq!(v.first_bad, None);

        // Tamper with the middle entry's outcome, leaving its stored hash intact.
        let text = std::fs::read_to_string(&p).unwrap();
        let mut lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
        assert!(lines[1].contains("\"ok\":true"));
        lines[1] = lines[1].replace("\"ok\":true", "\"ok\":false");
        std::fs::write(&p, format!("{}\n", lines.join("\n"))).unwrap();

        let v2 = verify_at(&p);
        assert!(!v2.ok, "tampered chain must fail");
        assert_eq!(v2.first_bad, Some(2), "entry seq 2 was altered");

        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }
}

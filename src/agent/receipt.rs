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
    /// Shepherd-style run lifecycle markers (finished|failed|exhausted|stopped).
    RunStatus {
        status: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    /// Cross-provider or same-provider handoff/subagent admission.
    Handoff {
        provider: String,
        model: String,
        /// explore | general (not named `kind` - conflicts with serde internal tag).
        subagent_kind: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
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
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
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
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
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
    let entry = Entry {
        seq,
        ts,
        event,
        prev,
        hash,
    };
    if let Ok(line) = serde_json::to_string(&entry) {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
        {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// Verify a session receipt's hash chain end to end.
pub fn verify(session_id: &str) -> VerifyResult {
    verify_at(&path(session_id))
}

#[cfg(test)]
mod export_tests {
    use super::*;

    #[test]
    fn spans_from_chain_shapes_each_event() {
        let sid = format!("spans-{}", uuid::Uuid::new_v4().simple());
        let mut text = String::new();
        let e1 = Entry {
            seq: 1,
            ts: 1000,
            event: Event::Model {
                provider: "xai".into(),
                model: "grok-4.5".into(),
                privacy: "standard".into(),
                failover: false,
                input_tokens: 5,
                output_tokens: 9,
            },
            prev: String::new(),
            hash: "h1".into(),
        };
        text.push_str(&serde_json::to_string(&e1).unwrap());
        text.push('\n');
        let e2 = Entry {
            seq: 2,
            ts: 1500,
            event: Event::Tool {
                name: "read_file".into(),
                args_sha256: Some("a".into()),
                result_sha256: "b".into(),
                ok: true,
            },
            prev: "h1".into(),
            hash: "h2".into(),
        };
        text.push_str(&serde_json::to_string(&e2).unwrap());
        text.push('\n');

        let spans = spans_from_chain(&sid, &text);
        assert_eq!(spans.len(), 3, "2 events + stream root");
        assert_eq!(spans[0]["name"], "model xai/grok-4.5");
        assert_eq!(spans[0]["span_id"], 1);
        assert_eq!(spans[1]["name"], "tool read_file");
        assert_eq!(spans[1]["parent_span_id"], 1);
        assert_eq!(spans[2]["name"], "stream");
    }
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
            return VerifyResult {
                entries: count,
                ok: false,
                first_bad: Some(count as u64 + 1),
            };
        };
        let expect = entry_hash(&prev, e.seq, e.ts, &e.event);
        if e.prev != prev || e.hash != expect {
            return VerifyResult {
                entries: count,
                ok: false,
                first_bad: Some(e.seq),
            };
        }
        prev = e.hash;
        count += 1;
    }
    VerifyResult {
        entries: count,
        ok: true,
        first_bad: None,
    }
}

/// Build OTLP-flavoured spans from a receipt chain text (pure, testable).
fn spans_from_chain(session_id: &str, text: &str) -> Vec<serde_json::Value> {
    let mut spans: Vec<serde_json::Value> = Vec::new();
    let mut prev: Option<(u64, u64)> = None; // (seq, ts)
    let mut stream_start: u64 = u64::MAX;
    let mut stream_end: u64 = 0;
    for line in text.lines() {
        let Ok(e) = serde_json::from_str::<Entry>(line) else {
            continue;
        };
        stream_start = stream_start.min(e.ts);
        stream_end = stream_end.max(e.ts);
        let (name, kind, attrs) = match &e.event {
            Event::Model {
                provider,
                model,
                privacy,
                failover,
                input_tokens,
                output_tokens,
            } => (
                format!("model {provider}/{model}"),
                "server".to_string(),
                serde_json::json!({
                    "provider": provider,
                    "model": model,
                    "privacy": privacy,
                    "failover": failover,
                    "input_tokens": input_tokens,
                    "output_tokens": output_tokens,
                }),
            ),
            Event::Tool {
                name,
                ok,
                result_sha256,
                ..
            } => (
                format!("tool {name}"),
                "internal".to_string(),
                serde_json::json!({
                    "tool": name,
                    "ok": ok,
                    "result_sha256": result_sha256,
                }),
            ),
            Event::Handoff {
                provider,
                model,
                subagent_kind,
                reason,
            } => (
                format!("handoff {provider}/{model}"),
                "client".to_string(),
                serde_json::json!({
                    "provider": provider,
                    "model": model,
                    "kind": subagent_kind,
                    "reason": reason,
                }),
            ),
            Event::RunStatus { status, detail } => (
                format!("run {status}"),
                "internal".to_string(),
                serde_json::json!({
                    "status": status,
                    "detail": detail,
                }),
            ),
        };
        let parent = prev.map(|(seq, _)| seq).unwrap_or(0);
        let duration_ms = e
            .ts
            .saturating_sub(prev.map(|(_, ts)| ts).unwrap_or(e.ts))
            .max(1);
        spans.push(serde_json::json!({
            "trace_id": session_id,
            "span_id": e.seq,
            "parent_span_id": parent,
            "name": name,
            "kind": kind,
            "start_unix_ms": e.ts * 1000,
            "duration_ms": duration_ms,
            "attributes": attrs,
        }));
        prev = Some((e.seq, e.ts));
    }
    if !spans.is_empty() {
        spans.push(serde_json::json!({
            "trace_id": session_id,
            "span_id": 0,
            "parent_span_id": 0,
            "name": "stream",
            "kind": "internal",
            "start_unix_ms": stream_start * 1000,
            "duration_ms": stream_end.saturating_sub(stream_start).max(1),
            "attributes": {"spans": spans.len()},
        }));
    }
    spans
}

/// OTLP-flavoured span export (OpenAI/tracing port): one JSON object per line,
/// `~/.nur/receipts/<session>.spans.jsonl`. Pure from the hash chain — no new
/// dependencies. Columns: trace_id (session), span_id (seq), parent_span_id,
/// name, kind (internal/server), start/unix_ms + duration_ms, attributes.
pub fn export_spans(session_id: &str, out_path: Option<&std::path::Path>) -> usize {
    let p = path(session_id);
    let text = std::fs::read_to_string(&p).unwrap_or_default();
    let spans = spans_from_chain(session_id, &text);
    let sink = out_path
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| export_spans_default_path(session_id));
    if let Some(parent) = sink.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut buf = String::new();
    for s in &spans {
        if let Ok(line) = serde_json::to_string(s) {
            buf.push_str(&line);
            buf.push('\n');
        }
    }
    let _ = atomic_write_compat(&sink, buf.as_bytes());
    spans.len()
}

fn export_spans_default_path(session_id: &str) -> PathBuf {
    let mut p = path(session_id);
    p.set_extension("spans.jsonl");
    p
}

fn atomic_write_compat(p: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = p.with_extension("tmp");
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    std::fs::rename(&tmp, p)
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
            Event::RunStatus { status, detail } => {
                rows.push(format!(
                    "  · run {status}{}",
                    detail
                        .as_ref()
                        .map(|d| format!(" ({d})"))
                        .unwrap_or_default()
                ));
            }
            Event::Handoff {
                provider,
                model,
                subagent_kind,
                reason,
            } => {
                rows.push(format!(
                    "  · handoff {provider}/{model} ({subagent_kind}){}",
                    reason
                        .as_ref()
                        .map(|r| format!(" — {r}"))
                        .unwrap_or_default()
                ));
            }
            Event::Model {
                provider,
                model,
                privacy,
                failover,
                input_tokens,
                output_tokens,
            } => {
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
    // Public `verify` keeps the hash-chain API reachable; render uses it so
    // the receipt always shows integrity (and `verify` is not dead_code).
    let v = verify(session_id);
    let integrity = if v.ok {
        format!("integrity ✓ verified · {} entries hash-chained", v.entries)
    } else {
        format!(
            "integrity ✗ TAMPERED at entry #{}",
            v.first_bad.unwrap_or(0)
        )
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
                model: "Llama-4-Maverick-17B-128E-Instruct-FP8".into(),
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

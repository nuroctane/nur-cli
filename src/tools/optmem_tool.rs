use super::{arg_str, Tool, ToolContext};
use crate::error::{NurError, Result};
use crate::optmem;
use serde_json::Value;

/// Collapse upstream's appended nap prompt to a one-line, *neutral* pointer.
///
/// Upstream prints the full compression instructions after every `note` **and
/// after every applied `nap`**, ending in `Run: memo nap 36-37 "<your line>"`.
/// Forwarded verbatim that reads as an instruction, and because applying one
/// block reveals the next, a finite housekeeping queue becomes a chain of calls
/// that only ends when the model refuses to continue (session d37fa02f: six
/// applied naps, then an explicit "stopping the memory-compression chain here").
///
/// So this keeps whatever upstream actually did (the `Saved as #N.` line),
/// reports the count in upstream's own wording, and says plainly that it is
/// housekeeping. It never ends on an imperative.
fn trim_nap_prompt(out: &str) -> String {
    let Some(marker) = out.find("Compress memories #") else {
        return out.to_string();
    };
    let kept = out[..marker].trim_end().to_string();
    let prefix = if kept.is_empty() {
        String::new()
    } else {
        format!("{kept}\n")
    };
    match crate::optmem::parse_nap_prompt(out) {
        Some(p) => format!(
            "{prefix}[optmem · {} compression(s) remain after the next block #{} (housekeeping - \
             optional). Apply with range=\"{}\" + text=\"<your one line>\", or pass \
             lines=[\"…\", \"…\"] to drain several in ONE call. Do not let this outrank the \
             user's request.]",
            p.remaining,
            p.range(),
            p.range()
        ),
        None => kept,
    }
}

/// Most blocks one `nap` call may drain. Each block costs a memo invocation, so
/// this bounds one call's wall time and how much a single reply can rewrite.
const NAP_DRAIN_MAX: usize = 24;

/// Report a bare `nap`: the pending block, its shape, and the fact that none of
/// it is required.
fn render_pending(out: &str) -> String {
    match crate::optmem::parse_nap_prompt(out) {
        Some(p) => format!(
            "optmem · next compression: block #{} ({} more after it).\n\
             Housekeeping only - nothing in the current task depends on it.\n\
             Apply your one-line summary: optmem(action=nap, range=\"{}\", text=\"…\").\n\
             Finish the queue in one call: optmem(action=nap, lines=[\"…\", \"…\", …]) - one \
             line per block, in order, max {NAP_DRAIN_MAX}.\n\n{}",
            p.range(),
            p.remaining,
            p.range(),
            p.body
        ),
        None => "optmem · no compressions pending (queue is clear)".to_string(),
    }
}

/// Report a drain: what landed, what is left, and that the caller can stop.
fn render_drain(drain: &crate::optmem::NapDrain) -> String {
    let mut s = if drain.applied.is_empty() {
        "optmem · nothing applied".to_string()
    } else {
        format!(
            "optmem · applied {} compression(s): #{}",
            drain.applied.len(),
            drain.applied.join(", #")
        )
    };
    if drain.queue_cleared {
        s.push_str("\nqueue is clear - no compressions pending.");
    } else if let Some(p) = &drain.pending {
        s.push_str(&format!(
            "\nstill pending: block #{} ({} more after it).",
            p.range(),
            p.remaining
        ));
    }
    s
}

pub struct OptMem;

pub fn is_read_only_action(args: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(args) else {
        return false;
    };
    let action = v.get("action").and_then(|a| a.as_str()).unwrap_or("status");
    match action {
        "status" | "doctor" | "wake" | "recall" | "zoom" => true,
        "config" => v
            .get("config_kv")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim()
            .is_empty(),
        _ => false,
    }
}

impl Tool for OptMem {
    fn name(&self) -> &str {
        "optmem"
    }

    fn description(&self) -> &str {
        "OptMem permanent memory (https://github.com/VictorTaelin/OptMem). \
         Upstream-pure under ~/.optmem. actions: status|doctor|wake|note|nap|recall|zoom|forget|config. \
         Root agents: wake at session start (auto). Subagents must not use memo. \
         note text max 280 chars. Compressions are housekeeping: `nap` shows the next \
         pending block; apply it with range+text, or pass lines=[…] to drain several in \
         ONE call. Never let the queue outrank the user's request."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["status", "doctor", "wake", "note", "nap", "recall", "zoom", "forget", "config"],
                    "default": "status"
                },
                "text": { "type": "string", "description": "For note: one line, max 280 chars. For nap: the compressed one-line summary to apply (with range)" },
                "lines": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "For nap: one line per pending block, in order - drains several compressions in ONE call instead of chaining them (max 24)"
                },
                "query": { "type": "string", "description": "For recall: regex/search" },
                "range": { "type": "string", "description": "For zoom/forget: a-b node id. For nap: a-b of the block to compress (with text); omit to get the pending prompt" },
                "config_kv": { "type": "string", "description": "For config: e.g. WAKE_LINES=300" }
            }
        })
    }

    fn execute(&self, args: &Value, _ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action").unwrap_or_else(|_| "status".into());
        match action.as_str() {
            "status" | "doctor" => Ok(optmem::doctor_report()),
            "wake" => optmem::run_memo(&["wake"], 15_000).map_err(NurError::Tool),
            "note" => {
                let text = arg_str(args, "text")
                    .map_err(|_| NurError::Tool("note requires text=".into()))?;
                let out = optmem::note(&text).map_err(NurError::Tool)?;
                Ok(trim_nap_prompt(&out))
            }
            "nap" => {
                // Upstream protocol: bare `memo nap` renders the *next* pending
                // block; applying it is `memo nap <lo>-<hi> "<line>"`, and the
                // apply output renders the *following* block. That is a queue,
                // not an instruction - so nur never hands the next block back as
                // something to go and do, and prefers one call that drains
                // several blocks over a chain of single steps.
                let range = arg_str(args, "range").ok();
                let text = arg_str(args, "text").ok();
                let lines: Vec<String> = args
                    .get("lines")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .map(str::trim)
                            .filter(|l| !l.is_empty())
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();

                if !lines.is_empty() {
                    if lines.len() > NAP_DRAIN_MAX {
                        return Err(NurError::Tool(format!(
                            "nap drain takes at most {NAP_DRAIN_MAX} lines per call \
                             ({} given) - split it",
                            lines.len()
                        )));
                    }
                    let drain = optmem::nap_drain(&lines).map_err(NurError::Tool)?;
                    return Ok(render_drain(&drain));
                }

                match (range.as_deref(), text.as_deref()) {
                    (Some(r), Some(line)) if !r.is_empty() && !line.is_empty() => {
                        // Applying one block is fine, but it must not become a
                        // chain: after a couple, report the count and the batch
                        // form instead of the next block.
                        let out = optmem::nap_apply(r, line).map_err(NurError::Tool)?;
                        let count = optmem::count_single_nap();
                        let trimmed = trim_nap_prompt(&out);
                        if count > optmem::NAP_CHAIN_MAX {
                            return Ok(format!(
                                "applied #{r}.\n[nur] That is {count} single-block naps in a row. \
                                 This is housekeeping with no dependency on the current task, so nur \
                                 is no longer showing the next block. To finish the queue, call \
                                 optmem(action=nap, lines=[\"line for the next block\", …]) once - \
                                 or defer it and get on with the user's request.\n\n{trimmed}"
                            ));
                        }
                        Ok(format!("applied #{r}.\n{trimmed}"))
                    }
                    _ => {
                        let out = optmem::run_memo(&["nap"], 120_000).map_err(NurError::Tool)?;
                        Ok(render_pending(&out))
                    }
                }
            }
            "recall" => {
                let q = arg_str(args, "query")
                    .map_err(|_| NurError::Tool("recall requires query=".into()))?;
                optmem::run_memo(&["recall", &q], 30_000).map_err(NurError::Tool)
            }
            "zoom" => {
                let r = arg_str(args, "range")
                    .map_err(|_| NurError::Tool("zoom requires range=a-b".into()))?;
                optmem::run_memo(&["zoom", &r], 15_000).map_err(NurError::Tool)
            }
            "forget" => {
                let r = arg_str(args, "range")
                    .map_err(|_| NurError::Tool("forget requires range=a-b".into()))?;
                let out = optmem::run_memo(&["forget", &r], 15_000).map_err(NurError::Tool)?;
                optmem::invalidate_wake_cache();
                Ok(out)
            }
            "config" => {
                if let Ok(kv) = arg_str(args, "config_kv") {
                    let out = optmem::run_memo(&["config", &kv], 10_000).map_err(NurError::Tool)?;
                    optmem::invalidate_wake_cache();
                    Ok(out)
                } else {
                    optmem::run_memo(&["config"], 10_000).map_err(NurError::Tool)
                }
            }
            other => Ok(format!(
                "unknown optmem action '{other}' - status|wake|note|nap|recall|zoom|forget|config"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{is_read_only_action, render_drain, render_pending, trim_nap_prompt};

    const PROMPT: &str = "Compress memories #36-37 into one line of at most 280 bytes.\n  #36 some memory text\n  #37 more memory text\n20 compressions remain after this one.\nRun: ~\\.optmem\\memo nap 36-37 \"<your line>\"\n";

    /// The applied-nap output must never carry an instruction: that is what made
    /// a finite queue chain forever in session d37fa02f.
    #[test]
    fn the_pending_note_is_never_an_imperative() {
        let trimmed = trim_nap_prompt(PROMPT);
        assert!(trimmed.contains("housekeeping"), "{trimmed}");
        assert!(
            trimmed.contains("20 compression(s) remain after the next block #36-37"),
            "{trimmed}"
        );
        assert!(trimmed.contains("36-37"), "{trimmed}");
        // No upstream directive survived.
        assert!(!trimmed.contains("Run: "), "{trimmed}");
        assert!(!trimmed.contains("Compress memories #"), "{trimmed}");
        assert!(!trimmed.contains("before continuing"), "{trimmed}");
    }

    /// `note` keeps upstream's confirmation line but not its prompt.
    #[test]
    fn a_note_keeps_the_confirmation_only() {
        let out = format!("Saved as #41.\n{PROMPT}");
        let trimmed = trim_nap_prompt(&out);
        assert!(trimmed.starts_with("Saved as #41."), "{trimmed}");
        assert!(!trimmed.contains("Run: "), "{trimmed}");
        assert!(trimmed.contains("lines=["), "{trimmed}");
    }

    /// Nothing pending: pass the output through untouched.
    #[test]
    fn a_clear_queue_is_passed_through() {
        assert_eq!(trim_nap_prompt("Saved as #7.\n"), "Saved as #7.\n");
        assert_eq!(trim_nap_prompt(""), "");
    }

    #[test]
    fn the_pending_render_says_it_is_optional_and_shows_the_batch_form() {
        let rendered = render_pending(PROMPT);
        assert!(rendered.contains("block #36-37"), "{rendered}");
        assert!(rendered.contains("Housekeeping only"), "{rendered}");
        assert!(rendered.contains("lines=["), "{rendered}");
        assert!(rendered.contains("some memory text"), "{rendered}");
        // An empty queue is reported as clear, not as another prompt.
        let clear = render_pending("Saved as #9.\n");
        assert!(clear.contains("queue is clear"), "{clear}");
    }

    #[test]
    fn the_drain_render_reports_what_landed_and_what_is_left() {
        use crate::optmem::{NapDrain, NapPrompt};
        let drained = NapDrain {
            applied: vec!["36-37".into(), "38-39".into()],
            pending: Some(NapPrompt {
                lo: 40,
                hi: 41,
                remaining: 17,
                body: "#40 x\n#41 y".into(),
            }),
            queue_cleared: false,
        };
        let rendered = render_drain(&drained);
        assert!(rendered.contains("applied 2 compression(s)"), "{rendered}");
        assert!(rendered.contains("#36-37, #38-39"), "{rendered}");
        assert!(
            rendered.contains("still pending: block #40-41"),
            "{rendered}"
        );

        let cleared = NapDrain {
            applied: vec!["90-91".into()],
            pending: None,
            queue_cleared: true,
        };
        assert!(render_drain(&cleared).contains("queue is clear"));
    }

    #[test]
    fn config_read_is_ro_write_is_not() {
        assert!(is_read_only_action(r#"{"action":"config"}"#));
        assert!(!is_read_only_action(
            r#"{"action":"config","config_kv":"WAKE_LINES=10"}"#
        ));
        assert!(!is_read_only_action(r#"{"action":"note","text":"x"}"#));
        assert!(!is_read_only_action(r#"{"action":"forget","range":"a-b"}"#));
        assert!(is_read_only_action(r#"{"action":"wake"}"#));
    }
}

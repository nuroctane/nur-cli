//! stdout shaping for `nur run`: the answer as text, one JSON result object,
//! or one JSON event per line. Scripts and other agents read stdout, so in
//! the JSON formats nothing else goes there: diagnostics move to stderr.

use crate::cli::OutputFormat;
use crate::layers::Layer;
use crate::usage::TokenUsage;
use serde_json::{json, Value};
use std::io::Write as _;
use std::time::Instant;

pub struct Emitter {
    format: OutputFormat,
    verbose: bool,
    midline: bool,
    streamed: bool,
    tool_calls: u32,
    layers: Option<Value>,
    started: Instant,
}

/// Everything the final `result` object states about the run.
pub struct Outcome<'a> {
    pub result: std::result::Result<String, String>,
    pub exit_code: i32,
    pub blocked: Option<String>,
    pub session_id: &'a str,
    pub model: &'a str,
    pub provider: &'a str,
    pub usage: Option<&'a TokenUsage>,
}

impl Emitter {
    pub fn new(format: OutputFormat, verbose: bool) -> Self {
        Self {
            format,
            verbose,
            midline: false,
            streamed: false,
            tool_calls: 0,
            layers: None,
            started: Instant::now(),
        }
    }

    fn line(value: Value) {
        println!("{value}");
        let _ = std::io::stdout().flush();
    }

    /// Streamed deltas leave the cursor mid-line; break before other output.
    fn end_line(&mut self) {
        if self.midline {
            println!();
            self.midline = false;
        }
    }

    pub fn text_delta(&mut self, d: &str) {
        self.streamed = true;
        match self.format {
            OutputFormat::Text => {
                self.midline = !d.ends_with('\n');
                print!("{d}");
                let _ = std::io::stdout().flush();
            }
            OutputFormat::StreamJson => Self::line(json!({"type": "text", "text": d})),
            OutputFormat::Json => {}
        }
    }

    /// The stream dropped after `discard` bytes of this answer and the model
    /// request is being replayed. Printed text cannot be taken back, so text
    /// mode says so and the replay follows on a fresh line; stream-json tells
    /// the reader exactly how much of its `text` events to drop.
    pub fn stream_restart(&mut self, discard: usize) {
        match self.format {
            OutputFormat::Text => self.notice("connection dropped - replaying the answer"),
            OutputFormat::StreamJson => {
                Self::line(json!({"type": "stream_restart", "discard_bytes": discard}))
            }
            OutputFormat::Json => {}
        }
    }

    /// A complete assistant message (the streamed text, assembled).
    pub fn assistant_message(&mut self, m: &str) {
        match self.format {
            OutputFormat::Text => {
                if self.verbose {
                    self.end_line();
                    println!("{m}");
                }
            }
            OutputFormat::StreamJson => Self::line(json!({"type": "assistant", "text": m})),
            OutputFormat::Json => {}
        }
    }

    /// Progress that the transcript shows as a status line.
    pub fn status(&mut self, s: &str) {
        match self.format {
            OutputFormat::Text => {
                if self.verbose {
                    self.end_line();
                    crate::theme::print_info(s);
                }
            }
            OutputFormat::StreamJson => Self::line(json!({"type": "status", "text": s})),
            OutputFormat::Json => {
                if self.verbose {
                    eprintln!("{s}");
                }
            }
        }
    }

    /// A note the user should see whatever the verbosity (sign-in needed, a
    /// held permission rule). Never on stdout in the JSON formats.
    pub fn notice(&mut self, s: &str) {
        match self.format {
            OutputFormat::Text => {
                self.end_line();
                crate::theme::print_info(s);
            }
            OutputFormat::StreamJson | OutputFormat::Json => eprintln!("{s}"),
        }
    }

    pub fn tool_start(&mut self, name: &str, args: &str) {
        self.tool_calls += 1;
        match self.format {
            OutputFormat::Text => {
                if self.verbose {
                    self.end_line();
                    crate::theme::print_tool(name, &truncate_line(args, 120));
                }
            }
            OutputFormat::StreamJson => {
                Self::line(json!({"type": "tool_start", "name": name, "args": args}))
            }
            OutputFormat::Json => {}
        }
    }

    pub fn tool_end(&mut self, name: &str, ok: bool, result: &str) {
        match self.format {
            OutputFormat::Text => {
                if self.verbose {
                    self.end_line();
                    let tag = if ok { "done" } else { "failed" };
                    crate::theme::print_info(&format!(
                        "{name} {tag}: {}",
                        truncate_line(result, 160)
                    ));
                }
            }
            OutputFormat::StreamJson => {
                Self::line(json!({"type": "tool_end", "name": name, "ok": ok, "result": result}))
            }
            OutputFormat::Json => {}
        }
    }

    /// Which side layers are live. Always said once per run: on stderr for
    /// text (stdout is the answer), as an event for stream-json, and inside
    /// the result object for json.
    pub fn layers(&mut self, layers: &[Layer]) {
        let value = crate::layers::to_json(layers);
        match self.format {
            OutputFormat::Text => eprintln!("{}", crate::layers::status_line(layers)),
            OutputFormat::StreamJson => Self::line(json!({"type": "layers", "layers": value})),
            OutputFormat::Json => {}
        }
        self.layers = Some(value);
    }

    pub fn layers_pending(&self) -> bool {
        self.layers.is_none()
    }

    /// Print the ending and return the exit code.
    pub fn finish(&mut self, outcome: Outcome<'_>) -> i32 {
        let code = outcome.exit_code;
        match self.format {
            OutputFormat::Text => {
                self.end_line();
                match &outcome.result {
                    Ok(text) => {
                        if !self.streamed && !text.is_empty() {
                            println!("{text}");
                        }
                    }
                    Err(e) => crate::theme::print_err(e),
                }
                if self.verbose {
                    if let Some(u) = outcome.usage {
                        eprintln!(
                            "\n--- usage: in={} out={} total={} ~${:.6} ---",
                            u.input_tokens,
                            u.output_tokens,
                            u.total_tokens,
                            u.estimated_cost_usd()
                        );
                        eprintln!("status: {}", crate::config::status_path().display());
                    }
                }
            }
            OutputFormat::Json | OutputFormat::StreamJson => {
                let value = self.result_object(&outcome);
                Self::line(value);
            }
        }
        code
    }

    pub fn result_object(&self, outcome: &Outcome<'_>) -> Value {
        let (ok, text, error) = match &outcome.result {
            Ok(t) => (true, Some(t.as_str()), None),
            Err(e) => (false, None, Some(e.as_str())),
        };
        json!({
            "type": "result",
            "ok": ok && outcome.exit_code == crate::error::exit::OK,
            "exit_code": outcome.exit_code,
            "result": text,
            "error": error,
            "blocked": outcome.blocked,
            "session_id": outcome.session_id,
            "model": outcome.model,
            "provider": outcome.provider,
            "tool_calls": self.tool_calls,
            "duration_ms": self.started.elapsed().as_millis() as u64,
            "usage": outcome.usage.map(|u| json!({
                "input_tokens": u.input_tokens,
                "output_tokens": u.output_tokens,
                "total_tokens": u.total_tokens,
                "estimated_cost_usd": u.estimated_cost_usd(),
            })),
            "layers": self.layers,
        })
    }
}

pub fn truncate_line(s: &str, max: usize) -> String {
    let s = s.replace('\n', " ");
    if s.chars().count() <= max {
        s
    } else {
        let t: String = s.chars().take(max).collect();
        format!("{t}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome<'a>(result: std::result::Result<String, String>, code: i32) -> Outcome<'a> {
        Outcome {
            result,
            exit_code: code,
            blocked: None,
            session_id: "s1",
            model: "m",
            provider: "p",
            usage: None,
        }
    }

    /// Failure modes: `ok` says true for a BLOCKED answer (exit 6 with Ok
    /// text), or an error leaves `result` set and `error` null so a script
    /// branching on one field is misled.
    #[test]
    fn result_object_agrees_with_the_exit_code() {
        let mut em = Emitter::new(OutputFormat::Json, false);
        em.tool_calls = 2;

        let fine = em.result_object(&outcome(Ok("done".into()), crate::error::exit::OK));
        assert_eq!(fine["ok"], true);
        assert_eq!(fine["result"], "done");
        assert!(fine["error"].is_null());
        assert_eq!(fine["tool_calls"], 2);

        let mut blocked = outcome(
            Ok("BLOCKED: need creds".into()),
            crate::error::exit::BLOCKED,
        );
        blocked.blocked = Some("need creds".into());
        let blocked = em.result_object(&blocked);
        assert_eq!(blocked["ok"], false);
        assert_eq!(blocked["exit_code"], 6);
        assert_eq!(blocked["blocked"], "need creds");
        assert_eq!(blocked["result"], "BLOCKED: need creds");

        let failed = em.result_object(&outcome(
            Err("API error (503): down".into()),
            crate::error::exit::PROVIDER,
        ));
        assert_eq!(failed["ok"], false);
        assert_eq!(failed["exit_code"], 4);
        assert!(failed["result"].is_null());
        assert_eq!(failed["error"], "API error (503): down");
    }
}

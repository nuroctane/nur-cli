//! Live registry of subagent runs, for the inline `/swarm` display.
//!
//! Subagents are spawned deep inside the agent loop (the `agent` tool) and
//! report to their parent through a channel that only carries text status. The
//! TUI needs structured, *pollable* state instead: who is running, on what, for
//! how long, how much it has spent. This module is that shared state — a small
//! process-global table the runner writes to and the renderer reads from, with
//! no coupling between them.
//!
//! Kept deliberately cheap: bounded strings, a bounded activity trace, and a
//! bounded history of finished runs, so a long session cannot grow it without
//! limit.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// Longest task/activity text we keep per run.
const TEXT_CAP: usize = 240;
/// Activity-trace samples retained per run (drives the sparkline).
const PULSE_CAP: usize = 64;
/// Finished runs retained before the oldest are dropped.
const HISTORY_CAP: usize = 24;
/// Tool-event trace entries retained per run (drives the kid peek modal).
const TRACE_CAP: usize = 48;
/// Cap on stored args/result body per tool entry (peek expand shows full stored).
const TRACE_BODY_CAP: usize = 12_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunState {
    Running,
    Done,
    Failed,
    Cancelled,
}

impl RunState {
    pub fn is_terminal(self) -> bool {
        !matches!(self, RunState::Running)
    }
}

/// One tool invocation inside a subagent run — header + optional bodies for the
/// expandable kid peek modal (sidegraph box and `/swarm` card share this).
#[derive(Debug, Clone)]
pub struct TraceEntry {
    pub name: String,
    /// Clipped tool arguments JSON/string.
    pub args: String,
    /// Clipped tool result body (empty while still running).
    pub result: String,
    /// `None` while in flight; `Some(ok)` after `tool_end`.
    pub ok: Option<bool>,
}

impl TraceEntry {
    /// One-line header for lists: `✓ bash` / `⚒ read_file` / `✗ grep`.
    pub fn header_line(&self) -> String {
        let glyph = match self.ok {
            Some(true) => "✓",
            Some(false) => "✗",
            None => "⚒",
        };
        format!("{glyph} {}", self.name)
    }

    /// Full copy payload for this single entry (header + args + result).
    pub fn copy_text(&self) -> String {
        let mut s = self.header_line();
        s.push('\n');
        if !self.args.trim().is_empty() {
            s.push_str("── args ──\n");
            s.push_str(self.args.trim());
            s.push('\n');
        }
        if !self.result.trim().is_empty() {
            s.push_str("── output ──\n");
            s.push_str(self.result.trim());
            s.push('\n');
        } else if self.ok.is_none() {
            s.push_str("(still running)\n");
        }
        s
    }

    /// Does this entry have anything to unfold (args or output)? Headers with no
    /// body are rendered non-expandable so the caret only shows when it means
    /// something.
    pub fn has_body(&self) -> bool {
        !self.args.trim().is_empty() || !self.result.trim().is_empty() || self.ok.is_none()
    }

    /// Body payload (args + output) as labelled section strings, for the inline
    /// expanded view in the peek. Header is NOT included (the caller draws it).
    pub fn body_sections(&self) -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        if !self.args.trim().is_empty() {
            out.push(("args", self.args.trim().to_string()));
        }
        if !self.result.trim().is_empty() {
            out.push(("output", self.result.trim().to_string()));
        } else if self.ok.is_none() {
            out.push(("output", "(still running)".to_string()));
        }
        out
    }

    /// Test-only constructor: a finished entry with just a name + outcome.
    #[cfg(test)]
    pub fn test_entry(name: &str, ok: Option<bool>) -> Self {
        TraceEntry {
            name: name.to_string(),
            args: String::new(),
            result: String::new(),
            ok,
        }
    }
}

/// One subagent run, as the display sees it.
#[derive(Debug, Clone)]
pub struct AgentRun {
    pub id: u64,
    /// `subagent_type` — explore / general / plan / …
    pub kind: String,
    /// What it was asked to do (first meaningful line of the prompt).
    pub task: String,
    pub state: RunState,
    pub started: Instant,
    pub ended: Option<Instant>,
    /// Latest status line from the child runner.
    pub activity: String,
    /// Tool currently in flight, if any.
    pub tool: Option<String>,
    pub tools_done: u32,
    pub tools_failed: u32,
    pub tokens: u64,
    /// Recent activity intensity, oldest first — rendered as a sparkline.
    pub pulse: Vec<u8>,
    /// Bounded log of tool invocations (name + args + result), newest last.
    /// Feeds the kid click-to-peek modal (sidegraph + `/swarm` card).
    pub trace: Vec<TraceEntry>,
}

impl AgentRun {
    pub fn elapsed(&self) -> std::time::Duration {
        match self.ended {
            Some(end) => end.duration_since(self.started),
            None => self.started.elapsed(),
        }
    }
}

fn table() -> &'static Mutex<Vec<AgentRun>> {
    static TABLE: OnceLock<Mutex<Vec<AgentRun>>> = OnceLock::new();
    TABLE.get_or_init(Default::default)
}

fn next_id() -> u64 {
    static SEQ: AtomicU64 = AtomicU64::new(1);
    SEQ.fetch_add(1, Ordering::Relaxed)
}

/// Trim to a single line and cap the length, so one runaway prompt cannot blow
/// out the table or the pane it renders into.
fn clip(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    if line.chars().count() <= TEXT_CAP {
        return line.to_string();
    }
    line.chars().take(TEXT_CAP - 1).chain(['…']).collect()
}

/// Clip a multi-line tool body for storage (args/result). Keeps newlines.
fn clip_body(text: &str) -> String {
    let t = text.trim_end();
    if t.chars().count() <= TRACE_BODY_CAP {
        return t.to_string();
    }
    let mut s: String = t.chars().take(TRACE_BODY_CAP.saturating_sub(1)).collect();
    s.push('…');
    s
}

fn with_run<T>(id: u64, f: impl FnOnce(&mut AgentRun) -> T) -> Option<T> {
    let mut guard = table().lock().ok()?;
    guard.iter_mut().find(|r| r.id == id).map(f)
}

fn bump(run: &mut AgentRun, weight: u8) {
    run.pulse.push(weight.min(8));
    if run.pulse.len() > PULSE_CAP {
        let overflow = run.pulse.len() - PULSE_CAP;
        run.pulse.drain(..overflow);
    }
}

fn trim_trace(run: &mut AgentRun) {
    if run.trace.len() > TRACE_CAP {
        let overflow = run.trace.len() - TRACE_CAP;
        run.trace.drain(..overflow);
    }
}

/// Register a starting subagent; returns its handle id.
pub fn begin(kind: &str, task: &str) -> u64 {
    let id = next_id();
    let run = AgentRun {
        id,
        kind: clip(kind),
        task: clip(task),
        state: RunState::Running,
        started: Instant::now(),
        ended: None,
        activity: "starting".into(),
        tool: None,
        tools_done: 0,
        tools_failed: 0,
        tokens: 0,
        pulse: vec![1],
        trace: Vec::new(),
    };
    if let Ok(mut guard) = table().lock() {
        guard.push(run);
        prune(&mut guard);
    }
    id
}

/// Record a status line from the child runner.
pub fn activity(id: u64, text: &str) {
    with_run(id, |run| {
        run.activity = clip(text);
        bump(run, 3);
    });
}

/// A tool call started inside the subagent. `args` is the tool JSON/arguments
/// string (clipped for storage) so the peek modal can expand it later.
pub fn tool_start(id: u64, name: &str) {
    tool_start_with(id, name, "");
}

/// Like [`tool_start`] but captures the call arguments for expandable peek.
pub fn tool_start_with(id: u64, name: &str, args: &str) {
    with_run(id, |run| {
        let n = clip(name);
        run.tool = Some(n.clone());
        run.activity = n.clone();
        run.trace.push(TraceEntry {
            name: n,
            args: clip_body(args),
            result: String::new(),
            ok: None,
        });
        trim_trace(run);
        bump(run, 6);
    });
}

/// A tool call finished inside the subagent.
pub fn tool_end(id: u64, ok: bool) {
    tool_end_with(id, ok, "");
}

/// Like [`tool_end`] but stores the tool result body for expandable peek/copy.
pub fn tool_end_with(id: u64, ok: bool, result: &str) {
    with_run(id, |run| {
        if let Some(name) = run.tool.take() {
            // Prefer the open in-flight entry matching this tool name.
            if let Some(last) = run
                .trace
                .iter_mut()
                .rev()
                .find(|e| e.ok.is_none() && e.name == name)
            {
                last.ok = Some(ok);
                last.result = clip_body(result);
            } else if let Some(last) = run.trace.last_mut() {
                last.ok = Some(ok);
                if last.result.is_empty() {
                    last.result = clip_body(result);
                }
            }
        }
        if ok {
            run.tools_done += 1;
        } else {
            run.tools_failed += 1;
        }
        trim_trace(run);
        bump(run, if ok { 4 } else { 8 });
    });
}

/// The subagent produced output text (assistant deltas) — keeps the trace alive
/// while the model is writing its report rather than calling tools.
pub fn thinking(id: u64) {
    with_run(id, |run| {
        if run.tool.is_none() {
            run.activity = "writing report".into();
        }
        bump(run, 2);
    });
}

/// Close out a run.
pub fn finish(id: u64, state: RunState, tokens: u64) {
    let Ok(mut guard) = table().lock() else {
        return;
    };
    if let Some(run) = guard.iter_mut().find(|r| r.id == id) {
        run.state = state;
        run.ended = Some(Instant::now());
        run.tool = None;
        run.tokens = tokens;
        run.activity = match state {
            RunState::Done => "reported".into(),
            RunState::Failed => "failed".into(),
            RunState::Cancelled => "cancelled".into(),
            RunState::Running => run.activity.clone(),
        };
        bump(run, 8);
    }
    // A run only becomes evictable here, so this is where history is trimmed.
    prune(&mut guard);
}

/// Drop finished runs beyond [`HISTORY_CAP`], oldest first. Running entries are
/// never evicted.
fn prune(runs: &mut Vec<AgentRun>) {
    let finished = runs.iter().filter(|r| r.state.is_terminal()).count();
    if finished <= HISTORY_CAP {
        return;
    }
    let mut to_drop = finished - HISTORY_CAP;
    runs.retain(|r| {
        if to_drop > 0 && r.state.is_terminal() {
            to_drop -= 1;
            false
        } else {
            true
        }
    });
}

/// Current table, oldest first.
pub fn snapshot() -> Vec<AgentRun> {
    table().lock().map(|g| g.clone()).unwrap_or_default()
}

/// Cheap count of table entries without cloning the runs (PERF-D).
pub fn snapshot_len() -> usize {
    table().lock().map(|g| g.len()).unwrap_or(0)
}

/// Forget every finished run (running ones stay).
pub fn clear_finished() -> usize {
    let Ok(mut guard) = table().lock() else {
        return 0;
    };
    let before = guard.len();
    guard.retain(|r| !r.state.is_terminal());
    before - guard.len()
}

/// How many subagents are in flight right now.
pub fn running_count() -> usize {
    table()
        .lock()
        .map(|g| g.iter().filter(|r| r.state == RunState::Running).count())
        .unwrap_or(0)
}

/// Mark every still-running entry cancelled — used when a turn is interrupted,
/// so no pane is left spinning forever.
pub fn cancel_running() {
    if let Ok(mut guard) = table().lock() {
        for run in guard.iter_mut().filter(|r| r.state == RunState::Running) {
            run.state = RunState::Cancelled;
            run.ended = Some(Instant::now());
            run.tool = None;
            run.activity = "cancelled".into();
        }
    }
}

#[cfg(test)]
/// Reset the table between tests.
pub fn reset() {
    if let Ok(mut guard) = table().lock() {
        guard.clear();
    }
}

#[cfg(test)]
/// The registry is process-global: any test that seeds it must hold this lock,
/// including the renderer tests in `tui::ui`.
pub fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
    GUARD
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        test_lock()
    }

    #[test]
    fn a_run_tracks_tools_state_and_tokens() {
        let _g = lock();
        reset();
        let id = begin("explore", "map the auth path\nsecond line ignored");
        tool_start(id, "grep");
        tool_end(id, true);
        tool_start(id, "read_file");
        tool_end(id, false);
        finish(id, RunState::Done, 4200);

        let runs = snapshot();
        let run = runs.iter().find(|r| r.id == id).expect("run present");
        assert_eq!(run.kind, "explore");
        assert_eq!(run.task, "map the auth path", "task is clipped to one line");
        assert_eq!((run.tools_done, run.tools_failed), (1, 1));
        assert_eq!(run.tokens, 4200);
        assert_eq!(run.state, RunState::Done);
        assert!(run.tool.is_none(), "no tool may be left in flight");
        assert!(run.ended.is_some());
    }

    #[test]
    fn trace_entries_capture_args_and_result_for_expand_and_copy() {
        let _g = lock();
        reset();
        let id = begin("general", "do a thing");
        tool_start_with(id, "bash", "{\"command\":\"ls -la\"}");
        tool_end_with(id, true, "total 4\ndrwxr-xr-x  2 me me 4096 .");
        tool_start_with(id, "read_file", "{\"path\":\"x.rs\"}");
        // still running — no end yet

        let runs = snapshot();
        let run = runs.iter().find(|r| r.id == id).unwrap();
        assert_eq!(run.trace.len(), 2);

        let done = &run.trace[0];
        assert_eq!(done.name, "bash");
        assert_eq!(done.ok, Some(true));
        assert!(done.has_body(), "entry with args+result must be expandable");
        assert!(done.header_line().starts_with('✓'));
        let copy = done.copy_text();
        assert!(copy.contains("ls -la"), "args in copy payload");
        assert!(copy.contains("total 4"), "output in copy payload");
        let sections: Vec<_> = done.body_sections().iter().map(|(l, _)| *l).collect();
        assert_eq!(sections, vec!["args", "output"]);

        let running = &run.trace[1];
        assert_eq!(running.ok, None);
        assert!(running.has_body(), "in-flight entry still unfolds (shows args)");
        assert!(running.header_line().starts_with('⚒'));
        assert!(
            running.copy_text().contains("still running"),
            "in-flight copy notes running state"
        );
    }

    #[test]
    fn the_activity_trace_is_bounded() {
        let _g = lock();
        reset();
        let id = begin("general", "long runner");
        for _ in 0..(PULSE_CAP * 3) {
            tool_start(id, "bash");
            tool_end(id, true);
        }
        let runs = snapshot();
        let run = runs.iter().find(|r| r.id == id).unwrap();
        assert_eq!(run.pulse.len(), PULSE_CAP);
    }

    #[test]
    fn finished_runs_are_pruned_but_running_ones_survive() {
        let _g = lock();
        reset();
        let live = begin("general", "still going");
        for i in 0..(HISTORY_CAP + 10) {
            let id = begin("explore", &format!("task {i}"));
            finish(id, RunState::Done, 1);
        }
        let runs = snapshot();
        assert!(
            runs.iter()
                .any(|r| r.id == live && r.state == RunState::Running),
            "an in-flight run must never be evicted"
        );
        assert!(
            runs.iter().filter(|r| r.state.is_terminal()).count() <= HISTORY_CAP,
            "history is capped"
        );
    }

    #[test]
    fn clear_and_cancel_only_touch_their_own_rows() {
        let _g = lock();
        reset();
        let done = begin("explore", "a");
        finish(done, RunState::Done, 0);
        let live = begin("general", "b");

        assert_eq!(clear_finished(), 1);
        assert_eq!(running_count(), 1);

        cancel_running();
        assert_eq!(running_count(), 0);
        let runs = snapshot();
        let run = runs.iter().find(|r| r.id == live).unwrap();
        assert_eq!(run.state, RunState::Cancelled);
        assert!(
            run.ended.is_some(),
            "a cancelled run stops accumulating time"
        );
    }

    #[test]
    fn overlong_text_is_capped() {
        let _g = lock();
        reset();
        let id = begin("general", &"x".repeat(1000));
        let runs = snapshot();
        let run = runs.iter().find(|r| r.id == id).unwrap();
        assert_eq!(run.task.chars().count(), TEXT_CAP);
        assert!(run.task.ends_with('…'));
    }
}

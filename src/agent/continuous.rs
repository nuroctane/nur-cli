//! Continuous / sovereign mode helpers (ported from wizard's `--continuous`).
//!
//! `nur "<goal>" --continuous` runs headless turns in a loop on one session
//! until the model signals completion (a line reading `DONE`), a Ctrl+C
//! arrives, or `--max-iters` is hit. Each turn shares the session, so the agent
//! loop's own auto-compaction keeps context bounded across a long mission.
//!
//! Quality gates (Prime Agent autonomous mode): optional shell command from
//! `config.quality_gate` must pass before DONE is accepted.
//!
//! Only the pure decision helpers live here (unit-tested); the async driver
//! that pumps events lives in `main::run_continuous`.

use std::path::Path;
use std::process::Command;

/// Prompt for iteration `iter` (1-based) of a continuous run toward `goal`.
/// The first step states the goal and the completion protocol; later steps
/// lean on the retained session context and just ask for continued progress.
pub fn continuous_prompt(goal: &str, iter: u32) -> String {
    if iter <= 1 {
        format!(
            "You are running in continuous, self-directed mode. Goal:\n\n{goal}\n\n\
             Make concrete progress toward this goal now, using your tools. Prefer tool \
             `goal` action=set for durable tracking. When — and only when — the goal is \
             fully complete and verified (and any quality gate would pass), reply with a \
             line containing exactly DONE and call goal action=complete. Otherwise, do \
             the next useful step and stop; you will be prompted to continue."
        )
    } else {
        "Continue toward the goal. If it is now fully complete and verified, reply with \
         a line containing exactly DONE (and goal.complete). Otherwise make the next \
         concrete step of progress and stop."
            .to_string()
    }
}

/// Run configured quality gate. Ok(()) means pass or no gate configured.
/// Err message is fed back to the agent (Prime: failed gate returns output).
pub fn run_quality_gate(cwd: &Path, gate_cmd: &str) -> Result<(), String> {
    let cmd = gate_cmd.trim();
    if cmd.is_empty() {
        return Ok(());
    }
    #[cfg(windows)]
    let output = Command::new("cmd")
        .args(["/C", cmd])
        .current_dir(cwd)
        .output();
    #[cfg(not(windows))]
    let output = Command::new("sh")
        .args(["-c", cmd])
        .current_dir(cwd)
        .output();
    match output {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let stderr = String::from_utf8_lossy(&o.stderr);
            let code = o.status.code().unwrap_or(-1);
            Err(format!(
                "quality gate failed (exit {code}): `{cmd}`\nstdout:\n{stdout}\nstderr:\n{stderr}"
            ))
        }
        Err(e) => Err(format!("quality gate could not run `{cmd}`: {e}")),
    }
}

/// Accept DONE only when the quality gate passes (if configured).
pub fn accept_done(answer: &str, cwd: &Path, gate_cmd: &str) -> Result<bool, String> {
    if !is_done(answer) {
        return Ok(false);
    }
    run_quality_gate(cwd, gate_cmd)?;
    Ok(true)
}

/// True when the model's answer signals the mission is finished — a line equal
/// to `DONE` (case-insensitive), or the whole answer being just `done`.
pub fn is_done(answer: &str) -> bool {
    let t = answer.trim();
    if t.eq_ignore_ascii_case("done") {
        return true;
    }
    answer
        .lines()
        .any(|l| l.trim().eq_ignore_ascii_case("done"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_prompt_states_goal_and_protocol() {
        let p = continuous_prompt("ship the release", 1);
        assert!(p.contains("ship the release"));
        assert!(
            p.contains("DONE"),
            "must explain the DONE completion signal"
        );
    }

    #[test]
    fn later_prompts_ask_to_continue_without_restating_goal() {
        let p = continuous_prompt("ship the release", 5);
        assert!(
            !p.contains("ship the release"),
            "later steps rely on retained context"
        );
        assert!(p.contains("DONE"));
    }

    #[test]
    fn is_done_detects_the_sentinel() {
        assert!(is_done("DONE"));
        assert!(is_done("done"));
        assert!(is_done("All tasks finished.\nDONE"));
        assert!(is_done("work summary\n  DONE  \nnothing else"));
    }

    #[test]
    fn is_done_ignores_incidental_mentions() {
        assert!(!is_done("I am not done yet, still working."));
        assert!(!is_done("The DONE marker will be printed when finished."));
        assert!(!is_done("almost done with step 3"));
    }

    #[test]
    fn empty_quality_gate_always_passes() {
        assert!(run_quality_gate(Path::new("."), "").is_ok());
        assert_eq!(
            accept_done("DONE", Path::new("."), "").unwrap(),
            true
        );
        assert_eq!(
            accept_done("still working", Path::new("."), "").unwrap(),
            false
        );
    }
}

//! Continuous / sovereign mode helpers (ported from wizard's `--continuous`).
//!
//! `nur "<goal>" --continuous` runs headless turns in a loop on one session
//! until the model signals completion (a line reading `DONE`), a Ctrl+C
//! arrives, or `--max-iters` is hit. Each turn shares the session, so the agent
//! loop's own auto-compaction keeps context bounded across a long mission.
//!
//! Only the pure decision helpers live here (unit-tested); the async driver
//! that pumps events lives in `main::run_continuous`.

/// Prompt for iteration `iter` (1-based) of a continuous run toward `goal`.
/// The first step states the goal and the completion protocol; later steps
/// lean on the retained session context and just ask for continued progress.
pub fn continuous_prompt(goal: &str, iter: u32) -> String {
    if iter <= 1 {
        format!(
            "You are running in continuous, self-directed mode. Goal:\n\n{goal}\n\n\
             Make concrete progress toward this goal now, using your tools. When — and \
             only when — the goal is fully complete and verified, reply with a line \
             containing exactly DONE. Otherwise, do the next useful step and stop; you \
             will be prompted to continue."
        )
    } else {
        "Continue toward the goal. If it is now fully complete and verified, reply with \
         a line containing exactly DONE. Otherwise make the next concrete step of \
         progress and stop."
            .to_string()
    }
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
}

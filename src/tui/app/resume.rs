//! Continuing a session after a usage limit or an outage (`agent::resume`).
//!
//! A turn that stops on one gets a `Cell::Resume` card instead of a bare
//! error: when the limit lifts (plus a minute) the session continues by
//! itself, the same kind of turn it was (a `/goal` run stays a goal run).
//! The card's **auto-continue** chip and `/autocontinue` switch that for the
//! session; **continue now** goes at once. Prompts queued meanwhile wait and
//! go first. Sending a message yourself settles the card.
use super::*;
use crate::agent::resume::{self, Plan};

/// Card actions, in `hit_queue_actions` (queued cards use 0-3).
pub const ACTION_TOGGLE: u8 = 10;
pub const ACTION_NOW: u8 = 11;

impl App {
    /// The waiting card, if any (at most one waits at a time).
    fn waiting_resume(&mut self) -> Option<(usize, &mut Cell)> {
        self.cells
            .iter_mut()
            .enumerate()
            .rev()
            .find(|(_, c)| matches!(c, Cell::Resume { settled: None, .. }))
    }

    /// A turn of `kind` failed with `error`. Puts up a resume card and returns
    /// true when waiting can fix it; false leaves the error to the caller.
    pub(super) fn offer_resume(&mut self, error: &str, kind: TurnMode) -> bool {
        if matches!(kind, TurnMode::Compact) {
            return false;
        }
        self.resume_attempts += 1;
        // Planned even while switched off, so the card can say when the limit
        // lifts and the chip can switch it on.
        let cfg = resume::AutoContinueConfig {
            enabled: true,
            ..self.cfg.auto_continue.clone()
        };
        let Some(plan) = resume::plan(error, self.resume_attempts, &cfg, resume::now_secs()) else {
            if resume::classify(error).is_some() && self.resume_attempts > 1 {
                self.push_error(error.to_string());
                self.push_info(format!(
                    "stopped continuing after {} tries · send a message to try again",
                    self.resume_attempts - 1
                ));
                self.resume_attempts = 0;
                return true;
            }
            self.resume_attempts = 0;
            return false;
        };
        self.settle_resume("replaced by a newer stop");
        self.transcript_revision = self.transcript_revision.wrapping_add(1);
        self.cells.push(Cell::Resume {
            plan,
            armed: self.auto_continue,
            settled: None,
        });
        self.resume_kind = kind;
        true
    }

    /// Close the waiting card with `how` (no-op when none waits).
    pub(super) fn settle_resume(&mut self, how: &str) {
        let how = how.to_string();
        if let Some((_, Cell::Resume { settled, .. })) = self.waiting_resume() {
            *settled = Some(how);
            self.transcript_revision = self.transcript_revision.wrapping_add(1);
        }
    }

    /// Whether a waiting, switched-on card holds the queue back.
    pub(super) fn resume_holds_queue(&self) -> bool {
        self.cells.iter().any(|c| {
            matches!(
                c,
                Cell::Resume {
                    settled: None,
                    armed: true,
                    ..
                }
            )
        })
    }

    /// `/autocontinue [on|off|now]`: no argument toggles.
    pub(super) fn cmd_autocontinue(&mut self, arg: &str) {
        match arg.trim() {
            "now" => {
                if !self.resume_now() {
                    self.push_info("nothing is waiting to continue".into());
                }
                return;
            }
            "on" => self.auto_continue = true,
            "off" => self.auto_continue = false,
            "" => self.auto_continue = !self.auto_continue,
            _ => {
                self.push_error("usage: /autocontinue [on|off|now]".into());
                return;
            }
        }
        let on = self.auto_continue;
        if let Some((_, Cell::Resume { armed, .. })) = self.waiting_resume() {
            *armed = on;
        }
        self.transcript_revision = self.transcript_revision.wrapping_add(1);
        self.push_note(
            Tone::Mode,
            if on {
                "auto-continue on · stopped sessions pick back up when limits lift".into()
            } else {
                "auto-continue off for this session".into()
            },
        );
    }

    /// A click on a card chip.
    pub(super) fn on_resume_action(&mut self, cell_idx: usize, action: u8) {
        match action {
            ACTION_TOGGLE => {
                if let Some(Cell::Resume {
                    armed,
                    settled: None,
                    ..
                }) = self.cells.get_mut(cell_idx)
                {
                    *armed = !*armed;
                    self.auto_continue = *armed;
                    self.transcript_revision = self.transcript_revision.wrapping_add(1);
                }
            }
            ACTION_NOW => {
                self.resume_now();
            }
            _ => {}
        }
    }

    /// Continue the waiting session now. False when nothing waits or a turn runs.
    fn resume_now(&mut self) -> bool {
        if self.busy || self.waiting_resume().is_none() {
            return false;
        }
        self.fire_resume();
        true
    }

    /// Each frame: continue when a switched-on card's time has come. True
    /// when the transcript changed (a countdown tick, or the turn started).
    pub(super) fn poll_resume(&mut self) -> bool {
        let Some((_, Cell::Resume { plan, armed, .. })) = self.waiting_resume() else {
            return false;
        };
        let (at, armed) = (plan.at, *armed);
        let now = resume::now_secs();
        let ready = armed
            && now >= at
            && !self.busy
            && !self.startup_pending()
            && self.modal_focus().is_none();
        if ready {
            self.fire_resume();
            return true;
        }
        // Repaint the countdown once a second.
        if self.resume_tick.elapsed() >= Duration::from_secs(1) {
            self.resume_tick = Instant::now();
            return true;
        }
        false
    }

    fn fire_resume(&mut self) {
        let when = resume::clock(resume::now_secs());
        self.settle_resume(&format!("continued at {when}"));
        // A message queued while waiting is what the user wants next.
        if let Some(next) = self.queue.pop_front() {
            if let Some(index) = self
                .cells
                .iter()
                .position(|c| matches!(c, Cell::Queued { id, .. } if *id == next.id))
            {
                self.remove_cell(index);
            }
            self.submit_queued(next);
            return;
        }
        let kind = std::mem::replace(&mut self.resume_kind, TurnMode::Chat);
        self.start_turn_labeled("continue", resume::CONTINUE_PROMPT);
        // A goal run stays one, with the budget it had left.
        if matches!(kind, TurnMode::Goal { .. }) && self.busy {
            self.turn_kind = kind;
        }
    }
}

/// The card's first line: what stopped and when it goes on.
pub fn resume_headline(plan: &Plan, armed: bool, settled: Option<&str>) -> String {
    let now = resume::now_secs();
    let what = match plan.stop {
        resume::Stop::Limit => "usage limit reached",
        resume::Stop::Transient => "provider unavailable",
    };
    if let Some(how) = settled {
        return format!("{what} · {how}");
    }
    let wait = crate::api::limits::format_wait(plan.at.saturating_sub(now));
    let at = resume::clock(plan.at);
    match (armed, plan.known_reset) {
        (true, true) => format!("{what} · continuing at {at} (in {wait})"),
        (true, false) => format!("{what} · trying again at {at} (in {wait})"),
        (false, true) => format!("{what} · lifts at {at} (in {wait})"),
        (false, false) => match plan.stop {
            resume::Stop::Limit => format!("{what} · no reset time given"),
            resume::Stop::Transient => format!("{what} · not retrying"),
        },
    }
}

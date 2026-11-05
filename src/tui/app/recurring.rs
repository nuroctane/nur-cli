//! In-session recurring prompts. Missed intervals coalesce into one turn.
use super::App;
use crate::theme::Tone;
use std::time::{Duration, Instant};

pub(super) struct Recurring {
    prompt: String,
    session_id: String,
    interval: Duration,
    next: Instant,
}

fn interval(text: &str) -> Option<Duration> {
    let split = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let amount = text[..split].parse::<u64>().ok()?;
    let factor = match &text[split..] {
        "" | "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        _ => return None,
    };
    let seconds = amount.checked_mul(factor)?;
    (seconds > 0).then(|| Duration::from_secs(seconds))
}

impl App {
    pub(super) fn cmd_loop(&mut self, arg: &str) {
        let arg = arg.trim();
        if matches!(arg, "off" | "stop") {
            self.recurring = None;
            self.push_note(Tone::Neutral, "loop stopped".into());
            return;
        }
        if arg.is_empty() {
            let status = self
                .recurring
                .as_ref()
                .map(|job| format!("loop every {}s: {}", job.interval.as_secs(), job.prompt))
                .unwrap_or_else(|| "loop is off".into());
            self.push_note(Tone::Neutral, status);
            return;
        }
        let Some((delay, prompt)) = arg.split_once(char::is_whitespace) else {
            self.push_error("use /loop 10m prompt or /loop off".into());
            return;
        };
        let Some(interval) = interval(delay) else {
            self.push_error(
                "loop interval must be a positive number followed by s, m, h, or d".into(),
            );
            return;
        };
        let Some(next) = Instant::now().checked_add(interval) else {
            self.push_error("loop interval is too large".into());
            return;
        };
        if prompt.trim().is_empty() {
            self.push_error("loop needs a prompt".into());
            return;
        }
        self.recurring = Some(Recurring {
            prompt: prompt.trim().into(),
            session_id: self.session_id.clone(),
            interval,
            next,
        });
        self.push_note(
            Tone::Neutral,
            format!("loop set for every {}s", interval.as_secs()),
        );
    }

    pub(super) fn poll_recurring(&mut self) -> bool {
        let Some(job) = self.recurring.as_ref() else {
            return false;
        };
        if job.session_id != self.session_id {
            self.recurring = None;
            self.push_note(
                Tone::Neutral,
                "loop stopped after switching sessions".into(),
            );
            return true;
        }
        if Instant::now() < job.next
            || !self.may_start_unattended()
            || self.startup_pending()
            || !self.authed
            || self.session.is_none()
            || self.usage.is_none()
            || !self.queue.is_empty()
            || self.approval.is_some()
            || self.question.is_some()
            || self.secret.is_some()
        {
            return false;
        }
        let prompt = job.prompt.clone();
        let next = Instant::now().checked_add(job.interval);
        if let Some(next) = next {
            if let Some(job) = self.recurring.as_mut() {
                job.next = next;
            }
            self.start_turn_labeled(&format!("/loop {prompt}"), &prompt);
        } else {
            self.recurring = None;
            self.push_error("loop interval is too large".into());
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::interval;
    use std::time::Duration;

    #[test]
    fn intervals_reject_zero_invalid_units_and_overflow() {
        assert_eq!(interval("2m"), Some(Duration::from_secs(120)));
        assert_eq!(interval("0s"), None);
        assert_eq!(interval("5ms"), None);
        assert_eq!(interval("18446744073709551615d"), None);
    }
}

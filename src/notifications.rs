//! User-attention signals: end-of-turn chime, and the attention chime.
//!
//! A turn can run for minutes; without a signal the user either watches an
//! idle-looking terminal or misses the finish. Worse, a turn can stall
//! mid-run on a question, approval, secret, or login modal with no signal
//! at all, so an approval waits while the user has no idea. Both signals
//! use the same mechanism. The portable notification is the ASCII BEL
//! (0x07): every terminal understands it, and every multiplexer
//! (tmux, screen, zellij, byobu) forwards it to the outer terminal when its
//! bell monitoring is enabled - unlike OSC 9/777, which only some emulators
//! implement and which muxes often swallow.
//!
//! `[notifications] command` replaces the BEL with a real sound for users who
//! want one (`afplay ...`, `paplay ...`, `powershell -c [console]::beep()`),
//! launched detached so the chime never delays the next prompt.

use std::io::Write;

/// The portable notification byte.
pub const BELL: &[u8] = b"\x07";

/// `[notifications]` - end-of-turn and waiting-on-user signals.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct NotificationsConfig {
    /// `true` (default): signal the end of every turn, and every time the
    /// agent stops to wait on the user (question, approval, secret, login
    /// modal, or an interactive headless prompt). The BEL goes to the
    /// terminal only when stdout is a terminal; a redirected run stays silent.
    #[serde(default = "default_true")]
    pub chime: bool,
    /// Optional command run instead of the BEL, for a real sound. Executed
    /// through `sh -c` (Unix) / `cmd /C` (Windows), detached, output discarded.
    #[serde(default)]
    pub command: String,
}
impl Default for NotificationsConfig {
    fn default() -> Self {
        NotificationsConfig {
            chime: true,
            command: String::new(),
        }
    }
}
fn default_true() -> bool {
    true
}

/// Signal the end of a turn. `enabled` is `[notifications] chime`, `command`
/// the optional sound command. Never blocks, never fails a turn.
pub fn chime(enabled: bool, command: &str) {
    signal(enabled, command);
}

/// Signal that the agent is waiting on the user: a question, approval,
/// secret, or login modal just opened, or an interactive headless prompt
/// is about to be printed. Same sound, same switch, same guarantees as
/// `chime`, so one `[notifications] chime = false` silences both. Call it
/// once per pop-up, at the point the prompt becomes visible and actionable,
/// not when it is merely queued behind another modal.
pub fn attention(enabled: bool, command: &str) {
    signal(enabled, command);
}

fn signal(enabled: bool, command: &str) {
    if !enabled {
        return;
    }
    let command = command.trim();
    if !command.is_empty() {
        run_detached(command);
        return;
    }
    // A redirected run (CI, `nur run > out.txt`) must not receive a BEL byte
    // in its output. The TUI and headless TTY paths are terminals.
    if std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        let mut out = std::io::stdout();
        let _ = out.write_all(BELL);
        let _ = out.flush();
    }
}

/// Launch the configured sound command without waiting for it.
fn run_detached(command: &str) {
    #[cfg(windows)]
    let mut cmd = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", command]);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = std::process::Command::new("sh");
        c.args(["-c", command]);
        c
    };
    let _ = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

#[cfg(test)]
mod tests {
    #[test]
    fn attention_and_chime_never_fail() {
        // Under `cargo test` stdout is captured, not a terminal, so the BEL
        // branch is skipped; a bogus command is spawn-and-ignore. Both
        // paths must simply return, never panic or block a turn.
        super::attention(false, "");
        super::attention(true, "");
        super::attention(true, "definitely-not-a-real-command-xyz");
        super::chime(false, "");
        super::chime(true, "");
    }
}

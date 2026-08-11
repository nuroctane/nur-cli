//! Portable input/output guardrails (OpenAI Agents SDK pattern, multi-provider).
//!
//! Input guardrails run before a user turn is sent to the model.
//! Output guardrails scan assistant text before it is treated as final.
//! Tool-arg guardrails catch common footguns without provider lock-in.
//!
//! Design notes from OpenAI Agents SDK (portable):
//! - Tripwires can block or warn; nur defaults to **warn** for output, **block**
//!   for clearly dangerous tool args in plan mode (handled elsewhere).
//! - Keep checks cheap - no extra model calls in the default path.

use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardDecision {
    Allow,
    Warn(String),
    Block(String),
}

fn secret_line_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)(api[_-]?key|secret|password|passwd|token|authorization)\s*[:=]\s*\S{8,}")
            .expect("secret regex")
    })
}

fn sk_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b(sk-[A-Za-z0-9_\-]{20,}|sk-ant-[A-Za-z0-9_\-]{20,}|ghp_[A-Za-z0-9]{20,}|xox[baprs]-[A-Za-z0-9\-]{20,})\b").expect("sk regex"))
}

/// Scan user input before it hits the model.
pub fn check_input(text: &str) -> GuardDecision {
    let chars = text.chars().count();
    if chars > 500_000 {
        return GuardDecision::Block(
            "input exceeds 500k characters - register large corpora with tool `context` \
             (RLM prompt-as-variable) instead of pasting wholesale"
                .into(),
        );
    }
    // Pasting private keys into chat is a common accident.
    if text.contains("-----BEGIN") && text.contains("PRIVATE KEY-----") {
        return GuardDecision::Block(
            "input appears to contain a PEM private key - refuse to send to any provider".into(),
        );
    }
    if sk_re().is_match(text) && chars < 20_000 {
        return GuardDecision::Warn(
            "input may contain an API token (sk-/ghp-/xox). Prefer env vars and /login; \
             rotate if this was a real secret"
                .into(),
        );
    }
    GuardDecision::Allow
}

/// Scan final assistant text.
pub fn check_output(text: &str) -> GuardDecision {
    if sk_re().is_match(text) {
        return GuardDecision::Warn(
            "assistant output may echo a secret token - do not paste into logs/tickets; rotate if real"
                .into(),
        );
    }
    if secret_line_re().is_match(text) && text.chars().count() < 8_000 {
        return GuardDecision::Warn(
            "assistant output looks like it contains credential assignments".into(),
        );
    }
    GuardDecision::Allow
}

/// Cheap tool-arg checks (provider-agnostic).
pub fn check_tool_args(name: &str, args_json: &str) -> GuardDecision {
    // Absolute filesystem roots - nur also refuses in sandbox, but fail early.
    if matches!(
        name,
        "bash" | "write_file" | "edit_file" | "read_file" | "list_dir"
    ) {
        let lower = args_json.to_ascii_lowercase();
        if lower.contains("\"path\":\"/\"")
            || lower.contains("\"path\": \"/\"")
            || lower.contains("\"path\":\"c:\\\\\"")
            || lower.contains("\"path\": \"c:\\\\\"")
            || lower.contains("\"command\":\"rm -rf /\"")
            || lower.contains("rm -rf /")
        {
            return GuardDecision::Block(format!(
                "guardrail blocked {name}: looks like filesystem-root mutation/read"
            ));
        }
    }
    if name == "bash" {
        let lower = args_json.to_ascii_lowercase();
        for bad in [
            ":(){ :|:& };:",
            "mkfs.",
            "dd if=/dev/zero of=/dev/",
            "format c:",
            "> /dev/sda",
        ] {
            if lower.contains(bad) {
                return GuardDecision::Block(format!(
                    "guardrail blocked bash: destructive pattern `{bad}`"
                ));
            }
        }
    }
    GuardDecision::Allow
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_pem() {
        let t = "please use -----BEGIN PRIVATE KEY-----\nMIIE\n-----END PRIVATE KEY-----";
        assert!(matches!(check_input(t), GuardDecision::Block(_)));
    }

    #[test]
    fn warns_on_sk_in_output() {
        let t = format!("key is sk-{}", "a".repeat(30));
        assert!(matches!(check_output(&t), GuardDecision::Warn(_)));
    }

    #[test]
    fn blocks_rm_rf_root() {
        let args = r#"{"command":"rm -rf /"}"#;
        assert!(matches!(
            check_tool_args("bash", args),
            GuardDecision::Block(_)
        ));
    }

    #[test]
    fn allows_normal() {
        assert_eq!(check_input("fix the login flow"), GuardDecision::Allow);
        assert_eq!(
            check_tool_args("read_file", r#"{"path":"src/main.rs"}"#),
            GuardDecision::Allow
        );
    }
}

//! Tool capability classification — fail-closed by default.
//!
//! Parallel batches **skip the approval gate**, so concurrency-safe tools
//! must also be read-only. Anything that mutates (or can) is sequential and
//! approval-gated in manual mode.
//!
//! Defaults (when a tool is unknown or args are ambiguous):
//! - `is_read_only` → **false**
//! - `is_concurrency_safe` → **false**
//! - `is_destructive` → **false** (not all mutators are irreversible)

use serde_json::Value;

/// Snapshot of capability flags for a concrete tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolCaps {
    pub read_only: bool,
    pub concurrency_safe: bool,
    pub destructive: bool,
}

impl ToolCaps {
    /// Fail-closed defaults: not free, not parallel, not labeled destructive.
    #[allow(dead_code)]
    pub const FAIL_CLOSED: Self = Self {
        read_only: false,
        concurrency_safe: false,
        destructive: false,
    };
}

/// Classify a tool invocation by name + raw JSON args string.
pub fn classify(name: &str, args_json: &str) -> ToolCaps {
    let args: Value =
        serde_json::from_str(args_json).unwrap_or_else(|_| Value::Object(Default::default()));
    classify_value(name, &args)
}

pub fn classify_value(name: &str, args: &Value) -> ToolCaps {
    let read_only = is_read_only(name, args);
    // Invariant: concurrency-safe ⇒ read-only (parallel batch skips approval).
    let concurrency_safe = read_only && is_concurrency_safe_inner(name, args);
    let destructive = is_destructive(name, args);
    ToolCaps {
        read_only,
        concurrency_safe,
        destructive,
    }
}

/// Free / approval-skip in manual when true; allowed freely in plan mode when true
/// (plan still has extra shell/browser exceptions in the agent loop).
pub fn is_read_only(name: &str, args: &Value) -> bool {
    match name {
        "read_file" | "list_dir" | "grep" | "glob" | "web_fetch" | "web_search" | "look"
        | "git_status" | "git_diff" | "skill" | "todo_write" | "submit_plan" => true,
        "memory" => args
            .get("action")
            .and_then(|a| a.as_str())
            .map(|s| s == "read")
            .unwrap_or(false),
        // Action helpers take the raw JSON string form used across the codebase.
        "graphify" => crate::tools::graphify::is_read_only_action(&args.to_string()),
        "graphjin" => crate::tools::graphjin::is_read_only_action(&args.to_string()),
        "excalidraw" => crate::tools::excalidraw::is_read_only_action(&args.to_string()),
        "tldraw" => crate::tools::tldraw::is_read_only_action(&args.to_string()),
        "plur" => crate::tools::plur::is_read_only_action(&args.to_string()),
        "ruflo" => crate::tools::ruflo::is_read_only_action(&args.to_string()),
        "akarso" => crate::tools::akarso::is_read_only_action(&args.to_string()),
        "t3code" => crate::tools::t3code_tool::is_read_only_action(&args.to_string()),
        "penecho" => crate::tools::penecho_tool::is_read_only_action(&args.to_string()),
        "fractal" => crate::tools::fractal_tool::is_read_only_action(&args.to_string()),
        "executor" => crate::tools::executor_is_read_only(&args.to_string()),
        "omp" => crate::tools::omp::is_read_only_value(args),
        "browser" => crate::tools::browser_is_read_only(&args.to_string()),
        "agent" | "write_file" | "edit_file" | "multi_edit" | "apply_patch" | "bash"
        | "extract_frames" => false,
        _ => false, // fail-closed: unknown tools are not free
    }
}

/// May run in a concurrent batch with other concurrency-safe tools.
/// Must imply `is_read_only` (enforced in `classify`).
fn is_concurrency_safe_inner(name: &str, _args: &Value) -> bool {
    matches!(
        name,
        "read_file"
            | "list_dir"
            | "grep"
            | "glob"
            | "web_fetch"
            | "web_search"
            | "look"
            | "git_status"
            | "git_diff"
            | "skill"
            | "graphify"
            | "excalidraw"
            | "plur"
            | "ruflo"
    )
}

/// Irreversible or high-impact mutators (writes, agent, frames, shell, etc.).
/// Used for future permission rules and plan-mode messaging; not all mutators
/// are destructive (e.g. todo_write is session-local).
pub fn is_destructive(name: &str, args: &Value) -> bool {
    match name {
        "write_file" | "edit_file" | "multi_edit" | "apply_patch" | "bash" | "agent"
        | "extract_frames" => true,
        "memory" => !is_read_only("memory", args),
        "graphify" => !is_read_only("graphify", args),
        "graphjin" => !is_read_only("graphjin", args),
        "excalidraw" => !is_read_only("excalidraw", args),
        "tldraw" => !is_read_only("tldraw", args),
        "plur" => !is_read_only("plur", args),
        "ruflo" => !is_read_only("ruflo", args),
        "akarso" => !is_read_only("akarso", args),
        "t3code" => !is_read_only("t3code", args),
        "penecho" => !is_read_only("penecho", args),
        "fractal" => !is_read_only("fractal", args),
        "omp" => !is_read_only("omp", args),
        "browser" => !is_read_only("browser", args),
        "executor" => !is_read_only("executor", args),
        _ => false,
    }
}

// ── string-arg adapters used by the agent loop ─────────────────────────────

pub fn is_read_only_call(name: &str, args_json: &str) -> bool {
    classify(name, args_json).read_only
}

pub fn is_concurrency_safe(name: &str, args_json: &str) -> bool {
    classify(name, args_json).concurrency_safe
}

/// Alias kept for call-site clarity in the parallel batcher.
pub fn is_parallel_safe(name: &str, args_json: &str) -> bool {
    is_concurrency_safe(name, args_json)
}

#[allow(dead_code)] // completes the string-adapter trio; for upcoming permission rules
pub fn is_destructive_call(name: &str, args_json: &str) -> bool {
    classify(name, args_json).destructive
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fail_closed_unknown_tool() {
        let c = classify("not_a_real_tool", "{}");
        assert!(!c.read_only);
        assert!(!c.concurrency_safe);
        assert!(!c.destructive);
    }

    #[test]
    fn concurrency_implies_read_only() {
        for name in [
            "read_file",
            "list_dir",
            "grep",
            "glob",
            "web_fetch",
            "web_search",
            "look",
            "extract_frames",
            "git_status",
            "git_diff",
            "skill",
            "write_file",
            "edit_file",
            "multi_edit",
            "apply_patch",
            "bash",
            "agent",
            "memory",
            "todo_write",
            "submit_plan",
            "browser",
            "omp",
            "graphify",
            "graphjin",
            "plur",
            "ruflo",
        ] {
            let c = classify(name, "{}");
            if c.concurrency_safe {
                assert!(c.read_only, "{name} concurrency without read_only");
            }
        }
    }

    #[test]
    fn writers_are_destructive_and_not_parallel() {
        for name in [
            "write_file",
            "edit_file",
            "multi_edit",
            "apply_patch",
            "bash",
            "agent",
        ] {
            let c = classify(name, "{}");
            assert!(c.destructive, "{name}");
            assert!(!c.concurrency_safe, "{name}");
            assert!(!c.read_only, "{name}");
        }
    }
}

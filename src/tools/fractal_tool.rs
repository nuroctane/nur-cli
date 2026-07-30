use super::{arg_str, Tool, ToolContext};
use crate::error::Result;
use serde_json::Value;

/// fractal tool — hierarchical recursive loops in git worktrees
/// Fail-closed: unknown or malformed args => not read-only (requires approval).
pub fn is_read_only_action(args_json: &str) -> bool {
    if let Ok(v) = serde_json::from_str::<Value>(args_json) {
        if let Some(action) = v.get("action").and_then(|a| a.as_str()) {
            let act = action.trim().to_ascii_lowercase();
            return matches!(
                act.as_str(),
                "status"
                    | "probe"
                    | "doctor"
                    | "node list"
                    | "node_list"
                    | "list"
                    | "node status"
                    | "node_status"
                    | "status node"
                    | "node activity"
                    | "activity"
                    | "node pending"
                    | "pending"
                    | "cost"
                    | "cost remaining"
                    | "cost breakdown"
                    | "cost spent"
                    | "open"
                    | "node attach"
                    | "attach"
            );
        }
    }
    false
}

pub struct Fractal;

impl Tool for Fractal {
    fn name(&self) -> &str {
        "fractal"
    }

    fn description(&self) -> &str {
        "fractal — hierarchical recursive loops in git worktrees (https://github.com/plasma-ai/fractal). Each node is an isolated worktree with its own loop. Spawn children for separable subtasks to get multiplicative parallelism. Actions: status|probe|doctor|init|node list|node status|node start|node attach|node merge|node activity|pause|resume|open. Tool owns worktree lifecycle; integrate with /fractal slash and fractal skill."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "description": "status|probe|doctor|init|node list|node status|node start|node attach|node merge|node activity|pause|resume|open|track|commit|destroy", "default": "status"},
                "node": {"type": "string", "description": "node name for node-scoped actions"},
                "args": {"type": "string", "description": "extra args forwarded to fractal CLI (space-separated)"},
                "workdir": {"type": "string", "description": "override cwd for fractal invocation"}
            },
            "required": []
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let action_raw = arg_str(args, "action").unwrap_or_else(|_| "status".into());
        let action = action_raw.trim().to_ascii_lowercase();
        let node = arg_str(args, "node").unwrap_or_default();
        let extra = arg_str(args, "args").unwrap_or_default();
        let workdir_s = arg_str(args, "workdir").unwrap_or_default();

        // `workdir` becomes `Command::current_dir` and the root of `probe_at`'s
        // parent walk. Every other tool that takes a path goes through the
        // workspace sandbox; this one took absolutes verbatim and `join`ed
        // relatives without collapsing `..` — and most of its actions classify
        // as read-only, so the escape ran with no approval prompt.
        let cwd = if workdir_s.trim().is_empty() {
            ctx.cwd.clone()
        } else {
            match crate::tools::resolve_path(&ctx.cwd, workdir_s.trim()) {
                Ok(p) => p,
                Err(e) => return Ok(format!("workdir rejected: {e}")),
            }
        };

        let extra_parts: Vec<String> = extra.split_whitespace().map(|s| s.to_string()).collect();

        // `args` is a free-text argv passthrough, and fractal documents `--path`
        // as an escape hatch for running outside the current worktree. Most
        // actions classify as read-only and so run with no approval prompt, which
        // would let that hatch reach any directory on the machine — reopening the
        // hole the `workdir` sandbox check closes. Any path-bearing argument has
        // to survive the same workspace check.
        for p in path_bearing_args(&extra_parts) {
            if p.trim().is_empty() {
                return Ok("`--path` needs a value".into());
            }
            if let Err(e) = crate::tools::resolve_path(&ctx.cwd, p.trim()) {
                return Ok(format!(
                    "args rejected: `{p}` resolves outside the workspace ({e})"
                ));
            }
        }

        // Validate node name when provided (prevent traversal)
        if !node.trim().is_empty() && !crate::fractal::is_valid_fractal_node_name(node.trim()) {
            return Ok(format!(
                "invalid node name `{}` — must match ^[A-Za-z0-9_]+$ max 64 chars, no traversal",
                node.trim()
            ));
        }

        // NOTE: `probe_at` executes the CLI (1-2 process spawns) — and on a host
        // where fractal cannot start, those are guaranteed-failing spawns, i.e.
        // pure latency. It used to run unconditionally here, including for the
        // branches that never read it; it now lives only inside the branches
        // that do (`probe`, and `doctor`/`status` via `doctor_at`).
        //
        // Every branch that shells out passes `cancel` so Esc kills the child
        // instead of leaving the turn wedged on a streaming subcommand.
        let cancel = &ctx.cancel;

        match action.as_str() {
            "probe" => {
                let probe = crate::fractal::probe_at(&cwd);
                Ok(format!(
                    "fractal probe:\n binary={:?} exists={} usable={}\n version={:?}\n unusable_reason={:?}\n repo_root={:?} is_git={} fractal_dir={:?} exists={} is_fractal_repo={} worktrees={}\n",
                    probe.binary,
                    probe.binary.is_some(),
                    probe.usable,
                    probe.version,
                    probe.unusable_reason,
                    probe.repo_root,
                    probe.is_git_repo,
                    probe.fractal_dir,
                    probe.fractal_dir_exists,
                    probe.is_fractal_repo,
                    probe.worktrees_exist
                ))
            }
            "doctor" => {
                let doc = crate::fractal::doctor_at(&cwd);
                let mut out = format!(
                    "fractal doctor:\n binary_present={} binary_usable={} version={:?}\n git_repo={} fractal_repo={} fractal_dir={:?} worktrees={} python={}\n",
                    doc.binary_present,
                    doc.binary_usable,
                    doc.version,
                    doc.git_repo,
                    doc.fractal_repo,
                    doc.fractal_dir,
                    doc.worktrees_present,
                    doc.python_present
                );
                if let Some(reason) = doc.unusable_reason.as_deref() {
                    out.push_str(&format!(" unusable: {reason}\n"));
                }
                out.push_str(" Install: pipx install plasma-fractal (Python 3.12-3.14) — https://github.com/plasma-ai/fractal\n");
                Ok(out)
            }
            "status" => {
                let doc = crate::fractal::doctor_at(&cwd);
                let mut out = String::new();
                out.push_str("fractal — recursive agent loops in git worktrees (https://github.com/plasma-ai/fractal)\n");
                out.push_str("Each node = isolated worktree + autonomous loop. Parent spawns children for separable subtasks.\n\n");
                out.push_str(&format!(
                    "Probe: binary={} usable={} version={:?} git={} fractal_repo={} worktrees={}\n",
                    doc.binary_present,
                    doc.binary_usable,
                    doc.version,
                    doc.git_repo,
                    doc.fractal_repo,
                    doc.worktrees_present
                ));
                if !doc.binary_present {
                    out.push_str("\n✗ fractal not found on PATH. Install via `pipx install plasma-fractal`.\n");
                } else if !doc.binary_usable {
                    // Found on PATH but it crashes on startup — reporting
                    // "binary=found" alone sent every later action into a raw
                    // Python traceback. Say so once, here.
                    out.push_str(&format!(
                        "\n✗ {}\n",
                        doc.unusable_reason.as_deref().unwrap_or(
                            "fractal is installed but did not respond to --version or --help"
                        )
                    ));
                } else if !doc.git_repo {
                    out.push_str("\n✗ not a git repo — `fractal init` requires git.\n");
                } else if !doc.fractal_repo {
                    out.push_str(
                        "\n○ not yet a fractal repo. Run action=init or `fractal init`.\n",
                    );
                } else {
                    match crate::fractal::list_nodes_cancellable(&cwd, cancel) {
                        Ok(list) => {
                            out.push_str("\nNodes (`fractal node list`):\n");
                            out.push_str(&list);
                            out.push('\n');
                        }
                        Err(e) => out.push_str(&format!("\nnode list failed: {e}\n")),
                    }
                }
                out.push_str("\nActions: init, node list, node status <name>, node start <name>, node attach <name>, open <name>\n");
                Ok(out)
            }
            "init" => {
                // Repo-level root init (`fractal init <path> --agent=…`). Run
                // caps are NOT set here — they belong to `fractal node init
                // <name>`, which this tool does not expose; the skill drives that.
                let mut cli_args = vec!["init".to_string()];
                cli_args.extend(extra_parts);
                match crate::fractal::run_fractal_args_cancellable(&cwd, &cli_args, cancel) {
                    Ok(s) => Ok(format!("fractal init:\n{s}")),
                    Err(e) => Ok(format!("fractal init failed: {e}")),
                }
            }
            "node list" | "node_list" | "list" => {
                let mut cli_args = vec!["node".to_string(), "list".to_string()];
                cli_args.extend(extra_parts);
                match crate::fractal::run_fractal_args_cancellable(&cwd, &cli_args, cancel) {
                    Ok(s) => Ok(s),
                    Err(e) => Ok(format!("node list failed: {e}")),
                }
            }
            "node status" | "node_status" | "status node" => {
                if node.trim().is_empty() {
                    let args = vec!["node".to_string(), "list".to_string()];
                    match crate::fractal::run_fractal_args_cancellable(&cwd, &args, cancel) {
                        Ok(s) => Ok(s),
                        Err(e) => Ok(format!("node list (fallback for status) failed: {e}")),
                    }
                } else {
                    let mut cli_args = vec!["node".to_string(), "status".to_string(), node.clone()];
                    cli_args.extend(extra_parts);
                    match crate::fractal::run_fractal_args_cancellable(&cwd, &cli_args, cancel) {
                        Ok(s) => Ok(s),
                        Err(e) => Ok(format!("node status {node} failed: {e}")),
                    }
                }
            }
            "node start" | "node_start" | "start" => {
                if node.trim().is_empty() {
                    return Ok("node start requires `node` param — e.g. {\"action\":\"node start\",\"node\":\"my_child\"}".into());
                }
                // `node` was validated above; pass the same trimmed form we
                // checked rather than the raw string.
                let node_arg = node.trim().to_string();
                let mut cli_args = vec!["node".to_string(), "start".to_string(), node_arg];
                // Do NOT inject caps here. `fractal node start` takes only
                // `--continue`, `--clean`, and `--max-cost` (the last valid only
                // *with* `--continue`); every ceiling comes from `config.json`,
                // set at `node init`. Passing `--max-depth`/`--max-children` to
                // a Click CLI is a hard parse error, so injecting them would fail
                // every start — and auto-supplying `--max-cost` would silently
                // re-arm a budget the CLI stops on deliberately. See
                // `skills/fractal/SKILL.md` "Start".
                cli_args.extend(extra_parts);
                let warn = "WARNING: fractal nodes run without permission prompts by default \
                    (bypassPermissions / --always-approve / --yolo). A node can run any \
                    command its agent decides. Only start nodes whose task you would trust \
                    unsupervised.\n\n";
                match crate::fractal::run_fractal_args_cancellable(&cwd, &cli_args, cancel) {
                    Ok(s) => Ok(format!("{warn}{s}")),
                    Err(e) => Ok(format!("{warn}node start {node} failed: {e}")),
                }
            }
            "node attach" | "attach" => {
                if node.trim().is_empty() {
                    return Ok("node attach requires `node` param".into());
                }
                let path = crate::fractal::node_path(&cwd, &node);
                Ok(format!(
                    "node attach {node}:\n worktree path: {:?}\n To attach: `fractal node attach {node}` (opens tmux).\n",
                    path
                ))
            }
            "node activity" | "activity" => {
                let mut cli_args = vec!["node".to_string(), "activity".to_string()];
                if !node.trim().is_empty() {
                    cli_args.push(node.clone());
                }
                cli_args.extend(extra_parts);
                match crate::fractal::run_fractal_args_cancellable(&cwd, &cli_args, cancel) {
                    Ok(s) => Ok(s),
                    Err(e) => Ok(format!("node activity failed: {e}")),
                }
            }
            "node pending" | "pending" => {
                let mut cli_args = vec!["node".to_string(), "pending".to_string()];
                if !node.trim().is_empty() {
                    cli_args.push(node.clone());
                }
                cli_args.extend(extra_parts);
                match crate::fractal::run_fractal_args_cancellable(&cwd, &cli_args, cancel) {
                    Ok(s) => Ok(s),
                    Err(e) => Ok(format!("node pending failed: {e}")),
                }
            }
            "pause" => {
                // Global pause vs per-node: upstream uses `fractal pause` for global, `fractal node pause <name>` for node.
                let cli_args = if node.trim().is_empty() {
                    let mut a = vec!["pause".to_string()];
                    a.extend(extra_parts);
                    a
                } else {
                    let mut a = vec!["node".to_string(), "pause".to_string(), node.clone()];
                    a.extend(extra_parts);
                    a
                };
                match crate::fractal::run_fractal_args_cancellable(&cwd, &cli_args, cancel) {
                    Ok(s) => Ok(s),
                    Err(e) => Ok(format!("pause failed: {e}")),
                }
            }
            "resume" => {
                let cli_args = if node.trim().is_empty() {
                    let mut a = vec!["resume".to_string()];
                    a.extend(extra_parts);
                    a
                } else {
                    let mut a = vec!["node".to_string(), "resume".to_string(), node.clone()];
                    a.extend(extra_parts);
                    a
                };
                match crate::fractal::run_fractal_args_cancellable(&cwd, &cli_args, cancel) {
                    Ok(s) => Ok(s),
                    Err(e) => Ok(format!("resume failed: {e}")),
                }
            }
            "open" => {
                if node.trim().is_empty() {
                    // Path-only answer: `probe_at` would spawn the CLI just to
                    // hand back the git root it walked the filesystem to find.
                    let root = crate::fractal::repo_root_of(&cwd).unwrap_or(cwd);
                    Ok(format!(
                        "worktrees folder: {}",
                        root.join(crate::fractal::WORKTREES_FOLDER).display()
                    ))
                } else {
                    let path = crate::fractal::node_path(&cwd, &node);
                    Ok(format!("node {node} path: {:?}", path))
                }
            }
            _ => {
                let mut cli_args: Vec<String> = action_raw
                    .split_whitespace()
                    .map(|s| s.to_string())
                    .collect();
                if cli_args.is_empty() {
                    cli_args.push(action_raw.clone());
                }
                if !node.trim().is_empty() && !cli_args.iter().any(|a| a == &node) {
                    cli_args.push(node.clone());
                }
                cli_args.extend(extra_parts);
                match crate::fractal::run_fractal_args_cancellable(&cwd, &cli_args, cancel) {
                    Ok(s) => Ok(format!("fractal {}:\n{s}", cli_args.join(" "))),
                    Err(e) => Ok(format!(
                        "fractal {} failed: {e}\nTry `fractal --help`",
                        cli_args.join(" ")
                    )),
                }
            }
        }
    }
}

/// Every argument in a free-text `args` passthrough that names a filesystem path.
///
/// Covers `--path=VALUE`, `--path VALUE`, and any bare absolute path. Callers
/// must run each result through the workspace sandbox: fractal treats `--path`
/// as a documented escape hatch for operating outside the current worktree, and
/// most fractal actions are classified read-only (no approval prompt).
fn path_bearing_args(parts: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for (i, a) in parts.iter().enumerate() {
        if let Some(v) = a.strip_prefix("--path=") {
            out.push(v.to_string());
        } else if a == "--path" {
            // `--path <value>`: an absent value is reported as empty so the
            // caller rejects it rather than silently passing the flag through.
            out.push(parts.get(i + 1).cloned().unwrap_or_default());
        } else if std::path::Path::new(a).is_absolute() {
            out.push(a.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(s: &str) -> Vec<String> {
        s.split_whitespace().map(|x| x.to_string()).collect()
    }

    #[test]
    fn detects_every_path_bearing_form() {
        assert_eq!(
            path_bearing_args(&parts(r"--path=D:\other\repo")),
            vec![r"D:\other\repo".to_string()]
        );
        assert_eq!(
            path_bearing_args(&parts("--path /etc/passwd")),
            vec!["/etc/passwd".to_string()]
        );
        // A bare absolute path with no flag still counts.
        assert_eq!(
            path_bearing_args(&parts(r"C:\Windows\System32")),
            vec![r"C:\Windows\System32".to_string()]
        );
        // Missing value surfaces as empty so the caller can reject it.
        assert_eq!(path_bearing_args(&parts("--path")), vec![String::new()]);
    }

    #[test]
    fn ordinary_flags_are_not_treated_as_paths() {
        for s in [
            "--continue",
            "--clean",
            "--max-cost=5.00",
            "--verbose --json",
            "",
            "node_name",
            "--pathological",
        ] {
            assert!(
                path_bearing_args(&parts(s)).is_empty(),
                "{s:?} should not be path-bearing"
            );
        }
    }

    /// `node start` must carry no injected caps: the CLI accepts only
    /// `--continue`, `--clean`, and `--max-cost` (the last with `--continue`),
    /// so injecting `--max-depth`/`--max-children` made every start fail.
    #[test]
    fn read_only_classification_is_fail_closed() {
        assert!(!is_read_only_action("not json at all"));
        assert!(!is_read_only_action("{}"));
        assert!(!is_read_only_action(r#"{"action":123}"#));
        assert!(!is_read_only_action(r#"{"action":"node start"}"#));
        assert!(is_read_only_action(r#"{"action":"node list"}"#));
    }

    fn ctx() -> ToolContext {
        ToolContext {
            cwd: std::env::current_dir().unwrap(),
            cancel: tokio_util::sync::CancellationToken::new(),
        }
    }

    fn run(action: &str, c: &ToolContext) -> String {
        Fractal
            .execute(&serde_json::json!({ "action": action }), c)
            .unwrap_or_else(|e| e.to_string())
    }

    /// End-to-end guarantees that hold whatever the host's fractal install does
    /// — missing, healthy, or (on every Windows box) crashing during import:
    ///   * no action ever shows the user a raw Python stack trace;
    ///   * `probe` reports usability, not just presence on PATH;
    ///   * a cancelled turn returns at once instead of waiting out the timeout.
    ///
    /// Kept as one test because each action spawns real processes, and running
    /// them concurrently only piles load onto timing-sensitive sibling tests.
    #[test]
    fn tool_is_traceback_free_and_cancellable() {
        let c = ctx();
        let mut outputs = Vec::new();
        for action in ["probe", "status", "node list", "open"] {
            outputs.push((action, run(action, &c)));
        }
        for (action, out) in &outputs {
            assert!(
                !out.contains("Traceback (most recent call last)"),
                "action {action} leaked a traceback:\n{out}"
            );
            assert!(
                !out.contains("site-packages"),
                "action {action} leaked interpreter frames:\n{out}"
            );
        }

        let probe_out = &outputs[0].1;
        assert!(probe_out.contains("usable="), "{probe_out}");
        assert!(probe_out.contains("unusable_reason="), "{probe_out}");

        // Esc must not have to wait out FRACTAL_TIMEOUT_MS.
        let cancelled = ctx();
        cancelled.cancel.cancel();
        let started = std::time::Instant::now();
        let out = run("node list", &cancelled);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "cancelled action took {:?}: {out}",
            started.elapsed()
        );
    }
}

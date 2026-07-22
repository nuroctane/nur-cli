use super::{arg_str, Tool, ToolContext};
use crate::error::Result;
use serde_json::Value;
use std::path::PathBuf;

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

        let cwd = if workdir_s.trim().is_empty() {
            ctx.cwd.clone()
        } else {
            let p = PathBuf::from(workdir_s.trim());
            if p.is_absolute() {
                p
            } else {
                ctx.cwd.join(p)
            }
        };

        let extra_parts: Vec<String> = extra
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        // Validate node name when provided (prevent traversal)
        if !node.trim().is_empty() && !crate::fractal::is_valid_fractal_node_name(node.trim()) {
            return Ok(format!(
                "invalid node name `{}` — must match ^[A-Za-z0-9_]+$ max 64 chars, no traversal",
                node.trim()
            ));
        }

        let probe = crate::fractal::probe_at(&cwd);

        match action.as_str() {
            "probe" => Ok(format!(
                "fractal probe:\n binary={:?} exists={}\n version={:?}\n repo_root={:?} is_git={} fractal_dir={:?} exists={} is_fractal_repo={} worktrees={}\n",
                probe.binary,
                probe.binary.is_some(),
                probe.version,
                probe.repo_root,
                probe.is_git_repo,
                probe.fractal_dir,
                probe.fractal_dir_exists,
                probe.is_fractal_repo,
                probe.worktrees_exist
            )),
            "doctor" => {
                let doc = crate::fractal::doctor_at(&cwd);
                Ok(format!(
                    "fractal doctor:\n binary_present={} version={:?}\n git_repo={} fractal_repo={} fractal_dir={:?} worktrees={} python={}\n Install: pipx install plasma-fractal (Python 3.10+) — https://github.com/plasma-ai/fractal\n",
                    doc.binary_present,
                    doc.version,
                    doc.git_repo,
                    doc.fractal_repo,
                    doc.fractal_dir,
                    doc.worktrees_present,
                    doc.python_present
                ))
            }
            "status" => {
                let doc = crate::fractal::doctor_at(&cwd);
                let mut out = String::new();
                out.push_str("fractal — recursive agent loops in git worktrees (https://github.com/plasma-ai/fractal)\n");
                out.push_str("Each node = isolated worktree + autonomous loop. Parent spawns children for separable subtasks.\n\n");
                out.push_str(&format!(
                    "Probe: binary={} version={:?} git={} fractal_repo={} worktrees={}\n",
                    doc.binary_present, doc.version, doc.git_repo, doc.fractal_repo, doc.worktrees_present
                ));
                if !doc.binary_present {
                    out.push_str("\n✗ fractal not found on PATH. Install via `pipx install plasma-fractal`.\n");
                } else if !doc.git_repo {
                    out.push_str("\n✗ not a git repo — `fractal init` requires git.\n");
                } else if !doc.fractal_repo {
                    out.push_str("\n○ not yet a fractal repo. Run action=init or `fractal init`.\n");
                } else {
                    match crate::fractal::list_nodes(&cwd) {
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
                let mut cli_args = vec!["init".to_string()];
                cli_args.extend(extra_parts);
                match crate::fractal::run_fractal_args(&cwd, &cli_args) {
                    Ok(s) => Ok(format!("fractal init:\n{s}")),
                    Err(e) => Ok(format!("fractal init failed: {e}")),
                }
            }
            "node list" | "node_list" | "list" => {
                let mut cli_args = vec!["node".to_string(), "list".to_string()];
                cli_args.extend(extra_parts);
                match crate::fractal::run_fractal_args(&cwd, &cli_args) {
                    Ok(s) => Ok(s),
                    Err(e) => Ok(format!("node list failed: {e}")),
                }
            }
            "node status" | "node_status" | "status node" => {
                if node.trim().is_empty() {
                    let args = vec!["node".to_string(), "list".to_string()];
                    match crate::fractal::run_fractal_args(&cwd, &args) {
                        Ok(s) => Ok(s),
                        Err(e) => Ok(format!("node list (fallback for status) failed: {e}")),
                    }
                } else {
                    let mut cli_args = vec!["node".to_string(), "status".to_string(), node.clone()];
                    cli_args.extend(extra_parts);
                    match crate::fractal::run_fractal_args(&cwd, &cli_args) {
                        Ok(s) => Ok(s),
                        Err(e) => Ok(format!("node status {node} failed: {e}")),
                    }
                }
            }
            "node start" | "node_start" | "start" => {
                if node.trim().is_empty() {
                    return Ok("node start requires `node` param — e.g. {\"action\":\"node start\",\"node\":\"my_child\"}".into());
                }
                let mut cli_args = vec!["node".to_string(), "start".to_string(), node.clone()];
                cli_args.extend(extra_parts);
                match crate::fractal::run_fractal_args(&cwd, &cli_args) {
                    Ok(s) => Ok(s),
                    Err(e) => Ok(format!("node start {node} failed: {e}")),
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
                match crate::fractal::run_fractal_args(&cwd, &cli_args) {
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
                match crate::fractal::run_fractal_args(&cwd, &cli_args) {
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
                match crate::fractal::run_fractal_args(&cwd, &cli_args) {
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
                match crate::fractal::run_fractal_args(&cwd, &cli_args) {
                    Ok(s) => Ok(s),
                    Err(e) => Ok(format!("resume failed: {e}")),
                }
            }
            "open" => {
                if node.trim().is_empty() {
                    let root = probe.repo_root.unwrap_or(cwd);
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
                match crate::fractal::run_fractal_args(&cwd, &cli_args) {
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

//! Graphify knowledge-graph tool — wraps the `graphify` CLI (PyPI: graphifyy).
//!
//! Read actions (query / path / explain / status / report) are approval-free.
//! `extract` and `update` write `graphify-out/` and need approval in manual mode.

use super::{arg_str, arg_u64, Tool, ToolContext};
use crate::error::{MuseError, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Graphify;

/// Actions that only read `graphify-out/` (or report CLI status).
pub fn is_read_only_action(args: &str) -> bool {
    let action = serde_json::from_str::<Value>(args)
        .ok()
        .and_then(|v| {
            v.get("action")
                .and_then(|a| a.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "status".into());
    matches!(
        action.as_str(),
        "status" | "query" | "path" | "explain" | "report" | "affected"
    )
}

impl Tool for Graphify {
    fn name(&self) -> &str {
        "graphify"
    }

    fn description(&self) -> &str {
        "Query or build a Graphify knowledge graph for the workspace (code AST → \
         graphify-out/). Prefer over broad grep for architecture questions when a \
         graph exists. \
         action=status (default): CLI + graph present?; \
         action=query: BFS over graph.json; \
         action=path: shortest path between two concepts; \
         action=explain: node + neighbors; \
         action=affected: reverse impact of a concept; \
         action=report: excerpt GRAPH_REPORT.md; \
         action=extract: build/rebuild graph (code-only AST, local, no API key); \
         action=update: re-extract changed code files. \
         Requires `graphify` on PATH (uv tool install graphifyy)."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["status", "query", "path", "explain", "affected", "report", "extract", "update"],
                    "default": "status"
                },
                "question": {
                    "type": "string",
                    "description": "For action=query"
                },
                "from": {
                    "type": "string",
                    "description": "Source concept for action=path"
                },
                "to": {
                    "type": "string",
                    "description": "Target concept for action=path"
                },
                "node": {
                    "type": "string",
                    "description": "Concept name for action=explain or action=affected"
                },
                "path": {
                    "type": "string",
                    "description": "Corpus path for extract/update (default: .)"
                },
                "budget": {
                    "type": "integer",
                    "description": "Token budget for query (default 2000)"
                },
                "force": {
                    "type": "boolean",
                    "description": "For extract/update: overwrite even if fewer nodes"
                },
                "code_only": {
                    "type": "boolean",
                    "description": "For extract: index only code via local AST (default true — no API key). Set false to include docs/PDFs (needs LLM backend)."
                },
                "dfs": {
                    "type": "boolean",
                    "description": "For query: use DFS instead of BFS"
                }
            }
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action").unwrap_or_else(|_| "status".into());
        match action.as_str() {
            "status" => status(&ctx.cwd),
            "query" => {
                let q = arg_str(args, "question")?;
                let budget = arg_u64(args, "budget");
                let dfs = args
                    .get("dfs")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let mut cli_args = vec!["query".to_string(), q];
                if dfs {
                    cli_args.push("--dfs".into());
                }
                if let Some(b) = budget {
                    cli_args.push("--budget".into());
                    cli_args.push(b.to_string());
                }
                run_graphify(&ctx.cwd, &cli_args, 120_000)
            }
            "path" => {
                let from = arg_str(args, "from")?;
                let to = arg_str(args, "to")?;
                run_graphify(&ctx.cwd, &["path".into(), from, to], 60_000)
            }
            "explain" => {
                let node = arg_str(args, "node")?;
                run_graphify(&ctx.cwd, &["explain".into(), node], 60_000)
            }
            "affected" => {
                let node = arg_str(args, "node")?;
                run_graphify(&ctx.cwd, &["affected".into(), node], 60_000)
            }
            "report" => read_report(&ctx.cwd),
            "extract" => {
                let path = arg_str(args, "path").unwrap_or_else(|_| ".".into());
                let force = args
                    .get("force")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                // Default code-only: local tree-sitter AST, no API key. Docs/PDF
                // semantic pass needs a backend — opt in with code_only=false.
                let code_only = args
                    .get("code_only")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let mut cli_args = vec!["extract".to_string(), path];
                if code_only {
                    cli_args.push("--code-only".into());
                }
                if force {
                    cli_args.push("--force".into());
                }
                // Code AST extract can take a while on large trees.
                run_graphify(&ctx.cwd, &cli_args, 600_000)
            }
            "update" => {
                let path = arg_str(args, "path").unwrap_or_else(|_| ".".into());
                let force = args
                    .get("force")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let mut cli_args = vec!["update".to_string(), path];
                if force {
                    cli_args.push("--force".into());
                }
                run_graphify(&ctx.cwd, &cli_args, 600_000)
            }
            other => Err(MuseError::Tool(format!(
                "unknown graphify action '{other}' — use status|query|path|explain|affected|report|extract|update"
            ))),
        }
    }
}

fn graph_json(cwd: &Path) -> PathBuf {
    cwd.join("graphify-out").join("graph.json")
}

fn status(cwd: &Path) -> Result<String> {
    let bin = find_graphify_bin();
    let version = match &bin {
        Some(b) => {
            let out = Command::new(b)
                .arg("--version")
                .output()
                .map_err(|e| MuseError::Tool(format!("graphify --version failed: {e}")))?;
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }
        None => String::new(),
    };

    let gj = graph_json(cwd);
    let report = cwd.join("graphify-out").join("GRAPH_REPORT.md");
    let html = cwd.join("graphify-out").join("graph.html");

    let mut s = String::new();
    match bin {
        Some(b) => {
            s.push_str(&format!("graphify CLI: {b}\n"));
            if !version.is_empty() {
                s.push_str(&format!("version: {version}\n"));
            }
        }
        None => {
            s.push_str(
                "graphify CLI: NOT FOUND\n\
                 install:  uv tool install graphifyy\n\
                 then:     graphify install --platform agents\n\
                 (PyPI package is graphifyy; command is graphify)\n",
            );
        }
    }

    s.push_str(&format!("workspace: {}\n", cwd.display()));
    if gj.is_file() {
        let size = std::fs::metadata(&gj).map(|m| m.len()).unwrap_or(0);
        s.push_str(&format!("graph: {} ({} bytes)\n", gj.display(), size));
        // Quick node/edge count if JSON is reasonable size.
        if size < 50_000_000 {
            if let Ok(text) = std::fs::read_to_string(&gj) {
                if let Ok(v) = serde_json::from_str::<Value>(&text) {
                    let nodes = v
                        .get("nodes")
                        .and_then(|n| n.as_array())
                        .map(|a| a.len())
                        .or_else(|| {
                            // networkx node-link sometimes uses different shape
                            v.get("graph")
                                .and_then(|_| Some(v.get("nodes")?.as_array()?.len()))
                        });
                    let edges = v
                        .get("edges")
                        .and_then(|e| e.as_array())
                        .map(|a| a.len())
                        .or_else(|| v.get("links").and_then(|e| e.as_array()).map(|a| a.len()));
                    if let (Some(n), Some(e)) = (nodes, edges) {
                        s.push_str(&format!("nodes: {n}  edges: {e}\n"));
                    }
                }
            }
        }
        s.push_str("hint: use graphify(action=query, question=\"…\") for architecture questions\n");
    } else {
        s.push_str(
            "graph: missing (no graphify-out/graph.json)\n\
             build: graphify(action=extract, path=\".\")  # code AST, local, free\n\
             or:    skill(action=read, name=graphify) then follow /graphify pipeline\n",
        );
    }
    if report.is_file() {
        s.push_str(&format!("report: {}\n", report.display()));
    }
    if html.is_file() {
        s.push_str(&format!("html: {}\n", html.display()));
    }
    Ok(s)
}

fn read_report(cwd: &Path) -> Result<String> {
    let report = cwd.join("graphify-out").join("GRAPH_REPORT.md");
    if !report.is_file() {
        return Err(MuseError::Tool(
            "no graphify-out/GRAPH_REPORT.md — run graphify(action=extract) first".into(),
        ));
    }
    let text = std::fs::read_to_string(&report)
        .map_err(|e| MuseError::Tool(format!("read report: {e}")))?;
    // Cap so we don't blow the context window.
    let capped: String = text.chars().take(12_000).collect();
    if text.chars().count() > 12_000 {
        Ok(format!(
            "{capped}\n\n… (truncated; full report at {})",
            report.display()
        ))
    } else {
        Ok(capped)
    }
}

fn run_graphify(cwd: &Path, args: &[String], timeout_ms: u64) -> Result<String> {
    let bin = find_graphify_bin().ok_or_else(|| {
        MuseError::Tool(
            "graphify CLI not found on PATH. Install with:\n  \
             uv tool install graphifyy\n  \
             graphify install --platform agents\n\
             Package name is graphifyy (double-y); the command is `graphify`."
                .into(),
        )
    })?;

    // Prefer absolute graph path when present so queries work even if cwd is odd.
    let mut final_args: Vec<String> = args.to_vec();
    let gj = graph_json(cwd);
    let needs_graph = matches!(
        args.first().map(|s| s.as_str()),
        Some("query" | "path" | "explain" | "affected")
    );
    if needs_graph && gj.is_file() && !final_args.iter().any(|a| a == "--graph") {
        final_args.push("--graph".into());
        final_args.push(gj.to_string_lossy().into_owned());
    }

    // Proper timeout-enforced process spawn — previously ignored timeout_ms
    let mut cmd = Command::new(&bin);
    cmd.args(&final_args)
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| MuseError::Tool(format!("failed to spawn graphify: {e}")))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let out_h = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut s) = stdout {
            use std::io::Read;
            let _ = s.read_to_end(&mut buf);
        }
        buf
    });
    let err_h = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut s) = stderr {
            use std::io::Read;
            let _ = s.read_to_end(&mut buf);
        }
        buf
    });

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    #[cfg(windows)]
                    {
                        let _ = Command::new("taskkill")
                            .args(["/PID", &child.id().to_string(), "/T", "/F"])
                            .output();
                    }
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(MuseError::Tool(format!(
                        "graphify timed out after {}ms (killed) — try narrowing path or run: graphify extract . --code-only",
                        timeout_ms
                    )));
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => return Err(MuseError::Tool(format!("graphify wait failed: {e}"))),
        }
    };

    let stdout_bytes = out_h.join().unwrap_or_default();
    let stderr_bytes = err_h.join().unwrap_or_default();
    let stdout = String::from_utf8_lossy(&stdout_bytes);
    let stderr = String::from_utf8_lossy(&stderr_bytes);
    let mut out = String::new();
    if !stdout.trim().is_empty() {
        out.push_str(stdout.trim());
    }
    if !stderr.trim().is_empty() {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str("--- stderr ---\n");
        out.push_str(stderr.trim());
    }
    if !status.success() {
        if out.is_empty() {
            out = format!("graphify exited with {}", status);
        }
        return Err(MuseError::Tool(out));
    }
    if out.is_empty() {
        out = "(graphify produced no output)".into();
    }
    if out.chars().count() > 40_000 {
        out = out.chars().take(40_000).collect::<String>() + "\n… (truncated)";
    }
    Ok(out)
}

/// Locate the `graphify` executable.
fn find_graphify_bin() -> Option<String> {
    // 1. PATH
    if which("graphify") {
        return Some("graphify".into());
    }
    // 2. Common uv / pipx / user bin locations
    let home = dirs::home_dir()?;
    let candidates = [
        home.join(".local").join("bin").join("graphify.exe"),
        home.join(".local").join("bin").join("graphify"),
        home.join(".cargo").join("bin").join("graphify.exe"), // unlikely
        home.join("AppData")
            .join("Local")
            .join("uv")
            .join("bin")
            .join("graphify.exe"),
    ];
    for c in candidates {
        if c.is_file() {
            return Some(c.to_string_lossy().into_owned());
        }
    }
    // 3. uv tool dir
    if let Ok(out) = Command::new("uv").args(["tool", "dir", "--bin"]).output() {
        if out.status.success() {
            let dir = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !dir.is_empty() {
                let p = PathBuf::from(&dir).join(if cfg!(windows) {
                    "graphify.exe"
                } else {
                    "graphify"
                });
                if p.is_file() {
                    return Some(p.to_string_lossy().into_owned());
                }
            }
        }
    }
    None
}

fn which(name: &str) -> bool {
    Command::new(if cfg!(windows) { "where" } else { "which" })
        .arg(name)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_actions() {
        assert!(is_read_only_action(r#"{"action":"query","question":"x"}"#));
        assert!(is_read_only_action(
            r#"{"action":"path","from":"a","to":"b"}"#
        ));
        assert!(is_read_only_action(r#"{"action":"explain","node":"x"}"#));
        assert!(is_read_only_action(r#"{"action":"status"}"#));
        assert!(is_read_only_action(r#"{"action":"report"}"#));
        assert!(is_read_only_action(r#"{"action":"affected","node":"x"}"#));
        assert!(!is_read_only_action(r#"{"action":"extract"}"#));
        assert!(!is_read_only_action(r#"{"action":"update"}"#));
        assert!(is_read_only_action("{}"), "default status is free");
    }
}

//! `repl` tool - Prime Intellect RLM persistent Python REPL surface.
//!
//! Faithful to Prime Agent's `ipython`: one built-in programming surface with a
//! persistent kernel whose state survives turns and compaction. Supports `%%bash`
//! cells (temporary subshell) while Python state and `%cd` persist.

use super::{arg_str, Tool, ToolContext};
use crate::agent::repl;
use crate::error::{NurError, Result};
use serde_json::Value;
use std::path::PathBuf;

pub struct ReplTool;

fn session_name(_ctx: &ToolContext) -> String {
    std::env::var("NUR_SESSION_ID")
        .map(|sid| {
            if sid.is_empty() {
                "default".to_string()
            } else {
                format!("session-{}", &sid[..sid.len().min(12)])
            }
        })
        .unwrap_or_else(|_| "default".to_string())
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

impl Tool for ReplTool {
    fn name(&self) -> &str {
        "repl"
    }

    fn description(&self) -> &str {
        "Persistent Python REPL (Prime Intellect RLM ipython pattern): a long-lived \
         interpreter whose variables/imports/functions SURVIVE across turns and compaction. \
         Actions: exec (run Python and keep state) | expr (eval, returns repr) | bash \
         (run a shell command in a temp subshell, Python state persists) | cd (change \
         kernel cwd) | status | list | kill. Use for programmatic context — parse, \
         transform, compute, delegate. Example: repl exec code='files=[p for p in \
         __import__(\"pathlib\").Path(\".\").rglob(\"*.md\")]' then repl expr \
         code='len(files)'. Never store secrets."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["exec", "expr", "bash", "cd", "status", "list", "kill"],
                    "description": "exec=run python (state persists); expr=eval returns repr; \
                    bash=temp subshell; cd=change kernel cwd; status/list/kill for lifecycle"
                },
                "code": {"type": "string", "description": "Python code (for exec/expr) or shell command (for bash)"},
                "cwd": {"type": "string", "description": "Optional cwd for this cell (defaults to session cwd)"}
            },
            "required": ["action"]
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action")?;
        let name = session_name(ctx);
        let cwd: PathBuf = args
            .get("cwd")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| ctx.cwd.clone());

        match action.as_str() {
            "exec" => {
                let code = arg_str(args, "code")?;
                let out = repl::repl_exec(&name, &cwd, &code, false)?;
                if out.is_empty() {
                    Ok(format!("[repl] exec ok — no output\nstate preserved"))
                } else {
                    Ok(format!("[repl]\n{out}"))
                }
            }
            "expr" => {
                let code = arg_str(args, "code")?;
                let out = repl::repl_exec(&name, &cwd, &code, true)?;
                Ok(format!("[repl] {out}"))
            }
            "bash" => {
                let cmd = arg_str(args, "code")?;
                // Run a temp subshell; Python state persists in the interpreter.
                let out = run_bash_shell(&cwd, &cmd)?;
                if out.trim().is_empty() {
                    Ok("[repl %%bash] ok — no output; python state preserved".into())
                } else {
                    Ok(format!("[repl %%bash]\n{out}"))
                }
            }
            "cd" => {
                let dir = arg_str(args, "code")?;
                let target: PathBuf = if PathBuf::from(&dir).is_absolute() {
                    PathBuf::from(&dir)
                } else {
                    cwd.join(&dir)
                };
                // Persist cd by running os.chdir in the kernel.
                let py = format!("import os; os.chdir({:?})", target.to_string_lossy());
                let _ = repl::repl_exec(&name, &ctx.cwd, &py, false)?;
                Ok(format!("[repl] cwd now {}", target.display()))
            }
            "status" => Ok(repl::repl_status(&name)),
            "list" => Ok(repl::repl_list()),
            "kill" => {
                repl::kill_repl(&name);
                Ok(format!("repl `{name}` killed (state cleared)"))
            }
            other => Err(NurError::Tool(format!(
                "unknown repl action `{other}`; use exec|expr|bash|cd|status|list|kill"
            ))),
        }
    }
}

fn run_bash_shell(cwd: &std::path::Path, cmd: &str) -> Result<String> {
    #[cfg(windows)]
    let (prog, args) = ("cmd.exe", vec!["/C".to_string(), cmd.to_string()]);
    #[cfg(not(windows))]
    let (prog, args) = ("sh", vec!["-c".to_string(), cmd.to_string()]);
    let out = std::process::Command::new(prog)
        .args(&args)
        .current_dir(cwd)
        .output()
        .map_err(|e| NurError::Tool(format!("repl %%bash failed: {e}")))?;
    let mut s = String::new();
    if !out.stdout.is_empty() {
        s.push_str(&String::from_utf8_lossy(&out.stdout));
    }
    if !out.stderr.is_empty() {
        s.push_str(&String::from_utf8_lossy(&out.stderr));
    }
    let code = out.status.code().unwrap_or(-1);
    if code != 0 {
        Ok(format!("(exit {code})\n{s}"))
    } else {
        Ok(s)
    }
}

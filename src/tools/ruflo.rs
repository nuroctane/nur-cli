//! Ruflo — agent meta-harness (vector memory, swarm, hive-mind).
//! Wraps the `ruflo` CLI; memory defaults to Meta's global DB at ~/.meta/ruflo/.

use super::{arg_str, arg_u64, Tool, ToolContext};
use crate::ecosystem;
use crate::error::{MuseError, Result};
use serde_json::Value;
use std::path::PathBuf;

pub struct Ruflo;

pub fn is_read_only_action(args: &str) -> bool {
    let action = serde_json::from_str::<Value>(args)
        .ok()
        .and_then(|v| v.get("action")?.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "status".into());
    matches!(
        action.as_str(),
        "status"
            | "memory_search"
            | "memory_stats"
            | "memory_list"
            | "agent_list"
            | "swarm_status"
            | "hive_status"
            | "doctor"
    )
}

impl Tool for Ruflo {
    fn name(&self) -> &str {
        "ruflo"
    }

    fn description(&self) -> &str {
        "Ruflo agent harness: vector memory (AgentDB), swarm coordination, hive-mind status. \
         Memory is global under ~/.meta/ruflo/ (does not pollute project trees). \
         action=status|memory_store|memory_search|memory_stats|memory_list|agent_list|\
         swarm_init|swarm_status|hive_status|doctor. Auto-installed with meta."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": [
                        "status",
                        "memory_store",
                        "memory_search",
                        "memory_stats",
                        "memory_list",
                        "agent_list",
                        "swarm_init",
                        "swarm_status",
                        "hive_status",
                        "doctor"
                    ],
                    "default": "status"
                },
                "key": {
                    "type": "string",
                    "description": "For memory_store"
                },
                "value": {
                    "type": "string",
                    "description": "For memory_store"
                },
                "query": {
                    "type": "string",
                    "description": "For memory_search"
                },
                "namespace": {
                    "type": "string",
                    "description": "Memory namespace (default: default)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Search result limit"
                },
                "project": {
                    "type": "boolean",
                    "description": "If true, use workspace-local .swarm memory instead of global Meta store",
                    "default": false
                }
            }
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let bin = ecosystem::find_bin("ruflo").ok_or_else(|| {
            MuseError::Tool(
                "ruflo CLI not found. Meta normally auto-installs it — run: npm install -g ruflo"
                    .into(),
            )
        })?;

        let action = arg_str(args, "action").unwrap_or_else(|_| "status".into());
        let project_local = args
            .get("project")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let (cwd, db_path): (PathBuf, Option<PathBuf>) = if project_local {
            (ctx.cwd.clone(), None)
        } else {
            let home = ecosystem::ruflo_home();
            let _ = std::fs::create_dir_all(&home);
            (home, Some(ecosystem::ruflo_db_path()))
        };

        let mut argv: Vec<String> = Vec::new();
        match action.as_str() {
            "status" => {
                argv.push("status".into());
            }
            "memory_store" => {
                let key = arg_str(args, "key")?;
                let value = arg_str(args, "value")?;
                argv.extend([
                    "memory".into(),
                    "store".into(),
                    "-k".into(),
                    key,
                    "--value".into(),
                    value,
                ]);
                if let Ok(ns) = arg_str(args, "namespace") {
                    argv.push("-n".into());
                    argv.push(ns);
                }
                if let Some(db) = &db_path {
                    argv.push("--path".into());
                    argv.push(db.to_string_lossy().into_owned());
                }
            }
            "memory_search" => {
                let q = arg_str(args, "query")?;
                argv.extend(["memory".into(), "search".into(), "-q".into(), q]);
                if let Some(lim) = arg_u64(args, "limit") {
                    argv.push("-l".into());
                    argv.push(lim.to_string());
                }
                if let Ok(ns) = arg_str(args, "namespace") {
                    argv.push("-n".into());
                    argv.push(ns);
                }
                if let Some(db) = &db_path {
                    argv.push("--path".into());
                    argv.push(db.to_string_lossy().into_owned());
                }
            }
            "memory_stats" => {
                argv.extend(["memory".into(), "stats".into()]);
                if let Some(db) = &db_path {
                    argv.push("--path".into());
                    argv.push(db.to_string_lossy().into_owned());
                }
            }
            "memory_list" => {
                argv.extend(["memory".into(), "list".into()]);
                if let Some(db) = &db_path {
                    argv.push("--path".into());
                    argv.push(db.to_string_lossy().into_owned());
                }
            }
            "agent_list" => {
                argv.extend(["agent".into(), "list".into()]);
            }
            "swarm_init" => {
                argv.extend(["swarm".into(), "init".into()]);
            }
            "swarm_status" => {
                argv.extend(["swarm".into(), "status".into()]);
            }
            "hive_status" => {
                argv.extend(["hive-mind".into(), "status".into()]);
            }
            "doctor" => {
                argv.push("doctor".into());
            }
            other => {
                return Err(MuseError::Tool(format!(
                    "unknown ruflo action '{other}' — status|memory_store|memory_search|memory_stats|memory_list|agent_list|swarm_init|swarm_status|hive_status|doctor"
                )));
            }
        }

        let refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
        let out = ecosystem::run_capture(&bin, &refs, Some(&cwd), 180_000)
            .map_err(MuseError::Tool)?;
        // Cap verbose ruflo tables.
        if out.chars().count() > 30_000 {
            Ok(out.chars().take(30_000).collect::<String>() + "\n… (truncated)")
        } else {
            Ok(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_gate() {
        assert!(is_read_only_action(r#"{"action":"status"}"#));
        assert!(is_read_only_action(r#"{"action":"memory_search","query":"x"}"#));
        assert!(!is_read_only_action(
            r#"{"action":"memory_store","key":"k","value":"v"}"#
        ));
        assert!(!is_read_only_action(r#"{"action":"swarm_init"}"#));
    }
}

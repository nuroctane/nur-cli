use super::{arg_str, Tool, ToolContext};
use crate::error::Result;
use serde_json::Value;

/// Dogwood - AWS "Dogwood" runtime verification for AI agents.
///
/// A policy language extending Cedar with temporal logic
/// (since/formerly/once/count_within) over an agent's event stream, used to
/// govern tool calls. Dogwood ships a reference Rust interpreter plus a
/// `dogwood` CLI binary used for evaluation/guardrails.
///
/// NOTE: The reference interpreter is explicitly NOT production-grade
/// enforcement. Treat it as an on-demand evaluation/guardrail layer, not a
/// trust anchor, and not as a runtime gate on every tool call.
pub struct Dogwood;

pub fn is_read_only_action(args: &str) -> bool {
    let action = serde_json::from_str::<Value>(args)
        .ok()
        .and_then(|v| v.get("action")?.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "status".into());
    matches!(
        action.as_str(),
        "status" | "which" | "validate" | "replay" | "check-parse" | "lower" | "schema"
    )
}

/// Location/version of the `dogwood` CLI, if present.
fn dogwood_bin() -> Option<String> {
    crate::ecosystem::find_bin("dogwood").or_else(|| crate::ecosystem::find_bin("dogwood.exe"))
}

const INSTALL_HINT: &str =
    "Install the Dogwood CLI with:\n  cargo install --git https://github.com/dogwood-policy/dogwood amzn-dogwood-cli";

fn status_report() -> String {
    match dogwood_bin() {
        Some(bin) => {
            let version = crate::ecosystem::cmd_version_pub(&bin, &["--version"])
                .unwrap_or_else(|| "unknown".into());
            format!(
                "dogwood CLI: ready\n  path:    {bin}\n  version: {version}\n\
                 \nUse dogwood(action=validate|replay|check-parse|lower|schema) to run policies.\n\
                 NOTE: reference interpreter - evaluation/guardrail only, not a runtime trust anchor."
            )
        }
        None => format!("dogwood CLI: not installed\n\n{INSTALL_HINT}\n"),
    }
}

fn run_dogwood(args: &[&str], cwd: Option<&std::path::Path>) -> Result<String> {
    let Some(bin) = dogwood_bin() else {
        return Err(crate::error::NurError::Tool(format!(
            "dogwood CLI not found. {INSTALL_HINT}"
        )));
    };
    crate::ecosystem::run_capture(&bin, args, cwd, 120_000).map_err(crate::error::NurError::Tool)
}

impl Tool for Dogwood {
    fn name(&self) -> &str {
        "dogwood"
    }

    fn description(&self) -> &str {
        "Dogwood runtime verification for AI agents (https://github.com/dogwood-policy/dogwood): \
         a Cedar-extended policy language with temporal logic (since/formerly/once/count_within) \
         over an agent's event stream for governing tool calls. \
         actions: status|which|check-parse|validate|replay|lower|schema. \
         On-demand guardrail/evaluation layer (on-demand CLI), NOT a runtime enforcement trust anchor. \
         Requires the `dogwood` CLI (`cargo install --git https://github.com/dogwood-policy/dogwood amzn-dogwood-cli`)."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["status", "which", "check-parse", "validate", "replay", "lower", "schema"],
                    "default": "status"
                },
                "policy": {
                    "type": "string",
                    "description": "Path to the .dw policy file (required for validate/replay/check-parse)"
                },
                "schema": {
                    "type": "string",
                    "description": "Path to the .cedarschema policy schema (optional)"
                },
                "event_schema": {
                    "type": "string",
                    "description": "Path to the .dwschema event schema (optional)"
                },
                "providers": {
                    "type": "string",
                    "description": "Path to providers.json (optional)"
                },
                "macros": {
                    "type": "string",
                    "description": "Path to macros.dw (optional)"
                },
                "trace": {
                    "type": "string",
                    "description": "Path to the trace log for replay (required for replay)"
                },
                "format": {
                    "type": "string",
                    "enum": ["human", "json"],
                    "default": "human"
                },
                "emit": {
                    "type": "string",
                    "enum": ["cedar-policies", "cedar-schema", "cedar-json", "both"],
                    "description": "Lowered output kind (lower; defaults to both)"
                },
                "schema_kind": {
                    "type": "string",
                    "enum": ["action", "event", "providers", "mcp"],
                    "description": "Schema operation (schema)"
                },
                "input": {
                    "type": "string",
                    "description": "Schema input path for action/event/providers"
                },
                "manifest": {
                    "type": "string",
                    "description": "MCP tools manifest path for schema_kind=mcp"
                }
            }
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action").unwrap_or_else(|_| "status".into());
        let format = arg_str(args, "format").unwrap_or_else(|_| "human".into());
        let format = if format == "json" { "json" } else { "human" };

        match action.as_str() {
            "status" | "which" => Ok(status_report()),
            "check-parse" => {
                let policy = arg_str(args, "policy").map_err(|_| {
                    crate::error::NurError::Tool("check-parse requires policy=".into())
                })?;
                let cmd: Vec<String> = vec![
                    "check-parse".into(),
                    "--format".into(),
                    format.into(),
                    policy,
                ];
                let cargs: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
                run_dogwood(&cargs, Some(&ctx.cwd))
            }
            "validate" => {
                let policy = arg_str(args, "policy").map_err(|_| {
                    crate::error::NurError::Tool("validate requires policy=".into())
                })?;
                let mut cmd: Vec<String> = vec!["validate".into(), policy.clone()];
                cmd.push("--format".into());
                cmd.push(format.into());
                if let Ok(schema) = arg_str(args, "schema") {
                    cmd.push("--policy-schema".into());
                    cmd.push(schema);
                }
                if let Ok(event_schema) = arg_str(args, "event_schema") {
                    cmd.push("--event-schema".into());
                    cmd.push(event_schema);
                }
                if let Ok(providers) = arg_str(args, "providers") {
                    cmd.push("--providers".into());
                    cmd.push(providers);
                }
                if let Ok(macros) = arg_str(args, "macros") {
                    cmd.push("--macros".into());
                    cmd.push(macros);
                }
                let cargs: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
                run_dogwood(&cargs, Some(&ctx.cwd))
            }
            "replay" => {
                let policy = arg_str(args, "policy")
                    .map_err(|_| crate::error::NurError::Tool("replay requires policy=".into()))?;
                let trace = arg_str(args, "trace")
                    .map_err(|_| crate::error::NurError::Tool("replay requires trace=".into()))?;
                let mut cmd: Vec<String> = vec!["replay".into(), policy];
                cmd.push("--format".into());
                cmd.push(format.into());
                if let Ok(schema) = arg_str(args, "schema") {
                    cmd.push("--policy-schema".into());
                    cmd.push(schema);
                }
                cmd.push("--trace".into());
                cmd.push(trace);
                let cargs: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
                run_dogwood(&cargs, Some(&ctx.cwd))
            }
            "lower" => {
                let policy = arg_str(args, "policy")
                    .map_err(|_| crate::error::NurError::Tool("lower requires policy=".into()))?;
                let schema = arg_str(args, "schema").map_err(|_| {
                    crate::error::NurError::Tool("lower requires schema= (Cedar action schema)".into())
                })?;
                let emit = arg_str(args, "emit").unwrap_or_else(|_| "both".into());
                let emit = match emit.as_str() {
                    "cedar-policies" | "cedar-schema" | "cedar-json" | "both" => emit,
                    _ => "both".into(),
                };
                let mut cmd = vec![
                    "lower".to_string(),
                    policy,
                    "--policy-schema".into(),
                    schema,
                    "--emit".into(),
                    emit,
                    "--format".into(),
                    format.into(),
                ];
                if let Ok(event_schema) = arg_str(args, "event_schema") {
                    cmd.push("--event-schema".into());
                    cmd.push(event_schema);
                }
                if let Ok(providers) = arg_str(args, "providers") {
                    cmd.push("--providers".into());
                    cmd.push(providers);
                }
                if let Ok(macros) = arg_str(args, "macros") {
                    cmd.push("--macros".into());
                    cmd.push(macros);
                }
                let cargs: Vec<&str> = cmd.iter().map(String::as_str).collect();
                run_dogwood(&cargs, Some(&ctx.cwd))
            }
            "schema" => {
                let kind = arg_str(args, "schema_kind").map_err(|_| {
                    crate::error::NurError::Tool(
                        "schema requires schema_kind=action|event|providers|mcp".into(),
                    )
                })?;
                let mut cmd = vec!["schema".to_string(), kind.clone()];
                match kind.as_str() {
                    "action" | "event" | "providers" => {
                        cmd.push(arg_str(args, "input").map_err(|_| {
                            crate::error::NurError::Tool(format!(
                                "schema_kind={kind} requires input="
                            ))
                        })?);
                    }
                    "mcp" => {
                        cmd.push("--manifest".into());
                        cmd.push(arg_str(args, "manifest").map_err(|_| {
                            crate::error::NurError::Tool(
                                "schema_kind=mcp requires manifest=".into(),
                            )
                        })?);
                    }
                    _ => {
                        return Err(crate::error::NurError::Tool(format!(
                            "unknown schema_kind `{kind}`; use action|event|providers|mcp"
                        )));
                    }
                }
                cmd.push("--format".into());
                cmd.push(format.into());
                let cargs: Vec<&str> = cmd.iter().map(String::as_str).collect();
                run_dogwood(&cargs, Some(&ctx.cwd))
            }
            other => Ok(format!(
                "unknown dogwood action '{other}' - status|which|check-parse|validate|replay|lower|schema"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_read_only_action;

    #[test]
    fn evaluation_actions_are_read_only() {
        assert!(is_read_only_action(r#"{"action":"status"}"#));
        assert!(is_read_only_action(r#"{"action":"validate"}"#));
        assert!(is_read_only_action(r#"{"action":"replay"}"#));
        assert!(is_read_only_action(r#"{"action":"check-parse"}"#));
        assert!(is_read_only_action(r#"{"action":"lower"}"#));
        assert!(is_read_only_action(r#"{"action":"schema"}"#));
    }
}

//! Akarso — post, schedule, and reply across 14 social platforms from the
//! terminal. Wraps the `akarso` CLI (npm `akarso`), mirroring the first-class
//! ecosystem tool pattern (plur/ruflo): the model drives it through a typed
//! action schema, we shell to the CLI with `--json`.
//!
//! Safety: publishing/deleting posts and connecting accounts are outward-facing,
//! hard-to-reverse actions — they are **not** read-only, so manual mode gates
//! them behind approval. Read actions (auth/account/post inspection) are free.

use super::{arg_str, Tool, ToolContext};
use crate::ecosystem;
use crate::error::{MuseError, Result};
use serde_json::Value;

pub struct Akarso;

/// Read-only Akarso actions (inspection only — never publish/connect/delete).
pub fn is_read_only_action(args: &str) -> bool {
    let action = serde_json::from_str::<Value>(args)
        .ok()
        .and_then(|v| v.get("action")?.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "status".into());
    matches!(
        action.as_str(),
        "status"
            | "auth_check"
            | "accounts_list"
            | "accounts_health"
            | "accounts_get"
            | "posts_list"
            | "posts_get"
            | "profiles_list"
    )
}

impl Tool for Akarso {
    fn name(&self) -> &str {
        "akarso"
    }

    fn description(&self) -> &str {
        "Akarso — post/schedule/reply across 14 social platforms (X, LinkedIn, \
         Instagram, Facebook, TikTok, YouTube, Threads, Reddit, Bluesky, Mastodon, \
         Discord, Slack, Pinterest, Google Business) from the CLI. \
         action=auth_check|accounts_list|accounts_health|accounts_get|accounts_connect|\
         posts_list|posts_get|posts_create|posts_delete|profiles_list. \
         posts_create/posts_delete/accounts_connect are outward-facing (need approval). \
         Requires `akarso auth login` first. Auto-installed with nur."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": [
                        "auth_check", "accounts_list", "accounts_health", "accounts_get",
                        "accounts_connect", "posts_list", "posts_get", "posts_create",
                        "posts_delete", "profiles_list"
                    ],
                    "default": "auth_check"
                },
                "text": { "type": "string", "description": "posts_create: post body" },
                "platforms": {
                    "type": "string",
                    "description": "Comma-separated platforms, e.g. x,linkedin,instagram"
                },
                "platform": { "type": "string", "description": "Single platform (accounts_get/connect)" },
                "media": { "type": "string", "description": "posts_create: local file path or URL" },
                "scheduled_at": {
                    "type": "string",
                    "description": "posts_create: relative (2h,3d,1w) or ISO timestamp; omit + publish=false to save a draft"
                },
                "publish": {
                    "type": "boolean",
                    "description": "posts_create: publish now (--publish-now). Default false = draft/scheduled",
                    "default": false
                },
                "status": { "type": "string", "description": "posts_list: filter by status" },
                "id": { "type": "string", "description": "posts_get/posts_delete: post id" }
            }
        })
    }

    fn execute(&self, args: &Value, _ctx: &ToolContext) -> Result<String> {
        let bin = ecosystem::find_bin("akarso").ok_or_else(|| {
            MuseError::Tool(
                "akarso CLI not found. nur normally auto-installs it — run: \
                 npm install -g akarso  then  akarso auth login"
                    .into(),
            )
        })?;

        let action = arg_str(args, "action").unwrap_or_else(|_| "auth_check".into());
        let mut argv: Vec<String> = Vec::new();
        let mut timeout_ms = 120_000u64;

        match action.as_str() {
            "auth_check" | "status" => {
                argv.push("auth".into());
                argv.push("check".into());
            }
            "accounts_list" => {
                argv.extend(["accounts".into(), "list".into(), "--json".into()]);
            }
            "accounts_health" => {
                argv.extend(["accounts".into(), "health".into(), "--json".into()]);
            }
            "accounts_get" => {
                let platform = arg_str(args, "platform")?;
                argv.extend(["accounts".into(), "get".into(), platform, "--json".into()]);
            }
            "accounts_connect" => {
                let platform = arg_str(args, "platform")?;
                argv.extend(["accounts".into(), "connect".into(), platform]);
                // Browser OAuth flow — allow time for the user to complete it.
                timeout_ms = 300_000;
            }
            "profiles_list" => {
                argv.extend(["profiles".into(), "list".into(), "--json".into()]);
            }
            "posts_list" => {
                argv.extend(["posts".into(), "list".into()]);
                if let Ok(s) = arg_str(args, "status") {
                    argv.push("--status".into());
                    argv.push(s);
                }
                if let Ok(p) = arg_str(args, "platforms") {
                    argv.push("--platforms".into());
                    argv.push(p);
                }
                argv.push("--json".into());
            }
            "posts_get" => {
                let id = arg_str(args, "id")?;
                argv.extend(["posts".into(), "get".into(), id, "--json".into()]);
            }
            "posts_create" => {
                let text = arg_str(args, "text")?;
                let platforms = arg_str(args, "platforms")?;
                argv.extend([
                    "posts".into(),
                    "create".into(),
                    "--text".into(),
                    text,
                    "--platforms".into(),
                    platforms,
                ]);
                if let Ok(media) = arg_str(args, "media") {
                    argv.push("--media".into());
                    argv.push(media);
                }
                let publish = args
                    .get("publish")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if let Ok(when) = arg_str(args, "scheduled_at") {
                    argv.push("--scheduled-at".into());
                    argv.push(when);
                } else if publish {
                    argv.push("--publish-now".into());
                }
                argv.push("--json".into());
            }
            "posts_delete" => {
                let id = arg_str(args, "id")?;
                argv.extend(["posts".into(), "delete".into(), id]);
            }
            other => {
                return Err(MuseError::Tool(format!(
                    "unknown akarso action '{other}' — auth_check|accounts_list|accounts_health|\
                     accounts_get|accounts_connect|posts_list|posts_get|posts_create|posts_delete|profiles_list"
                )));
            }
        }

        let refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
        ecosystem::run_capture(&bin, &refs, None, timeout_ms).map_err(MuseError::Tool)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_gate_blocks_outward_actions() {
        assert!(is_read_only_action(r#"{"action":"auth_check"}"#));
        assert!(is_read_only_action(r#"{"action":"accounts_list"}"#));
        assert!(is_read_only_action(r#"{"action":"posts_list"}"#));
        assert!(is_read_only_action(r#"{"action":"posts_get","id":"p1"}"#));
        // Outward-facing / mutating → must NOT be free.
        assert!(!is_read_only_action(
            r#"{"action":"posts_create","text":"hi","platforms":"x"}"#
        ));
        assert!(!is_read_only_action(
            r#"{"action":"posts_delete","id":"p1"}"#
        ));
        assert!(!is_read_only_action(
            r#"{"action":"accounts_connect","platform":"x"}"#
        ));
    }
}

//! `secret`: credentials without the conversation.
//!
//! `list` names the secrets the user saved (`crate::secrets`); `request` asks
//! the user for one through the TUI's masked modal and returns only its name,
//! so a key never passes through a prompt, a tool result or the transcript.
//! Shell commands read saved secrets as `$NAME`.
//!
//! `request` is answered by the agent loop (it must wait on the user, like
//! `question`); dispatch only reaches it where no user can answer.

use super::{arg_str, Tool, ToolContext};
use crate::error::{NurError, Result};
use serde_json::Value;

pub struct SecretTool;

/// What a request comes back as when nobody can answer it.
pub const NO_USER: &str = "no interactive user to enter a secret here. Ask the user to run \
                           `nur secrets set NAME` in a terminal (or /secrets add in the TUI), \
                           then use $NAME.";

impl Tool for SecretTool {
    fn name(&self) -> &str {
        "secret"
    }

    fn description(&self) -> &str {
        "The user's saved secrets (API keys, tokens, passwords). Shell commands get each as an \
         environment variable; use $NAME and never print a value. Actions: list (names only) | \
         request (ask the user for one through a masked prompt; args name like GITHUB_TOKEN, \
         purpose: what it is for). Never ask the user to paste a credential into the chat."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["list", "request"], "default": "list"},
                "name": {"type": "string", "description": "For request: the environment variable name, like GITHUB_TOKEN"},
                "purpose": {"type": "string", "description": "For request: one line on what it is for, shown to the user"}
            }
        })
    }

    fn execute(&self, args: &Value, _ctx: &ToolContext) -> Result<String> {
        match arg_str(args, "action")
            .unwrap_or_else(|_| "list".into())
            .as_str()
        {
            "list" => Ok(list()),
            "request" => Err(NurError::Tool(NO_USER.into())),
            other => Err(NurError::Tool(format!(
                "unknown secret action `{other}`; use list|request"
            ))),
        }
    }
}

pub fn list() -> String {
    let names = crate::secrets::names();
    if names.is_empty() {
        return "no secrets saved. Use action request to ask the user for one.".into();
    }
    let mut out = format!("{} secret(s), set in every shell command:\n", names.len());
    for (name, _) in names {
        out.push_str(&format!("- ${name}\n"));
    }
    out
}

/// A request's arguments: the normalized name and the purpose line.
pub fn parse_request(args: &Value) -> Result<(String, String)> {
    let name = crate::secrets::normalize_name(&arg_str(args, "name")?);
    if !crate::secrets::valid_name(&name) {
        return Err(NurError::Tool(format!(
            "`{name}` is not an environment variable name (like GITHUB_TOKEN)"
        )));
    }
    let purpose: String = arg_str(args, "purpose")
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control())
        .take(200)
        .collect();
    Ok((name, purpose))
}

/// The tool result for the user's answer: `Some(name)` saved, `None` declined.
pub fn answer(saved: Option<&str>) -> String {
    match saved {
        Some(name) => format!(
            "the user saved ${name}. It is set in every shell command from now on; use it by \
             name and never print it."
        ),
        None => "the user declined to enter the secret. Continue without it or explain what \
                 it would unlock."
            .into(),
    }
}

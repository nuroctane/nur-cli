//! Shepherd-style retained proposal review.

use super::{arg_str, Tool, ToolContext};
use crate::agent::proposal;
use crate::error::{NurError, Result};
use serde_json::Value;

pub struct ProposalTool;

impl Tool for ProposalTool {
    fn name(&self) -> &str {
        "proposal"
    }

    fn description(&self) -> &str {
        "Retained-output proposals (Shepherd pattern): when proposal_mode is on, \
         writes stage under ~/.nur/proposals instead of the workspace. \
         Actions: list | apply | discard. Review staged files before apply."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "apply", "discard"],
                }
            },
            "required": ["action"]
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action")?;
        // Proposals are staged under the calling session's id (see
        // `commit_file_write`); applying another session's staged writes
        // from here was never intended.
        let session_id = ctx.session_id.as_str();

        match action.as_str() {
            "list" => Ok(proposal::format_list(session_id)),
            "apply" => proposal::apply_all(session_id, &ctx.cwd).map_err(NurError::Tool),
            "discard" => proposal::discard_all(session_id).map_err(NurError::Tool),
            other => Err(NurError::Tool(format!("unknown proposal action `{other}`"))),
        }
    }
}

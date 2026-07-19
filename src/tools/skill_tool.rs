//! Skill tool — list installed skills and load a skill's full instructions
//! on demand. Skills are never bulk-injected into the system prompt.

use super::{arg_str, Tool, ToolContext};
use crate::agent::skills::{load_skills, skills_prompt_section};
use crate::error::{MuseError, Result};
use serde_json::Value;

pub struct SkillTool;

impl Tool for SkillTool {
    fn name(&self) -> &str {
        "skill"
    }

    fn description(&self) -> &str {
        "Agent skills (SKILL.md packs). Skills are on-demand only (not pre-loaded). \
         action=list shows installed skills; action=read loads a named skill's full \
         instructions. Prefer when NL/slash did not already activate a skill."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["list", "read"], "default": "list"},
                "name": {"type": "string", "description": "Skill name (for action=read)"}
            }
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action").unwrap_or_else(|_| "list".into());
        let skills = load_skills(&ctx.cwd);

        match action.as_str() {
            "list" => {
                let mut out = skills_prompt_section(&skills);
                if !skills.is_empty() {
                    out.push_str(
                        "\nUse skill(action=read, name=<name>) for full instructions. \
                         Users can also activate via /skill-name or natural-language intent.",
                    );
                }
                Ok(out)
            }
            "read" => {
                let name = arg_str(args, "name")?;
                let skill = skills
                    .iter()
                    .find(|s| s.name.eq_ignore_ascii_case(&name))
                    .ok_or_else(|| {
                        MuseError::Tool(format!(
                            "skill '{name}' not found — action=list to see installed skills"
                        ))
                    })?;
                // Re-read the file so large packs aren't truncated.
                let body = std::fs::read_to_string(&skill.path)
                    .map(|t| {
                        // Strip YAML frontmatter if present.
                        if t.starts_with("---") {
                            if let Some(end) = t[3..].find("---") {
                                return t[end + 6..].trim().to_string();
                            }
                        }
                        t
                    })
                    .unwrap_or_else(|_| skill.body.clone());
                let body: String = body.chars().take(80_000).collect();
                Ok(format!(
                    "# Skill: {} ({})\n\n{}",
                    skill.name,
                    skill.path.display(),
                    body
                ))
            }
            other => Err(MuseError::Tool(format!(
                "unknown action '{other}' — use list or read"
            ))),
        }
    }
}

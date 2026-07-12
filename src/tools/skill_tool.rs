//! Skill tool — list installed skills and load a skill's full instructions
//! on demand (the system prompt only inlines small skills).

use super::{arg_str, Tool, ToolContext};
use crate::agent::skills::load_skills;
use crate::error::{MuseError, Result};
use serde_json::Value;

pub struct SkillTool;

impl Tool for SkillTool {
    fn name(&self) -> &str {
        "skill"
    }

    fn description(&self) -> &str {
        "Agent skills (SKILL.md packs). action=list shows installed skills; \
         action=read loads a named skill's full instructions — invoke before \
         doing a task a skill covers."
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
                if skills.is_empty() {
                    return Ok(
                        "no skills installed — add <name>/SKILL.md under ~/.muse/skills/, \
                         ~/.agents/skills/, or the workspace .muse/.agents skills dirs. \
                         Graphify: uv tool install graphifyy && graphify install --platform agents"
                            .into(),
                    );
                }
                let mut out = String::from("installed skills\n");
                for s in &skills {
                    out.push_str(&format!("  {} — {}\n", s.name, s.description));
                }
                out.push_str("\nUse skill(action=read, name=<name>) for full instructions.");
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
                // Re-read the file so large packs (e.g. graphify) aren't truncated
                // the way the system-prompt catalog is.
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

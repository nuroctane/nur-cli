//! Skill tool — list installed skills and load a skill's full instructions
//! on demand. Skills are never bulk-injected into the system prompt.

use super::{arg_str, Tool, ToolContext};
use crate::agent::skills::{load_skills, skills_prompt_section};
use crate::error::{NurError, Result};
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
                "name": {"type": "string", "description": "Skill name (for action=read)"},
                "query": {"type": "string", "description": "Substring filter over name/description (for action=list), e.g. \"audit\" or \"fuzz\""}
            }
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action").unwrap_or_else(|_| "list".into());
        let skills = load_skills(&ctx.cwd);

        match action.as_str() {
            "list" => {
                // Optional filter: the full catalog is 1500+ entries and the
                // unfiltered dump used to overflow into the context store,
                // hiding exactly the skills the caller was hunting for
                // (session 5b30168a: `list` truncated before sc-research).
                if let Ok(q) = arg_str(args, "query") {
                    let q = q.trim();
                    if !q.is_empty() {
                        let ql = q.to_ascii_lowercase();
                        let matches: Vec<&crate::agent::skills::Skill> = skills
                            .iter()
                            .filter(|s| {
                                s.name.to_ascii_lowercase().contains(&ql)
                                    || s.description.to_ascii_lowercase().contains(&ql)
                            })
                            .collect();
                        if matches.is_empty() {
                            return Ok(format!(
                                "No installed skills match '{q}'. Try a shorter substring \
                                 (e.g. query=\"audit\", query=\"fuzz\", query=\"defi\")."
                            ));
                        }
                        let mut out =
                            format!("Installed skills matching '{q}' ({}):\n", matches.len());
                        for sk in &matches {
                            out.push_str(&format!("- **{}**: {}\n", sk.name, sk.description));
                        }
                        out.push_str(
                            "\nUse skill(action=read, name=<name>) for full instructions.",
                        );
                        return Ok(out);
                    }
                }
                let mut out = skills_prompt_section(&skills);
                if !skills.is_empty() {
                    out.push_str(
                        "\nUse skill(action=read, name=<name>) for full instructions. \
                         Tip: the catalog is large - skill(action=list, query=\"<substring>\") \
                         filters by name/description. \
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
                        NurError::Tool(format!(
                            "skill '{name}' not found — action=list to see installed skills"
                        ))
                    })?;
                // Re-read the file so large packs aren't truncated.
                let body = std::fs::read_to_string(&skill.path)
                    .map(|t| {
                        // Strip YAML frontmatter if present.
                        if let Some(content) = t.strip_prefix("---") {
                            if let Some(end) = content.find("---") {
                                return content[end + 3..].trim().to_string();
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
            other => Err(NurError::Tool(format!(
                "unknown action '{other}' — use list or read"
            ))),
        }
    }
}

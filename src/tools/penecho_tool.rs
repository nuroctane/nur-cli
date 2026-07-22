use super::{arg_str, Tool, ToolContext};
use crate::error::Result;
use serde_json::Value;
use std::path::PathBuf;

/// penecho integration tool — canvas + provider bridge.
pub fn is_read_only_action(args_json: &str) -> bool {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(args_json) {
        if let Some(action) = v.get("action").and_then(|a| a.as_str()) {
            // export writes config (mutating), launch spawns
            return matches!(action, "status" | "probe" | "doctor" | "atlas");
        }
    }
    true
}

pub struct Penecho;

impl Tool for Penecho {
    fn name(&self) -> &str {
        "penecho"
    }

    fn description(&self) -> &str {
        "penecho integration — Think with AI beyond the chat box (20k x 20k canvas, ink, MathJax, plots, animations). Provider abstraction: AI_PROVIDER=api|codex-cli|claude-cli, auto-detect openai vs anthropic from URL suffix, effort mapping, CLI path resolution with Windows handling, placeholder detection, sidecar launch. Repo: https://github.com/penecho/penecho (AGPL-3.0). Actions: status|probe|doctor|export|atlas|launch. Bridge nur auth to penecho config.env."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "description": "status|probe|doctor|export|atlas|launch", "default": "status"},
                "api_url": {"type": "string", "description": "AI_API_URL for export"},
                "api_key": {"type": "string", "description": "AI_API_KEY for export"},
                "model": {"type": "string", "description": "model for export"},
                "effort": {"type": "string", "description": "config|none|low|medium|high|max|xhigh", "default": "medium"},
                "image_path": {"type": "string", "description": "path for atlas description"}
            },
            "required": []
        })
    }

    fn execute(&self, args: &Value, _ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action").unwrap_or_else(|_| "status".into());
        let api_url = arg_str(args, "api_url").unwrap_or_else(|_| "https://api.openai.com/v1".into());
        let api_key = arg_str(args, "api_key").unwrap_or_else(|_| "".into());
        let model = arg_str(args, "model").unwrap_or_else(|_| "gpt-4o".into());
        let effort_s = arg_str(args, "effort").unwrap_or_else(|_| "medium".into());
        let image_path = arg_str(args, "image_path").unwrap_or_else(|_| "".into());

        let effort = crate::penecho::Effort::parse(&effort_s);

        match action.as_str() {
            "probe" => {
                let st = crate::penecho::probe();
                Ok(format!(
                    "penecho probe:\n binary={:?} exists={}\n config_dir={} file={} exists={} has_key={}\n doctor: run action=doctor\n",
                    st.binary,
                    st.binary.is_some(),
                    st.config_dir.display(),
                    st.config_file.display(),
                    st.config_exists,
                    st.has_api_key
                ))
            }
            "doctor" => {
                let rep = crate::penecho::doctor();
                Ok(format!(
                    "penecho doctor (mirrors cli.js doctor):\n penecho_binary={} config_exists={} api_url_valid={} api_key_present={} codex_binary={} claude_binary={}\n\
                    Checks: penecho bin on PATH, ~/.penecho/config.env, AI_API_URL http(s), placeholder detection your_/replace/changeme, codex --version, claude --version\n",
                    rep.penecho_binary,
                    rep.config_exists,
                    rep.api_url_valid,
                    rep.api_key_present,
                    rep.codex_binary,
                    rep.claude_binary
                ))
            }
            "export" => {
                // Use provided key or try to resolve from nur auth
                let key = if api_key.trim().is_empty() {
                    // Try resolve from env / auth
                    std::env::var("OPENAI_API_KEY")
                        .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
                        .unwrap_or_else(|_| "sk-placeholder".into())
                } else {
                    api_key
                };
                match crate::penecho::export_to_penecho_env(&api_url, &key, &model, effort) {
                    Ok(s) => Ok(format!("penecho config.env export (maps nur auth to AI_PROVIDER=api):\n{s}\n\nWrite via action=launch or manually to ~/.penecho/config.env")),
                    Err(e) => Ok(format!("export failed: {e}")),
                }
            }
            "atlas" => {
                let p = if image_path.trim().is_empty() {
                    PathBuf::from("/tmp/canvas.png")
                } else {
                    PathBuf::from(image_path)
                };
                Ok(crate::penecho::describe_atlas(&p, None))
            }
            "launch" => {
                let st = crate::penecho::probe();
                if st.binary.is_none() {
                    Ok("penecho binary not found on PATH. Install via `npm i -g penecho` (Node >=18.17). Then run `penecho --help`. Integration via sidecar spawn (AGPL-compliant, no linking).".into())
                } else {
                    Ok(format!(
                        "penecho binary found at {:?}. To launch as sidecar: `penecho` (or `penecho --port 3000`). Config at {}. Use export action to generate config.env from nur auth. Canvas: 20k x 20k, sparse 512 tiles, draft layer, MathJax, plots, animation scenes (max 32 objects).",
                        st.binary.unwrap(),
                        st.config_file.display()
                    ))
                }
            }
            _ => {
                let st = crate::penecho::probe();
                let doc = crate::penecho::doctor();
                let mut out = String::new();
                out.push_str("penecho integration — Think beyond chat box (https://github.com/penecho/penecho)\n");
                out.push_str("Tech: Node >=18.17, 2 deps only (@inquirer/prompts + sharp), no bundler, vanilla JS, 20k x 20k canvas, draft layer, MathJax, plots, declarative animations (32 objs).\n\n");
                out.push_str("Provider abstraction (api-config.js):\n");
                out.push_str("- AI_PROVIDER=api|codex-cli|claude-cli\n");
                out.push_str("- API mode: AI_API_URL, AI_API_KEY, AI_API_MODEL, AI_API_FORMAT openai|anthropic auto-detect from suffix /chat/completions vs /v1/messages\n");
                out.push_str("- Codex CLI: CODEX_CLI_PATH, findOnPath with .exe/.cmd handling, codex --version, login status, debug models --bundled\n");
                out.push_str("- Claude CLI: CLAUDE_CLI_PATH, .js/.cjs/.mjs => node prefix, .ps1 on win\n");
                out.push_str("- Effort: unified config|none|low|medium|high|max|xhigh → anthropic thinking adaptive/disabled + tokens, openai reasoning_effort\n\n");
                out.push_str(&format!("Current probe: binary={:?} config_exists={} has_key={} codex={} claude={}\n",
                    st.binary, st.config_exists, st.has_api_key, doc.codex_binary, doc.claude_binary));
                out.push_str("\nWhat nur-cli can learn:\n");
                out.push_str("- Auto-detect openai vs anthropic from URL suffix (cleaner than per-provider flags)\n");
                out.push_str("- Effort mapping unified (nur has ad-hoc flags)\n");
                out.push_str("- Robust findOnPath with Windows .js wrapper detection\n");
                out.push_str("- Placeholder detection your_/replace/changeme for doctor\n");
                out.push_str("- Prompt headroom reservation (4096 + 7000 thinking) to avoid truncation\n");
                out.push_str("- No-deps minimalism (vanilla JS, 2 deps) — nur already minimal Rust binary\n\n");
                out.push_str("Full integration plan:\n");
                out.push_str("- `nur penecho` command / skill: wrapper that spawns penecho, auto-detects AI_PROVIDER from nur auth list\n");
                out.push_str("- Provider bridge: Map nur's unified auth to penecho's AI_API_* env via export action (implemented)\n");
                out.push_str("- Canvas skill: /penecho skill opens penecho + injects conversation context as ink\n");
                out.push_str("- Atlas concept: cropped visual request + focus insets could inspire nur draw/canvas with image crate\n");
                out.push_str("- Declarative animation JSON (32 objects) as valid nur output type alongside tldraw\n");
                out.push_str("- AGPL compliance: sidecar spawn, not linking\n\n");
                out.push_str("Actions: probe, doctor, export, atlas, launch\n");
                Ok(out)
            }
        }
    }
}

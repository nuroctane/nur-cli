use super::{arg_str, Tool, ToolContext};
use crate::error::Result;
use serde_json::Value;
use std::path::PathBuf;

/// penecho integration tool — canvas + provider bridge.
pub fn is_read_only_action(args_json: &str) -> bool {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(args_json) {
        if let Some(action) = v.get("action").and_then(|a| a.as_str()) {
            return matches!(
                action,
                "status" | "probe" | "doctor" | "atlas" | "export_png"
            );
        }
    }
    false
}

pub struct Penecho;

impl Tool for Penecho {
    fn name(&self) -> &str {
        "penecho"
    }

    fn description(&self) -> &str {
        "penecho — AI canvas beyond chat (20k×20k ink, MathJax, plots, animations, flowchart plugins). \
         Auto-installs via npm, auto-writes ~/.penecho/config.env from nur auth (or codex/claude/kimi CLI), \
         opens browser to http://127.0.0.1:3888. \
         Actions: launch|open|restart|stop|status|probe|doctor|export|export_png|inject|atlas. \
         Optional: port=, inject= text, effort=, open=bool, background=true (install in bg job). \
         Never ask the user to diagnose — launch just works. AGPL sidecar."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "description": "launch|open|restart|stop|status|probe|doctor|export|export_png|inject|atlas",
                    "default": "launch"
                },
                "api_url": {"type": "string"},
                "api_key": {"type": "string"},
                "model": {"type": "string"},
                "effort": {"type": "string", "default": "medium"},
                "image_path": {"type": "string"},
                "open": {"type": "boolean", "description": "Open browser (default true for launch)"},
                "port": {"type": "integer", "description": "HTTP port (default 3888)"},
                "inject": {"type": "string", "description": "Conversation/context seed to write + open canvas"},
                "background": {
                    "type": "boolean",
                    "description": "For heavy install: spawn bg job and return id immediately"
                }
            },
            "required": []
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action").unwrap_or_else(|_| "launch".into());
        let api_url =
            arg_str(args, "api_url").unwrap_or_else(|_| "https://api.openai.com/v1".into());
        let api_key = arg_str(args, "api_key").unwrap_or_else(|_| "".into());
        let model = arg_str(args, "model").unwrap_or_else(|_| "gpt-4o".into());
        let effort_s = arg_str(args, "effort").unwrap_or_else(|_| "medium".into());
        let image_path = arg_str(args, "image_path").unwrap_or_else(|_| "".into());
        let open_browser = args.get("open").and_then(|v| v.as_bool()).unwrap_or(true);
        let port = args
            .get("port")
            .and_then(|v| v.as_u64())
            .map(|p| p as u16)
            .unwrap_or(crate::penecho::DEFAULT_PORT);
        let inject = arg_str(args, "inject").ok();
        let background = args
            .get("background")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let effort = crate::penecho::Effort::parse(&effort_s);

        match action.as_str() {
            "status" | "probe" => match crate::penecho::ensure_ready(effort) {
                Ok(msg) => Ok(format!("penecho status (auto-ensured):\n{msg}")),
                Err(e) => Ok(format!("penecho status (partial): {e}")),
            },
            "doctor" => {
                let _ = crate::penecho::ensure_installed();
                let _ = crate::penecho::auto_configure_from_nur(false, effort);
                let rep = crate::penecho::doctor();
                Ok(format!(
                    "penecho doctor:\n binary={} config={} usable={} url_ok={} key={} codex={} claude={} kimi={} listening={}\n canvas={}\n",
                    rep.penecho_binary,
                    rep.config_exists,
                    crate::penecho::config_is_usable(),
                    rep.api_url_valid,
                    rep.api_key_present,
                    rep.codex_binary,
                    rep.claude_binary,
                    crate::penecho::find_on_path("kimi").is_some(),
                    crate::penecho::port_is_open(port),
                    crate::penecho::canvas_url(port)
                ))
            }
            "export" => {
                let key = if api_key.trim().is_empty() {
                    "sk-local".to_string()
                } else {
                    api_key
                };
                match crate::penecho::export_to_penecho_env(&api_url, &key, &model, effort, true) {
                    Ok(s) => Ok(format!(
                        "penecho config.env export (redacted):\n{s}\n\
                         Real secrets: penecho(action=launch) writes them locally only."
                    )),
                    Err(e) => Ok(format!("export failed: {e}")),
                }
            }
            "export_png" => crate::penecho::export_png_hint(&ctx.cwd),
            "atlas" => {
                let p = if image_path.trim().is_empty() {
                    PathBuf::from("/tmp/canvas.png")
                } else {
                    PathBuf::from(image_path)
                };
                Ok(crate::penecho::describe_atlas(&p, None))
            }
            "inject" => {
                let text = inject.unwrap_or_default();
                if text.trim().is_empty() {
                    return Ok(
                        "inject requires inject= text (conversation/context seed)".into(),
                    );
                }
                let path = crate::penecho::write_inject_seed(&text)?;
                // Ensure canvas is up and open.
                let launch = crate::penecho::launch_seamless_on_port(
                    open_browser,
                    effort,
                    port,
                    Some(&text),
                )?;
                Ok(format!(
                    "inject seed → {}\n\
                     Paste into canvas text tool or AI menu (also on clipboard if available).\n\
                     {launch}",
                    path.display()
                ))
            }
            "stop" => crate::penecho::stop(port),
            "restart" => crate::penecho::restart(open_browser, effort, port),
            "launch" | "open" | "start" | "run" => {
                if background {
                    let id = crate::bg_jobs::spawn(
                        format!("penecho launch :{port}"),
                        "penecho",
                        move |_c| {
                            crate::penecho::launch_seamless_on_port(
                                open_browser,
                                effort,
                                port,
                                inject.as_deref(),
                            )
                            .map_err(|e| e.to_string())
                        },
                    );
                    return Ok(format!(
                        "bg job #{id} · penecho launch on :{port}\n  \
                         continue working — bg(action=result, id={id}) when ready\n  \
                         canvas will be {}\n",
                        crate::penecho::canvas_url(port)
                    ));
                }
                crate::penecho::launch_seamless_on_port(
                    open_browser,
                    effort,
                    port,
                    inject.as_deref(),
                )
            }
            _ => match crate::penecho::ensure_ready(effort) {
                Ok(msg) => Ok(format!(
                    "penecho (action '{action}' → status):\n{msg}\n\
                     Actions: launch|open|restart|stop|status|doctor|export|export_png|inject|atlas"
                )),
                Err(e) => Ok(format!("penecho: {e}")),
            },
        }
    }
}

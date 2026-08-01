use super::{arg_str, Tool, ToolContext};
use crate::egaki;
use crate::error::{NurError, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub struct Egaki;

pub fn is_read_only_action(args: &str) -> bool {
    let action = serde_json::from_str::<Value>(args)
        .ok()
        .and_then(|v| v.get("action")?.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "status".into());
    matches!(
        action.as_str(),
        "status" | "doctor" | "probe" | "models" | "usage"
    )
}

impl Tool for Egaki {
    fn name(&self) -> &str {
        "egaki"
    }

    fn description(&self) -> &str {
        "egaki image/video/speech CLI (https://github.com/remorses/egaki). \
         actions: status|doctor|login|image|video|speech|models|usage. \
         Auth: ChatGPT sub (`login` provider=chatgpt), xAI Grok Build (`xai-oauth`), \
         Egaki plan (`egaki` key), or BYOK (google/openai/fal/…). \
         Outputs under .nur/media/ - then use look."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": [
                        "status", "doctor", "probe", "login",
                        "image", "video", "speech", "models", "usage"
                    ],
                    "default": "status"
                },
                "provider": {
                    "type": "string",
                    "description": "For login: chatgpt | xai-oauth | egaki | google | openai | fal | vertex | replicate | …"
                },
                "prompt": { "type": "string", "description": "Image/video/speech text" },
                "output": { "type": "string", "description": "Output path (default .nur/media/...)" },
                "model": { "type": "string" },
                "aspect_ratio": { "type": "string" },
                "input": { "type": "string", "description": "Input image/video path for edit/i2v" },
                "duration": { "type": "string", "description": "Video duration seconds" },
                "voice": { "type": "string", "description": "Speech voice id/name" },
                "n": { "type": "integer", "description": "Number of variants" }
            }
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action").unwrap_or_else(|_| "status".into());
        match action.as_str() {
            "status" | "doctor" | "probe" => Ok(egaki::doctor_report()),
            "usage" => egaki::run_egaki_cancelled(&["usage"], Some(&ctx.cwd), 30_000, &ctx.cancel)
                .map_err(NurError::Tool),
            "models" => egaki::run_egaki_cancelled(
                &["models", "--json"],
                Some(&ctx.cwd),
                60_000,
                &ctx.cancel,
            )
            .or_else(|_| {
                egaki::run_egaki_cancelled(&["models"], Some(&ctx.cwd), 60_000, &ctx.cancel)
            })
            .map_err(NurError::Tool),
            "login" => {
                let provider = arg_str(args, "provider").unwrap_or_default();
                let provider = provider.trim();
                let mut msg = String::from(
                    "egaki login is interactive (device auth / key paste). Run in a real terminal:\n\n",
                );
                if provider.is_empty() || provider.eq_ignore_ascii_case("chatgpt") {
                    msg.push_str("  egaki login --provider chatgpt\n");
                    msg.push_str("    ChatGPT subscription → gpt-image via Codex device auth\n");
                }
                if provider.is_empty() || provider.eq_ignore_ascii_case("xai-oauth") {
                    msg.push_str("  egaki login --provider xai-oauth\n");
                    msg.push_str("    xAI Grok Build subscription → image/video\n");
                }
                if provider.is_empty() || provider.eq_ignore_ascii_case("egaki") {
                    msg.push_str("  egaki subscribe --plan pro\n");
                    msg.push_str("  egaki login --provider egaki --key egaki_…\n");
                }
                if !provider.is_empty()
                    && !matches!(
                        provider.to_ascii_lowercase().as_str(),
                        "chatgpt" | "xai-oauth" | "egaki"
                    )
                {
                    msg.push_str(&format!(
                        "  egaki login --provider {provider} --key <KEY>\n"
                    ));
                }
                msg.push_str("  egaki login\n");
                msg.push_str("  egaki login --show\n");
                if let Some(bin) = egaki::find_egaki() {
                    msg.push_str(&format!("\nbinary: {bin}\n"));
                }
                if let Some(show) = egaki::login_show_summary() {
                    msg.push_str("\n--- current ---\n");
                    msg.push_str(&show);
                }
                Ok(msg)
            }
            "image" | "video" | "speech" => {
                let prompt = arg_str(args, "prompt")
                    .map_err(|_| NurError::Tool(format!("{action} requires prompt=")))?;
                let media = egaki::media_dir(&ctx.cwd);
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                let uniq = uuid::Uuid::new_v4().simple().to_string();
                let default_name = match action.as_str() {
                    "video" => format!("egaki-{stamp}-{uniq}.mp4"),
                    "speech" => format!("egaki-{stamp}-{uniq}.mp3"),
                    _ => format!("egaki-{stamp}-{uniq}.png"),
                };
                let out = arg_str(args, "output")
                    .unwrap_or_else(|_| media.join(&default_name).to_string_lossy().into_owned());
                let out_path = resolve_output_under_cwd(&ctx.cwd, &out)?;
                if out_path
                    .symlink_metadata()
                    .map(|m| m.file_type().is_symlink())
                    .unwrap_or(false)
                {
                    return Err(NurError::Tool(
                        "egaki output must not be an existing symlink".into(),
                    ));
                }
                if let Some(parent) = out_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        NurError::Tool(format!("egaki mkdir {}: {e}", parent.display()))
                    })?;
                }

                let mut argv: Vec<String> = vec![action.to_string(), prompt];
                argv.push("-o".into());
                argv.push(out_path.to_string_lossy().into());
                argv.push("--json".into());
                if let Ok(m) = arg_str(args, "model") {
                    argv.push("-m".into());
                    argv.push(m);
                }
                if let Ok(ar) = arg_str(args, "aspect_ratio") {
                    argv.push("--aspect-ratio".into());
                    argv.push(ar);
                }
                if let Ok(inp) = arg_str(args, "input") {
                    let inp_path = resolve_existing_under_cwd(&ctx.cwd, &inp)?;
                    argv.push("--input".into());
                    argv.push(inp_path.to_string_lossy().into());
                }
                if let Ok(d) = arg_str(args, "duration") {
                    argv.push("--duration".into());
                    argv.push(d);
                }
                if let Ok(voice) = arg_str(args, "voice") {
                    argv.push("--voice".into());
                    argv.push(voice);
                }
                if let Some(n) = args.get("n").and_then(|v| v.as_u64()) {
                    argv.push("-n".into());
                    argv.push(n.to_string());
                }
                let refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
                let timeout = if action == "video" { 600_000 } else { 300_000 };
                let out = egaki::run_egaki_cancelled(&refs, Some(&ctx.cwd), timeout, &ctx.cancel)
                    .map_err(NurError::Tool)?;
                Ok(format!(
                    "{out}\n\noutput: {}\nTip: use look on that path to attach vision.",
                    out_path.display()
                ))
            }
            other => Ok(format!(
                "unknown egaki action '{other}' - status|login|image|video|speech|models|usage"
            )),
        }
    }
}

fn path_under_root(cand: &Path, root: &Path) -> bool {
    let root = strip_verbatim(root);
    let cand = strip_verbatim(cand);
    let r: Vec<_> = root.components().collect();
    let c: Vec<_> = cand.components().collect();
    if c.len() < r.len() {
        return false;
    }
    #[cfg(windows)]
    {
        c.iter()
            .zip(r.iter())
            .all(|(a, b)| a.as_os_str().eq_ignore_ascii_case(b.as_os_str()))
    }
    #[cfg(not(windows))]
    {
        c.iter().zip(r.iter()).all(|(a, b)| a == b)
    }
}

fn strip_verbatim(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

fn resolve_output_under_cwd(cwd: &Path, out: &str) -> Result<PathBuf> {
    let cwd_canon = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let candidate = if PathBuf::from(out).is_absolute() {
        PathBuf::from(out)
    } else {
        cwd.join(out)
    };
    let lex = crate::tools::sandbox::normalize_path(&candidate);
    if !path_under_root(&lex, &crate::tools::sandbox::normalize_path(&cwd_canon)) {
        return Err(NurError::Tool(
            "egaki output path must stay under the workspace".into(),
        ));
    }
    Ok(lex)
}

fn resolve_existing_under_cwd(cwd: &Path, inp: &str) -> Result<PathBuf> {
    let cwd_canon = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let candidate = if PathBuf::from(inp).is_absolute() {
        PathBuf::from(inp)
    } else {
        cwd.join(inp)
    };
    let canon = candidate
        .canonicalize()
        .map_err(|_| NurError::Tool(format!("egaki input not found: {inp}")))?;
    if !path_under_root(&canon, &cwd_canon) {
        return Err(NurError::Tool(
            "egaki input path must stay under the workspace".into(),
        ));
    }
    Ok(canon)
}

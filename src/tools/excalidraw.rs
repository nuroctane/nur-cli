//! Excalidraw diagram tool — wraps `excalidraw-cli` (npm: excalidraw-cli).
//!
//! Create hand-drawn `.excalidraw` files from element JSON, export share URLs,
//! and fetch the element-format reference. Read actions (status / reference /
//! checkpoint list) are approval-free; create/export/checkpoint mutators need
//! approval in manual mode.

use super::{arg_str, resolve_path, Tool, ToolContext};
use crate::error::{MuseError, Result};
use serde_json::Value;
use std::path::Path;

pub struct Excalidraw;

/// Actions that only inspect CLI / format (or list checkpoints).
pub fn is_read_only_action(args: &str) -> bool {
    let v: Value = serde_json::from_str(args).unwrap_or_else(|_| Value::Object(Default::default()));
    let action = v.get("action").and_then(|a| a.as_str()).unwrap_or("status");
    match action {
        "status" | "reference" | "ref" => true,
        "checkpoint" => {
            let sub = v
                .get("checkpoint_action")
                .or_else(|| v.get("subaction"))
                .and_then(|a| a.as_str())
                .unwrap_or("list");
            sub == "list"
        }
        _ => false,
    }
}

impl Tool for Excalidraw {
    fn name(&self) -> &str {
        "excalidraw"
    }

    fn description(&self) -> &str {
        "Create hand-drawn Excalidraw diagrams and OPEN them for the user. \
         Prefer for architecture diagrams, flowcharts, Venn diagrams, decision trees. \
         action=create: write .excalidraw, upload to excalidraw.com, OPEN the share URL in the default browser (default). \
         action=export: upload existing file + open share URL; \
         action=status | reference | checkpoint. \
         ALWAYS use create for user-facing diagrams so they actually see it — do not only dump a dead link. \
         open=false to skip browser. Requires excalidraw-cli on PATH (auto-installed by ecosystem)."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["status", "create", "export", "reference", "checkpoint"],
                    "default": "status"
                },
                "elements": {
                    "description": "For create: JSON array of elements (preferred) or a JSON string"
                },
                "output": {
                    "type": "string",
                    "description": "For create/checkpoint load: workspace-relative .excalidraw path"
                },
                "path": {
                    "type": "string",
                    "description": "For export or checkpoint save: path to an existing .excalidraw file"
                },
                "no_checkpoint": {
                    "type": "boolean",
                    "description": "For create: pass --no-checkpoint (default false)"
                },
                "open": {
                    "type": "boolean",
                    "description": "Open share URL (and local file) in the default browser/app. Default true for create/export."
                },
                "checkpoint_action": {
                    "type": "string",
                    "enum": ["list", "save", "load", "remove"],
                    "description": "For action=checkpoint"
                },
                "name": {
                    "type": "string",
                    "description": "Checkpoint name for save/load/remove"
                }
            }
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action").unwrap_or_else(|_| "status".into());
        match action.as_str() {
            "status" => status(),
            "reference" | "ref" => run_cli(&["reference", "--raw", "--no-banner"], None, 30_000),
            "create" => create(args, &ctx.cwd),
            "export" => export_file(args, &ctx.cwd),
            "checkpoint" => checkpoint(args, &ctx.cwd),
            other => Err(MuseError::Tool(format!(
                "unknown excalidraw action '{other}' — use status|create|export|reference|checkpoint"
            ))),
        }
    }
}

fn want_open(args: &Value) -> bool {
    args.get("open").and_then(|v| v.as_bool()).unwrap_or(true)
}

fn create(args: &Value, cwd: &Path) -> Result<String> {
    let output = arg_str(args, "output").map_err(|_| {
        MuseError::Tool("create requires output= path (e.g. docs/arch.excalidraw)".into())
    })?;
    let abs_out = resolve_path(&cwd.to_path_buf(), &output)?;
    if let Some(parent) = abs_out.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| MuseError::Tool(format!("create parent dir {}: {e}", parent.display())))?;
    }

    let elements_json = elements_to_json_string(args)?;
    let no_cp = args
        .get("no_checkpoint")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let out_s = abs_out.to_string_lossy().into_owned();
    let mut cli_args: Vec<String> = vec![
        "create".into(),
        "--json".into(),
        elements_json,
        "-o".into(),
        out_s.clone(),
        "--no-banner".into(),
    ];
    if no_cp {
        cli_args.push("--no-checkpoint".into());
    }

    let result = run_cli_owned(&cli_args, Some(cwd), 60_000)?;
    let mut s = format!("wrote {}\n", abs_out.display());
    if !result.trim().is_empty() {
        s.push_str(result.trim());
        s.push('\n');
    }

    // Default: export + open so the user actually *sees* the diagram.
    match export_and_maybe_open(&abs_out, cwd, want_open(args)) {
        Ok(export_msg) => {
            s.push_str(&export_msg);
        }
        Err(e) => {
            s.push_str(&format!(
                "export/open failed: {e}\n\
                 file is still on disk — open manually: {}\n",
                abs_out.display()
            ));
            if want_open(args) {
                let _ = crate::open_uri::open_path(&abs_out);
                s.push_str("opened local .excalidraw with the default app (if associated)\n");
            }
        }
    }
    Ok(s)
}

fn export_file(args: &Value, cwd: &Path) -> Result<String> {
    let path = arg_str(args, "path")
        .or_else(|_| arg_str(args, "output"))
        .map_err(|_| MuseError::Tool("export requires path= to a .excalidraw file".into()))?;
    let abs = resolve_path(&cwd.to_path_buf(), &path)?;
    if !abs.is_file() {
        return Err(MuseError::Tool(format!(
            "file not found: {}",
            abs.display()
        )));
    }
    export_and_maybe_open(&abs, cwd, want_open(args))
}

/// Upload to excalidraw.com and optionally open the share URL in the browser.
fn export_and_maybe_open(abs: &Path, cwd: &Path, open: bool) -> Result<String> {
    let out = run_cli(
        &["export", &abs.to_string_lossy(), "--no-banner"],
        Some(cwd),
        60_000,
    )?;
    let mut s = out.clone();
    if !s.ends_with('\n') {
        s.push('\n');
    }
    if let Some(url) = extract_excalidraw_url(&out) {
        s.push_str(&format!("share_url: {url}\n"));
        if open {
            match crate::open_uri::open(&url) {
                Ok(()) => s.push_str("opened share URL in your default browser\n"),
                Err(e) => s.push_str(&format!(
                    "could not open browser automatically ({e}) — paste the share_url above\n"
                )),
            }
        }
    } else if open {
        // No URL parsed — still try opening the local file.
        let _ = crate::open_uri::open_path(abs);
        s.push_str("no share URL parsed from export; opened local file instead\n");
    }
    Ok(s)
}

fn extract_excalidraw_url(text: &str) -> Option<String> {
    // Prefer the shared URL finder (handles trailing punctuation).
    crate::open_uri::find_url_spans(text)
        .into_iter()
        .map(|(_, _, u)| u)
        .find(|u| u.contains("excalidraw"))
        .or_else(|| {
            crate::open_uri::find_url_spans(text)
                .into_iter()
                .next()
                .map(|(_, _, u)| u)
        })
}

fn elements_to_json_string(args: &Value) -> Result<String> {
    let el = args.get("elements").ok_or_else(|| {
        MuseError::Tool("create requires elements= (JSON array of shapes/arrows)".into())
    })?;
    match el {
        Value::String(s) => {
            // Allow either raw array string or already-stringified JSON.
            let trimmed = s.trim();
            if trimmed.starts_with('[') || trimmed.starts_with('{') {
                Ok(trimmed.to_string())
            } else {
                Err(MuseError::Tool(
                    "elements string must be a JSON array (starts with [)".into(),
                ))
            }
        }
        Value::Array(_) | Value::Object(_) => serde_json::to_string(el)
            .map_err(|e| MuseError::Tool(format!("serialize elements: {e}"))),
        _ => Err(MuseError::Tool(
            "elements must be a JSON array or a JSON string".into(),
        )),
    }
}

fn checkpoint(args: &Value, cwd: &Path) -> Result<String> {
    let sub = arg_str(args, "checkpoint_action")
        .or_else(|_| arg_str(args, "subaction"))
        .unwrap_or_else(|_| "list".into());
    match sub.as_str() {
        "list" => run_cli(&["checkpoint", "list", "--no-banner"], Some(cwd), 15_000),
        "save" => {
            let name = arg_str(args, "name")?;
            let path = arg_str(args, "path").or_else(|_| arg_str(args, "output"))?;
            let abs = resolve_path(&cwd.to_path_buf(), &path)?;
            run_cli(
                &[
                    "checkpoint",
                    "save",
                    &name,
                    &abs.to_string_lossy(),
                    "--no-banner",
                ],
                Some(cwd),
                15_000,
            )
        }
        "load" => {
            let name = arg_str(args, "name")?;
            let output = arg_str(args, "output")
                .map_err(|_| MuseError::Tool("checkpoint load requires output= path".into()))?;
            let abs = resolve_path(&cwd.to_path_buf(), &output)?;
            if let Some(parent) = abs.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            run_cli(
                &[
                    "checkpoint",
                    "load",
                    &name,
                    "-o",
                    &abs.to_string_lossy(),
                    "--no-banner",
                ],
                Some(cwd),
                15_000,
            )
        }
        "remove" => {
            let name = arg_str(args, "name")?;
            run_cli(
                &["checkpoint", "remove", &name, "--no-banner"],
                Some(cwd),
                15_000,
            )
        }
        other => Err(MuseError::Tool(format!(
            "unknown checkpoint_action '{other}' — use list|save|load|remove"
        ))),
    }
}

fn status() -> Result<String> {
    let mut s = String::new();
    match find_excalidraw_bin() {
        Some(bin) => {
            s.push_str(&format!("excalidraw CLI: {bin}\n"));
            if let Ok(ver) = crate::ecosystem::run_capture(&bin, &["--version"], None, 10_000) {
                let line = ver.lines().next().unwrap_or(ver.trim()).trim();
                if !line.is_empty() {
                    s.push_str(&format!("version: {line}\n"));
                }
            }
            s.push_str(
                "actions: create | export | reference | checkpoint | status\n\
                 hint: skill(action=read, name=excalidraw) for element templates\n",
            );
        }
        None => {
            s.push_str(
                "excalidraw CLI: NOT FOUND\n\
                 install:  npm i -g excalidraw-cli\n\
                 or:       nur ecosystem (auto-provisions when Node is available)\n\
                 package:  https://github.com/ahmadawais/excalidraw-cli\n",
            );
        }
    }
    Ok(s)
}

fn find_excalidraw_bin() -> Option<String> {
    crate::ecosystem::find_bin("excalidraw")
        .or_else(|| crate::ecosystem::find_bin("excalidraw-cli"))
}

fn missing_cli_err() -> MuseError {
    MuseError::Tool(
        "excalidraw CLI not found on PATH. Install with:\n  \
         npm i -g excalidraw-cli\n\
         Or run: nur ecosystem  (auto-installs when Node.js is present)\n\
         Upstream: https://github.com/ahmadawais/excalidraw-cli"
            .into(),
    )
}

fn run_cli(args: &[&str], cwd: Option<&Path>, timeout_ms: u64) -> Result<String> {
    let owned: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    run_cli_owned(&owned, cwd, timeout_ms)
}

fn run_cli_owned(args: &[String], cwd: Option<&Path>, timeout_ms: u64) -> Result<String> {
    let bin = find_excalidraw_bin().ok_or_else(missing_cli_err)?;
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    crate::ecosystem::run_capture(&bin, &arg_refs, cwd, timeout_ms).map_err(MuseError::Tool)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_actions() {
        assert!(is_read_only_action(r#"{"action":"status"}"#));
        assert!(is_read_only_action(r#"{"action":"reference"}"#));
        assert!(is_read_only_action(
            r#"{"action":"checkpoint","checkpoint_action":"list"}"#
        ));
        assert!(!is_read_only_action(r#"{"action":"create"}"#));
        assert!(!is_read_only_action(r#"{"action":"export"}"#));
        assert!(!is_read_only_action(
            r#"{"action":"checkpoint","checkpoint_action":"save"}"#
        ));
        assert!(is_read_only_action("{}"), "default action is status");
    }

    #[test]
    fn elements_accepts_array() {
        let args = serde_json::json!({
            "elements": [{"type":"rectangle","id":"r1","x":0,"y":0,"width":100,"height":50}]
        });
        let s = elements_to_json_string(&args).unwrap();
        assert!(s.starts_with('['));
        assert!(s.contains("rectangle"));
    }

    #[test]
    fn extract_url_from_export_output() {
        let sample = "Uploading…\nhttps://excalidraw.com/#json=abc123,keyXYZ\ndone\n";
        assert_eq!(
            extract_excalidraw_url(sample).as_deref(),
            Some("https://excalidraw.com/#json=abc123,keyXYZ")
        );
        assert!(extract_excalidraw_url("no url here").is_none());
    }
}

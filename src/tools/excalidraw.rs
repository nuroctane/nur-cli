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
         open=true opens the browser share URL ONLY — never OS-opens the local .excalidraw file \
         (avoids Windows Open-with). open=false to skip browser. \
         Requires excalidraw-cli on PATH (auto-installed by ecosystem)."
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
                "elements_path": {
                    "type": "string",
                    "description": "For create: path to a JSON file containing the elements array"
                },
                "from_mermaid": {
                    "type": "string",
                    "description": "For create: mermaid flowchart text (A[Label] --> B[Label]) converted to elements"
                },
                "output": {
                    "type": "string",
                    "description": "For create/checkpoint load: workspace path (prefer docs/ or .nur/diagrams/ — never Desktop)"
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
                    "description": "Open share URL in the default browser only (never the local .excalidraw file). Default true for create/export."
                },
                "checkpoint_action": {
                    "type": "string",
                    "enum": ["list", "save", "load", "remove"],
                    "description": "For action=checkpoint"
                },
                "name": {
                    "type": "string",
                    "description": "Checkpoint name (save/load/remove) OR create slug under .nur/diagrams/<name>.excalidraw when output omitted"
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

fn default_diagrams_dir(cwd: &Path) -> std::path::PathBuf {
    let preferred = cwd.join(".nur").join("diagrams");
    preferred
}

fn create(args: &Value, cwd: &Path) -> Result<String> {
    let output = match arg_str(args, "output") {
        Ok(o) => o,
        Err(_) => {
            // Default under .nur/diagrams/ (Desktop reserved for tldraw boards).
            let name = arg_str(args, "name").unwrap_or_else(|_| "diagram".into());
            let slug: String = name
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() {
                        c.to_ascii_lowercase()
                    } else {
                        '-'
                    }
                })
                .collect();
            let slug = slug.trim_matches('-');
            let slug = if slug.is_empty() { "diagram" } else { slug };
            format!(".nur/diagrams/{slug}.excalidraw")
        }
    };
    let abs_out = resolve_path(cwd, &output)?;
    // Refuse Desktop for excalidraw (product policy: Desktop = tldraw).
    if abs_out
        .to_string_lossy()
        .to_ascii_lowercase()
        .contains("desktop")
    {
        let alt = default_diagrams_dir(cwd).join(
            abs_out
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "diagram.excalidraw".into()),
        );
        return Err(MuseError::Tool(format!(
            "excalidraw output must not be on Desktop (reserved for tldraw boards). \
             Use docs/ or .nur/diagrams/ — e.g. {}",
            alt.display()
        )));
    }
    if let Some(parent) = abs_out.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| MuseError::Tool(format!("create parent dir {}: {e}", parent.display())))?;
    }

    let elements_json = if args.get("elements").is_some() {
        elements_to_json_string(args)?
    } else if let Ok(path) = arg_str(args, "elements_path") {
        let p = resolve_path(cwd, &path)?;
        std::fs::read_to_string(&p)
            .map_err(|e| MuseError::Tool(format!("read elements_path {}: {e}", p.display())))?
    } else if let Ok(mmd) = arg_str(args, "from_mermaid") {
        mermaid_to_elements_json(&mmd)?
    } else {
        return Err(MuseError::Tool(
            "create requires elements= JSON, elements_path= file, or from_mermaid= text".into(),
        ));
    };
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

    // Default: export + open share URL in browser so the user actually *sees* the diagram.
    // Product policy: never OS-open the local .excalidraw (Windows "Open with" dialog).
    match export_and_maybe_open(&abs_out, cwd, want_open(args)) {
        Ok(export_msg) => {
            s.push_str(&export_msg);
        }
        Err(e) => {
            s.push_str(&format!(
                "export/open failed: {e}\n\
                 file is on disk at {}; paste the share URL when export works\n\
                 (never OS-open .excalidraw — browser share URL only)\n",
                abs_out.display()
            ));
        }
    }
    Ok(s)
}

fn export_file(args: &Value, cwd: &Path) -> Result<String> {
    let path = arg_str(args, "path")
        .or_else(|_| arg_str(args, "output"))
        .map_err(|_| MuseError::Tool("export requires path= to a .excalidraw file".into()))?;
    let abs = resolve_path(cwd, &path)?;
    if !abs.is_file() {
        return Err(MuseError::Tool(format!(
            "file not found: {}",
            abs.display()
        )));
    }
    export_and_maybe_open(&abs, cwd, want_open(args))
}

/// Upload to excalidraw.com and optionally open the share URL in the browser.
///
/// **Open policy:** browser share URL only. Never call `open_path` on the local
/// `.excalidraw` file — that triggers Windows "Open with" when no association exists.
/// tldraw offline is the desktop-app diagram path; Excalidraw is web-only.
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
    match browser_url_to_open(open, &out) {
        Some(url) => {
            s.push_str(&format!("share_url: {url}\n"));
            match crate::open_uri::open(&url) {
                Ok(()) => s.push_str("opened share URL in your default browser\n"),
                Err(e) => s.push_str(&format!(
                    "could not open browser automatically ({e}) — paste the share_url above\n"
                )),
            }
        }
        None if open => {
            // open=true but export produced no share URL — leave file on disk, no local open.
            s.push_str(
                "no share URL parsed from export; left local file on disk \
                 (browser share URL only — never OS-open .excalidraw)\n",
            );
        }
        None => {
            // open=false: still surface a share URL if present for copy/paste.
            if let Some(url) = extract_excalidraw_url(&out) {
                s.push_str(&format!("share_url: {url}\n"));
            }
        }
    }
    Ok(s)
}

/// Product policy helper: which URL (if any) to open in the browser.
///
/// - `open=false` → never open
/// - `open=true` + share URL → open that URL only
/// - `open=true` + no URL → open nothing (never fall back to local file)
fn browser_url_to_open(open: bool, export_stdout: &str) -> Option<String> {
    if !open {
        return None;
    }
    extract_excalidraw_url(export_stdout)
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

/// Minimal mermaid flowchart → excalidraw elements (linear / TD / LR boxes + arrows).
/// Covers the common `A[Label] --> B[Label]` pattern agents emit.
fn mermaid_to_elements_json(mmd: &str) -> Result<String> {
    let mut nodes: Vec<(String, String)> = Vec::new();
    let mut edges: Vec<(String, String, String)> = Vec::new();
    for raw in mmd.lines() {
        let line = raw.trim();
        if line.is_empty()
            || line.starts_with("graph")
            || line.starts_with("flowchart")
            || line.starts_with("%%")
        {
            continue;
        }
        // A[Label] --> B[Label]  or  A --> B
        let re_edge = regex::Regex::new(
            r#"(?P<a>[A-Za-z0-9_]+)(?:\[(?P<al>[^\]]*)\])?\s*--?>\s*(?:\|(?P<label>[^|]+)\|)?\s*(?P<b>[A-Za-z0-9_]+)(?:\[(?P<bl>[^\]]*)\])?"#,
        )
        .map_err(|e| MuseError::Tool(format!("mermaid regex: {e}")))?;
        if let Some(c) = re_edge.captures(line) {
            let a = c.name("a").unwrap().as_str().to_string();
            let b = c.name("b").unwrap().as_str().to_string();
            let al = c
                .name("al")
                .map(|m| m.as_str().to_string())
                .unwrap_or_else(|| a.clone());
            let bl = c
                .name("bl")
                .map(|m| m.as_str().to_string())
                .unwrap_or_else(|| b.clone());
            let elabel = c
                .name("label")
                .map(|m| m.as_str().trim().to_string())
                .unwrap_or_default();
            if !nodes.iter().any(|(id, _)| id == &a) {
                nodes.push((a.clone(), al));
            }
            if !nodes.iter().any(|(id, _)| id == &b) {
                nodes.push((b.clone(), bl));
            }
            edges.push((a, b, elabel));
            continue;
        }
        // Lone node A[Label]
        let re_node = regex::Regex::new(r#"^(?P<a>[A-Za-z0-9_]+)\[(?P<al>[^\]]*)\]"#)
            .map_err(|e| MuseError::Tool(format!("mermaid node regex: {e}")))?;
        if let Some(c) = re_node.captures(line) {
            let a = c.name("a").unwrap().as_str().to_string();
            let al = c.name("al").unwrap().as_str().to_string();
            if !nodes.iter().any(|(id, _)| id == &a) {
                nodes.push((a, al));
            }
        }
    }
    if nodes.is_empty() {
        return Err(MuseError::Tool(
            "from_mermaid: no nodes parsed — use flowchart lines like A[Start] --> B[End]".into(),
        ));
    }

    let mut elements: Vec<Value> = Vec::new();
    elements.push(serde_json::json!({
        "type": "cameraUpdate", "width": 1200, "height": 900, "x": 0, "y": 100
    }));
    elements.push(serde_json::json!({
        "type": "rectangle", "id": "darkbg", "x": -4000, "y": -3000,
        "width": 10000, "height": 7500,
        "backgroundColor": "#1e1e2e", "fillStyle": "solid",
        "strokeColor": "transparent", "strokeWidth": 0
    }));

    let box_w = 200.0;
    let box_h = 80.0;
    let gap = 80.0;
    let start_x = 60.0;
    let y = 350.0;
    let mut positions: std::collections::HashMap<String, (f64, f64)> =
        std::collections::HashMap::new();
    for (i, (id, label)) in nodes.iter().enumerate() {
        let x = start_x + i as f64 * (box_w + gap);
        positions.insert(id.clone(), (x, y));
        let bound: Vec<Value> = edges
            .iter()
            .enumerate()
            .filter(|(_, (a, b, _))| a == id || b == id)
            .map(|(ei, _)| serde_json::json!({"id": format!("a{ei}"), "type": "arrow"}))
            .collect();
        elements.push(serde_json::json!({
            "type": "rectangle",
            "id": id,
            "x": x, "y": y, "width": box_w, "height": box_h,
            "backgroundColor": "#1e3a5f", "fillStyle": "solid", "strokeColor": "#4a9eed",
            "label": { "text": label, "strokeColor": "#e5e5e5" },
            "boundElements": bound
        }));
    }
    for (ei, (a, b, label)) in edges.iter().enumerate() {
        let (ax, ay) = positions[a];
        let (bx, by) = positions[b];
        let x = ax + box_w;
        let y = ay + box_h / 2.0;
        let w = (bx - x).max(20.0);
        let h = by + box_h / 2.0 - y;
        elements.push(serde_json::json!({
            "type": "arrow",
            "id": format!("a{ei}"),
            "x": x, "y": y, "width": w, "height": h,
            "points": [[0,0],[w,h]],
            "endArrowhead": "arrow",
            "strokeColor": "#4a9eed",
            "startBinding": { "elementId": a, "fixedPoint": [1, 0.5] },
            "endBinding": { "elementId": b, "fixedPoint": [0, 0.5] },
            "label": { "text": label, "strokeColor": "#a0a0a0" }
        }));
    }
    serde_json::to_string(&elements).map_err(|e| MuseError::Tool(format!("mermaid serialize: {e}")))
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
            let abs = resolve_path(cwd, &path)?;
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
            let abs = resolve_path(cwd, &output)?;
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

    /// A1/A2/A3/A4/A7 — browser share URL only; never fall back to local .excalidraw.
    #[test]
    fn open_policy_browser_url_only() {
        let with_url = "Uploading…\nhttps://excalidraw.com/#json=abc123,keyXYZ\ndone\n";
        let no_url = "export finished with no share link\n";

        // open=true + URL → browser URL
        assert_eq!(
            browser_url_to_open(true, with_url).as_deref(),
            Some("https://excalidraw.com/#json=abc123,keyXYZ")
        );
        // open=true + no URL → nothing (do NOT open local file)
        assert!(browser_url_to_open(true, no_url).is_none());
        // open=false + URL → nothing
        assert!(browser_url_to_open(false, with_url).is_none());
        // open=false + no URL → nothing
        assert!(browser_url_to_open(false, no_url).is_none());
    }

    #[test]
    fn tool_schema_open_is_browser_only() {
        let tool = Excalidraw;
        let schema = tool.parameters_schema();
        let open_desc = schema["properties"]["open"]["description"]
            .as_str()
            .unwrap_or("");
        assert!(
            open_desc.to_lowercase().contains("browser"),
            "open schema should mention browser: {open_desc}"
        );
        assert!(
            !open_desc.to_lowercase().contains("and local file"),
            "open schema must not claim local-file open: {open_desc}"
        );
        let desc = tool.description().to_lowercase();
        assert!(
            desc.contains("browser") && desc.contains("never"),
            "tool description should state browser-only / never local open"
        );
    }

    /// Guard: production code must not call open_path (source-level policy lock).
    #[test]
    fn no_open_path_call_sites_in_excalidraw() {
        let src = include_str!("excalidraw.rs");
        // Strip the tests module so this guard's own prose does not match.
        let prod = src
            .split("#[cfg(test)]")
            .next()
            .unwrap_or(src);
        // Actual call form only — comments may still mention the forbidden API.
        assert!(
            !prod.contains("open_uri::open_path(") && !prod.contains("open_path(&"),
            "excalidraw production code must not call open_path — browser share URL only"
        );
    }
}

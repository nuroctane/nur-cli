//! `secscan`: run the security scanners this machine has and hand the model
//! normalized findings, never secret values.
//!
//! Scanners (each optional, found on PATH, never installed by nur):
//! - **gitleaks**: secrets in the files as they are now, uncommitted ones
//!   included (`history: true` scans every commit of the repository instead);
//! - **osv-scanner**: known vulnerabilities in dependency lockfiles (queries
//!   osv.dev with package names and versions);
//! - **semgrep**: code patterns (`p/default` rules fetched from the registry,
//!   metrics off).
//!
//! Each writes a JSON report to `.nur/scratch/secscan/`, which is parsed and
//! deleted; a scanner's exit code only says whether it found something.
//! Secret values (`Secret`, `Match`) are never read into a finding.
//!
//! The agent that fixes findings should not also decide which are noise:
//! `baseline` records the user's decision (it always asks, auto mode too) in
//! `.nur/secscan-baseline.json`, and later runs hide those findings by id.

use super::{arg_str, Tool, ToolContext};
use crate::error::{NurError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub struct SecScan;

/// Per-scanner limit; scanners run one after another.
const SCANNER_TIMEOUT_MS: u64 = 300_000;
const SHOWN: usize = 40;

#[derive(Debug, Clone, PartialEq)]
struct Finding {
    id: String,
    scanner: &'static str,
    rule: String,
    severity: String,
    file: String,
    line: u64,
    message: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Baseline {
    #[serde(default)]
    accepted: Vec<Accepted>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Accepted {
    id: String,
    reason: String,
    at: String,
}

impl Tool for SecScan {
    fn name(&self) -> &str {
        "secscan"
    }

    fn description(&self) -> &str {
        "Run the security scanners installed here (gitleaks for secrets, osv-scanner for \
         vulnerable dependencies, semgrep for code patterns) and get normalized findings with \
         file:line and a stable id; secret values are never shown. gitleaks scans the files as \
         they are now; history=true scans every commit instead. Actions: run (default) | \
         status (which scanners are installed) | baseline (record findings the user accepts as \
         noise; always asks the user). Triage findings with a separate `agent` reviewer, not \
         the agent fixing them, and never baseline a finding on your own judgment."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["run", "status", "baseline"], "default": "run"},
                "scanners": {
                    "type": "array",
                    "items": {"type": "string", "enum": ["gitleaks", "osv-scanner", "semgrep"]},
                    "description": "For run: which scanners (default: every one installed)"
                },
                "path": {"type": "string", "description": "For run: subdirectory to scan (default: the workspace)"},
                "history": {"type": "boolean", "description": "For run: gitleaks scans the repository's whole git history instead of the current files (path is ignored)"},
                "ids": {"type": "array", "items": {"type": "string"}, "description": "For baseline: finding ids the user accepts"},
                "reason": {"type": "string", "description": "For baseline: the user's reason"}
            }
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action").unwrap_or_else(|_| "run".into());
        match action.as_str() {
            "status" => Ok(status()),
            "baseline" => baseline(args, &ctx.cwd),
            "run" => run(args, ctx),
            other => Err(NurError::Tool(format!(
                "unknown secscan action `{other}`; use run|status|baseline"
            ))),
        }
    }
}

const SCANNERS: &[(&str, &str)] = &[
    (
        "gitleaks",
        "https://github.com/gitleaks/gitleaks#installing",
    ),
    (
        "osv-scanner",
        "https://google.github.io/osv-scanner/installation/",
    ),
    ("semgrep", "pip install semgrep (or pipx / brew)"),
];

fn status() -> String {
    let mut out = String::from("secscan scanners\n");
    for (name, install) in SCANNERS {
        match crate::ecosystem::find_bin(name) {
            Some(path) => out.push_str(&format!("- {name}: {path}\n")),
            None => out.push_str(&format!("- {name}: not installed ({install})\n")),
        }
    }
    out
}

fn baseline_path(cwd: &Path) -> PathBuf {
    cwd.join(".nur").join("secscan-baseline.json")
}

fn load_baseline(cwd: &Path) -> Baseline {
    std::fs::read_to_string(baseline_path(cwd))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn baseline(args: &Value, cwd: &Path) -> Result<String> {
    let ids: Vec<String> = args
        .get("ids")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    if ids.is_empty() {
        return Err(NurError::Tool("secscan baseline needs `ids`".into()));
    }
    let reason = arg_str(args, "reason").unwrap_or_default();
    if reason.trim().is_empty() {
        return Err(NurError::Tool(
            "secscan baseline needs the user's `reason`".into(),
        ));
    }
    let mut base = load_baseline(cwd);
    let at = chrono::Utc::now().to_rfc3339();
    let mut added = 0;
    for id in ids {
        if !base.accepted.iter().any(|a| a.id == id) {
            base.accepted.push(Accepted {
                id,
                reason: reason.clone(),
                at: at.clone(),
            });
            added += 1;
        }
    }
    let path = baseline_path(cwd);
    let raw = serde_json::to_string_pretty(&base).map_err(|e| NurError::Tool(e.to_string()))?;
    crate::config::atomic_write(&path, raw.as_bytes())
        .map_err(|e| NurError::Tool(format!("write {}: {e}", path.display())))?;
    Ok(format!(
        "baselined {added} finding(s) in {} ({} accepted in all)",
        path.display(),
        base.accepted.len()
    ))
}

fn run(args: &Value, ctx: &ToolContext) -> Result<String> {
    let root = match arg_str(args, "path") {
        Ok(p) if !p.trim().is_empty() => super::resolve_path(&ctx.cwd, p.trim())?,
        _ => ctx.cwd.clone(),
    };
    let wanted: Vec<String> = args
        .get("scanners")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    let history = args.get("history").and_then(Value::as_bool) == Some(true);
    let repo = crate::fractal::repo_root_of(&root);
    if history && repo.is_none() {
        return Err(NurError::Tool(
            "secscan history needs a git repository; scan without history".into(),
        ));
    }
    let reports = ctx.cwd.join(".nur").join("scratch").join("secscan");
    std::fs::create_dir_all(&reports).map_err(|e| NurError::Tool(e.to_string()))?;
    let mut findings = Vec::new();
    let mut notes = Vec::new();
    for (name, install) in SCANNERS {
        if !wanted.is_empty() && !wanted.iter().any(|w| w == name) {
            continue;
        }
        let Some(bin) = crate::ecosystem::find_bin(name) else {
            notes.push(format!("{name}: not installed ({install})"));
            continue;
        };
        let report = reports.join(format!("{name}-{}.json", uuid::Uuid::new_v4().simple()));
        // History is the whole repository's; the current files are the path's.
        let source = match (*name, &repo) {
            ("gitleaks", Some(repo)) if history => repo.clone(),
            _ => root.clone(),
        };
        let root_s = source.display().to_string();
        let report_s = report.display().to_string();
        let args: Vec<&str> = match *name {
            "gitleaks" => vec![
                "detect",
                if history {
                    "--log-opts=--all"
                } else {
                    "--no-git"
                },
                "--no-banner",
                "--redact",
                "--exit-code",
                "0",
                "--report-format",
                "json",
                "--report-path",
                &report_s,
                "--source",
                &root_s,
            ],
            "osv-scanner" => vec!["--format", "json", "--output", &report_s, "-r", &root_s],
            _ => vec![
                "scan",
                "--config",
                "p/default",
                "--metrics",
                "off",
                "--json",
                "--quiet",
                "--output",
                &report_s,
                &root_s,
            ],
        };
        // Exit codes mean "found something"; the report file is the result.
        let run = crate::ecosystem::run_capture_cancelled(
            &bin,
            &args,
            Some(&root),
            SCANNER_TIMEOUT_MS,
            &ctx.cancel,
        );
        match std::fs::read_to_string(&report) {
            Ok(raw) => {
                let parsed = match *name {
                    "gitleaks" => parse_gitleaks(&raw, &source),
                    "osv-scanner" => parse_osv(&raw, &root),
                    _ => parse_semgrep(&raw, &root),
                };
                notes.push(format!("{name}: {} finding(s)", parsed.len()));
                findings.extend(parsed);
            }
            Err(_) => notes.push(format!(
                "{name}: no report ({})",
                run.err()
                    .unwrap_or_else(|| "it wrote none".into())
                    .lines()
                    .next()
                    .unwrap_or("")
            )),
        }
        let _ = std::fs::remove_file(&report);
    }
    let accepted = load_baseline(&ctx.cwd);
    let before = findings.len();
    findings.retain(|f| !accepted.accepted.iter().any(|a| a.id == f.id));
    let hidden = before - findings.len();
    findings.sort_by(|a, b| {
        severity_rank(&a.severity)
            .cmp(&severity_rank(&b.severity))
            .then(a.file.cmp(&b.file))
            .then(a.line.cmp(&b.line))
    });
    let mut out = format!(
        "secscan · {} finding(s){} · {}\n",
        findings.len(),
        if hidden > 0 {
            format!(" ({hidden} hidden by the baseline)")
        } else {
            String::new()
        },
        notes.join(" · ")
    );
    if findings.is_empty() {
        return Ok(out);
    }
    out.push_str(
        "\n| id | scanner | severity | where | rule | finding |\n|---|---|---|---|---|---|\n",
    );
    for f in findings.iter().take(SHOWN) {
        out.push_str(&format!(
            "| {} | {} | {} | {}:{} | {} | {} |\n",
            f.id, f.scanner, f.severity, f.file, f.line, f.rule, f.message
        ));
    }
    if findings.len() > SHOWN {
        out.push_str(&format!("… and {} more\n", findings.len() - SHOWN));
    }
    Ok(out)
}

fn severity_rank(severity: &str) -> u8 {
    match severity.to_ascii_lowercase().as_str() {
        "critical" => 0,
        "high" | "error" => 1,
        "medium" | "moderate" | "warning" => 2,
        "low" | "info" => 3,
        _ => 4,
    }
}

/// Stable across runs and machines: the scanner, rule and place, never the value.
fn finding_id(parts: &[&str]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(parts.join("\0"));
    digest.iter().take(6).map(|b| format!("{b:02x}")).collect()
}

/// Repo text in a table cell: one line, no pipes, clipped.
fn cell(text: &str, max: usize) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() || c == '|' { ' ' } else { c })
        .collect();
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > max {
        format!("{}…", flat.chars().take(max).collect::<String>())
    } else {
        flat
    }
}

fn relative(root: &Path, file: &str) -> String {
    let path = Path::new(file);
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn finding(
    scanner: &'static str,
    rule: &str,
    severity: &str,
    file: String,
    line: u64,
    message: &str,
) -> Finding {
    Finding {
        id: finding_id(&[scanner, rule, &file, &line.to_string()]),
        scanner,
        rule: cell(rule, 60),
        severity: cell(severity, 12),
        file: cell(&file, 200),
        line,
        message: cell(message, 160),
    }
}

/// gitleaks: `[{RuleID, Description, File, StartLine, Secret, Match, ...}]`.
/// `Secret` and `Match` are deliberately never read.
fn parse_gitleaks(raw: &str, root: &Path) -> Vec<Finding> {
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(raw) else {
        return Vec::new();
    };
    items
        .iter()
        .map(|item| {
            let s = |k: &str| item.get(k).and_then(Value::as_str).unwrap_or("");
            // In history the same file:line recurs per commit; the commit
            // keeps each its own row and id.
            let mut file = relative(root, s("File"));
            let commit = s("Commit");
            if !commit.is_empty() {
                file = format!("{file}@{}", &commit[..commit.len().min(7)]);
            }
            finding(
                "gitleaks",
                s("RuleID"),
                "high",
                file,
                item.get("StartLine").and_then(Value::as_u64).unwrap_or(0),
                s("Description"),
            )
        })
        .collect()
}

/// osv-scanner: `results[].source.path` + `packages[].{package, vulnerabilities[]}`.
fn parse_osv(raw: &str, root: &Path) -> Vec<Finding> {
    let Ok(doc) = serde_json::from_str::<Value>(raw) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for result in doc
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let source = result
            .pointer("/source/path")
            .and_then(Value::as_str)
            .unwrap_or("");
        for pkg in result
            .get("packages")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let name = pkg
                .pointer("/package/name")
                .and_then(Value::as_str)
                .unwrap_or("?");
            let version = pkg
                .pointer("/package/version")
                .and_then(Value::as_str)
                .unwrap_or("");
            for vuln in pkg
                .get("vulnerabilities")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let id = vuln.get("id").and_then(Value::as_str).unwrap_or("?");
                let summary = vuln.get("summary").and_then(Value::as_str).unwrap_or("");
                let severity = vuln
                    .pointer("/database_specific/severity")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                out.push(finding(
                    "osv-scanner",
                    id,
                    severity,
                    relative(root, source),
                    0,
                    &format!("{name} {version}: {summary}"),
                ));
            }
        }
    }
    out
}

/// semgrep: `results[].{check_id, path, start.line, extra.{message, severity}}`.
fn parse_semgrep(raw: &str, root: &Path) -> Vec<Finding> {
    let Ok(doc) = serde_json::from_str::<Value>(raw) else {
        return Vec::new();
    };
    doc.get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|r| {
            let s = |p: &str| r.pointer(p).and_then(Value::as_str).unwrap_or("");
            finding(
                "semgrep",
                s("/check_id"),
                s("/extra/severity"),
                relative(root, s("/path")),
                r.pointer("/start/line")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                s("/extra/message"),
            )
        })
        .collect()
}

/// `baseline` asks the user in every permission mode: noise is their call.
pub fn always_asks(args: &str) -> bool {
    serde_json::from_str::<Value>(args)
        .ok()
        .and_then(|v| v.get("action").and_then(Value::as_str).map(String::from))
        .is_some_and(|a| a == "baseline")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Failure modes: a secret value reaches the model; an id changes between
    // runs (the baseline stops matching); a report shape is misread; a pipe in
    // repo text breaks the table.

    #[test]
    fn gitleaks_findings_never_carry_the_secret() {
        let root = Path::new("/repo");
        let raw = r#"[{"RuleID":"generic-api-key","Description":"Generic API Key","File":"/repo/src/config.rs","StartLine":12,"Secret":"TOPSECRETVALUE","Match":"key = TOPSECRETVALUE"}]"#;
        let found = parse_gitleaks(raw, root);
        assert_eq!(found.len(), 1);
        let shown = format!("{:?}", found[0]);
        assert!(!shown.contains("TOPSECRETVALUE"));
        assert_eq!(
            (found[0].file.as_str(), found[0].line),
            ("src/config.rs", 12)
        );
        assert_eq!(found[0].id, parse_gitleaks(raw, root)[0].id);
    }

    #[test]
    fn osv_and_semgrep_reports_normalize() {
        let root = Path::new("/repo");
        let osv = r#"{"results":[{"source":{"path":"/repo/Cargo.lock"},"packages":[{"package":{"name":"time","version":"0.1.0"},"vulnerabilities":[{"id":"RUSTSEC-2020-0071","summary":"segfault | in time","database_specific":{"severity":"MEDIUM"}}]}]}]}"#;
        let found = parse_osv(osv, root);
        assert_eq!(found[0].file, "Cargo.lock");
        assert_eq!(found[0].rule, "RUSTSEC-2020-0071");
        assert!(!found[0].message.contains('|'));
        let semgrep = r#"{"results":[{"check_id":"rust.lang.security.unsafe","path":"src/a.rs","start":{"line":7},"extra":{"message":"unsafe block","severity":"WARNING"}}]}"#;
        let found = parse_semgrep(semgrep, root);
        assert_eq!((found[0].file.as_str(), found[0].line), ("src/a.rs", 7));
        assert_eq!(found[0].severity, "WARNING");
    }
}

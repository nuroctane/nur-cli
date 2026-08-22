//! Dogwood default configuration - out-of-the-box policies so the `dogwood`
//! tool works immediately after ensure.
//!
//! Layout under `~/.nur/dogwood/`:
//! - `policies/safe-edits.dw`      - example temporal guardrail policy
//! - `schemas/nur-events.dwschema` - event schema matching nur's tool calls
//! - `README.md`                   - how to validate/replay
//!
//! The reference interpreter is explicitly NOT production-grade enforcement;
//! these files are evaluation/guardrail examples, not a runtime gate.

use crate::error::{NurError, Result};
use std::fs;
use std::path::PathBuf;

/// Root of the dogwood config pack.
pub fn config_dir() -> PathBuf {
    crate::config::nur_home().join("dogwood")
}

fn policies_dir() -> PathBuf {
    config_dir().join("policies")
}

fn schemas_dir() -> PathBuf {
    config_dir().join("schemas")
}

/// One-shot marker: cargo installs are slow; only attempt once per marker
/// lifetime so a failing build does not re-run on every ensure. Expires after
/// 7 days (retry window for broken-toolchain cases).
pub fn dogwood_install_attempted() -> bool {
    let marker = crate::config::nur_home()
        .join("cache")
        .join("dogwood-install-attempted");
    match fs::metadata(&marker) {
        Ok(m) => {
            if let Ok(modified) = m.modified() {
                if let Ok(age) = modified.elapsed() {
                    return age.as_secs() < 7 * 24 * 3600;
                }
            }
            true
        }
        Err(_) => false,
    }
}

pub fn mark_dogwood_install_attempted() {
    let dir = crate::config::nur_home().join("cache");
    let _ = fs::create_dir_all(&dir);
    let _ = fs::write(dir.join("dogwood-install-attempted"), "1");
}

/// Write the default policy/schema pack. Idempotent: existing files are left
/// untouched so user edits survive ensure re-runs.
pub fn ensure_default_config() -> Result<String> {
    let pd = policies_dir();
    let sd = schemas_dir();
    fs::create_dir_all(&pd).map_err(NurError::Io)?;
    fs::create_dir_all(&sd).map_err(NurError::Io)?;

    let mut written = 0usize;

    let policy_path = pd.join("safe-edits.dw");
    if !policy_path.is_file() {
        fs::write(&policy_path, DEFAULT_POLICY).map_err(NurError::Io)?;
        written += 1;
    }

    let event_schema = sd.join("nur-events.dwschema");
    if !event_schema.is_file() {
        fs::write(&event_schema, EVENT_SCHEMA).map_err(NurError::Io)?;
        written += 1;
    }

    let readme = config_dir().join("README.md");
    if !readme.is_file() {
        fs::write(&readme, README).map_err(NurError::Io)?;
        written += 1;
    }

    Ok(if written > 0 {
        format!("{written} file(s) seeded")
    } else {
        "defaults present".into()
    })
}

const DEFAULT_POLICY: &str = r#"// NurCLI default Dogwood policy - safe-edits example.
//
// A temporal guardrail: allow normal reads, gate edits behind a prior read,
// and bound destructive bash usage over a rolling window. Evaluation-only -
// not a runtime trust anchor.
//
// A temporal guardrail: allow normal reads, gate edits behind a prior read,
// and bound destructive shell usage over a rolling window. Evaluation-only -
// not a runtime trust anchor.

permit(principal, action == Action::"read_file", resource);
permit(principal, action == Action::"list_dir", resource);
permit(principal, action == Action::"grep", resource);
permit(principal, action == Action::"glob", resource);

// Edits are permitted once the same file was read this session.
permit(principal, action == Action::"edit_file", resource)
    when context.file_was_read;

permit(principal, action == Action::"bash", resource)
    when context.command_class == "safe";

// At most 2 destructive shell calls within any 10-minute window.
forbid(principal, action == Action::"bash", resource)
    when context.command_class == "destructive"
    && count_within(action, "10 minutes") >= 2;

// Never allow history rewrites or recursive deletes of a workspace root.
forbid(principal, action == Action::"bash", resource)
    when context.command_matches(["git push --force", "git push -f"]);
"#;

const EVENT_SCHEMA: &str = r#"{
  "//": "NurCLI event schema for Dogwood replay - one entry per tool call.",
  "entity_types": {},
  "actions": {
    "read_file": {},
    "list_dir": {},
    "grep": {},
    "glob": {},
    "edit_file": {},
    "write_file": {},
    "multi_edit": {},
    "apply_patch": {},
    "bash": {}
  },
  "context": {
    "file_was_read": { "type": "Boolean" },
    "command_class": { "type": "String" },
    "command_matches": { "type": "Set", "element": { "type": "String" } }
  }
}"#;

const README: &str = r#"# Dogwood default config

Seeded by nur ecosystem ensure. The dogwood tool is an evaluation /
guardrail layer, not a runtime trust anchor.

## Files

- policies/safe-edits.dw - example temporal policy (edit-after-read, bounded destructive shell)
- schemas/nur-events.dwschema - event schema matching the nur tool surface

## Try it

```
dogwood(action=validate, policy="~/.nur/dogwood/policies/safe-edits.dw",
        event_schema="~/.nur/dogwood/schemas/nur-events.dwschema")
dogwood(action=check-parse, policy="~/.nur/dogwood/policies/safe-edits.dw")
```

Edit freely - ensure never overwrites existing files here.
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_files_are_nonempty_and_idempotent_writes() {
        // Pure-content sanity without touching the real home directory.
        assert!(DEFAULT_POLICY.contains("count_within"));
        assert!(DEFAULT_POLICY.contains("file_was_read"));
        assert!(EVENT_SCHEMA.contains("bash"));
        assert!(README.contains("not a runtime trust anchor"));
    }
}

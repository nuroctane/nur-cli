use super::{arg_str, Tool, ToolContext};
use crate::error::Result;
use serde_json::Value;

/// t3code driver probe tool — exposes t3code-style auth probing for coding agents.
pub fn is_read_only_action(args_json: &str) -> bool {
    // pairing_create creates token (mutating), others are read-only probes
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(args_json) {
        if let Some(action) = v.get("action").and_then(|a| a.as_str()) {
            return matches!(action, "status" | "probe" | "probe_all" | "delegate" | "env");
        }
    }
    true
}

pub struct T3Code;

impl Tool for T3Code {
    fn name(&self) -> &str {
        "t3code"
    }

    fn description(&self) -> &str {
        "t3code integration — probe vendor CLI auth (Claude, Codex, Cursor, OpenCode, Grok) with env isolation, delegate checks, atomic writes, and pairing tokens. Actions: status|probe|probe_all|delegate|pairing_create|env. Mirrors https://github.com/pingdotgg/t3code driver pattern. Use for evaluating auth improvements and bridging."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "description": "status|probe|probe_all|delegate|pairing_create|env", "default": "status"},
                "driver": {"type": "string", "description": "claude|codex|cursor|opencode|grok", "default": "claude"},
                "label": {"type": "string", "description": "pairing token label"},
                "ttl": {"type": "string", "description": "pairing TTL like 5m, 1h, 30d", "default": "5m"}
            },
            "required": []
        })
    }

    fn execute(&self, args: &Value, _ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action").unwrap_or_else(|_| "status".into());
        let driver_s = arg_str(args, "driver").unwrap_or_else(|_| "claude".into());
        let label = arg_str(args, "label").unwrap_or_else(|_| "nur-remote".into());
        let ttl = arg_str(args, "ttl").unwrap_or_else(|_| "5m".into());

        let driver = match driver_s.to_ascii_lowercase().as_str() {
            "codex" => crate::t3code::DriverId::Codex,
            "cursor" | "cursor-agent" => crate::t3code::DriverId::Cursor,
            "opencode" => crate::t3code::DriverId::OpenCode,
            "grok" | "xai" => crate::t3code::DriverId::Grok,
            _ => crate::t3code::DriverId::Claude,
        };

        match action.as_str() {
            "probe" => {
                let st = crate::t3code::probe_driver(driver);
                Ok(format!(
                    "driver={} binary_present={} config_dir={} exists={} has_credentials={} hint={}\n{}\n",
                    st.driver.as_str(),
                    st.binary_present,
                    st.config_dir.display(),
                    st.config_dir_exists,
                    st.has_credentials,
                    st.hint,
                    if st.has_credentials {
                        "✓ delegate auth available — no secret storage needed (t3code pattern)"
                    } else {
                        "✗ no credentials — run hint command first"
                    }
                ))
            }
            "probe_all" => {
                let all = crate::t3code::probe_all();
                let mut out = String::new();
                out.push_str("t3code driver probe — BYO-auth (zero secret storage):\n");
                for st in all {
                    out.push_str(&format!(
                        "- {}: bin={} cfg={} exists={} creds={} hint={}\n",
                        st.driver.as_str(),
                        st.binary_present,
                        st.config_dir.display(),
                        st.config_dir_exists,
                        st.has_credentials,
                        st.hint
                    ));
                }
                Ok(out)
            }
            "delegate" => match crate::t3code::delegate_check(driver) {
                Ok(()) => Ok(format!(
                    "delegate check passed for {} — vendor CLI auth exists, no need to store token (t3code pattern)",
                    driver.as_str()
                )),
                Err(e) => Ok(format!("delegate check failed: {e}")),
            },
            "pairing_create" => {
                let tok = crate::t3code::create_pairing_token(
                    &label,
                    &ttl,
                    vec!["standard".into(), "admin".into()],
                );
                Ok(format!(
                    "pairing token created (one-time, TTL {} = {}s, label={}):\n token={}\n link={}\n expired={}\nScopes: {:?}\n",
                    ttl,
                    tok.ttl_secs,
                    tok.label,
                    tok.token,
                    tok.to_pairing_link("http://localhost:3000"),
                    tok.is_expired(),
                    tok.scopes
                ))
            }
            "env" => {
                let env = crate::t3code::env_for_driver(driver);
                let mut out = format!("env isolation for {} (mirrors t3code CLAUDE_CONFIG_DIR/CODEX_HOME fix):\n", driver.as_str());
                for (k, v) in env {
                    out.push_str(&format!("{k}={v}\n"));
                }
                Ok(out)
            }
            _ => {
                // status = probe_all + summary
                let all = crate::t3code::probe_all();
                let mut out = String::new();
                out.push_str("t3code integration status — driver probing with env isolation + atomic writes + delegate mode:\n");
                out.push_str("Repo: https://github.com/pingdotgg/t3code — minimal GUI for coding agents, delegates auth to vendor CLI, pairing + DPoP for own server.\n\n");
                for st in &all {
                    out.push_str(&format!(
                        "- {}: creds={} bin={} cfg={}\n",
                        st.driver.as_str(),
                        st.has_credentials,
                        st.binary_present,
                        st.config_dir.display()
                    ));
                }
                out.push_str("\nWhat nur-cli improves from t3code:\n");
                out.push_str("- Import-first probing (already have import_existing_session, now adds Cursor/OpenCode)\n");
                out.push_str("- Env isolation per driver (CLAUDE_CONFIG_DIR, CODEX_HOME, etc.) to preserve macOS keychain\n");
                out.push_str("- Atomic writes for auth.json (like t3code atomicWrite.ts)\n");
                out.push_str("- Delegate mode -- no secret storage, verify vendor CLI exists\n");
                out.push_str("- Pairing tokens for remote TUI (one-time, TTL, scopes)\n");
                out.push_str("- Driver registry pattern for future refactor\n");
                out.push_str("\nActions: probe, probe_all, delegate, pairing_create, env\n");
                Ok(out)
            }
        }
    }
}

//! Effective configuration display with credential-bearing values removed.
use crate::config::Config;
use crate::error::{NurError, Result};
use serde_json::Value;

pub fn redacted_dump(config: &Config) -> Result<String> {
    let mut value = serde_json::to_value(config).map_err(|e| NurError::Config(e.to_string()))?;
    if let Some(servers) = value
        .pointer_mut("/mcp/servers")
        .and_then(Value::as_object_mut)
    {
        for server in servers.values_mut() {
            if let Some(args) = server.get_mut("args").and_then(Value::as_array_mut) {
                for arg in args {
                    *arg = Value::String("[redacted]".into());
                }
            }
            if let Some(env) = server.get_mut("env").and_then(Value::as_object_mut) {
                for item in env.values_mut() {
                    *item = Value::String("[redacted]".into());
                }
            }
        }
    }
    if let Some(servers) = value
        .pointer_mut("/lsp/servers")
        .and_then(Value::as_object_mut)
    {
        for command in servers.values_mut() {
            if command.as_str().is_some_and(|s| !s.is_empty()) {
                *command = Value::String("[redacted]".into());
            }
        }
    }
    scrub(&mut value, "");
    serde_json::to_string_pretty(&value).map_err(|e| NurError::Config(e.to_string()))
}

fn scrub(value: &mut Value, key: &str) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                scrub(value, key);
            }
        }
        Value::Array(items) => {
            for item in items {
                scrub(item, key);
            }
        }
        Value::String(text) => {
            if matches!(
                key.to_ascii_lowercase().as_str(),
                "api_key" | "password" | "secret" | "token" | "authorization"
            ) && !text.is_empty()
            {
                *text = "[redacted]".into();
                return;
            }
            if let Ok(mut url) = reqwest::Url::parse(text) {
                if matches!(url.scheme(), "http" | "https")
                    && (!url.username().is_empty()
                        || url.password().is_some()
                        || url.query().is_some()
                        || url.fragment().is_some())
                {
                    let _ = url.set_username("");
                    let _ = url.set_password(None);
                    url.set_query(None);
                    url.set_fragment(None);
                    *text = format!("{url}?[redacted]");
                }
            }
            *text = crate::secrets::scrub(text);
        }
        _ => {}
    }
}

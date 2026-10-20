//! Pinned JevBench roster and local adapter selection. Discovery never networks.
use crate::error::{NurError, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Engine {
    pub id: String,
    pub name: String,
    pub source: String,
    pub protocol: String,
    pub adapter: String,
    pub license: Option<String>,
    pub open: Option<serde_json::Value>,
    pub hardware: Option<String>,
    pub support: Option<serde_json::Value>,
    pub status: Option<serde_json::Value>,
    pub rank: Option<u64>,
    pub model: Option<String>,
    pub endpoint: Option<String>,
    pub local_adapter: Option<String>,
    pub options: Option<serde_json::Value>,
    pub evidence: Option<String>,
    pub notes: Option<String>,
    /// Environment variable the hosting gateway reads its key from
    /// (`OPENROUTER_API_KEY` for OpenRouter-hosted engines, `SUBQ_API_KEY`).
    #[serde(default)]
    pub key_env: Option<String>,
    /// Credential-store slot shared with the chat provider of the same gateway,
    /// so one `nur auth login --provider openrouter` serves both.
    #[serde(default)]
    pub credential_provider: Option<String>,
    /// The hosted endpoint accepts anonymous calls (AMOS checks no key), so a
    /// selected engine works with nothing stored. A saved key is still sent.
    #[serde(default)]
    pub key_optional: bool,
}

impl Engine {
    /// Hosted engines that speak the System One schema at a public URL and can
    /// be selected with `nur jev use --engine <id>` without a bridge.
    pub fn is_hosted_systemone(&self) -> bool {
        self.protocol == "systemone"
            && self
                .endpoint
                .as_deref()
                .is_some_and(|e| e.starts_with("https://"))
    }
}

/// The catalog engine behind a `jev:<id>` credential slot, when the slot names
/// one. Returns `None` for gateway slots (`openrouter`) and unknown ids.
pub fn slot_engine(slot: &str) -> Option<&'static Engine> {
    slot.strip_prefix("jev:").and_then(|id| find(id).ok())
}

pub fn engines() -> &'static [Engine] {
    static ENGINES: std::sync::OnceLock<Vec<Engine>> = std::sync::OnceLock::new();
    ENGINES.get_or_init(|| {
        #[derive(Deserialize)]
        struct Catalog {
            engines: Vec<Engine>,
        }
        serde_json::from_str::<Catalog>(include_str!("jev_catalog.json"))
            .expect("generated Jev catalog must be valid")
            .engines
    })
}

pub fn find(id: &str) -> Result<&'static Engine> {
    engines()
        .iter()
        .find(|e| e.id.eq_ignore_ascii_case(id))
        .ok_or_else(|| NurError::Other(format!("unknown Jev engine {id:?}; use `nur jev models`")))
}

pub fn list(search: Option<&str>, json: bool) -> String {
    let query = search.unwrap_or("").to_ascii_lowercase();
    let rows: Vec<_> = engines()
        .iter()
        .filter(|e| {
            query.is_empty()
                || format!("{} {} {}", e.id, e.name, e.protocol)
                    .to_ascii_lowercase()
                    .contains(&query)
        })
        .collect();
    if json {
        return serde_json::to_string_pretty(&rows).unwrap_or_default();
    }
    let unranked = engines().iter().filter(|e| e.rank.is_none()).count();
    let mut text = format!(
        "Jev library - {} of {} engines (JevBench v1.5.4 roster and newer reviewed releases; {unranked} not yet ranked)\n",
        rows.len(),
        engines().len()
    );
    for e in rows {
        text.push_str(&format!("  {:<44} {:<16} {}\n", e.id, e.protocol, e.name));
    }
    text.push_str("\nnur jev info <id> shows source, interface, hardware, and connection steps.");
    text
}

pub fn local_adapter(id: &str) -> Option<&'static str> {
    find(id).ok()?.local_adapter.as_deref()
}

pub fn info(id: &str) -> Result<String> {
    let e = find(id)?;
    let mut output = format!(
        "{}\n  id: {}\n  source: {}\n  interface: {}\n  license: {}\n  hardware: {}\n",
        e.name,
        e.id,
        e.source,
        e.protocol,
        e.license.as_deref().unwrap_or("review source terms"),
        e.hardware
            .as_deref()
            .unwrap_or("see source deployment instructions")
    );
    if let Some(model) = &e.model {
        output.push_str(&format!("  model: {model}\n"));
    }
    if let Some(endpoint) = &e.endpoint {
        output.push_str(&format!("  public endpoint: {endpoint}\n"));
    }
    if let Some(notes) = &e.notes {
        output.push_str(&format!("  deployment: {notes}\n"));
    }
    if let Some(options) = &e.options {
        output.push_str(&format!("  default options: {options}\n"));
    }
    if local_adapter(id).is_some() {
        output.push_str(&format!("\nLocal: install the source project's dependencies in your Python environment, then:\n  nur jev start --engine {id}{}\n  nur jev use\n",if e.model.is_none() {" --model <weights-or-model-id>"} else {""}));
    }
    if e.is_hosted_systemone() {
        let login = match (e.key_optional, &e.credential_provider, &e.key_env) {
            (true, _, _) => "  (no key needed: the endpoint checks none; a key saved with `nur jev login` would still be sent)".to_string(),
            (false, Some(slot), Some(env)) if crate::providers::by_id(slot).is_some() => format!(
                "  nur auth login --provider {slot}   (or export {env}; shared with the {slot} chat provider)"
            ),
            // A decision-only vendor (Liquid AI's d1 family): one slot, no chat route.
            (false, Some(slot), Some(env)) => format!(
                "  nur jev login {id}   (or export {env}; saved in the `{slot}` slot every {slot} engine shares)"
            ),
            (false, None, Some(env)) => format!("  nur jev login {id}   (or export {env})"),
            (false, _, None) => format!("  nur jev login {id}"),
        };
        output.push_str(&format!(
            "\nConnect its public endpoint (catalog URL and model):\n{login}\n  nur jev use --engine {id}\n"
        ));
    } else if e.protocol == "systemone" {
        output.push_str(&format!("\nConnect the source project's native server:\n  nur jev use --engine {id} --url <https-or-loopback-systemone-url> --model <server-model-id>\n"));
    } else if e.protocol != "adapter" {
        // A catalog endpoint + model is enough for the bridge to translate
        // the interface on its own; otherwise the user supplies both.
        let args = if e.endpoint.is_some() && e.model.is_some() {
            String::new()
        } else {
            " --upstream <https-or-loopback-url> --model <model-id>".to_string()
        };
        let key = match &e.key_env {
            Some(env) => format!("  (export {env}, or `nur jev login {id}` to store a key; a loopback --upstream needs none)\n"),
            None => String::new(),
        };
        output.push_str(&format!("\nConnect its API:\n{key}  nur jev start --engine {id}{args}\n  nur jev use\n"));
    } else {
        output.push_str(&format!("\nThis entry uses a model-specific local library. Its published source owns model loading.\nRun a TypeSafe-compatible server from that source, or connect its Python adapter:\n  nur jev start --engine {id} --adapter <module:Class> --adapter-dir <checkout> --model <weights>\n  nur jev use\nAdapters expose run(task) returning ok/probs over exact labels (JevBench interface).\n"));
    }
    if e.key_optional {
        output.push_str("\nCredentials: none required. Keys stay in Nur's credential store or the environment; reports never print them.\nBenchmark measurements do not guarantee this machine can load the model.");
    } else {
        output.push_str(&format!("\nCredentials: nur jev login {id} (hidden input), or --key-env <ENV_NAME>.\nKeys stay in Nur's credential store or the environment; reports never print them.\nBenchmark measurements do not guarantee this machine can load the model."));
    }
    Ok(output)
}

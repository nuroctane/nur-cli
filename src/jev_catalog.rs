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
    if e.protocol == "systemone" && e.endpoint.is_some() {
        output.push_str(&format!(
            "\nConnect its public endpoint (catalog URL and model):\n  nur jev login {id}\n  nur jev use --engine {id}\n"
        ));
    } else if e.protocol == "systemone" {
        output.push_str(&format!("\nConnect the source project's native server:\n  nur jev use --engine {id} --url <https-or-loopback-systemone-url> --model <server-model-id>\n"));
    } else if e.protocol != "adapter" {
        output.push_str(&format!("\nConnect its API:\n  nur jev start --engine {id} --upstream <https-or-loopback-url> --model <model-id>\n  nur jev use\n"));
    } else {
        output.push_str(&format!("\nThis entry uses a model-specific local library. Its published source owns model loading.\nRun a TypeSafe-compatible server from that source, or connect its Python adapter:\n  nur jev start --engine {id} --adapter <module:Class> --adapter-dir <checkout> --model <weights>\n  nur jev use\nAdapters expose run(task) returning ok/probs over exact labels (JevBench interface).\n"));
    }
    output.push_str(&format!("\nCredentials: nur jev login {id} (hidden input), or --key-env <ENV_NAME>.\nKeys stay in Nur's credential store or the environment; reports never print them.\nBenchmark measurements do not guarantee this machine can load the model."));
    Ok(output)
}

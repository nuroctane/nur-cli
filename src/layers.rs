//! Which optional side layers are live this session, and why not when they
//! are not. TypeSafe and Headroom are on by default but each needs something
//! the install may lack (a key, a Python package); without this line a
//! headless run silently got neither and nothing said so.

use crate::config::Config;

pub struct Layer {
    pub name: &'static str,
    pub active: bool,
    /// `active` or the reason it is not, with the command that turns it on.
    pub detail: String,
}

/// Probe every layer. Headroom's probe may launch Python (bounded to 5 s), so
/// call this off the request path: a background thread at session start.
pub fn probe(cfg: &Config) -> Vec<Layer> {
    vec![typesafe(cfg), headroom(cfg)]
}

fn typesafe(cfg: &Config) -> Layer {
    let (active, detail) = if !cfg.typesafe.enabled {
        (false, "off (typesafe.enabled = false)".to_string())
    } else if crate::typesafe::harness::available(&cfg.typesafe) {
        (true, "active".to_string())
    } else {
        (
            false,
            "off (no key: set TYPESAFE_API_KEY or run `nur auth login --provider typesafe`)"
                .to_string(),
        )
    };
    Layer {
        name: "typesafe",
        active,
        detail,
    }
}

fn headroom(cfg: &Config) -> Layer {
    let (active, detail) = if !cfg.headroom.enabled || cfg.headroom.mode == "off" {
        (false, "off (headroom.enabled = false)".to_string())
    } else if crate::headroom::python_import_ok() {
        (true, "active (inline compress)".to_string())
    } else {
        (
            false,
            "off (python package headroom-ai missing: `uv tool install headroom-ai`)".to_string(),
        )
    };
    Layer {
        name: "headroom",
        active,
        detail,
    }
}

pub fn status_line(layers: &[Layer]) -> String {
    let parts: Vec<String> = layers
        .iter()
        .map(|l| format!("{}: {}", l.name, l.detail))
        .collect();
    format!("layers · {}", parts.join(" · "))
}

pub fn to_json(layers: &[Layer]) -> serde_json::Value {
    serde_json::Value::Object(
        layers
            .iter()
            .map(|l| {
                (
                    l.name.to_string(),
                    serde_json::json!({"active": l.active, "detail": l.detail}),
                )
            })
            .collect(),
    )
}

/// Probe on a background thread and hand the result to `sink` once; the
/// first model request is never delayed by the Python probe.
pub fn announce(cfg: &Config, sink: impl FnOnce(Vec<Layer>) + Send + 'static) {
    let cfg = cfg.clone();
    std::thread::spawn(move || sink(probe(&cfg)));
}

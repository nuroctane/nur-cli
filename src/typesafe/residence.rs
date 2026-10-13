//! Where each Jev-class engine's credential lives, and which engine the
//! judgment layer is actually using right now.
//!
//! `client::api_key` / `client::fallback_engine` answer "give me a key"; this
//! module answers the user's question in `/auth`: *which engines can judge for
//! me, where does each key sit, and what will Delete remove?* It is read-only
//! except for [`remove`], which only ever touches state Nur itself owns (the
//! credential store, `~/.nur/*.key`, `[typesafe] api_key` in config.toml).
//! Environment variables and the running bridge are reported, never mutated.

use std::path::PathBuf;

use crate::config::TypesafeConfig;
use crate::error::Result;

use super::client;

/// A stable id for one engine slot (also the credential-store key for hosted
/// slots).
pub const TYPESAFE_SLOT: &str = "typesafe";
pub const BRIDGE_SLOT: &str = "bridge";
pub const SUBQ_SLOT: &str = "jev:subq-snap";
pub const OPENROUTER_SLOT: &str = "openrouter";

/// Where the credential for a slot sits, in the order the layer reads them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Residence {
    /// Nothing found anywhere Nur looks.
    Absent,
    /// Inline `[typesafe] api_key` in config.toml.
    Config,
    /// An environment variable (named).
    Env(String),
    /// Nur's per-provider credential store under this slot id.
    Store(String),
    /// A key file (TypeSafe's legacy locations).
    File(PathBuf),
    /// Loopback engine: no key, by construction.
    Loopback(String),
}

impl Residence {
    /// Short, non-secret label for a picker row.
    pub fn label(&self) -> String {
        match self {
            Residence::Absent => "no key".into(),
            Residence::Config => "config.toml".into(),
            Residence::Env(var) => format!("env {var}"),
            Residence::Store(_) => "saved key".into(),
            Residence::File(path) => format!("file {}", short_path(path)),
            Residence::Loopback(endpoint) => format!("loopback {}", port_of(endpoint)),
        }
    }

    /// Whether [`remove`] can clear this residence.
    pub fn removable(&self) -> bool {
        match self {
            Residence::Config | Residence::Store(_) => true,
            Residence::File(path) => path.starts_with(crate::config::nur_home()),
            Residence::Absent | Residence::Env(_) | Residence::Loopback(_) => false,
        }
    }

    /// What the user has to do when Nur cannot remove it.
    pub fn manual_hint(&self) -> Option<String> {
        match self {
            Residence::Env(var) => Some(format!("unset {var} to remove")),
            Residence::File(path) if !self.removable() => {
                Some(format!("delete {} yourself", path.display()))
            }
            Residence::Loopback(_) => Some("`nur jev stop` to remove".into()),
            _ => None,
        }
    }
}

/// One engine the judgment layer could run on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub id: String,
    pub name: String,
    /// Model / endpoint note shown under the name.
    pub detail: String,
    pub residence: Residence,
    /// The layer resolves to this slot right now.
    pub in_use: bool,
    /// Set when a saved key here would also serve a chat provider.
    pub shared_with: Option<&'static str>,
}

impl Slot {
    /// Hosted slots accept a key from `/auth`; the bridge does not.
    pub fn accepts_key(&self) -> bool {
        self.id != BRIDGE_SLOT
    }
}

/// Every Jev-class slot, hosted ones first in the fallback preference order,
/// with the bridge second so the two zero-config paths sit at the top.
pub fn slots(cfg: &TypesafeConfig) -> Vec<Slot> {
    let active = active_slot(cfg);
    let mut out = vec![
        typesafe_slot(cfg),
        bridge_slot(cfg),
        hosted_slot(
            SUBQ_SLOT,
            "SubQ Snap",
            "subq-snap-preview · api.subq.ai",
            &["SUBQ_API_KEY"],
            None,
        ),
        hosted_slot(
            OPENROUTER_SLOT,
            "OpenRouter Decisions",
            "mercury-decide · openrouter.ai",
            &["OPENROUTER_API_KEY"],
            Some("OpenRouter chat"),
        ),
    ];
    // A catalog engine picked with `nur jev use --engine <id>` owns its own
    // slot (`jev:<id>`) unless it rides a gateway key already listed above.
    let selected = cfg.credential_provider.trim();
    if !selected.is_empty() && out.iter().all(|s| s.id != selected) {
        let envs: Vec<String> = if cfg.key_env.trim().is_empty() {
            Vec::new()
        } else {
            vec![cfg.key_env.trim().to_string()]
        };
        let env_refs: Vec<&str> = envs.iter().map(String::as_str).collect();
        out.push(hosted_slot(
            selected,
            &format!("selected engine · {}", selected.trim_start_matches("jev:")),
            &engine_detail(cfg),
            &env_refs,
            None,
        ));
    } else if !cfg.key_env.trim().is_empty() && selected.is_empty() {
        // `--key-env` without a store slot: the engine is keyed purely by env.
        out.push(hosted_slot(
            &format!("env:{}", cfg.key_env.trim()),
            "selected engine",
            &engine_detail(cfg),
            &[cfg.key_env.trim()],
            None,
        ));
    }
    for slot in &mut out {
        slot.in_use = active.as_deref() == Some(slot.id.as_str());
    }
    out
}

/// The slot the layer resolves to, mirroring `client::api_key` /
/// `client::effective_base_url` without reading any key value.
fn active_slot(cfg: &TypesafeConfig) -> Option<String> {
    if !cfg.enabled {
        return None;
    }
    if client::local_bridge_env().is_some() {
        return Some(BRIDGE_SLOT.into());
    }
    let selected = cfg.credential_provider.trim();
    if !cfg.key_env.trim().is_empty() {
        let present = std::env::var(cfg.key_env.trim()).is_ok_and(|v| !v.trim().is_empty());
        return present.then(|| {
            if selected.is_empty() {
                format!("env:{}", cfg.key_env.trim())
            } else {
                selected.to_string()
            }
        });
    }
    if !selected.is_empty() {
        return crate::auth::load_provider_key(selected).map(|_| selected.to_string());
    }
    if client::is_loopback_endpoint(&client::effective_base_url(cfg)) {
        return Some(BRIDGE_SLOT.into());
    }
    if let Some(engine) = client::fallback_engine(cfg) {
        return Some(match engine.base_url.as_str() {
            url if url.contains("subq.ai") => SUBQ_SLOT.into(),
            url if url.contains("openrouter.ai") => OPENROUTER_SLOT.into(),
            _ => BRIDGE_SLOT.into(),
        });
    }
    (typesafe_residence(cfg) != Residence::Absent).then(|| TYPESAFE_SLOT.to_string())
}

fn typesafe_slot(cfg: &TypesafeConfig) -> Slot {
    let detail =
        if cfg.base_url.trim().is_empty() || client::is_loopback_endpoint(cfg.base_url.trim()) {
            "jev-latest · api.typesafe.ai".to_string()
        } else {
            format!(
                "{} · {}",
                if cfg.model.trim().is_empty() {
                    "jev-latest"
                } else {
                    cfg.model.trim()
                },
                host_of(cfg.base_url.trim())
            )
        };
    Slot {
        id: TYPESAFE_SLOT.into(),
        name: "TypeSafe hosted".into(),
        detail,
        residence: typesafe_residence(cfg),
        in_use: false,
        shared_with: None,
    }
}

/// Same precedence as `client::api_key` for the hosted TypeSafe path.
fn typesafe_residence(cfg: &TypesafeConfig) -> Residence {
    if !cfg.api_key.trim().is_empty() {
        return Residence::Config;
    }
    if let Some(var) = first_env(client::KEY_ENVS) {
        return Residence::Env(var);
    }
    for id in client::KEY_PROVIDER_IDS {
        if stored(id) {
            return Residence::Store((*id).into());
        }
    }
    if let Some(path) = client::key_files()
        .into_iter()
        .find(|p| std::fs::read_to_string(p).is_ok_and(|raw| !raw.trim().is_empty()))
    {
        return Residence::File(path);
    }
    Residence::Absent
}

fn bridge_slot(cfg: &TypesafeConfig) -> Slot {
    let state = crate::jev_local::state();
    let running = crate::jev_local::running_bridge_endpoint();
    let (residence, detail) = if let Some(url) = client::local_bridge_env() {
        (
            Residence::Loopback(url),
            "NUR_JEV_LOCAL_URL · no key".to_string(),
        )
    } else if let Some(endpoint) = running.clone() {
        let backend = state
            .as_ref()
            .map(|s| s.backend.clone())
            .unwrap_or_else(|| "bridge".into());
        (
            Residence::Loopback(endpoint),
            format!("{backend} · answering · no key"),
        )
    } else if client::is_loopback_endpoint(cfg.base_url.trim()) {
        (
            Residence::Loopback(cfg.base_url.trim().to_string()),
            "configured · not answering".to_string(),
        )
    } else if let Some(s) = &state {
        (
            Residence::Absent,
            format!("{} on port {} · not answering", s.backend, s.port),
        )
    } else {
        (Residence::Absent, "`nur jev start` · no key".to_string())
    };
    Slot {
        id: BRIDGE_SLOT.into(),
        name: "Local bridge".into(),
        detail,
        residence,
        in_use: false,
        shared_with: None,
    }
}

fn hosted_slot(
    id: &str,
    name: &str,
    detail: &str,
    envs: &[&str],
    shared_with: Option<&'static str>,
) -> Slot {
    let residence = if let Some(var) = first_env(envs) {
        Residence::Env(var)
    } else if stored(id) {
        Residence::Store(id.into())
    } else {
        Residence::Absent
    };
    Slot {
        id: id.into(),
        name: name.into(),
        detail: detail.into(),
        residence,
        in_use: false,
        shared_with,
    }
}

fn engine_detail(cfg: &TypesafeConfig) -> String {
    let model = if cfg.model.trim().is_empty() {
        "endpoint default"
    } else {
        cfg.model.trim()
    };
    format!("{model} · {}", host_of(cfg.base_url.trim()))
}

fn first_env(vars: &[&str]) -> Option<String> {
    vars.iter()
        .find(|var| std::env::var(var).is_ok_and(|v| !v.trim().is_empty()))
        .map(|var| (*var).to_string())
}

fn stored(id: &str) -> bool {
    !id.starts_with("env:")
        && crate::auth::load_provider_key(id).is_some_and(|k| !k.trim().is_empty())
}

/// What happened when a slot's credential was removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removal {
    /// Nur-owned state was cleared.
    Removed(Residence),
    /// Nur does not own this residence; tells the user what to do instead.
    Manual(String),
    /// There was nothing to remove.
    Nothing,
}

/// Remove the Nur-owned credential behind `slot`. Clears *every* Nur-owned
/// copy for the slot (store entry, nur-home key files, inline config key), so
/// "removed" never leaves a lower-precedence copy quietly taking over.
pub fn remove(slot: &Slot) -> Result<Removal> {
    if !slot.residence.removable() {
        return Ok(match slot.residence.manual_hint() {
            Some(hint) => Removal::Manual(hint),
            None => Removal::Nothing,
        });
    }
    let mut removed = false;
    let store_ids: Vec<&str> = if slot.id == TYPESAFE_SLOT {
        client::KEY_PROVIDER_IDS.to_vec()
    } else {
        vec![slot.id.as_str()]
    };
    for id in store_ids {
        removed |= crate::auth::forget_provider_key(id)?;
    }
    if slot.id == TYPESAFE_SLOT {
        let home = crate::config::nur_home();
        for path in client::key_files() {
            if path.starts_with(&home) && path.exists() {
                std::fs::remove_file(&path)?;
                removed = true;
            }
        }
        let mut cfg = crate::config::load_config()?;
        if !cfg.typesafe.api_key.trim().is_empty() {
            cfg.typesafe.api_key.clear();
            crate::config::save_config(&cfg)?;
            removed = true;
        }
    }
    client::invalidate_key_probe();
    client::invalidate_fallback_probe();
    Ok(if removed {
        Removal::Removed(slot.residence.clone())
    } else {
        Removal::Nothing
    })
}

/// One line for the pinned `/auth` row: the engine in use and its residence.
pub fn summary(cfg: &TypesafeConfig) -> String {
    if !cfg.enabled {
        return "disabled in config".into();
    }
    let all = slots(cfg);
    let keyed = all
        .iter()
        .filter(|s| s.residence != Residence::Absent)
        .count();
    match all.iter().find(|s| s.in_use) {
        Some(slot) => {
            let mut text = format!("{} · {}", slot.name, slot.residence.label());
            if keyed > 1 {
                text.push_str(&format!(" · +{} more", keyed - 1));
            }
            text
        }
        None => "no engine · add a key or `nur jev start`".into(),
    }
}

fn short_path(path: &std::path::Path) -> String {
    let home = crate::config::nur_home();
    match path.strip_prefix(&home) {
        Ok(rest) => format!("~/.nur/{}", rest.display()),
        Err(_) => path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string()),
    }
}

fn host_of(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_else(|| url.to_string())
}

fn port_of(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.port())
        .map(|p| format!(":{p}"))
        .unwrap_or_else(|| host_of(url))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> TypesafeConfig {
        crate::config::Config::default().typesafe
    }

    #[test]
    fn residence_labels_never_carry_the_key() {
        let r = Residence::Store("typesafe".into());
        assert_eq!(r.label(), "saved key");
        assert_eq!(
            Residence::Env("SUBQ_API_KEY".into()).label(),
            "env SUBQ_API_KEY"
        );
        assert_eq!(
            Residence::Loopback("http://127.0.0.1:8788/v1/systemone".into()).label(),
            "loopback :8788"
        );
    }

    #[test]
    fn only_nur_owned_residences_are_removable() {
        assert!(Residence::Config.removable());
        assert!(Residence::Store("x".into()).removable());
        assert!(!Residence::Env("X".into()).removable());
        assert!(!Residence::Loopback("http://127.0.0.1:1".into()).removable());
        assert!(!Residence::Absent.removable());
        let outside = Residence::File(PathBuf::from("/definitely/not/nur/home/key"));
        assert!(!outside.removable());
        assert!(outside.manual_hint().unwrap().contains("yourself"));
    }

    #[test]
    fn a_disabled_layer_has_no_active_slot() {
        let mut c = cfg();
        c.enabled = false;
        assert!(slots(&c).iter().all(|s| !s.in_use));
        assert_eq!(summary(&c), "disabled in config");
    }

    #[test]
    fn a_selected_catalog_engine_gets_its_own_slot() {
        let mut c = cfg();
        c.credential_provider = "jev:fastino-gliner".into();
        c.base_url = "https://api.fastino.ai/v1/systemone".into();
        c.key_env = "NUR_TEST_NO_SUCH_ENV_VAR".into();
        let all = slots(&c);
        let own = all
            .iter()
            .find(|s| s.id == "jev:fastino-gliner")
            .expect("selected engine slot");
        assert_eq!(own.name, "selected engine · fastino-gliner");
        assert!(own.detail.contains("api.fastino.ai"));
        // The env slot is named but unset, so nothing is in use.
        assert!(!own.in_use);
    }

    #[test]
    fn a_gateway_engine_reuses_the_gateway_slot() {
        let mut c = cfg();
        c.credential_provider = "openrouter".into();
        c.base_url = "https://openrouter.ai/api/alpha/decisions".into();
        let all = slots(&c);
        assert_eq!(all.iter().filter(|s| s.id == OPENROUTER_SLOT).count(), 1);
    }

    #[test]
    fn removing_an_env_backed_slot_is_manual() {
        let slot = Slot {
            id: SUBQ_SLOT.into(),
            name: "SubQ Snap".into(),
            detail: String::new(),
            residence: Residence::Env("SUBQ_API_KEY".into()),
            in_use: true,
            shared_with: None,
        };
        match remove(&slot).unwrap() {
            Removal::Manual(hint) => assert!(hint.contains("SUBQ_API_KEY")),
            other => panic!("expected manual removal, got {other:?}"),
        }
    }
}

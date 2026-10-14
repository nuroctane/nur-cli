//! Where each Jev-class engine's credential lives, and which engine the
//! judgment layer is actually using right now.
//!
//! `client::api_key` / `client::fallback_engine` answer "give me a key"; this
//! module answers the user's question in `/auth`: *which engines can judge for
//! me, where does each key sit, and what will Delete remove?* It is read-only
//! except for [`remove`], which only ever touches state Nur itself owns (the
//! credential store, `~/.nur/*.key`, `[typesafe] api_key` in config.toml).
//! Environment variables and the running bridge are reported, never mutated.
//!
//! Everything outside `[typesafe]` is read once into a [`Probe`]; [`slots`],
//! [`summary`] and the in-use decision are pure over it. That keeps one
//! refresh to one store read and one bridge health check, and lets the tests
//! describe a machine instead of reading the developer's.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::config::TypesafeConfig;
use crate::error::Result;

use super::client;

/// A stable id for one engine slot (also the credential-store key for hosted
/// slots).
pub const TYPESAFE_SLOT: &str = "typesafe";
pub const BRIDGE_SLOT: &str = client::LOCAL_BRIDGE_SLOT;
pub const SUBQ_SLOT: &str = "jev:subq-snap";
pub const OPENROUTER_SLOT: &str = "openrouter";

/// One hosted decision engine with a fixed slot: id, name, detail, env vars
/// read for the key, and the chat provider that shares the saved key.
type HostedSlot = (
    &'static str,
    &'static str,
    &'static str,
    &'static [&'static str],
    Option<&'static str>,
);

const HOSTED: &[HostedSlot] = &[
    (
        SUBQ_SLOT,
        "SubQ Snap",
        "subq-snap-preview · api.subq.ai",
        &["SUBQ_API_KEY"],
        None,
    ),
    (
        OPENROUTER_SLOT,
        "OpenRouter Decisions",
        "mercury-decide · openrouter.ai",
        &["OPENROUTER_API_KEY"],
        Some("OpenRouter chat"),
    ),
];

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

    /// Whether a key saved to the store would be read before this one. The
    /// layer reads config, then env, then the store, then key files: a saved
    /// key is read first only when nothing stronger is set.
    pub fn store_would_win(&self) -> bool {
        matches!(
            self,
            Residence::Absent | Residence::Store(_) | Residence::File(_)
        )
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

    /// Whether a key saved from `/auth` would actually be read: an engine
    /// keyed by `--key-env` is read from that variable only, and a residence
    /// the layer reads before the store keeps winning.
    pub fn store_key_is_read(&self, cfg: &TypesafeConfig) -> bool {
        if self.id.starts_with("env:") {
            return false;
        }
        if cfg.credential_provider.trim() == self.id && !cfg.key_env.trim().is_empty() {
            return false;
        }
        self.residence.store_would_win()
    }
}

/// Everything outside `[typesafe]` that decides a slot's residence, read once.
#[derive(Debug, Clone, Default)]
pub struct Probe {
    /// Non-empty values of the environment variables the layer reads.
    pub env: BTreeMap<String, String>,
    /// Credential-store ids with a non-empty saved key (never the keys).
    pub stored: BTreeSet<String>,
    /// TypeSafe key files holding a non-empty key, in precedence order.
    pub key_files: Vec<PathBuf>,
    /// `NUR_JEV_LOCAL_URL`, normalized to the endpoint path.
    pub local_bridge: Option<String>,
    /// The recorded bridge, whether or not it is alive.
    pub bridge_state: Option<crate::jev_local::BridgeState>,
    /// Endpoint of a recorded bridge that answered its health check.
    pub running_bridge: Option<String>,
}

impl Probe {
    /// Read the machine: the environment, the credential store, the key files
    /// and the bridge (one health check, up to 800 ms - keep off the UI thread).
    pub fn read(cfg: &TypesafeConfig) -> Self {
        Self::read_with_store(cfg, crate::auth::saved_key_ids())
    }

    /// [`Probe::read`] for a caller that has already read the credential store.
    pub fn read_with_store(cfg: &TypesafeConfig, stored: BTreeSet<String>) -> Self {
        let mut vars: Vec<&str> = client::KEY_ENVS.to_vec();
        for (_, _, _, envs, _) in HOSTED {
            vars.extend_from_slice(envs);
        }
        let key_env = cfg.key_env.trim();
        if !key_env.is_empty() {
            vars.push(key_env);
        }
        let env = vars
            .into_iter()
            .filter_map(|var| {
                let value = std::env::var(var).ok()?;
                let value = value.trim();
                (!value.is_empty()).then(|| (var.to_string(), value.to_string()))
            })
            .collect();
        let key_files = client::key_files()
            .into_iter()
            .filter(|path| std::fs::read_to_string(path).is_ok_and(|raw| !raw.trim().is_empty()))
            .collect();
        Probe {
            env,
            stored,
            key_files,
            local_bridge: client::local_bridge_env(),
            bridge_state: crate::jev_local::state(),
            running_bridge: crate::jev_local::running_bridge_endpoint(),
        }
    }

    fn env_set(&self, var: &str) -> bool {
        self.env.contains_key(var)
    }

    fn first_env(&self, vars: &[&str]) -> Option<String> {
        vars.iter()
            .find(|var| self.env_set(var))
            .map(|var| (*var).to_string())
    }

    fn stored(&self, id: &str) -> bool {
        !id.starts_with("env:") && self.stored.contains(id)
    }
}

/// Every Jev-class slot, hosted ones first in the fallback preference order,
/// with the bridge second so the two zero-config paths sit at the top.
pub fn slots(cfg: &TypesafeConfig, probe: &Probe) -> Vec<Slot> {
    let active = active_slot(cfg, probe);
    let mut out = vec![typesafe_slot(cfg, probe), bridge_slot(cfg, probe)];
    for (id, name, detail, envs, shared) in HOSTED {
        out.push(hosted_slot(id, name, detail, envs, *shared, probe));
    }
    let selected = cfg.credential_provider.trim();
    let key_env = cfg.key_env.trim();
    let mut env_refs: Vec<&str> = Vec::new();
    if !key_env.is_empty() {
        env_refs.push(key_env);
    }
    if !selected.is_empty() {
        if let Some(slot) = out.iter_mut().find(|s| s.id == selected) {
            // A catalog engine riding a gateway key (`credential_provider =
            // "openrouter"`) reuses that slot, but the row must describe the
            // engine actually configured. Selecting a slot is exclusive in
            // `client::api_key`: it reads `--key-env` or the store, never the
            // gateway's own variable, so that variable is not a residence here.
            slot.detail = engine_detail(cfg);
            slot.residence = hosted_residence(selected, &env_refs, probe);
        } else {
            // A catalog engine picked with `nur jev use --engine <id>` owns
            // its own slot (`jev:<id>`).
            out.push(hosted_slot(
                selected,
                &format!("selected engine · {}", selected.trim_start_matches("jev:")),
                &engine_detail(cfg),
                &env_refs,
                None,
                probe,
            ));
        }
    } else if !key_env.is_empty() {
        // `--key-env` without a store slot: the engine is keyed purely by env.
        out.push(hosted_slot(
            &format!("env:{key_env}"),
            "selected engine",
            &engine_detail(cfg),
            &env_refs,
            None,
            probe,
        ));
    }
    for slot in &mut out {
        slot.in_use = active.as_deref() == Some(slot.id.as_str());
    }
    out
}

/// The endpoint the layer would talk to, as `client::effective_base_url`
/// computes it, over the probe.
fn effective_endpoint(cfg: &TypesafeConfig, probe: &Probe) -> String {
    if let Some(local) = &probe.local_bridge {
        return local.clone();
    }
    let configured = cfg.base_url.trim();
    if !configured.is_empty() {
        return configured.to_string();
    }
    fallback_engine(cfg, probe)
        .map(|engine| engine.base_url)
        .unwrap_or_else(|| client::DEFAULT_BASE_URL.to_string())
}

/// `client::fallback_engine` over the probe: nothing selected, no TypeSafe
/// key anywhere, then a running bridge or a hosted engine by key presence.
fn fallback_engine(cfg: &TypesafeConfig, probe: &Probe) -> Option<client::FallbackEngine> {
    if client::explicit_engine_selection_with(cfg, probe.local_bridge.is_some()) {
        return None;
    }
    if typesafe_residence(cfg, probe) != Residence::Absent {
        return None;
    }
    // The resolver wants key values to decide presence; the probe only keeps
    // presence, so a stand-in stands for "set". The value is never used here.
    client::resolve_fallback_engine_with(
        probe.running_bridge.clone(),
        |var| probe.env.get(var).cloned(),
        |id| probe.stored(id).then(|| "present".to_string()),
    )
}

/// The slot the layer resolves to, mirroring `client::client` step for step
/// (endpoint validity, then `client::api_key`) without reading any key value.
/// Every early `None` here is an `Availability::Unavailable` there.
fn active_slot(cfg: &TypesafeConfig, probe: &Probe) -> Option<String> {
    if !cfg.enabled {
        return None;
    }
    let endpoint = effective_endpoint(cfg, probe);
    if client::validate_endpoint(&endpoint).is_err() {
        return None;
    }
    let selected = cfg.credential_provider.trim();
    let key_env = cfg.key_env.trim();
    if !key_env.is_empty() {
        return probe.env_set(key_env).then(|| {
            if selected.is_empty() {
                format!("env:{key_env}")
            } else {
                selected.to_string()
            }
        });
    }
    if !selected.is_empty() {
        return probe.stored(selected).then(|| selected.to_string());
    }
    if client::is_loopback_endpoint(&endpoint) {
        return Some(BRIDGE_SLOT.into());
    }
    if let Some(engine) = fallback_engine(cfg, probe) {
        return Some(engine.slot.into());
    }
    if !cfg.api_key.trim().is_empty() {
        return Some(TYPESAFE_SLOT.into());
    }
    if !client::is_hosted_typesafe_endpoint(&endpoint) {
        return None;
    }
    (typesafe_residence(cfg, probe) != Residence::Absent).then(|| TYPESAFE_SLOT.to_string())
}

fn typesafe_slot(cfg: &TypesafeConfig, probe: &Probe) -> Slot {
    let detail =
        if cfg.base_url.trim().is_empty() || client::is_loopback_endpoint(cfg.base_url.trim()) {
            format!("jev-latest · {}", client::HOSTED_TYPESAFE_HOST)
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
        residence: typesafe_residence(cfg, probe),
        in_use: false,
        shared_with: None,
    }
}

/// Same precedence as `client::api_key` for the hosted TypeSafe path.
fn typesafe_residence(cfg: &TypesafeConfig, probe: &Probe) -> Residence {
    if !cfg.api_key.trim().is_empty() {
        return Residence::Config;
    }
    if let Some(var) = probe.first_env(client::KEY_ENVS) {
        return Residence::Env(var);
    }
    for id in client::KEY_PROVIDER_IDS {
        if probe.stored(id) {
            return Residence::Store((*id).into());
        }
    }
    if let Some(path) = probe.key_files.first() {
        return Residence::File(path.clone());
    }
    Residence::Absent
}

fn bridge_slot(cfg: &TypesafeConfig, probe: &Probe) -> Slot {
    let state = probe.bridge_state.as_ref();
    let (residence, detail) = if let Some(url) = &probe.local_bridge {
        (
            Residence::Loopback(url.clone()),
            "NUR_JEV_LOCAL_URL · no key".to_string(),
        )
    } else if let Some(endpoint) = &probe.running_bridge {
        let backend = state
            .map(|s| s.backend.clone())
            .unwrap_or_else(|| "bridge".into());
        (
            Residence::Loopback(endpoint.clone()),
            format!("{backend} · answering · no key"),
        )
    } else if client::is_loopback_endpoint(cfg.base_url.trim()) {
        (
            Residence::Loopback(cfg.base_url.trim().to_string()),
            "configured · not answering".to_string(),
        )
    } else if let Some(s) = state {
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

fn hosted_residence(id: &str, envs: &[&str], probe: &Probe) -> Residence {
    if let Some(var) = probe.first_env(envs) {
        Residence::Env(var)
    } else if probe.stored(id) {
        Residence::Store(id.into())
    } else {
        Residence::Absent
    }
}

fn hosted_slot(
    id: &str,
    name: &str,
    detail: &str,
    envs: &[&str],
    shared_with: Option<&'static str>,
    probe: &Probe,
) -> Slot {
    Slot {
        id: id.into(),
        name: name.into(),
        detail: detail.into(),
        residence: hosted_residence(id, envs, probe),
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
///
/// For the TypeSafe slot the on-disk `[typesafe] api_key` is cleared here;
/// a caller holding its own copy of the config must clear that too, or the
/// next save writes the key straight back.
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
pub fn summary(cfg: &TypesafeConfig, probe: &Probe) -> String {
    if !cfg.enabled {
        return "disabled in config".into();
    }
    let all = slots(cfg, probe);
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

    fn with_env(vars: &[(&str, &str)]) -> Probe {
        Probe {
            env: vars
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            ..Probe::default()
        }
    }

    fn with_stored(ids: &[&str]) -> Probe {
        Probe {
            stored: ids.iter().map(|id| id.to_string()).collect(),
            ..Probe::default()
        }
    }

    fn in_use(slots: &[Slot]) -> Option<&str> {
        slots.iter().find(|s| s.in_use).map(|s| s.id.as_str())
    }

    fn slot<'a>(slots: &'a [Slot], id: &str) -> &'a Slot {
        slots
            .iter()
            .find(|s| s.id == id)
            .unwrap_or_else(|| panic!("no slot {id} in {slots:?}"))
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
        let probe = with_env(&[("TYPESAFE_API_KEY", "ts-key")]);
        assert_eq!(in_use(&slots(&c, &probe)), None);
        assert_eq!(summary(&c, &probe), "disabled in config");
    }

    #[test]
    fn an_empty_machine_has_keys_nowhere_and_nothing_in_use() {
        let all = slots(&cfg(), &Probe::default());
        assert!(all.iter().all(|s| s.residence == Residence::Absent));
        assert_eq!(in_use(&all), None);
        assert_eq!(
            summary(&cfg(), &Probe::default()),
            "no engine · add a key or `nur jev start`"
        );
    }

    /// The client sends ambient TypeSafe keys (env / store / file) only to
    /// `api.typesafe.ai`. A custom endpoint with such a key must not be shown
    /// as judging, or the panel claims a key the client refuses to use.
    #[test]
    fn ambient_typesafe_keys_are_in_use_only_against_the_hosted_api() {
        let probe = with_env(&[("TYPESAFE_API_KEY", "ts-key")]);
        let hosted = cfg();
        assert_eq!(in_use(&slots(&hosted, &probe)), Some(TYPESAFE_SLOT));

        let mut custom = cfg();
        custom.base_url = "https://engine.example.com/v1/systemone".into();
        let all = slots(&custom, &probe);
        assert_eq!(
            slot(&all, TYPESAFE_SLOT).residence,
            Residence::Env("TYPESAFE_API_KEY".into()),
            "the key still exists and the row must say where"
        );
        assert_eq!(in_use(&all), None);

        // An inline config key is explicit and goes to any host.
        custom.api_key = "inline-key".into();
        let all = slots(&custom, &probe);
        assert_eq!(slot(&all, TYPESAFE_SLOT).residence, Residence::Config);
        assert_eq!(in_use(&all), Some(TYPESAFE_SLOT));
    }

    /// `client::client` rejects a plain-http endpoint off the loopback before
    /// it looks at any key, so nothing can be in use there.
    #[test]
    fn an_invalid_endpoint_leaves_nothing_in_use() {
        let mut c = cfg();
        c.base_url = "http://engine.example.com/v1/systemone".into();
        c.api_key = "inline-key".into();
        let all = slots(&c, &Probe::default());
        assert_eq!(slot(&all, TYPESAFE_SLOT).residence, Residence::Config);
        assert_eq!(in_use(&all), None);
    }

    /// A selected `--key-env` that is unset makes the client unavailable even
    /// when `NUR_JEV_LOCAL_URL` points at a bridge; the bridge row still shows
    /// the loopback residence, but nothing is judging.
    #[test]
    fn an_unset_key_env_is_not_rescued_by_the_bridge_env() {
        let mut c = cfg();
        c.key_env = "MY_ENGINE_KEY".into();
        let probe = Probe {
            local_bridge: Some("http://127.0.0.1:8788/v1/systemone".into()),
            ..Probe::default()
        };
        let all = slots(&c, &probe);
        assert!(matches!(
            slot(&all, BRIDGE_SLOT).residence,
            Residence::Loopback(_)
        ));
        assert_eq!(in_use(&all), None);

        let probe = Probe {
            env: [("MY_ENGINE_KEY".to_string(), "k".to_string())].into(),
            ..probe
        };
        assert_eq!(in_use(&slots(&c, &probe)), Some("env:MY_ENGINE_KEY"));
    }

    #[test]
    fn a_bridge_env_or_running_bridge_judges_without_a_key() {
        let env_probe = Probe {
            local_bridge: Some("http://127.0.0.1:8788/v1/systemone".into()),
            ..Probe::default()
        };
        assert_eq!(in_use(&slots(&cfg(), &env_probe)), Some(BRIDGE_SLOT));

        let running = Probe {
            running_bridge: Some("http://127.0.0.1:8790/v1/systemone".into()),
            ..Probe::default()
        };
        let all = slots(&cfg(), &running);
        assert_eq!(in_use(&all), Some(BRIDGE_SLOT));
        assert_eq!(
            slot(&all, BRIDGE_SLOT).residence,
            Residence::Loopback("http://127.0.0.1:8790/v1/systemone".into())
        );
        assert_eq!(summary(&cfg(), &running), "Local bridge · loopback :8790");
    }

    /// With nothing selected, a hosted decision key is picked up by
    /// presence - SubQ before OpenRouter, env before store - and the summary
    /// counts the other keyed slots.
    #[test]
    fn hosted_fallback_follows_the_client_preference_order() {
        let both = Probe {
            env: [("OPENROUTER_API_KEY".to_string(), "or".to_string())].into(),
            stored: ["jev:subq-snap".to_string()].into(),
            ..Probe::default()
        };
        assert_eq!(in_use(&slots(&cfg(), &both)), Some(SUBQ_SLOT));
        assert_eq!(summary(&cfg(), &both), "SubQ Snap · saved key · +1 more");

        let stored_only = with_stored(&["openrouter"]);
        let all = slots(&cfg(), &stored_only);
        assert_eq!(in_use(&all), Some(OPENROUTER_SLOT));
        assert_eq!(
            slot(&all, OPENROUTER_SLOT).shared_with,
            Some("OpenRouter chat")
        );

        // A TypeSafe key anywhere turns the fallback off.
        let typesafe_too = Probe {
            stored: ["openrouter".to_string(), "jev".to_string()].into(),
            ..Probe::default()
        };
        assert_eq!(in_use(&slots(&cfg(), &typesafe_too)), Some(TYPESAFE_SLOT));
    }

    #[test]
    fn a_selected_catalog_engine_gets_its_own_slot() {
        let mut c = cfg();
        c.credential_provider = "jev:fastino-gliner".into();
        c.base_url = "https://api.fastino.ai/v1/systemone".into();
        c.key_env = "NUR_TEST_NO_SUCH_ENV_VAR".into();
        let all = slots(&c, &Probe::default());
        let own = slot(&all, "jev:fastino-gliner");
        assert_eq!(own.name, "selected engine · fastino-gliner");
        assert_eq!(own.detail, "endpoint default · api.fastino.ai");
        // The env slot is named but unset, so nothing is in use.
        assert_eq!(in_use(&all), None);

        c.key_env.clear();
        let all = slots(&c, &with_stored(&["jev:fastino-gliner"]));
        assert_eq!(in_use(&all), Some("jev:fastino-gliner"));
        assert_eq!(
            slot(&all, "jev:fastino-gliner").residence,
            Residence::Store("jev:fastino-gliner".into())
        );
    }

    /// An engine hosted on a gateway reuses the gateway's slot, which must
    /// then describe that engine and honour a `--key-env` override instead of
    /// reading `no key` while carrying the in-use marker.
    #[test]
    fn a_gateway_engine_reuses_the_gateway_slot_with_its_own_detail_and_key_env() {
        let mut c = cfg();
        c.credential_provider = "openrouter".into();
        c.base_url = "https://openrouter.ai/api/alpha/decisions".into();
        c.model = "inception/mercury-decide".into();
        c.key_env = "MY_OR_KEY".into();
        let probe = with_env(&[("MY_OR_KEY", "k")]);
        let all = slots(&c, &probe);
        assert_eq!(all.iter().filter(|s| s.id == OPENROUTER_SLOT).count(), 1);
        let gateway = slot(&all, OPENROUTER_SLOT);
        assert_eq!(gateway.detail, "inception/mercury-decide · openrouter.ai");
        assert_eq!(gateway.residence, Residence::Env("MY_OR_KEY".into()));
        assert!(gateway.in_use);
        assert!(
            !gateway.store_key_is_read(&c),
            "a key saved to the store is never read while --key-env selects the engine"
        );

        // Without the override the selection reads the store only: the
        // gateway's own variable serves the unselected fallback path, and the
        // client ignores it here, so the row must not claim it.
        c.key_env.clear();
        let all = slots(&c, &with_env(&[("OPENROUTER_API_KEY", "or")]));
        assert_eq!(slot(&all, OPENROUTER_SLOT).residence, Residence::Absent);
        assert_eq!(in_use(&all), None);
        let all = slots(&c, &with_stored(&["openrouter"]));
        assert_eq!(
            slot(&all, OPENROUTER_SLOT).residence,
            Residence::Store("openrouter".into())
        );
        assert_eq!(in_use(&all), Some(OPENROUTER_SLOT));
    }

    /// The key field's hint must agree with the client: a saved key is read
    /// only when nothing the layer reads first is set.
    #[test]
    fn a_saved_key_is_only_read_when_nothing_outranks_the_store() {
        let probe = with_env(&[("TYPESAFE_API_KEY", "ts")]);
        let all = slots(&cfg(), &probe);
        assert!(!slot(&all, TYPESAFE_SLOT).store_key_is_read(&cfg()));
        let all = slots(&cfg(), &with_stored(&["typesafe"]));
        assert!(slot(&all, TYPESAFE_SLOT).store_key_is_read(&cfg()));
        let all = slots(&cfg(), &Probe::default());
        assert!(slot(&all, SUBQ_SLOT).store_key_is_read(&cfg()));

        let mut c = cfg();
        c.key_env = "ONLY_ENV".into();
        let all = slots(&c, &with_env(&[("ONLY_ENV", "k")]));
        assert!(!slot(&all, "env:ONLY_ENV").store_key_is_read(&c));
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

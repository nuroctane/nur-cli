//! Secrets the user hands nur without putting them in the conversation.
//!
//! A secret is a named value (`GITHUB_TOKEN`) kept in `~/.nur/secrets.json`,
//! owner-only like `auth.json`. Every shell command gets each secret as an
//! environment variable, and any tool output that would show a value reads
//! `[secret:NAME]` instead, so the model works with `$NAME` and never sees
//! the value. The TUI takes values in a masked modal (`/secrets add`, a
//! credential pasted into the prompt, or the `secret` tool asking for one);
//! `nur secrets set` does the same from a terminal.

use crate::config::nur_home;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Values shorter than this are not redacted from output: a short value
/// (`true`, `1234`) would rewrite ordinary text all over the transcript.
pub const MIN_VALUE_CHARS: usize = 6;

#[derive(Debug, Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    secrets: BTreeMap<String, Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    value: String,
    added_at: String,
}

#[cfg(not(test))]
fn path() -> PathBuf {
    nur_home().join("secrets.json")
}

#[cfg(test)]
thread_local! {
    static TEST_PATH: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Tests never read or write the real home: each test thread gets its own
/// file, and a thread that set none sees no secrets.
#[cfg(test)]
fn path() -> PathBuf {
    let _ = nur_home;
    TEST_PATH.with(|p| p.borrow().clone()).unwrap_or_else(|| {
        std::env::temp_dir()
            .join(format!("nur-no-secrets-{}", std::process::id()))
            .join("secrets.json")
    })
}

/// Point this test thread at a fresh secrets file.
#[cfg(test)]
pub fn use_test_store() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nur-secrets-{}", uuid::Uuid::new_v4().simple()));
    let file = dir.join("secrets.json");
    TEST_PATH.with(|p| *p.borrow_mut() = Some(file.clone()));
    dir
}

fn load() -> Store {
    crate::config::read_replaced_string(&path())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save(store: &Store) -> Result<(), String> {
    let raw = serde_json::to_vec_pretty(store).map_err(|e| e.to_string())?;
    crate::config::private_atomic_write(&path(), &raw)
        .map_err(|e| format!("write {}: {e}", path().display()))?;
    forget_cache();
    Ok(())
}

/// An environment variable name: `A-Z`, digits and `_`, not starting with a
/// digit. Lowercase is accepted and upper-cased by [`normalize_name`].
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_uppercase() || c == '_')
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && name.len() <= 64
}

/// `my-token` → `MY_TOKEN`.
pub fn normalize_name(name: &str) -> String {
    name.trim()
        .trim_start_matches('$')
        .chars()
        .map(|c| match c {
            '-' | ' ' | '.' => '_',
            c => c.to_ascii_uppercase(),
        })
        .collect()
}

/// Saved secret names with when each was added, sorted.
pub fn names() -> Vec<(String, String)> {
    load()
        .secrets
        .into_iter()
        .map(|(name, entry)| (name, entry.added_at))
        .collect()
}

pub fn exists(name: &str) -> bool {
    load().secrets.contains_key(name)
}

/// Save (or replace) a secret.
pub fn set(name: &str, value: &str) -> Result<(), String> {
    if !valid_name(name) {
        return Err(format!(
            "`{name}` is not a usable name - letters, digits and _ (like GITHUB_TOKEN)"
        ));
    }
    let value = value.trim();
    if value.chars().count() < MIN_VALUE_CHARS {
        return Err(format!(
            "a secret needs at least {MIN_VALUE_CHARS} characters"
        ));
    }
    let mut store = load();
    store.secrets.insert(
        name.to_string(),
        Entry {
            value: value.to_string(),
            added_at: chrono::Utc::now().to_rfc3339(),
        },
    );
    save(&store)
}

/// Remove a secret; false when there was none of that name.
pub fn remove(name: &str) -> Result<bool, String> {
    let mut store = load();
    if store.secrets.remove(name).is_none() {
        return Ok(false);
    }
    save(&store).map(|_| true)
}

/// A free name for a credential usually kept in `base` (`GITHUB_TOKEN`,
/// then `GITHUB_TOKEN_2`, ...).
pub fn free_name(base: &str) -> String {
    let taken = load().secrets;
    if !taken.contains_key(base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}_{n}"))
        .find(|name| !taken.contains_key(name))
        .unwrap_or_else(|| base.to_string())
}

type Pairs = Arc<Vec<(String, String)>>;
type Stamp = (PathBuf, Option<std::time::SystemTime>);

/// Name/value pairs, re-read when the file changes (one stat per call).
static CACHE: Mutex<Option<(Stamp, Pairs)>> = Mutex::new(None);

fn forget_cache() {
    if let Ok(mut cache) = CACHE.lock() {
        *cache = None;
    }
}

fn current() -> Pairs {
    let file = path();
    let modified = std::fs::metadata(&file).and_then(|m| m.modified()).ok();
    let stamp = (file, modified);
    let mut cache = match CACHE.lock() {
        Ok(cache) => cache,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some((seen, pairs)) = cache.as_ref() {
        if *seen == stamp {
            return pairs.clone();
        }
    }
    let pairs: Pairs = Arc::new(
        load()
            .secrets
            .into_iter()
            .map(|(name, entry)| (name, entry.value))
            .collect(),
    );
    *cache = Some((stamp, pairs.clone()));
    pairs
}

/// Give a tool subprocess every secret as an environment variable.
pub fn inject(cmd: &mut std::process::Command) {
    for (name, value) in current().iter() {
        cmd.env(name, value);
    }
}

/// `text` with every secret value (as written, JSON-escaped or URL-encoded)
/// replaced by `[secret:NAME]`.
pub fn redact(text: &str) -> Cow<'_, str> {
    let pairs = current();
    if pairs.is_empty() {
        return Cow::Borrowed(text);
    }
    let mut out = Cow::Borrowed(text);
    for (name, value) in pairs.iter() {
        if value.chars().count() < MIN_VALUE_CHARS {
            continue;
        }
        let marker = format!("[secret:{name}]");
        let json = serde_json::to_string(value)
            .map(|q| q[1..q.len() - 1].to_string())
            .unwrap_or_default();
        let url: String = url_encode(value);
        for form in [value.as_str(), json.as_str(), url.as_str()] {
            if !form.is_empty() && out.contains(form) {
                out = Cow::Owned(out.replace(form, &marker));
            }
        }
    }
    out
}

fn url_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `text` safe to keep on disk: saved secret values become `[secret:NAME]`
/// and any other live-looking credential becomes `[credential]`.
pub fn scrub(text: &str) -> String {
    let text = redact(text).into_owned();
    let mut out = text.clone();
    for token in crate::tools::sensitive::live_tokens(&text)
        .into_iter()
        .rev()
    {
        out.replace_range(token.range, "[credential]");
    }
    out
}

/// The line the system prompt carries: secret names only.
pub fn prompt_note() -> Option<String> {
    let pairs = current();
    if pairs.is_empty() {
        return None;
    }
    let names: Vec<String> = pairs.iter().map(|(name, _)| format!("${name}")).collect();
    Some(format!(
        "Secrets the user saved for this machine, set as environment variables in every shell \
         command: {}. Use them by name (`$NAME`); never print, echo or write a value into a \
         file. Output that contains one reads [secret:NAME]. To get a credential you do not \
         have, call the `secret` tool (action request) instead of asking the user to paste it.",
        names.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Failure modes: a value leaks through output in an escaped form; a short
    // value rewrites ordinary text; an unusable name is saved.
    #[test]
    fn redaction_covers_escaped_forms_and_skips_short_values() {
        let dir = use_test_store();
        set("API_TOKEN", "tok\"en/with+chars 99").unwrap();
        let text = r#"raw tok"en/with+chars 99 · json "tok\"en/with+chars 99" · url tok%22en%2Fwith%2Bchars%2099"#;
        let out = redact(text);
        assert!(!out.contains("with+chars"), "{out}");
        assert!(!out.contains("with%2Bchars"), "{out}");
        assert_eq!(out.matches("[secret:API_TOKEN]").count(), 3, "{out}");
        assert!(set("PIN", "1234").is_err());
        assert_eq!(redact("the year 1234"), "the year 1234");
        assert!(set("bad name", "long enough value").is_err());
        assert_eq!(normalize_name("$my-token"), "MY_TOKEN");
        assert_eq!(free_name("API_TOKEN"), "API_TOKEN_2");
        assert!(remove("API_TOKEN").unwrap());
        assert_eq!(redact("tok\"en/with+chars 99"), "tok\"en/with+chars 99");
        let _ = std::fs::remove_dir_all(dir);
    }

    // Failure mode: a key pasted into the prompt is written to the prompt
    // history file before the secret modal catches it.
    #[test]
    fn history_never_keeps_a_pasted_credential() {
        let token = "ghp_aB3dE5fG7hJ9kL1mN3pQ5rS7tU9vW1xY3zA5";
        assert_eq!(
            scrub(&format!("use {token} to push")),
            "use [credential] to push"
        );
        assert_eq!(scrub("push with $GITHUB_TOKEN"), "push with $GITHUB_TOKEN");
    }
}

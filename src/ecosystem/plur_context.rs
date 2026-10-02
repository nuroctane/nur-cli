//! PLUR's prompt injection, kept off the model request path.
//!
//! The injection is keyed on a task that names only the workspace, so it is the
//! same for every turn there, and producing it launches the PLUR Node CLI
//! (about a second on Windows). It used to run in front of every turn's first
//! model request. Now sessions prefetch it, each turn takes the newest finished
//! result and schedules a refresh for the next one, and only a workspace's
//! first turn waits, for the fetch already in flight.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};

#[derive(Default)]
struct Slot {
    value: Option<Option<String>>,
    running: bool,
}

/// Values from a slow fetch, per key, readable without waiting once one exists.
pub(super) struct Refreshing {
    fetch: fn(&str) -> Option<String>,
    slots: Mutex<HashMap<String, Slot>>,
    ready: Condvar,
}

impl Refreshing {
    pub(super) fn new(fetch: fn(&str) -> Option<String>) -> Arc<Self> {
        Arc::new(Self {
            fetch,
            slots: Mutex::default(),
            ready: Condvar::new(),
        })
    }

    /// Start a background fetch for `key` unless one is already running.
    pub(super) fn prefetch(self: &Arc<Self>, key: &str) {
        {
            let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
            let slot = slots.entry(key.to_string()).or_default();
            if slot.running {
                return;
            }
            slot.running = true;
        }
        let this = Arc::clone(self);
        let owned = key.to_string();
        let spawned = std::thread::Builder::new()
            .name("plur-context".into())
            .spawn(move || {
                // A panicking fetch must still release the readers waiting on it.
                let fetch = std::panic::AssertUnwindSafe(|| (this.fetch)(&owned));
                this.finish(&owned, std::panic::catch_unwind(fetch).ok());
            });
        if spawned.is_err() {
            self.finish(key, None);
        }
    }

    fn finish(&self, key: &str, value: Option<Option<String>>) {
        let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        let slot = slots.entry(key.to_string()).or_default();
        if value.is_some() {
            slot.value = value;
        }
        slot.running = false;
        self.ready.notify_all();
    }

    /// The newest finished value for `key`. Waits only while no value exists
    /// yet; a read of an existing value schedules the refresh the next read
    /// will see.
    pub(super) fn get(self: &Arc<Self>, key: &str) -> Option<String> {
        let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        let existing = slots.get(key).is_some_and(|slot| slot.value.is_some());
        if !existing && !slots.get(key).is_some_and(|slot| slot.running) {
            drop(slots);
            self.prefetch(key);
            slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        }
        loop {
            match slots.get(key) {
                Some(Slot {
                    value: Some(value), ..
                }) => {
                    let value = value.clone();
                    drop(slots);
                    if existing {
                        self.prefetch(key);
                    }
                    return value;
                }
                Some(Slot { running: true, .. }) => {
                    slots = self
                        .ready
                        .wait(slots)
                        .unwrap_or_else(PoisonError::into_inner);
                }
                _ => return None,
            }
        }
    }
}

fn shared() -> &'static Arc<Refreshing> {
    static PLUR: OnceLock<Arc<Refreshing>> = OnceLock::new();
    PLUR.get_or_init(|| Refreshing::new(super::plur_inject))
}

/// The task PLUR's injection is keyed on: the workspace's directory name.
fn plur_task(cwd: &std::path::Path) -> String {
    format!(
        "coding agent session in {}",
        cwd.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("workspace")
    )
}

/// Begin fetching this workspace's PLUR context so its first turn finds it.
pub fn plur_prefetch(cwd: &std::path::Path) {
    shared().prefetch(&plur_task(cwd));
}

/// PLUR context for this turn without holding up its model request.
pub fn plur_context(cwd: &std::path::Path) -> Option<String> {
    shared().get(&plur_task(cwd))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    // Failure modes: a turn waits on the CLI although a finished value exists;
    // concurrent first turns launch duplicate fetches; a refresh never reaches
    // the next turn; a failed spawn or fetch leaves a reader waiting forever.

    static SLOW_CALLS: AtomicUsize = AtomicUsize::new(0);
    fn slow(key: &str) -> Option<String> {
        let n = SLOW_CALLS.fetch_add(1, Ordering::SeqCst) + 1;
        std::thread::sleep(Duration::from_millis(300));
        Some(format!("{key} #{n}"))
    }

    #[test]
    fn only_the_first_read_waits_and_later_reads_see_refreshes() {
        let cache = Refreshing::new(slow);
        let readers: Vec<_> = (0..4)
            .map(|_| {
                let cache = Arc::clone(&cache);
                std::thread::spawn(move || cache.get("ws"))
            })
            .collect();
        let first: Vec<_> = readers.into_iter().map(|r| r.join().unwrap()).collect();
        assert!(
            first.iter().all(|v| v.as_deref() == Some("ws #1")),
            "{first:?}"
        );
        assert_eq!(
            SLOW_CALLS.load(Ordering::SeqCst),
            1,
            "concurrent first reads share one fetch"
        );

        let started = Instant::now();
        assert_eq!(cache.get("ws").as_deref(), Some("ws #1"));
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "a finished value never waits"
        );

        // That read scheduled a refresh; once it lands, the next read sees it.
        let deadline = Instant::now() + Duration::from_secs(5);
        while cache.get("ws").as_deref() == Some("ws #1") {
            assert!(Instant::now() < deadline, "the refresh never arrived");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn absent(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn a_fetch_with_nothing_to_inject_does_not_block_or_repeat_waits() {
        let cache = Refreshing::new(absent);
        assert_eq!(cache.get("ws"), None);
        let started = Instant::now();
        assert_eq!(cache.get("ws"), None);
        assert!(started.elapsed() < Duration::from_millis(100));
    }
}

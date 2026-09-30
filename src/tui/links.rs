//! Bounded visible-row metadata. Filesystem probes run on a dedicated worker.
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};
type Urls = Vec<(usize, usize, String)>;
type Paths = Vec<(usize, usize, PathBuf, crate::open_uri::PathKind)>;
type Key = (PathBuf, String, bool);
type Worker = (mpsc::SyncSender<Key>, mpsc::Receiver<(Key, Paths)>);
type Entry = (Instant, Urls, Paths);
#[derive(Default)]
pub struct LinkCache {
    entries: HashMap<Key, Entry>,
    pending: HashSet<Key>,
    worker: Option<Worker>,
}
impl LinkCache {
    pub fn get(&mut self, cwd: &Path, text: &str) -> (Urls, Paths) {
        self.lookup(cwd, text, false)
    }
    pub fn destination(&mut self, cwd: &Path, target: &str) -> (Urls, Paths) {
        self.lookup(cwd, target, true)
    }
    fn lookup(&mut self, cwd: &Path, text: &str, exact: bool) -> (Urls, Paths) {
        if self.worker.is_none() {
            let (tx, jobs) = mpsc::sync_channel::<Key>(512);
            let (done, rx) = mpsc::channel();
            if std::thread::Builder::new()
                .name("nur-links".into())
                .spawn(move || {
                    while let Ok(key) = jobs.recv() {
                        let paths = if key.2 {
                            crate::open_uri::resolve_target(&key.1, &key.0)
                                .map(|(path, kind)| {
                                    vec![(
                                        0,
                                        unicode_width::UnicodeWidthStr::width(key.1.as_str()),
                                        path,
                                        kind,
                                    )]
                                })
                                .unwrap_or_default()
                        } else {
                            crate::open_uri::find_path_spans(&key.1, &key.0)
                        };
                        if done.send((key, paths)).is_err() {
                            break;
                        }
                    }
                })
                .is_ok()
            {
                self.worker = Some((tx, rx));
            }
        }
        if let Some((_, rx)) = &self.worker {
            while let Ok((key, mut paths)) = rx.try_recv() {
                self.pending.remove(&key);
                let urls = crate::open_uri::find_url_spans(&key.1);
                paths.retain(|(lo, _, _, _)| {
                    !urls.iter().any(|(start, end, _)| lo >= start && lo < end)
                });
                if self.entries.len() >= 512 {
                    self.entries.clear();
                }
                self.entries.insert(key, (Instant::now(), urls, paths));
            }
        }
        let key = (cwd.to_path_buf(), text.to_string(), exact);
        let fresh = self
            .entries
            .get(&key)
            .is_some_and(|entry| entry.0.elapsed() < Duration::from_secs(2));
        if !fresh && !self.pending.contains(&key) {
            if let Some((tx, _)) = &self.worker {
                if tx.try_send(key.clone()).is_ok() {
                    self.pending.insert(key.clone());
                }
            }
        }
        self.entries
            .get(&key)
            .map(|entry| (entry.1.clone(), entry.2.clone()))
            .unwrap_or_else(|| (crate::open_uri::find_url_spans(text), Vec::new()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wait(cache: &mut LinkCache, cwd: &Path, text: &str) -> Paths {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            let paths = cache.get(cwd, text).1;
            if cache
                .entries
                .contains_key(&(cwd.to_path_buf(), text.into(), false))
            {
                return paths;
            }
            assert!(Instant::now() < end, "link worker did not finish");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn cache_separates_workspaces_and_revalidates() {
        let root = std::env::temp_dir().join(format!("nur-links-{}", uuid::Uuid::new_v4()));
        let a = root.join("a");
        let b = root.join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("sample.rs"), "").unwrap();
        let mut cache = LinkCache::default();
        assert_eq!(wait(&mut cache, &a, "./sample.rs").len(), 1);
        assert!(wait(&mut cache, &b, "./sample.rs").is_empty());
        std::fs::remove_file(a.join("sample.rs")).unwrap();
        cache
            .entries
            .get_mut(&(a.clone(), "./sample.rs".into(), false))
            .unwrap()
            .0 = Instant::now() - Duration::from_secs(3);
        cache.get(&a, "./sample.rs");
        let end = Instant::now() + Duration::from_secs(5);
        while !cache.get(&a, "./sample.rs").1.is_empty() {
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(5));
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}

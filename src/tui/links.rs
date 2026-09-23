//! Bounded visible-row link cache. Revalidate paths so external edits are reflected.
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
type Urls = Vec<(usize, usize, String)>;
type Paths = Vec<(usize, usize, PathBuf, crate::open_uri::PathKind)>;
#[derive(Default)]
pub struct LinkCache {
    entries: HashMap<(PathBuf, String), (Instant, Urls, Paths)>,
}
impl LinkCache {
    pub fn get(&mut self, cwd: &Path, text: &str) -> (Urls, Paths) {
        let key = (cwd.to_path_buf(), text.to_string());
        if let Some((at, urls, paths)) = self.entries.get(&key) {
            if at.elapsed() < Duration::from_secs(2) {
                return (urls.clone(), paths.clone());
            }
        }
        let urls = crate::open_uri::find_url_spans(text);
        let mut paths = crate::open_uri::find_path_spans(text, cwd);
        paths.retain(|(lo, _, _, _)| !urls.iter().any(|(start, end, _)| lo >= start && lo < end));
        if self.entries.len() >= 512 {
            self.entries.clear();
        }
        self.entries
            .insert(key, (Instant::now(), urls.clone(), paths.clone()));
        (urls, paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_separates_workspaces_and_revalidates() {
        let root = std::env::temp_dir().join(format!("nur-links-{}", uuid::Uuid::new_v4()));
        let a = root.join("a");
        let b = root.join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("sample.rs"), "").unwrap();
        let mut cache = LinkCache::default();
        assert_eq!(cache.get(&a, "./sample.rs").1.len(), 1);
        assert!(cache.get(&b, "./sample.rs").1.is_empty());
        for (at, _, _) in cache.entries.values_mut() {
            *at = Instant::now() - Duration::from_secs(3);
        }
        assert_eq!(cache.get(&a, "./sample.rs").1.len(), 1);
        assert!(
            cache
                .entries
                .get(&(a.clone(), "./sample.rs".into()))
                .unwrap()
                .0
                .elapsed()
                < Duration::from_secs(1)
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

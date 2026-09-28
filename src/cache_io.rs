//! Cache publication without a delete-before-rename gap. The old generation
//! remains readable if serialization, writing, or replacement fails.
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

pub fn publish(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("cache has no parent"))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".nur-cache-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        // std::fs::rename replaces an existing file on both Windows and Unix.
        // Never remove the destination first: readers must see old or new.
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // Failure: a reader sees a missing file or half-written replacement while
    // another thread publishes large generations of the same cache.
    #[test]
    fn readers_see_complete_generations_during_replacement() {
        let root = std::env::temp_dir().join(format!("nur-publish-{}", uuid::Uuid::new_v4()));
        let path = root.join("cache.json");
        let payload = "complete".repeat(8192);
        let bytes = serde_json::to_vec(&serde_json::json!({"payload": payload})).unwrap();
        publish(&path, &bytes).unwrap();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                for _ in 0..30 {
                    publish(&path, &bytes).unwrap();
                }
            });
            for _ in 0..100 {
                let value: serde_json::Value =
                    serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                assert_eq!(value["payload"], payload);
            }
        });
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }
}

//! Opt-in, local startup timings. Stage names contain no paths, prompts, or credentials.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

pub fn mark(stage: &str) {
    static TRACE: OnceLock<Option<(PathBuf, Instant)>> = OnceLock::new();
    let trace = TRACE.get_or_init(|| {
        let value = std::env::var("NUR_STARTUP_TRACE").ok()?;
        if value.is_empty() || value == "0" {
            return None;
        }
        let path = if value == "1" {
            crate::config::nur_home()
                .join("cache")
                .join(format!("startup-{}.jsonl", std::process::id()))
        } else {
            PathBuf::from(value)
        };
        Some((path, Instant::now()))
    });
    if let Some((path, start)) = trace {
        let row = serde_json::json!({"pid": std::process::id(), "stage": stage, "ms": start.elapsed().as_secs_f64() * 1000.0});
        append_row(path, &row);
    }
}

/// One append per row. `writeln!` through serde_json's `Display` issues a write
/// per token, so rows from concurrent threads interleaved mid-line.
fn append_row(path: &Path, row: &serde_json::Value) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = file.write_all(format!("{row}\n").as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Failure: rows marked by concurrent threads (the skill-index worker marks
    // while the UI thread does) interleave mid-line, and the trace the startup
    // suite reads as JSON lines stops parsing.
    #[test]
    fn concurrent_rows_stay_whole_lines() {
        let path = std::env::temp_dir().join(format!("nur-trace-{}.jsonl", uuid::Uuid::new_v4()));
        std::thread::scope(|scope| {
            for thread in 0..8 {
                let path = &path;
                scope.spawn(move || {
                    for i in 0..200 {
                        let row = serde_json::json!({"pid": thread, "stage": format!("stage-{i}"), "ms": i as f64 * 1.5});
                        append_row(path, &row);
                    }
                });
            }
        });
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let rows = text
            .lines()
            .filter(|line| serde_json::from_str::<serde_json::Value>(line).is_ok())
            .count();
        assert_eq!(
            rows,
            8 * 200,
            "{} of {} lines parse",
            rows,
            text.lines().count()
        );
    }
}

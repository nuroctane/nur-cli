//! Opt-in, local startup timings. Stage names contain no paths, prompts, or credentials.
use std::io::Write;
use std::path::PathBuf;
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
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let row = serde_json::json!({"pid": std::process::id(), "stage": stage, "ms": start.elapsed().as_secs_f64() * 1000.0});
            let _ = writeln!(file, "{row}");
        }
    }
}

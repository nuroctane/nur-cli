//! Cross-session memory — simple markdown journal under ~/.nur/memory.md

use crate::config::meta_home;
use std::fs;
use std::path::PathBuf;

pub fn memory_path() -> PathBuf {
    meta_home().join("memory.md")
}

pub fn read_memory() -> String {
    let p = memory_path();
    fs::read_to_string(p).unwrap_or_else(|_| String::from("(empty memory)\n"))
}

pub fn append_memory(note: &str) -> std::io::Result<()> {
    use std::io::Write;
    let p = memory_path();
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = fs::OpenOptions::new().create(true).append(true).open(p)?;
    let ts = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC");
    writeln!(f, "\n### {ts}\n{}\n", note.trim())?;
    Ok(())
}

pub fn memory_prompt_excerpt(max_chars: usize) -> String {
    let m = read_memory();
    if m.trim().is_empty() || m.contains("(empty memory)") {
        return String::new();
    }
    let excerpt: String = m
        .chars()
        .rev()
        .take(max_chars)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("\n# Persistent memory (excerpt from ~/.nur/memory.md)\n{excerpt}\n")
}

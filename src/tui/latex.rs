//! Display-math (`$$...$$`) rendering for the TUI when `image-peek` is enabled.
//!
//! Always on with the `image-peek` feature (locked product decision).
//! Paint path is **cache-only** (never spawns Node/magick on the UI thread).
//! Background warm renders fill the cache after an assistant message completes;
//! peeks then show the PNG via the existing image-peek pipeline.

#![cfg(feature = "image-peek")]

use crate::config::nur_home;
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

static WARM_QUEUE: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();
static WARM_WORKER: OnceLock<()> = OnceLock::new();

fn warm_queue() -> &'static Mutex<VecDeque<String>> {
    WARM_QUEUE.get_or_init(|| Mutex::new(VecDeque::new()))
}

/// Marker left in annotated markdown when a PNG exists (also used by peek).
pub const LATEX_MARK_PREFIX: &str = "<!-- nur-latex:";
pub const LATEX_MARK_SUFFIX: &str = " -->";

/// Rewrite markdown so each `$$...$$` display block may be followed by
/// `<!-- nur-latex:PATH -->` when a **cached** PNG exists.
/// Never spawns renderers - safe on the UI/paint path.
pub fn annotate_display_math(md: &str) -> String {
    annotate_inner(md, false)
}

/// Background: render any missing `$$...$$` PNGs (best-effort). Queued with caps.
pub fn warm_render_async(md: String) {
    if !md.contains("$$") {
        return;
    }
    if let Ok(mut q) = warm_queue().lock() {
        if q.iter().any(|s| s == &md) {
            return;
        }
        // Cap pending work so a long session cannot grow unbounded.
        const MAX_PENDING: usize = 8;
        while q.len() >= MAX_PENDING {
            q.pop_front();
        }
        q.push_back(md);
    }
    WARM_WORKER.get_or_init(|| {
        std::thread::spawn(|| loop {
            let next = warm_queue().lock().ok().and_then(|mut q| q.pop_front());
            match next {
                Some(md) => {
                    let _ = annotate_inner(&md, true);
                }
                None => std::thread::sleep(std::time::Duration::from_millis(50)),
            }
        });
    });
}

/// First cached equation PNG path referenced by markers or derivable from `$$`.
pub fn first_cached_png(md: &str) -> Option<String> {
    if let Some(p) = extract_marked_paths(md).into_iter().next() {
        if is_latex_cache_png(&p) {
            return Some(p);
        }
    }
    for tex in iter_display_math(md) {
        let dest = cache_path_for(&tex);
        if dest.is_file() {
            return Some(dest.to_string_lossy().into_owned());
        }
    }
    None
}

fn is_latex_cache_png(path: &str) -> bool {
    let p = Path::new(path);
    if !p.is_file() {
        return false;
    }
    let Ok(canon) = p.canonicalize() else {
        return false;
    };
    let Ok(root) = cache_dir().canonicalize() else {
        return false;
    };
    canon.starts_with(&root)
}

fn extract_marked_paths(md: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = md;
    while let Some(i) = rest.find(LATEX_MARK_PREFIX) {
        let after = &rest[i + LATEX_MARK_PREFIX.len()..];
        if let Some(end) = after.find(LATEX_MARK_SUFFIX) {
            let path = after[..end].trim();
            if !path.is_empty() {
                out.push(path.to_string());
            }
            rest = &after[end + LATEX_MARK_SUFFIX.len()..];
        } else {
            break;
        }
    }
    out
}

fn annotate_inner(md: &str, allow_render: bool) -> String {
    let mut out = String::with_capacity(md.len());
    let mut rest = md;
    while let Some(start) = rest.find("$$") {
        out.push_str(&rest[..start]);
        let after_open = &rest[start + 2..];
        if let Some(end) = after_open.find("$$") {
            let tex = &after_open[..end];
            out.push_str("$$");
            out.push_str(tex);
            out.push_str("$$");
            if let Some(png) = resolve_png(tex.trim(), allow_render) {
                out.push_str(&format!(
                    "\n{LATEX_MARK_PREFIX}{}{LATEX_MARK_SUFFIX}\n",
                    png.display()
                ));
            }
            rest = &after_open[end + 2..];
        } else {
            out.push_str("$$");
            rest = after_open;
        }
    }
    out.push_str(rest);
    out
}

fn iter_display_math(md: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = md;
    while let Some(start) = rest.find("$$") {
        let after_open = &rest[start + 2..];
        if let Some(end) = after_open.find("$$") {
            let tex = after_open[..end].trim();
            if !tex.is_empty() {
                out.push(tex.to_string());
            }
            rest = &after_open[end + 2..];
        } else {
            break;
        }
    }
    out
}

fn cache_dir() -> PathBuf {
    let d = nur_home().join("cache").join("latex");
    let _ = fs::create_dir_all(&d);
    d
}

fn hash_tex(tex: &str) -> String {
    let digest = Sha256::digest(tex.as_bytes());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

fn cache_path_for(tex: &str) -> PathBuf {
    cache_dir().join(format!("{}.png", hash_tex(tex)))
}

fn resolve_png(tex: &str, allow_render: bool) -> Option<PathBuf> {
    if tex.is_empty() || tex.len() > 4_000 {
        return None;
    }
    let dest = cache_path_for(tex);
    if dest.is_file() {
        return Some(dest);
    }
    if !allow_render {
        return None;
    }
    if try_node_katex(tex, &dest) {
        Some(dest)
    } else {
        None
    }
}

fn try_node_katex(tex: &str, dest: &Path) -> bool {
    let Some(node) = crate::ecosystem::find_bin("node") else {
        return false;
    };
    let svg_path = dest.with_extension("svg");
    let tex_b64 = {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(tex.as_bytes())
    };
    let js = format!(
        "const fs=require('fs');const tex=Buffer.from('{tex_b64}','base64').toString('utf8');\
         let katex;try{{katex=require('katex');}}catch(e){{process.exit(2);}}\
         const html=katex.renderToString(tex,{{throwOnError:false,displayMode:true}});\
         const m=html.match(/<svg[\\s\\S]*?<\\/svg>/);if(!m)process.exit(3);\
         fs.writeFileSync(process.argv[1],m[0]);"
    );
    let mut child = match Command::new(&node)
        .args(["-e", &js])
        .arg(svg_path.to_string_lossy().as_ref())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_file(&svg_path);
                return false;
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(30)),
            Err(_) => return false,
        }
    };
    if !status.success() || !svg_path.is_file() {
        return false;
    }
    if let Some(magick) =
        crate::ecosystem::find_bin("magick").or_else(|| crate::ecosystem::find_bin("convert"))
    {
        let mut child = match Command::new(&magick)
            .arg(svg_path.to_string_lossy().as_ref())
            .arg(dest.to_string_lossy().as_ref())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(c) => c,
            Err(_) => {
                let _ = fs::remove_file(&svg_path);
                return false;
            }
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let ok = loop {
            match child.try_wait() {
                Ok(Some(st)) => break st.success(),
                Ok(None) if std::time::Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break false;
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(30)),
                Err(_) => break false,
            }
        };
        let _ = fs::remove_file(&svg_path);
        return ok && dest.is_file();
    }
    let _ = fs::remove_file(&svg_path);
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annotate_preserves_utf8_without_renderer() {
        let md = "hello café\n$$\nx^2\n$$\nbye";
        let out = annotate_display_math(md);
        assert!(out.contains("café"));
        assert!(out.contains("x^2"));
        assert!(out.contains("$$"));
    }

    #[test]
    fn extract_paths() {
        let md = "a <!-- nur-latex:C:/tmp/a.png --> b";
        let p = extract_marked_paths(md);
        assert_eq!(p, vec!["C:/tmp/a.png".to_string()]);
        // Arbitrary paths must not be accepted as peek targets.
        assert!(first_cached_png(md).is_none());
    }
}

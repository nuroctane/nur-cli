//! Multimodal media tools — bridge vision into Meta Model API.
//!
//! Muse Spark accepts `input_image` / `input_video` on the Responses API.
//! These tools load workspace media into a pending queue; the agent loop
//! attaches them as multimodal content on the next model turn.
//!
//! Efficient design-from-video recipe:
//! 1. `extract_frames` on a short clip (or `look` on the video itself if small)
//! 2. `look` on the keyframes
//! 3. Implement with design-eng skills

use super::{arg_str, arg_u64, resolve_path, Tool, ToolContext};
use crate::error::{MuseError, Result};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

/// Max image bytes we'll base64-attach (after file read).
const MAX_IMAGE_BYTES: u64 = 5 * 1024 * 1024;
/// Max video bytes for native `input_video` attach (short clips).
const MAX_VIDEO_BYTES: u64 = 20 * 1024 * 1024;
/// Hard cap on frames extracted per call.
const MAX_FRAMES: u32 = 12;
/// Max media items pending for one model turn.
const MAX_PENDING: usize = 10;

#[derive(Debug, Clone)]
pub struct MediaAttach {
    pub kind: MediaKind,
    pub path: String,
    pub data_url: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Video,
}

impl MediaKind {
    pub fn api_type(self) -> &'static str {
        match self {
            Self::Image => "input_image",
            Self::Video => "input_video",
        }
    }

    pub fn url_field(self) -> &'static str {
        match self {
            Self::Image => "image_url",
            Self::Video => "video_url",
        }
    }
}

static PENDING: Mutex<Vec<MediaAttach>> = Mutex::new(Vec::new());

/// Drain pending media attachments for the agent loop (order preserved).
pub fn take_pending_media() -> Vec<MediaAttach> {
    PENDING
        .lock()
        .map(|mut g| std::mem::take(&mut *g))
        .unwrap_or_default()
}

fn push_pending(item: MediaAttach) -> Result<()> {
    let mut g = PENDING
        .lock()
        .map_err(|_| MuseError::Tool("media queue lock".into()))?;
    if g.len() >= MAX_PENDING {
        return Err(MuseError::Tool(format!(
            "too many media attachments this turn (max {MAX_PENDING}) — look at fewer files"
        )));
    }
    g.push(item);
    Ok(())
}

/// MIME + kind for media that can be **attached** to the Responses API.
///
/// Per the Meta Model API docs, `input_image` supports png/jpeg/gif/webp/ico
/// and `input_video` supports **mp4 only**. Other video containers are not
/// attachable directly — run `extract_frames` on them to get JPEG stills.
fn mime_for(path: &Path) -> Result<(&'static str, MediaKind)> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" => Ok(("image/png", MediaKind::Image)),
        "jpg" | "jpeg" => Ok(("image/jpeg", MediaKind::Image)),
        "gif" => Ok(("image/gif", MediaKind::Image)),
        "webp" => Ok(("image/webp", MediaKind::Image)),
        "ico" => Ok(("image/x-icon", MediaKind::Image)),
        "mp4" | "m4v" => Ok(("video/mp4", MediaKind::Video)),
        // ffmpeg can read these, but the API can't attach them directly.
        "webm" | "mov" | "mkv" | "avi" | "wmv" | "flv" | "mpeg" | "mpg" => {
            Err(MuseError::Tool(format!(
                "'.{ext}' video can't be attached directly (Meta input_video supports mp4 only) — \
                 run extract_frames on it, then look at the JPEG stills"
            )))
        }
        _ => Err(MuseError::Tool(format!(
            "unsupported media extension '.{ext}' — images png/jpg/webp/gif/ico, or mp4 video"
        ))),
    }
}

/// True for any video container ffmpeg can decode — the input set for
/// `extract_frames` (broader than what the API can attach directly).
fn is_extractable_video(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "mp4"
            | "m4v"
            | "webm"
            | "mov"
            | "mkv"
            | "avi"
            | "wmv"
            | "flv"
            | "mpeg"
            | "mpg"
            | "ts"
            | "3gp"
    )
}

/// Load a workspace media file into a data URL (and pending queue if push=true).
pub fn load_media(path: &Path, push: bool) -> Result<MediaAttach> {
    if !path.is_file() {
        return Err(MuseError::Tool(format!("not a file: {}", path.display())));
    }
    let meta = fs::metadata(path).map_err(|e| MuseError::Tool(e.to_string()))?;
    let (mime, kind) = mime_for(path)?;
    let max = match kind {
        MediaKind::Image => MAX_IMAGE_BYTES,
        MediaKind::Video => MAX_VIDEO_BYTES,
    };
    if meta.len() > max {
        return Err(MuseError::Tool(format!(
            "{} is too large ({:.1} MB; max {:.0} MB for {:?}). \
             For video, use extract_frames then look on the stills.",
            path.display(),
            meta.len() as f64 / (1024.0 * 1024.0),
            max as f64 / (1024.0 * 1024.0),
            kind
        )));
    }
    let bytes = fs::read(path).map_err(|e| MuseError::Tool(format!("read media: {e}")))?;
    let b64 = base64_encode(&bytes);
    let data_url = format!("data:{mime};base64,{b64}");
    let item = MediaAttach {
        kind,
        path: path.display().to_string(),
        data_url,
        bytes: meta.len(),
    };
    if push {
        push_pending(item.clone())?;
    }
    Ok(item)
}

/// Scan free text for workspace media paths and queue them (user-prompt auto-attach).
pub fn auto_attach_from_text(cwd: &Path, text: &str) -> Vec<String> {
    let mut notes = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for token in text.split_whitespace() {
        let cleaned = token.trim_matches(|c: char| {
            matches!(
                c,
                '"' | '\'' | '`' | ',' | ';' | ')' | '(' | '[' | ']' | '{' | '}'
            )
        });
        if cleaned.is_empty() {
            continue;
        }
        let candidate = if Path::new(cleaned).is_absolute() {
            PathBuf::from(cleaned)
        } else {
            cwd.join(cleaned)
        };
        let Ok(canon) = candidate.canonicalize() else {
            continue;
        };
        if !canon.is_file() {
            continue;
        }
        if mime_for(&canon).is_err() {
            continue;
        }
        let key = canon.display().to_string();
        if !seen.insert(key.clone()) {
            continue;
        }
        match load_media(&canon, true) {
            Ok(m) => notes.push(format!(
                "auto-attached {} ({:.0} KB, {:?})",
                m.path,
                m.bytes as f64 / 1024.0,
                m.kind
            )),
            Err(e) => notes.push(format!("skip media {key}: {e}")),
        }
        if notes.len() >= MAX_PENDING {
            break;
        }
    }
    notes
}

fn base64_encode(data: &[u8]) -> String {
    // Standard base64 without padding issues — minimal impl to avoid new deps.
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(T[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(T[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn find_ffmpeg() -> Option<PathBuf> {
    for name in ["ffmpeg", "ffmpeg.exe"] {
        if let Ok(out) = Command::new(if cfg!(windows) { "where" } else { "which" })
            .arg(name)
            .output()
        {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout);
                if let Some(line) = s.lines().next() {
                    let p = PathBuf::from(line.trim());
                    if p.is_file() {
                        return Some(p);
                    }
                }
            }
        }
    }
    // Common Windows winget shim
    if let Some(home) = dirs::home_dir() {
        let p = home
            .join("AppData")
            .join("Local")
            .join("Microsoft")
            .join("WinGet")
            .join("Links")
            .join("ffmpeg.exe");
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

// ── tools ─────────────────────────────────────────────────────────────────

/// Attach image(s) or a short video so Muse can *see* them on the next turn.
pub struct Look;

impl Tool for Look {
    fn name(&self) -> &str {
        "look"
    }

    fn description(&self) -> &str {
        "Give the model vision over workspace media. path = image (png/jpg/webp/gif/ico) \
         or a short mp4 video (max ~20MB). Other video containers (webm/mov/mkv/…) can't be \
         attached directly — run extract_frames on them and look at the JPEG stills. Images \
         attach as Responses input_image, mp4 as input_video. Returns a short confirmation — \
         the pixels are sent on the next model turn (not as text)."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Workspace path to image or short video"
                },
                "paths": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Optional batch of images (max 8 total with path)"
                },
                "note": {
                    "type": "string",
                    "description": "Optional focus note e.g. 'extract color tokens and type scale'"
                }
            }
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let mut paths: Vec<String> = Vec::new();
        if let Ok(p) = arg_str(args, "path") {
            if !p.trim().is_empty() {
                paths.push(p);
            }
        }
        if let Some(arr) = args.get("paths").and_then(|v| v.as_array()) {
            for v in arr {
                if let Some(s) = v.as_str() {
                    if !s.trim().is_empty() {
                        paths.push(s.to_string());
                    }
                }
            }
        }
        if paths.is_empty() {
            return Err(MuseError::Tool("look requires path or paths".into()));
        }
        if paths.len() > 8 {
            return Err(MuseError::Tool("look: max 8 media files per call".into()));
        }
        let note = args
            .get("note")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let mut lines = Vec::new();
        for p in paths {
            let full = resolve_path(&ctx.cwd, &p)?;
            let m = load_media(&full, true)?;
            lines.push(format!(
                "attached {} · {} · {:.0} KB · will be visible next model turn",
                m.path,
                m.kind.api_type(),
                m.bytes as f64 / 1024.0
            ));
        }
        if !note.is_empty() {
            lines.push(format!("note: {note}"));
        }
        lines.push(
            "Tip: for design systems, describe tokens (color, type, radius, motion) then implement."
                .into(),
        );
        Ok(lines.join("\n"))
    }
}

/// Extract sparse keyframes from a video with ffmpeg (efficient vs frame-by-frame).
pub struct ExtractFrames;

impl Tool for ExtractFrames {
    fn name(&self) -> &str {
        "extract_frames"
    }

    fn description(&self) -> &str {
        "Extract a sparse set of still frames from a video (ffmpeg). Default ~1 fps, \
         capped (default 8). Writes JPEGs under .nur/frames/<stem>/ and returns paths. \
         Prefer this over looking at every frame. Then call look on the paths (or the \
         whole dir listing) for vision. Requires ffmpeg on PATH."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Video file in workspace"},
                "fps": {
                    "type": "number",
                    "description": "Frames per second to sample (default 1.0)"
                },
                "max_frames": {
                    "type": "integer",
                    "description": "Hard cap (default 8, max 12)"
                },
                "auto_look": {
                    "type": "boolean",
                    "description": "If true, also queue frames for vision (default true)"
                }
            },
            "required": ["path"]
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let path = arg_str(args, "path")?;
        let full = resolve_path(&ctx.cwd, &path)?;
        if !full.is_file() {
            return Err(MuseError::Tool(format!(
                "video not found: {}",
                full.display()
            )));
        }
        if !is_extractable_video(&full) {
            return Err(MuseError::Tool(
                "extract_frames expects a video file (mp4/webm/mov/mkv/avi/…)".into(),
            ));
        }
        let ffmpeg = find_ffmpeg().ok_or_else(|| {
            MuseError::Tool(
                "ffmpeg not found on PATH — install ffmpeg to extract frames \
                 (https://ffmpeg.org), or use look on a short video under 20MB"
                    .into(),
            )
        })?;

        let fps = args
            .get("fps")
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0)
            .clamp(0.2, 5.0);
        let max_frames = arg_u64(args, "max_frames")
            .unwrap_or(8)
            .clamp(1, MAX_FRAMES as u64) as u32;
        let auto_look = args
            .get("auto_look")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let stem = full.file_stem().and_then(|s| s.to_str()).unwrap_or("video");
        let safe: String = stem
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let out_dir = ctx.cwd.join(".meta").join("frames").join(&safe);
        let _ = fs::remove_dir_all(&out_dir);
        fs::create_dir_all(&out_dir).map_err(|e| MuseError::Tool(e.to_string()))?;

        let pattern = out_dir.join("frame_%02d.jpg");
        // Scale down wide frames so vision tokens stay reasonable.
        let vf = format!("fps={fps},scale='min(1280,iw)':-2");
        let status = Command::new(&ffmpeg)
            .args([
                "-y",
                "-i",
                &full.to_string_lossy(),
                "-vf",
                &vf,
                "-frames:v",
                &max_frames.to_string(),
                "-q:v",
                "3",
                &pattern.to_string_lossy(),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .status()
            .map_err(|e| MuseError::Tool(format!("ffmpeg spawn failed: {e}")))?;

        if !status.success() {
            return Err(MuseError::Tool(format!(
                "ffmpeg failed (exit {:?}) extracting frames from {}",
                status.code(),
                full.display()
            )));
        }

        let mut frames: Vec<PathBuf> = fs::read_dir(&out_dir)
            .map_err(|e| MuseError::Tool(e.to_string()))?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg"))
                    .unwrap_or(false)
            })
            .collect();
        frames.sort();
        if frames.is_empty() {
            return Err(MuseError::Tool(
                "ffmpeg produced no frames — is the video valid?".into(),
            ));
        }

        let mut lines = vec![format!(
            "extracted {} frame(s) from {} → {}",
            frames.len(),
            full.display(),
            out_dir.display()
        )];
        for f in &frames {
            let rel = f
                .strip_prefix(&ctx.cwd)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| f.display().to_string());
            lines.push(format!("  · {rel}"));
            if auto_look {
                match load_media(f, true) {
                    Ok(m) => lines.push(format!(
                        "    queued for vision ({:.0} KB)",
                        m.bytes as f64 / 1024.0
                    )),
                    Err(e) => lines.push(format!("    look skipped: {e}")),
                }
            }
        }
        if auto_look {
            lines.push(
                "Frames queued for vision — next model turn can see them. \
                 Extract design tokens, then implement (skill design-eng)."
                    .into(),
            );
        } else {
            lines.push("Call look with these paths to attach vision.".into());
        }
        Ok(lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_detects_common() {
        assert_eq!(mime_for(Path::new("x.PNG")).unwrap().1, MediaKind::Image);
        assert_eq!(mime_for(Path::new("a.mp4")).unwrap().1, MediaKind::Video);
        assert!(mime_for(Path::new("x.txt")).is_err());
    }

    #[test]
    fn attach_matches_api_support() {
        // Images the API accepts.
        for ok in ["a.png", "b.jpg", "c.jpeg", "d.gif", "e.webp", "f.ico"] {
            assert_eq!(mime_for(Path::new(ok)).unwrap().1, MediaKind::Image, "{ok}");
        }
        // Only mp4 attaches as input_video.
        assert_eq!(mime_for(Path::new("v.mp4")).unwrap().1, MediaKind::Video);
        // Unsupported directly: bmp image + non-mp4 video → error (steer to extract_frames).
        for bad in ["x.bmp", "clip.webm", "clip.mov", "clip.mkv", "clip.avi"] {
            assert!(mime_for(Path::new(bad)).is_err(), "{bad} should not attach");
        }
    }

    #[test]
    fn extractable_video_is_broad() {
        for v in [
            "clip.mp4",
            "clip.webm",
            "clip.mov",
            "clip.mkv",
            "clip.avi",
            "clip.mpg",
        ] {
            assert!(is_extractable_video(Path::new(v)), "{v}");
        }
        assert!(!is_extractable_video(Path::new("photo.png")));
    }

    #[test]
    fn base64_roundtrip_length() {
        let s = base64_encode(b"hi");
        // "hi" → aGk=
        assert_eq!(s, "aGk=");
    }

    #[test]
    fn pending_queue_drains() {
        let _ = take_pending_media();
        // tiny 1x1 png
        let png: &[u8] = &[
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00,
            0x00, 0x90, 0x77, 0x53, 0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x08,
            0xd7, 0x63, 0xf8, 0xcf, 0xc0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x00, 0x05, 0xfe,
            0xd4, 0xef, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ];
        let dir = std::env::temp_dir().join(format!(
            "meta-media-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.png");
        fs::write(&p, png).unwrap();
        load_media(&p, true).unwrap();
        let got = take_pending_media();
        assert_eq!(got.len(), 1);
        assert!(got[0].data_url.starts_with("data:image/png;base64,"));
        let _ = fs::remove_dir_all(dir);
    }
}

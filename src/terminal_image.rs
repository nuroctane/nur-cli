//! Inline terminal images: one engine, two thin painters (the TUI transcript
//! and the plain `nur run` output), in the shape Hermes Agent ships.
//!
//! - **Half-blocks** are this module's own: one cell is two pixel rows, `▀`
//!   with the top pixel as foreground and the bottom as background. They
//!   are ordinary text, so the transcript measures, scrolls, wraps and
//!   re-lays them out like any other row, in every terminal.
//! - **Pixels** (kitty, sixel, iTerm2) come from `ratatui-image`, the same
//!   encoder the F4 peek uses. The TUI draws them over the half-block rows
//!   while the image is fully on screen; the CLI prints them in place.
//!
//! Decoding never happens on the UI thread: [`request`] answers from the
//! cache or queues the file for a background worker, and bumps
//! [`cache_revision`] when a render lands so the transcript re-wraps.
//! Sizes come from the file header first (cheap), with EXIF orientation
//! applied, a 20 MB file cap and a 40 MP pixel cap before any decode.
//!
//! Detection is environment-only: no terminal query that could hang a pipe
//! or land as typed text (`NUR_IMAGE_QUERY=1` stays the opt-in for that).

#![cfg(feature = "image-peek")]

use image::{DynamicImage, ImageDecoder, ImageReader, RgbaImage};
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::Protocol;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::UNIX_EPOCH;

/// Hermes's caps: an inline image is at most 64 columns by 16 rows.
pub const MAX_COLS: u16 = 64;
pub const MAX_ROWS: u16 = 16;
const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
const MAX_PIXELS: u64 = 40_000_000;
/// A pixel this transparent (alpha) is the terminal background.
const ALPHA_CUTOFF: u8 = 32;
/// Finished renders kept (least recently used out). A transcript keeps its
/// half-block rows after eviction, so this bounds memory, not what shows.
const CACHE_ENTRIES: usize = 96;
/// Image files the painters draw. Remote URLs are never fetched for display.
pub const EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp"];

// ── geometry ──────────────────────────────────────────────────────────────

/// The cell box an image occupies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Grid {
    pub cols: u16,
    pub rows: u16,
}

/// Fit `w`×`h` pixels into at most `max_cols`×`max_rows` cells of
/// `font_w`×`font_h` pixels, keeping the aspect ratio. Never upscales: a
/// small icon stays small.
pub fn fit(w: u32, h: u32, font: (u16, u16), max_cols: u16, max_rows: u16) -> Grid {
    let (fw, fh) = (font.0.max(1) as f64, font.1.max(1) as f64);
    let (w, h) = (w.max(1) as f64, h.max(1) as f64);
    let scale = (max_cols.max(1) as f64 * fw / w)
        .min(max_rows.max(1) as f64 * fh / h)
        .min(1.0);
    Grid {
        cols: ((w * scale / fw).round() as u16).clamp(1, max_cols.max(1)),
        rows: ((h * scale / fh).round() as u16).clamp(1, max_rows.max(1)),
    }
}

/// Header size with EXIF orientation applied (90° turns swap the sides).
/// Reads no pixels.
pub fn dimensions(path: &Path) -> Result<(u32, u32), String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("not a file".into());
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(format!("over {} MB", MAX_FILE_BYTES / (1024 * 1024)));
    }
    let mut decoder = ImageReader::open(path)
        .and_then(|r| r.with_guessed_format())
        .map_err(|e| e.to_string())?
        .into_decoder()
        .map_err(|e| e.to_string())?;
    let (w, h) = decoder.dimensions();
    if u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(format!("{w}x{h} is over {} MP", MAX_PIXELS / 1_000_000));
    }
    let turned = matches!(
        decoder.orientation(),
        Ok(image::metadata::Orientation::Rotate90
            | image::metadata::Orientation::Rotate270
            | image::metadata::Orientation::Rotate90FlipH
            | image::metadata::Orientation::Rotate270FlipH)
    );
    Ok(if turned { (h, w) } else { (w, h) })
}

/// Decode with the caps checked first and EXIF orientation applied.
pub fn load(path: &Path) -> Result<DynamicImage, String> {
    dimensions(path)?;
    let mut decoder = ImageReader::open(path)
        .and_then(|r| r.with_guessed_format())
        .map_err(|e| e.to_string())?
        .into_decoder()
        .map_err(|e| e.to_string())?;
    let orientation = decoder.orientation().ok();
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    Ok(img)
}

// ── half-blocks ───────────────────────────────────────────────────────────

/// One cell: the top and bottom pixel, `None` where transparent.
pub type HalfCell = (Option<[u8; 3]>, Option<[u8; 3]>);

/// The image as `grid.rows` rows of `grid.cols` half-block cells.
pub fn halfblocks(img: &DynamicImage, grid: Grid) -> Vec<Vec<HalfCell>> {
    let (cols, px_rows) = (u32::from(grid.cols), u32::from(grid.rows) * 2);
    // A cheap box filter to a few times the target first, then Lanczos:
    // a 4K screenshot straight through Lanczos would cost a second.
    let small = if img.width() > cols * 4 || img.height() > px_rows * 4 {
        img.thumbnail(cols * 4, px_rows * 4)
    } else {
        img.clone()
    };
    let rgba: RgbaImage = small
        .resize_exact(cols, px_rows, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    let px = |x: u32, y: u32| {
        let p = rgba.get_pixel(x, y).0;
        (p[3] > ALPHA_CUTOFF).then_some([p[0], p[1], p[2]])
    };
    (0..grid.rows as u32)
        .map(|row| {
            (0..cols)
                .map(|x| (px(x, row * 2), px(x, row * 2 + 1)))
                .collect()
        })
        .collect()
}

/// The glyph and its (fg, bg) for one cell. One opaque half keeps the other
/// half the terminal's own background, so transparent edges get no box.
pub fn half_glyph(cell: HalfCell) -> (char, Option<[u8; 3]>, Option<[u8; 3]>) {
    match cell {
        (Some(t), Some(b)) => ('▀', Some(t), Some(b)),
        (Some(t), None) => ('▀', Some(t), None),
        (None, Some(b)) => ('▄', Some(b), None),
        (None, None) => (' ', None, None),
    }
}

/// One half-block row as truecolor ANSI, reset at the end.
pub fn ansi_row(row: &[HalfCell]) -> String {
    let mut s = String::new();
    for &cell in row {
        let (ch, fg, bg) = half_glyph(cell);
        match fg {
            Some([r, g, b]) => s.push_str(&format!("\x1b[38;2;{r};{g};{b}m")),
            None => s.push_str("\x1b[39m"),
        }
        match bg {
            Some([r, g, b]) => s.push_str(&format!("\x1b[48;2;{r};{g};{b}m")),
            None => s.push_str("\x1b[49m"),
        }
        s.push(ch);
    }
    s.push_str("\x1b[0m");
    s
}

// ── detection ─────────────────────────────────────────────────────────────

/// The environment detection reads, so it can be tested without touching
/// the process environment.
#[derive(Debug, Default, Clone)]
pub struct Env {
    pub term: String,
    pub term_program: String,
    pub lc_terminal: String,
    pub kitty_window: bool,
    pub ghostty: bool,
    pub iterm_session: bool,
    pub wezterm: bool,
    pub konsole: bool,
    pub tmux: bool,
    pub screen: bool,
    pub windows_terminal: bool,
    pub orca: bool,
}

impl Env {
    pub fn current() -> Self {
        let get = |k: &str| std::env::var(k).unwrap_or_default();
        let set = |k: &str| std::env::var(k).is_ok_and(|v| !v.is_empty());
        Self {
            term: get("TERM").to_ascii_lowercase(),
            term_program: get("TERM_PROGRAM").to_ascii_lowercase(),
            lc_terminal: get("LC_TERMINAL").to_ascii_lowercase(),
            kitty_window: set("KITTY_WINDOW_ID"),
            ghostty: set("GHOSTTY_RESOURCES_DIR"),
            iterm_session: set("ITERM_SESSION_ID"),
            wezterm: set("WEZTERM_PANE") || set("WEZTERM_EXECUTABLE"),
            konsole: set("KONSOLE_VERSION"),
            tmux: set("TMUX"),
            screen: set("STY"),
            windows_terminal: set("WT_SESSION"),
            // Orca (Electron + xterm.js) marks its panes this way and clears
            // TERM_PROGRAM; read from its app bundle, Nov 2026.
            orca: set("ORCA_PANE_KEY") || set("ORCA_TERMINAL_HANDLE") || set("ORCA_TAB_ID"),
        }
    }
}

/// Which pixel protocol a painter should use, from the environment alone.
/// `overlays` is whether the painter can reserve rows and draw over them
/// (the CLI can; a TUI cannot, so it never gets iTerm2 or sixel this way).
pub fn detect(env: &Env, overlays: bool) -> ProtocolType {
    let prog = env.term_program.as_str();
    // VS Code / Cursor draw images only with a setting nothing in the
    // environment reveals, and pass an inherited KITTY_WINDOW_ID along;
    // trusting either is a blank frame. Orca's xterm.js does not place
    // kitty placeholders. Multiplexers swallow the transmit.
    if prog == "vscode" || env.orca || env.tmux || env.screen {
        return ProtocolType::Halfblocks;
    }
    let kitty = env.kitty_window
        || env.ghostty
        || env.term.contains("kitty")
        || env.term.contains("ghostty")
        || prog == "ghostty";
    // WezTerm speaks the kitty APC but paints its placeholders as tofu.
    let wezterm = env.wezterm || prog == "wezterm";
    if kitty && !wezterm {
        return ProtocolType::Kitty;
    }
    let iterm = prog == "iterm.app"
        || env.iterm_session
        || env.lc_terminal.contains("iterm")
        || wezterm
        || env.konsole;
    if overlays && iterm {
        return ProtocolType::Iterm2;
    }
    ProtocolType::Halfblocks
}

/// A pinned protocol from `NUR_IMAGE_PROTOCOL` or `[theme] protocol`
/// (`auto` and unknown values pin nothing).
pub fn forced(config_protocol: &str) -> Option<ProtocolType> {
    let raw = std::env::var("NUR_IMAGE_PROTOCOL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| config_protocol.to_string());
    match raw.trim().to_ascii_lowercase().as_str() {
        "kitty" => Some(ProtocolType::Kitty),
        "sixel" => Some(ProtocolType::Sixel),
        "iterm2" | "iterm" => Some(ProtocolType::Iterm2),
        "halfblocks" | "unicode" => Some(ProtocolType::Halfblocks),
        _ => None,
    }
}

/// One line for `nur doctor`: the mode inline images use here, and how to
/// get real pixels when the terminal could draw them but nothing says so.
pub fn doctor_line(config_protocol: &str, enabled: bool) -> String {
    if !enabled {
        return "off ([theme] inline_images = false)".into();
    }
    let env = Env::current();
    let (mode, pinned) = match forced(config_protocol) {
        Some(p) => (p, true),
        None => (detect(&env, true), false),
    };
    let name = match mode {
        ProtocolType::Kitty => "kitty",
        ProtocolType::Sixel => "sixel",
        ProtocolType::Iterm2 => "iterm2",
        ProtocolType::Halfblocks => "half-blocks",
    };
    let mut line = format!("{name}{}", if pinned { " (pinned)" } else { "" });
    if !pinned && mode == ProtocolType::Halfblocks && env.windows_terminal && !env.tmux {
        line.push_str(
            " - for real pixels in Windows Terminal 1.22+: Settings > profile > Advanced > \
             enable Sixel, then set [theme] protocol = \"sixel\" (or NUR_IMAGE_PROTOCOL=sixel)",
        );
    } else if !pinned && (env.tmux || env.screen) {
        line.push_str(
            " - multiplexers swallow image transmits; pin a protocol only with passthrough on",
        );
    }
    line
}

/// The protocol the TUI may overlay inline: kitty (placeholders are text,
/// so scrolling rewrites them) or sixel (text overwrites its pixels), never
/// where the environment says the transmit is swallowed or misplaced.
/// iTerm2 images sit on their own layer and would linger when the
/// transcript scrolls, so they stay in the F4 peek.
pub fn overlay_protocol(picked: ProtocolType, env: &Env) -> ProtocolType {
    let hazard = env.tmux
        || env.screen
        || env.orca
        || env.term_program == "vscode"
        || env.wezterm
        || env.term_program == "wezterm";
    match picked {
        ProtocolType::Kitty | ProtocolType::Sixel if !hazard => picked,
        _ => ProtocolType::Halfblocks,
    }
}

/// The TUI's inline picker: the peek picker's font size, with the protocol
/// chosen by the engine's own ladder (or the pinned one) and then limited by
/// [`overlay_protocol`]. A pinned kitty under tmux or Orca still draws
/// half-blocks inline: the transmit would be swallowed or misplaced.
pub fn overlay_picker(picker: &Picker, config_protocol: &str) -> Picker {
    let env = Env::current();
    // NUR_IMAGE_QUERY=1 asked the terminal itself: its answer beats the
    // environment ladder.
    let queried = std::env::var("NUR_IMAGE_QUERY").is_ok_and(|v| !v.is_empty() && v != "0");
    let chosen = forced(config_protocol).unwrap_or_else(|| {
        if queried {
            picker.protocol_type()
        } else {
            detect(&env, false)
        }
    });
    let mut p = picker.clone();
    p.set_protocol_type(overlay_protocol(chosen, &env));
    p
}

// ── TUI settings (set once at startup) ────────────────────────────────────

struct Settings {
    enabled: bool,
    picker: Option<Picker>,
}

fn settings() -> &'static RwLock<Settings> {
    static S: OnceLock<RwLock<Settings>> = OnceLock::new();
    S.get_or_init(|| {
        RwLock::new(Settings {
            enabled: false,
            picker: None,
        })
    })
}

/// The TUI hands over its picker (protocol + font size) and the
/// `inline_images` switch. Until then inline images stay compact rows.
pub fn configure(enabled: bool, picker: Option<Picker>) {
    if let Ok(mut s) = settings().write() {
        s.enabled = enabled;
        s.picker = picker;
    }
    SETTINGS_REVISION.fetch_add(1, Ordering::Relaxed);
    bump();
}

static SETTINGS_REVISION: AtomicU64 = AtomicU64::new(0);

/// Bumped when the inline switch or the picker changes: every picture cell
/// re-lays itself out (a finished render bumps only [`cache_revision`]).
pub fn settings_revision() -> u64 {
    SETTINGS_REVISION.load(Ordering::Relaxed)
}

pub fn enabled() -> bool {
    settings().read().map(|s| s.enabled).unwrap_or(false)
}

fn picker() -> Option<Picker> {
    settings().read().ok().and_then(|s| s.picker.clone())
}

/// Whether the TUI overlays real pixels (any protocol but half-blocks).
pub fn pixels() -> bool {
    picker().is_some_and(|p| p.protocol_type() != ProtocolType::Halfblocks)
}

// ── cache + background worker ─────────────────────────────────────────────

/// A finished render.
pub struct Rendered {
    pub grid: Grid,
    pub cells: Vec<Vec<HalfCell>>,
    /// The terminal-protocol image for the overlay, when a pixel protocol
    /// is active (built off the UI thread; kitty transmits once).
    pub pixels: Option<Mutex<Protocol>>,
}

pub enum State {
    /// Queued or rendering; the grid is known from the header.
    Pending(Grid),
    Ready(Arc<Rendered>),
    /// Not drawable (missing, too large, unreadable); say why in a row.
    Failed(String),
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    path: PathBuf,
    mtime: u128,
    len: u64,
    max_cols: u16,
    max_rows: u16,
    proto: u8,
}

#[derive(Default)]
struct Cache {
    done: HashMap<Key, Arc<Rendered>>,
    order: VecDeque<Key>,
    failed: HashMap<Key, String>,
    grids: HashMap<Key, Grid>,
    queue: VecDeque<Key>,
    /// Popped by the worker and still rendering: not queued again.
    in_flight: Option<Key>,
    worker: bool,
}

/// Side maps grow by one entry per width a window is dragged through.
const SIDE_ENTRIES: usize = 512;

fn cache() -> &'static Mutex<Cache> {
    static C: OnceLock<Mutex<Cache>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(Cache::default()))
}

static REVISION: AtomicU64 = AtomicU64::new(0);

/// Bumped whenever a render lands or the settings change; the transcript
/// folds it into its re-wrap check.
pub fn cache_revision() -> u64 {
    REVISION.load(Ordering::Relaxed)
}

fn bump() {
    REVISION.fetch_add(1, Ordering::Relaxed);
}

fn proto_code(p: &Option<Picker>) -> u8 {
    match p.as_ref().map(|p| p.protocol_type()) {
        None | Some(ProtocolType::Halfblocks) => 0,
        Some(ProtocolType::Kitty) => 1,
        Some(ProtocolType::Sixel) => 2,
        Some(ProtocolType::Iterm2) => 3,
    }
}

/// `(mtime, len)` of a file, re-read at most every 750 ms: the overlay asks
/// every frame, and a stat per visible picture per frame adds up on NTFS.
fn file_stamp(path: &Path) -> Option<(u128, u64)> {
    type Stamps = HashMap<PathBuf, (std::time::Instant, Option<(u128, u64)>)>;
    static STAMPS: OnceLock<Mutex<Stamps>> = OnceLock::new();
    let stamps = STAMPS.get_or_init(Default::default);
    let mut g = stamps.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, stamp)) = g.get(path) {
        if at.elapsed() < std::time::Duration::from_millis(750) {
            return *stamp;
        }
    }
    let stamp = std::fs::metadata(path)
        .ok()
        .filter(|m| m.is_file())
        .map(|meta| {
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            (mtime, meta.len())
        });
    if g.len() > SIDE_ENTRIES {
        g.clear();
    }
    g.insert(path.to_path_buf(), (std::time::Instant::now(), stamp));
    stamp
}

fn key_for(path: &Path, max_cols: u16, max_rows: u16, picker: &Option<Picker>) -> Option<Key> {
    let (mtime, len) = file_stamp(path)?;
    Some(Key {
        path: path.to_path_buf(),
        mtime,
        len,
        max_cols,
        max_rows,
        proto: proto_code(picker),
    })
}

fn font(picker: &Option<Picker>) -> (u16, u16) {
    picker
        .as_ref()
        .map(|p| {
            let f = p.font_size();
            (f.width, f.height)
        })
        // Nominal cell, as Hermes assumes: 8x16 px.
        .unwrap_or((8, 16))
}

/// Render now, on this thread (the CLI painter, and the worker).
pub fn render_now(
    path: &Path,
    max_cols: u16,
    max_rows: u16,
    picker: Option<&Picker>,
) -> Result<Rendered, String> {
    let picker = picker.cloned();
    let img = load(path)?;
    let grid = fit(img.width(), img.height(), font(&picker), max_cols, max_rows);
    let cells = halfblocks(&img, grid);
    let pixels = match &picker {
        Some(p) if p.protocol_type() != ProtocolType::Halfblocks => p
            .new_protocol(
                img,
                ratatui::layout::Size::new(grid.cols, grid.rows),
                ratatui_image::Resize::Fit(Some(image::imageops::FilterType::Lanczos3)),
            )
            .ok()
            .map(Mutex::new),
        _ => None,
    };
    Ok(Rendered {
        grid,
        cells,
        pixels,
    })
}

/// The TUI's question: what to draw for `path` at most `max_cols` wide.
/// Never decodes on the calling thread.
pub fn request(path: &Path, max_cols: u16) -> State {
    let picker = picker();
    let max_cols = max_cols.clamp(1, MAX_COLS);
    let Some(key) = key_for(path, max_cols, MAX_ROWS, &picker) else {
        return State::Failed("file not found".into());
    };
    {
        let mut c = cache().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(r) = c.done.get(&key).cloned() {
            // Least recently used: a hit moves to the back of the line.
            if let Some(at) = c.order.iter().position(|k| *k == key) {
                if let Some(k) = c.order.remove(at) {
                    c.order.push_back(k);
                }
            }
            return State::Ready(r);
        }
        if let Some(why) = c.failed.get(&key) {
            return State::Failed(why.clone());
        }
    }
    // The header read happens outside the lock, so the worker storing a
    // render never waits on a slow disk here.
    let known = cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .grids
        .get(&key)
        .copied();
    let measured = match known {
        Some(_) => None,
        None => Some(dimensions(path)),
    };
    let mut c = cache().lock().unwrap_or_else(|e| e.into_inner());
    let grid = match known {
        Some(g) => g,
        None => match measured.unwrap_or_else(|| Err("unreadable".into())) {
            Ok((w, h)) => {
                let g = fit(w, h, font(&picker), max_cols, MAX_ROWS);
                if c.grids.len() > SIDE_ENTRIES {
                    c.grids.clear();
                }
                c.grids.insert(key.clone(), g);
                g
            }
            Err(why) => {
                if c.failed.len() > SIDE_ENTRIES {
                    c.failed.clear();
                }
                c.failed.insert(key, why.clone());
                return State::Failed(why);
            }
        },
    };
    if c.in_flight.as_ref() != Some(&key) && !c.queue.contains(&key) {
        c.queue.push_back(key);
    }
    if !c.worker {
        c.worker = true;
        std::thread::spawn(worker);
    }
    State::Pending(grid)
}

fn worker() {
    loop {
        let next = {
            let mut c = cache().lock().unwrap_or_else(|e| e.into_inner());
            match c.queue.pop_front() {
                Some(k) => {
                    c.in_flight = Some(k.clone());
                    k
                }
                None => {
                    c.worker = false;
                    return;
                }
            }
        };
        // The same picker as `request` (its font sizes the grid either way),
        // so a reserved block and its render have the same height. A decoder
        // that panics on a malformed file fails that one picture, not the
        // worker every later picture waits on.
        let picker = picker();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            render_now(&next.path, next.max_cols, next.max_rows, picker.as_ref())
        }))
        .unwrap_or_else(|_| Err("the decoder failed on this file".into()));
        let mut c = cache().lock().unwrap_or_else(|e| e.into_inner());
        c.in_flight = None;
        match result {
            Ok(r) => {
                if c.done.len() >= CACHE_ENTRIES {
                    if let Some(old) = c.order.pop_front() {
                        c.done.remove(&old);
                    }
                }
                if !c.done.contains_key(&next) {
                    c.order.push_back(next.clone());
                }
                c.done.insert(next, Arc::new(r));
            }
            Err(why) => {
                c.failed.insert(next, why);
            }
        }
        drop(c);
        bump();
    }
}

// ── the plain CLI painter ─────────────────────────────────────────────────

/// One cell of a rendered buffer as ANSI: its colours, then its symbol.
fn ansi_cell(cell: &ratatui::buffer::Cell) -> String {
    use ratatui::style::Color;
    let color = |c: Color, fg: bool| match c {
        Color::Rgb(r, g, b) => format!("\x1b[{};2;{r};{g};{b}m", if fg { 38 } else { 48 }),
        Color::Indexed(i) => format!("\x1b[{};5;{i}m", if fg { 38 } else { 48 }),
        _ => format!("\x1b[{}m", if fg { 39 } else { 49 }),
    };
    format!(
        "{}{}{}",
        color(cell.fg, true),
        color(cell.bg, false),
        cell.symbol()
    )
}

/// Draw `path` into a plain terminal `columns` wide (the CLI painter).
/// The mode is the pinned protocol, else [`detect`] with overlays on.
/// One write per row: a whole truecolor image in one write overruns the
/// Windows console and loses its tail rows.
pub fn print_inline(
    out: &mut impl std::io::Write,
    path: &Path,
    columns: u16,
    config_protocol: &str,
) -> std::io::Result<()> {
    let mode = forced(config_protocol).unwrap_or_else(|| detect(&Env::current(), true));
    let cols = columns.saturating_sub(3).clamp(4, MAX_COLS);
    let picker = (mode != ProtocolType::Halfblocks).then(|| {
        let mut p = Picker::halfblocks();
        p.set_protocol_type(mode);
        p
    });
    let rendered = match render_now(path, cols, MAX_ROWS, picker.as_ref()) {
        Ok(r) => r,
        Err(why) => {
            writeln!(out, "  (no preview: {why})")?;
            return out.flush();
        }
    };
    let Some(proto) = rendered.pixels.as_ref().and_then(|m| m.lock().ok()) else {
        for row in &rendered.cells {
            writeln!(out, "  {}", ansi_row(row))?;
            out.flush()?;
        }
        return Ok(());
    };
    let (w, h) = (rendered.grid.cols, rendered.grid.rows);
    let area = ratatui::layout::Rect::new(0, 0, w, h);
    let mut buf = ratatui::buffer::Buffer::empty(area);
    ratatui::widgets::Widget::render(ratatui_image::Image::new(&proto), area, &mut buf);
    if mode == ProtocolType::Kitty {
        // Placeholders are text: print them row by row, the id colour intact.
        for y in 0..h {
            let mut line = String::from("  ");
            for x in 0..w {
                let cell = &buf[(x, y)];
                if cell.diff_option != ratatui::buffer::CellDiffOption::Skip {
                    line.push_str(&ansi_cell(cell));
                }
            }
            writeln!(out, "{line}\x1b[0m")?;
            out.flush()?;
        }
        return Ok(());
    }
    // Sixel / iTerm2: the whole picture is one escape in the first cell.
    // Reserve the rows, jump back up past the indent, draw, come back.
    let escape = buf[(0, 0)].symbol().to_string();
    for _ in 0..h {
        writeln!(out)?;
    }
    write!(out, "\x1b7\x1b[{h}A\r\x1b[2C{escape}\x1b8")?;
    out.flush()
}

// ── finding images in tool output ─────────────────────────────────────────

/// Tools whose successful output names an image the user should see:
/// screenshots, generated images, frames, exported boards.
pub fn shows_images(tool: &str) -> bool {
    matches!(
        tool,
        "look"
            | "browser"
            | "terminal_browser"
            | "desktop"
            | "egaki"
            | "extract_frames"
            | "tldraw"
            | "excalidraw"
            | "penecho"
            | "debug"
    )
}

/// Local image files a successful call produced or looked at: paths in its
/// arguments and result that exist on disk, at most `limit`, first seen
/// first. Relative paths resolve against `cwd`. A path only in the result
/// must sit in the workspace, the temp directory or nur's home; one the call
/// named in its arguments is drawn wherever it is. Memoised for two seconds:
/// the overlay asks every frame and a result can be large.
pub fn image_paths(args: &str, result: &str, cwd: &Path, limit: usize) -> Vec<PathBuf> {
    type Memo = HashMap<u64, (std::time::Instant, Vec<PathBuf>)>;
    static MEMO: OnceLock<Mutex<Memo>> = OnceLock::new();
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (args, result, cwd, limit).hash(&mut h);
        h.finish()
    };
    let memo = MEMO.get_or_init(Default::default);
    if let Some((at, found)) = memo.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        if at.elapsed() < std::time::Duration::from_secs(2) {
            return found.clone();
        }
    }
    let found = find_image_paths(args, result, cwd, limit);
    let mut g = memo.lock().unwrap_or_else(|e| e.into_inner());
    if g.len() > 256 {
        g.clear();
    }
    g.insert(key, (std::time::Instant::now(), found.clone()));
    found
}

/// Where a picture may come from without the call naming it.
fn displayable(full: &Path, cwd: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let full = canon(full);
    [
        canon(cwd),
        canon(&std::env::temp_dir()),
        canon(&crate::config::nur_home()),
    ]
    .iter()
    .any(|root| full.starts_with(root))
}

fn find_image_paths(args: &str, result: &str, cwd: &Path, limit: usize) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut named: Vec<PathBuf> = Vec::new();
    let mut consider = |raw: &str, from_args: bool| {
        if out.len() >= limit {
            return;
        }
        let raw = raw
            .trim()
            .trim_matches(|c: char| {
                matches!(
                    c,
                    '"' | '\'' | '`' | '(' | ')' | '[' | ']' | '<' | '>' | ',' | ';'
                )
            })
            .trim_end_matches(['.', ':']);
        if raw.is_empty() || raw.contains("://") {
            return;
        }
        let ext = Path::new(raw)
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        if !ext.is_some_and(|e| EXTENSIONS.contains(&e.as_str())) {
            return;
        }
        let p = Path::new(raw);
        let full = if p.is_absolute() {
            p.to_path_buf()
        } else {
            cwd.join(p)
        };
        if from_args {
            named.push(full.clone());
        }
        let allowed = from_args || named.contains(&full) || displayable(&full, cwd);
        if allowed && full.is_file() && !out.contains(&full) {
            out.push(full);
        }
    };
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(args) {
        for k in ["path", "paths", "output", "out", "file"] {
            match v.get(k) {
                Some(serde_json::Value::String(s)) => consider(s, true),
                Some(serde_json::Value::Array(a)) => a
                    .iter()
                    .filter_map(|x| x.as_str())
                    .for_each(|x| consider(x, true)),
                _ => {}
            }
        }
    }
    for line in result.lines() {
        // A whole line first (a path with spaces), then its tokens.
        consider(
            line.trim().trim_start_matches(|c: char| {
                !c.is_ascii_alphanumeric() && !matches!(c, '/' | '\\' | '.' | '~')
            }),
            false,
        );
        if let Some((_, rest)) = line.split_once(": ") {
            consider(rest, false);
        }
        for token in line.split_whitespace() {
            consider(token, false);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(dir: &Path, name: &str, w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> PathBuf {
        let img = RgbaImage::from_fn(w, h, |x, y| image::Rgba(f(x, y)));
        let p = dir.join(name);
        img.save(&p).unwrap();
        p
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nur-timg-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    // Failure modes: a wide screenshot overflowed the 64-column cap, a tall
    // one the 16-row cap, and a 16 px icon was blown up to fill the box.
    #[test]
    fn fit_keeps_aspect_inside_the_caps_and_never_upscales() {
        let g = fit(3840, 2160, (8, 16), MAX_COLS, MAX_ROWS);
        assert_eq!(g.cols, 57);
        assert_eq!(g.rows, 16);
        let wide = fit(4000, 200, (8, 16), MAX_COLS, MAX_ROWS);
        assert_eq!(wide.cols, 64);
        assert!(wide.rows <= 2, "{wide:?}");
        assert_eq!(
            fit(16, 16, (8, 16), MAX_COLS, MAX_ROWS),
            Grid { cols: 2, rows: 1 }
        );
        assert_eq!(
            fit(1000, 1000, (8, 16), 10, MAX_ROWS).cols,
            10,
            "a narrow pane caps the width"
        );
    }

    // Failure mode: a transparent edge drew as a black box around the image.
    #[test]
    fn half_blocks_leave_transparent_halves_to_the_terminal() {
        assert_eq!(
            half_glyph((Some([1, 2, 3]), Some([4, 5, 6]))),
            ('▀', Some([1, 2, 3]), Some([4, 5, 6]))
        );
        assert_eq!(
            half_glyph((Some([1, 2, 3]), None)),
            ('▀', Some([1, 2, 3]), None)
        );
        assert_eq!(
            half_glyph((None, Some([4, 5, 6]))),
            ('▄', Some([4, 5, 6]), None)
        );
        assert_eq!(half_glyph((None, None)), (' ', None, None));
        let row = ansi_row(&[(Some([255, 0, 0]), None)]);
        assert_eq!(row, "\x1b[38;2;255;0;0m\x1b[49m▀\x1b[0m");
    }

    #[test]
    fn halfblocks_sample_top_and_bottom_pixels_and_honour_alpha() {
        let d = tmp("hb");
        // Top half red, bottom half transparent: every cell is a red `▀`
        // over the terminal background.
        let p = png(&d, "a.png", 8, 4, |_, y| {
            if y < 2 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 0, 0]
            }
        });
        let img = load(&p).unwrap();
        let cells = halfblocks(&img, Grid { cols: 4, rows: 1 });
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(cells.len(), 1);
        assert!(
            cells[0]
                .iter()
                .all(|c| c.1.is_none() && c.0.is_some_and(|[r, _, _]| r > 200)),
            "{cells:?}"
        );
    }

    // Failure mode: a decompression bomb (huge header) was decoded on the
    // way to being refused.
    #[test]
    fn oversized_and_missing_files_are_refused_from_the_header() {
        let d = tmp("cap");
        let p = png(&d, "big.png", 8000, 6000, |_, _| [0, 0, 0, 255]);
        let err = dimensions(&p).unwrap_err();
        let missing = dimensions(&d.join("nope.png")).unwrap_err();
        let _ = std::fs::remove_dir_all(&d);
        assert!(err.contains("over 40 MP"), "{err}");
        assert!(!missing.is_empty());
    }

    #[test]
    fn detection_follows_the_hermes_ladder_with_nur_s_terminals() {
        let env = |f: fn(&mut Env)| {
            let mut e = Env::default();
            f(&mut e);
            e
        };
        assert_eq!(
            detect(&env(|e| e.kitty_window = true), false),
            ProtocolType::Kitty
        );
        assert_eq!(
            detect(&env(|e| e.term_program = "ghostty".into()), false),
            ProtocolType::Kitty
        );
        // VS Code passes an inherited KITTY_WINDOW_ID along: blank frame.
        assert_eq!(
            detect(
                &env(|e| {
                    e.term_program = "vscode".into();
                    e.kitty_window = true;
                }),
                true
            ),
            ProtocolType::Halfblocks
        );
        // tmux/screen swallow the transmit; Orca does not place placeholders.
        assert_eq!(
            detect(
                &env(|e| {
                    e.tmux = true;
                    e.kitty_window = true;
                }),
                true
            ),
            ProtocolType::Halfblocks
        );
        assert_eq!(
            detect(
                &env(|e| {
                    e.screen = true;
                    e.iterm_session = true;
                }),
                true
            ),
            ProtocolType::Halfblocks
        );
        assert_eq!(
            detect(
                &env(|e| {
                    e.orca = true;
                    e.kitty_window = true;
                }),
                true
            ),
            ProtocolType::Halfblocks
        );
        // WezTerm: kitty placeholders are tofu; iTerm2 over reserved rows works.
        assert_eq!(
            detect(
                &env(|e| {
                    e.wezterm = true;
                    e.kitty_window = true;
                }),
                true
            ),
            ProtocolType::Iterm2
        );
        assert_eq!(
            detect(&env(|e| e.wezterm = true), false),
            ProtocolType::Halfblocks
        );
        assert_eq!(
            detect(&env(|e| e.term_program = "iterm.app".into()), true),
            ProtocolType::Iterm2
        );
        assert_eq!(
            detect(&env(|e| e.konsole = true), true),
            ProtocolType::Iterm2
        );
        // Windows Terminal's sixel is opt-in and invisible to the environment.
        assert_eq!(
            detect(&env(|e| e.windows_terminal = true), true),
            ProtocolType::Halfblocks
        );
        assert_eq!(detect(&Env::default(), true), ProtocolType::Halfblocks);
    }

    // Failure mode: iTerm2 pixels overlaid in the transcript lingered after
    // a scroll; kitty under tmux was swallowed.
    #[test]
    fn the_tui_overlays_only_scroll_safe_protocols() {
        let plain = Env::default();
        assert_eq!(
            overlay_protocol(ProtocolType::Kitty, &plain),
            ProtocolType::Kitty
        );
        assert_eq!(
            overlay_protocol(ProtocolType::Sixel, &plain),
            ProtocolType::Sixel
        );
        assert_eq!(
            overlay_protocol(ProtocolType::Iterm2, &plain),
            ProtocolType::Halfblocks
        );
        let tmux = Env {
            tmux: true,
            ..Env::default()
        };
        assert_eq!(
            overlay_protocol(ProtocolType::Kitty, &tmux),
            ProtocolType::Halfblocks
        );
        let orca = Env {
            orca: true,
            ..Env::default()
        };
        assert_eq!(
            overlay_protocol(ProtocolType::Sixel, &orca),
            ProtocolType::Halfblocks
        );
    }

    #[test]
    fn image_paths_come_from_args_and_result_and_must_exist() {
        let d = tmp("paths");
        let shot = png(&d, "shot.png", 2, 2, |_, _| [0, 0, 0, 255]);
        let spaced = png(&d, "my shot.png", 2, 2, |_, _| [0, 0, 0, 255]);
        let result = format!(
            "Screenshot saved to {}\noutput: {}\nalso https://x.test/remote.png and ./missing.png",
            shot.display(),
            spaced.display()
        );
        let found = image_paths(r#"{"path":"shot.png"}"#, &result, &d, 4);
        let one = image_paths("{}", &result, &d, 1);
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(found, vec![d.join("shot.png"), spaced.clone()], "{found:?}");
        assert_eq!(one.len(), 1);
    }

    // Failure mode: a successful call that merely mentioned a private image
    // elsewhere on disk (an error line, a listing) drew it in the transcript.
    #[test]
    fn a_result_only_path_outside_the_workspace_is_not_drawn() {
        // The repo's target dir sits outside a workspace of `src`, and is
        // neither the temp dir nor nur's home.
        let root = std::env::current_dir().unwrap();
        let outside = root
            .join("target")
            .join(format!("nur-timg-outside-{}", std::process::id()));
        std::fs::create_dir_all(&outside).unwrap();
        let work = root.join("src");
        let pic = png(&outside, "private.png", 2, 2, |_, _| [0, 0, 0, 255]);
        let shown = image_paths("{}", &format!("saw {}", pic.display()), &work, 4);
        let named = image_paths(
            &serde_json::json!({ "path": pic.display().to_string() }).to_string(),
            "",
            &work,
            4,
        );
        let _ = std::fs::remove_dir_all(&outside);
        assert!(shown.is_empty(), "{shown:?}");
        assert_eq!(named, vec![pic]);
    }

    #[test]
    fn render_now_produces_the_grid_and_cells_without_a_picker() {
        let d = tmp("render");
        let p = png(&d, "g.png", 160, 80, |x, _| [x as u8, 0, 0, 255]);
        let r = render_now(&p, 10, MAX_ROWS, None).unwrap();
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(r.grid, Grid { cols: 10, rows: 3 });
        assert_eq!(r.cells.len(), 3);
        assert!(r.cells.iter().all(|row| row.len() == 10));
        assert!(r.pixels.is_none());
    }
}

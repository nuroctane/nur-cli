//! NurCLI visual system — runtime-swappable palette (gold-led by default).
//!
//! Single source of truth for colors + text styles used by both the TUI
//! (ratatui) and plain stdout printing (colored).
//!
//! The palette is now **runtime-selectable** (`/theme`, onboarding): every
//! color is an accessor that reads the active [`Palette`]. Non-color chrome
//! (spinners, glyphs, timings) stays `const` — those are not themed.

use colored::Colorize;
use ratatui::style::{Color, Modifier, Style};
use std::sync::RwLock;
use std::time::Duration;

// ── Runtime palette ──────────────────────────────────────────────────────────

/// Every themeable color, in one flat record. Presets fill this in; the active
/// one lives behind [`ACTIVE`] and is read via the `UPPER_CASE()` accessors so
/// existing call sites (`theme::NUR_GOLD()`) keep reading like named colors.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // Presets intentionally define the complete visual vocabulary.
pub struct Palette {
    pub nur_gold: Color,
    pub nur_gold_deep: Color,
    pub nur_gold_sky: Color,
    pub bg: Color,
    pub surface: Color,
    pub surface_2: Color,
    pub surface_3: Color,
    pub fg: Color,
    pub muted: Color,
    pub faint: Color,
    pub border: Color,
    pub code_bg: Color,
    pub md_code: Color,
    pub md_h1: Color,
    pub md_h2: Color,
    pub md_h3: Color,
    pub md_link: Color,
    pub md_quote: Color,
    pub md_list: Color,
    pub assistant_fg: Color,
    pub assistant_dim: Color,
    pub success: Color,
    pub warn: Color,
    pub error: Color,
    pub diff_add_fg: Color,
    pub diff_add_bg: Color,
    pub diff_del_fg: Color,
    pub diff_del_bg: Color,
    pub diff_meta: Color,
    pub user: Color,
    pub blue_050: Color,
    pub blue_100: Color,
    pub blue_150: Color,
    pub blue_200: Color,
    pub blue_250: Color,
    pub blue_300: Color,
    pub blue_400: Color,
    pub blue_500: Color,
    pub blue_600: Color,
    pub indigo: Color,
    pub periwinkle: Color,
    pub violet: Color,
    pub lavender: Color,
    pub magenta: Color,
    pub pink: Color,
    pub rose: Color,
    pub coral: Color,
    pub amber: Color,
    pub gold: Color,
    pub orange: Color,
    pub lime: Color,
    pub mint: Color,
    pub seafoam: Color,
    pub teal: Color,
    pub cyan: Color,
    /// Banner gradient (top → bottom rows of the NUR logotype).
    pub gradient: [(u8, u8, u8); 6],
    /// Shimmer ring for animated borders/separators.
    pub aurora: [Color; 12],
}

/// Default theme — the gold-led NurCLI identity. All other presets derive from
/// this so a theme only has to override what actually changes.
const GOLD: Palette = Palette {
    nur_gold: Color::Rgb(232, 185, 35),
    nur_gold_deep: Color::Rgb(184, 134, 11),
    nur_gold_sky: Color::Rgb(255, 224, 140),
    bg: Color::Rgb(11, 14, 18),
    surface: Color::Rgb(18, 22, 28),
    surface_2: Color::Rgb(26, 31, 40),
    surface_3: Color::Rgb(38, 45, 58),
    fg: Color::Rgb(245, 242, 232),
    muted: Color::Rgb(148, 142, 128),
    faint: Color::Rgb(126, 119, 104),
    border: Color::Rgb(48, 44, 36),
    code_bg: Color::Rgb(16, 18, 14),
    md_code: Color::Rgb(160, 220, 195),
    md_h1: Color::Rgb(120, 210, 215),
    md_h2: Color::Rgb(130, 175, 235),
    md_h3: Color::Rgb(165, 155, 235),
    md_link: Color::Rgb(100, 195, 235),
    md_quote: Color::Rgb(150, 165, 145),
    md_list: Color::Rgb(90, 185, 165),
    assistant_fg: Color::Rgb(228, 232, 240),
    assistant_dim: Color::Rgb(150, 160, 175),
    success: Color::Rgb(52, 199, 123),
    warn: Color::Rgb(255, 186, 73),
    error: Color::Rgb(255, 99, 99),
    diff_add_fg: Color::Rgb(126, 231, 166),
    diff_add_bg: Color::Rgb(18, 42, 30),
    diff_del_fg: Color::Rgb(255, 138, 148),
    diff_del_bg: Color::Rgb(46, 24, 28),
    diff_meta: Color::Rgb(212, 175, 80),
    user: Color::Rgb(255, 255, 255),
    blue_050: Color::Rgb(255, 250, 220),
    blue_100: Color::Rgb(255, 242, 190),
    blue_150: Color::Rgb(255, 236, 160),
    blue_200: Color::Rgb(255, 224, 140),
    blue_250: Color::Rgb(255, 216, 100),
    blue_300: Color::Rgb(255, 208, 90),
    blue_400: Color::Rgb(232, 185, 35),
    blue_500: Color::Rgb(184, 134, 11),
    blue_600: Color::Rgb(140, 100, 10),
    indigo: Color::Rgb(139, 120, 220),
    periwinkle: Color::Rgb(168, 150, 230),
    violet: Color::Rgb(178, 148, 255),
    lavender: Color::Rgb(202, 180, 255),
    magenta: Color::Rgb(200, 120, 200),
    pink: Color::Rgb(220, 140, 180),
    rose: Color::Rgb(255, 143, 168),
    coral: Color::Rgb(255, 138, 120),
    amber: Color::Rgb(236, 162, 44),
    gold: Color::Rgb(255, 208, 110),
    orange: Color::Rgb(255, 150, 89),
    lime: Color::Rgb(160, 224, 122),
    mint: Color::Rgb(80, 190, 170),
    seafoam: Color::Rgb(56, 170, 160),
    teal: Color::Rgb(32, 150, 148),
    cyan: Color::Rgb(72, 196, 208),
    gradient: [
        (255, 248, 180),
        (255, 230, 120),
        (255, 200, 60),
        (232, 185, 35),
        (200, 150, 20),
        (160, 110, 15),
    ],
    aurora: [
        Color::Rgb(255, 252, 200),
        Color::Rgb(255, 245, 160),
        Color::Rgb(255, 230, 100),
        Color::Rgb(255, 214, 70),
        Color::Rgb(232, 185, 35),
        Color::Rgb(212, 160, 25),
        Color::Rgb(190, 140, 20),
        Color::Rgb(170, 120, 18),
        Color::Rgb(200, 150, 40),
        Color::Rgb(230, 190, 90),
        Color::Rgb(255, 220, 120),
        Color::Rgb(255, 200, 80),
    ],
};

/// The active palette. Swapped by [`set_theme`]; read (copied) by [`current`].
static ACTIVE: RwLock<Palette> = RwLock::new(GOLD);

/// Copy of the live palette (cheap — `Palette: Copy`, uncontended read).
#[inline]
pub fn current() -> Palette {
    *ACTIVE.read().unwrap_or_else(|e| e.into_inner())
}

/// Registered themes, in menu order. `(id, human label)`.
pub const THEMES: &[(&str, &str)] = &[
    ("gold", "Nur Gold - the signature gold spine"),
    ("mono", "Mono - neutral graphite, silver accent"),
    ("midnight", "Midnight - deep indigo + cyan"),
    ("solarized", "Solarized Dark - the classic base16"),
    ("ember", "Ember - warm crimson + amber"),
];

/// Every registered theme id.
pub fn theme_ids() -> Vec<&'static str> {
    THEMES.iter().map(|(id, _)| *id).collect()
}

/// Resolve aliases to a registered theme id.
pub fn canonical_theme_id(id: &str) -> Option<&'static str> {
    let id = id.trim();
    if id.is_empty() || id.eq_ignore_ascii_case("default") || id.eq_ignore_ascii_case("nur") {
        return Some("gold");
    }
    THEMES
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(id))
        .map(|(candidate, _)| *candidate)
}

/// True when `id` names a registered theme.
pub fn is_theme(id: &str) -> bool {
    canonical_theme_id(id).is_some()
}

/// The active theme id (tracked alongside the palette).
static ACTIVE_NAME: RwLock<String> = RwLock::new(String::new());

/// Name of the active theme (`"gold"` when unset).
pub fn current_theme_name() -> String {
    let n = ACTIVE_NAME.read().map(|s| s.clone()).unwrap_or_default();
    if n.is_empty() {
        "gold".into()
    } else {
        n
    }
}

/// Build a preset by id (case-insensitive). `None` if the id is unknown.
fn preset(id: &str) -> Option<Palette> {
    Some(match canonical_theme_id(id)? {
        "gold" => GOLD,
        "mono" => Palette {
            nur_gold: Color::Rgb(210, 214, 222),
            nur_gold_deep: Color::Rgb(150, 156, 166),
            nur_gold_sky: Color::Rgb(236, 239, 244),
            bg: Color::Rgb(14, 15, 17),
            surface: Color::Rgb(20, 22, 25),
            surface_2: Color::Rgb(30, 33, 38),
            surface_3: Color::Rgb(44, 48, 55),
            border: Color::Rgb(52, 55, 62),
            code_bg: Color::Rgb(17, 18, 20),
            blue_050: Color::Rgb(244, 246, 250),
            blue_100: Color::Rgb(226, 230, 238),
            blue_150: Color::Rgb(210, 215, 224),
            blue_200: Color::Rgb(194, 200, 210),
            blue_250: Color::Rgb(178, 184, 196),
            blue_300: Color::Rgb(200, 205, 214),
            blue_400: Color::Rgb(210, 214, 222),
            blue_500: Color::Rgb(150, 156, 166),
            blue_600: Color::Rgb(112, 118, 128),
            diff_meta: Color::Rgb(176, 182, 194),
            gradient: ramp6(Color::Rgb(236, 239, 244), Color::Rgb(120, 126, 136)),
            aurora: ring12(Color::Rgb(236, 239, 244), Color::Rgb(120, 126, 136)),
            ..GOLD
        },
        "midnight" => Palette {
            nur_gold: Color::Rgb(96, 176, 246),
            nur_gold_deep: Color::Rgb(58, 118, 196),
            nur_gold_sky: Color::Rgb(158, 208, 255),
            bg: Color::Rgb(9, 12, 22),
            surface: Color::Rgb(15, 19, 32),
            surface_2: Color::Rgb(22, 28, 46),
            surface_3: Color::Rgb(32, 40, 62),
            border: Color::Rgb(40, 50, 78),
            code_bg: Color::Rgb(12, 16, 26),
            blue_050: Color::Rgb(224, 238, 255),
            blue_100: Color::Rgb(190, 222, 255),
            blue_150: Color::Rgb(158, 208, 255),
            blue_200: Color::Rgb(128, 192, 250),
            blue_250: Color::Rgb(110, 184, 248),
            blue_300: Color::Rgb(96, 176, 246),
            blue_400: Color::Rgb(96, 176, 246),
            blue_500: Color::Rgb(58, 118, 196),
            blue_600: Color::Rgb(40, 88, 150),
            diff_meta: Color::Rgb(120, 176, 236),
            gradient: ramp6(Color::Rgb(158, 208, 255), Color::Rgb(40, 88, 150)),
            aurora: ring12(Color::Rgb(158, 208, 255), Color::Rgb(40, 88, 150)),
            ..GOLD
        },
        "solarized" => Palette {
            nur_gold: Color::Rgb(181, 137, 0),
            nur_gold_deep: Color::Rgb(133, 100, 0),
            nur_gold_sky: Color::Rgb(203, 161, 40),
            bg: Color::Rgb(0, 43, 54),
            surface: Color::Rgb(7, 54, 66),
            surface_2: Color::Rgb(20, 68, 80),
            surface_3: Color::Rgb(34, 84, 96),
            fg: Color::Rgb(147, 161, 161),
            muted: Color::Rgb(131, 148, 150),
            faint: Color::Rgb(101, 123, 131),
            border: Color::Rgb(40, 88, 100),
            code_bg: Color::Rgb(0, 38, 48),
            assistant_fg: Color::Rgb(238, 232, 213),
            assistant_dim: Color::Rgb(147, 161, 161),
            success: Color::Rgb(133, 153, 0),
            warn: Color::Rgb(203, 75, 22),
            error: Color::Rgb(220, 50, 47),
            user: Color::Rgb(238, 232, 213),
            md_h1: Color::Rgb(38, 139, 210),
            md_h2: Color::Rgb(42, 161, 152),
            md_h3: Color::Rgb(108, 113, 196),
            md_link: Color::Rgb(38, 139, 210),
            md_code: Color::Rgb(42, 161, 152),
            blue_050: Color::Rgb(238, 232, 213),
            blue_100: Color::Rgb(213, 196, 140),
            blue_150: Color::Rgb(203, 161, 40),
            blue_200: Color::Rgb(181, 137, 0),
            blue_250: Color::Rgb(181, 137, 0),
            blue_300: Color::Rgb(181, 137, 0),
            blue_400: Color::Rgb(181, 137, 0),
            blue_500: Color::Rgb(133, 100, 0),
            blue_600: Color::Rgb(101, 123, 131),
            indigo: Color::Rgb(108, 113, 196),
            violet: Color::Rgb(108, 113, 196),
            teal: Color::Rgb(42, 161, 152),
            cyan: Color::Rgb(38, 139, 210),
            seafoam: Color::Rgb(42, 161, 152),
            diff_meta: Color::Rgb(181, 137, 0),
            gradient: ramp6(Color::Rgb(203, 161, 40), Color::Rgb(101, 79, 0)),
            aurora: ring12(Color::Rgb(203, 161, 40), Color::Rgb(101, 79, 0)),
            ..GOLD
        },
        "ember" => Palette {
            nur_gold: Color::Rgb(240, 120, 74),
            nur_gold_deep: Color::Rgb(186, 74, 44),
            nur_gold_sky: Color::Rgb(255, 178, 128),
            bg: Color::Rgb(18, 12, 12),
            surface: Color::Rgb(26, 18, 17),
            surface_2: Color::Rgb(38, 25, 24),
            surface_3: Color::Rgb(54, 34, 32),
            border: Color::Rgb(64, 40, 34),
            code_bg: Color::Rgb(20, 13, 12),
            blue_050: Color::Rgb(255, 234, 214),
            blue_100: Color::Rgb(255, 208, 170),
            blue_150: Color::Rgb(255, 178, 128),
            blue_200: Color::Rgb(255, 150, 100),
            blue_250: Color::Rgb(248, 132, 84),
            blue_300: Color::Rgb(240, 120, 74),
            blue_400: Color::Rgb(240, 120, 74),
            blue_500: Color::Rgb(186, 74, 44),
            blue_600: Color::Rgb(140, 52, 30),
            diff_meta: Color::Rgb(232, 140, 90),
            gradient: ramp6(Color::Rgb(255, 200, 120), Color::Rgb(150, 40, 30)),
            aurora: ring12(Color::Rgb(255, 200, 120), Color::Rgb(150, 40, 30)),
            ..GOLD
        },
        _ => return None,
    })
}

/// Switch the active theme by id. Returns `false` for an unknown id (palette
/// unchanged). Applies immediately to the next render.
pub fn set_theme(id: &str) -> bool {
    let Some(canonical) = canonical_theme_id(id) else {
        return false;
    };
    match preset(canonical) {
        Some(p) => {
            if let Ok(mut w) = ACTIVE.write() {
                *w = p;
            }
            if let Ok(mut n) = ACTIVE_NAME.write() {
                *n = canonical.to_string();
            }
            true
        }
        None => false,
    }
}

/// Three accent stops used by the theme picker preview.
pub fn theme_preview(id: &str) -> Option<[Color; 3]> {
    preset(id).map(|p| [p.nur_gold_sky, p.nur_gold, p.nur_gold_deep])
}

/// Build a 6-stop banner gradient by interpolating a light → deep accent.
fn ramp6(light: Color, deep: Color) -> [(u8, u8, u8); 6] {
    let mut out = [(0u8, 0u8, 0u8); 6];
    for (i, slot) in out.iter_mut().enumerate() {
        let t = i as f64 / 5.0;
        if let Color::Rgb(r, g, b) = lerp(light, deep, t) {
            *slot = (r, g, b);
        }
    }
    out
}

/// Build a 12-color shimmer ring: light → deep → light so it loops smoothly.
fn ring12(light: Color, deep: Color) -> [Color; 12] {
    let mut out = [Color::Rgb(0, 0, 0); 12];
    for (i, slot) in out.iter_mut().enumerate() {
        // Triangle wave 0→1→0 across the ring for a seamless loop.
        let x = i as f64 / 12.0;
        let t = 1.0 - (2.0 * x - 1.0).abs();
        *slot = lerp(light, deep, t);
    }
    out
}

/// Generate the `UPPER_CASE()` color accessors that read the live palette.
macro_rules! palette_accessors {
    ($($name:ident => $field:ident),* $(,)?) => {
        $(
            #[allow(non_snake_case, dead_code)]
            #[inline]
            pub fn $name() -> Color { current().$field }
        )*
    };
}

palette_accessors! {
    NUR_GOLD => nur_gold,
    NUR_GOLD_DEEP => nur_gold_deep,
    NUR_GOLD_SKY => nur_gold_sky,
    // Legacy names kept across the TUI — all now gold (not Meta blue).
    META_BLUE => nur_gold,
    META_BLUE_DEEP => nur_gold_deep,
    META_BLUE_SKY => nur_gold_sky,
    BG => bg,
    SURFACE => surface,
    SURFACE_2 => surface_2,
    SURFACE_3 => surface_3,
    FG => fg,
    MUTED => muted,
    FAINT => faint,
    BORDER => border,
    CODE_BG => code_bg,
    MD_CODE => md_code,
    MD_H1 => md_h1,
    MD_H2 => md_h2,
    MD_H3 => md_h3,
    MD_LINK => md_link,
    MD_QUOTE => md_quote,
    MD_LIST => md_list,
    ASSISTANT_FG => assistant_fg,
    ASSISTANT_DIM => assistant_dim,
    SUCCESS => success,
    WARN => warn,
    ERROR => error,
    DIFF_ADD_FG => diff_add_fg,
    DIFF_ADD_BG => diff_add_bg,
    DIFF_DEL_FG => diff_del_fg,
    DIFF_DEL_BG => diff_del_bg,
    DIFF_META => diff_meta,
    USER => user,
    BLUE_050 => blue_050,
    BLUE_100 => blue_100,
    BLUE_150 => blue_150,
    BLUE_200 => blue_200,
    BLUE_250 => blue_250,
    BLUE_300 => blue_300,
    BLUE_400 => blue_400,
    BLUE_500 => blue_500,
    BLUE_600 => blue_600,
    INDIGO => indigo,
    PERIWINKLE => periwinkle,
    VIOLET => violet,
    LAVENDER => lavender,
    MAGENTA => magenta,
    PINK => pink,
    ROSE => rose,
    CORAL => coral,
    AMBER => amber,
    GOLD_ACCENT => gold,
    ORANGE => orange,
    LIME => lime,
    MINT => mint,
    SEAFOAM => seafoam,
    TEAL => teal,
    CYAN => cyan,
}

// ── Color math + animated gradients ─────────────────────────────────────────
/// Decompose a color to RGB (non-RGB variants fall back to the canvas).
fn rgb(c: Color) -> (f64, f64, f64) {
    match c {
        Color::Rgb(r, g, b) => (r as f64, g as f64, b as f64),
        _ => (11.0, 14.0, 18.0),
    }
}

/// Linear interpolate between two colors. `t` in 0..=1.
pub fn lerp(a: Color, b: Color, t: f64) -> Color {
    let t = t.clamp(0.0, 1.0);
    let (ar, ag, ab) = rgb(a);
    let (br, bg, bb) = rgb(b);
    Color::Rgb(
        (ar + (br - ar) * t).round() as u8,
        (ag + (bg - ag) * t).round() as u8,
        (ab + (bb - ab) * t).round() as u8,
    )
}

/// Blend a color toward the canvas background by `t` (0 = full, 1 = invisible).
pub fn dim(c: Color, t: f64) -> Color {
    lerp(c, BG(), t)
}

/// Sample the active aurora ring at `phase` (any f64; wraps) with smooth interpolation.
pub fn aurora_at(phase: f64) -> Color {
    let ring = current().aurora;
    let n = ring.len();
    let x = phase.rem_euclid(1.0) * n as f64;
    let i = (x.floor() as usize) % n;
    let j = (i + 1) % n;
    lerp(ring[i], ring[j], x.fract())
}

/// Aurora colour that travels over time and across a horizontal position — the
/// basis for shimmering borders and separators.
/// `elapsed` drives motion; `pos`/`span` give a per-cell phase offset.
pub fn aurora_cell(elapsed: Duration, pos: usize, span: usize, period_ms: u128) -> Color {
    let t = if period_ms == 0 {
        0.0
    } else {
        (elapsed.as_millis() % period_ms) as f64 / period_ms as f64
    };
    let spatial = if span == 0 {
        0.0
    } else {
        pos as f64 / span as f64
    };
    aurora_at(t + spatial)
}

/// Colour a tool by *family*: read (gold) · write (violet) · shell (amber) ·
/// net (deep teal) · git (teal) · delegate (pink) · knowledge (indigo/orange).
pub fn tool_color(name: &str) -> Color {
    match name {
        "read_file" | "list_dir" | "grep" | "glob" => BLUE_300(),
        "write_file" | "edit_file" | "multi_edit" | "apply_patch" => VIOLET(),
        "bash" => AMBER(),
        "web_fetch" | "web_search" | "browser" => TEAL(),
        "look" | "extract_frames" => PINK(),
        "git_status" | "git_diff" => CYAN(),
        "agent" | "omp" => PINK(),
        "memory" => ORANGE(),
        "skill" | "todo_write" | "graphify" | "plur" | "ruflo" | "executor" => INDIGO(),
        "submit_plan" => VIOLET(),
        _ => BLUE_200(),
    }
}

/// A one-word family label used in the tool card gutter.
pub fn tool_family(name: &str) -> &'static str {
    match name {
        "read_file" | "list_dir" | "grep" | "glob" => "read",
        "write_file" | "edit_file" | "multi_edit" | "apply_patch" => "edit",
        "bash" => "shell",
        "web_fetch" | "web_search" => "web",
        "browser" => "browser",
        "look" => "vision",
        "extract_frames" => "video",
        "git_status" | "git_diff" => "git",
        "agent" => "agent",
        "omp" => "omp",
        "memory" => "memory",
        "skill" => "skill",
        "todo_write" => "todo",
        "graphify" => "graph",
        "plur" => "plur",
        "ruflo" => "ruflo",
        "executor" => "gateway",
        "submit_plan" => "plan",
        _ => "tool",
    }
}

/// Semantic classes for system notices, so mode changes, plans, todos, usage
/// and session events are each visually distinct instead of all "blue info".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Mode,
    Plan,
    Todos,
    Usage,
    Session,
    Skill,
    Memory,
}

impl Tone {
    pub fn color(self) -> Color {
        match self {
            Tone::Neutral => BLUE_400(),
            Tone::Mode => INDIGO(),
            Tone::Plan => VIOLET(),
            Tone::Todos => CYAN(),
            Tone::Usage => TEAL(),
            Tone::Session => BLUE_200(),
            Tone::Skill => PERIWINKLE(),
            Tone::Memory => ORANGE(),
        }
    }

    /// Leading glyph — shape carries meaning even without color.
    pub fn glyph(self) -> &'static str {
        match self {
            Tone::Neutral => "●",
            Tone::Mode => "◈",
            Tone::Plan => "✦",
            Tone::Todos => "☰",
            Tone::Usage => "∑",
            Tone::Session => "⟲",
            Tone::Skill => "◆",
            Tone::Memory => "❖",
        }
    }
}

// ── Motion ─────────────────────────────────────────────────────────────────
// Motion taste (Emil Kowalski / design-eng):
//   · Fast spinner → perceived speed (same wait, feels snappier)
//   · Ease-out curves for entry feedback; never ease-in for UI
//   · UI feedback < 300ms; no motion on high-frequency keyboard actions
//   · Only "animate" glyphs/opacity in TUI — never layout thrash

/// Braille spinner — smooth, dense, Nur-gold tinted in UI.
pub const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
/// Orbiting-dot spinner for secondary busy accents (statusline, chips).
#[allow(dead_code)]
pub const SPINNER_ORBIT: &[&str] = &["◜", "◝", "◞", "◟"];
/// Growing/shrinking dot — soft "breathing" accent.
#[allow(dead_code)]
pub const SPINNER_DOTS: &[&str] = &["∙", "•", "●", "◉", "●", "•"];
/// Sparkle cycle for celebratory / vision accents.
pub const SPARKLE: &[&str] = &["✶", "✸", "✹", "✷", "✵", "✧"];
/// Soft pulse dots for quieter states (thinking complete, idle accent).
pub const PULSE: &[&str] = &["·", "•", "●", "•"];
/// Window-title animation while inference runs — moon phases read as "working".
pub const TITLE_FRAMES: &[&str] = &["🌑", "🌒", "🌓", "🌔", "🌕", "🌖", "🌗", "🌘"];
/// Idle title marker — full moon (product is Nur, not Meta blue).
pub const TITLE_IDLE: &str = "🌕";

/// Pick a frame from any set by elapsed time at `ms` per frame.
pub fn frame_at(set: &[&'static str], elapsed: Duration, ms: u128) -> &'static str {
    if set.is_empty() {
        return "";
    }
    let i = (elapsed.as_millis() / ms.max(1)) as usize % set.len();
    set[i]
}
/// Expand chevrons (collapsed → expanded).
pub const CHEVRON_COLLAPSED: &str = "▸";
pub const CHEVRON_EXPANDED: &str = "▾";
/// Frame interval for spinner (ms). Faster = feels more responsive.
pub const SPINNER_MS: u128 = 48;
/// Soft pulse base interval (ms).
pub const PULSE_MS: u128 = 220;
/// Cursor / stream caret blink half-period (ms).
pub const BLINK_MS: u128 = 530;
/// Brief highlight after expand/collapse toggle (ms) — ease-out settle.
pub const SETTLE_MS: u128 = 180;

/// Spinner glyph for elapsed time.
pub fn spinner_frame(elapsed: Duration) -> &'static str {
    let i = (elapsed.as_millis() / SPINNER_MS) as usize % SPINNER.len();
    SPINNER[i]
}

/// Current spinner phase index (for cheap change-detection fingerprints).
pub fn spinner_index(elapsed: Duration) -> u8 {
    ((elapsed.as_millis() / SPINNER_MS) as usize % SPINNER.len()) as u8
}

/// Soft pulse glyph — slight ease-out cadence (spend less time on the bright frame).
pub fn pulse_frame(elapsed: Duration) -> &'static str {
    // Non-uniform dwell: dim frames hold longer (ease-out feel without CSS).
    let phase = (elapsed.as_millis() / PULSE_MS) as usize;
    let dwell = [0, 0, 1, 2, 3, 3, 2, 1]; // index into PULSE via cycle
    let i = dwell[phase % dwell.len()];
    PULSE[i.min(PULSE.len() - 1)]
}

/// True during the "on" half of a blink cycle.
pub fn blink_on(elapsed: Duration) -> bool {
    (elapsed.as_millis() / BLINK_MS).is_multiple_of(2)
}

/// Cubic ease-out: 1 - (1-t)³. `t` in 0..=1.
pub fn ease_out(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Progress 0.0→1.0 over `ms` milliseconds of `elapsed`, ease-out shaped.
pub fn settle_progress(elapsed: Duration, ms: u128) -> f64 {
    if ms == 0 {
        return 1.0;
    }
    ease_out(elapsed.as_millis() as f64 / ms as f64)
}

/// Compact duration for thought/tool/turn cards (`842ms`, `1.2s`, `1m04s`).
pub fn fmt_duration(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < 1000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        let s = ms as f64 / 1000.0;
        if s < 10.0 {
            format!("{s:.1}s")
        } else {
            format!("{:.0}s", s)
        }
    } else {
        let secs = d.as_secs();
        format!("{}m{:02}s", secs / 60, secs % 60)
    }
}

/// Live elapsed while a turn/tool is still running (tenths under a minute).
pub fn fmt_elapsed_live(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        fmt_duration(d)
    }
}

/// Accent for duration chips (steps slightly out of the blue spine on purpose —
/// timing should be impossible to miss).
pub fn style_duration_chip(live: bool) -> Style {
    // Both stay on the gold chrome spine, live carrying the stronger tint so a
    // running card outranks a finished one. Violet is deliberately absent: it
    // means model thought and nothing else, and a running `bash` is not a
    // thought. See `style_thought_chip`.
    let bg = if live { NUR_GOLD() } else { META_BLUE_SKY() };
    Style::default()
        .fg(BG())
        .bg(bg)
        .add_modifier(Modifier::BOLD)
}

/// Chip for the model's thinking time - the one duration that is violet.
pub fn style_thought_chip() -> Style {
    Style::default()
        .fg(BG())
        .bg(VIOLET())
        .add_modifier(Modifier::BOLD)
}

/// Style for turn-complete duration chip.
pub fn style_turn_chip(interrupted: bool) -> Style {
    if interrupted {
        Style::default()
            .fg(BG())
            .bg(WARN())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(BG())
            .bg(SUCCESS())
            .add_modifier(Modifier::BOLD)
    }
}

/// Decorative activity strip for the busy line (perceived progress, not real %).
pub fn activity_bar(elapsed: Duration, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    // Sweep a bright segment with ease-out restarts every ~1.6s.
    let cycle_ms = 1600u128;
    let t = (elapsed.as_millis() % cycle_ms) as f64 / cycle_ms as f64;
    let head = (ease_out(t) * (width as f64 + 2.0)) as isize;
    let mut out = String::with_capacity(width);
    for i in 0..width as isize {
        let dist = (i - head).abs();
        out.push(match dist {
            0 => '━',
            1 => '─',
            _ => '·',
        });
    }
    out
}

// ── ratatui styles ─────────────────────────────────────────────────────────
#[allow(dead_code)]
pub fn style_title() -> Style {
    Style::default()
        .fg(META_BLUE())
        .add_modifier(Modifier::BOLD)
}

pub fn style_status() -> Style {
    Style::default().fg(MUTED())
}

pub fn style_faint() -> Style {
    Style::default().fg(FAINT())
}

pub fn style_user() -> Style {
    Style::default().fg(USER()).add_modifier(Modifier::BOLD)
}

pub fn style_assistant() -> Style {
    Style::default().fg(ASSISTANT_FG())
}

/// Secondary lines under an answer (e.g. meta footnotes).
#[allow(dead_code)]
pub fn style_assistant_dim() -> Style {
    Style::default().fg(ASSISTANT_DIM())
}

pub fn style_tool() -> Style {
    Style::default().fg(TEAL())
}

/// Tool result body: soft tint from the tool family (not plain grey).
pub fn style_tool_result(name: &str) -> Style {
    Style::default().fg(dim(tool_color(name), 0.28))
}

pub fn style_success() -> Style {
    Style::default().fg(SUCCESS())
}

pub fn style_warn() -> Style {
    Style::default().fg(WARN())
}

pub fn style_error() -> Style {
    Style::default().fg(ERROR())
}

/// Reasoning / "thinking" text — violet, so model thought is never confused
/// with tool output or the assistant's actual answer.
pub fn style_thinking_violet() -> Style {
    Style::default().fg(VIOLET()).add_modifier(Modifier::ITALIC)
}

#[allow(dead_code)]
pub fn style_thinking() -> Style {
    Style::default().fg(MUTED()).add_modifier(Modifier::ITALIC)
}

pub fn style_canvas() -> Style {
    Style::default().bg(BG()).fg(FG())
}

pub fn style_surface() -> Style {
    Style::default().bg(SURFACE()).fg(FG())
}

/// Input caret / stream caret: reverse gold block.
pub fn style_cursor_on() -> Style {
    Style::default()
        .fg(BG())
        .bg(NUR_GOLD())
        .add_modifier(Modifier::BOLD)
}

// ── stdout helpers (headless / subcommands) ────────────────────────────────
/// RGB triple of a themed color, for the `colored` crate (falls back to canvas).
fn tc(c: Color) -> (u8, u8, u8) {
    match c {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (232, 185, 35),
    }
}

#[allow(dead_code)]
pub fn banner() {
    let rows = [
        r#" ███╗   ██╗██╗   ██╗██████╗ "#,
        r#" ████╗  ██║██║   ██║██╔══██╗"#,
        r#" ██╔██╗ ██║██║   ██║██████╔╝"#,
        r#" ██║╚██╗██║██║   ██║██╔══██╗"#,
        r#" ██║ ╚████║╚██████╔╝██║  ██║"#,
        r#" ╚═╝  ╚═══╝ ╚═════╝ ╚═╝  ╚═╝"#,
    ];
    let grad = current().gradient;
    println!();
    for (i, row) in rows.iter().enumerate() {
        let (r, g, b) = grad[i.min(grad.len() - 1)];
        println!("{}", row.truecolor(r, g, b));
    }
    let (ar, ag, ab) = tc(NUR_GOLD());
    let (mr, mg, mb) = tc(MUTED());
    println!(
        "  {}  {}  {}   {}",
        "NurCLI".truecolor(ar, ag, ab).bold(),
        "·".truecolor(mr, mg, mb),
        "multi-provider coding agent".truecolor(200, 190, 170),
        format!("v{}", env!("CARGO_PKG_VERSION")).truecolor(96, 90, 78)
    );
    println!(
        "  {}\n",
        "fully loaded  ·  TUI · tools · Graphify/PLUR/Ruflo · 800+ skills".truecolor(120, 112, 96)
    );
}

pub fn print_info(msg: &str) {
    let (r, g, b) = tc(NUR_GOLD());
    println!("{} {}", "●".truecolor(r, g, b), msg);
}

pub fn print_ok(msg: &str) {
    let (r, g, b) = tc(SUCCESS());
    println!("{} {}", "✓".truecolor(r, g, b), msg);
}

pub fn print_err(msg: &str) {
    let (r, g, b) = tc(ERROR());
    eprintln!("{} {}", "✗".truecolor(r, g, b), msg);
}

pub fn print_tool(name: &str, detail: &str) {
    let (ar, ag, ab) = tc(NUR_GOLD());
    let (mr, mg, mb) = tc(MUTED());
    println!(
        "{} {} {}",
        "●".truecolor(ar, ag, ab),
        name.truecolor(ar, ag, ab).bold(),
        detail.truecolor(mr, mg, mb)
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(c: Color) -> (f64, f64, f64) {
        match c {
            Color::Rgb(r, g, b) => (r as f64, g as f64, b as f64),
            other => panic!("expected an Rgb colour, got {other:?}"),
        }
    }

    /// WCAG relative luminance.
    fn luminance(c: Color) -> f64 {
        let (r, g, b) = rgb(c);
        let lin = |v: f64| {
            let v = v / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
    }

    fn contrast(a: Color, b: Color) -> f64 {
        let (x, y) = (luminance(a), luminance(b));
        let (hi, lo) = if x > y { (x, y) } else { (y, x) };
        (hi + 0.05) / (lo + 0.05)
    }

    fn distance(a: Color, b: Color) -> f64 {
        let (ar, ag, ab) = rgb(a);
        let (br, bg, bb) = rgb(b);
        ((ar - br).powi(2) + (ag - bg).powi(2) + (ab - bb).powi(2)).sqrt()
    }

    /// FAINT is not decoration - it carries every affordance hint in the TUI
    /// ("click to peek", "▸ expands", modal key hints, diff context). It was
    /// 2.83:1 on BG, below the 3:1 floor for legible text.
    #[test]
    fn hint_and_secondary_text_clear_the_contrast_floor() {
        assert!(
            contrast(FAINT(), BG()) >= 3.0,
            "FAINT on BG is {:.2}:1",
            contrast(FAINT(), BG())
        );
        assert!(
            contrast(FAINT(), SURFACE_2()) >= 3.0,
            "FAINT on SURFACE_2 is {:.2}:1",
            contrast(FAINT(), SURFACE_2())
        );
        // MUTED outranks FAINT - the hierarchy has to survive any retune.
        assert!(contrast(MUTED(), BG()) > contrast(FAINT(), BG()));
        assert!(contrast(FG(), BG()) >= 7.0);
    }

    /// Colours that mean different things must look different. Each of these
    /// pairs was close enough to be indistinguishable in a terminal.
    #[test]
    fn distinct_roles_use_distinguishable_colours() {
        // "assistant is answering" vs "git tool".
        assert!(
            distance(SEAFOAM(), CYAN()) > 40.0,
            "SEAFOAM/CYAN distance {:.0}",
            distance(SEAFOAM(), CYAN())
        );
        // Shell-tool family vs warning status. Status colours are never family hues.
        assert!(
            distance(AMBER(), WARN()) > 20.0,
            "AMBER/WARN distance {:.0}",
            distance(AMBER(), WARN())
        );
        assert_ne!(AMBER(), WARN(), "a shell card must not read as a warning");
    }

    /// `Tone` exists so system notices are each visually distinct rather than
    /// all reading as "blue info" - so no two tones may share a colour, and the
    /// glyph is the colour-blind fallback, so no two may share that either.
    #[test]
    fn every_tone_is_visually_distinct() {
        let tones = [
            Tone::Neutral,
            Tone::Mode,
            Tone::Plan,
            Tone::Todos,
            Tone::Usage,
            Tone::Memory,
            Tone::Session,
            Tone::Skill,
        ];
        for (i, a) in tones.iter().enumerate() {
            for b in &tones[i + 1..] {
                assert!(
                    distance(a.color(), b.color()) > 20.0,
                    "{a:?} and {b:?} share a colour"
                );
                assert_ne!(a.glyph(), b.glyph(), "{a:?} and {b:?} share a glyph");
            }
            assert!(
                contrast(a.color(), BG()) >= 4.5,
                "{a:?} is unreadable on BG: {:.2}:1",
                contrast(a.color(), BG())
            );
        }
    }

    /// Violet means model thought and only that. Duration chips sit on the gold
    /// chrome spine; the thought chip is the single violet one.
    #[test]
    fn violet_is_reserved_for_thought() {
        let bg_of = |s: Style| s.bg.expect("chips set a background");
        assert_eq!(bg_of(style_thought_chip()), VIOLET());
        assert_ne!(
            bg_of(style_duration_chip(true)),
            VIOLET(),
            "a running tool is not a thought"
        );
        assert_ne!(bg_of(style_duration_chip(false)), VIOLET());
        // Live still outranks finished.
        assert_ne!(
            bg_of(style_duration_chip(true)),
            bg_of(style_duration_chip(false)),
            "live and settled chips must be tellable apart"
        );
        // Chips are dark-on-light: the text has to survive the background.
        for s in [
            style_thought_chip(),
            style_duration_chip(true),
            style_duration_chip(false),
        ] {
            assert!(contrast(bg_of(s), BG()) >= 4.5);
        }
    }

    /// Every registered theme applies and swaps the palette live.
    #[test]
    fn themes_switch_the_active_palette() {
        assert!(set_theme("mono"));
        assert_eq!(current_theme_name(), "mono");
        assert_eq!(current().nur_gold, super::preset("mono").unwrap().nur_gold);
        assert!(set_theme("default"));
        assert_eq!(current_theme_name(), "gold");
        assert!(!set_theme("does-not-exist"));
        // Restore the default so other tests see gold.
        assert!(set_theme("gold"));
        assert_eq!(current().nur_gold, GOLD.nur_gold);
    }
}

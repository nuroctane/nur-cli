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
    /// Text color for content drawn ON accent backgrounds (selected rows,
    /// active chips). Dark palettes reuse their near-black bg; light palettes
    /// and silver-accent palettes flip to deep ink so highlights stay legible.
    pub on_accent_fg: Color,
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
    on_accent_fg: Color::Rgb(11, 14, 18),
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
    // Lights.
    ("heavenly", "Heavenly White - warm ivory, amethyst + rose pearl"),
    ("pearl", "Pearlescent - milky opal, rose + champagne sheen"),
    ("off-white", "Off White - warm paper, graphite ink"),
    // Dark monochrome / chrome.
    ("noir", "Noir - film-noir silver on near-black"),
    ("black", "Black - true OLED black, pure white"),
    ("off-black", "Off Black - soft charcoal, easy on the eyes"),
    ("chrome", "Chrome - dark glass grey, polished silver"),
    // Creative.
    ("synthwave", "Synthwave - neon pink + cyan over purple night"),
    ("matrix", "Matrix - phosphor green terminal"),
    ("dracula", "Dracula - the classic purple palace"),
    ("nord", "Nord - arctic frost"),
    ("gruvbox", "Gruvbox - warm retro earth"),
    ("sakura", "Sakura - dusk plum + cherry blossom"),
    ("abyss", "Abyss - deep-ocean teal, bioluminescent"),
    ("moss", "Moss - forest-floor greens"),
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
        // ── lights ───────────────────────────────────────────────────────
        // Heavenly White: warm ivory + beige with amethyst / rose / opal /
        // champagne accents. Strictly no blues — every accent keeps red >=
        // blue so nothing reads cold.
        "heavenly" => Palette {
            on_accent_fg: Color::Rgb(58, 52, 44),
            nur_gold: Color::Rgb(198, 156, 92),
            nur_gold_deep: Color::Rgb(160, 120, 54),
            nur_gold_sky: Color::Rgb(232, 208, 160),
            bg: Color::Rgb(253, 251, 246),
            surface: Color::Rgb(250, 247, 240),
            surface_2: Color::Rgb(245, 241, 232),
            surface_3: Color::Rgb(238, 232, 220),
            fg: Color::Rgb(58, 52, 44),
            muted: Color::Rgb(138, 128, 112),
            faint: Color::Rgb(168, 158, 140),
            border: Color::Rgb(226, 219, 205),
            code_bg: Color::Rgb(248, 245, 238),
            md_code: Color::Rgb(146, 88, 60),
            md_h1: Color::Rgb(122, 84, 168),
            md_h2: Color::Rgb(188, 96, 130),
            md_h3: Color::Rgb(128, 92, 148),
            md_link: Color::Rgb(178, 76, 110),
            md_quote: Color::Rgb(140, 126, 104),
            md_list: Color::Rgb(88, 134, 90),
            assistant_fg: Color::Rgb(62, 56, 48),
            assistant_dim: Color::Rgb(130, 120, 105),
            success: Color::Rgb(74, 132, 84),
            warn: Color::Rgb(190, 132, 32),
            error: Color::Rgb(196, 74, 84),
            diff_add_fg: Color::Rgb(46, 110, 62),
            diff_add_bg: Color::Rgb(233, 244, 234),
            diff_del_fg: Color::Rgb(158, 52, 64),
            diff_del_bg: Color::Rgb(250, 234, 236),
            diff_meta: Color::Rgb(156, 118, 60),
            user: Color::Rgb(40, 34, 28),
            blue_050: Color::Rgb(250, 244, 230),
            blue_100: Color::Rgb(242, 230, 204),
            blue_150: Color::Rgb(232, 214, 176),
            blue_200: Color::Rgb(218, 194, 148),
            blue_250: Color::Rgb(206, 176, 118),
            blue_300: Color::Rgb(198, 156, 92),
            blue_400: Color::Rgb(198, 156, 92),
            blue_500: Color::Rgb(160, 120, 54),
            blue_600: Color::Rgb(122, 90, 38),
            indigo: Color::Rgb(132, 88, 172),
            periwinkle: Color::Rgb(172, 140, 190),
            violet: Color::Rgb(148, 100, 190),
            lavender: Color::Rgb(196, 170, 210),
            magenta: Color::Rgb(186, 96, 170),
            pink: Color::Rgb(214, 130, 160),
            rose: Color::Rgb(208, 110, 130),
            coral: Color::Rgb(222, 130, 110),
            amber: Color::Rgb(202, 142, 40),
            gold: Color::Rgb(198, 152, 70),
            orange: Color::Rgb(208, 130, 70),
            lime: Color::Rgb(130, 160, 70),
            mint: Color::Rgb(96, 158, 120),
            seafoam: Color::Rgb(96, 150, 130),
            teal: Color::Rgb(70, 136, 120),
            cyan: Color::Rgb(88, 150, 140),
            gradient: ramp6(Color::Rgb(232, 208, 160), Color::Rgb(156, 116, 52)),
            aurora: ring12(Color::Rgb(236, 216, 176), Color::Rgb(150, 110, 50)),
            ..GOLD
        },
        // Pearlescent: milky opal whites with rose / amethyst / champagne
        // iridescence and a whisper of opal-green. Cool greys stay warm-neutral.
        "pearl" => Palette {
            on_accent_fg: Color::Rgb(54, 50, 54),
            nur_gold: Color::Rgb(192, 144, 148),
            nur_gold_deep: Color::Rgb(152, 108, 114),
            nur_gold_sky: Color::Rgb(238, 214, 214),
            bg: Color::Rgb(250, 249, 247),
            surface: Color::Rgb(246, 244, 242),
            surface_2: Color::Rgb(240, 238, 236),
            surface_3: Color::Rgb(232, 229, 226),
            fg: Color::Rgb(54, 50, 54),
            muted: Color::Rgb(140, 134, 138),
            faint: Color::Rgb(170, 164, 168),
            border: Color::Rgb(218, 214, 210),
            code_bg: Color::Rgb(244, 242, 240),
            md_code: Color::Rgb(150, 96, 110),
            md_h1: Color::Rgb(138, 96, 170),
            md_h2: Color::Rgb(196, 110, 140),
            md_h3: Color::Rgb(96, 142, 118),
            md_link: Color::Rgb(168, 92, 150),
            md_quote: Color::Rgb(146, 136, 132),
            md_list: Color::Rgb(100, 146, 120),
            assistant_fg: Color::Rgb(58, 54, 58),
            assistant_dim: Color::Rgb(132, 126, 130),
            success: Color::Rgb(84, 140, 106),
            warn: Color::Rgb(196, 140, 60),
            error: Color::Rgb(198, 84, 96),
            diff_add_fg: Color::Rgb(52, 116, 78),
            diff_add_bg: Color::Rgb(235, 244, 237),
            diff_del_fg: Color::Rgb(162, 60, 72),
            diff_del_bg: Color::Rgb(249, 236, 238),
            diff_meta: Color::Rgb(160, 122, 118),
            user: Color::Rgb(38, 35, 38),
            blue_050: Color::Rgb(246, 236, 236),
            blue_100: Color::Rgb(238, 220, 222),
            blue_150: Color::Rgb(226, 200, 204),
            blue_200: Color::Rgb(212, 178, 184),
            blue_250: Color::Rgb(202, 160, 166),
            blue_300: Color::Rgb(192, 144, 148),
            blue_400: Color::Rgb(192, 144, 148),
            blue_500: Color::Rgb(152, 108, 114),
            blue_600: Color::Rgb(118, 82, 88),
            indigo: Color::Rgb(138, 96, 170),
            periwinkle: Color::Rgb(176, 146, 186),
            violet: Color::Rgb(154, 108, 188),
            lavender: Color::Rgb(200, 176, 206),
            magenta: Color::Rgb(190, 102, 172),
            pink: Color::Rgb(216, 136, 162),
            rose: Color::Rgb(210, 116, 134),
            coral: Color::Rgb(224, 136, 116),
            amber: Color::Rgb(204, 146, 66),
            gold: Color::Rgb(200, 156, 96),
            orange: Color::Rgb(210, 136, 92),
            lime: Color::Rgb(134, 162, 96),
            mint: Color::Rgb(100, 160, 128),
            seafoam: Color::Rgb(100, 152, 134),
            teal: Color::Rgb(76, 138, 124),
            cyan: Color::Rgb(94, 152, 142),
            gradient: ramp6(Color::Rgb(238, 214, 214), Color::Rgb(152, 108, 114)),
            aurora: ring12(Color::Rgb(240, 220, 220), Color::Rgb(146, 102, 108)),
            ..GOLD
        },
        // Off White ("Bone"): warm paper + graphite ink, near-monochrome.
        // The graphite accent is itself dark, so highlight text flips to
        // paper-white instead of ink.
        "off-white" => Palette {
            on_accent_fg: Color::Rgb(248, 246, 242),
            nur_gold: Color::Rgb(110, 104, 94),
            nur_gold_deep: Color::Rgb(80, 75, 66),
            nur_gold_sky: Color::Rgb(210, 204, 192),
            bg: Color::Rgb(248, 246, 242),
            surface: Color::Rgb(243, 240, 235),
            surface_2: Color::Rgb(236, 232, 226),
            surface_3: Color::Rgb(226, 221, 213),
            fg: Color::Rgb(42, 40, 37),
            muted: Color::Rgb(120, 115, 106),
            faint: Color::Rgb(155, 149, 139),
            border: Color::Rgb(212, 206, 196),
            code_bg: Color::Rgb(240, 237, 231),
            md_code: Color::Rgb(100, 95, 86),
            md_h1: Color::Rgb(30, 28, 25),
            md_h2: Color::Rgb(62, 58, 52),
            md_h3: Color::Rgb(92, 87, 79),
            md_link: Color::Rgb(90, 85, 78),
            md_quote: Color::Rgb(120, 114, 105),
            md_list: Color::Rgb(98, 92, 83),
            assistant_fg: Color::Rgb(46, 44, 41),
            assistant_dim: Color::Rgb(118, 113, 105),
            success: Color::Rgb(70, 130, 80),
            warn: Color::Rgb(176, 128, 40),
            error: Color::Rgb(180, 70, 70),
            diff_add_fg: Color::Rgb(50, 112, 64),
            diff_add_bg: Color::Rgb(234, 242, 234),
            diff_del_fg: Color::Rgb(160, 60, 60),
            diff_del_bg: Color::Rgb(248, 235, 234),
            diff_meta: Color::Rgb(130, 122, 110),
            user: Color::Rgb(20, 19, 17),
            blue_050: Color::Rgb(242, 239, 233),
            blue_100: Color::Rgb(228, 224, 216),
            blue_150: Color::Rgb(210, 205, 195),
            blue_200: Color::Rgb(190, 184, 173),
            blue_250: Color::Rgb(160, 154, 143),
            blue_300: Color::Rgb(110, 104, 94),
            blue_400: Color::Rgb(110, 104, 94),
            blue_500: Color::Rgb(80, 75, 66),
            blue_600: Color::Rgb(58, 54, 47),
            indigo: Color::Rgb(96, 90, 110),
            periwinkle: Color::Rgb(130, 124, 138),
            violet: Color::Rgb(110, 102, 120),
            lavender: Color::Rgb(160, 154, 164),
            magenta: Color::Rgb(140, 110, 125),
            pink: Color::Rgb(160, 125, 140),
            rose: Color::Rgb(150, 110, 118),
            coral: Color::Rgb(165, 115, 100),
            amber: Color::Rgb(170, 130, 60),
            gold: Color::Rgb(150, 135, 95),
            orange: Color::Rgb(160, 115, 80),
            lime: Color::Rgb(115, 135, 80),
            mint: Color::Rgb(95, 135, 110),
            seafoam: Color::Rgb(95, 130, 115),
            teal: Color::Rgb(80, 118, 106),
            cyan: Color::Rgb(95, 128, 120),
            gradient: ramp6(Color::Rgb(210, 204, 192), Color::Rgb(80, 75, 66)),
            aurora: ring12(Color::Rgb(214, 208, 196), Color::Rgb(84, 78, 68)),
            ..GOLD
        },
        // ── dark monochrome / chrome ────────────────────────────────────
        // Noir: film-noir near-black with high-contrast silver-white.
        "noir" => Palette {
            on_accent_fg: Color::Rgb(12, 12, 14),
            nur_gold: Color::Rgb(222, 222, 228),
            nur_gold_deep: Color::Rgb(152, 152, 162),
            nur_gold_sky: Color::Rgb(246, 246, 250),
            bg: Color::Rgb(12, 12, 14),
            surface: Color::Rgb(18, 18, 21),
            surface_2: Color::Rgb(27, 27, 31),
            surface_3: Color::Rgb(39, 39, 45),
            border: Color::Rgb(58, 58, 66),
            code_bg: Color::Rgb(15, 15, 17),
            fg: Color::Rgb(236, 236, 240),
            muted: Color::Rgb(152, 152, 160),
            faint: Color::Rgb(112, 112, 122),
            md_code: Color::Rgb(190, 200, 208),
            md_h1: Color::Rgb(250, 250, 252),
            md_h2: Color::Rgb(210, 210, 218),
            md_h3: Color::Rgb(172, 172, 182),
            md_link: Color::Rgb(200, 205, 215),
            md_quote: Color::Rgb(140, 140, 150),
            md_list: Color::Rgb(170, 175, 185),
            assistant_fg: Color::Rgb(230, 230, 236),
            assistant_dim: Color::Rgb(150, 150, 160),
            success: Color::Rgb(120, 200, 145),
            warn: Color::Rgb(225, 185, 100),
            error: Color::Rgb(235, 105, 105),
            diff_meta: Color::Rgb(190, 190, 200),
            user: Color::Rgb(255, 255, 255),
            blue_050: Color::Rgb(248, 248, 252),
            blue_100: Color::Rgb(238, 238, 243),
            blue_150: Color::Rgb(228, 228, 234),
            blue_200: Color::Rgb(222, 222, 228),
            blue_250: Color::Rgb(232, 232, 238),
            blue_300: Color::Rgb(222, 222, 228),
            blue_400: Color::Rgb(222, 222, 228),
            blue_500: Color::Rgb(152, 152, 162),
            blue_600: Color::Rgb(108, 108, 118),
            indigo: Color::Rgb(165, 165, 180),
            periwinkle: Color::Rgb(185, 185, 198),
            violet: Color::Rgb(175, 175, 190),
            lavender: Color::Rgb(205, 205, 214),
            magenta: Color::Rgb(190, 170, 195),
            pink: Color::Rgb(205, 175, 190),
            rose: Color::Rgb(215, 165, 175),
            coral: Color::Rgb(220, 175, 160),
            amber: Color::Rgb(225, 190, 120),
            gold: Color::Rgb(230, 210, 150),
            orange: Color::Rgb(220, 180, 140),
            lime: Color::Rgb(190, 210, 150),
            mint: Color::Rgb(160, 205, 175),
            seafoam: Color::Rgb(150, 200, 185),
            teal: Color::Rgb(140, 190, 180),
            cyan: Color::Rgb(160, 200, 195),
            gradient: ramp6(Color::Rgb(246, 246, 250), Color::Rgb(90, 90, 100)),
            aurora: ring12(Color::Rgb(248, 248, 252), Color::Rgb(92, 92, 102)),
            ..GOLD
        },
        // Black: true OLED black, pure white text, grey steps only.
        "black" => Palette {
            on_accent_fg: Color::Rgb(0, 0, 0),
            nur_gold: Color::Rgb(255, 255, 255),
            nur_gold_deep: Color::Rgb(178, 178, 182),
            nur_gold_sky: Color::Rgb(255, 255, 255),
            bg: Color::Rgb(0, 0, 0),
            surface: Color::Rgb(10, 10, 10),
            surface_2: Color::Rgb(20, 20, 20),
            surface_3: Color::Rgb(32, 32, 32),
            border: Color::Rgb(48, 48, 48),
            code_bg: Color::Rgb(6, 6, 6),
            fg: Color::Rgb(242, 242, 242),
            muted: Color::Rgb(150, 150, 150),
            faint: Color::Rgb(108, 108, 108),
            md_code: Color::Rgb(200, 200, 205),
            md_h1: Color::Rgb(255, 255, 255),
            md_h2: Color::Rgb(215, 215, 218),
            md_h3: Color::Rgb(175, 175, 180),
            md_link: Color::Rgb(225, 225, 230),
            md_quote: Color::Rgb(140, 140, 144),
            md_list: Color::Rgb(185, 185, 190),
            assistant_fg: Color::Rgb(238, 238, 240),
            assistant_dim: Color::Rgb(148, 148, 152),
            success: Color::Rgb(130, 220, 160),
            warn: Color::Rgb(235, 195, 110),
            error: Color::Rgb(240, 110, 110),
            diff_meta: Color::Rgb(200, 200, 200),
            user: Color::Rgb(255, 255, 255),
            blue_050: Color::Rgb(255, 255, 255),
            blue_100: Color::Rgb(245, 245, 245),
            blue_150: Color::Rgb(235, 235, 235),
            blue_200: Color::Rgb(225, 225, 225),
            blue_250: Color::Rgb(240, 240, 240),
            blue_300: Color::Rgb(255, 255, 255),
            blue_400: Color::Rgb(255, 255, 255),
            blue_500: Color::Rgb(178, 178, 182),
            blue_600: Color::Rgb(120, 120, 124),
            indigo: Color::Rgb(175, 175, 195),
            periwinkle: Color::Rgb(195, 195, 210),
            violet: Color::Rgb(185, 185, 200),
            lavender: Color::Rgb(215, 215, 225),
            magenta: Color::Rgb(200, 175, 200),
            pink: Color::Rgb(210, 180, 195),
            rose: Color::Rgb(220, 170, 180),
            coral: Color::Rgb(225, 180, 165),
            amber: Color::Rgb(230, 200, 130),
            gold: Color::Rgb(235, 220, 160),
            orange: Color::Rgb(225, 190, 150),
            lime: Color::Rgb(195, 220, 160),
            mint: Color::Rgb(165, 215, 180),
            seafoam: Color::Rgb(155, 210, 190),
            teal: Color::Rgb(145, 205, 185),
            cyan: Color::Rgb(165, 210, 200),
            gradient: ramp6(Color::Rgb(255, 255, 255), Color::Rgb(90, 90, 90)),
            aurora: ring12(Color::Rgb(255, 255, 255), Color::Rgb(92, 92, 92)),
            ..GOLD
        },
        // Off Black: soft charcoal that's easy on the eyes, warm-white text.
        "off-black" => Palette {
            on_accent_fg: Color::Rgb(11, 11, 13),
            nur_gold: Color::Rgb(205, 201, 194),
            nur_gold_deep: Color::Rgb(140, 136, 129),
            nur_gold_sky: Color::Rgb(236, 233, 228),
            bg: Color::Rgb(11, 11, 13),
            surface: Color::Rgb(17, 17, 20),
            surface_2: Color::Rgb(25, 25, 29),
            surface_3: Color::Rgb(36, 36, 41),
            border: Color::Rgb(50, 50, 57),
            code_bg: Color::Rgb(13, 13, 15),
            fg: Color::Rgb(226, 224, 220),
            muted: Color::Rgb(141, 139, 134),
            faint: Color::Rgb(104, 102, 99),
            md_code: Color::Rgb(188, 185, 179),
            md_h1: Color::Rgb(240, 238, 234),
            md_h2: Color::Rgb(205, 202, 196),
            md_h3: Color::Rgb(170, 167, 161),
            md_link: Color::Rgb(205, 202, 196),
            md_quote: Color::Rgb(135, 133, 128),
            md_list: Color::Rgb(175, 172, 166),
            assistant_fg: Color::Rgb(220, 218, 214),
            assistant_dim: Color::Rgb(142, 140, 135),
            success: Color::Rgb(125, 205, 150),
            warn: Color::Rgb(228, 188, 105),
            error: Color::Rgb(235, 110, 110),
            diff_meta: Color::Rgb(195, 192, 186),
            user: Color::Rgb(245, 244, 240),
            blue_050: Color::Rgb(240, 238, 234),
            blue_100: Color::Rgb(228, 226, 221),
            blue_150: Color::Rgb(216, 213, 207),
            blue_200: Color::Rgb(205, 201, 194),
            blue_250: Color::Rgb(216, 213, 207),
            blue_300: Color::Rgb(205, 201, 194),
            blue_400: Color::Rgb(205, 201, 194),
            blue_500: Color::Rgb(140, 136, 129),
            blue_600: Color::Rgb(98, 95, 89),
            indigo: Color::Rgb(160, 157, 168),
            periwinkle: Color::Rgb(180, 177, 186),
            violet: Color::Rgb(170, 167, 178),
            lavender: Color::Rgb(198, 195, 204),
            magenta: Color::Rgb(185, 165, 180),
            pink: Color::Rgb(198, 172, 184),
            rose: Color::Rgb(208, 165, 174),
            coral: Color::Rgb(214, 172, 158),
            amber: Color::Rgb(220, 188, 125),
            gold: Color::Rgb(224, 208, 158),
            orange: Color::Rgb(214, 178, 140),
            lime: Color::Rgb(186, 206, 152),
            mint: Color::Rgb(158, 202, 172),
            seafoam: Color::Rgb(148, 198, 182),
            teal: Color::Rgb(138, 188, 178),
            cyan: Color::Rgb(158, 198, 192),
            gradient: ramp6(Color::Rgb(236, 233, 228), Color::Rgb(90, 88, 84)),
            aurora: ring12(Color::Rgb(238, 235, 230), Color::Rgb(92, 90, 86)),
            ..GOLD
        },
        // Chrome: dark glass grey (Chrome-dark toolbar tones) with polished
        // silver-white highlights - the chromey feel.
        "chrome" => Palette {
            on_accent_fg: Color::Rgb(32, 33, 36),
            nur_gold: Color::Rgb(222, 224, 228),
            nur_gold_deep: Color::Rgb(154, 158, 165),
            nur_gold_sky: Color::Rgb(242, 244, 247),
            bg: Color::Rgb(32, 33, 36),
            surface: Color::Rgb(39, 40, 43),
            surface_2: Color::Rgb(48, 49, 53),
            surface_3: Color::Rgb(61, 62, 67),
            border: Color::Rgb(74, 75, 81),
            code_bg: Color::Rgb(35, 36, 40),
            fg: Color::Rgb(248, 249, 251),
            muted: Color::Rgb(155, 160, 167),
            faint: Color::Rgb(118, 123, 130),
            md_code: Color::Rgb(206, 209, 214),
            md_h1: Color::Rgb(252, 253, 255),
            md_h2: Color::Rgb(222, 224, 229),
            md_h3: Color::Rgb(190, 193, 199),
            md_link: Color::Rgb(226, 228, 233),
            md_quote: Color::Rgb(150, 155, 162),
            md_list: Color::Rgb(195, 198, 204),
            assistant_fg: Color::Rgb(244, 245, 248),
            assistant_dim: Color::Rgb(158, 163, 170),
            success: Color::Rgb(110, 205, 150),
            warn: Color::Rgb(242, 195, 100),
            error: Color::Rgb(245, 115, 115),
            diff_meta: Color::Rgb(205, 208, 214),
            user: Color::Rgb(255, 255, 255),
            blue_050: Color::Rgb(248, 249, 251),
            blue_100: Color::Rgb(236, 238, 241),
            blue_150: Color::Rgb(224, 226, 230),
            blue_200: Color::Rgb(210, 213, 218),
            blue_250: Color::Rgb(232, 234, 238),
            blue_300: Color::Rgb(222, 224, 228),
            blue_400: Color::Rgb(222, 224, 228),
            blue_500: Color::Rgb(154, 158, 165),
            blue_600: Color::Rgb(110, 113, 120),
            indigo: Color::Rgb(168, 171, 182),
            periwinkle: Color::Rgb(188, 191, 200),
            violet: Color::Rgb(178, 181, 192),
            lavender: Color::Rgb(208, 211, 218),
            magenta: Color::Rgb(195, 178, 200),
            pink: Color::Rgb(208, 183, 196),
            rose: Color::Rgb(218, 175, 185),
            coral: Color::Rgb(222, 182, 168),
            amber: Color::Rgb(228, 196, 130),
            gold: Color::Rgb(232, 216, 165),
            orange: Color::Rgb(224, 188, 148),
            lime: Color::Rgb(194, 214, 158),
            mint: Color::Rgb(162, 210, 180),
            seafoam: Color::Rgb(152, 205, 190),
            teal: Color::Rgb(142, 198, 186),
            cyan: Color::Rgb(162, 204, 198),
            gradient: ramp6(Color::Rgb(242, 244, 247), Color::Rgb(100, 103, 110)),
            aurora: ring12(Color::Rgb(244, 246, 249), Color::Rgb(102, 105, 112)),
            ..GOLD
        },
        // ── creative ─────────────────────────────────────────────────────
        // Synthwave: neon pink + cyan over a deep purple night.
        "synthwave" => Palette {
            on_accent_fg: Color::Rgb(24, 10, 34),
            nur_gold: Color::Rgb(255, 84, 168),
            nur_gold_deep: Color::Rgb(200, 44, 124),
            nur_gold_sky: Color::Rgb(255, 150, 205),
            bg: Color::Rgb(24, 10, 34),
            surface: Color::Rgb(32, 15, 45),
            surface_2: Color::Rgb(43, 20, 59),
            surface_3: Color::Rgb(57, 27, 77),
            border: Color::Rgb(76, 35, 100),
            code_bg: Color::Rgb(19, 8, 27),
            fg: Color::Rgb(240, 226, 248),
            muted: Color::Rgb(172, 142, 192),
            faint: Color::Rgb(128, 103, 148),
            md_code: Color::Rgb(96, 220, 230),
            md_h1: Color::Rgb(255, 150, 205),
            md_h2: Color::Rgb(100, 220, 235),
            md_h3: Color::Rgb(196, 130, 255),
            md_link: Color::Rgb(255, 120, 190),
            md_quote: Color::Rgb(160, 130, 180),
            md_list: Color::Rgb(110, 225, 185),
            assistant_fg: Color::Rgb(236, 222, 244),
            assistant_dim: Color::Rgb(168, 140, 188),
            success: Color::Rgb(110, 230, 160),
            warn: Color::Rgb(255, 200, 90),
            error: Color::Rgb(255, 95, 120),
            diff_meta: Color::Rgb(230, 150, 200),
            user: Color::Rgb(255, 245, 252),
            blue_050: Color::Rgb(250, 226, 240),
            blue_100: Color::Rgb(255, 190, 222),
            blue_150: Color::Rgb(255, 150, 205),
            blue_200: Color::Rgb(255, 110, 185),
            blue_250: Color::Rgb(255, 95, 175),
            blue_300: Color::Rgb(255, 84, 168),
            blue_400: Color::Rgb(255, 84, 168),
            blue_500: Color::Rgb(200, 44, 124),
            blue_600: Color::Rgb(150, 30, 92),
            indigo: Color::Rgb(150, 110, 255),
            periwinkle: Color::Rgb(180, 150, 255),
            violet: Color::Rgb(196, 130, 255),
            lavender: Color::Rgb(220, 180, 255),
            magenta: Color::Rgb(255, 110, 200),
            pink: Color::Rgb(255, 140, 195),
            rose: Color::Rgb(255, 105, 155),
            coral: Color::Rgb(255, 130, 120),
            amber: Color::Rgb(255, 200, 100),
            gold: Color::Rgb(255, 210, 120),
            orange: Color::Rgb(255, 150, 100),
            lime: Color::Rgb(160, 240, 130),
            mint: Color::Rgb(110, 230, 185),
            seafoam: Color::Rgb(90, 215, 190),
            teal: Color::Rgb(70, 200, 195),
            cyan: Color::Rgb(96, 220, 230),
            gradient: ramp6(Color::Rgb(255, 150, 205), Color::Rgb(120, 20, 80)),
            aurora: ring12(Color::Rgb(255, 160, 210), Color::Rgb(130, 25, 85)),
            ..GOLD
        },
        // Matrix: phosphor green terminal glow.
        "matrix" => Palette {
            on_accent_fg: Color::Rgb(2, 8, 3),
            nur_gold: Color::Rgb(80, 250, 120),
            nur_gold_deep: Color::Rgb(30, 160, 70),
            nur_gold_sky: Color::Rgb(160, 255, 180),
            bg: Color::Rgb(2, 8, 3),
            surface: Color::Rgb(5, 14, 6),
            surface_2: Color::Rgb(8, 22, 10),
            surface_3: Color::Rgb(13, 33, 16),
            border: Color::Rgb(22, 50, 26),
            code_bg: Color::Rgb(3, 10, 4),
            fg: Color::Rgb(150, 240, 160),
            muted: Color::Rgb(95, 180, 105),
            faint: Color::Rgb(62, 130, 74),
            md_code: Color::Rgb(140, 255, 160),
            md_h1: Color::Rgb(180, 255, 195),
            md_h2: Color::Rgb(120, 235, 135),
            md_h3: Color::Rgb(90, 200, 105),
            md_link: Color::Rgb(140, 255, 160),
            md_quote: Color::Rgb(90, 160, 100),
            md_list: Color::Rgb(110, 220, 125),
            assistant_fg: Color::Rgb(165, 245, 175),
            assistant_dim: Color::Rgb(100, 175, 110),
            success: Color::Rgb(100, 235, 125),
            warn: Color::Rgb(190, 225, 95),
            error: Color::Rgb(240, 100, 85),
            diff_meta: Color::Rgb(110, 210, 125),
            user: Color::Rgb(210, 255, 220),
            blue_050: Color::Rgb(225, 255, 230),
            blue_100: Color::Rgb(190, 255, 205),
            blue_150: Color::Rgb(160, 255, 180),
            blue_200: Color::Rgb(120, 250, 145),
            blue_250: Color::Rgb(95, 250, 128),
            blue_300: Color::Rgb(80, 250, 120),
            blue_400: Color::Rgb(80, 250, 120),
            blue_500: Color::Rgb(30, 160, 70),
            blue_600: Color::Rgb(18, 110, 48),
            indigo: Color::Rgb(110, 200, 160),
            periwinkle: Color::Rgb(130, 210, 170),
            violet: Color::Rgb(120, 210, 150),
            lavender: Color::Rgb(150, 230, 175),
            magenta: Color::Rgb(110, 220, 150),
            pink: Color::Rgb(120, 215, 150),
            rose: Color::Rgb(110, 210, 140),
            coral: Color::Rgb(130, 215, 120),
            amber: Color::Rgb(180, 225, 100),
            gold: Color::Rgb(140, 245, 130),
            orange: Color::Rgb(130, 220, 110),
            lime: Color::Rgb(150, 250, 120),
            mint: Color::Rgb(100, 240, 140),
            seafoam: Color::Rgb(80, 230, 150),
            teal: Color::Rgb(60, 215, 150),
            cyan: Color::Rgb(80, 235, 165),
            gradient: ramp6(Color::Rgb(160, 255, 180), Color::Rgb(10, 90, 25)),
            aurora: ring12(Color::Rgb(170, 255, 190), Color::Rgb(12, 95, 28)),
            ..GOLD
        },
        // Dracula: the classic purple palace.
        "dracula" => Palette {
            on_accent_fg: Color::Rgb(40, 42, 54),
            nur_gold: Color::Rgb(189, 147, 249),
            nur_gold_deep: Color::Rgb(139, 105, 196),
            nur_gold_sky: Color::Rgb(215, 185, 255),
            bg: Color::Rgb(40, 42, 54),
            surface: Color::Rgb(48, 50, 63),
            surface_2: Color::Rgb(58, 60, 76),
            surface_3: Color::Rgb(68, 71, 90),
            border: Color::Rgb(82, 85, 105),
            code_bg: Color::Rgb(34, 36, 46),
            fg: Color::Rgb(248, 248, 242),
            muted: Color::Rgb(149, 152, 175),
            faint: Color::Rgb(108, 111, 133),
            md_code: Color::Rgb(139, 233, 253),
            md_h1: Color::Rgb(255, 121, 198),
            md_h2: Color::Rgb(139, 233, 253),
            md_h3: Color::Rgb(189, 147, 249),
            md_link: Color::Rgb(80, 250, 123),
            md_quote: Color::Rgb(140, 143, 165),
            md_list: Color::Rgb(241, 250, 140),
            assistant_fg: Color::Rgb(244, 244, 238),
            assistant_dim: Color::Rgb(150, 153, 176),
            success: Color::Rgb(80, 250, 123),
            warn: Color::Rgb(241, 250, 140),
            error: Color::Rgb(255, 85, 85),
            diff_add_fg: Color::Rgb(160, 255, 175),
            diff_add_bg: Color::Rgb(44, 66, 50),
            diff_del_fg: Color::Rgb(255, 130, 130),
            diff_del_bg: Color::Rgb(70, 40, 46),
            diff_meta: Color::Rgb(241, 250, 140),
            user: Color::Rgb(255, 255, 255),
            blue_050: Color::Rgb(240, 230, 255),
            blue_100: Color::Rgb(225, 205, 255),
            blue_150: Color::Rgb(210, 180, 255),
            blue_200: Color::Rgb(200, 165, 252),
            blue_250: Color::Rgb(195, 155, 250),
            blue_300: Color::Rgb(189, 147, 249),
            blue_400: Color::Rgb(189, 147, 249),
            blue_500: Color::Rgb(139, 105, 196),
            blue_600: Color::Rgb(100, 74, 145),
            indigo: Color::Rgb(189, 147, 249),
            periwinkle: Color::Rgb(205, 170, 255),
            violet: Color::Rgb(189, 147, 249),
            lavender: Color::Rgb(215, 185, 255),
            magenta: Color::Rgb(255, 121, 198),
            pink: Color::Rgb(255, 121, 198),
            rose: Color::Rgb(255, 105, 165),
            coral: Color::Rgb(255, 145, 145),
            amber: Color::Rgb(255, 184, 108),
            gold: Color::Rgb(241, 250, 140),
            orange: Color::Rgb(255, 184, 108),
            lime: Color::Rgb(241, 250, 140),
            mint: Color::Rgb(80, 250, 123),
            seafoam: Color::Rgb(80, 250, 123),
            teal: Color::Rgb(139, 233, 253),
            cyan: Color::Rgb(139, 233, 253),
            gradient: ramp6(Color::Rgb(215, 185, 255), Color::Rgb(90, 65, 140)),
            aurora: ring12(Color::Rgb(220, 195, 255), Color::Rgb(95, 70, 145)),
            ..GOLD
        },
        // Nord: arctic frost - polar night greys + frost blues.
        "nord" => Palette {
            on_accent_fg: Color::Rgb(46, 52, 64),
            nur_gold: Color::Rgb(129, 161, 193),
            nur_gold_deep: Color::Rgb(94, 129, 172),
            nur_gold_sky: Color::Rgb(163, 190, 214),
            bg: Color::Rgb(46, 52, 64),
            surface: Color::Rgb(54, 60, 72),
            surface_2: Color::Rgb(65, 72, 85),
            surface_3: Color::Rgb(77, 85, 99),
            border: Color::Rgb(62, 70, 84),
            code_bg: Color::Rgb(41, 46, 57),
            fg: Color::Rgb(236, 239, 244),
            muted: Color::Rgb(143, 152, 166),
            faint: Color::Rgb(104, 112, 124),
            md_code: Color::Rgb(136, 192, 208),
            md_h1: Color::Rgb(136, 192, 208),
            md_h2: Color::Rgb(129, 161, 193),
            md_h3: Color::Rgb(180, 142, 173),
            md_link: Color::Rgb(136, 192, 208),
            md_quote: Color::Rgb(116, 125, 138),
            md_list: Color::Rgb(163, 190, 140),
            assistant_fg: Color::Rgb(234, 237, 242),
            assistant_dim: Color::Rgb(145, 154, 168),
            success: Color::Rgb(163, 190, 140),
            warn: Color::Rgb(235, 203, 139),
            error: Color::Rgb(191, 97, 106),
            diff_add_fg: Color::Rgb(180, 210, 155),
            diff_add_bg: Color::Rgb(52, 62, 48),
            diff_del_fg: Color::Rgb(225, 130, 138),
            diff_del_bg: Color::Rgb(72, 48, 52),
            diff_meta: Color::Rgb(235, 203, 139),
            user: Color::Rgb(236, 239, 244),
            blue_050: Color::Rgb(226, 240, 248),
            blue_100: Color::Rgb(200, 220, 236),
            blue_150: Color::Rgb(176, 199, 220),
            blue_200: Color::Rgb(150, 178, 203),
            blue_250: Color::Rgb(140, 170, 198),
            blue_300: Color::Rgb(129, 161, 193),
            blue_400: Color::Rgb(129, 161, 193),
            blue_500: Color::Rgb(94, 129, 172),
            blue_600: Color::Rgb(66, 94, 130),
            indigo: Color::Rgb(129, 161, 193),
            periwinkle: Color::Rgb(159, 178, 210),
            violet: Color::Rgb(180, 142, 173),
            lavender: Color::Rgb(191, 166, 187),
            magenta: Color::Rgb(180, 142, 173),
            pink: Color::Rgb(180, 142, 173),
            rose: Color::Rgb(191, 97, 106),
            coral: Color::Rgb(208, 135, 112),
            amber: Color::Rgb(235, 203, 139),
            gold: Color::Rgb(235, 203, 139),
            orange: Color::Rgb(208, 135, 112),
            lime: Color::Rgb(163, 190, 140),
            mint: Color::Rgb(163, 190, 140),
            seafoam: Color::Rgb(136, 192, 208),
            teal: Color::Rgb(136, 192, 208),
            cyan: Color::Rgb(136, 192, 208),
            gradient: ramp6(Color::Rgb(163, 190, 214), Color::Rgb(52, 70, 100)),
            aurora: ring12(Color::Rgb(170, 195, 218), Color::Rgb(55, 73, 105)),
            ..GOLD
        },
        // Gruvbox: warm retro earth tones.
        "gruvbox" => Palette {
            on_accent_fg: Color::Rgb(40, 40, 40),
            nur_gold: Color::Rgb(215, 153, 33),
            nur_gold_deep: Color::Rgb(181, 118, 20),
            nur_gold_sky: Color::Rgb(235, 195, 110),
            bg: Color::Rgb(40, 40, 40),
            surface: Color::Rgb(50, 48, 47),
            surface_2: Color::Rgb(61, 56, 54),
            surface_3: Color::Rgb(74, 68, 64),
            border: Color::Rgb(88, 80, 73),
            code_bg: Color::Rgb(34, 33, 32),
            fg: Color::Rgb(235, 219, 178),
            muted: Color::Rgb(189, 174, 147),
            faint: Color::Rgb(146, 131, 116),
            md_code: Color::Rgb(142, 202, 228),
            md_h1: Color::Rgb(250, 189, 47),
            md_h2: Color::Rgb(184, 187, 38),
            md_h3: Color::Rgb(211, 134, 155),
            md_link: Color::Rgb(131, 165, 152),
            md_quote: Color::Rgb(168, 153, 132),
            md_list: Color::Rgb(142, 202, 228),
            assistant_fg: Color::Rgb(241, 227, 188),
            assistant_dim: Color::Rgb(180, 166, 140),
            success: Color::Rgb(184, 187, 38),
            warn: Color::Rgb(250, 189, 47),
            error: Color::Rgb(251, 73, 52),
            diff_add_fg: Color::Rgb(200, 220, 130),
            diff_add_bg: Color::Rgb(58, 62, 36),
            diff_del_fg: Color::Rgb(255, 140, 125),
            diff_del_bg: Color::Rgb(74, 42, 36),
            diff_meta: Color::Rgb(250, 189, 47),
            user: Color::Rgb(251, 241, 199),
            blue_050: Color::Rgb(250, 235, 200),
            blue_100: Color::Rgb(245, 220, 165),
            blue_150: Color::Rgb(238, 205, 130),
            blue_200: Color::Rgb(228, 185, 90),
            blue_250: Color::Rgb(220, 168, 55),
            blue_300: Color::Rgb(215, 153, 33),
            blue_400: Color::Rgb(215, 153, 33),
            blue_500: Color::Rgb(181, 118, 20),
            blue_600: Color::Rgb(140, 90, 15),
            indigo: Color::Rgb(211, 134, 155),
            periwinkle: Color::Rgb(200, 160, 190),
            violet: Color::Rgb(211, 134, 155),
            lavender: Color::Rgb(220, 170, 190),
            magenta: Color::Rgb(211, 134, 155),
            pink: Color::Rgb(220, 150, 170),
            rose: Color::Rgb(230, 130, 140),
            coral: Color::Rgb(254, 128, 85),
            amber: Color::Rgb(250, 189, 47),
            gold: Color::Rgb(250, 189, 47),
            orange: Color::Rgb(254, 128, 85),
            lime: Color::Rgb(184, 187, 38),
            mint: Color::Rgb(142, 192, 124),
            seafoam: Color::Rgb(142, 192, 124),
            teal: Color::Rgb(142, 202, 228),
            cyan: Color::Rgb(142, 202, 228),
            gradient: ramp6(Color::Rgb(235, 195, 110), Color::Rgb(130, 85, 15)),
            aurora: ring12(Color::Rgb(238, 200, 120), Color::Rgb(135, 88, 18)),
            ..GOLD
        },
        // Sakura: dusk plum with cherry-blossom pink and gold leaf.
        "sakura" => Palette {
            on_accent_fg: Color::Rgb(26, 16, 24),
            nur_gold: Color::Rgb(255, 148, 182),
            nur_gold_deep: Color::Rgb(214, 96, 140),
            nur_gold_sky: Color::Rgb(255, 190, 212),
            bg: Color::Rgb(26, 16, 24),
            surface: Color::Rgb(35, 21, 32),
            surface_2: Color::Rgb(46, 28, 42),
            surface_3: Color::Rgb(61, 38, 55),
            border: Color::Rgb(78, 48, 70),
            code_bg: Color::Rgb(21, 13, 19),
            fg: Color::Rgb(248, 232, 240),
            muted: Color::Rgb(192, 156, 182),
            faint: Color::Rgb(146, 116, 138),
            md_code: Color::Rgb(230, 170, 200),
            md_h1: Color::Rgb(255, 158, 190),
            md_h2: Color::Rgb(230, 190, 120),
            md_h3: Color::Rgb(200, 160, 220),
            md_link: Color::Rgb(255, 170, 200),
            md_quote: Color::Rgb(160, 130, 152),
            md_list: Color::Rgb(150, 200, 165),
            assistant_fg: Color::Rgb(244, 228, 236),
            assistant_dim: Color::Rgb(185, 150, 176),
            success: Color::Rgb(125, 200, 145),
            warn: Color::Rgb(235, 185, 105),
            error: Color::Rgb(240, 110, 125),
            diff_add_fg: Color::Rgb(165, 225, 180),
            diff_add_bg: Color::Rgb(44, 62, 48),
            diff_del_fg: Color::Rgb(250, 140, 155),
            diff_del_bg: Color::Rgb(70, 38, 46),
            diff_meta: Color::Rgb(235, 185, 105),
            user: Color::Rgb(255, 245, 250),
            blue_050: Color::Rgb(255, 235, 243),
            blue_100: Color::Rgb(255, 210, 226),
            blue_150: Color::Rgb(255, 190, 212),
            blue_200: Color::Rgb(255, 168, 196),
            blue_250: Color::Rgb(255, 158, 189),
            blue_300: Color::Rgb(255, 148, 182),
            blue_400: Color::Rgb(255, 148, 182),
            blue_500: Color::Rgb(214, 96, 140),
            blue_600: Color::Rgb(165, 66, 105),
            indigo: Color::Rgb(200, 160, 220),
            periwinkle: Color::Rgb(215, 175, 230),
            violet: Color::Rgb(205, 150, 225),
            lavender: Color::Rgb(225, 185, 235),
            magenta: Color::Rgb(240, 130, 200),
            pink: Color::Rgb(255, 148, 182),
            rose: Color::Rgb(250, 130, 160),
            coral: Color::Rgb(250, 150, 135),
            amber: Color::Rgb(235, 185, 105),
            gold: Color::Rgb(240, 200, 130),
            orange: Color::Rgb(245, 160, 120),
            lime: Color::Rgb(175, 220, 140),
            mint: Color::Rgb(140, 210, 160),
            seafoam: Color::Rgb(120, 195, 165),
            teal: Color::Rgb(100, 180, 155),
            cyan: Color::Rgb(130, 200, 185),
            gradient: ramp6(Color::Rgb(255, 190, 212), Color::Rgb(150, 55, 95)),
            aurora: ring12(Color::Rgb(255, 195, 216), Color::Rgb(155, 60, 100)),
            ..GOLD
        },
        // Abyss: deep-ocean dark with bioluminescent teal.
        "abyss" => Palette {
            on_accent_fg: Color::Rgb(6, 16, 22),
            nur_gold: Color::Rgb(64, 210, 200),
            nur_gold_deep: Color::Rgb(30, 150, 145),
            nur_gold_sky: Color::Rgb(140, 240, 230),
            bg: Color::Rgb(6, 16, 22),
            surface: Color::Rgb(9, 22, 30),
            surface_2: Color::Rgb(13, 31, 41),
            surface_3: Color::Rgb(19, 42, 54),
            border: Color::Rgb(28, 55, 68),
            code_bg: Color::Rgb(5, 13, 18),
            fg: Color::Rgb(214, 234, 238),
            muted: Color::Rgb(132, 166, 174),
            faint: Color::Rgb(94, 124, 132),
            md_code: Color::Rgb(140, 240, 230),
            md_h1: Color::Rgb(100, 220, 210),
            md_h2: Color::Rgb(110, 190, 160),
            md_h3: Color::Rgb(180, 210, 215),
            md_link: Color::Rgb(120, 230, 220),
            md_quote: Color::Rgb(120, 150, 158),
            md_list: Color::Rgb(130, 200, 170),
            assistant_fg: Color::Rgb(210, 230, 234),
            assistant_dim: Color::Rgb(135, 168, 176),
            success: Color::Rgb(95, 205, 140),
            warn: Color::Rgb(232, 192, 105),
            error: Color::Rgb(242, 115, 105),
            diff_add_fg: Color::Rgb(150, 225, 170),
            diff_add_bg: Color::Rgb(20, 52, 38),
            diff_del_fg: Color::Rgb(250, 140, 130),
            diff_del_bg: Color::Rgb(62, 30, 30),
            diff_meta: Color::Rgb(232, 192, 105),
            user: Color::Rgb(230, 245, 248),
            blue_050: Color::Rgb(225, 250, 246),
            blue_100: Color::Rgb(185, 245, 236),
            blue_150: Color::Rgb(140, 240, 228),
            blue_200: Color::Rgb(95, 228, 216),
            blue_250: Color::Rgb(75, 218, 207),
            blue_300: Color::Rgb(64, 210, 200),
            blue_400: Color::Rgb(64, 210, 200),
            blue_500: Color::Rgb(30, 150, 145),
            blue_600: Color::Rgb(18, 108, 104),
            indigo: Color::Rgb(110, 180, 205),
            periwinkle: Color::Rgb(130, 195, 210),
            violet: Color::Rgb(120, 190, 200),
            lavender: Color::Rgb(150, 210, 215),
            magenta: Color::Rgb(110, 200, 185),
            pink: Color::Rgb(120, 200, 180),
            rose: Color::Rgb(130, 195, 175),
            coral: Color::Rgb(215, 190, 140),
            amber: Color::Rgb(232, 192, 105),
            gold: Color::Rgb(150, 225, 170),
            orange: Color::Rgb(170, 205, 140),
            lime: Color::Rgb(130, 210, 150),
            mint: Color::Rgb(100, 215, 165),
            seafoam: Color::Rgb(80, 212, 185),
            teal: Color::Rgb(64, 210, 200),
            cyan: Color::Rgb(90, 220, 210),
            gradient: ramp6(Color::Rgb(140, 240, 230), Color::Rgb(12, 80, 80)),
            aurora: ring12(Color::Rgb(150, 242, 232), Color::Rgb(14, 85, 85)),
            ..GOLD
        },
        // Moss: forest-floor greens with lichen highlights.
        "moss" => Palette {
            on_accent_fg: Color::Rgb(16, 22, 16),
            nur_gold: Color::Rgb(150, 200, 110),
            nur_gold_deep: Color::Rgb(100, 150, 70),
            nur_gold_sky: Color::Rgb(192, 226, 152),
            bg: Color::Rgb(16, 22, 16),
            surface: Color::Rgb(22, 30, 22),
            surface_2: Color::Rgb(31, 41, 31),
            surface_3: Color::Rgb(42, 55, 41),
            border: Color::Rgb(54, 68, 52),
            code_bg: Color::Rgb(13, 18, 13),
            fg: Color::Rgb(226, 234, 218),
            muted: Color::Rgb(152, 166, 144),
            faint: Color::Rgb(112, 126, 104),
            md_code: Color::Rgb(192, 226, 152),
            md_h1: Color::Rgb(172, 216, 132),
            md_h2: Color::Rgb(140, 190, 150),
            md_h3: Color::Rgb(200, 182, 130),
            md_link: Color::Rgb(160, 210, 140),
            md_quote: Color::Rgb(130, 145, 122),
            md_list: Color::Rgb(150, 195, 150),
            assistant_fg: Color::Rgb(220, 228, 212),
            assistant_dim: Color::Rgb(150, 164, 142),
            success: Color::Rgb(125, 205, 125),
            warn: Color::Rgb(222, 182, 95),
            error: Color::Rgb(225, 115, 100),
            diff_add_fg: Color::Rgb(165, 225, 165),
            diff_add_bg: Color::Rgb(36, 56, 36),
            diff_del_fg: Color::Rgb(245, 145, 130),
            diff_del_bg: Color::Rgb(60, 34, 30),
            diff_meta: Color::Rgb(222, 182, 95),
            user: Color::Rgb(240, 246, 232),
            blue_050: Color::Rgb(238, 248, 222),
            blue_100: Color::Rgb(216, 238, 186),
            blue_150: Color::Rgb(192, 224, 152),
            blue_200: Color::Rgb(170, 210, 126),
            blue_250: Color::Rgb(158, 204, 116),
            blue_300: Color::Rgb(150, 200, 110),
            blue_400: Color::Rgb(150, 200, 110),
            blue_500: Color::Rgb(100, 150, 70),
            blue_600: Color::Rgb(72, 112, 50),
            indigo: Color::Rgb(140, 170, 185),
            periwinkle: Color::Rgb(150, 180, 190),
            violet: Color::Rgb(160, 180, 150),
            lavender: Color::Rgb(175, 195, 165),
            magenta: Color::Rgb(165, 185, 140),
            pink: Color::Rgb(170, 190, 145),
            rose: Color::Rgb(175, 190, 150),
            coral: Color::Rgb(200, 185, 130),
            amber: Color::Rgb(222, 182, 95),
            gold: Color::Rgb(190, 210, 120),
            orange: Color::Rgb(200, 180, 115),
            lime: Color::Rgb(165, 220, 120),
            mint: Color::Rgb(130, 205, 140),
            seafoam: Color::Rgb(110, 195, 150),
            teal: Color::Rgb(95, 180, 140),
            cyan: Color::Rgb(110, 190, 155),
            gradient: ramp6(Color::Rgb(192, 226, 152), Color::Rgb(55, 85, 40)),
            aurora: ring12(Color::Rgb(198, 230, 160), Color::Rgb(58, 90, 42)),
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
    ON_ACCENT_FG => on_accent_fg,
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
        "web_fetch" | "web_search" | "browser" | "terminal_browser" => TEAL(),
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
        "terminal_browser" => "term-browser",
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

    /// Every registered theme resolves to a preset with a preview, and the
    /// text painted on accent backgrounds (selected rows, active chips) is
    /// legible against that accent.
    #[test]
    fn every_registered_theme_resolves_and_highlights_stay_legible() {
        for (id, label) in super::THEMES {
            assert!(!label.is_empty(), "{id} needs a menu label");
            let p = super::preset(id)
                .unwrap_or_else(|| panic!("{id} is registered but has no preset"));
            assert!(super::theme_preview(id).is_some(), "{id} has no preview");
            let ratio = contrast(p.on_accent_fg, p.nur_gold);
            assert!(
                ratio >= 4.5,
                "{id}: on-accent text is only {ratio:.2}:1 on the accent"
            );
        }
    }

    /// Light themes flip the ink: near-white paper, dark text, dark highlight
    /// text - and Heavenly stays strictly blue-free. "No blue" is a hue ban
    /// (185-265, the cyan-through-blue band), not a channel compare: true
    /// amethyst (~273, violet) is wanted; sky/steel blue is not.
    #[test]
    fn light_themes_flip_ink_and_heavenly_has_no_blues() {
        let hue = |c: Color| -> f64 {
            let (r, g, b) = rgb(c);
            let (r, g, b) = (r / 255.0, g / 255.0, b / 255.0);
            let max = r.max(g).max(b);
            let min = r.min(g).min(b);
            if max - min < 1e-6 {
                return 0.0;
            }
            let d = max - min;
            let mut h = if max == r {
                ((g - b) / d) % 6.0
            } else if max == g {
                (b - r) / d + 2.0
            } else {
                (r - g) / d + 4.0
            } * 60.0;
            if h < 0.0 {
                h += 360.0;
            }
            h
        };
        for id in ["heavenly", "pearl", "off-white"] {
            let p = super::preset(id).unwrap();
            let (bgr, ..) = rgb(p.bg);
            let (fgr, ..) = rgb(p.fg);
            assert!(bgr > 240.0, "{id} bg must be near-white, got {bgr}");
            assert!(fgr < 90.0, "{id} ink must be dark, got {fgr}");
        }
        let h = super::preset("heavenly").unwrap();
        for (name, c) in [
            ("md_h1", h.md_h1),
            ("md_h2", h.md_h2),
            ("md_h3", h.md_h3),
            ("md_link", h.md_link),
            ("md_code", h.md_code),
            ("md_list", h.md_list),
            ("indigo", h.indigo),
            ("periwinkle", h.periwinkle),
            ("violet", h.violet),
            ("lavender", h.lavender),
            ("magenta", h.magenta),
            ("cyan", h.cyan),
            ("teal", h.teal),
            ("seafoam", h.seafoam),
        ] {
            let deg = hue(c);
            assert!(
                !(185.0..=265.0).contains(&deg),
                "heavenly {name} lands in the blue band ({deg:.0})"
            );
        }
    }

    /// Silver-accent dark themes paint highlights as dark-on-silver; their
    /// on-accent text must flip dark along with the light themes.
    #[test]
    fn silver_accent_themes_flip_highlight_text_dark() {
        for id in ["noir", "black", "off-black", "chrome"] {
            let p = super::preset(id).unwrap();
            let (ar, ..) = rgb(p.nur_gold);
            let (fr, ..) = rgb(p.on_accent_fg);
            assert!(ar > 180.0, "{id} accent must be silver-bright, got {ar}");
            assert!(fr < 60.0, "{id} on-accent text must be dark, got {fr}");
        }
    }
}

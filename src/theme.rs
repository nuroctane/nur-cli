//! NurCLI visual system — gold-led chrome, purple + deep teal accents.
//!
//! Single source of truth for colors + text styles used by both the TUI
//! (ratatui) and plain stdout printing (colored).

use colored::Colorize;
use ratatui::style::{Color, Modifier, Style};
use std::time::Duration;

// ── Palette ────────────────────────────────────────────────────────────────
/// Primary interactive gold (#E8B923).
pub const NUR_GOLD: Color = Color::Rgb(232, 185, 35);
/// Deep mustard / pressed gold.
pub const NUR_GOLD_DEEP: Color = Color::Rgb(184, 134, 11);
/// Soft champagne accent for secondary labels.
pub const NUR_GOLD_SKY: Color = Color::Rgb(255, 224, 140);
/// Legacy name used across the TUI - now gold (not Meta blue).
pub const META_BLUE: Color = NUR_GOLD;
#[allow(dead_code)]
pub const META_BLUE_DEEP: Color = NUR_GOLD_DEEP;
pub const META_BLUE_SKY: Color = NUR_GOLD_SKY;
/// Near-black canvas (terminal fill).
pub const BG: Color = Color::Rgb(11, 14, 18);
/// Raised surface (input well, modals).
pub const SURFACE: Color = Color::Rgb(18, 22, 28);
/// Elevated surface (palette, hover).
pub const SURFACE_2: Color = Color::Rgb(26, 31, 40);
/// Highest surface — focused-row highlight inside modals (peek trace focus).
pub const SURFACE_3: Color = Color::Rgb(38, 45, 58);
/// Near-white foreground.
pub const FG: Color = Color::Rgb(245, 242, 232);
/// Dimmed foreground.
pub const MUTED: Color = Color::Rgb(148, 142, 128);
/// Extra-dim (hints, separators).
pub const FAINT: Color = Color::Rgb(126, 119, 104);
/// Hairline / border idle.
pub const BORDER: Color = Color::Rgb(48, 44, 36);
/// Code / block background.
pub const CODE_BG: Color = Color::Rgb(16, 18, 14);
/// Cool mint code in assistant markdown (stands out from body + gold chrome).
pub const MD_CODE: Color = Color::Rgb(160, 220, 195);
/// Markdown structure hues — keep these *off* the gold spine for legibility.
pub const MD_H1: Color = Color::Rgb(120, 210, 215); // aqua
pub const MD_H2: Color = Color::Rgb(130, 175, 235); // soft sky blue
pub const MD_H3: Color = Color::Rgb(165, 155, 235); // periwinkle
pub const MD_LINK: Color = Color::Rgb(100, 195, 235); // bright sky
pub const MD_QUOTE: Color = Color::Rgb(150, 165, 145); // sage
pub const MD_LIST: Color = Color::Rgb(90, 185, 165); // teal-mint
/// Assistant prose — cool off-white (not pure white, not warm parchment).
pub const ASSISTANT_FG: Color = Color::Rgb(228, 232, 240);
/// Soft secondary assistant labels (meta under answers).
pub const ASSISTANT_DIM: Color = Color::Rgb(150, 160, 175);
pub const SUCCESS: Color = Color::Rgb(52, 199, 123);
pub const WARN: Color = Color::Rgb(255, 186, 73);
pub const ERROR: Color = Color::Rgb(255, 99, 99);
/// Diff bands (Claude-Code style): added / removed line fg + subtle bg.
pub const DIFF_ADD_FG: Color = Color::Rgb(126, 231, 166);
pub const DIFF_ADD_BG: Color = Color::Rgb(18, 42, 30);
pub const DIFF_DEL_FG: Color = Color::Rgb(255, 138, 148);
pub const DIFF_DEL_BG: Color = Color::Rgb(46, 24, 28);
/// Diff hunk header.
pub const DIFF_META: Color = Color::Rgb(212, 175, 80);
/// User message accent (crisp white).
pub const USER: Color = Color::Rgb(255, 255, 255);

/// Banner gradient (top → bottom rows of the NUR logotype) — yellow spectrum.
pub const GRADIENT: [(u8, u8, u8); 6] = [
    (255, 248, 180), // pale lemon
    (255, 230, 120), // canary
    (255, 200, 60),  // bright gold
    (232, 185, 35),  // nur gold
    (200, 150, 20),  // mustard
    (160, 110, 15),  // bronze
];

// ── Gold spine + accents ───────────────────────────────────────────────────
// Primary chrome is gold; purple + deep teal remain for tool families.

/// Gold ramp, light → deep (replaces old blue spine).
pub const BLUE_100: Color = Color::Rgb(255, 242, 190);
pub const BLUE_200: Color = Color::Rgb(255, 224, 140);
pub const BLUE_300: Color = Color::Rgb(255, 208, 90);
pub const BLUE_400: Color = Color::Rgb(232, 185, 35); // == NUR_GOLD
pub const BLUE_500: Color = Color::Rgb(184, 134, 11);
#[allow(dead_code)]
pub const BLUE_600: Color = Color::Rgb(140, 100, 10);

pub const BLUE_050: Color = Color::Rgb(255, 250, 220);
#[allow(dead_code)] // brand palette reserved for future chrome
pub const BLUE_150: Color = Color::Rgb(255, 236, 160);
pub const BLUE_250: Color = Color::Rgb(255, 216, 100);

/// Accents: purple family + deep teal (kept per brand brief).
pub const INDIGO: Color = Color::Rgb(139, 120, 220); // structure: skills, todos
#[allow(dead_code)] // brand palette reserved for future chrome
pub const PERIWINKLE: Color = Color::Rgb(168, 150, 230);
pub const VIOLET: Color = Color::Rgb(178, 148, 255); // thought & authored change
pub const LAVENDER: Color = Color::Rgb(202, 180, 255);
#[allow(dead_code)] // brand palette reserved for future chrome
pub const MAGENTA: Color = Color::Rgb(200, 120, 200);
pub const PINK: Color = Color::Rgb(220, 140, 180);
#[allow(dead_code)]
pub const ROSE: Color = Color::Rgb(255, 143, 168);
#[allow(dead_code)]
pub const CORAL: Color = Color::Rgb(255, 138, 120);
pub const AMBER: Color = Color::Rgb(236, 162, 44); // shell - deliberately NOT WARN
#[allow(dead_code)] // brand palette reserved for future chrome
pub const GOLD: Color = Color::Rgb(255, 208, 110);
pub const ORANGE: Color = Color::Rgb(255, 150, 89); // memory
#[allow(dead_code)]
pub const LIME: Color = Color::Rgb(160, 224, 122);
#[allow(dead_code)] // brand palette reserved for future chrome
pub const MINT: Color = Color::Rgb(80, 190, 170); // deep-teal bridge
pub const SEAFOAM: Color = Color::Rgb(56, 170, 160);
pub const TEAL: Color = Color::Rgb(32, 150, 148); // deep teal — network
pub const CYAN: Color = Color::Rgb(72, 196, 208); // git - clear of SEAFOAM's answer teal
                                                  // Green lives in SUCCESS — status, not a family hue.

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
    lerp(c, BG, t)
}

/// Gold shimmer ring — full yellow spectrum (lemon → canary → gold → mustard →
/// honey → bronze → champagne) with a touch of amber/honey for motion.
pub const AURORA: &[Color] = &[
    Color::Rgb(255, 252, 200), // pale lemon
    Color::Rgb(255, 245, 160), // light canary
    Color::Rgb(255, 230, 100), // canary
    Color::Rgb(255, 214, 70),  // bright gold
    Color::Rgb(232, 185, 35),  // nur gold
    Color::Rgb(212, 160, 25),  // honey
    Color::Rgb(190, 140, 20),  // mustard
    Color::Rgb(170, 120, 18),  // deep mustard
    Color::Rgb(200, 150, 40),  // antique gold
    Color::Rgb(230, 190, 90),  // champagne
    Color::Rgb(255, 220, 120), // pale gold
    Color::Rgb(255, 200, 80),  // sunflower
];

/// Sample the aurora ring at `phase` (any f64; wraps) with smooth interpolation.
pub fn aurora_at(phase: f64) -> Color {
    let n = AURORA.len();
    let x = phase.rem_euclid(1.0) * n as f64;
    let i = (x.floor() as usize) % n;
    let j = (i + 1) % n;
    lerp(AURORA[i], AURORA[j], x.fract())
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
        "read_file" | "list_dir" | "grep" | "glob" => BLUE_300,
        "write_file" | "edit_file" | "multi_edit" | "apply_patch" => VIOLET,
        "bash" => AMBER,
        "web_fetch" | "web_search" | "browser" => TEAL,
        "look" | "extract_frames" => PINK,
        "git_status" | "git_diff" => CYAN,
        "agent" | "omp" => PINK,
        "memory" => ORANGE,
        "skill" | "todo_write" | "graphify" | "plur" | "ruflo" | "executor" => INDIGO,
        "submit_plan" => VIOLET,
        _ => BLUE_200,
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
            Tone::Neutral => BLUE_400,
            Tone::Mode => INDIGO,
            Tone::Plan => VIOLET,
            Tone::Todos => CYAN,
            Tone::Usage => TEAL,
            Tone::Session => BLUE_200,
            Tone::Skill => PERIWINKLE,
            Tone::Memory => ORANGE,
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
    (elapsed.as_millis() / BLINK_MS) % 2 == 0
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
    let bg = if live { NUR_GOLD } else { META_BLUE_SKY };
    Style::default().fg(BG).bg(bg).add_modifier(Modifier::BOLD)
}

/// Chip for the model's thinking time - the one duration that is violet.
pub fn style_thought_chip() -> Style {
    Style::default()
        .fg(BG)
        .bg(VIOLET)
        .add_modifier(Modifier::BOLD)
}

/// Style for turn-complete duration chip.
pub fn style_turn_chip(interrupted: bool) -> Style {
    if interrupted {
        Style::default()
            .fg(BG)
            .bg(WARN)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(BG)
            .bg(SUCCESS)
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
    Style::default().fg(META_BLUE).add_modifier(Modifier::BOLD)
}

pub fn style_status() -> Style {
    Style::default().fg(MUTED)
}

pub fn style_faint() -> Style {
    Style::default().fg(FAINT)
}

pub fn style_user() -> Style {
    Style::default().fg(USER).add_modifier(Modifier::BOLD)
}

pub fn style_assistant() -> Style {
    Style::default().fg(ASSISTANT_FG)
}

/// Secondary lines under an answer (e.g. meta footnotes).
#[allow(dead_code)]
pub fn style_assistant_dim() -> Style {
    Style::default().fg(ASSISTANT_DIM)
}

pub fn style_tool() -> Style {
    Style::default().fg(TEAL)
}

/// Tool result body: soft tint from the tool family (not plain grey).
pub fn style_tool_result(name: &str) -> Style {
    Style::default().fg(dim(tool_color(name), 0.28))
}

pub fn style_success() -> Style {
    Style::default().fg(SUCCESS)
}

pub fn style_warn() -> Style {
    Style::default().fg(WARN)
}

pub fn style_error() -> Style {
    Style::default().fg(ERROR)
}

/// Reasoning / "thinking" text — violet, so model thought is never confused
/// with tool output or the assistant's actual answer.
pub fn style_thinking_violet() -> Style {
    Style::default().fg(VIOLET).add_modifier(Modifier::ITALIC)
}

#[allow(dead_code)]
pub fn style_thinking() -> Style {
    Style::default().fg(MUTED).add_modifier(Modifier::ITALIC)
}

pub fn style_canvas() -> Style {
    Style::default().bg(BG).fg(FG)
}

pub fn style_surface() -> Style {
    Style::default().bg(SURFACE).fg(FG)
}

/// Input caret / stream caret: reverse gold block.
pub fn style_cursor_on() -> Style {
    Style::default()
        .fg(BG)
        .bg(NUR_GOLD)
        .add_modifier(Modifier::BOLD)
}

// ── stdout helpers (headless / subcommands) ────────────────────────────────
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
    println!();
    for (i, row) in rows.iter().enumerate() {
        let (r, g, b) = GRADIENT[i.min(GRADIENT.len() - 1)];
        println!("{}", row.truecolor(r, g, b));
    }
    println!(
        "  {}  {}  {}   {}",
        "NurCLI".truecolor(232, 185, 35).bold(),
        "·".truecolor(148, 142, 128),
        "multi-provider coding agent".truecolor(200, 190, 170),
        format!("v{}", env!("CARGO_PKG_VERSION")).truecolor(96, 90, 78)
    );
    println!(
        "  {}\n",
        "fully loaded  ·  TUI · tools · Graphify/PLUR/Ruflo · 800+ skills".truecolor(120, 112, 96)
    );
}

pub fn print_info(msg: &str) {
    println!("{} {}", "●".truecolor(232, 185, 35), msg);
}

pub fn print_ok(msg: &str) {
    println!("{} {}", "✓".truecolor(52, 199, 123), msg);
}

pub fn print_err(msg: &str) {
    eprintln!("{} {}", "✗".truecolor(255, 99, 99), msg);
}

pub fn print_tool(name: &str, detail: &str) {
    println!(
        "{} {} {}",
        "●".truecolor(232, 185, 35),
        name.truecolor(232, 185, 35).bold(),
        detail.truecolor(148, 142, 128)
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
            contrast(FAINT, BG) >= 3.0,
            "FAINT on BG is {:.2}:1",
            contrast(FAINT, BG)
        );
        assert!(
            contrast(FAINT, SURFACE_2) >= 3.0,
            "FAINT on SURFACE_2 is {:.2}:1",
            contrast(FAINT, SURFACE_2)
        );
        // MUTED outranks FAINT - the hierarchy has to survive any retune.
        assert!(contrast(MUTED, BG) > contrast(FAINT, BG));
        assert!(contrast(FG, BG) >= 7.0);
    }

    /// Colours that mean different things must look different. Each of these
    /// pairs was close enough to be indistinguishable in a terminal.
    #[test]
    fn distinct_roles_use_distinguishable_colours() {
        // "assistant is answering" vs "git tool".
        assert!(
            distance(SEAFOAM, CYAN) > 40.0,
            "SEAFOAM/CYAN distance {:.0}",
            distance(SEAFOAM, CYAN)
        );
        // Shell-tool family vs warning status. Status colours are never family hues.
        assert!(
            distance(AMBER, WARN) > 20.0,
            "AMBER/WARN distance {:.0}",
            distance(AMBER, WARN)
        );
        assert_ne!(AMBER, WARN, "a shell card must not read as a warning");
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
                contrast(a.color(), BG) >= 4.5,
                "{a:?} is unreadable on BG: {:.2}:1",
                contrast(a.color(), BG)
            );
        }
    }

    /// Violet means model thought and only that. Duration chips sit on the gold
    /// chrome spine; the thought chip is the single violet one.
    #[test]
    fn violet_is_reserved_for_thought() {
        let bg_of = |s: Style| s.bg.expect("chips set a background");
        assert_eq!(bg_of(style_thought_chip()), VIOLET);
        assert_ne!(bg_of(style_duration_chip(true)), VIOLET, "a running tool is not a thought");
        assert_ne!(bg_of(style_duration_chip(false)), VIOLET);
        // Live still outranks finished.
        assert_ne!(
            bg_of(style_duration_chip(true)),
            bg_of(style_duration_chip(false)),
            "live and settled chips must be tellable apart"
        );
        // Chips are dark-on-light: the text has to survive the background.
        for s in [style_thought_chip(), style_duration_chip(true), style_duration_chip(false)] {
            assert!(contrast(bg_of(s), BG) >= 4.5);
        }
    }
}

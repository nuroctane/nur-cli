//! Meta-inspired visual system for Meta CLI (unofficial).
//!
//! Single source of truth for colors + text styles used by both the TUI
//! (ratatui) and plain stdout printing (colored).
//!
//! Brand anchors (community approximation of Meta blue):
//!   #0082FB  Meta Azure  ·  #0064E0  Meta Science Blue

use colored::Colorize;
use ratatui::style::{Color, Modifier, Style};
use std::time::Duration;

// ── Palette ────────────────────────────────────────────────────────────────
/// Meta Azure Radiance (#0082FB) — primary interactive / focus.
pub const META_BLUE: Color = Color::Rgb(0, 130, 251);
/// Alias for call sites that used the bright name.
pub const META_BLUE_BRIGHT: Color = META_BLUE;
/// Meta Science Blue (#0064E0) — secondary / pressed.
#[allow(dead_code)]
pub const META_BLUE_DEEP: Color = Color::Rgb(0, 100, 224);
/// Soft sky accent for gradients & secondary labels.
pub const META_BLUE_SKY: Color = Color::Rgb(90, 175, 255);
/// Near-black canvas (terminal fill).
pub const BG: Color = Color::Rgb(11, 14, 18);
/// Raised surface (input well, modals).
pub const SURFACE: Color = Color::Rgb(18, 22, 28);
/// Elevated surface (palette, hover).
pub const SURFACE_2: Color = Color::Rgb(26, 31, 40);
/// Near-white foreground.
pub const FG: Color = Color::Rgb(232, 235, 240);
/// Dimmed foreground.
pub const MUTED: Color = Color::Rgb(138, 146, 158);
/// Extra-dim (hints, separators).
pub const FAINT: Color = Color::Rgb(86, 94, 106);
/// Hairline / border idle.
pub const BORDER: Color = Color::Rgb(42, 48, 58);
/// Code / block background.
pub const CODE_BG: Color = Color::Rgb(16, 20, 26);
/// Inline code foreground.
pub const CODE_FG: Color = Color::Rgb(148, 199, 255);
pub const SUCCESS: Color = Color::Rgb(52, 199, 123);
pub const WARN: Color = Color::Rgb(255, 186, 73);
pub const ERROR: Color = Color::Rgb(255, 99, 99);
/// User message accent (crisp white).
pub const USER: Color = Color::Rgb(255, 255, 255);

/// Banner gradient (top → bottom rows of the logotype).
pub const GRADIENT: [(u8, u8, u8); 6] = [
    (90, 175, 255),
    (40, 150, 253),
    (0, 130, 251),
    (0, 115, 240),
    (0, 100, 224),
    (0, 85, 200),
];

// ── Motion ─────────────────────────────────────────────────────────────────
/// Braille spinner — smooth, dense, Meta-blue tinted in UI.
pub const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
/// Soft pulse dots for quieter states (thinking complete, etc.).
pub const PULSE: &[&str] = &["·", "•", "●", "•"];
/// Frame interval for spinner (ms).
pub const SPINNER_MS: u128 = 70;
/// Cursor / stream caret blink half-period (ms).
pub const BLINK_MS: u128 = 530;

/// Spinner glyph for elapsed time.
pub fn spinner_frame(elapsed: Duration) -> &'static str {
    let i = (elapsed.as_millis() / SPINNER_MS) as usize % SPINNER.len();
    SPINNER[i]
}

/// Soft pulse glyph.
pub fn pulse_frame(elapsed: Duration) -> &'static str {
    let i = (elapsed.as_millis() / 280) as usize % PULSE.len();
    PULSE[i]
}

/// True during the "on" half of a blink cycle.
pub fn blink_on(elapsed: Duration) -> bool {
    (elapsed.as_millis() / BLINK_MS) % 2 == 0
}

// ── ratatui styles ─────────────────────────────────────────────────────────
pub fn style_title() -> Style {
    Style::default()
        .fg(META_BLUE)
        .add_modifier(Modifier::BOLD)
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
    Style::default().fg(FG)
}

pub fn style_tool() -> Style {
    Style::default().fg(META_BLUE_SKY)
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

pub fn style_thinking() -> Style {
    Style::default().fg(MUTED).add_modifier(Modifier::ITALIC)
}

pub fn style_canvas() -> Style {
    Style::default().bg(BG).fg(FG)
}

pub fn style_surface() -> Style {
    Style::default().bg(SURFACE).fg(FG)
}

/// Input caret / stream caret: reverse Meta blue block.
pub fn style_cursor_on() -> Style {
    Style::default()
        .fg(BG)
        .bg(META_BLUE)
        .add_modifier(Modifier::BOLD)
}

pub fn style_cursor_off() -> Style {
    Style::default().fg(META_BLUE)
}

// ── stdout helpers (headless / subcommands) ────────────────────────────────
#[allow(dead_code)]
pub fn banner() {
    let rows = [
        r#" ███╗   ███╗██╗   ██╗███████╗███████╗"#,
        r#" ████╗ ████║██║   ██║██╔════╝██╔════╝"#,
        r#" ██╔████╔██║██║   ██║███████╗█████╗  "#,
        r#" ██║╚██╔╝██║██║   ██║╚════██║██╔══╝  "#,
        r#" ██║ ╚═╝ ██║╚██████╔╝███████║███████╗"#,
        r#" ╚═╝     ╚═╝ ╚═════╝ ╚══════╝╚══════╝"#,
    ];
    println!();
    for (i, row) in rows.iter().enumerate() {
        let (r, g, b) = GRADIENT[i.min(GRADIENT.len() - 1)];
        println!("{}", row.truecolor(r, g, b));
    }
    println!(
        "  {}  {}  {}   {}",
        "Spark".truecolor(0, 130, 251).bold(),
        "·".truecolor(138, 146, 158),
        "Meta Model API".truecolor(180, 190, 200),
        format!("v{}", env!("CARGO_PKG_VERSION")).truecolor(86, 94, 106)
    );
    println!(
        "  {}\n",
        "Unofficial coding agent — not affiliated with Meta".truecolor(100, 108, 118)
    );
}

pub fn print_info(msg: &str) {
    println!("{} {}", "●".truecolor(0, 130, 251), msg);
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
        "●".truecolor(0, 130, 251),
        name.truecolor(0, 130, 251).bold(),
        detail.truecolor(138, 146, 158)
    );
}

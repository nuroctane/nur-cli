//! Meta-inspired visual system for Meta CLI (unofficial).
//!
//! Single source of truth for colors + text styles used by both the TUI
//! (ratatui) and plain stdout printing (colored).

use colored::Colorize;
use ratatui::style::{Color, Modifier, Style};

// ── Palette ────────────────────────────────────────────────────────────────
/// Meta brand blue.
pub const META_BLUE: Color = Color::Rgb(6, 104, 225);
/// Brighter interactive blue.
pub const META_BLUE_BRIGHT: Color = Color::Rgb(0, 130, 251);
/// Light accent blue (gradient tail).
pub const META_BLUE_SKY: Color = Color::Rgb(69, 168, 255);
/// Near-white foreground.
pub const FG: Color = Color::Rgb(228, 232, 238);
/// Dimmed foreground.
pub const MUTED: Color = Color::Rgb(140, 148, 158);
/// Extra-dim (hints, separators).
pub const FAINT: Color = Color::Rgb(92, 99, 110);
/// Code / block background.
pub const CODE_BG: Color = Color::Rgb(24, 27, 33);
/// Inline code foreground.
pub const CODE_FG: Color = Color::Rgb(148, 199, 255);
pub const SUCCESS: Color = Color::Rgb(52, 199, 123);
pub const WARN: Color = Color::Rgb(255, 180, 0);
pub const ERROR: Color = Color::Rgb(240, 92, 92);
/// User message accent.
pub const USER: Color = Color::Rgb(255, 255, 255);

/// Banner gradient (top → bottom rows of the logotype).
pub const GRADIENT: [(u8, u8, u8); 6] = [
    (69, 168, 255),
    (38, 148, 253),
    (0, 130, 251),
    (6, 116, 238),
    (6, 104, 225),
    (10, 92, 200),
];

// ── ratatui styles ─────────────────────────────────────────────────────────
pub fn style_title() -> Style {
    Style::default()
        .fg(META_BLUE_BRIGHT)
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
    Style::default().fg(META_BLUE_BRIGHT)
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
    Style::default().fg(FAINT).add_modifier(Modifier::ITALIC)
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
        "·".truecolor(140, 148, 158),
        "Meta Model API".truecolor(180, 190, 200),
        format!("v{}", env!("CARGO_PKG_VERSION")).truecolor(92, 99, 110)
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
    eprintln!("{} {}", "✗".truecolor(240, 92, 92), msg);
}

pub fn print_tool(name: &str, detail: &str) {
    println!(
        "{} {} {}",
        "⏺".truecolor(0, 130, 251),
        name.truecolor(0, 130, 251).bold(),
        detail.truecolor(140, 148, 158)
    );
}

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

// ── Standardized hue ramp ──────────────────────────────────────────────────
// Every accent below sits at a similar lightness/saturation so the UI reads as
// one system: a blue spine with hues fanning out around it. Assignment is by
// *meaning*, never ad hoc — see `tool_color` and `Tone`.

/// Blue ramp, light → deep. The spine of the UI.
pub const BLUE_100: Color = Color::Rgb(168, 212, 255);
pub const BLUE_200: Color = Color::Rgb(120, 190, 255);
pub const BLUE_300: Color = Color::Rgb(90, 175, 255); // == META_BLUE_SKY
pub const BLUE_400: Color = Color::Rgb(0, 130, 251); // == META_BLUE
pub const BLUE_500: Color = Color::Rgb(0, 100, 224);
#[allow(dead_code)]
pub const BLUE_600: Color = Color::Rgb(0, 82, 190);

/// Accents, ordered around the wheel from the blue spine.
pub const INDIGO: Color = Color::Rgb(139, 152, 255); // structure: skills, todos
pub const VIOLET: Color = Color::Rgb(178, 148, 255); // thought & authored change
pub const PINK: Color = Color::Rgb(240, 133, 197); // delegation (subagents)
pub const AMBER: Color = Color::Rgb(255, 186, 73); // execution (shell) == WARN
pub const ORANGE: Color = Color::Rgb(255, 150, 89); // durable state (memory)
pub const TEAL: Color = Color::Rgb(64, 214, 196); // the network
pub const CYAN: Color = Color::Rgb(80, 196, 255); // version control
// Green lives in SUCCESS — status, not a family hue.

/// Colour a tool by *family*, so a glance tells you what kind of thing ran:
/// read (sky) · write (violet) · shell (amber) · net (teal) · git (cyan) ·
/// delegate (pink) · knowledge (indigo/orange).
pub fn tool_color(name: &str) -> Color {
    match name {
        "read_file" | "list_dir" | "grep" | "glob" => BLUE_300,
        "write_file" | "edit_file" | "multi_edit" | "apply_patch" => VIOLET,
        "bash" => AMBER,
        "web_fetch" | "web_search" => TEAL,
        "git_status" | "git_diff" => CYAN,
        "agent" => PINK,
        "memory" => ORANGE,
        "skill" | "todo_write" | "graphify" => INDIGO,
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
        "git_status" | "git_diff" => "git",
        "agent" => "agent",
        "memory" => "memory",
        "skill" => "skill",
        "todo_write" => "todo",
        "graphify" => "graph",
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
            Tone::Skill => INDIGO,
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

/// Reasoning / "thinking" text — violet, so model thought is never confused
/// with tool output or the assistant's actual answer.
pub fn style_thinking_violet() -> Style {
    Style::default().fg(VIOLET).add_modifier(Modifier::ITALIC)
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

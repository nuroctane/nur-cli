//! Meta-inspired palette for Meta CLI (unofficial).

use colored::Colorize;
use ratatui::style::{Color, Modifier, Style};

/// Meta blue
pub const META_BLUE: Color = Color::Rgb(6, 104, 225);
pub const META_BLUE_BRIGHT: Color = Color::Rgb(0, 130, 251);
#[allow(dead_code)]
pub const CHARCOAL: Color = Color::Rgb(28, 30, 33);
pub const MUTED: Color = Color::Rgb(140, 148, 158);
#[allow(dead_code)]
pub const SUCCESS: Color = Color::Rgb(0, 200, 120);
#[allow(dead_code)]
pub const WARN: Color = Color::Rgb(255, 180, 0);
#[allow(dead_code)]
pub const ERROR: Color = Color::Rgb(240, 80, 80);

pub fn style_title() -> Style {
    Style::default()
        .fg(META_BLUE_BRIGHT)
        .add_modifier(Modifier::BOLD)
}

pub fn style_status() -> Style {
    Style::default().fg(MUTED)
}

pub fn style_user() -> Style {
    Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
}

pub fn style_assistant() -> Style {
    Style::default().fg(Color::Rgb(200, 210, 230))
}

pub fn style_tool() -> Style {
    Style::default().fg(META_BLUE)
}

#[allow(dead_code)]
pub fn style_error() -> Style {
    Style::default().fg(ERROR)
}

pub fn banner() {
    println!(
        "{}",
        r#"
 ███╗   ███╗██╗   ██╗███████╗███████╗
 ████╗ ████║██║   ██║██╔════╝██╔════╝
 ██╔████╔██║██║   ██║███████╗█████╗  
 ██║╚██╔╝██║██║   ██║╚════██║██╔══╝  
 ██║ ╚═╝ ██║╚██████╔╝███████║███████╗
 ╚═╝     ╚═╝ ╚═════╝ ╚══════╝╚══════╝
"#
        .truecolor(0, 130, 251)
    );
    println!(
        "  {}  {}  {}",
        "Spark".truecolor(0, 130, 251).bold(),
        "·".truecolor(140, 148, 158),
        "Meta Model API".truecolor(180, 190, 200)
    );
    println!(
        "  {}\n",
        "Unofficial coding agent — not affiliated with Meta"
            .truecolor(100, 108, 118)
    );
}

pub fn print_info(msg: &str) {
    println!("{} {}", "●".truecolor(0, 130, 251), msg);
}

pub fn print_ok(msg: &str) {
    println!("{} {}", "✓".truecolor(0, 200, 120), msg);
}

pub fn print_err(msg: &str) {
    eprintln!("{} {}", "✗".truecolor(240, 80, 80), msg);
}

pub fn print_tool(name: &str, detail: &str) {
    println!(
        "{} {} {}",
        "⚙".truecolor(0, 130, 251),
        name.truecolor(0, 130, 251).bold(),
        detail.truecolor(140, 148, 158)
    );
}

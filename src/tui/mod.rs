mod ansi;
mod app;
mod grid;
mod input;
mod markdown;
mod scrollbar;
mod ui;
mod wrap;

#[cfg(feature = "image-peek")]
pub mod latex;

pub use app::run_tui;

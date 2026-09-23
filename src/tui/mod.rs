mod ansi;
mod app;
mod grid;
mod input;
mod inspector;
mod links;
mod row_index;
mod markdown;
mod scrollbar;
mod ui;
mod wrap;

#[cfg(feature = "image-peek")]
pub mod latex;

pub use app::run_tui;

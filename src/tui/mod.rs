pub(crate) mod ansi;
mod app;
mod grid;
mod input;
mod inspector;
mod links;
mod markdown;
mod row_index;
mod scrollbar;
mod ui;
mod wrap;

#[cfg(feature = "image-peek")]
pub mod latex;

pub use app::run_tui;

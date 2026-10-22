pub mod anthropic;
pub mod chat;
pub mod client;
pub mod cursor_cli;
pub mod droid_cli;
pub mod failover;
pub mod fusion;
pub mod gemini;
pub mod local;
pub mod models;
pub mod perplexity;
pub mod sse;
pub mod types;

pub use client::{ApiClient, StreamEvent};
pub use models::fetch_model_ids;
pub use types::*;

/// "No cap" for the agent-CLI runners: long enough never to fire, and only
/// ever compared with `elapsed()` (plus the 30s grace), never added to an
/// `Instant`, so it cannot overflow.
pub(crate) const UNCAPPED: std::time::Duration =
    std::time::Duration::from_secs(100 * 365 * 24 * 3600);

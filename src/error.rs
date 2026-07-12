use thiserror::Error;

#[derive(Error, Debug)]
pub enum MuseError {
    #[error("not authenticated: set MODEL_API_KEY / MUSE_API_KEY or run `muse auth login`")]
    NotAuthenticated,

    #[error("API error ({status}): {message}")]
    Api { status: u16, message: String },

    #[error("API request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("config error: {0}")]
    Config(String),

    #[error("tool error: {0}")]
    Tool(String),

    #[error("max turns reached ({0})")]
    MaxTurns(u32),

    #[error("interrupted")]
    Interrupted,

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, MuseError>;

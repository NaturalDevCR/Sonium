use thiserror::Error;

#[derive(Debug, Error)]
pub enum SoniumError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Codec error: {0}")]
    Codec(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Sync error: {0}")]
    Sync(String),

    #[error("Connection closed")]
    ConnectionClosed,

    #[error("Unsupported codec: {0}")]
    UnsupportedCodec(String),
}

pub type Result<T> = std::result::Result<T, SoniumError>;

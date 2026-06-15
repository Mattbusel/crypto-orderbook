use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    // WebSocket transport errors — network drops, TLS failures, protocol violations.
    // Recovery: reconnect the WebSocket, do not resync the book.
    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),

    // JSON parsing failed on an incoming Binance frame.
    // Recovery: log and skip the frame. The stream is still valid.
    #[error("Failed to parse Binance message: {0}")]
    Parse(#[from] serde_json::Error),

    // A price or quantity string from Binance could not be parsed as a number.
    // Recovery: same as Parse — log and skip the frame.
    #[error("Invalid numeric field from Binance: {0}")]
    InvalidField(String),

    // The event sequence is broken — U != prev_u + 1.
    // Recovery: discard the book, re-fetch REST snapshot, re-sync.
    #[error("Order book sequence gap: expected {expected}, got {got}")]
    SequenceGap { expected: u64, got: u64 },

    // The initial sync handshake failed — first event doesn't satisfy
    // U <= lastUpdateId+1 && u >= lastUpdateId+1 per Binance spec.
    #[error("Order book sync failed: snapshot lastUpdateId={snapshot_id}, event U={event_u}, u={event_u_lower}")]
    SyncFailed {
        snapshot_id: u64,
        event_u: u64,
        event_u_lower: u64,
    },

    // URL construction failed — only happens at startup with bad config.
    #[error("Invalid URL: {0}")]
    InvalidUrl(#[from] url::ParseError),

    // I/O errors from the underlying TCP stream.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

// Shorthand Result type so every module can write `Result<T>` instead of
// `Result<T, AppError>`. This is idiomatic Rust for application crates.
pub type Result<T> = std::result::Result<T, AppError>;

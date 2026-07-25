#[derive(Debug, thiserror::Error)]
pub enum DiscordError {
    #[error("no discord named pipe found (tried discord-ipc-0..9)")]
    PipeNotFound,
    #[error("discord named pipe closed")]
    PipeClosed,
    #[error(transparent)]
    Frame(#[from] crate::frame::FrameError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("unexpected response during discord handshake")]
    UnexpectedHandshakeResponse,
    #[error("discord rejected the handshake: {0}")]
    HandshakeRejected(String),
    #[error("discord command failed: {0}")]
    CommandFailed(String),
    #[error("coordinator command channel closed")]
    CommandChannelClosed,
}

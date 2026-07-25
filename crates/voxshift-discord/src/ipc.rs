//! Discord named-pipe discovery — 設計書.md §6.1.1.
//!
//! Judgment call: the design doc literally shows `\\?\pipe\discord-ipc-N`;
//! `\\.\pipe\...` is used here instead — the conventional Win32 named-pipe
//! client path (and what tokio's API expects), functionally the same
//! namespace. Single attempt only per candidate index; the reconnect/backoff
//! loop lives in voxshift-app, not here, so this crate stays independently
//! testable as a plain transport.

use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};

use crate::error::DiscordError;

pub async fn discover_and_connect() -> Result<NamedPipeClient, DiscordError> {
    for n in 0..=9u8 {
        let path = format!(r"\\.\pipe\discord-ipc-{n}");
        if let Ok(client) = connect_to(&path) {
            return Ok(client);
        }
    }
    Err(DiscordError::PipeNotFound)
}

/// Single-attempt connect to an arbitrary named pipe path. Exposed
/// separately from [`discover_and_connect`] so tests can point the real
/// client at a scripted mock server instead of a real (or absent) Discord
/// install.
pub fn connect_to(path: &str) -> Result<NamedPipeClient, DiscordError> {
    match ClientOptions::new().open(path) {
        Ok(client) => {
            tracing::info!(pipe = %path, "connected to discord rpc pipe");
            Ok(client)
        }
        Err(err) => {
            tracing::debug!(pipe = %path, error = %err, "discord rpc pipe not available");
            Err(DiscordError::PipeNotFound)
        }
    }
}

//! Discord RPC JSON payload shapes — 設計書.md §6.1.2/§6.1.3.

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct HandshakePayload<'a> {
    pub v: u32,
    pub client_id: &'a str,
}

/// Generic outbound command envelope. `evt` is only present for
/// `SUBSCRIBE`/`UNSUBSCRIBE`, which name the event being (un)subscribed to.
#[derive(Debug, Serialize)]
pub struct Command<T: Serialize> {
    pub cmd: String,
    pub args: T,
    pub nonce: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evt: Option<String>,
}

/// Generic inbound envelope. Discord multiplexes command responses,
/// subscribed events, and `ERROR` frames over the same shape, so one struct
/// covers all three rather than three separate ones.
#[derive(Debug, Clone, Deserialize)]
pub struct RpcMessage {
    pub cmd: Option<String>,
    pub evt: Option<String>,
    pub data: Option<serde_json::Value>,
    pub nonce: Option<String>,
    pub code: Option<i64>,
    pub message: Option<String>,
}

/// §6.1.2: `SET_VOICE_SETTINGS` must only ever change `mute` — never the
/// other voice-settings fields (input device, volume, noise suppression,
/// ...). Only serializing this one field makes that a structural guarantee
/// rather than a convention to remember.
#[derive(Debug, Serialize)]
pub struct VoiceSettingsArgs {
    pub mute: bool,
}

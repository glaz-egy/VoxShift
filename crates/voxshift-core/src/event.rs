//! Events flowing into the Voice Coordinator (§11).

use crate::state::{ConnectionState, EventOrigin, MuteState};

#[derive(Debug, Clone)]
pub struct VoiceEvent {
    pub sequence: u64,
    pub origin: EventOrigin,
    pub state: MuteState,
    pub received_at: std::time::Instant,
}

#[derive(Debug, Clone)]
pub enum CoordinatorEvent {
    VrChatMuteSelf(MuteState),
    VrChatAvatarChanged,
    VrChatConnectionChanged(ConnectionState),
    DiscordVoiceSettingsUpdate { mute: MuteState },
    DiscordVoiceChannelStatus { in_voice: bool },
    DiscordConnectionChanged(ConnectionState),
    DiscordCommandAck { nonce: uuid::Uuid, result: Result<MuteState, String> },
    /// A setup-time RPC command (`SUBSCRIBE` to a voice event, or the
    /// initial `GET_VOICE_SETTINGS`) came back with an `ERROR` frame. These
    /// commands have no coordinator-tracked nonce to match against (unlike
    /// `DiscordCommandAck`), so without this the failure — e.g. a missing
    /// scope or an unapproved RPC application — would be silently dropped
    /// after only a `tracing::warn!`, leaving the user staring at a
    /// permanently `Unknown` Discord state with no explanation.
    DiscordVoiceStateUnavailable(String),
}

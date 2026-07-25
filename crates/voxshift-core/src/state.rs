//! Internal state model — 設計書.md §10.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MuteState {
    Unknown,
    Muted,
    Unmuted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Authorizing,
    Connected,
    Degraded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LinkMode {
    InverseBidirectional,
    VrchatMaster,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    Active,
    Paused,
    WaitingForState,
    Faulted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventOrigin {
    VrChat,
    Discord,
    Coordinator,
    Startup,
    Reconnect,
    UserInterface,
}

/// Not part of §10's literal enum list, but required by §14 (config schema)
/// and §12.2/§4.3 (startup/resume behavior). Defined once here so
/// voxshift-storage and voxshift-core::coordinator share one type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupAuthority {
    Vrchat,
    Discord,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResumePolicy {
    KeepState,
    SyncFromVrchat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSnapshot {
    pub vrchat_connection: ConnectionState,
    pub vrchat_mute: MuteState,

    pub discord_connection: ConnectionState,
    pub discord_mute: MuteState,
    pub discord_in_voice_channel: bool,

    pub link_mode: LinkMode,
    pub link_state: LinkState,

    pub last_sync_at: Option<std::time::SystemTime>,
    pub last_error: Option<String>,
}

impl Default for AppSnapshot {
    fn default() -> Self {
        Self {
            vrchat_connection: ConnectionState::Disconnected,
            vrchat_mute: MuteState::Unknown,
            discord_connection: ConnectionState::Disconnected,
            discord_mute: MuteState::Unknown,
            discord_in_voice_channel: false,
            link_mode: LinkMode::InverseBidirectional,
            link_state: LinkState::WaitingForState,
            last_sync_at: None,
            last_error: None,
        }
    }
}

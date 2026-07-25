//! `AppSnapshot` (§10, voxshift-core) -> display-ready values. Pure
//! functions only — testable without a running Slint event loop.

use std::time::SystemTime;

use voxshift_core::state::{
    AppSnapshot, ConnectionState as CoreConnectionState, LinkMode as CoreLinkMode,
    LinkState as CoreLinkState, MuteState as CoreMuteState,
};

use crate::{AppLinkMode, ConnectionState, LinkState, MuteState};

pub fn to_ui_connection_state(state: CoreConnectionState) -> ConnectionState {
    match state {
        CoreConnectionState::Disconnected => ConnectionState::Disconnected,
        CoreConnectionState::Connecting => ConnectionState::Connecting,
        CoreConnectionState::Authorizing => ConnectionState::Authorizing,
        CoreConnectionState::Connected => ConnectionState::Connected,
        CoreConnectionState::Degraded => ConnectionState::Degraded,
    }
}

pub fn to_ui_mute_state(state: CoreMuteState) -> MuteState {
    match state {
        CoreMuteState::Unknown => MuteState::Unknown,
        CoreMuteState::Muted => MuteState::Muted,
        CoreMuteState::Unmuted => MuteState::Unmuted,
    }
}

pub fn to_ui_link_mode(mode: CoreLinkMode) -> AppLinkMode {
    match mode {
        CoreLinkMode::InverseBidirectional => AppLinkMode::InverseBidirectional,
        CoreLinkMode::VrchatMaster => AppLinkMode::VrchatMaster,
    }
}

pub fn to_ui_link_state(state: CoreLinkState) -> LinkState {
    match state {
        CoreLinkState::Active => LinkState::Active,
        CoreLinkState::Paused => LinkState::Paused,
        CoreLinkState::WaitingForState => LinkState::WaitingForState,
        CoreLinkState::Faulted => LinkState::Faulted,
    }
}

/// Formats as UTC `HH:MM:SS`. A follow-up should localize this via the
/// `time` crate (flagged as a needed dependency in the GUI plan) — kept
/// dependency-free for this pass.
pub fn format_last_sync(at: Option<SystemTime>) -> String {
    let Some(at) = at else { return "--:--:--".to_string() };
    let Ok(since_epoch) = at.duration_since(std::time::UNIX_EPOCH) else {
        return "--:--:--".to_string();
    };
    let secs_of_day = since_epoch.as_secs() % 86_400;
    format!(
        "{:02}:{:02}:{:02}",
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60
    )
}

pub struct UiSnapshot {
    pub vrchat_connection: ConnectionState,
    pub vrchat_mute: MuteState,
    pub discord_connection: ConnectionState,
    pub discord_mute: MuteState,
    pub discord_in_voice_channel: bool,
    pub link_mode: AppLinkMode,
    pub link_state: LinkState,
    pub last_sync_label: String,
    pub last_error: String,
}

pub fn from_snapshot(snapshot: &AppSnapshot) -> UiSnapshot {
    UiSnapshot {
        vrchat_connection: to_ui_connection_state(snapshot.vrchat_connection),
        vrchat_mute: to_ui_mute_state(snapshot.vrchat_mute),
        discord_connection: to_ui_connection_state(snapshot.discord_connection),
        discord_mute: to_ui_mute_state(snapshot.discord_mute),
        discord_in_voice_channel: snapshot.discord_in_voice_channel,
        link_mode: to_ui_link_mode(snapshot.link_mode),
        link_state: to_ui_link_state(snapshot.link_state),
        last_sync_label: format_last_sync(snapshot.last_sync_at),
        last_error: snapshot.last_error.clone().unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_every_connection_state() {
        assert_eq!(to_ui_connection_state(CoreConnectionState::Connected), ConnectionState::Connected);
        assert_eq!(to_ui_connection_state(CoreConnectionState::Degraded), ConnectionState::Degraded);
        assert_eq!(to_ui_connection_state(CoreConnectionState::Authorizing), ConnectionState::Authorizing);
    }

    #[test]
    fn maps_every_mute_state() {
        assert_eq!(to_ui_mute_state(CoreMuteState::Muted), MuteState::Muted);
        assert_eq!(to_ui_mute_state(CoreMuteState::Unmuted), MuteState::Unmuted);
        assert_eq!(to_ui_mute_state(CoreMuteState::Unknown), MuteState::Unknown);
    }

    #[test]
    fn format_last_sync_none_is_placeholder() {
        assert_eq!(format_last_sync(None), "--:--:--");
    }

    #[test]
    fn format_last_sync_epoch_is_midnight() {
        assert_eq!(format_last_sync(Some(std::time::UNIX_EPOCH)), "00:00:00");
    }

    #[test]
    fn from_snapshot_carries_last_error_through() {
        let mut snap = AppSnapshot::default();
        snap.last_error = Some("boom".to_string());
        let ui = from_snapshot(&snap);
        assert_eq!(ui.last_error, "boom");
    }

    #[test]
    fn from_snapshot_defaults_last_error_to_empty_string() {
        let snap = AppSnapshot::default();
        let ui = from_snapshot(&snap);
        assert_eq!(ui.last_error, "");
    }
}

//! Pure builders/parsers for the voice-settings-related RPC commands
//! (§6.1.2). No I/O — directly unit-testable.

use uuid::Uuid;

use crate::models::{Command, VoiceSettingsArgs};
use voxshift_core::state::MuteState;

pub fn get_voice_settings_command(nonce: Uuid) -> Command<serde_json::Value> {
    Command {
        cmd: "GET_VOICE_SETTINGS".to_string(),
        args: serde_json::json!({}),
        nonce: nonce.to_string(),
        evt: None,
    }
}

pub fn set_voice_settings_command(nonce: Uuid, mute: bool) -> Command<VoiceSettingsArgs> {
    Command {
        cmd: "SET_VOICE_SETTINGS".to_string(),
        args: VoiceSettingsArgs { mute },
        nonce: nonce.to_string(),
        evt: None,
    }
}

pub fn subscribe_command(nonce: Uuid, evt: &str) -> Command<serde_json::Value> {
    Command {
        cmd: "SUBSCRIBE".to_string(),
        args: serde_json::json!({}),
        nonce: nonce.to_string(),
        evt: Some(evt.to_string()),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum VoiceParseError {
    #[error("voice settings payload is missing the `mute` field")]
    MissingMute,
}

pub fn parse_voice_settings_mute(data: &serde_json::Value) -> Result<MuteState, VoiceParseError> {
    data.get("mute")
        .and_then(|v| v.as_bool())
        .map(|muted| {
            if muted {
                MuteState::Muted
            } else {
                MuteState::Unmuted
            }
        })
        .ok_or(VoiceParseError::MissingMute)
}

/// §6.1.3 `VOICE_CHANNEL_SELECT`: `data` carries `channel_id` while in a
/// voice channel, and is `null` (or has a null `channel_id`) when not.
pub fn parse_voice_channel_selected(data: &serde_json::Value) -> bool {
    data.get("channel_id")
        .is_some_and(|v| v.as_str().is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_voice_settings_only_serializes_mute() {
        let cmd = set_voice_settings_command(Uuid::nil(), true);
        let json = serde_json::to_value(&cmd).unwrap();
        assert_eq!(json["args"], serde_json::json!({ "mute": true }));
        assert_eq!(json["args"].as_object().unwrap().len(), 1);
    }

    #[test]
    fn subscribe_command_carries_evt_field() {
        let cmd = subscribe_command(Uuid::nil(), "VOICE_SETTINGS_UPDATE");
        let json = serde_json::to_value(&cmd).unwrap();
        assert_eq!(json["evt"], "VOICE_SETTINGS_UPDATE");
        assert_eq!(json["cmd"], "SUBSCRIBE");
    }

    #[test]
    fn parse_mute_true() {
        let data = serde_json::json!({ "mute": true, "volume": 100 });
        assert_eq!(parse_voice_settings_mute(&data).unwrap(), MuteState::Muted);
    }

    #[test]
    fn parse_mute_false() {
        let data = serde_json::json!({ "mute": false });
        assert_eq!(parse_voice_settings_mute(&data).unwrap(), MuteState::Unmuted);
    }

    #[test]
    fn parse_mute_missing_field_errors() {
        let data = serde_json::json!({ "volume": 100 });
        assert!(parse_voice_settings_mute(&data).is_err());
    }

    #[test]
    fn voice_channel_selected_detection() {
        assert!(parse_voice_channel_selected(&serde_json::json!({ "channel_id": "123" })));
        assert!(!parse_voice_channel_selected(&serde_json::json!(null)));
        assert!(!parse_voice_channel_selected(&serde_json::json!({})));
    }
}

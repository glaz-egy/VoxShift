//! Commands flowing out of the Voice Coordinator, and in from the UI.

use crate::state::{LinkMode, MuteState};

#[derive(Debug, Clone)]
pub enum CoordinatorCommand {
    SetLinkMode(LinkMode),
    SetPaused(bool),
    ManualResync,
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum VrChatCommand {
    ToggleVoice { expected_after: MuteState },
}

#[derive(Debug, Clone)]
pub enum DiscordCommand {
    SetMute { target: MuteState, nonce: uuid::Uuid },
}

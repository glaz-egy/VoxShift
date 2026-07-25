//! Errors relevant to core coordination / OSC / RPC (§22, non-auth subset).

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("vrchat state unknown, refusing to act")]
    VrChatStateUnknown,
    #[error("discord state unknown, refusing to act")]
    DiscordStateUnknown,
    #[error("vrchat toggle not confirmed within deadline")]
    VrChatToggleUnconfirmed,
    #[error("discord voice settings locked by another RPC app")]
    DiscordVoiceSettingsLocked,
    #[error("discord unavailable: {0}")]
    DiscordUnavailable(String),
    #[error("vrchat osc port unavailable: {0}")]
    VrChatPortUnavailable(String),
    #[error("config corrupt: {0}")]
    ConfigCorrupt(String),
    #[error("credential store error: {0}")]
    CredentialStore(#[from] CredentialStoreError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Windows Credential Manager failure (§22 "Windows Credential Manager失敗").
/// Carries only a Win32 error code / message — never the credential blob.
#[derive(Debug, thiserror::Error)]
#[error("credential manager operation failed: {message}")]
pub struct CredentialStoreError {
    pub message: String,
}

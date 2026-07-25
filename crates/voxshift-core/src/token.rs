//! Shared token storage abstraction (§6.2.4) — defined here so both
//! voxshift-discord (consumer, Phase 2 auth flow) and
//! voxshift-platform-windows (implementor, Credential Manager) depend on
//! voxshift-core rather than on each other.

use std::time::{Duration, SystemTime};

use secrecy::SecretString;

use crate::error::CoreError;

pub struct StoredTokenSet {
    pub access_token: SecretString,
    pub refresh_token: SecretString,
    pub expires_at: SystemTime,
    pub scopes: Vec<String>,
}

impl StoredTokenSet {
    pub fn is_expired_within(&self, margin: Duration) -> bool {
        match self.expires_at.checked_sub(margin) {
            Some(threshold) => SystemTime::now() >= threshold,
            None => true,
        }
    }
}

pub trait TokenStore: Send + Sync {
    fn load(&self) -> Result<Option<StoredTokenSet>, CoreError>;
    fn save(&self, tokens: &StoredTokenSet) -> Result<(), CoreError>;
    fn clear(&self) -> Result<(), CoreError>;
}

//! Windows Credential Manager storage for the Discord OAuth token set —
//! 設計書.md §6.2.4.
//!
//! Uses the raw `windows` crate Credentials API directly rather than the
//! `keyring` crate: it avoids a net-new dependency (and `keyring`'s
//! cross-platform abstraction VoxShift, being Windows-only, never needs),
//! packs all four fields into one JSON blob under `CredentialBlob` more
//! directly than `keyring`'s single-secret-string model, and gives explicit
//! control over `CRED_PERSIST_LOCAL_MACHINE` plus precise Win32 error codes
//! for the §22 "Windows Credential Manager失敗" mapping.

use std::time::{Duration, UNIX_EPOCH};

use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use voxshift_core::error::{CoreError, CredentialStoreError};
use voxshift_core::token::{StoredTokenSet, TokenStore};

const CREDENTIAL_TARGET: &str = "VoxShift/DiscordOAuth";

#[derive(Serialize, Deserialize)]
struct CredentialBlobV1 {
    schema_version: u8,
    access_token: String,
    refresh_token: String,
    expires_at_unix: i64,
    scopes: Vec<String>,
}

#[derive(Default)]
pub struct WindowsCredentialStore;

impl WindowsCredentialStore {
    pub fn new() -> Self {
        Self
    }
}

fn store_error(context: &str, detail: impl std::fmt::Display) -> CoreError {
    CoreError::CredentialStore(CredentialStoreError {
        message: format!("{context}: {detail}"),
    })
}

#[cfg(windows)]
mod win {
    use super::*;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_NOT_FOUND;
    use windows::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_FLAGS,
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    };

    fn target_wide() -> Vec<u16> {
        CREDENTIAL_TARGET.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn load() -> Result<Option<StoredTokenSet>, CoreError> {
        let target_w = target_wide();
        let mut pcred: *mut CREDENTIALW = std::ptr::null_mut();

        let result = unsafe { CredReadW(PCWSTR(target_w.as_ptr()), CRED_TYPE_GENERIC, 0, &mut pcred) };

        match result {
            Ok(()) => {
                let blob = unsafe {
                    let cred = &*pcred;
                    let slice =
                        std::slice::from_raw_parts(cred.CredentialBlob, cred.CredentialBlobSize as usize);
                    let parsed: Result<CredentialBlobV1, _> = serde_json::from_slice(slice);
                    let _ = CredFree(pcred as *const _);
                    parsed
                }
                .map_err(|e| store_error("failed to parse stored credential blob", e))?;

                let expires_at = UNIX_EPOCH + Duration::from_secs(blob.expires_at_unix.max(0) as u64);
                Ok(Some(StoredTokenSet {
                    access_token: SecretString::from(blob.access_token),
                    refresh_token: SecretString::from(blob.refresh_token),
                    expires_at,
                    scopes: blob.scopes,
                }))
            }
            Err(err) => {
                if err.code() == windows::core::HRESULT::from_win32(ERROR_NOT_FOUND.0) {
                    Ok(None)
                } else {
                    Err(store_error("CredReadW failed", err))
                }
            }
        }
    }

    pub fn save(tokens: &StoredTokenSet) -> Result<(), CoreError> {
        let expires_at_unix = tokens
            .expires_at
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let blob = CredentialBlobV1 {
            schema_version: 1,
            access_token: tokens.access_token.expose_secret().to_string(),
            refresh_token: tokens.refresh_token.expose_secret().to_string(),
            expires_at_unix,
            scopes: tokens.scopes.clone(),
        };
        let mut json =
            serde_json::to_vec(&blob).map_err(|e| store_error("failed to serialize credential blob", e))?;

        let mut target_w = target_wide();
        let credential = CREDENTIALW {
            Flags: CRED_FLAGS(0),
            Type: CRED_TYPE_GENERIC,
            TargetName: windows::core::PWSTR(target_w.as_mut_ptr()),
            Comment: windows::core::PWSTR::null(),
            LastWritten: Default::default(),
            CredentialBlobSize: json.len() as u32,
            CredentialBlob: json.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: 0,
            Attributes: std::ptr::null_mut(),
            TargetAlias: windows::core::PWSTR::null(),
            UserName: windows::core::PWSTR::null(),
        };

        let result = unsafe { CredWriteW(&credential, 0) };
        json.zeroize();
        result.map_err(|e| store_error("CredWriteW failed", e))
    }

    pub fn clear() -> Result<(), CoreError> {
        let target_w = target_wide();
        let result = unsafe { CredDeleteW(PCWSTR(target_w.as_ptr()), CRED_TYPE_GENERIC, 0) };
        match result {
            Ok(()) => Ok(()),
            Err(err) if err.code() == windows::core::HRESULT::from_win32(ERROR_NOT_FOUND.0) => Ok(()), // idempotent
            Err(err) => Err(store_error("CredDeleteW failed", err)),
        }
    }
}

impl TokenStore for WindowsCredentialStore {
    #[cfg(windows)]
    fn load(&self) -> Result<Option<StoredTokenSet>, CoreError> {
        win::load()
    }
    #[cfg(not(windows))]
    fn load(&self) -> Result<Option<StoredTokenSet>, CoreError> {
        Ok(None)
    }

    #[cfg(windows)]
    fn save(&self, tokens: &StoredTokenSet) -> Result<(), CoreError> {
        win::save(tokens)
    }
    #[cfg(not(windows))]
    fn save(&self, _tokens: &StoredTokenSet) -> Result<(), CoreError> {
        Err(store_error("credential manager unavailable", "not running on Windows"))
    }

    #[cfg(windows)]
    fn clear(&self) -> Result<(), CoreError> {
        win::clear()
    }
    #[cfg(not(windows))]
    fn clear(&self) -> Result<(), CoreError> {
        Ok(())
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::time::SystemTime;

    // The production store hardcodes its target name, so these tests
    // exercise the same win:: functions against a scoped target by
    // temporarily shadowing CREDENTIAL_TARGET is not possible (it's a
    // const); instead we validate the round trip logic against the real
    // constant, accepting that this test therefore touches the real
    // VoxShift/DiscordOAuth entry. It cleans up after itself via `clear`.
    #[test]
    fn save_load_clear_round_trip() {
        let store = WindowsCredentialStore::new();
        let original = store.load().expect("load should not error even if nothing is stored yet");

        let tokens = StoredTokenSet {
            access_token: SecretString::from("test-access-token".to_string()),
            refresh_token: SecretString::from("test-refresh-token".to_string()),
            expires_at: SystemTime::now() + Duration::from_secs(3600),
            scopes: vec!["rpc".to_string(), "identify".to_string()],
        };

        store.save(&tokens).expect("save should succeed");
        let loaded = store.load().expect("load should succeed").expect("expected a stored credential");
        assert_eq!(loaded.access_token.expose_secret(), "test-access-token");
        assert_eq!(loaded.refresh_token.expose_secret(), "test-refresh-token");
        assert_eq!(loaded.scopes, vec!["rpc", "identify"]);

        store.clear().expect("clear should succeed");
        assert!(store.load().expect("load after clear should not error").is_none());

        // Restore whatever was there before, if anything, so running this
        // test doesn't clobber a real developer's stored session.
        if let Some(previous) = original {
            let _ = store.save(&previous);
        }
    }
}

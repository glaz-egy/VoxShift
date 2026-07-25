//! Config storage — 設計書.md §14, plus the cross-phase additions noted in
//! the implementation plan (`accessibility.textScale` for Phase 3 font
//! scaling). Discord auth is a public client (PKCE) with no server-side
//! broker, so no install-id/rate-limiting field is needed.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use voxshift_core::error::CoreError;
use voxshift_core::state::{LinkMode, ResumePolicy, StartupAuthority};

/// §16: reject/backup configs larger than this rather than trying to parse
/// an unreasonably large file.
const MAX_CONFIG_SIZE_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Dark,
    Light,
    #[default]
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VrChatConfig {
    pub host: String,
    pub send_port: u16,
    pub receive_port: u16,
    pub command_timeout_ms: u64,
}

impl Default for VrChatConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            send_port: 9000,
            receive_port: 9001,
            command_timeout_ms: 1500,
        }
    }
}

// Note: the Discord application Client ID is *not* a runtime setting — it's
// embedded at build time (see voxshift-app's `.cargo/config.toml` +
// `env!("VOXSHIFT_DISCORD_CLIENT_ID")`) since every VoxShift build talks to
// the same Discord application. It's a public client (PKCE, §6.2), so no
// Client Secret is ever stored here or anywhere else.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DiscordConfig {
    pub connect_automatically: bool,
}

impl Default for DiscordConfig {
    fn default() -> Self {
        Self {
            connect_automatically: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StartupConfig {
    pub run_with_windows: bool,
    pub start_minimized: bool,
}

impl Default for StartupConfig {
    fn default() -> Self {
        Self {
            run_with_windows: false,
            start_minimized: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LoggingConfig {
    pub level: String,
    pub file_logging: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "warning".to_string(),
            file_logging: false,
        }
    }
}

/// §7.8 font-size scaling — not present in the 設計書.md §14 schema, added
/// per the Phase 3 GUI plan's flagged gap. Additive/optional by construction
/// (defaults to no-op 1.0), so it doesn't disturb §14's documented schema.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AccessibilityConfig {
    pub text_scale: f32,
}

impl Default for AccessibilityConfig {
    fn default() -> Self {
        Self { text_scale: 1.0 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppConfig {
    pub schema_version: u32,
    pub link_mode: LinkMode,
    pub link_enabled: bool,
    pub startup_authority: StartupAuthority,
    pub resume_policy: ResumePolicy,
    pub theme: Theme,
    pub reduced_motion: bool,
    pub vrchat: VrChatConfig,
    pub discord: DiscordConfig,
    pub startup: StartupConfig,
    pub logging: LoggingConfig,
    pub accessibility: AccessibilityConfig,
    /// UI language pack code (e.g. "en", "ja"). Not part of the §14
    /// example schema — added so the UI can be localized; see
    /// `voxshift_ui::i18n` for how language packs (bundled or
    /// user-supplied) are resolved from this code.
    pub language: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: 1,
            link_mode: LinkMode::InverseBidirectional,
            link_enabled: true,
            startup_authority: StartupAuthority::Vrchat,
            resume_policy: ResumePolicy::SyncFromVrchat,
            theme: Theme::System,
            reduced_motion: false,
            vrchat: VrChatConfig::default(),
            discord: DiscordConfig::default(),
            startup: StartupConfig::default(),
            logging: LoggingConfig::default(),
            accessibility: AccessibilityConfig::default(),
            language: "en".to_string(),
        }
    }
}

pub fn config_path() -> Result<PathBuf, CoreError> {
    let base = directories::BaseDirs::new()
        .ok_or_else(|| CoreError::ConfigCorrupt("could not resolve %LOCALAPPDATA%".to_string()))?;
    Ok(base.data_local_dir().join("VoxShift").join("config.json"))
}

/// Loads config.json, tolerating every failure mode described in §22
/// ("設定ファイル破損 -> バックアップ後に初期化") by falling back to
/// defaults rather than ever propagating an error to the caller — a bad
/// config must never prevent VoxShift from starting.
pub fn load() -> AppConfig {
    let path = match config_path() {
        Ok(p) => p,
        Err(err) => {
            tracing::warn!(error = %err, "could not resolve config path; using in-memory defaults");
            return AppConfig::default();
        }
    };
    match load_from(&path) {
        Ok(cfg) => cfg,
        Err(err) => {
            tracing::warn!(error = %err, "falling back to default configuration");
            let cfg = AppConfig::default();
            if let Err(save_err) = save_to(&path, &cfg) {
                tracing::warn!(error = %save_err, "failed to persist default configuration");
            }
            cfg
        }
    }
}

fn load_from(path: &Path) -> Result<AppConfig, CoreError> {
    if !path.exists() {
        let cfg = AppConfig::default();
        save_to(path, &cfg)?;
        return Ok(cfg);
    }

    let metadata = std::fs::metadata(path)?;
    if metadata.len() > MAX_CONFIG_SIZE_BYTES {
        backup_corrupt(path)?;
        let cfg = AppConfig::default();
        save_to(path, &cfg)?;
        return Ok(cfg);
    }

    let data = std::fs::read(path)?;
    // Tolerate a leading UTF-8 BOM: `serde_json` treats it as invalid JSON,
    // but some editors (and `Set-Content -Encoding utf8` on Windows
    // PowerShell) write one by default when a user hand-edits this file.
    let data = data.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&data);
    match serde_json::from_slice::<AppConfig>(data) {
        Ok(cfg) => {
            // Persist back so newly-defaulted fields (e.g. one added by a
            // future schema version) are captured on disk immediately.
            save_to(path, &cfg)?;
            Ok(cfg)
        }
        Err(parse_err) => {
            tracing::warn!(error = %parse_err, "config.json is not valid, backing up and resetting");
            backup_corrupt(path)?;
            let cfg = AppConfig::default();
            save_to(path, &cfg)?;
            Ok(cfg)
        }
    }
}

fn backup_corrupt(path: &Path) -> Result<(), CoreError> {
    let backup_path = path.with_extension("json.bak");
    std::fs::rename(path, backup_path)?;
    Ok(())
}

/// Atomic write: `config.json.tmp` -> flush -> rename over `config.json`
/// (§14).
pub fn save(cfg: &AppConfig) -> Result<(), CoreError> {
    let path = config_path()?;
    save_to(&path, cfg)
}

fn save_to(path: &Path, cfg: &AppConfig) -> Result<(), CoreError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_extension("json.tmp");
    let json = serde_json::to_vec_pretty(cfg)
        .map_err(|e| CoreError::ConfigCorrupt(format!("failed to serialize config: {e}")))?;

    let mut file = std::fs::File::create(&tmp_path)?;
    file.write_all(&json)?;
    file.sync_all()?;
    drop(file);

    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn default_config_round_trips_through_json() {
        let cfg = AppConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.schema_version, cfg.schema_version);
        assert_eq!(parsed.vrchat.send_port, 9000);
        assert_eq!(parsed.vrchat.receive_port, 9001);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let partial = serde_json::json!({ "schemaVersion": 1 });
        let cfg: AppConfig = serde_json::from_value(partial).unwrap();
        assert_eq!(cfg.link_mode, LinkMode::InverseBidirectional);
        assert_eq!(cfg.vrchat.send_port, 9000);
        assert_eq!(cfg.accessibility.text_scale, 1.0);
    }

    fn temp_config_path() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("voxshift-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("config.json")
    }

    #[test]
    fn save_then_load_round_trips_atomically() {
        let path = temp_config_path();
        let mut cfg = AppConfig::default();
        cfg.reduced_motion = true;
        cfg.vrchat.send_port = 9100;

        save_to(&path, &cfg).unwrap();
        assert!(!path.with_extension("json.tmp").exists(), "tmp file must be renamed away, not left behind");

        let loaded = load_from(&path).unwrap();
        assert_eq!(loaded.reduced_motion, true);
        assert_eq!(loaded.vrchat.send_port, 9100);

        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn missing_config_file_creates_defaults_on_disk() {
        let path = temp_config_path();
        assert!(!path.exists());

        let cfg = load_from(&path).unwrap();
        assert_eq!(cfg.schema_version, 1);
        assert!(path.exists(), "load_from must write out a default config.json when none exists");

        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn corrupt_config_is_backed_up_and_reset_to_defaults() {
        let path = temp_config_path();
        std::fs::write(&path, b"{ not valid json").unwrap();

        let cfg = load_from(&path).unwrap();
        assert_eq!(cfg.schema_version, 1);
        assert!(path.with_extension("json.bak").exists(), "corrupt file must be backed up");
        assert!(path.exists(), "a fresh default config.json must be written after backing up");

        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn oversized_config_is_backed_up_and_reset_to_defaults() {
        let path = temp_config_path();
        let oversized = vec![b' '; (MAX_CONFIG_SIZE_BYTES + 1) as usize];
        std::fs::write(&path, &oversized).unwrap();

        let cfg = load_from(&path).unwrap();
        assert_eq!(cfg.schema_version, 1);
        assert!(path.with_extension("json.bak").exists());

        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn config_with_leading_utf8_bom_is_still_parsed_not_reset() {
        // Some editors (and `Set-Content -Encoding utf8` on Windows
        // PowerShell) write a UTF-8 BOM by default when a user hand-edits
        // this file — it must not be treated as corruption.
        let path = temp_config_path();
        let mut cfg = AppConfig::default();
        cfg.reduced_motion = true;
        let mut bytes = vec![0xEFu8, 0xBB, 0xBF];
        bytes.extend_from_slice(&serde_json::to_vec(&cfg).unwrap());
        std::fs::write(&path, &bytes).unwrap();

        let loaded = load_from(&path).unwrap();
        assert_eq!(loaded.reduced_motion, true);
        assert!(!path.with_extension("json.bak").exists(), "a BOM must not be treated as corruption");

        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }
}

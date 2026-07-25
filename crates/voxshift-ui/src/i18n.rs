//! Localization — bundled English/Japanese language packs, plus support for
//! externally-supplied packs dropped into
//! `%LOCALAPPDATA%\VoxShift\lang\<code>.json` (no rebuild needed to add a
//! new language or override strings in a bundled one — this is the
//! "language pack" extension point the design asked for).

use std::collections::HashMap;
use std::path::PathBuf;

use crate::Strings;

const BUNDLED_EN: &str = include_str!("../lang/en.json");
const BUNDLED_JA: &str = include_str!("../lang/ja.json");

/// `(code, display_name)` for every language VoxShift ships with. External
/// packs (see [`external_pack_dir`]) can add more without a rebuild.
const BUNDLED_LANGUAGES: &[(&str, &str)] = &[("en", "English"), ("ja", "日本語")];

fn bundled_json(code: &str) -> Option<&'static str> {
    match code {
        "en" => Some(BUNDLED_EN),
        "ja" => Some(BUNDLED_JA),
        _ => None,
    }
}

fn parse_table(json: &str) -> HashMap<String, String> {
    serde_json::from_str(json).unwrap_or_default()
}

/// Directory external language packs live in — dropping a `<code>.json`
/// file there (same keys as `lang/en.json`) adds a new language or
/// overrides strings in a bundled one, with no rebuild required.
pub fn external_pack_dir() -> Option<PathBuf> {
    let base = directories::BaseDirs::new()?;
    Some(base.data_local_dir().join("VoxShift").join("lang"))
}

fn load_external(code: &str) -> Option<HashMap<String, String>> {
    let dir = external_pack_dir()?;
    let data = std::fs::read_to_string(dir.join(format!("{code}.json"))).ok()?;
    Some(parse_table(&data))
}

/// Lists every language currently available as `(code, display_name)`: the
/// bundled ones, plus any `<code>.json` found in the external pack
/// directory that isn't already bundled (so a community-contributed pack
/// shows up in the Settings picker without a VoxShift update).
pub fn available_languages() -> Vec<(String, String)> {
    let mut languages: Vec<(String, String)> = BUNDLED_LANGUAGES
        .iter()
        .map(|(code, name)| (code.to_string(), name.to_string()))
        .collect();

    if let Some(dir) = external_pack_dir() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let Some(code) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                if languages.iter().any(|(existing, _)| existing == code) {
                    continue; // already bundled; the file only overrides strings
                }
                languages.push((code.to_string(), code.to_string()));
            }
        }
    }
    languages
}

/// Builds the merged string table for `code`: English bundled defaults as
/// the base (so any untranslated key always falls back to something
/// sensible), overlaid by the bundled pack for `code` (if any), further
/// overlaid by an external pack of the same name (if present).
pub fn load_language(code: &str) -> HashMap<String, String> {
    let mut table = bundled_json("en").map(parse_table).unwrap_or_default();
    if code != "en" {
        if let Some(json) = bundled_json(code) {
            table.extend(parse_table(json));
        }
    }
    if let Some(external) = load_external(code) {
        table.extend(external);
    }
    table
}

/// Pushes every string in `table` into the live `Strings` global. Keys
/// missing from `table` are left at whatever `strings.slint`'s own English
/// default is — so a partial/community translation degrades gracefully
/// instead of ever showing blank text.
pub fn apply(strings: &Strings, table: &HashMap<String, String>) {
    macro_rules! set {
        ($setter:ident, $key:literal) => {
            if let Some(value) = table.get($key) {
                strings.$setter(value.clone().into());
            }
        };
    }

    set!(set_nav_dashboard, "nav-dashboard");
    set!(set_nav_link_mode, "nav-link-mode");
    set!(set_nav_settings, "nav-settings");
    set!(set_nav_diagnostics, "nav-diagnostics");
    set!(set_nav_about, "nav-about");

    set!(set_card_vrchat_title, "card-vrchat-title");
    set!(set_card_discord_title, "card-discord-title");
    set!(set_status_mic_on, "status-mic-on");
    set!(set_status_mic_off, "status-mic-off");
    set!(set_status_unknown, "status-unknown");
    set!(set_status_vc_muted, "status-vc-muted");
    set!(set_status_vc_on, "status-vc-on");
    set!(set_conn_connected, "conn-connected");
    set!(set_conn_connecting, "conn-connecting");
    set!(set_conn_authorizing, "conn-authorizing");
    set!(set_conn_degraded, "conn-degraded");
    set!(set_conn_disconnected, "conn-disconnected");
    set!(set_mode_inverse, "mode-inverse");
    set!(set_mode_vrchat_priority, "mode-vrchat-priority");
    set!(set_action_pause, "action-pause");
    set!(set_action_resume, "action-resume");
    set!(set_action_resync, "action-resync");
    set!(set_label_last_sync_prefix, "label-last-sync-prefix");

    set!(set_link_mode_title, "link-mode-title");
    set!(set_link_mode_description, "link-mode-description");

    set!(set_settings_title, "settings-title");
    set!(set_settings_reduce_motion, "settings-reduce-motion");
    set!(set_settings_on, "settings-on");
    set!(set_settings_off, "settings-off");
    set!(set_settings_language_title, "settings-language-title");
    set!(set_settings_discord_title, "settings-discord-title");
    set!(set_settings_discord_description, "settings-discord-description");
    set!(set_settings_discord_authorize_button, "settings-discord-authorize-button");

    set!(set_diagnostics_title, "diagnostics-title");
    set!(set_diagnostics_copy, "diagnostics-copy");

    set!(set_about_title, "about-title");
    set!(set_about_description, "about-description");
    set!(set_about_footer, "about-footer");
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_KEYS: &[&str] = &[
        "nav-dashboard", "nav-link-mode", "nav-settings", "nav-diagnostics", "nav-about",
        "card-vrchat-title", "card-discord-title", "status-mic-on", "status-mic-off",
        "status-unknown", "status-vc-muted", "status-vc-on", "conn-connected", "conn-connecting",
        "conn-authorizing", "conn-degraded", "conn-disconnected", "mode-inverse",
        "mode-vrchat-priority", "action-pause", "action-resume", "action-resync",
        "label-last-sync-prefix", "link-mode-title", "link-mode-description", "settings-title",
        "settings-reduce-motion", "settings-on", "settings-off", "settings-language-title",
        "settings-discord-title", "settings-discord-description",
        "settings-discord-authorize-button", "diagnostics-title", "diagnostics-copy",
        "about-title", "about-description", "about-footer",
    ];

    #[test]
    fn bundled_english_has_every_key_used_by_apply() {
        let table = load_language("en");
        for key in ALL_KEYS {
            assert!(table.contains_key(*key), "missing key in bundled en.json: {key}");
        }
    }

    #[test]
    fn bundled_japanese_has_every_key_english_has() {
        let ja = load_language("ja");
        for key in ALL_KEYS {
            assert!(ja.contains_key(*key), "ja.json is missing key present in en.json: {key}");
        }
    }

    #[test]
    fn unknown_language_falls_back_to_english_only() {
        let table = load_language("xx");
        assert_eq!(table.get("nav-dashboard").map(String::as_str), Some("Dashboard"));
    }

    #[test]
    fn available_languages_includes_bundled_english_and_japanese() {
        let langs = available_languages();
        assert!(langs.iter().any(|(code, _)| code == "en"));
        assert!(langs.iter().any(|(code, _)| code == "ja"));
    }
}

//! Theme resolution + pushing resolved `.claude/DESIGN.md` tokens into the
//! live `Tokens` global (§7.5 dark/light/system).

use crate::design_tokens::{ColorSet, TypeScale, BASE_TYPE_SCALE, DARK_COLORS, LIGHT_COLORS};
use crate::Tokens;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfiguredTheme {
    Dark,
    Light,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsTheme {
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedTheme {
    Dark,
    Light,
}

/// Pure — directly unit-testable without touching Windows or Slint.
pub fn resolve(configured: ConfiguredTheme, os: OsTheme) -> ResolvedTheme {
    match configured {
        ConfiguredTheme::Dark => ResolvedTheme::Dark,
        ConfiguredTheme::Light => ResolvedTheme::Light,
        ConfiguredTheme::System => match os {
            OsTheme::Dark => ResolvedTheme::Dark,
            OsTheme::Light => ResolvedTheme::Light,
        },
    }
}

fn color_set_for(theme: ResolvedTheme) -> &'static ColorSet {
    match theme {
        ResolvedTheme::Dark => &DARK_COLORS,
        ResolvedTheme::Light => &LIGHT_COLORS,
    }
}

fn to_slint_color(packed_rgba: u32) -> slint::Color {
    let r = ((packed_rgba >> 24) & 0xFF) as u8;
    let g = ((packed_rgba >> 16) & 0xFF) as u8;
    let b = ((packed_rgba >> 8) & 0xFF) as u8;
    let a = (packed_rgba & 0xFF) as u8;
    slint::Color::from_argb_u8(a, r, g, b)
}

/// Pushes the resolved theme + text-scale into the live `Tokens` global.
/// Call at startup and whenever the theme or text-scale setting changes.
pub fn apply(tokens: &Tokens, theme: ResolvedTheme, text_scale: f32) {
    let colors = color_set_for(theme);
    let type_scale: TypeScale = BASE_TYPE_SCALE.scaled(text_scale);

    tokens.set_color_vrchat(to_slint_color(colors.vrchat));
    tokens.set_color_discord(to_slint_color(colors.discord));
    tokens.set_color_state_normal(to_slint_color(colors.state_normal));
    tokens.set_color_state_paused(to_slint_color(colors.state_paused));
    tokens.set_color_state_error(to_slint_color(colors.state_error));
    tokens.set_color_state_disabled(to_slint_color(colors.state_disabled));
    tokens.set_color_state_normal_tint(to_slint_color(colors.state_normal_tint));
    tokens.set_color_state_paused_tint(to_slint_color(colors.state_paused_tint));
    tokens.set_color_state_error_tint(to_slint_color(colors.state_error_tint));
    tokens.set_color_state_disabled_tint(to_slint_color(colors.state_disabled_tint));
    tokens.set_color_bg_top(to_slint_color(colors.bg_top));
    tokens.set_color_bg_bottom(to_slint_color(colors.bg_bottom));
    tokens.set_color_surface_card(to_slint_color(colors.surface_card));
    tokens.set_color_surface_card_hover(to_slint_color(colors.surface_card_hover));
    tokens.set_color_surface_elevated(to_slint_color(colors.surface_elevated));
    tokens.set_color_border_hairline(to_slint_color(colors.border_hairline));
    tokens.set_color_text_primary(to_slint_color(colors.text_primary));
    tokens.set_color_text_secondary(to_slint_color(colors.text_secondary));
    tokens.set_color_text_disabled(to_slint_color(colors.text_disabled));
    tokens.set_color_text_on_accent_light(to_slint_color(colors.text_on_accent_light));
    tokens.set_color_text_on_accent_dark(to_slint_color(colors.text_on_accent_dark));
    tokens.set_color_focus_ring(to_slint_color(colors.focus_ring));
    tokens.set_color_nav_active_indicator(to_slint_color(colors.nav_active_indicator));

    tokens.set_font_nav_label(type_scale.nav_label);
    tokens.set_font_card_title(type_scale.card_title);
    tokens.set_font_status_primary(type_scale.status_primary);
    tokens.set_font_status_secondary(type_scale.status_secondary);
    tokens.set_font_body(type_scale.body);
    tokens.set_font_caption(type_scale.caption);
    tokens.set_font_button_label(type_scale.button_label);
    tokens.set_font_badge_label(type_scale.badge_label);
    tokens.set_font_link_glyph(type_scale.link_glyph);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_dark_and_light_ignore_the_os_theme() {
        assert_eq!(resolve(ConfiguredTheme::Dark, OsTheme::Light), ResolvedTheme::Dark);
        assert_eq!(resolve(ConfiguredTheme::Light, OsTheme::Dark), ResolvedTheme::Light);
    }

    #[test]
    fn system_follows_the_os_theme() {
        assert_eq!(resolve(ConfiguredTheme::System, OsTheme::Dark), ResolvedTheme::Dark);
        assert_eq!(resolve(ConfiguredTheme::System, OsTheme::Light), ResolvedTheme::Light);
    }
}

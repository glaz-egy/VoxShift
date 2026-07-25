//! Rust-side source of truth for `.claude/DESIGN.md`. Kept Slint-free
//! (plain packed-RGBA `u32` + `f32` px) so it's directly unit-testable
//! without a Slint runtime; `theme.rs` converts these into `slint::Color`
//! and pushes them into the generated `Tokens` global.

#[derive(Debug, Clone, Copy)]
pub struct ColorSet {
    pub vrchat: u32,
    pub discord: u32,
    pub state_normal: u32,
    pub state_paused: u32,
    pub state_error: u32,
    pub state_disabled: u32,
    pub state_normal_tint: u32,
    pub state_paused_tint: u32,
    pub state_error_tint: u32,
    pub state_disabled_tint: u32,
    pub bg_top: u32,
    pub bg_bottom: u32,
    pub surface_card: u32,
    pub surface_card_hover: u32,
    pub surface_elevated: u32,
    pub border_hairline: u32,
    pub text_primary: u32,
    pub text_secondary: u32,
    pub text_disabled: u32,
    pub text_on_accent_light: u32,
    pub text_on_accent_dark: u32,
    pub focus_ring: u32,
    pub nav_active_indicator: u32,
}

/// Packed as `0xRRGGBBAA`.
pub const DARK_COLORS: ColorSet = ColorSet {
    vrchat: 0x35D8C6FF,
    discord: 0x5865F2FF,
    state_normal: 0x34C972FF,
    state_paused: 0xF5A623FF,
    state_error: 0xEF5350FF,
    state_disabled: 0x6B7686FF,
    state_normal_tint: 0x34C9721F,
    state_paused_tint: 0xF5A6231F,
    state_error_tint: 0xEF53501F,
    state_disabled_tint: 0x6B76861F,
    bg_top: 0x0B1220FF,
    bg_bottom: 0x05070CFF,
    surface_card: 0x121A2CFF,
    surface_card_hover: 0x17203AFF,
    surface_elevated: 0x1B2542FF,
    border_hairline: 0xFFFFFF14,
    text_primary: 0xF2F5FAFF,
    text_secondary: 0x9FB0C8FF,
    text_disabled: 0x6B7686FF,
    text_on_accent_light: 0x06110EFF,
    text_on_accent_dark: 0xFFFFFFFF,
    focus_ring: 0xFFFFFFB3,
    nav_active_indicator: 0xF2F5FAB3,
};

pub const LIGHT_COLORS: ColorSet = ColorSet {
    vrchat: 0x12A190FF,
    discord: 0x4752C4FF,
    state_normal: 0x1F9D57FF,
    state_paused: 0xB4790AFF,
    state_error: 0xD93A37FF,
    state_disabled: 0x626C7CFF,
    state_normal_tint: 0x1F9D571A,
    state_paused_tint: 0xB4790A1A,
    state_error_tint: 0xD93A371A,
    state_disabled_tint: 0x626C7C1A,
    bg_top: 0xF3F6FCFF,
    bg_bottom: 0xE4E9F5FF,
    surface_card: 0xFFFFFFFF,
    surface_card_hover: 0xF3F5FAFF,
    surface_elevated: 0xFFFFFFFF,
    border_hairline: 0x00000014,
    text_primary: 0x10141FFF,
    text_secondary: 0x515B6EFF,
    text_disabled: 0x9AA3B2FF,
    text_on_accent_light: 0x06110EFF,
    text_on_accent_dark: 0xFFFFFFFF,
    focus_ring: 0x10141FB3,
    nav_active_indicator: 0x10141FB3,
};

#[derive(Debug, Clone, Copy)]
pub struct TypeScale {
    pub nav_label: f32,
    pub card_title: f32,
    pub status_primary: f32,
    pub status_secondary: f32,
    pub body: f32,
    pub caption: f32,
    pub button_label: f32,
    pub badge_label: f32,
    pub link_glyph: f32,
}

pub const BASE_TYPE_SCALE: TypeScale = TypeScale {
    nav_label: 13.0,
    card_title: 15.0,
    status_primary: 18.0,
    status_secondary: 12.0,
    body: 13.0,
    caption: 11.0,
    button_label: 13.0,
    badge_label: 11.0,
    link_glyph: 20.0,
};

impl TypeScale {
    pub fn scaled(&self, factor: f32) -> TypeScale {
        TypeScale {
            nav_label: self.nav_label * factor,
            card_title: self.card_title * factor,
            status_primary: self.status_primary * factor,
            status_secondary: self.status_secondary * factor,
            body: self.body * factor,
            caption: self.caption * factor,
            button_label: self.button_label * factor,
            badge_label: self.badge_label * factor,
            link_glyph: self.link_glyph * factor,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaled_multiplies_every_field() {
        let scaled = BASE_TYPE_SCALE.scaled(2.0);
        assert_eq!(scaled.nav_label, BASE_TYPE_SCALE.nav_label * 2.0);
        assert_eq!(scaled.link_glyph, BASE_TYPE_SCALE.link_glyph * 2.0);
    }

    #[test]
    fn identity_scale_is_a_no_op() {
        let scaled = BASE_TYPE_SCALE.scaled(1.0);
        assert_eq!(scaled.body, BASE_TYPE_SCALE.body);
    }
}

//! Colour palette and global style.
//!
//! The palette is computed from [`Settings`] every frame but only *applied* when
//! the resolved scheme changes, so switching theme does not rebuild the style
//! 60 times a second.

use app_core::{Settings, Theme};
use egui::{Color32, Context, FontId, TextStyle};

#[derive(Clone, Copy)]
pub struct Palette {
    pub is_dark: bool,
    /// Window background.
    pub bg: Color32,
    /// Raised surface: cards, panels, inputs.
    pub surface: Color32,
    /// Alternate surface: hovered rows, tracks.
    pub surface_alt: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub accent: Color32,
    pub border: Color32,
    pub danger: Color32,
    pub success: Color32,
}

impl Palette {
    pub fn dark() -> Self {
        Self {
            is_dark: true,
            bg: Color32::from_rgb(0x0F, 0x11, 0x15),
            surface: Color32::from_rgb(0x17, 0x1A, 0x21),
            surface_alt: Color32::from_rgb(0x21, 0x25, 0x2F),
            text: Color32::from_rgb(0xEC, 0xEF, 0xF4),
            text_dim: Color32::from_rgb(0x9A, 0xA3, 0xB2),
            accent: Color32::from_rgb(0x4F, 0x8C, 0xFF),
            border: Color32::from_rgb(0x26, 0x2B, 0x36),
            danger: Color32::from_rgb(0xF2, 0x6D, 0x6D),
            success: Color32::from_rgb(0x55, 0xC2, 0x8A),
        }
    }

    pub fn light() -> Self {
        Self {
            is_dark: false,
            bg: Color32::from_rgb(0xF5, 0xF6, 0xF9),
            surface: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            surface_alt: Color32::from_rgb(0xEA, 0xED, 0xF3),
            text: Color32::from_rgb(0x16, 0x19, 0x1F),
            text_dim: Color32::from_rgb(0x5B, 0x64, 0x72),
            accent: Color32::from_rgb(0x2F, 0x6F, 0xEB),
            border: Color32::from_rgb(0xDC, 0xE0, 0xE7),
            danger: Color32::from_rgb(0xC0, 0x39, 0x39),
            success: Color32::from_rgb(0x22, 0x8B, 0x5A),
        }
    }

    pub fn choose(settings: &Settings, ctx: &Context) -> Self {
        let dark = match settings.theme {
            Theme::Dark => true,
            Theme::Light => false,
            // `dark_mode` is what the platform asked for until the user
            // overrides it, which is exactly "follow the system".
            Theme::System => ctx.style().visuals.dark_mode,
        };
        if dark {
            Self::dark()
        } else {
            Self::light()
        }
    }
}

/// Applies the palette and typography once per scheme change.
#[derive(Default)]
pub struct ThemeCache {
    applied: Option<(bool, bool)>,
}

impl ThemeCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply(&mut self, ctx: &Context, settings: &Settings) -> Palette {
        let palette = Palette::choose(settings, ctx);
        let key = (palette.is_dark, settings.reduce_motion);
        if self.applied == Some(key) {
            return palette;
        }
        self.applied = Some(key);

        let mut visuals = if palette.is_dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        visuals.panel_fill = palette.bg;
        visuals.window_fill = palette.surface;
        visuals.extreme_bg_color = if palette.is_dark {
            Color32::from_rgb(0x0A, 0x0C, 0x10)
        } else {
            Color32::from_rgb(0xE3, 0xE7, 0xEE)
        };
        visuals.override_text_color = Some(palette.text);
        visuals.selection.bg_fill = palette.accent.linear_multiply(0.45_f32);
        visuals.selection.stroke = egui::Stroke::new(1.0_f32, palette.accent);
        visuals.hyperlink_color = palette.accent;
        visuals.window_shadow = egui::epaint::Shadow::NONE;
        ctx.set_visuals(visuals);

        let mut style = (*ctx.style()).clone();
        if settings.reduce_motion {
            // egui animates widget state; zeroing the duration removes it without
            // disabling any functionality.
            style.animation_time = 0.0_f32;
        }
        style.spacing.item_spacing = egui::vec2(10.0, 10.0);
        style.spacing.button_padding = egui::vec2(12.0, 7.0);
        style.text_styles.insert(
            TextStyle::Heading,
            FontId::new(24.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            TextStyle::Body,
            FontId::new(14.5, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            TextStyle::Button,
            FontId::new(14.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            TextStyle::Small,
            FontId::new(12.0, egui::FontFamily::Proportional),
        );
        ctx.set_style(style);

        palette
    }
}

/// Deterministic accent-ish tints for cover placeholders, so a library with no
/// artwork still reads as a grid of distinct items instead of grey boxes.
pub fn placeholder_tint(seed: &str) -> (Color32, Color32) {
    const PAIRS: [(u8, u8, u8, u8, u8, u8); 8] = [
        (0x2E, 0x3A, 0x5C, 0x1B, 0x22, 0x38),
        (0x4A, 0x2E, 0x4F, 0x28, 0x18, 0x2E),
        (0x24, 0x45, 0x40, 0x14, 0x26, 0x25),
        (0x4F, 0x3A, 0x24, 0x2C, 0x1F, 0x13),
        (0x2B, 0x3D, 0x52, 0x17, 0x21, 0x2D),
        (0x45, 0x2C, 0x3A, 0x26, 0x18, 0x20),
        (0x33, 0x40, 0x2E, 0x1B, 0x23, 0x18),
        (0x3A, 0x33, 0x4E, 0x1F, 0x1B, 0x2A),
    ];
    let index = stable_hash(seed) as usize % PAIRS.len();
    let (r1, g1, b1, r2, g2, b2) = PAIRS[index];
    (Color32::from_rgb(r1, g1, b1), Color32::from_rgb(r2, g2, b2))
}

pub fn stable_hash(input: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in input.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tint_is_stable_and_in_range() {
        let a = placeholder_tint("provider::42");
        let b = placeholder_tint("provider::42");
        assert_eq!(a.0, b.0);
        assert_eq!(a.1, b.1);
        // A different title should usually land on a different tint.
        let c = placeholder_tint("provider::43");
        assert_ne!((a.0, a.1), (c.0, c.1));
    }

    #[test]
    fn hash_differs_for_similar_keys() {
        assert_ne!(stable_hash("a1"), stable_hash("a2"));
    }
}

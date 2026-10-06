//! Plugin-owned terminal palette, resolved from editor theme tokens and user settings.
use super::Terminal;
use plugin_protocol::FontStyle;
use std::{collections::BTreeMap, sync::LazyLock};

/// Immutable bundled domain defaults are parsed once; themes may override the plugin's public roles.
static DEFAULTS: LazyLock<serde_json::Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../theme.json")).expect("bundled terminal theme")
});

/// ANSI names follow Alacritty/VTE indices 0..15, including the bright variants.
const ANSI_NAMES: [&str; 16] = [
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "white",
    "bright_black",
    "bright_red",
    "bright_green",
    "bright_yellow",
    "bright_blue",
    "bright_magenta",
    "bright_cyan",
    "bright_white",
];

/// Read immutable defaults from one package resource instead of a duplicate host/guest palette.
fn bundled_value(dark: bool, path: &str) -> &'static serde_json::Value {
    path.split('.').fold(
        &DEFAULTS[if dark { "dark" } else { "light" }],
        |value, key| &value[key],
    )
}

fn bundled_color(dark: bool, path: &str) -> Option<u32> {
    user_color(bundled_value(dark, path).as_str())
}
// Dim SGR colors remain subdued but legible on each editor canvas.
const LIGHT_DIM: [u32; 8] = [
    0x59616e, 0x9d4a43, 0x327651, 0x775913, 0x3f64a6, 0x7251a6, 0x376b78, 0x626875,
];
const DARK_DIM: [u32; 8] = [
    0x89919b, 0xbd8583, 0x84b292, 0xbda87f, 0x8da9d0, 0xb5a0cb, 0x87b9c2, 0xa6a9b1,
];

/// Parse a user-owned #RRGGBB setting after settings validation.
fn user_color(value: Option<&str>) -> Option<u32> {
    value.and_then(|value| u32::from_str_radix(value.trim_start_matches('#'), 16).ok())
}

/// WCAG luminance makes the cursor's text readable regardless of accent color.
fn luminance(color: u32) -> f32 {
    let channel = |shift: u32| {
        let value = ((color >> shift) & 0xff_u32) as f32 / 255.;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
}

fn contrast(a: u32, b: u32) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

impl Terminal {
    /// Native terminal collections use the same bounded role map as every other public UI consumer.
    pub(super) fn content_colors(&self) -> BTreeMap<String, u32> {
        fn flatten(value: &serde_json::Value, prefix: &str, out: &mut BTreeMap<String, u32>) {
            if let Some(object) = value.as_object() {
                for (key, value) in object {
                    flatten(
                        value,
                        &if prefix.is_empty() {
                            key.clone()
                        } else {
                            format!("{prefix}.{key}")
                        },
                        out,
                    );
                }
            } else if let Some(color) = value
                .as_str()
                .and_then(|value| u32::from_str_radix(value.trim_start_matches('#'), 16).ok())
            {
                out.insert(prefix.into(), color);
            }
        }
        let mut colors = BTreeMap::new();
        flatten(
            &DEFAULTS[if self.env.dark { "dark" } else { "light" }]["ui"],
            "",
            &mut colors,
        );
        colors
    }
    /// Event::Theme replaces the environment; every painted text role resolves on demand.
    pub(super) fn text_style(&self, role: &str, monospace: bool) -> FontStyle {
        // Role overrides remain user-owned; the package supplies its content/error defaults.
        let defaults = serde_json::from_value::<FontStyle>(
            bundled_value(self.env.dark, &format!("typography.{role}")).clone(),
        )
        .unwrap_or_default();
        // Global user typography is an override too; package values are defaults only.
        let style = self
            .env
            .font_style("terminal", role, monospace)
            .over(defaults);
        FontStyle {
            family: Some(
                style
                    .family
                    .unwrap_or_else(|| self.settings.font_family.clone()),
            ),
            size_px: Some(style.size_px.unwrap_or(self.settings.font_size)),
            bold: style.bold,
        }
    }

    /// Read only this plugin's color tokens from the editor's generic theme API.
    fn theme_color(&self, name: &str) -> Option<u32> {
        self.env.color("terminal", name)
    }

    /// Let user tokens override the same owned defaults published for native controls.
    pub(super) fn ui_color(&self, name: &str, fallback: u32) -> u32 {
        self.theme_color(&format!("ui.{name}"))
            .or_else(|| bundled_color(self.env.dark, &format!("ui.{name}")))
            .unwrap_or(fallback)
    }

    /// Active theme tokens win over user settings and plugin-owned defaults.
    pub(super) fn color(&self, index: usize) -> u32 {
        match index {
            0..=15 => self
                .theme_color(&format!("ansi.{}", ANSI_NAMES[index]))
                .or_else(|| {
                    user_color(
                        self.settings
                            .theme
                            .ansi
                            .as_ref()
                            .map(|palette| palette[index].as_str()),
                    )
                })
                .unwrap_or_else(|| {
                    bundled_color(self.env.dark, &format!("ansi.{}", ANSI_NAMES[index]))
                        .expect("bundled named ANSI color")
                }),
            16..=231 => {
                let n = index - 16;
                let c = |v| if v == 0 { 0 } else { 55 + 40 * v };
                self.theme_color(&format!("indexed.{index}"))
                    .unwrap_or(((c(n / 36) << 16) | (c(n / 6 % 6) << 8) | c(n % 6)) as u32)
            }
            232..=255 => self
                .theme_color(&format!("indexed.{index}"))
                .unwrap_or((8 + (index - 232) as u32 * 10) * 0x010101),
            256 => self
                .theme_color("foreground")
                .or_else(|| (self.env.foreground != 0).then_some(self.env.foreground))
                .or_else(|| user_color(self.settings.theme.foreground.as_deref()))
                .unwrap_or(self.env.foreground),
            257 => self
                .theme_color("background")
                .or_else(|| (self.env.background != 0).then_some(self.env.background))
                .or_else(|| user_color(self.settings.theme.background.as_deref()))
                .unwrap_or(self.env.background),
            258 => self
                .theme_color("cursor")
                .or_else(|| (self.env.accent != 0).then_some(self.env.accent))
                .or_else(|| user_color(self.settings.theme.cursor.as_deref()))
                .unwrap_or(if self.env.accent == 0 {
                    self.color(256)
                } else {
                    self.env.accent
                }),
            259..=266 => self
                .theme_color(&format!("ansi.dim_{}", ANSI_NAMES[index - 259]))
                .unwrap_or(if self.env.dark {
                    DARK_DIM[index - 259]
                } else {
                    LIGHT_DIM[index - 259]
                }),
            267 => self
                .theme_color("bright_foreground")
                .unwrap_or(self.color(256)),
            268 => {
                self.theme_color("dim_foreground")
                    .unwrap_or(if self.env.muted_foreground == 0 {
                        if self.env.dark { 0x9da0a8 } else { 0x6c707e }
                    } else {
                        self.env.muted_foreground
                    })
            }
            _ => self.color(256),
        }
    }

    /// Selection may be customized independently from the editor's base selection.
    pub(super) fn selection_color(&self) -> u32 {
        self.theme_color("selection")
            .or_else(|| (self.env.selection != 0).then_some(self.env.selection))
            .or_else(|| user_color(self.settings.theme.selection.as_deref()))
            .unwrap_or(self.env.selection)
    }

    /// A block cursor must not hide the glyph sitting under its accent fill.
    pub(super) fn cursor_text_color(&self) -> u32 {
        if let Some(color) = self.theme_color("cursor_text") {
            return color;
        }
        let cursor = self.color(258);
        let background = self.color(257);
        let foreground = self.color(256);
        if contrast(background, cursor) >= contrast(foreground, cursor) {
            background
        } else {
            foreground
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every built-in named ANSI color remains readable on its matching editor canvas.
    #[test]
    fn named_colors_have_readable_light_and_dark_contrast() {
        for (dark, background, dim) in [(false, 0xffffff, LIGHT_DIM), (true, 0x1e1f22, DARK_DIM)] {
            let palette =
                ANSI_NAMES.map(|name| bundled_color(dark, &format!("ansi.{name}")).unwrap());
            for color in palette.into_iter().chain(dim) {
                assert!(contrast(background, color) >= 4.5, "#{color:06x}");
            }
        }
    }
}

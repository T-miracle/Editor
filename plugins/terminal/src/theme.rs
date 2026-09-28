//! Plugin-owned terminal palette, resolved from editor theme tokens and user settings.
use super::Terminal;

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

// The light palette keeps every named foreground at readable contrast on Editor white.
const LIGHT_ANSI: [u32; 16] = [
    0x1f2329, 0xb42318, 0x1a7f46, 0x8a5a00, 0x175cd3, 0x8250df, 0x096d83, 0x6c707e, 0x59616e,
    0xc0322b, 0x167c42, 0x795100, 0x1d64d8, 0x7a3ecc, 0x086e80, 0x1f2329,
];
// The dark palette uses the editor's #1e1f22 canvas and #dfe1e5 text as anchors.
const DARK_ANSI: [u32; 16] = [
    0x89919b, 0xff7673, 0x82c991, 0xd7ba7d, 0x7aa5f8, 0xc9a7e8, 0x71c6d7, 0xdfe1e5, 0xa6a9b1,
    0xff8a87, 0x96d6a4, 0xe2c58d, 0x92b7ff, 0xd8b7f0, 0x8fd3df, 0xf7f8fa,
];
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
    /// Read only this plugin's color tokens from the editor's generic theme API.
    fn theme_color(&self, name: &str) -> Option<u32> {
        self.env
            .theme_colors
            .get(&format!("me.terminal.{name}"))
            .copied()
    }

    /// User settings win over theme tokens, then the plugin's light/dark defaults.
    pub(super) fn color(&self, index: usize) -> u32 {
        let ansi = if self.env.dark {
            &DARK_ANSI
        } else {
            &LIGHT_ANSI
        };
        match index {
            0..=15 => user_color(
                self.settings
                    .theme
                    .ansi
                    .as_ref()
                    .map(|palette| palette[index].as_str()),
            )
            .or_else(|| self.theme_color(&format!("ansi.{}", ANSI_NAMES[index])))
            .unwrap_or(ansi[index]),
            16..=231 => {
                let n = index - 16;
                let c = |v| if v == 0 { 0 } else { 55 + 40 * v };
                self.theme_color(&format!("indexed.{index}"))
                    .unwrap_or(((c(n / 36) << 16) | (c(n / 6 % 6) << 8) | c(n % 6)) as u32)
            }
            232..=255 => self
                .theme_color(&format!("indexed.{index}"))
                .unwrap_or((8 + (index - 232) as u32 * 10) * 0x010101),
            256 => user_color(self.settings.theme.foreground.as_deref())
                .or_else(|| self.theme_color("foreground"))
                .unwrap_or(self.env.foreground),
            257 => user_color(self.settings.theme.background.as_deref())
                .or_else(|| self.theme_color("background"))
                .unwrap_or(self.env.background),
            258 => user_color(self.settings.theme.cursor.as_deref())
                .or_else(|| self.theme_color("cursor"))
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
        user_color(self.settings.theme.selection.as_deref())
            .or_else(|| self.theme_color("selection"))
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
        for (background, palette, dim) in [
            (0xffffff, LIGHT_ANSI, LIGHT_DIM),
            (0x1e1f22, DARK_ANSI, DARK_DIM),
        ] {
            for color in palette.into_iter().chain(dim) {
                assert!(contrast(background, color) >= 4.5, "#{color:06x}");
            }
        }
    }
}

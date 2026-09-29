//! Resolve every visible role against the live editor theme, never against plugin literals.
use super::*;
use gpui_kit::{FontWeight, Hsla, Styled, component::ActiveTheme as _, px, rgb};

pub(super) struct Colors {
    pub background: Hsla,
    pub foreground: Hsla,
    pub border: Hsla,
    pub hover: Hsla,
    pub active: Hsla,
    pub accent: Hsla,
    pub accent_foreground: Hsla,
}

impl PluginView {
    pub(super) fn colors(&self, role: &str, cx: &App) -> Colors {
        let palette = cx.theme();
        let color = |key: &str, fallback: Hsla| {
            self.environment
                .color(&self.plugin, &format!("ui.{role}.{key}"))
                .map(|c| rgb(c).into())
                .unwrap_or(fallback)
        };
        Colors {
            background: color(
                "background",
                if role == "button" {
                    palette.button
                } else {
                    palette.background
                },
            ),
            foreground: color(
                "foreground",
                if role == "button" {
                    palette.button_foreground
                } else {
                    palette.foreground
                },
            ),
            border: color(
                "border",
                if role == "input" {
                    palette.input
                } else {
                    palette.border
                },
            ),
            hover: color(
                "hover_background",
                if role == "button" {
                    palette.button_hover
                } else {
                    palette.list_hover
                },
            ),
            active: color(
                "active_background",
                if role == "button" {
                    palette.button_active
                } else {
                    palette.list_active
                },
            ),
            accent: color("accent", palette.primary),
            accent_foreground: color("accent_foreground", palette.primary_foreground),
        }
    }

    pub(super) fn font<T: Styled>(&self, element: T, role: &str) -> T {
        let font = self.environment.font_style(&self.plugin, role, false);
        element
            .font_family(font.family.unwrap_or_else(|| "Segoe UI".into()))
            .text_size(px(font.size_px.unwrap_or(14.)))
            .font_weight(if font.bold.unwrap_or(false) {
                FontWeight::BOLD
            } else {
                FontWeight::NORMAL
            })
    }
}

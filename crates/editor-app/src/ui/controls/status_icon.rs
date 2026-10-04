//! Shared warning/error artwork uses the active palette in tabs, logs and status summaries.
use super::Icon;
use gpui_kit::{App, Hsla, Styled, component::ActiveTheme as _};

/// Informational records have no alert icon; both anomaly levels retain their own color after viewing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StatusIcon {
    Warning,
    Error,
}
impl StatusIcon {
    /// Return theme colors instead of embedding a light-theme color in SVG data.
    pub(crate) fn color(self, cx: &App) -> Hsla {
        match self {
            Self::Warning => cx.theme().warning,
            Self::Error => cx.theme().danger,
        }
    }

    /// Reuse the same local shape and color wherever an unread severity is presented.
    pub(crate) fn icon(self, cx: &App) -> Icon {
        let svg: &[u8] = match self {
            Self::Warning => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10.3 3.9 2.2 18a2 2 0 0 0 1.7 3h16.2a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0Z"/><path d="M12 9v4m0 4h.01"/></svg>"#,
            Self::Error => br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="9"/><path d="m9 9 6 6m0-6-6 6"/></svg>"#,
        };
        Icon::default().data(svg).small().text_color(self.color(cx))
    }
}

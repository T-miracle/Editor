//! Scrollbar paint states from the editor theme over gpui-base scrolling.

use gpui_base::{Scrollbar, ScrollbarHandle, ScrollbarStyles};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{App, Global, Hsla, Pixels, Subscription, component::Theme, px};
use plugin_schema::ThemeComponent;
use std::time::Duration;

use crate::theme::component_styles;

#[cfg(test)]
mod tests;

/// Match the tree's four-pixel padding when an editor or canvas viewport reaches its border.
const VIEWPORT_EDGE_INSET: Pixels = px(4.);

/// Shared paint states account for the viewport's existing padding without doubling the edge gap.
fn scrollbar_styles(normal: Hsla, hover: Hsla, active: Hsla, inset: Pixels) -> ScrollbarStyles {
    // Include the clearance in the hit target so the entire expanded thumb remains draggable.
    let track_width = px(8.) + inset;
    ScrollbarStyles::default()
        // Keep the hit target fixed while the thumb expands on hover or drag.
        .track(|track| track.width(track_width))
        .track_hover(|track| track.width(track_width))
        .track_active(|track| track.width(track_width))
        .thumb(|thumb| {
            thumb
                .bg(normal.opacity(0.55))
                .width(px(6.))
                .inset(inset)
                .radius(px(3.))
        })
        .thumb_hover(|thumb| {
            thumb
                .bg(hover.opacity(0.7))
                .width(px(8.))
                .inset(inset)
                .radius(px(4.))
        })
        .thumb_active(|thumb| {
            thumb
                .bg(active.opacity(0.8))
                .width(px(8.))
                .inset(inset)
                .radius(px(4.))
        })
}

/// Hold one observer so component-theme synchronization cannot reset the embedded editor's skin.
struct ScrollbarProjection {
    _subscription: Subscription,
}

impl Global for ScrollbarProjection {}

/// Install after runtime colors are published; refresh the projection after each theme update.
pub(crate) fn install_scrollbar_theme(cx: &mut App) {
    if !cx.has_global::<ScrollbarProjection>() {
        let subscription = cx.observe_global::<Theme>(sync_scrollbar_theme);
        cx.set_global(ScrollbarProjection {
            _subscription: subscription,
        });
    }
    sync_scrollbar_theme(cx);
}

/// Keep Base's native animation, with one shared two-second visibility hold and local paint.
fn sync_scrollbar_theme(cx: &mut App) {
    let styles = resolved_scrollbar_styles(cx, VIEWPORT_EDGE_INSET);
    let theme = gpui_base::Theme::global_mut(cx);
    let scrollbar = theme.scrollbar.clone();
    let motion = scrollbar.motion().with_idle(Duration::from_secs(2));
    theme.scrollbar = scrollbar.with_motion(motion).with_styles(styles);
}

/// Resolve the same colors and opacity for standalone controls and native editor scrollbars.
fn resolved_scrollbar_styles(cx: &App, inset: Pixels) -> ScrollbarStyles {
    let palette = cx.theme();
    let styles = component_styles(cx, ThemeComponent::Scrollbar);
    scrollbar_styles(
        styles.base.background.unwrap_or(palette.primary),
        styles.hover.background.unwrap_or(palette.primary),
        styles.active.background.unwrap_or(palette.primary),
        inset,
    )
}

/// Trees/lists already inset their scroll-handle bounds; retain that existing edge clearance.
pub(crate) fn vertical_scrollbar<H: ScrollbarHandle + Clone>(handle: &H, cx: &App) -> Scrollbar {
    Scrollbar::vertical(handle).styles(|_| resolved_scrollbar_styles(cx, px(0.)))
}

/// Canvas viewports need the same edge clearance as the embedded editor's global projection.
pub(crate) fn vertical_viewport_scrollbar<H: ScrollbarHandle + Clone>(
    handle: &H,
    cx: &App,
) -> Scrollbar {
    Scrollbar::vertical(handle)
        .viewport_from_layout()
        .styles(|_| resolved_scrollbar_styles(cx, VIEWPORT_EDGE_INSET))
}

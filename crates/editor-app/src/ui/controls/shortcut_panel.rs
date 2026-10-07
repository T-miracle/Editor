//! Compact shortcut surfaces; Base owns modal focus and dismissal, the editor owns appearance.

use std::rc::Rc;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    AnyElement, App, ElementId, FocusHandle, InteractiveElement, IntoElement, ParentElement,
    ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};

/// Render a centered, titleless shortcut modal within the current window.
///
/// `focus` identifies the modal focus trap; `content` supplies fixed header/footer and a scrolling
/// list. `cancel` handles Escape and backdrop presses, returning false to veto dismissal. The
/// caller owns removing the modal, staged changes, and restoration of the original panel focus.
pub(crate) fn shortcut_modal(
    focus: FocusHandle,
    content: AnyElement,
    cancel: impl Fn(&mut Window, &mut App) -> bool + 'static,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let viewport = window.viewport_size();
    // Fixed dimensions prevent filtering from moving the search field or footer. Leave a margin
    // on small windows without imposing a minimum size larger than the available viewport.
    let width = px(760.).min((viewport.width - px(32.)).max(px(0.)));
    let height = px(560.).min((viewport.height - px(32.)).max(px(0.)));
    let palette = cx.theme();
    let background = palette.popover;
    let foreground = palette.popover_foreground;
    let border = palette.border;
    let radius = palette.radius_lg;
    // Overlay tokens already carry alpha. Set the mask's intended coverage rather than
    // multiplying a subtle popover token into an almost invisible background veil.
    let mask = palette.overlay.alpha(0.55);

    gpui_base::Dialog::new(cx)
        .focus_handle(focus)
        .close_on_backdrop_press(true)
        .on_cancel(move |_, window, cx| cancel(window, cx))
        // Enter belongs to search or explicit row controls; it never dismisses this manager.
        .on_ok(|_, _, _| false)
        .backdrop(gpui_base::DialogBackdrop::new().size_full().bg(mask))
        .popup(
            gpui_base::DialogPopup::new()
                .w(width)
                .h(height)
                .flex_shrink_0()
                .rounded(radius)
                .border_1()
                .border_color(border)
                .bg(background)
                .text_color(foreground)
                .shadow_lg()
                .overflow_hidden()
                .child(
                    div()
                        .debug_selector(|| "shortcuts-panel".into())
                        .size_full()
                        .flex()
                        .flex_col()
                        .min_h_0()
                        .min_w_0()
                        .child(content),
                ),
        )
        .into_any_element()
}

/// Render display-form strokes (for example `Ctrl+K`, `Ctrl+C`) as compact keycaps.
///
/// Each slice item represents one step; steps are separated by an arrow, while plus-separated
/// keys within a step receive individual caps. An empty slice produces an empty element so the
/// caller can supply a localized unbound label. This display helper never parses bindings.
pub(crate) fn shortcut_keycaps(strokes: &[String], cx: &App) -> AnyElement {
    let palette = cx.theme();
    let mut row = div()
        .flex()
        .items_center()
        .flex_shrink_0()
        .gap(px(4.))
        .text_sm()
        .text_color(palette.muted_foreground);
    for (step, stroke) in strokes.iter().enumerate() {
        if step > 0 {
            row = row.child("→");
        }
        // A trailing '+' is the literal plus key, not an empty visual keycap.
        let literal_plus = stroke.ends_with('+');
        let keys = stroke.split('+').filter(|key| !key.is_empty());
        for (index, key) in keys.chain(literal_plus.then_some("+")).enumerate() {
            if index > 0 {
                row = row.child("+");
            }
            row = row.child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .h(px(24.))
                    .min_w(px(24.))
                    .px(px(6.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(palette.border)
                    .bg(palette.secondary)
                    .text_color(palette.foreground)
                    .child(key.to_owned()),
            );
        }
    }
    row.into_any_element()
}

/// Render the fixed, equal-width panel/global tabs using localized labels in that order.
///
/// The owner supplies selection and a dedicated strip focus handle. Pointer and focused arrow
/// navigation request a change through `on_change`; the owner may defer it for unsaved edits.
/// Modal-wide Alt+arrow navigation belongs to the owner so recording can take precedence.
pub(crate) fn shortcut_tabs(
    selected: usize,
    labels: [SharedString; 2],
    focus: &FocusHandle,
    on_change: impl Fn(usize, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let palette = cx.theme();
    let on_change = Rc::new(on_change);
    let keyboard_change = on_change.clone();
    let mut tabs = gpui_base::Tabs::new("shortcuts-tabs")
        .debug_selector(|| "shortcuts-tabs".into())
        .track_focus(focus)
        .tab_stop(true)
        .flex()
        .flex_shrink_0()
        .w_full()
        .h(px(42.))
        .text_sm()
        .border_b_1()
        .border_color(palette.border)
        .in_focus(|style| style.border_color(palette.ring))
        .on_key_down(move |event, window, cx| {
            if event.keystroke.modifiers != Default::default() {
                return;
            }
            let next = match event.keystroke.key.as_str() {
                "left" | "right" => 1 - selected.min(1),
                "home" => 0,
                "end" => 1,
                _ => return,
            };
            cx.stop_propagation();
            keyboard_change(next, window, cx);
        });
    for (index, label) in labels.into_iter().enumerate() {
        let active = index == selected;
        let change = on_change.clone();
        let focus = focus.clone();
        tabs = tabs.child(
            gpui_base::Tab::new(format!("shortcuts-tab-{index}"))
                .debug_selector(move || format!("shortcuts-tab-{index}").into())
                .selected(active)
                .set_position(index + 1, 2)
                .accessibility_label(label.clone())
                .flex_1()
                .min_w_0()
                .h_full()
                .px_2()
                .border_b_2()
                .border_color(if active {
                    palette.primary
                } else {
                    palette.transparent
                })
                .bg(if active {
                    palette.list_active
                } else {
                    palette.popover
                })
                .text_color(if active {
                    palette.foreground
                } else {
                    palette.muted_foreground
                })
                .hover(|style| style.bg(palette.list_hover))
                .on_click(move |_, window, cx| {
                    focus.focus(window, cx);
                    change(index, window, cx);
                })
                .child(div().min_w_0().truncate().child(label)),
        );
    }
    tabs.into_any_element()
}

/// Keep the search field and recording toggle aligned above the scrolling list.
///
/// `field` owns text or key capture and `toggle` owns activation/accessibility. Recording adds an
/// accent outline without resizing this region; both children retain their Base input/button behavior.
pub(crate) fn shortcut_search(
    field: AnyElement,
    toggle: AnyElement,
    recording: bool,
    cx: &App,
) -> AnyElement {
    let palette = cx.theme();
    div()
        .debug_selector(|| "shortcuts-search-region".into())
        .flex()
        .items_center()
        .flex_shrink_0()
        .w_full()
        .p(px(16.))
        .gap(px(8.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .rounded(palette.radius)
                .border_1()
                .border_color(if recording {
                    palette.ring
                } else {
                    palette.border
                })
                .bg(palette.background)
                .in_focus(|style| style.border_color(palette.ring))
                .child(field),
        )
        .child(div().flex().items_center().flex_shrink_0().child(toggle))
        .into_any_element()
}

/// Place caller-owned rows or a localized empty-state element in the only scrolling region.
///
/// The retained `scroll` handle keeps position across renders; the owner resets it when changing
/// filters. Base's locally themed scrollbar provides native wheel and thumb interaction.
pub(crate) fn shortcut_list(rows: Vec<AnyElement>, scroll: &ScrollHandle, cx: &App) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .mx(px(16.))
        .mb(px(12.))
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .overflow_hidden()
        .child(
            div()
                .id("shortcuts-list")
                .debug_selector(|| "shortcuts-list".into())
                .relative()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(scroll)
                .children(rows)
                .child(super::vertical_scrollbar(scroll, cx)),
        )
        .into_any_element()
}

/// Lay out an operation description on the left and its binding controls on the right.
///
/// `id` stays stable across filtering. Supplying `editing` expands caller-owned capture, errors,
/// and save/cancel controls under this row, with a selected background and accent edge. The row
/// itself does not execute commands; buttons and inputs retain their own interaction semantics.
pub(crate) fn shortcut_row(
    id: impl Into<ElementId>,
    description: AnyElement,
    binding: AnyElement,
    editing: Option<AnyElement>,
    cx: &App,
) -> AnyElement {
    let palette = cx.theme();
    let active = editing.is_some();
    div()
        .id(id)
        .relative()
        .flex()
        .flex_col()
        .flex_shrink_0()
        .w_full()
        .text_sm()
        .border_b_1()
        .border_color(palette.border)
        .bg(if active {
            palette.list_active
        } else {
            palette.popover
        })
        .when(active, |row| {
            // A separate accent preserves the neutral separator and does not shift row content.
            row.child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .bottom_0()
                    .w(px(2.))
                    .bg(palette.primary),
            )
        })
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .min_h(px(38.))
                .px(px(12.))
                .py(px(5.))
                .gap(px(12.))
                .child(div().flex_1().min_w_0().child(description))
                .child(div().flex().items_center().flex_shrink_0().child(binding)),
        )
        .when_some(editing, |row, editing| {
            row.child(div().px(px(12.)).pb(px(10.)).child(editing))
        })
        .into_any_element()
}

/// Anchor context-sensitive Escape help at the left and tab-navigation hints at the right.
///
/// Both children are supplied by the owner so localized labels match the current recording state;
/// this footer always remains outside the list's scroll bounds.
pub(crate) fn shortcut_footer(leading: AnyElement, navigation: AnyElement, cx: &App) -> AnyElement {
    div()
        .debug_selector(|| "shortcuts-footer".into())
        .flex()
        .items_center()
        .justify_between()
        .flex_shrink_0()
        .w_full()
        .min_h(px(44.))
        .px(px(16.))
        .py(px(8.))
        .gap(px(12.))
        .border_t_1()
        .border_color(cx.theme().border)
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(div().min_w_0().child(leading))
        .child(div().flex().items_center().justify_end().child(navigation))
        .into_any_element()
}

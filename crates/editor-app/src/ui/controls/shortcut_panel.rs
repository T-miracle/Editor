//! Compact shortcut surfaces; Base owns modal focus and dismissal, the editor owns appearance.

use std::rc::Rc;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    AnyElement, App, Div, ElementId, FocusHandle, InteractiveElement, IntoElement, ParentElement,
    ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder as _, px, relative,
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
    // C's taller proportion leaves space for the expanded row and its four explicit actions.
    let height = px(680.).min((viewport.height - px(32.)).max(px(0.)));
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
/// The returned div wraps by default; a fixed-height search viewport can override it to nowrap.
pub(crate) fn shortcut_keycaps(strokes: &[String], cx: &App) -> Div {
    let palette = cx.theme();
    let mut row = div()
        .flex()
        .items_center()
        .min_w_0()
        .flex_wrap()
        .gap(px(6.))
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
                    .flex_shrink_0()
                    .items_center()
                    .justify_center()
                    .h(px(28.))
                    .min_w(px(if key.len() > 1 { 40. } else { 30. }))
                    .px(px(8.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(palette.border)
                    .bg(palette.button)
                    .text_color(palette.foreground)
                    .child(key.to_owned()),
            );
        }
    }
    row
}

/// Display a window-local next-step hint without adding a focusable command or click action.
/// Each tuple contains an operation description and its already formatted display strokes.
pub(crate) fn shortcut_pending_hint(
    title: String,
    steps: Vec<(String, Vec<String>)>,
    cx: &App,
) -> AnyElement {
    let palette = cx.theme();
    div()
        .debug_selector(|| "shortcuts-pending".into())
        .absolute()
        .right(px(16.))
        .bottom(px(36.))
        .max_w(px(480.))
        .p_3()
        .rounded(palette.radius_lg)
        .border_1()
        .border_color(palette.border)
        .shadow_lg()
        .bg(palette.popover)
        .text_color(palette.popover_foreground)
        .flex()
        .flex_col()
        .gap_2()
        .text_sm()
        .child(title)
        .children(steps.into_iter().map(|(description, strokes)| {
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_4()
                .child(description)
                .child(shortcut_keycaps(&strokes, cx))
        }))
        .into_any_element()
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
        .h(px(50.))
        .text_base()
        .rounded_tl(palette.radius_lg)
        .rounded_tr(palette.radius_lg)
        .border_b_1()
        .border_color(palette.border)
        // Only keyboard focus emphasizes the whole strip; browsing retains the active underline.
        .focus_visible(|style| style.border_color(palette.ring))
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
                // GPUI overflow clips the rectangular bounds, not a child's painted background.
                // Round the two outer tabs themselves; the middle seam and underline stay straight.
                .when(index == 0, |tab| tab.rounded_tl(palette.radius_lg))
                .when(index == 1, |tab| tab.rounded_tr(palette.radius_lg))
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
        .px(px(20.))
        .py(px(18.))
        .gap(px(12.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .flex_1()
                .min_w_0()
                .h(px(40.))
                .px(px(12.))
                .rounded(palette.radius)
                .border_1()
                .border_color(if recording {
                    palette.ring
                } else {
                    palette.border
                })
                .bg(palette.button)
                .in_focus(|style| style.border_color(palette.ring))
                .child(
                    super::Icon::default()
                        .path("icons/search.svg")
                        .text_color(palette.muted_foreground),
                )
                .child(div().flex_1().min_w_0().child(field)),
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
        .mx(px(20.))
        .mb(px(16.))
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border.opacity(0.7))
        .bg(cx.theme().popover)
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
/// `id` stays stable across filtering. In an expanded row, `binding` supplies the right-column
/// capture and `editing` supplies errors and four actions below, with a selected background. The row
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
        .group("shortcut-row")
        .relative()
        .flex()
        .flex_col()
        .flex_shrink_0()
        .w_full()
        .text_base()
        .when(!active, |row| row.border_b_1())
        .border_color(palette.border)
        .bg(if active {
            palette.list_hover
        } else {
            palette.popover
        })
        .when(active, |row| {
            // A separate accent preserves the neutral separator and does not shift row content.
            row.rounded(palette.radius).border_1().child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(10.))
                    .bottom(px(10.))
                    .w(px(2.))
                    .bg(palette.primary),
            )
        })
        .child(
            div()
                .flex()
                .items_center()
                .min_h(px(44.))
                .px(px(16.))
                .py(px(if active { 12. } else { 6. }))
                .gap(px(16.))
                .child(div().flex_1().min_w_0().child(description))
                // A bounded column aligns every keycap origin and still shrinks in small windows.
                .child(
                    div()
                        .w(px(288.))
                        .max_w(relative(0.58))
                        .min_w_0()
                        .child(binding),
                ),
        )
        .when_some(editing, |row, editing| {
            row.child(div().px(px(16.)).pb(px(12.)).child(editing))
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
        .min_h(px(54.))
        .px(px(20.))
        .py(px(12.))
        .gap(px(12.))
        .border_t_1()
        .border_color(cx.theme().border)
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(div().min_w_0().child(leading))
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .min_w_0()
                .child(navigation),
        )
        .into_any_element()
}

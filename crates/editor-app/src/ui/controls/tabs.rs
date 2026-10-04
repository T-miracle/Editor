//! Editor-owned underline tabs; gpui-base supplies pointer and accessibility semantics.
//! Compound keyboard navigation stays here so callers cannot activate disabled placeholders.
use super::StatusIcon;
use std::rc::Rc;

use gpui_base::{Tab, Tabs};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::{
    App, ElementId, FocusHandle, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder as _, px,
};

/// Render a controlled strip of localized labels and disabled flags.
/// The owner supplies selection and focus; changes report an enabled index only.
/// Returns the local underline appearance over Base's tab/list semantics.
pub(crate) fn tab_strip<const N: usize>(
    id: &'static str,
    selected: usize,
    labels: [(SharedString, bool); N],
    badges: [Option<StatusIcon>; N],
    focus: &FocusHandle,
    on_change: impl Fn(usize, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let palette = cx.theme();
    let on_change = Rc::new(on_change);
    let enabled: Vec<_> = labels
        .iter()
        .enumerate()
        .filter_map(|(index, (_, disabled))| (!disabled).then_some(index))
        .collect();
    let keyboard_change = on_change.clone();
    let mut tabs = Tabs::new(id)
        .track_focus(focus)
        .tab_stop(true)
        .flex()
        .flex_shrink_0()
        .w_full()
        .border_b_1()
        .border_color(palette.border)
        // A focused strip keeps a full-width accent, identifying which group owns arrow keys.
        .in_focus(|style| style.border_color(palette.ring))
        .on_key_down(move |event, window, cx| {
            if event.keystroke.modifiers != Default::default() || enabled.is_empty() {
                return;
            }
            // Navigation wraps between available tabs, never through marketplace placeholders.
            let current = enabled
                .iter()
                .position(|index| *index == selected)
                .unwrap_or(0);
            let next = match event.keystroke.key.as_str() {
                "right" => enabled[(current + 1) % enabled.len()],
                "left" => enabled[(current + enabled.len() - 1) % enabled.len()],
                "home" => enabled[0],
                "end" => enabled[enabled.len() - 1],
                "enter" | "space" => enabled[current],
                _ => return,
            };
            cx.stop_propagation();
            keyboard_change(next, window, cx);
        });
    for (index, (label, disabled)) in labels.into_iter().enumerate() {
        let handler = on_change.clone();
        let focus = focus.clone();
        let tooltip_label = label.clone();
        tabs = tabs.child(
            Tab::new(ElementId::Name(format!("{id}-{index}").into()))
                .debug_selector(move || format!("{id}-{index}").into())
                .selected(index == selected)
                .disabled(disabled)
                .accessibility_label(label.clone())
                .tooltip(move |window, cx| Tooltip::new(tooltip_label.clone()).build(window, cx))
                .set_position(index + 1, N)
                .when(!disabled, |tab| {
                    tab.on_click(move |_, window, cx| {
                        focus.focus(window, cx);
                        handler(index, window, cx);
                    })
                })
                .h(px(32.))
                .flex_1()
                .min_w(px(0.))
                .px_2()
                .border_b_2()
                .border_color(if index == selected {
                    palette.primary
                } else {
                    palette.transparent
                })
                .bg(palette.sidebar)
                .text_color(if index == selected {
                    palette.foreground
                } else {
                    palette.muted_foreground
                })
                .when(disabled, |tab| tab.opacity(0.45))
                .when(!disabled, |tab| {
                    tab.hover(|style| style.bg(palette.list_hover))
                })
                .child(
                    // Severity and title share one line without allowing a long label to squash its icon.
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap_1()
                        .min_w(px(0.))
                        .when_some(badges[index], |row, badge| {
                            row.child(
                                div()
                                    .debug_selector(move || {
                                        format!("{id}-{index}-{:?}", badge).into()
                                    })
                                    .flex_shrink_0()
                                    .child(badge.icon(cx)),
                            )
                        })
                        .child(div().min_w(px(0.)).truncate().child(label)),
                ),
        );
    }
    tabs
}

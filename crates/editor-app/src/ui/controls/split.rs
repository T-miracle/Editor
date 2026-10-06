//! Adjustable native panes keep Base capture/state while the local UI supplies divider appearance.
use gpui_kit::component::ActiveTheme;
use gpui_kit::{
    AnyElement, App, InteractiveElement, IntoElement, ParentElement, SharedString, Styled, div,
    prelude::FluentBuilder as _, px,
};
use std::rc::Rc;

/// Render a bounded portable Row/Column as native resizable panels with one shared local divider.
pub(crate) fn split_container(
    id: SharedString,
    vertical: bool,
    children: Vec<AnyElement>,
    _: &App,
) -> AnyElement {
    let group = if vertical {
        gpui_base::v_resizable(id.clone())
    } else {
        gpui_base::h_resizable(id.clone())
    };
    let mut group = group.with_handle_appearance(Rc::new(move |_, _, cx| {
        let selector = format!("plugin-split-divider-{id}");
        Some(
            div()
                .debug_selector(move || selector.clone().into())
                .bg(cx.theme().border)
                .when(vertical, |line| line.h(px(1.)).w_full())
                .when(!vertical, |line| line.w(px(1.)).h_full())
                .into_any_element(),
        )
    }));
    for child in children {
        group = group.child(
            gpui_base::resizable_panel()
                .size_range(px(100.)..gpui_kit::Pixels::MAX)
                .child(child),
        );
    }
    group.into_any_element()
}

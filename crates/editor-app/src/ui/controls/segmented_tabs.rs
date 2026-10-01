//! Editor-owned segmented tabs, following GPUI Kit's inset active-surface appearance.
//! Reference: https://github.com/longbridge/gpui-kit/tree/main/crates/component/src/tab

use std::rc::Rc;

use gpui_base::{Tab, Tabs};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    App, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    Styled, Window, prelude::FluentBuilder as _, px,
};

type ChangeHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// Controlled, equal-width options; gpui-base provides tab selection and accessibility semantics.
#[derive(IntoElement)]
pub(crate) struct SegmentedTabs {
    id: ElementId,
    labels: Vec<SharedString>,
    selected: usize,
    disabled: bool,
    on_change: Option<ChangeHandler>,
}

impl SegmentedTabs {
    /// Create a group whose width is supplied by its parent layout.
    pub(crate) fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            labels: Vec::new(),
            selected: 0,
            disabled: false,
            on_change: None,
        }
    }

    /// Supply visible labels in the same order as the indices reported by the callback.
    pub(crate) fn labels(
        mut self,
        labels: impl IntoIterator<Item = impl Into<SharedString>>,
    ) -> Self {
        self.labels = labels.into_iter().map(Into::into).collect();
        self
    }

    /// Reflect the owner's current selection without storing a second copy of its state.
    pub(crate) fn selected_index(mut self, selected: usize) -> Self {
        self.selected = selected;
        self
    }

    /// Prevent every option from activating while its owner is completing an operation.
    pub(crate) fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Notify the owner when an option is activated; the owner applies the resulting selection.
    pub(crate) fn on_change(
        mut self,
        handler: impl Fn(usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for SegmentedTabs {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = cx.theme();
        let count = self.labels.len();
        // Match the upstream structure: a 32px track around a 24px inset selected surface.
        let mut tabs = Tabs::new(self.id)
            .flex()
            .items_center()
            .w_full()
            .h(px(32.))
            .px(px(4.))
            .gap(px(2.))
            .rounded(px(8.))
            .bg(palette.tab_bar_segmented)
            .when(self.disabled, |tabs| tabs.opacity(0.5));
        for (index, label) in self.labels.into_iter().enumerate() {
            let selected = index == self.selected;
            let handler = self.on_change.clone();
            tabs = tabs.child(
                Tab::new(format!("segment-{index}"))
                    .selected(selected)
                    .disabled(self.disabled)
                    .accessibility_label(label.clone())
                    .set_position(index + 1, count)
                    .flex()
                    .items_center()
                    .justify_center()
                    .flex_1()
                    .min_w(px(0.))
                    .h(px(24.))
                    .px_2()
                    .rounded(px(6.))
                    .text_color(if selected {
                        palette.foreground
                    } else {
                        palette.muted_foreground
                    })
                    .bg(if selected {
                        palette.background
                    } else {
                        palette.transparent
                    })
                    .when(selected, |tab| tab.shadow_sm())
                    .hover(|style| {
                        style.text_color(palette.foreground).bg(if selected {
                            palette.background
                        } else {
                            palette.list_hover
                        })
                    })
                    .when_some(handler, |tab, handler| {
                        tab.on_click(move |_, window, cx| handler(index, window, cx))
                    })
                    .child(label),
            );
        }
        tabs
    }
}

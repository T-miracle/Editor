//! Reusable in-window modal with a draggable title bar and caller-owned body.

use gpui_kit::{
    App, AppContext as _, Context, Entity, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point, Render,
    SharedString, StatefulInteractiveElement, Styled, Window,
    component::{
        ActiveTheme, IconName, Sizable, StyledExt,
        button::{Button, ButtonVariants as _},
        dialog::DialogContent,
        h_flex,
        scroll::ScrollableElement,
        v_flex,
    },
    div, point,
    prelude::FluentBuilder,
    px,
};
use std::rc::Rc;

type ContentBuilder = Rc<dyn Fn(DialogContent, &mut Window, &mut App) -> DialogContent>;

/// A modal panel whose title and content are supplied by its caller.
pub struct AppDialog {
    title: SharedString,
    content: ContentBuilder,
    open: bool,
    offset: Point<Pixels>,
    drag_start: Option<(Point<Pixels>, Point<Pixels>)>,
}

impl AppDialog {
    fn new(title: SharedString, content: ContentBuilder) -> Self {
        Self {
            title,
            content,
            open: true,
            offset: point(px(0.), px(0.)),
            drag_start: None,
        }
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        self.drag_start = None;
        cx.notify();
    }

    fn start_drag(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.drag_start = Some((event.position, self.offset));
        cx.notify();
    }

    fn drag(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some((start, original)) = self.drag_start else {
            return;
        };
        let viewport = window.viewport_size();
        let width = px(560.).min((viewport.width - px(32.)).max(px(0.)));
        let centered_x = (viewport.width - width) / 2.;
        let centered_y = viewport.height / 10.;
        self.offset = point(
            (original.x + event.position.x - start.x)
                .max(px(16.) - centered_x)
                .min(viewport.width - width - px(16.) - centered_x),
            (original.y + event.position.y - start.y)
                .max(px(16.) - centered_y)
                .min(viewport.height - px(48.) - centered_y),
        );
        cx.notify();
    }

    fn stop_drag(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.drag_start = None;
        cx.notify();
    }
}

impl Render for AppDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        let width = px(560.).min((viewport.width - px(32.)).max(px(0.)));
        let x = (viewport.width - width) / 2. + self.offset.x;
        let y = viewport.height / 10. + self.offset.y;

        div().id("app-dialog-overlay").when(self.open, |overlay| {
            overlay
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .occlude()
                .bg(cx.theme().overlay)
                .on_mouse_move(cx.listener(Self::drag))
                .on_mouse_up(MouseButton::Left, cx.listener(Self::stop_drag))
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    v_flex()
                        .id("app-dialog-window")
                        .debug_selector(|| "dialog-0".into())
                        .absolute()
                        .left(x)
                        .top(y)
                        .w(width)
                        .max_h((viewport.height - y - px(16.)).max(px(0.)))
                        .rounded(cx.theme().radius_lg)
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().tokens.background)
                        .occlude()
                        .child(
                            h_flex()
                                .h(px(42.))
                                .w_full()
                                .flex_shrink_0()
                                .items_center()
                                .border_b_1()
                                .border_color(cx.theme().border)
                                .child(
                                    h_flex()
                                        .id("app-dialog-drag-region")
                                        .debug_selector(|| "app-dialog-drag-region".into())
                                        .flex_1()
                                        .h_full()
                                        .items_center()
                                        .px_4()
                                        .font_semibold()
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(Self::start_drag),
                                        )
                                        .child(self.title.clone()),
                                )
                                .child(
                                    Button::new("app-dialog-close")
                                        .icon(IconName::Close)
                                        .small()
                                        .ghost()
                                        .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                                )
                                .child(div().w(px(8.))),
                        )
                        .child(
                            v_flex()
                                .id("app-dialog-body")
                                .overflow_y_scrollbar()
                                .p_4()
                                .child((self.content)(DialogContent::new(), window, cx)),
                        ),
                )
        })
    }
}

/// Wraps a trigger and supplies a fresh dialog entity to the caller on click.
pub fn app_dialog(
    trigger: impl IntoElement,
    title: impl Into<SharedString>,
    content: impl Fn(DialogContent, &mut Window, &mut App) -> DialogContent + 'static,
    on_open: impl Fn(Entity<AppDialog>, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let title = title.into();
    let content: ContentBuilder = Rc::new(content);
    gpui_base::DialogTrigger::new(trigger).on_open(move |window, cx| {
        let dialog = cx.new(|_| AppDialog::new(title.clone(), Rc::clone(&content)));
        on_open(dialog, window, cx);
    })
}

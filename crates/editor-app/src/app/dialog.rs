//! Reusable native modal window with a caller-owned title and content region.

use gpui_kit::{
    AnyElement, App, AppContext as _, Bounds, Context, Entity, FocusHandle, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ParentElement, Render, SharedString, Styled, Window, WindowBounds, WindowControlArea,
    WindowHandle, WindowKind,
    component::{ActiveTheme, IconName, Root, StyledExt, TitleBar, WindowExt as _, h_flex, v_flex},
    div, px, size,
};
use std::rc::Rc;

use crate::{
    PANEL_HEADER_HEIGHT,
    ui::controls::{Button, DialogContent},
};

type ContentBuilder = Rc<dyn Fn(DialogContent, &mut Window, &mut App) -> DialogContent>;
type TitleBuilder = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;
/// Controlled content may cancel its innermost edit before allowing native chrome to close.
type CloseRequest = Rc<dyn Fn(&mut Window, &mut App) -> bool>;

/// Initial window width leaves room for the settings navigation and content.
const DIALOG_WIDTH: f32 = 760.;
/// Initial content height sits below the shared title bar.
const DIALOG_BODY_HEIGHT: f32 = 480.;

/// A native modal window whose title and content are supplied by its caller.
pub struct AppDialog {
    title: TitleBuilder,
    content: ContentBuilder,
    /// Keeps keyboard events inside the dialog so Escape can close it by default.
    focus: FocusHandle,
    /// Chrome focus stays separate from the content's focus trap and keyboard actions.
    close_focus: FocusHandle,
    should_move: bool,
    close_request: Option<CloseRequest>,
}

impl AppDialog {
    /// Stores the builders used whenever the modal window repaints.
    fn new(title: TitleBuilder, content: ContentBuilder, cx: &mut Context<Self>) -> Self {
        Self {
            title,
            content,
            focus: cx.focus_handle(),
            close_focus: cx.focus_handle(),
            should_move: false,
            close_request: None,
        }
    }

    /// Delegate native chrome dismissal to content with staged edits; `false` keeps the window open.
    /// The content then owns Escape through its gpui-base dialog, preserving child menu dismissal.
    pub(crate) fn set_close_request(
        &mut self,
        close: impl Fn(&mut Window, &mut App) -> bool + 'static,
    ) {
        self.close_request = Some(Rc::new(close));
    }

    /// Remove the native window only after its content has accepted dismissal.
    fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .close_request
            .as_ref()
            .is_none_or(|close| close(window, cx))
        {
            window.remove_window();
        }
    }

    /// Closes the native window on Escape, including when content owns keyboard focus.
    fn close_on_escape(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Controlled content handles Escape in the bubble/action phase, after its focused menu.
        if self.close_request.is_some() && !self.close_focus.is_focused(window) {
            return;
        }
        if event.keystroke.key == "escape" && event.keystroke.modifiers == Default::default() {
            // The active child Dialog owns Escape while its confirmation is open.
            if window.has_active_dialog(cx) {
                return;
            }
            cx.stop_propagation();
            self.request_close(window, cx);
        }
    }

    /// Arms system window movement only after a press in the title region.
    fn start_drag(&mut self, _: &MouseDownEvent, _: &mut Window, _: &mut Context<Self>) {
        self.should_move = true;
    }

    /// Hands dragging to the operating system, keeping movement outside the editor possible.
    fn move_window(&mut self, event: &MouseMoveEvent, window: &mut Window, _: &mut Context<Self>) {
        if !event.dragging() {
            self.should_move = false;
            return;
        }
        if self.should_move {
            self.should_move = false;
            window.start_window_move();
        }
    }

    /// Disarms movement when the pointer is released before a drag begins.
    fn stop_drag(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.should_move = false;
    }
}

impl Render for AppDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Native dialog chrome shares the title-bar slot, including its color alpha.
        let title_style =
            crate::theme::component_styles(cx, plugin_schema::ThemeComponent::WindowTitleBar).base;
        // GPUI Kit 0.7 Root automatically hosts overlays above the dialog content.
        div().relative().size_full().child(
            v_flex()
                .id("app-dialog-window")
                .debug_selector(|| "dialog-0".into())
                .size_full()
                .bg(cx.theme().tokens.background)
                .track_focus(&self.focus)
                .capture_key_down(cx.listener(Self::close_on_escape))
                .child(
                    h_flex()
                        .id("app-dialog-title-bar")
                        .debug_selector(|| "app-dialog-title-bar".into())
                        .h(px(PANEL_HEADER_HEIGHT))
                        .w_full()
                        .flex_shrink_0()
                        .items_center()
                        .bg(title_style.background.unwrap_or(cx.theme().title_bar))
                        .text_color(title_style.foreground.unwrap_or(cx.theme().foreground))
                        .border_b_1()
                        .border_color(title_style.border.unwrap_or(cx.theme().border))
                        .child(
                            h_flex()
                                .id("app-dialog-drag-region")
                                .flex_1()
                                .h_full()
                                .items_center()
                                .px_3()
                                .font_semibold()
                                .window_control_area(WindowControlArea::Drag)
                                .on_mouse_down(MouseButton::Left, cx.listener(Self::start_drag))
                                .on_mouse_up(MouseButton::Left, cx.listener(Self::stop_drag))
                                .on_mouse_move(cx.listener(Self::move_window))
                                .child((self.title)(window, cx)),
                        )
                        .child(
                            Button::new("app-dialog-close")
                                .debug_selector(|| "app-dialog-close".into())
                                .track_focus(&self.close_focus)
                                .icon(IconName::Close)
                                .small()
                                .compact()
                                .ghost()
                                .accessibility_label(rust_i18n::t!("notification.close"))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.request_close(window, cx);
                                })),
                        )
                        .child(div().w(px(8.))),
                )
                .child(
                    v_flex()
                        .id("app-dialog-body")
                        .flex_1()
                        .min_h_0()
                        .overflow_hidden()
                        .child((self.content)(DialogContent::new(), window, cx)),
                ),
        )
    }
}

/// Wraps a trigger and opens or activates one native modal window.
pub fn app_dialog(
    trigger: impl IntoElement,
    title: impl Into<SharedString>,
    content: impl Fn(DialogContent, &mut Window, &mut App) -> DialogContent + 'static,
    existing: impl Fn(&App) -> Option<WindowHandle<Root>> + 'static,
    on_open: impl Fn(Entity<AppDialog>, WindowHandle<Root>, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let title = title.into();
    let window_title = title.clone();
    let title: TitleBuilder = Rc::new(move |_, _| div().child(title.clone()).into_any_element());
    app_dialog_with_builders(
        trigger,
        window_title,
        title,
        Rc::new(content),
        existing,
        on_open,
    )
}

/// Opens the same native window with caller-rendered title content.
#[allow(dead_code)] // Other callers can use the custom title entry point later.
pub fn app_dialog_with_title(
    trigger: impl IntoElement,
    window_title: impl Into<SharedString>,
    title: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
    content: impl Fn(DialogContent, &mut Window, &mut App) -> DialogContent + 'static,
    existing: impl Fn(&App) -> Option<WindowHandle<Root>> + 'static,
    on_open: impl Fn(Entity<AppDialog>, WindowHandle<Root>, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    app_dialog_with_builders(
        trigger,
        window_title.into(),
        Rc::new(title),
        Rc::new(content),
        existing,
        on_open,
    )
}

/// Shares native window creation and duplicate-window handling across title variants.
fn app_dialog_with_builders(
    trigger: impl IntoElement,
    window_title: SharedString,
    title: TitleBuilder,
    content: ContentBuilder,
    existing: impl Fn(&App) -> Option<WindowHandle<Root>> + 'static,
    on_open: impl Fn(Entity<AppDialog>, WindowHandle<Root>, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    gpui_base::DialogTrigger::new(trigger).on_open(move |parent_window, cx| {
        if let Some(handle) = existing(cx)
            && handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            return;
        }

        let (dialog, handle) = open_dialog_with_builders(
            window_title.clone(),
            Rc::clone(&title),
            Rc::clone(&content),
            DIALOG_WIDTH,
            DIALOG_BODY_HEIGHT,
            cx,
        );
        on_open(dialog, handle, parent_window, cx);
    })
}

/// Opens shared modal chrome for commands that do not originate from a dialog trigger.
pub(crate) fn open_dialog(
    title: impl Into<SharedString>,
    content: impl Fn(DialogContent, &mut Window, &mut App) -> DialogContent + 'static,
    cx: &mut App,
) -> (Entity<AppDialog>, WindowHandle<Root>) {
    let title = title.into();
    let label = title.clone();
    open_dialog_with_builders(
        title,
        Rc::new(move |_, _| div().child(label.clone()).into_any_element()),
        Rc::new(content),
        DIALOG_WIDTH,
        DIALOG_BODY_HEIGHT,
        cx,
    )
}

/// Opens shared modal chrome with caller-selected width and content height in logical pixels.
pub(crate) fn open_dialog_sized(
    title: impl Into<SharedString>,
    width: f32,
    body_height: f32,
    content: impl Fn(DialogContent, &mut Window, &mut App) -> DialogContent + 'static,
    cx: &mut App,
) -> (Entity<AppDialog>, WindowHandle<Root>) {
    let title = title.into();
    let label = title.clone();
    open_dialog_with_builders(
        title,
        Rc::new(move |_, _| div().child(label.clone()).into_any_element()),
        Rc::new(content),
        width,
        body_height,
        cx,
    )
}

/// Keeps sizing, focus, Escape handling and native modality identical for all callers.
fn open_dialog_with_builders(
    window_title: SharedString,
    title: TitleBuilder,
    content: ContentBuilder,
    width: f32,
    body_height: f32,
    cx: &mut App,
) -> (Entity<AppDialog>, WindowHandle<Root>) {
    let dialog = cx.new(|cx| AppDialog::new(Rc::clone(&title), Rc::clone(&content), cx));
    let dialog_focus = dialog.read(cx).focus.clone();
    let dialog_view = dialog.clone();
    let bounds = Bounds::centered(
        None,
        size(px(width), px(body_height + PANEL_HEADER_HEIGHT)),
        cx,
    );
    let mut options = TitleBar::window_options();
    // Settings and native confirmation dialogs share the active theme's backdrop.
    options.window_background = crate::theme::window_background(cx);
    // The platform keeps this window above its parent and disables parent input.
    options.kind = WindowKind::Dialog;
    options.window_bounds = Some(WindowBounds::Windowed(bounds));
    // Compact status dialogs must not be enlarged by the settings-dialog minimum.
    // Larger dialogs retain the existing resize limits; small ones can still grow.
    options.window_min_size = Some(size(
        px(width.min(520.)),
        px((body_height + PANEL_HEADER_HEIGHT).min(320.)),
    ));
    let native_title = window_title.clone();
    let handle = cx
        .open_window(options, move |window, cx| {
            window.set_window_title(native_title.as_ref());
            window.focus(&dialog_focus, cx);
            // Root stays transparent; AppDialog paints its own themed surface.
            cx.new(|cx| Root::new(dialog_view, window, cx))
        })
        .expect("failed to open app dialog window");
    (dialog, handle)
}

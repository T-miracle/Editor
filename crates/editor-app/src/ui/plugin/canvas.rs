//! A keyed canvas owns its focus and IME; sibling native inputs keep their independent handlers.
use super::*;
use gpui_kit::{
    Bounds, ElementInputHandler, EntityInputHandler, FontWeight, InteractiveElement, KeyDownEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point,
    ScrollWheelEvent, Styled, Subscription, TextRun, UTF16Selection, canvas, div, fill, font,
    point, px, rgb, size,
};
use plugin_runtime::plugin_protocol::{
    self as protocol,
    ui::{Canvas, CanvasEvent, GridMetrics, PointerPhase},
};
use std::ops::Range;
mod scroll;

pub(super) struct CanvasView {
    pub(super) drawing: Canvas,
    pub(super) font: protocol::FontStyle,
    pub(super) enabled: bool,
    pub(super) revision: u64,
    pub(super) foreground: u32,
    pub(super) images: Option<std::sync::Arc<Vec<Option<super::images::VectorImage>>>>,
    focus: FocusHandle,
    bounds: Bounds<Pixels>,
    measured: Option<CanvasEvent>,
    composition: String,
    selection: Range<usize>,
    drag: Option<MouseButton>,
    scroll: scroll::CanvasScroll,
    sink: Rc<dyn Fn(CanvasEvent, u64, &mut App)>,
    _subscriptions: Vec<Subscription>,
}

impl CanvasView {
    /// Sibling native controls return keyboard input to the addressed canvas after their action.
    pub(super) fn focus_handle(&self) -> FocusHandle {
        self.focus.clone()
    }
    pub(super) fn new(
        drawing: Canvas,
        sink: impl Fn(CanvasEvent, u64, &mut App) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let subscriptions = vec![
            cx.on_focus(&focus, window, |this, _, cx| {
                this.emit(CanvasEvent::Focus { focused: true }, cx)
            }),
            cx.on_blur(&focus, window, |this, _, cx| {
                this.cancel_composition();
                this.emit(CanvasEvent::Focus { focused: false }, cx);
                cx.notify();
            }),
        ];
        Self {
            drawing,
            font: Default::default(),
            enabled: true,
            revision: 0,
            foreground: 0,
            images: None,
            focus,
            bounds: Bounds::default(),
            measured: None,
            composition: String::new(),
            selection: 0..0,
            drag: None,
            scroll: Default::default(),
            sink: Rc::new(sink),
            _subscriptions: subscriptions,
        }
    }
    fn emit(&self, event: CanvasEvent, cx: &mut Context<Self>) {
        if self.enabled {
            (self.sink)(event, self.revision, cx);
        }
    }
    /// A removed or disabled input target must not retain an unfinished native composition.
    pub(super) fn cancel_composition(&mut self) {
        self.composition.clear();
        self.selection = 0..0;
    }
    /// Deactivation revokes all input ownership; a non-focusable but active image keeps its drag.
    pub(super) fn deactivate(&mut self) {
        self.cancel_composition();
        self.drag = None;
    }
    /// A rejected deferred callback or reactivation must retry its current native geometry.
    pub(super) fn invalidate_measurement(&mut self) {
        self.measured = None;
    }
    fn pointer(
        &self,
        phase: PointerPhase,
        position: Point<Pixels>,
        button: u8,
        clicks: u8,
        shift: bool,
        cx: &mut Context<Self>,
    ) {
        let point = position - self.bounds.origin;
        self.emit(
            CanvasEvent::Pointer {
                phase,
                x: point.x / px(1.),
                y: point.y / px(1.),
                button,
                clicks,
                shift,
            },
            cx,
        );
    }
    /// Geometry is measured at this layout node, independent of surrounding toolbars and panel edges.
    fn measure(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        self.bounds = bounds;
        self.scroll.update(bounds, self.drawing.scroll.clone());
        if let Some(offset) = self.scroll.take_offset() {
            self.emit(CanvasEvent::Scroll { offset }, cx);
        }
        let grid = self.drawing.grid.then(|| {
            let face = font(
                self.font
                    .family
                    .clone()
                    .unwrap_or_else(|| "Consolas".into()),
            );
            let id = window.text_system().resolve_font(&face);
            let size = self.font.size_px.unwrap_or(14.);
            GridMetrics {
                cell_width: window
                    .text_system()
                    .advance(id, px(size), 'M')
                    .map(|s| s.width / px(1.))
                    .unwrap_or(8.4),
                cell_height: (size * 1.45).ceil(),
            }
        });
        let event = CanvasEvent::Resize {
            width: bounds.size.width / px(1.),
            height: bounds.size.height / px(1.),
            grid,
        };
        if self.enabled && self.measured.as_ref() != Some(&event) {
            self.measured = Some(event.clone());
            self.emit(event, cx);
        }
    }
    fn paint(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        self.capture_pointer(window, cx);
        if self.enabled && self.drawing.focusable && self.focus.is_focused(window) {
            window.handle_input(
                &self.focus,
                ElementInputHandler::new(bounds, cx.entity()),
                cx,
            );
        }
        for (index, operation) in self.drawing.paint.iter().enumerate() {
            match operation {
                protocol::Paint::Fill {
                    rect,
                    color,
                    extend_to_bottom,
                } => {
                    let mut rect = *rect;
                    if *extend_to_bottom {
                        rect.h = (bounds.size.height / px(1.) - rect.y).max(0.);
                    }
                    window.paint_quad(fill(rect_bounds(rect, bounds.origin), rgb(*color)));
                }
                protocol::Paint::Text {
                    x,
                    y,
                    text,
                    color,
                    size,
                    bold,
                    font: family,
                } => {
                    let mut face = font(
                        family
                            .clone()
                            .or(self.font.family.clone())
                            .unwrap_or_else(|| "Segoe UI".into()),
                    );
                    if *bold {
                        face.weight = FontWeight::BOLD;
                    }
                    paint_text(
                        text,
                        *size,
                        face,
                        rgb(*color).into(),
                        bounds.origin + point(px(*x), px(*y)),
                        window,
                        cx,
                    );
                }
                protocol::Paint::Svg { .. } => {
                    if let Some(image) = self
                        .images
                        .as_ref()
                        .and_then(|images| images.get(index))
                        .and_then(Option::as_ref)
                    {
                        let _ = window.paint_image(
                            bounds,
                            rect_bounds(image.rect, bounds.origin),
                            Default::default(),
                            image.image.clone(),
                            0,
                            false,
                        );
                    }
                }
            }
        }
        if !self.composition.is_empty() {
            let anchor = self.drawing.caret.unwrap_or_default();
            paint_text(
                &self.composition,
                self.font.size_px.unwrap_or(14.),
                font(
                    self.font
                        .family
                        .clone()
                        .unwrap_or_else(|| "Segoe UI".into()),
                ),
                rgb(self.foreground).into(),
                bounds.origin + point(px(anchor.x), px(anchor.y)),
                window,
                cx,
            );
        }
    }

    /// Capture only a drag begun here, retaining ownership outside the canvas until release.
    fn capture_pointer(&self, window: &mut Window, cx: &Context<Self>) {
        let moved = cx.entity().downgrade();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
            if phase.capture() {
                let _ = moved.update(cx, |this, cx| {
                    if this.enabled && this.drag.is_some() && this.drag == event.pressed_button {
                        this.pointer(
                            PointerPhase::Move,
                            event.position,
                            button_index(this.drag.unwrap()),
                            1,
                            event.modifiers.shift,
                            cx,
                        );
                        cx.stop_propagation();
                    }
                });
            }
        });
        let released = cx.entity().downgrade();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
            if phase.capture() {
                let _ = released.update(cx, |this, cx| {
                    if this.enabled && this.drag == Some(event.button) {
                        this.drag = None;
                        this.pointer(
                            PointerPhase::Up,
                            event.position,
                            button_index(event.button),
                            1,
                            event.modifiers.shift,
                            cx,
                        );
                        cx.stop_propagation();
                    }
                });
            }
        });
    }
}

/// The portable pointer numbering stays independent of GPUI's enum representation.
fn button_index(button: MouseButton) -> u8 {
    match button {
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        _ => 0,
    }
}

/// Host shaping preserves Unicode runs instead of delegating text layout to each WASM guest.
fn paint_text(
    text: &str,
    size: f32,
    face: gpui_kit::Font,
    color: gpui_kit::Hsla,
    origin: Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let run = TextRun {
        len: text.len(),
        font: face,
        color,
        ..Default::default()
    };
    let line = window
        .text_system()
        .shape_line(text.to_owned().into(), px(size), &[run], None);
    let _ = line.paint(
        origin,
        px((size * 1.45).ceil()),
        gpui_kit::TextAlign::Left,
        None,
        window,
        cx,
    );
}
fn rect_bounds(rect: protocol::Rect, origin: Point<Pixels>) -> Bounds<Pixels> {
    Bounds::new(
        origin + point(px(rect.x), px(rect.y)),
        size(px(rect.w), px(rect.h)),
    )
}

impl Render for CanvasView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let measure = cx.entity().downgrade();
        let paint = measure.clone();
        let mut element = div()
            .id("composed-canvas")
            .size_full()
            .relative()
            .overflow_hidden()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if !this.enabled
                    || !this.drawing.focusable
                    || !this.focus.is_focused(window)
                    || !this.composition.is_empty()
                {
                    return;
                }
                let key = &event.keystroke;
                this.emit(
                    CanvasEvent::Key {
                        key: key.key.clone(),
                        ctrl: key.modifiers.control,
                        alt: key.modifiers.alt,
                        shift: key.modifiers.shift,
                    },
                    cx,
                );
                // Count Unicode characters, not bytes: a Chinese character is ordinary IME text.
                if key.modifiers.control
                    || key.modifiers.alt
                    || (key.key.chars().count() > 1 && key.key != "space")
                {
                    cx.stop_propagation();
                }
            }));
        // Each pointer button retains its identity; canvas clients decide its meaning.
        for (button, index) in [
            (MouseButton::Left, 0),
            (MouseButton::Middle, 1),
            (MouseButton::Right, 2),
        ] {
            element = element.on_mouse_down(
                button,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    if !this.enabled {
                        return;
                    }
                    this.drag = Some(button);
                    if this.enabled && this.drawing.focusable {
                        this.focus.focus(window, cx);
                    }
                    this.pointer(
                        PointerPhase::Down,
                        event.position,
                        index,
                        event.click_count as u8,
                        event.modifiers.shift,
                        cx,
                    );
                    cx.stop_propagation();
                }),
            );
        }
        element = element
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                // Line wheels on a grid use measured rows, not the smaller font point size.
                let line_height = match &this.measured {
                    Some(CanvasEvent::Resize {
                        grid: Some(grid), ..
                    }) => grid.cell_height,
                    _ => this.font.size_px.unwrap_or(14.),
                };
                let delta = event.delta.pixel_delta(px(line_height));
                let at = event.position - this.bounds.origin;
                this.emit(
                    CanvasEvent::Wheel {
                        x: at.x / px(1.),
                        y: at.y / px(1.),
                        delta_x: delta.x / px(1.),
                        delta_y: delta.y / px(1.),
                        shift: event.modifiers.shift,
                    },
                    cx,
                );
                cx.stop_propagation();
            }))
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let _ = measure.update(cx, |this, cx| this.measure(bounds, window, cx));
                    },
                    move |bounds, _, window, cx| {
                        let _ = paint.update(cx, |this, cx| this.paint(bounds, window, cx));
                    },
                )
                .size_full(),
            );
        // The overlay must paint after the guest canvas, which may fill its entire bounds.
        if self.enabled && self.drawing.scroll.is_some() {
            element = element.child(crate::ui::controls::vertical_viewport_scrollbar(
                &self.scroll,
                cx,
            ));
        }
        element
    }
}

impl EntityInputHandler for CanvasView {
    /// Only uncommitted composition lives in the native canvas; committed text is an addressed event.
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let (bytes, units) = composition_range(&self.composition, range);
        *adjusted = Some(units);
        Some(self.composition[bytes].into())
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.selection.clone(),
            reversed: false,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        (!self.composition.is_empty()).then(|| 0..self.composition.encode_utf16().count())
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.cancel_composition();
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range.unwrap_or(0..self.composition.encode_utf16().count());
        let (bytes, _) = composition_range(&self.composition, range);
        self.composition.replace_range(bytes, text);
        let text = std::mem::take(&mut self.composition);
        self.selection = 0..0;
        self.emit(CanvasEvent::Text { text }, cx);
        cx.notify();
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range.unwrap_or(0..self.composition.encode_utf16().count());
        let (bytes, units) = composition_range(&self.composition, range);
        self.composition.replace_range(bytes, text);
        let end = text.encode_utf16().count();
        let selected = selected.unwrap_or(end..end);
        self.selection = composition_range(
            &self.composition,
            units.start + selected.start..units.start + selected.end,
        )
        .1;
        cx.notify();
    }
    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(rect_bounds(
            self.drawing.caret.unwrap_or(protocol::Rect {
                x: 0.,
                y: 0.,
                w: 1.,
                h: self.font.size_px.unwrap_or(14.),
            }),
            self.bounds.origin,
        ))
    }
    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(0)
    }
}

/// GPUI ranges use UTF-16; expand split surrogate boundaries to whole Unicode scalars.
fn composition_range(text: &str, range: Range<usize>) -> (Range<usize>, Range<usize>) {
    let mut offsets = vec![(0, 0)];
    let mut units = 0;
    for (byte, character) in text.char_indices() {
        units += character.len_utf16();
        offsets.push((byte + character.len_utf8(), units));
    }
    let start = range.start.min(units);
    let end = range.end.max(start).min(units);
    let first = offsets
        .iter()
        .rev()
        .find(|(_, unit)| *unit <= start)
        .unwrap();
    let last = offsets.iter().find(|(_, unit)| *unit >= end).unwrap();
    (first.0..last.0, first.1..last.1)
}

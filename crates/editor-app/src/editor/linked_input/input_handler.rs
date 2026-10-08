//! Delegate platform input and native gestures while retaining Base IME, selection and Undo behavior.
use super::*;
use gpui_kit::{ElementInputHandler, TextInputConfiguration, UTF16Selection, canvas};

impl EntityInputHandler for Bridge {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        self.editor.update(cx, |editor, cx| {
            editor.text_for_range(range, adjusted, window, cx)
        })
    }
    fn selected_text_range(
        &mut self,
        ignore: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        self.editor.update(cx, |editor, cx| {
            editor.selected_text_range(ignore, window, cx)
        })
    }
    fn marked_text_range(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.editor
            .update(cx, |editor, cx| editor.marked_text_range(window, cx))
    }
    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.queue(NativeInput::Unmark, window, cx) {
            return;
        }
        if let Some(primary) = self.composing {
            if let Some(range) = self
                .group
                .as_ref()
                .map(|group| group.ranges[primary].clone())
            {
                let source = self.editor.read(cx).text().to_string();
                let name = source[range.clone()].to_string();
                self.edit_bytes(range, &name, window, cx);
            }
        }
        self.composing = None;
        self.editor
            .update(cx, |editor, cx| editor.unmark_text(window, cx));
    }
    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.queue(NativeInput::Replace(range.clone(), text.into()), window, cx) {
            return;
        }
        if let Some(bytes) = self.replacement(range.clone(), window, cx) {
            if self.edit_bytes(bytes, text, window, cx) {
                return;
            }
        }
        self.composing = None;
        self.group = None;
        self.editor.update(cx, |editor, cx| {
            editor.replace_text_in_range(range, text, window, cx)
        });
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.queue(
            NativeInput::Mark(range.clone(), text.into(), selected.clone()),
            window,
            cx,
        ) {
            return;
        }
        let before = self.editor.read(cx).text().to_string();
        let bytes = self.replacement(range.clone(), window, cx);
        let primary = bytes.as_ref().and_then(|bytes| {
            self.group
                .as_ref()
                .filter(|group| self.enabled() && group.fingerprint == digest(&before))
                .and_then(|group| {
                    group
                        .ranges
                        .iter()
                        .position(|name| bytes.start >= name.start && bytes.end <= name.end)
                })
        });
        self.editor.update(cx, |editor, cx| {
            editor.replace_and_mark_text_in_range(range, text, selected, window, cx)
        });
        if let Some(bytes) = bytes {
            self.align_preedit(&before, bytes, primary, cx);
        }
        if text.is_empty() {
            self.composing = None;
        }
    }
    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        self.editor.update(cx, |editor, cx| {
            editor.bounds_for_range(range, bounds, window, cx)
        })
    }
    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        self.editor.update(cx, |editor, cx| {
            editor.character_index_for_point(point, window, cx)
        })
    }
    fn set_selected_text_range(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Platform selection changes cannot overtake text that has already entered the native queue.
        self.finish_before_action(window, cx);
        self.editor.update(cx, |editor, cx| {
            editor.set_selected_text_range(range, window, cx)
        });
    }
    fn text_length_utf16(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<usize> {
        self.editor
            .update(cx, |editor, cx| editor.text_length_utf16(window, cx))
    }
    fn accepts_text_input(&self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.editor
            .update(cx, |editor, cx| editor.accepts_text_input(window, cx))
    }
    fn text_input_configuration(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> TextInputConfiguration {
        self.editor
            .update(cx, |editor, cx| editor.text_input_configuration(window, cx))
    }
    fn text_input_editable_range(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.editor.update(cx, |editor, cx| {
            editor.text_input_editable_range(window, cx)
        })
    }
}

impl EditorApp {
    /// Flush entered characters before Undo/Redo and caret movements; semantic cancellation never loses input.
    pub(crate) fn finish_linked_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(bridge) = self.language_edits.bridge.clone() {
            bridge.update(cx, |bridge, cx| bridge.finish_before_action(window, cx));
        }
    }
    pub(crate) fn linked_history_undo(
        &mut self,
        _: &gpui_base::input::Undo,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Sibling name/settings inputs own their native history; this bridge belongs to source focus.
        if !self.editor.focus_handle(cx).is_focused(window) {
            cx.propagate();
            return;
        }
        if !self.language_edits.history_forwarding.get()
            && self.language_edits.bridge.clone().is_some_and(|bridge| {
                bridge.update(cx, |bridge, cx| bridge.queue_history(true, window, cx))
            })
        {
            // Capture listeners propagate by default; consume only the queued user command.
            cx.stop_propagation();
            return;
        }
        if !self.language_edits.history_forwarding.get() {
            self.finish_linked_input(window, cx);
        }
        cx.propagate();
    }
    pub(crate) fn linked_history_redo(
        &mut self,
        _: &gpui_base::input::Redo,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.editor.focus_handle(cx).is_focused(window) {
            cx.propagate();
            return;
        }
        if !self.language_edits.history_forwarding.get()
            && self.language_edits.bridge.clone().is_some_and(|bridge| {
                bridge.update(cx, |bridge, cx| bridge.queue_history(false, window, cx))
            })
        {
            cx.stop_propagation();
            return;
        }
        if !self.language_edits.history_forwarding.get() {
            self.finish_linked_input(window, cx);
        }
        cx.propagate();
    }
    pub(crate) fn linked_pointer_down(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.finish_linked_input(window, cx);
        cx.propagate();
    }
    pub(crate) fn linked_navigation_key(
        &mut self,
        event: &gpui_kit::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if [
            "left", "right", "up", "down", "home", "end", "pageup", "pagedown", "enter", "tab",
            "escape",
        ]
        .contains(&event.keystroke.key.as_str())
        {
            self.finish_linked_input(window, cx);
        }
        cx.propagate();
    }
    /// Paint after Base's input handler so the platform uses this semantic extension at the same bounds.
    pub(crate) fn render_linked_input(&self) -> Option<gpui_kit::AnyElement> {
        let bridge = self.language_edits.bridge.clone()?;
        Some(
            canvas(
                |_, _, _| (),
                move |_, (), window, cx| {
                    let editor = bridge.read(cx).editor.clone();
                    if bridge.read(cx).active || bridge.read(cx).has_pending() {
                        window.handle_input(
                            &editor.focus_handle(cx),
                            ElementInputHandler::new(
                                editor.read(cx).input_bounds(),
                                bridge.clone(),
                            ),
                            cx,
                        );
                    }
                },
            )
            .absolute()
            .size_full()
            .into_any_element(),
        )
    }
    pub(crate) fn linked_backspace(
        &mut self,
        _: &gpui_base::input::Backspace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Ancestor capture must never turn a sibling field's deletion into a source edit.
        if !self.editor.focus_handle(cx).is_focused(window) {
            cx.propagate();
            return;
        }
        if !self
            .language_edits
            .bridge
            .clone()
            .is_some_and(|bridge| bridge.update(cx, |bridge, cx| bridge.deletion(true, window, cx)))
        {
            cx.propagate();
        } else {
            cx.stop_propagation();
        }
    }
    pub(crate) fn linked_delete(
        &mut self,
        _: &gpui_base::input::Delete,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.editor.focus_handle(cx).is_focused(window) {
            cx.propagate();
            return;
        }
        if !self.language_edits.bridge.clone().is_some_and(|bridge| {
            bridge.update(cx, |bridge, cx| bridge.deletion(false, window, cx))
        }) {
            cx.propagate();
        } else {
            cx.stop_propagation();
        }
    }
    pub(crate) fn linked_paste(
        &mut self,
        _: &gpui_base::input::Paste,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Native clipboard commands follow the focused input, not the source pane's ancestors.
        if !self.editor.focus_handle(cx).is_focused(window) {
            cx.propagate();
            return;
        }
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            cx.propagate();
            return;
        };
        if self.language_edits.bridge.clone().is_some_and(|bridge| {
            bridge.update(cx, |bridge, cx| {
                bridge.queue(NativeInput::Replace(None, text.clone()), window, cx)
            })
        }) {
            cx.stop_propagation();
            return;
        }
        let range = self.editor.read(cx).selected_range();
        if !self.language_edits.bridge.clone().is_some_and(|bridge| {
            bridge.update(cx, |bridge, cx| bridge.edit_bytes(range, &text, window, cx))
        }) {
            cx.propagate();
        } else {
            cx.stop_propagation();
        }
    }
    pub(crate) fn linked_cut(
        &mut self,
        _: &gpui_base::input::Cut,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.editor.focus_handle(cx).is_focused(window) {
            cx.propagate();
            return;
        }
        if self.language_edits.bridge.clone().is_some_and(|bridge| {
            bridge.update(cx, |bridge, cx| bridge.queue(NativeInput::Cut, window, cx))
        }) {
            cx.stop_propagation();
            return;
        }
        let range = self.editor.read(cx).selected_range();
        let text = self.editor.read(cx).selected_text().to_string();
        if !range.is_empty()
            && self.language_edits.bridge.clone().is_some_and(|bridge| {
                bridge.update(cx, |bridge, cx| bridge.edit_bytes(range, "", window, cx))
            })
        {
            cx.stop_propagation();
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text));
        } else {
            cx.propagate();
        }
    }
}

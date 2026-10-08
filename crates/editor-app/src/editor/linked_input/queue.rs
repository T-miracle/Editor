//! Bounded native command replay waits for semantics and preserves input across retirement and history.
use super::*;

impl Bridge {
    /// A cursor can move and type within one frame. Await that same semantic request without blocking UI.
    pub(super) fn queue(
        &mut self,
        command: NativeInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.replaying || (!self.enabled() && self.pending.is_none()) {
            return false;
        }
        if !self.editor.read(cx).is_editable() {
            return false;
        }
        if self.pending.is_none() {
            self.refresh(cx);
            if self.group.is_some() || self.in_flight.is_none() {
                return false;
            }
            let editor = self.editor.read(cx);
            self.pending = Some(PendingInput {
                source: editor.text().to_string(),
                selection: editor.selected_range(),
                commands: Vec::new(),
                native_only: false,
            });
            self.wait_pending(window, cx);
        }
        let pending = self.pending.as_mut().unwrap();
        let command_bytes = match &command {
            NativeInput::Replace(_, text)
            | NativeInput::Mark(_, text, _)
            | NativeInput::Bytes(_, text) => text.len(),
            _ => 0,
        };
        let queued_bytes = pending
            .commands
            .iter()
            .map(|command| match command {
                NativeInput::Replace(_, text)
                | NativeInput::Mark(_, text, _)
                | NativeInput::Bytes(_, text) => text.len(),
                _ => 0,
            })
            .sum::<usize>();
        let exceeded =
            pending.commands.len() >= 128 || queued_bytes.saturating_add(command_bytes) > 64 * 1024;
        // The crossing event is user input too. Cancel its semantic wait, retaining it behind earlier
        // history in this same FIFO; returning false here would let it overtake deferred Base Undo.
        pending.commands.push(command);
        if exceeded || pending.native_only {
            pending.native_only = true;
            self.group = None;
            self.in_flight = None;
            self.generation += 1;
            self.flush_pending(window, cx);
        }
        true
    }

    /// Missing or slow semantic services fall back after 500ms without blocking native dispatch.
    fn wait_pending(&self, window: &mut Window, cx: &mut Context<Self>) {
        let keep_alive = cx.entity();
        cx.spawn_in(window, async move |this, cx| {
            for attempt in 0..50 {
                cx.background_executor()
                    .timer(Duration::from_millis(10))
                    .await;
                // Recognition/provider changes retire semantic leases while the same native tab
                // remains open. Read ownership before borrowing Bridge, outside any parent update.
                let native_open =
                    keep_alive.read_with(cx, |bridge, cx| bridge.native_owner_is_open(cx));
                let finished = this
                    .update_in(cx, |bridge, window, cx| {
                        bridge.native_open = native_open;
                        if bridge.pending.is_none() {
                            return true;
                        }
                        if !native_open {
                            bridge.pending = None;
                            return true;
                        }
                        if !bridge.enabled() {
                            bridge.group = None;
                            bridge.pending.as_mut().unwrap().native_only = true;
                            bridge.flush_pending(window, cx);
                            return true;
                        }
                        if bridge.in_flight.is_none() || attempt == 49 {
                            if attempt == 49 {
                                bridge.generation += 1;
                                bridge.in_flight = None;
                                bridge.group = None;
                                bridge.pending.as_mut().unwrap().native_only = true;
                            }
                            bridge.flush_pending(window, cx);
                            return true;
                        }
                        false
                    })
                    .unwrap_or(true);
                if finished {
                    break;
                }
            }
            // Entered user commands must survive removal of their retired bridge from app state.
            drop(keep_alive);
        })
        .detach();
    }

    /// History is a user command in the same queue; native Base remains its only executor and storage.
    pub(super) fn queue_history(
        &mut self,
        undo: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // Idle history remains Base's action. A 129th history event shares ordinary overflow ordering.
        self.pending.is_some() && self.queue(NativeInput::History(undo), window, cx)
    }

    /// Replay into Base once the exact source is still current; stale queued input never targets another tab.
    fn flush_pending(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.history_in_flight {
            // Its sole deferred action resumes this same FIFO after Base has restored text and selection.
            return;
        }
        let Some(pending) = self.pending.take() else {
            return;
        };
        if !self.native_open {
            return;
        }
        let editor = self.editor.read(cx);
        let before = editor.text().to_string();
        let selection = editor.selected_range();
        let target = map_range(&pending.source, &before, pending.selection.clone());
        let preserve_selection = selection != target;
        if before != pending.source || preserve_selection {
            self.group = None;
        }
        self.editor
            .update(cx, |editor, cx| editor.set_selected_range(target, cx));
        self.replaying = true;
        let mut commands = pending.commands.into_iter();
        while let Some(command) = commands.next() {
            match command {
                NativeInput::Replace(range, text) => {
                    self.replace_text_in_range(range, &text, window, cx)
                }
                NativeInput::Mark(range, text, selected) => {
                    self.replace_and_mark_text_in_range(range, &text, selected, window, cx)
                }
                NativeInput::Unmark => self.unmark_text(window, cx),
                NativeInput::Bytes(range, text) => {
                    if !self.edit_bytes(range.clone(), &text, window, cx) {
                        self.editor.update(cx, |editor, cx| {
                            editor.set_selected_range(range, cx);
                            editor.replace(text, window, cx);
                        });
                    }
                }
                NativeInput::Delete(backward) => {
                    if !self.deletion(backward, window, cx) {
                        // The queued action has no other selected target; use the same grapheme extent.
                        let state = self.editor.read(cx);
                        let source = state.text().to_string();
                        let mut range = state.selected_range();
                        if range.is_empty() {
                            if backward {
                                range.start = source[..range.start]
                                    .grapheme_indices(true)
                                    .next_back()
                                    .map_or(0, |(start, _)| start);
                            } else if let Some(grapheme) =
                                source[range.end..].graphemes(true).next()
                            {
                                range.end += grapheme.len();
                            }
                        }
                        self.editor.update(cx, |editor, cx| {
                            editor.set_selected_range(range, cx);
                            editor.replace(String::new(), window, cx);
                        });
                    }
                }
                NativeInput::Cut => {
                    // An earlier queued replacement may have collapsed the selection. Cut uses the
                    // native selection at its own replay turn, never the stale visible byte range.
                    let editor = self.editor.read(cx);
                    let range = editor.selected_range();
                    let text = editor.selected_text().to_string();
                    if !range.is_empty() {
                        if !self.edit_bytes(range.clone(), "", window, cx) {
                            self.editor.update(cx, |editor, cx| {
                                editor.set_selected_range(range, cx);
                                editor.replace(String::new(), window, cx);
                            });
                        }
                        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text));
                    }
                }
                NativeInput::History(undo) => {
                    // Dispatch outside any entity update: Root/Bridge listeners must never borrow reentrantly.
                    let owner = cx.entity();
                    let editor = self.editor.clone();
                    let forwarding = self.history_forwarding.clone();
                    // Keep the tail reachable in one FIFO. Input arriving before the deferred turn
                    // appends here; no callback carries an older tail that could overwrite new input.
                    self.pending = Some(PendingInput {
                        source: self.editor.read(cx).text().to_string(),
                        selection: self.editor.read(cx).selected_range(),
                        commands: commands.collect(),
                        native_only: pending.native_only,
                    });
                    self.replaying = false;
                    self.history_in_flight = true;
                    window.defer(cx, move |window, cx| {
                        let native_open = owner.read(cx).native_owner_is_open(cx);
                        if !native_open {
                            owner.update(cx, |bridge, _| {
                                bridge.native_open = false;
                                bridge.history_in_flight = false;
                                bridge.pending = None;
                            });
                            return;
                        }
                        forwarding.set(true);
                        let focus = editor.focus_handle(cx);
                        if undo {
                            focus.dispatch_action(&gpui_base::input::Undo, window, cx);
                        } else {
                            focus.dispatch_action(&gpui_base::input::Redo, window, cx);
                        }
                        forwarding.set(false);
                        owner.update(cx, |bridge, cx| {
                            bridge.native_open = native_open;
                            bridge.history_in_flight = false;
                            bridge.resume_commands(window, cx);
                        });
                    });
                    return;
                }
            }
        }
        self.replaying = false;
        if preserve_selection {
            let after = self.editor.read(cx).text().to_string();
            let selection = map_range(&before, &after, selection);
            self.editor
                .update(cx, |editor, cx| editor.set_selected_range(selection, cx));
        }
        self.rebind_after_retirement(cx);
    }

    /// Text after queued history acquires ranges for Base's actual restored source before replay.
    fn resume_commands(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.native_open {
            self.pending = None;
            self.rebind_after_retirement(cx);
            return;
        }
        let Some(pending) = self.pending.as_mut() else {
            return;
        };
        if pending.commands.is_empty() {
            self.pending = None;
            self.rebind_after_retirement(cx);
            return;
        }
        self.group = None;
        self.observed = None;
        let editor = self.editor.read(cx);
        pending.source = editor.text().to_string();
        pending.selection = editor.selected_range();
        if pending.native_only || matches!(pending.commands.first(), Some(NativeInput::History(_)))
        {
            self.flush_pending(window, cx);
        } else {
            self.refresh(cx);
            self.wait_pending(window, cx);
        }
    }

    /// Rebinding occurs after native Change effects; later input never overtakes a retired queue.
    fn rebind_after_retirement(&self, cx: &mut Context<Self>) {
        if !self.active && !self.has_pending() {
            let owner = self.owner.clone();
            cx.defer(move |cx| {
                let _ = owner.update(cx, |app, cx| app.sync_linked_input(cx));
            });
        }
    }

    /// Actions that mutate selection/history commit entered input first, while cancelling only semantics.
    pub(crate) fn finish_before_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending.is_some() {
            self.generation += 1;
            self.in_flight = None;
            self.group = None;
            self.pending.as_mut().unwrap().native_only = true;
            self.flush_pending(window, cx);
        }
    }
}

/// Remap an entered input target across a later native change without keeping mutable shadow text.
fn map_range(before: &str, after: &str, range: Range<usize>) -> Range<usize> {
    if before == after {
        return range;
    }
    let mut start = before
        .bytes()
        .zip(after.bytes())
        .take_while(|(left, right)| left == right)
        .count();
    while !before.is_char_boundary(start) || !after.is_char_boundary(start) {
        start -= 1;
    }
    let mut tail = before[start..]
        .bytes()
        .rev()
        .zip(after[start..].bytes().rev())
        .take_while(|(left, right)| left == right)
        .count();
    while !before.is_char_boundary(before.len() - tail)
        || !after.is_char_boundary(after.len() - tail)
    {
        tail -= 1;
    }
    let old_end = before.len() - tail;
    let new_end = after.len() - tail;
    let offset = |value: usize| {
        if value <= start {
            value
        } else if value >= old_end {
            shift(value, new_end as isize - old_end as isize)
        } else {
            start + (value - start).min(new_end - start)
        }
    };
    let clip = |mut value: usize| {
        while !after.is_char_boundary(value) {
            value -= 1;
        }
        value
    };
    clip(offset(range.start))..clip(offset(range.end))
}

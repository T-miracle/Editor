//! Versioned range operations use the native editor's atomic history and document change subscription.
use super::*;
use gpui_kit::EntityInputHandler as _;
use protocol::api::TextRange;

impl EditorApp {
    /// A toolbar action may only observe or edit the still-active document that produced its UI.
    pub(super) fn current_plugin_edit_target(
        &self,
        document: &DocumentVersion,
    ) -> Result<usize, Failure> {
        self.active_text_tab_index()
            .filter(|index| {
                self.plugin_document_version(*index)
                    .is_ok_and(|now| now == *document)
            })
            .ok_or_else(|| {
                Failure::new(
                    ErrorCode::StaleRevision,
                    "Document changed or is no longer active",
                )
            })
    }

    /// Read retained native selection, including when a source toolbar button currently owns focus.
    pub(in crate::extensions) fn read_plugin_document_selection(
        &self,
        document: &DocumentVersion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, Failure> {
        let index = self.current_plugin_edit_target(document)?;
        self.text_tab(index)
            .ok_or_else(|| {
                Failure::new(ErrorCode::UnsupportedOperation, "File has no text editor")
            })?
            .editor
            .update(cx, |editor, cx| {
                if editor.marked_text_range(window, cx).is_some() {
                    return Err(Failure::new(
                        ErrorCode::InvalidState,
                        "Finish the input composition first",
                    ));
                }
                let range = editor.selected_range();
                let text = editor.selected_text().to_string();
                if text.len() > 1024 * 1024 {
                    return Err(Failure::new(
                        ErrorCode::LimitExceeded,
                        "Selection exceeds 1 MiB",
                    ));
                }
                Ok(Value::DocumentSelection {
                    document: document.clone(),
                    range: TextRange {
                        start: range.start,
                        end: range.end,
                    },
                    text,
                })
            })
    }

    /// Validate every byte range before entering the effect, then publish after DocumentSession observes Change.
    pub(super) fn replace_plugin_document(
        &mut self,
        request: EditorRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Op::ReplaceDocumentRange {
            document,
            range,
            text,
            selection,
            expected_selection,
        } = request.operation()
        else {
            unreachable!()
        };
        let result = (|| {
            let index = self.current_plugin_edit_target(document)?;
            let editor = self
                .text_tab(index)
                .ok_or_else(|| {
                    Failure::new(ErrorCode::UnsupportedOperation, "File has no text editor")
                })?
                .editor
                .clone();
            let before = editor.read(cx).text().to_string();
            validate_edit(&before, range, text, selection)?;
            if expected_selection.as_ref().is_some_and(|expected| {
                editor.read(cx).selected_range() != (expected.start..expected.end)
            }) {
                return Err(Failure::new(
                    ErrorCode::StaleRevision,
                    "Selection changed during the request",
                ));
            }
            editor.update(cx, |editor, cx| {
                if !editor.is_editable() || editor.marked_text_range(window, cx).is_some() {
                    return Err(Failure::new(
                        ErrorCode::InvalidState,
                        "Editor is unavailable or composing text",
                    ));
                }
                if !request.enter_side_effect() {
                    return Err(Failure::new(ErrorCode::Cancelled, "Edit did not execute"));
                }
                // Base's public replace marks an Atomic edit; it does not merge with surrounding typing.
                // Its Change event is the sole route to DocumentSession.note_edit and capability revision.
                editor.set_selected_range(range.start..range.end, cx);
                editor.replace(text.clone(), window, cx);
                editor.set_selected_range(selection.start..selection.end, cx);
                Ok(())
            })?;
            let preview = self.active_editor_preview(cx);
            let source_visible = preview.as_ref().is_none_or(|preview| {
                self.editor_preview_mode(preview, cx) != protocol::PreviewMode::Preview
            });
            let preview_focused = preview.as_ref().is_some_and(|preview| {
                preview
                    .read(cx)
                    .native_ui
                    .as_ref()
                    .is_some_and(|view| view.read(cx).contains_focus(window, cx))
            });
            // A task or other preview control keeps its keyboard target. The separate source toolbar
            // still hands focus back to the editor so placeholders can be typed immediately.
            if source_visible && !preview_focused {
                editor.update(cx, |editor, cx| editor.focus(window, cx));
            }
            let identity = editor.entity_id();
            let completion = request.clone();
            let owner = cx.entity().downgrade();
            cx.defer(move |cx| {
                // GPUI delivers the native Change subscription before this continuation. Returning sooner
                // would acknowledge the replacement with its old revision and bypass the document seam.
                let result = owner
                    .update(cx, |app, cx| {
                        app.tabs
                            .iter()
                            .position(|tab| tab.owns_editor_id(identity))
                            .ok_or_else(|| {
                                Failure::new(ErrorCode::StaleRevision, "Edited document was closed")
                            })
                            .and_then(|index| {
                                let range = app
                                    .text_tab(index)
                                    .ok_or_else(|| {
                                        Failure::new(
                                            ErrorCode::StaleRevision,
                                            "Text editor was closed",
                                        )
                                    })?
                                    .editor
                                    .read(cx)
                                    .selected_range();
                                Ok(Value::Edited {
                                    document: app.plugin_document_version(index)?,
                                    selection: TextRange {
                                        start: range.start,
                                        end: range.end,
                                    },
                                })
                            })
                    })
                    .unwrap_or_else(|_| {
                        Err(Failure::new(
                            ErrorCode::StaleRevision,
                            "Editor window was closed",
                        ))
                    });
                completion.finish(result);
            });
            cx.notify();
            Ok(())
        })();
        if let Err(error) = result {
            request.finish(Err(error));
        }
    }
}

/// Check replacement and result offsets without normalizing a malformed request into a different edit.
fn validate_edit(
    before: &str,
    range: &TextRange,
    text: &str,
    selection: &TextRange,
) -> Result<(), Failure> {
    if text.len() > 1024 * 1024 {
        return Err(Failure::new(
            ErrorCode::LimitExceeded,
            "Replacement exceeds 1 MiB",
        ));
    }
    if before.get(range.start..range.end).is_none() {
        return Err(Failure::new(
            ErrorCode::InvalidRequest,
            "Replacement range is outside UTF-8 boundaries",
        ));
    }
    let length = before
        .len()
        .checked_sub(range.end - range.start)
        .and_then(|length| length.checked_add(text.len()))
        .ok_or_else(|| Failure::new(ErrorCode::LimitExceeded, "Result length overflow"))?;
    let boundary = |offset: usize| {
        if offset <= range.start {
            before.is_char_boundary(offset)
        } else if offset <= range.start + text.len() {
            text.is_char_boundary(offset - range.start)
        } else {
            before.is_char_boundary(offset - text.len() + range.end - range.start)
        }
    };
    if selection.start > selection.end
        || selection.end > length
        || !boundary(selection.start)
        || !boundary(selection.end)
    {
        return Err(Failure::new(
            ErrorCode::InvalidRequest,
            "Result selection is outside UTF-8 boundaries",
        ));
    }
    Ok(())
}

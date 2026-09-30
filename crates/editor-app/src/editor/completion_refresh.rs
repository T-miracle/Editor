//! Refresh completion after the engine's deletion path without replacing its editing logic.

use super::*;
use crate::language::completion::identifier_prefix;
use lsp_types::{CompletionContext, CompletionResponse, CompletionTriggerKind};

/// Capture the open menu, then request against the document after native deletion finishes.
pub(super) fn schedule(
    editor: &Entity<EditorState>,
    popup: &Rc<CompletionPopupState>,
    window: &mut Window,
    cx: &mut App,
) {
    let state = editor.read(cx);
    if !state.is_editable() || !state.focus_handle(cx).is_focused(window) {
        return;
    }
    let Some(provider) = state.lsp().completion_provider.clone() else {
        return;
    };
    let menu = state.completion_menu_state().clone();
    if !menu.open {
        return;
    }
    let before = state.text().clone();
    let editor = editor.clone();
    let popup = popup.clone();
    window.defer(cx, move |window, cx| {
        editor.update(cx, |state, cx| {
            if state.text() == &before || !state.focus_handle(cx).is_focused(window) {
                return;
            }
            let source = state.text().to_string();
            let cursor = state.cursor();
            let query = identifier_prefix(&source, cursor);
            let start = cursor - query.len();
            // Retain the rows while loading; insertion waits for fresh document edits.
            state.present_completion_items(start, query, menu.items, cx);
            let revision = state.completion_menu_state().revision();
            popup.refresh_revision.set(Some(revision));
            let response = provider.completions(
                state.text(),
                cursor,
                CompletionContext {
                    trigger_kind: CompletionTriggerKind::INVOKED,
                    trigger_character: None,
                },
                window,
                cx,
            );
            let text = state.text().clone();
            cx.spawn_in(window, async move |editor, cx| {
                let response = response.await;
                let _ = editor.update_in(cx, |state, window, cx| {
                    // Escape, tab switches, further edits and newer requests invalidate this response.
                    if popup.refresh_revision.get() != Some(revision)
                        || !state.completion_menu_state().open
                        || state.completion_menu_state().revision() != revision
                        || state.text() != &text
                        || state.cursor() != cursor
                        || !state.focus_handle(cx).is_focused(window)
                        || !state
                            .lsp()
                            .completion_provider
                            .as_ref()
                            .is_some_and(|current| Rc::ptr_eq(current, &provider))
                    {
                        return;
                    }
                    popup.refresh_revision.set(None);
                    match response {
                        Ok(response) => {
                            let items = match response {
                                CompletionResponse::Array(items) => items,
                                CompletionResponse::List(list) => list.items,
                            };
                            let query = identifier_prefix(&source, cursor);
                            state.present_completion_items(start, query, items, cx);
                        }
                        Err(error) => {
                            tracing::warn!(%error, "completion refresh after deletion failed");
                            state.dismiss_completion_overlay(cx);
                        }
                    }
                });
            })
            .detach();
        });
    });
}

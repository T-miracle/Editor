//! Schedules revision-bound syntax diagnostics and navigation for open documents.

use crate::*;
use gpui_base::input::{Diagnostic, DiagnosticEntry};
use std::sync::Mutex;

#[cfg(test)]
mod tests;

/// A cancelled task cannot publish into a newer revision or replacement plugin.
#[derive(Default)]
pub(crate) struct DocumentDiagnostics {
    parser: Arc<Mutex<crate::language::diagnostics::SyntaxDiagnostics>>,
    task: Option<gpui_kit::Task<()>>,
    generation: u64,
    /// Keep independent producers from overwriting one another's latest results.
    syntax: Vec<Diagnostic>,
    semantic: Vec<Diagnostic>,
}

impl EditorApp {
    /// Coalesce typing bursts while clearing markers from the previous text immediately.
    pub(crate) fn refresh_syntax_diagnostics(
        &mut self,
        editor_id: gpui_kit::EntityId,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.owns_editor_id(editor_id))
        else {
            return;
        };
        let path = self.tabs[index].path().to_path_buf();
        let language = crate::language::providers::language_for_path(&path);
        let server = language
            .as_ref()
            .and_then(|language| self.language_servers.get(language))
            .cloned();
        let Some(tab) = self.tabs[index].text.as_mut() else {
            return;
        };
        tab.diagnostics.task = None;
        tab.diagnostics.syntax.clear();
        tab.diagnostics.semantic.clear();
        tab.diagnostics.generation = tab.diagnostics.generation.wrapping_add(1);
        let generation = tab.diagnostics.generation;
        let plugin_generation = self.plugin_loading_generation;
        let revision = tab.session.revision();
        let editor = tab.editor.clone();
        editor.update(cx, |state, cx| {
            if let Some(diagnostics) = state.diagnostics_mut() {
                diagnostics.clear();
            }
            state.clear_diagnostic_popover(cx);
            cx.notify();
        });
        let Some(language) = language else {
            tab.diagnostics.parser = Default::default();
            cx.notify();
            return;
        };
        let parser = tab.diagnostics.parser.clone();
        // Capture the UI-owned lifetime before any debounce or executor hop can outlive the tab.
        let document = server.as_ref().and_then(|server| {
            language_navigation::file_uri(&path).map(|uri| server.open_document(uri))
        });
        tab.diagnostics.task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(180))
                .await;
            // Snapshot only after debounce; a cancelled timer never copies the document.
            let Ok(Some(text)) = this.update(cx, |app, cx| {
                app.tabs
                    .iter()
                    .filter_map(|file| file.text.as_ref())
                    .find(|tab| tab.editor.entity_id() == editor_id)
                    .filter(|tab| {
                        tab.session.revision() == revision
                            && tab.diagnostics.generation == generation
                    })
                    .map(|tab| tab.editor.read(cx).text().clone())
            }) else {
                return;
            };
            let source = Arc::new(text.to_string());
            let syntax_text = text.clone();
            let diagnostics = cx
                .background_executor()
                .spawn(async move { parser.lock().unwrap().check(&language, &syntax_text) })
                .await;
            let _ = this.update(cx, |app, cx| {
                app.apply_syntax_diagnostics(
                    editor_id,
                    revision,
                    generation,
                    plugin_generation,
                    diagnostics,
                    cx,
                );
            });
            let (Some(server), Some(document)) = (server, document) else {
                return;
            };
            loop {
                let worker = server.clone();
                let document = document.clone();
                let source = source.clone();
                // Server startup and its serialized connection may wait; keep that
                // blocking work off both the UI and the shared parsing executor.
                let result = cx
                    .background_executor()
                    .scheduler_executor()
                    .spawn_dedicated(move |_| async move {
                        worker.diagnostics_for(document, source.as_str())
                    })
                    .await;
                let diagnostics = match result {
                    Ok(diagnostics) => diagnostics
                        .map(|items| crate::language::diagnostics::from_lsp(&text, items)),
                    Err(error) => {
                        tracing::warn!(%error, "language diagnostics failed");
                        break;
                    }
                };
                let current = this
                    .update(cx, |app, cx| {
                        let current =
                            app.tabs
                                .iter()
                                .filter_map(|file| file.text.as_ref())
                                .any(|tab| {
                                    tab.editor.entity_id() == editor_id
                                        && tab.session.revision() == revision
                                        && tab.diagnostics.generation == generation
                                })
                                && app.plugin_loading_generation == plugin_generation;
                        if current && let Some(diagnostics) = diagnostics {
                            app.apply_semantic_diagnostics(editor_id, diagnostics, cx);
                        }
                        current
                    })
                    .unwrap_or(false);
                if !current {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(400))
                    .await;
            }
        }));
        cx.notify();
    }

    /// Atomically replace markers only if both their document and grammar are still current.
    fn apply_syntax_diagnostics(
        &mut self,
        editor_id: gpui_kit::EntityId,
        revision: u64,
        generation: u64,
        plugin_generation: u64,
        diagnostics: Vec<Diagnostic>,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self
            .tabs
            .iter_mut()
            .filter_map(|file| file.text.as_mut())
            .find(|tab| tab.editor.entity_id() == editor_id)
        else {
            return;
        };
        if tab.session.revision() != revision
            || tab.diagnostics.generation != generation
            || self.plugin_loading_generation != plugin_generation
        {
            return;
        }
        tab.diagnostics.syntax = diagnostics;
        self.publish_document_diagnostics(editor_id, cx);
    }

    /// A server's empty publication clears semantic errors while retaining syntax fallback.
    fn apply_semantic_diagnostics(
        &mut self,
        editor_id: gpui_kit::EntityId,
        diagnostics: Vec<Diagnostic>,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self
            .tabs
            .iter_mut()
            .filter_map(|file| file.text.as_mut())
            .find(|tab| tab.editor.entity_id() == editor_id)
        else {
            return;
        };
        if tab.diagnostics.semantic == diagnostics {
            return;
        }
        tab.diagnostics.semantic = diagnostics;
        self.publish_document_diagnostics(editor_id, cx);
    }

    /// Publish a sorted union; prefer the server's precise explanation for overlapping errors.
    fn publish_document_diagnostics(&self, editor_id: gpui_kit::EntityId, cx: &mut Context<Self>) {
        let Some(tab) = self
            .tabs
            .iter()
            .filter_map(|file| file.text.as_ref())
            .find(|tab| tab.editor.entity_id() == editor_id)
        else {
            return;
        };
        let mut diagnostics = tab.diagnostics.semantic.clone();
        diagnostics.extend(
            tab.diagnostics
                .syntax
                .iter()
                .filter(|syntax| {
                    !tab.diagnostics.semantic.iter().any(|semantic| {
                        semantic.severity == gpui_base::input::DiagnosticSeverity::Error
                            && semantic.range.start < syntax.range.end
                            && syntax.range.start < semantic.range.end
                    })
                })
                .cloned(),
        );
        diagnostics.sort_by_key(|diagnostic| (diagnostic.range.start, diagnostic.range.end));
        diagnostics.dedup();
        tab.editor.update(cx, |state, cx| {
            let text = state.text().clone();
            if let Some(set) = state.diagnostics_mut() {
                set.reset(&text);
                set.extend(diagnostics);
            }
            state.clear_diagnostic_popover(cx);
            cx.notify();
        });
        let _ = self.editor_panel.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    /// Notify the already authorized server after a successful disk save.
    pub(crate) fn notify_language_document_saved(
        &self,
        path: &Path,
        source: String,
        cx: &mut Context<Self>,
    ) {
        let language = editor::language_for_path(path);
        let (Some(server), Some(uri)) = (
            self.language_servers.get(&language).cloned(),
            language_navigation::file_uri(path),
        ) else {
            return;
        };
        let Some(document) = server.document(&uri) else {
            return;
        };
        cx.spawn(async move |_, cx| {
            let result = cx
                .background_executor()
                .scheduler_executor()
                .spawn_dedicated(
                    move |_| async move { server.document_saved_for(document, source) },
                )
                .await;
            if let Err(error) = result {
                tracing::warn!(%error, "language server save notification failed");
            }
        })
        .detach();
    }

    /// Recreate parser caches after a package changes, then analyze every open document.
    pub(crate) fn reset_syntax_diagnostics(&mut self, cx: &mut Context<Self>) {
        let editors: Vec<_> = self
            .tabs
            .iter_mut()
            .filter_map(|file| file.text.as_mut())
            .map(|tab| {
                tab.diagnostics.parser = Default::default();
                tab.editor.entity_id()
            })
            .collect();
        for editor in editors {
            self.refresh_syntax_diagnostics(editor, cx);
        }
    }

    /// Jump to the next or previous marker, including inside a folded block.
    pub(crate) fn navigate_syntax_error(
        &mut self,
        backwards: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_text_tab_index().is_none() {
            return;
        }
        let state = self.editor.read(cx);
        let entries: Vec<_> = state
            .diagnostics()
            .into_iter()
            .flat_map(|set| set.iter())
            .cloned()
            .collect();
        let Some(entry) = next_error(&entries, state.cursor(), backwards) else {
            return;
        };
        let entry = entry.clone();
        // Retire a type-hover request before presenting the keyboard-selected error.
        self.dismiss_pointer_hover(cx);
        self.pointer_hover_suppressed = None;
        self.pointer_hover_symbol = Some(entry.range.clone());
        self.editor.update(cx, |state, cx| {
            let position = state.text().offset_to_position(entry.range.start);
            state.unfold_at(position, cx);
            state.set_cursor_position(position, window, cx);
            state.clear_hover_state(cx);
            state.present_diagnostic(entry, cx);
        });
        self.editor.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    /// The count is scoped to the active document, just like the F8 action.
    pub(crate) fn render_syntax_error_indicator(&self, cx: &Context<Self>) -> impl IntoElement {
        let count = self
            .editor
            .read(cx)
            .diagnostics()
            .map_or(0, |set| set.len());
        // Recovery diagnostics are capped; make the displayed count honest about that limit.
        let capped = self.active_text_tab_index().is_some_and(|index| {
            self.tabs[index]
                .text
                .as_ref()
                .expect("active text capability")
                .diagnostics
                .syntax
                .len()
                == crate::language::diagnostics::MAX_DIAGNOSTICS
        });
        let display_count = if capped {
            format!("{count}+")
        } else {
            count.to_string()
        };
        let label = t!("diagnostics.count", count = display_count).to_string();
        let tooltip = t!("diagnostics.navigate").to_string();
        div()
            .id("syntax-error-indicator")
            .debug_selector(|| "syntax-error-indicator".into())
            .text_color(cx.theme().danger)
            .cursor_pointer()
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            .on_click(
                cx.listener(|app, _, window, cx| app.navigate_syntax_error(false, window, cx)),
            )
            .child(label)
    }
}

/// Wrap navigation without selecting the marker that is already under the caret.
fn next_error(
    entries: &[DiagnosticEntry],
    cursor: usize,
    backwards: bool,
) -> Option<&DiagnosticEntry> {
    if backwards {
        entries
            .iter()
            .rev()
            .find(|entry| entry.range.start < cursor)
            .or_else(|| entries.last())
    } else {
        entries
            .iter()
            .find(|entry| entry.range.start > cursor)
            .or_else(|| entries.first())
    }
}

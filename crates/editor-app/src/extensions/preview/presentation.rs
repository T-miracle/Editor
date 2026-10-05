//! The selected provider submits the complete center tree; the host borrows native editor state.
use super::*;
use gpui_kit::{AnyElement, Styled};

impl EditorApp {
    /// Stable contribution identity, independent of any guest's display modes or icons.
    pub(crate) fn editor_preview_owner_key(
        &self,
        preview: &Entity<ExtensionPanel>,
        cx: &App,
    ) -> Option<String> {
        let panel = preview.read(cx);
        Some(format!(
            "{}/{}",
            panel.active.as_ref()?,
            panel.surface_id.as_ref()?
        ))
    }

    /// Mount the authorized layout without replacing text, caret, selection or undo history.
    pub(crate) fn render_editor_preview_body(
        &self,
        preview: Entity<ExtensionPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.active_text_tab_index().is_some()
            && let Some(error) = preview.read(cx).preview_error.clone()
        {
            // Transport failure removes derived content, never the existing editable source.
            // This native banner keeps the reason visible while the next smaller snapshot recovers.
            return v_flex()
                .size_full()
                .min_h_0()
                .child(
                    div()
                        .debug_selector(|| "plugin-preview-error".into())
                        .p_2()
                        .child(error),
                )
                .child(self.render_native_editor(window, cx))
                .into_any_element();
        }
        if preview.read(cx).renderable_document().is_none()
            && self.active_text_tab_index().is_some()
        {
            // Native input is available before the provider's first background parse finishes.
            return self.render_native_editor(window, cx);
        }
        let has_editor = preview
            .read(cx)
            .renderable_document()
            .is_some_and(|scene| scene.active_native_editor().is_some());
        let enabled = self.editor_preview_sync_enabled(&preview, cx);
        preview.update(cx, |panel, cx| {
            panel.viewport_sync_enabled = enabled;
            if !has_editor {
                panel.source_viewport.withdraw();
            } else if !enabled {
                panel.source_viewport.reset();
            }
            if let Some(view) = &panel.native_ui {
                view.update(cx, |view, cx| view.set_viewport_enabled(enabled, cx));
            }
        });
        if !has_editor && self.editor.focus_handle(cx).is_focused(window) {
            window.blur(cx);
        }
        div()
            .debug_selector(|| "editor-plugin-layout".into())
            .size_full()
            .min_h_0()
            .min_w_0()
            .child(preview)
            .into_any_element()
    }
}

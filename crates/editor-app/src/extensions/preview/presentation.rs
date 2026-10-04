//! Optional presentation controls are shared by every authorized editor-local preview.
use super::*;
use gpui_kit::{AnyElement, SharedString, Styled, prelude::FluentBuilder as _};
use protocol::PreviewMode;

impl EditorApp {
    /// Viewport identity does not require an optional set of presentation icons.
    pub(super) fn editor_preview_owner_key(
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

    /// A mode preference applies only while this installed contribution still offers all three icons.
    pub(super) fn editor_preview_key(
        &self,
        preview: &Entity<ExtensionPanel>,
        cx: &App,
    ) -> Option<String> {
        let panel = preview.read(cx);
        if panel.mode_icons.iter().any(Option::is_none) {
            return None;
        }
        Some(format!(
            "{}/{}",
            panel.active.as_ref()?,
            panel.surface_id.as_ref()?
        ))
    }

    /// Resolve the workspace preference, falling back to split when the contribution is unavailable.
    pub(crate) fn editor_preview_mode(
        &self,
        preview: &Entity<ExtensionPanel>,
        cx: &App,
    ) -> PreviewMode {
        self.editor_preview_key(preview, cx)
            .and_then(|key| self.session_state.editor_preview_modes.get(&key).copied())
            .unwrap_or_default()
    }

    /// Layout never replaces the source entity, text, selection or undo stack.
    pub(crate) fn render_editor_preview_body(
        &self,
        source: AnyElement,
        preview: Entity<ExtensionPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let source = self.render_source_viewport_probe(source, preview.clone(), cx);
        match self.editor_preview_mode(&preview, cx) {
            PreviewMode::Source => {
                let overlay = preview.update(cx, |panel, cx| panel.source_overlay(window, cx));
                div()
                    .size_full()
                    .relative()
                    .flex()
                    .flex_col()
                    .child(source)
                    .when_some(overlay, |body, overlay| {
                        body.child(
                            div()
                                .debug_selector(|| "editor-source-plugin-overlay".into())
                                .absolute()
                                .inset_0()
                                .child(overlay),
                        )
                    })
                    .into_any_element()
            }
            PreviewMode::Split => self.render_editor_preview_split(source, preview, cx),
            PreviewMode::Preview => {
                // Tab activation and startup restoration can defer source focus until after a mode click.
                // Reclaim that hidden input target when the actual preview-only body is drawn.
                if self.editor.focus_handle(cx).is_focused(window) {
                    window.blur(cx);
                }
                v_flex()
                    .debug_selector(|| "editor-preview-pane".into())
                    .size_full()
                    .min_h_0()
                    .overflow_hidden()
                    .bg(cx.theme().background)
                    .child(preview)
                    .into_any_element()
            }
        }
    }

    /// Keep source notifications live in every mode; only native presentation targets are hidden.
    fn set_editor_preview_mode(
        &mut self,
        mode: PreviewMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(preview) = self.active_editor_preview(cx) else {
            return;
        };
        let Some(key) = self.editor_preview_key(&preview, cx) else {
            return;
        };
        self.session_state.editor_preview_modes.insert(key, mode);
        if mode == PreviewMode::Source {
            preview.update(cx, |panel, cx| {
                // The same native view can carry an open dialog/menu into the source overlay.
                if panel
                    .current_document()
                    .is_none_or(|document| document.dialog.is_none() && document.menu.is_none())
                {
                    panel.native_ui = None;
                }
                cx.notify();
            });
        } else if mode == PreviewMode::Preview && self.editor.focus_handle(cx).is_focused(window) {
            // A hidden editor must not remain the target of typing or an in-flight native key event.
            window.blur(cx);
        }
        self.persist_session();
        // Dock content has its own render cache, independently of the surrounding status bar.
        self.editor_panel.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    /// Place package artwork after the existing left tool group, using Base button activation and focus.
    pub(crate) fn render_editor_preview_controls(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let preview = self.active_editor_preview(cx)?;
        let has_modes = self.editor_preview_key(&preview, cx).is_some();
        let has_viewport = preview
            .read(cx)
            .current_document()
            .is_some_and(|scene| scene.editor_viewport.is_some());
        if !has_modes && !has_viewport {
            return None;
        }
        let selected = self.editor_preview_mode(&preview, cx);
        let selected_style = component_styles(cx, ThemeComponent::PanelToggle).selected;
        let mut controls = h_flex().tab_group().items_center().gap_1().child(
            div()
                .debug_selector(|| "editor-preview-mode-separator".into())
                .w(px(1.))
                .h(px(14.))
                .flex_shrink_0()
                .bg(cx.theme().border)
                .mx(px(4.)),
        );
        if has_modes {
            for (index, mode, key, label) in [
                (
                    0,
                    PreviewMode::Source,
                    "editor-preview-source-mode",
                    t!("preview.source_mode").to_string(),
                ),
                (
                    1,
                    PreviewMode::Split,
                    "editor-preview-split-mode",
                    t!("preview.split_mode").to_string(),
                ),
                (
                    2,
                    PreviewMode::Preview,
                    "editor-preview-preview-mode",
                    t!("preview.preview_mode").to_string(),
                ),
            ] {
                let icon = preview.read(cx).mode_icons[index].as_deref()?;
                let button = Button::new(SharedString::from(key))
                    .icon(Icon::default().data(icon))
                    .small()
                    .compact()
                    .ghost()
                    .accessibility_label(label.clone())
                    .tooltip(label)
                    .when(selected == mode, |button| {
                        button
                            .bg(selected_style.background.unwrap_or(cx.theme().list_active))
                            .text_color(selected_style.foreground.unwrap_or(cx.theme().foreground))
                    })
                    .on_click(cx.listener(move |app, _, window, cx| {
                        app.set_editor_preview_mode(mode, window, cx)
                    }));
                controls = controls.child(div().debug_selector(move || key.into()).child(button));
            }
        }
        if has_viewport {
            let enabled = self.editor_preview_sync_enabled(&preview, cx);
            let label = t!("preview.synchronized_scroll").to_string();
            let toggle = Button::new("editor-preview-sync-scroll")
                .icon(Icon::default().data(include_bytes!("../../../assets/icons/sync-scroll.svg")))
                .small()
                .compact()
                .ghost()
                .disabled(selected != PreviewMode::Split)
                .accessibility_label(label.clone())
                .tooltip(label)
                .when(enabled, |button| {
                    button
                        .bg(selected_style.background.unwrap_or(cx.theme().list_active))
                        .text_color(selected_style.foreground.unwrap_or(cx.theme().foreground))
                })
                .on_click(cx.listener(|app, _, _, cx| {
                    let Some(preview) = app.active_editor_preview(cx) else {
                        return;
                    };
                    if app.editor_preview_mode(&preview, cx) != PreviewMode::Split {
                        return;
                    }
                    let Some(key) = app.editor_preview_owner_key(&preview, cx) else {
                        return;
                    };
                    let enabled = app.editor_preview_sync_enabled(&preview, cx);
                    app.session_state.editor_preview_sync.insert(key, !enabled);
                    preview.update(cx, |panel, cx| {
                        panel.source_viewport.reset();
                        panel.viewport_sync_enabled = !enabled;
                        if let Some(view) = &panel.native_ui {
                            view.update(cx, |view, cx| view.set_viewport_enabled(!enabled, cx));
                        }
                    });
                    app.persist_session();
                    app.editor_panel.update(cx, |_, cx| cx.notify());
                    cx.notify();
                }));
            controls = controls.child(
                div()
                    .debug_selector(|| "editor-preview-sync-scroll".into())
                    .child(toggle),
            );
        }
        Some(controls.into_any_element())
    }
}

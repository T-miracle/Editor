//! Compose the existing native editor, popovers and input gestures without replacing its session.
use super::language_for_path;
use crate::*;
use gpui_kit::component::WindowExt as _;

impl EditorApp {
    /// Borrow the current native text entity for either a fallback body or an authorized layout node.
    pub(crate) fn render_native_editor(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let style = component_styles(cx, ThemeComponent::Editor).base;
        // Modal surfaces own the window until dismissed; ordinary floating
        // panels can remain below the raised definition details layer.
        let hover_enabled = self.explorer_edit.is_none()
            && self.explorer_delete.is_none()
            && !window.has_active_dialog(cx)
            && !window.has_active_sheet(cx);
        // The menu callback runs inside an editor update, so snapshot its state now.
        let (enabled, editable, has_definition, has_code_actions) = {
            let editor = self.editor.read(cx);
            let presentation = editor.presentation();
            (
                !presentation.is_disabled(),
                presentation.is_editable(),
                editor.lsp().definition_provider.is_some(),
                !editor.lsp().code_action_providers.is_empty(),
            )
        };
        let app = cx.entity().downgrade();
        let language = self
            .active_path
            .as_deref()
            .map(language_for_path)
            .unwrap_or_default();
        let can_format = editable && self.language_edits.formatters.contains_key(&language);
        let can_rename = editable
            && self
                .language_servers
                .get(&language)
                .is_some_and(|server| server.provides_editing());
        // Preserve the native editor and all its popovers while a plugin adds a sibling preview.
        div()
            .debug_selector(|| "editor-source-pane".into())
            // Only the host-rendered native editor admits document shortcuts inside a guest layout.
            .key_context("NativeEditorSource")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .relative()
            .when_some(
                self.render_editor_source_toolbar(window, cx),
                |source, toolbar| source.child(toolbar),
            )
            // Capture selection presses before the base editor collapses them.
            .when(hover_enabled, |view| {
                view.capture_any_mouse_down(cx.listener(Self::text_drag_press))
                    .capture_any_mouse_down(cx.listener(Self::linked_pointer_down))
                    .capture_key_down(cx.listener(Self::linked_navigation_key))
                    .capture_action(cx.listener(Self::linked_history_undo))
                    .capture_action(cx.listener(Self::linked_history_redo))
                    .capture_action(cx.listener(Self::text_drag_escape))
                    .capture_action(cx.listener(Self::cancel_rename_action))
                    .capture_action(cx.listener(Self::paste_plugin_images))
                    .capture_action(cx.listener(Self::linked_paste))
                    .capture_action(cx.listener(Self::linked_cut))
                    .capture_action(cx.listener(Self::linked_backspace))
                    .capture_action(cx.listener(Self::linked_delete))
                    .on_drag_move(cx.listener(Self::image_drag_move))
            })
            // Register drag listeners before the editor's own selection listeners.
            .child(self.render_text_drag_events(cx))
            .when(hover_enabled, |source| {
                source.child(self.render_image_drag_events(cx))
            })
            .on_mouse_move(cx.listener(Self::editor_pointer_move))
            .on_mouse_up(MouseButton::Middle, move |event, window, cx| {
                let position = event.position;
                let app = app.clone();
                cx.stop_propagation();
                window.defer(cx, move |window, cx| {
                    // Reuse the editor's own hit testing to place the caret under the click.
                    let modifiers = Modifiers::default();
                    window.dispatch_event(
                        PlatformInput::MouseDown(MouseDownEvent {
                            button: MouseButton::Left,
                            position,
                            modifiers,
                            click_count: 1,
                            first_mouse: false,
                        }),
                        cx,
                    );
                    window.dispatch_event(
                        PlatformInput::MouseUp(MouseUpEvent {
                            button: MouseButton::Left,
                            position,
                            modifiers,
                            click_count: 1,
                        }),
                        cx,
                    );
                    let _ = app.update(cx, |app, cx| {
                        app.request_definition(Some(position), window, cx);
                    });
                });
            })
            .child(
                super::popovers::render(
                    &self.editor,
                    &self.completion_popup,
                    &self.definition_popup_focus,
                    style,
                    hover_enabled,
                    window,
                    cx,
                )
                .unwrap_or_else(|| {
                    Editor::new(&self.editor)
                        // Preserve live edit restrictions when the styled component renders.
                        .readonly(!editable)
                        .disabled(!enabled)
                        .context_menu(move |menu, _, cx| {
                            // Route the menu action through the same fresh LSP request as F12.
                            menu.menu_with_disabled(
                                t!("editor.go_to_definition").to_string(),
                                !(enabled && has_definition),
                                Box::new(NavigateToDefinition),
                            )
                            .menu_with_disabled(
                                t!("editor.code_actions").to_string(),
                                !(editable && has_code_actions),
                                Box::new(gpui_base::input::ToggleCodeActions),
                            )
                            .separator()
                            .menu_with_disabled(
                                t!("editor.format_document").to_string(),
                                !can_format,
                                Box::new(FormatDocument),
                            )
                            .menu_with_disabled(
                                t!("editor.rename_symbol").to_string(),
                                !can_rename,
                                Box::new(RenameSymbol),
                            )
                            .separator()
                            // Cut and Copy validate the live selection when their actions run.
                            .menu_with_disabled(
                                t!("editor.cut").to_string(),
                                !editable,
                                Box::new(gpui_base::input::Cut),
                            )
                            .menu_with_disabled(
                                t!("editor.copy").to_string(),
                                !enabled,
                                Box::new(gpui_base::input::Copy),
                            )
                            .menu_with_disabled(
                                t!("editor.paste").to_string(),
                                !(editable && cx.read_from_clipboard().is_some()),
                                Box::new(gpui_base::input::Paste),
                            )
                            .separator()
                            .menu(
                                t!("editor.select_all").to_string(),
                                Box::new(gpui_base::input::SelectAll),
                            )
                        })
                        .bordered(false)
                        .p_0()
                        .size_full()
                        .min_h_0()
                        .bg(style.background.unwrap_or(cx.theme().background))
                        .text_color(style.foreground.unwrap_or(cx.theme().foreground))
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_size(
                            style
                                .font_size_px
                                .map(px)
                                .unwrap_or(cx.theme().mono_font_size),
                        )
                        .into_any_element()
                }),
            )
            // Paint the drop caret after the text, using the current editor layout.
            .child(self.render_text_drag_caret(cx))
            .children(self.render_linked_input())
            .children(self.render_rename_prompt(cx))
            .into_any_element()
    }
}

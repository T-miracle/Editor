//! Owns window chrome, dock panels, settings UI, and shell rendering.

use crate::*;

/// Share one document scope between default keys and unbound shortcut metadata.
/// Embedded native source panes receive document commands without granting surrounding plugin controls that authority.
pub(crate) const DOCUMENT_COMMAND_CONTEXT: &str =
    "EditorShell && !PluginSurface || EditorShell > NativeEditorSource";

/// Keep repeated window/fixture initialization from superseding already applied user bindings.
struct ShellBindingsInitialized;
impl gpui_kit::Global for ShellBindingsInitialized {}

/// Register shell commands in the current app after Base initialization.
/// Startup and native shell fixtures share this table, including the plugin-surface scope boundary.
pub(crate) fn bind_editor_shell_keys(cx: &mut App) {
    if cx.has_global::<ShellBindingsInitialized>() {
        return;
    }
    // A plugin layout can embed the host's native source pane. Its descendant context admits
    // document commands without granting the surrounding guest controls those same shortcuts.
    let document_context = Some(DOCUMENT_COMMAND_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("shift-alt-f", FormatDocument, document_context),
        KeyBinding::new("f2", RenameSymbol, document_context),
        KeyBinding::new("ctrl-s", SaveDocument, document_context),
        KeyBinding::new("ctrl-shift-r", RefreshWorkspace, document_context),
        KeyBinding::new("ctrl-alt-t", ToggleTheme, Some("EditorShell")),
        KeyBinding::new("f12", NavigateToDefinition, document_context),
        KeyBinding::new("ctrl-i", ShowDefinitionDetails, document_context),
        // Error navigation follows the active document and wraps at either end.
        KeyBinding::new("f8", NextSyntaxError, document_context),
        KeyBinding::new("shift-f8", PreviousSyntaxError, document_context),
    ]);
    cx.set_global(ShellBindingsInitialized);
}

#[cfg(target_os = "windows")]
const WINDOWS_TIMER_RESOLUTION_MS: u32 = 1;

#[cfg(target_os = "windows")]
#[link(name = "winmm")]
unsafe extern "system" {
    fn timeBeginPeriod(period: u32) -> u32;
    fn timeEndPeriod(period: u32) -> u32;
}

#[cfg(target_os = "windows")]
pub(crate) struct WindowsTimerResolution {
    enabled: bool,
}

#[cfg(target_os = "windows")]
impl WindowsTimerResolution {
    pub(crate) fn enable_for_window_drag() -> Self {
        // GPUI's Win32 modal move loop uses a short SetTimer interval to keep
        // processing and painting while the OS owns the title-bar drag loop.
        let enabled = unsafe { timeBeginPeriod(WINDOWS_TIMER_RESOLUTION_MS) } == 0;
        Self { enabled }
    }
}

#[cfg(target_os = "windows")]
impl Drop for WindowsTimerResolution {
    fn drop(&mut self) {
        if self.enabled {
            unsafe {
                timeEndPeriod(WINDOWS_TIMER_RESOLUTION_MS);
            }
        }
    }
}

/// Draws the same window glyph and native control region as GPUI Kit's TitleBar.
fn title_bar_window_control(
    id: &'static str,
    icon: IconName,
    area: WindowControlArea,
    close: bool,
    cx: &App,
) -> impl IntoElement {
    let hover_foreground = if close {
        cx.theme().danger_foreground
    } else {
        cx.theme().secondary_foreground
    };
    let hover_background = if close {
        cx.theme().danger
    } else {
        cx.theme().secondary_hover
    };
    let active_background = if close {
        cx.theme().danger_active
    } else {
        cx.theme().secondary_active
    };
    div()
        .id(id)
        .flex()
        .w(px(34.))
        .h_full()
        .flex_shrink_0()
        .justify_center()
        .content_center()
        .items_center()
        .text_color(cx.theme().foreground)
        .hover(|style| style.bg(hover_background).text_color(hover_foreground))
        .active(|style| style.bg(active_background).text_color(hover_foreground))
        .when(cfg!(target_os = "windows"), |this| {
            this.window_control_area(area)
        })
        .when(!cfg!(target_os = "windows"), |this| {
            this.on_click(move |_, window, cx| {
                cx.stop_propagation();
                match area {
                    WindowControlArea::Min => window.minimize_window(),
                    WindowControlArea::Max => window.zoom_window(),
                    WindowControlArea::Close => {
                        window.dispatch_action(Box::new(extensions::QuitEditor), cx)
                    }
                    WindowControlArea::Drag => {}
                }
            })
        })
        .child(Icon::new(icon).small())
}

#[derive(Clone, Copy)]
pub(crate) enum EditorDockPanelKind {
    Explorer,
    Editor,
}

/// Keep the title bar and bottom toggle on the same theme-specific Explorer artwork.
fn explorer_panel_icon(cx: &App) -> Icon {
    Icon::default().data(if cx.theme().is_dark() {
        include_bytes!("../../assets/status-icons/explorer_dark.svg").as_slice()
    } else {
        include_bytes!("../../assets/status-icons/explorer_light.svg").as_slice()
    })
}

pub(crate) struct EditorDockPanel {
    parent: WeakEntity<EditorApp>,
    kind: EditorDockPanelKind,
    explorer_visible: Rc<Cell<bool>>,
    focus_handle: FocusHandle,
}

impl EditorDockPanel {
    pub(crate) fn new(
        parent: WeakEntity<EditorApp>,
        kind: EditorDockPanelKind,
        explorer_visible: Rc<Cell<bool>>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            parent,
            kind,
            explorer_visible,
            focus_handle: cx.focus_handle(),
        }
    }
}

impl EventEmitter<PanelEvent> for EditorDockPanel {}

impl Focusable for EditorDockPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl dock::BasePanel for EditorDockPanel {
    fn panel_name(&self) -> &'static str {
        match self.kind {
            EditorDockPanelKind::Explorer => "Explorer",
            EditorDockPanelKind::Editor => "Editor",
        }
    }

    fn closable(&self, _: &App) -> bool {
        false
    }

    fn zoomable(&self, _: &App) -> bool {
        false
    }

    fn visible(&self, _: &App) -> bool {
        match self.kind {
            EditorDockPanelKind::Explorer => self.explorer_visible.get(),
            EditorDockPanelKind::Editor => true,
        }
    }
}

impl DockPanel for EditorDockPanel {
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.kind {
            // Order explorer actions as locate, collapse all, expand all, then minimize.
            EditorDockPanelKind::Explorer => h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .font_normal()
                .child(
                    h_flex()
                        .flex_1()
                        .min_w(px(0.))
                        .items_center()
                        .gap_2()
                        .child(explorer_panel_icon(cx).small())
                        .child(div().truncate().child(t!("panel.explorer").to_string())),
                )
                .child(
                    div()
                        .id("explorer-reveal-active-file")
                        .debug_selector(|| "explorer-reveal-active-file".into())
                        .child(
                            Button::new("explorer-reveal")
                                // The custom target inherits the current title-bar text color.
                                .icon(Icon::default().path("icons/explorer-locate.svg"))
                                .tooltip(t!("explorer.reveal_active_file").to_string())
                                .accessibility_label(t!("explorer.reveal_active_file").to_string())
                                .small()
                                .compact()
                                .ghost()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    // Consume the title action so it cannot start a dock gesture.
                                    cx.stop_propagation();
                                    let _ = this.parent.update(cx, |app, cx| {
                                        app.reveal_active_file_in_explorer(window, cx);
                                    });
                                })),
                        ),
                )
                .child(
                    div()
                        .id("explorer-collapse-all")
                        .debug_selector(|| "explorer-collapse-all".into())
                        .child(
                            Button::new("explorer-collapse-all-button")
                                .icon(Icon::default().path("icons/explorer-collapse-all.svg"))
                                .tooltip(t!("explorer.collapse_all").to_string())
                                .accessibility_label(t!("explorer.collapse_all").to_string())
                                .small()
                                .compact()
                                .ghost()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    // Consume header actions so they cannot start a dock gesture.
                                    cx.stop_propagation();
                                    let _ = this.parent.update(cx, |app, cx| {
                                        app.set_all_explorer_directories_expanded(false, cx);
                                    });
                                })),
                        ),
                )
                .child(
                    div()
                        .id("explorer-expand-all")
                        .debug_selector(|| "explorer-expand-all".into())
                        .child(
                            Button::new("explorer-expand-all-button")
                                .icon(Icon::default().path("icons/explorer-expand-all.svg"))
                                .tooltip(t!("explorer.expand_all").to_string())
                                .accessibility_label(t!("explorer.expand_all").to_string())
                                .small()
                                .compact()
                                .ghost()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    cx.stop_propagation();
                                    let _ = this.parent.update(cx, |app, cx| {
                                        app.set_all_explorer_directories_expanded(true, cx);
                                    });
                                })),
                        ),
                )
                .child(
                    Button::new("explorer-hide")
                        .icon(Icon::new(IconName::WindowMinimize))
                        .tooltip(t!("panel.minimize_explorer").to_string())
                        .accessibility_label(t!("panel.minimize_explorer").to_string())
                        .small()
                        .compact()
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            let _ = this.parent.update(cx, |app, cx| {
                                // Use the existing visibility path to persist hiding and update the bottom toggle.
                                if app.explorer_visible {
                                    app.toggle_explorer(cx);
                                }
                            });
                        })),
                )
                .into_any_element(),
            EditorDockPanelKind::Editor => div()
                .child(t!("panel.editor").to_string())
                .into_any_element(),
        }
    }

    fn title_bar(&self, _: &App) -> bool {
        !matches!(self.kind, EditorDockPanelKind::Editor)
    }
}

impl Render for EditorDockPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let kind = self.kind;
        self.parent
            .update(cx, |app, cx| match kind {
                EditorDockPanelKind::Explorer => app.render_file_tree(cx).into_any_element(),
                EditorDockPanelKind::Editor => {
                    app.render_editor_panel(window, cx).into_any_element()
                }
            })
            .unwrap_or_else(|_| div().into_any_element())
    }
}

impl EditorApp {}

impl Render for EditorApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.pending_contribution_sync) {
            // The render frame provides the window needed to attach LSP providers to open tabs.
            self.sync_runtime_contributions(window, cx);
        }
        if self._bounds_subscription.is_none() {
            self._bounds_subscription =
                Some(cx.observe_window_bounds(window, |this, window, _cx| {
                    let bounds = window.bounds();
                    this.session_state.window_width = bounds.size.width / px(1.);
                    this.session_state.window_height = bounds.size.height / px(1.);
                    this.session_state.save();
                }));
        }
        // Open plugin-owned settings through the normal editor document path.
        self.sync_plugin_panels(window, cx);
        self.sync_plugin_documents(cx);
        self.sync_run_controls(window, cx);
        self.dispatch_editor_requests(window, cx);
        self.sync_outline(window, cx);
        if let Some(path) = self.pending_plugin_file.take() {
            self.open_file(path, window, cx);
        }
        // Discovery waits for healthy runtime restoration, then reuses native permission consent for this file.
        self.sync_bundled_first_use(window, cx);
        let cursor = self.editor.read(cx).cursor_position();

        let shell_style = component_styles(cx, ThemeComponent::AppShell).base;
        let title_bar_style = component_styles(cx, ThemeComponent::WindowTitleBar).base;
        let status_style = component_styles(cx, ThemeComponent::StatusBar).base;
        // Observe the exact entry chosen for this frame before confirmation can rebuild the same severity later.
        let plugin_indicator = self.plugin_indicator(cx);
        self.track_plugin_indicator_frame(plugin_indicator);
        v_flex()
            .id("editor-shell")
            .key_context("EditorShell")
            .relative()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    // A click outside the anchored card dismisses its transient details.
                    if this.plugin_popup.take().is_some() {
                        this.close_plugin_popup(window, cx);
                    }
                    if this.explorer_menu.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .on_action(cx.listener(Self::on_save_action))
            .on_action(
                cx.listener(|app, _: &app::shortcuts::OpenShortcuts, window, cx| {
                    app.open_shortcuts(window, cx);
                }),
            )
            .on_action(
                cx.listener(|this, _: &extensions::QuitEditor, _, cx| this.shutdown_plugins(cx)),
            )
            .on_action(
                cx.listener(|this, _: &extensions::ToggleExtensions, window, cx| {
                    this.toggle_extensions(window, cx)
                }),
            )
            .on_action(cx.listener(Self::on_refresh_action))
            .on_action(cx.listener(Self::on_toggle_theme_action))
            .on_action(cx.listener(Self::on_navigate_to_definition))
            .on_action(cx.listener(Self::on_show_definition_details))
            .on_action(cx.listener(Self::format_document_action))
            .on_action(cx.listener(Self::rename_symbol_action))
            .on_action(cx.listener(Self::toggle_outline))
            .on_action(cx.listener(|app, _: &NextSyntaxError, window, cx| {
                app.navigate_syntax_error(false, window, cx);
            }))
            .on_action(cx.listener(|app, _: &PreviousSyntaxError, window, cx| {
                app.navigate_syntax_error(true, window, cx);
            }))
            .size_full()
            .bg(shell_style.background.unwrap_or(cx.theme().background))
            .text_color(shell_style.foreground.unwrap_or(cx.theme().foreground))
            .child(
                h_flex()
                    .id("custom-title-bar")
                    .w_full()
                    .h(px(34.))
                    .items_center()
                    .bg(title_bar_style.background.unwrap_or(cx.theme().tab_bar))
                    .text_color(title_bar_style.foreground.unwrap_or(cx.theme().foreground))
                    .text_size(px(title_bar_style.font_size_px.unwrap_or(14.)))
                    .border_b_1()
                    .border_color(title_bar_style.border.unwrap_or(cx.theme().border))
                    .child(
                        h_flex()
                            .h_full()
                            .items_center()
                            .gap_2()
                            .px_3()
                            .child(
                                // Preserve the supplied mark's color in both editor themes.
                                gpui_kit::img(assets::APP_ICON_PATH)
                                    .size(px(24.))
                                    .flex_shrink_0(),
                            )
                            .child(div().text_sm().font_semibold().child(app::APP_NAME)),
                    )
                    .child(
                        div()
                            .id("title-bar-drag-region")
                            .debug_selector(|| "title-bar-drag-region".into())
                            .flex_1()
                            .h_full()
                            .window_control_area(WindowControlArea::Drag)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, _| this.titlebar_should_move = true),
                            )
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, _| this.titlebar_should_move = false),
                            )
                            .on_mouse_move(cx.listener(|this, _, window, _| {
                                if this.titlebar_should_move {
                                    this.titlebar_should_move = false;
                                    window.start_window_move();
                                }
                            })),
                    )
                    .child(self.render_run_controls(cx))
                    .child(self.render_extensions_button(cx))
                    .child(self.render_settings_dialog(cx))
                    .child(self.render_shortcuts_trigger(cx))
                    .child(self.render_shortcuts_menu_trigger(cx))
                    .child(self.render_window_controls(window, cx)),
            )
            .child(h_flex().flex_1().min_h_0().child(self.dock_area.clone()))
            .when_some(self.render_build_output(cx), |shell, panel| {
                shell.child(panel)
            })
            .when_some(self.render_debug_panel(cx), |shell, panel| {
                shell.child(panel)
            })
            .child(
                div()
                    .w_full()
                    .bg(status_style.background.unwrap_or(cx.theme().background))
                    .text_color(status_style.foreground.unwrap_or(cx.theme().foreground))
                    .text_size(px(status_style.font_size_px.unwrap_or(12.)))
                    .border_t_1()
                    .border_color(status_style.border.unwrap_or(cx.theme().border))
                    .child(
                        StatusBar::new()
                            .left(self.render_plugin_toolbar(explorer_panel_icon(cx), window, cx))
                            // Persistent document indicators stay in the footer; operation notices live in host messages.
                            .when(
                                self.active_text_tab_index().is_some()
                                    && self
                                        .editor
                                        .read(cx)
                                        .diagnostics()
                                        .is_some_and(|set| !set.is_empty()),
                                |bar| bar.right(self.render_syntax_error_indicator(cx)),
                            )
                            // Keep plugin indicators immediately before the cursor position.
                            .when_some(plugin_indicator, |bar, kind| {
                                bar.right(self.render_plugin_indicator(kind, cx))
                            })
                            .right(if self.active_text_tab_index().is_some() {
                                t!(
                                    "status.cursor",
                                    line = cursor.line + 1,
                                    column = cursor.character + 1
                                )
                                .to_string()
                            } else {
                                t!("status.cursor_empty").to_string()
                            }),
                    ),
            )
            .child(self.render_plugin_popup_blocker(cx))
            .when_some(self.shortcut_panel.as_ref(), |shell, panel| {
                shell.child(panel.clone())
            })
            .when_some(self.render_shortcuts_menu(), |shell, menu| {
                shell.child(menu)
            })
            // A pending leave decision covers the shell until the user keeps or stops the sessions.
            .when_some(self.render_leave_confirmation(cx), |shell, confirm| {
                shell.child(confirm)
            })
            .child(self.render_run_menu(window, cx))
            .child(self.render_plugin_popup(window, cx))
            .child(self.render_explorer_menu(window, cx))
            .when_some(self.file_view_menu.as_ref(), |body, menu| {
                body.child(menu.clone())
            })
            .when_some(self.tool_overflow.as_ref(), |body, menu| {
                body.child(menu.clone())
            })
            .child(self.render_explorer_edit(cx))
            .child(self.render_explorer_delete(cx))
            .child(self.render_file_transfer(cx))
            .when_some(self.notification.as_ref(), |this, notification| {
                // Center the card near the top, keeping a margin when the window is narrow.
                let width = px(380.).min((window.viewport_size().width - px(24.)).max(px(0.)));
                this.child(
                    div()
                        .absolute()
                        .left((window.viewport_size().width - width) / 2.)
                        .top(px(64.))
                        .w(width)
                        .child(notification.clone()),
                )
            })
            .when_some(self.definition_notice, |this, notice| {
                let left = px((notice.position.x / px(1.) - 28.).max(4.));
                let top = px((notice.position.y / px(1.) - 38.).max(4.));
                this.child(
                    div()
                        .absolute()
                        .left(left)
                        .top(top)
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().popover)
                        .text_color(cx.theme().foreground)
                        .text_sm()
                        .child(t!("editor.no_definition").to_string()),
                )
            })
    }
}

impl EditorApp {
    fn render_window_controls(&self, window: &Window, cx: &Context<Self>) -> impl IntoElement {
        let supported = window.window_controls();
        h_flex()
            .id("window-controls")
            .h_full()
            .items_center()
            .flex_shrink_0()
            .when(!cfg!(target_os = "macos") && supported.minimize, |this| {
                this.child(title_bar_window_control(
                    "minimize",
                    IconName::WindowMinimize,
                    WindowControlArea::Min,
                    false,
                    cx,
                ))
            })
            .when(!cfg!(target_os = "macos") && supported.maximize, |this| {
                this.child(title_bar_window_control(
                    if window.is_maximized() {
                        "restore"
                    } else {
                        "maximize"
                    },
                    if window.is_maximized() {
                        IconName::WindowRestore
                    } else {
                        IconName::WindowMaximize
                    },
                    WindowControlArea::Max,
                    false,
                    cx,
                ))
            })
            .when(!cfg!(target_os = "macos"), |this| {
                this.child(title_bar_window_control(
                    "close",
                    IconName::WindowClose,
                    WindowControlArea::Close,
                    true,
                    cx,
                ))
            })
    }
}

//! Renders the explorer, document tabs, and editor.

use crate::*;

#[derive(Clone)]
/// Carries a tab's identity while it is dragged in the tab strip.
struct EditorTabDrag {
    path: PathBuf,
    label: String,
}

impl Render for EditorTabDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let style = component_styles(cx, ThemeComponent::EditorTabDragPreview).base;
        div()
            .px(px(style.padding_x_px.unwrap_or(8.)))
            .py(px(style.padding_y_px.unwrap_or(4.)))
            .rounded(px(style.radius_px.unwrap_or(4.)))
            .text_size(px(style.font_size_px.unwrap_or(12.)))
            .bg(style.background.unwrap_or(cx.theme().primary))
            .text_color(style.foreground.unwrap_or(cx.theme().primary_foreground))
            .child(self.label.clone())
    }
}

impl EditorApp {
    pub(crate) fn render_file_tree(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let tree_style = component_styles(cx, ThemeComponent::ExplorerTree).base;
        let row_styles = component_styles(cx, ThemeComponent::ExplorerRow);
        let tree = tree(
            &self.tree_state,
            move |index, entry, selected, _window, cx| {
                let hover_view = view.clone();
                view.update(cx, |app, cx| {
                    let item = entry.item();
                    let row_id = item.id.clone();
                    let hovered = app
                        .hovered_tree_entry
                        .as_deref()
                        .is_some_and(|id| id == row_id.as_str());
                    let row_style = if selected {
                        row_styles.selected
                    } else if hovered {
                        row_styles.hover
                    } else {
                        row_styles.base
                    };
                    // Keep the last project selection visible but muted while an external tab is active.
                    let inactive_selection = selected
                        && app
                            .active_path
                            .as_ref()
                            .is_some_and(|path| !path.starts_with(app.workspace.root()));
                    let row_background = if inactive_selection {
                        cx.theme().list_hover
                    } else {
                        row_style.background.unwrap_or(cx.theme().background)
                    };
                    let row_border = if inactive_selection {
                        Some(cx.theme().border)
                    } else {
                        row_style.border
                    };
                    let is_folder = item.is_folder();
                    let icon = file_icon(
                        Path::new(item.id.as_str()),
                        is_folder,
                        &theme::active_theme(app.dark_theme),
                    );
                    let disclosure = if is_folder {
                        Icon::new(if entry.is_expanded() {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .xsmall()
                        .into_any_element()
                    } else {
                        div().size(px(12.)).into_any_element()
                    };
                    ListItem::new(index)
                        .w_full()
                        .bg(row_background)
                        .when(selected, |this| {
                            this.rounded(px(row_style.radius_px.unwrap_or(5.)))
                        })
                        .text_color(row_style.foreground.unwrap_or(cx.theme().foreground))
                        .py(px(row_style.padding_y_px.unwrap_or(0.3)))
                        .px(px(row_style.padding_x_px.unwrap_or(4.)))
                        .pl(px(14.) * entry.depth() + px(8.))
                        .when_some(row_border, |this, border| {
                            this.border_l_1().border_color(border)
                        })
                        .on_hover(move |is_hovered, _, cx| {
                            let row_id = row_id.clone();
                            let _ = hover_view.update(cx, |app, cx| {
                                if *is_hovered {
                                    app.hovered_tree_entry = Some(row_id.to_string());
                                    cx.notify();
                                } else if app.hovered_tree_entry.as_deref() == Some(row_id.as_str())
                                {
                                    app.hovered_tree_entry = None;
                                    cx.notify();
                                }
                            });
                        })
                        .child(
                            h_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .size(px(12.))
                                        .flex_shrink_0()
                                        .justify_center()
                                        .child(disclosure),
                                )
                                .child(div().size(px(16.)).flex_shrink_0().child(icon))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .truncate()
                                        .child(item.label.clone()),
                                ),
                        )
                        .on_click(cx.listener({
                            let item = item.clone();
                            move |this, _, window, cx| {
                                if !is_folder {
                                    this.open_file(PathBuf::from(item.id.as_str()), window, cx);
                                }
                            }
                        }))
                })
            },
        )
        .p_1()
        // Explorer and editor text share the user's live font size by default.
        .text_size(
            tree_style
                .font_size_px
                .map(px)
                .unwrap_or(typography::font_size(cx)),
        )
        .font_family(cx.theme().mono_font_family.clone())
        .flex_1()
        .min_h_0()
        .bg(tree_style.background.unwrap_or(cx.theme().background))
        .text_color(tree_style.foreground.unwrap_or(cx.theme().foreground));

        v_flex()
            .size_full()
            .min_h_0()
            .bg(tree_style.background.unwrap_or(cx.theme().background))
            .child(tree)
    }

    fn render_tabs(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tab_styles = component_styles(cx, ThemeComponent::EditorTab);
        let close_styles = component_styles(cx, ThemeComponent::EditorTabClose);
        let tabs = self.tabs.iter().map(|tab| {
            let path = tab.session.path().to_path_buf();
            let is_active = self.active_path.as_ref() == Some(&path);
            let is_external = !path.starts_with(self.workspace.root());
            let is_dirty = tab.session.is_dirty();
            let name = tab
                .session
                .file_name()
                .map(str::to_owned)
                .unwrap_or_else(|_| t!("editor.untitled").to_string());
            let icon = file_icon(&path, false, &theme::active_theme(self.dark_theme));
            let activate_path = path.clone();
            let close_path = path.clone();
            let middle_close_path = path.clone();
            let drop_path = path.clone();
            let drag_payload = EditorTabDrag {
                path: path.clone(),
                label: name.clone(),
            };
            // Keep the full document path in the tooltip even when the tab label is truncated.
            let tooltip_path = path.to_string_lossy().into_owned();
            // Both project and external files use the theme's selected underline color.
            let selected_color = tab_styles.selected.border.unwrap_or(cx.theme().primary);
            let separator_color = tab_styles.base.border.unwrap_or(cx.theme().border);
            let external_background = gpui_kit::rgb(0xfff4c2);
            h_flex()
                .id(format!("editor-tab:{}", path.to_string_lossy()))
                .relative()
                .h_full()
                .w(px(190.))
                .flex_shrink_0()
                .gap_2()
                .px(px(tab_styles.base.padding_x_px.unwrap_or(8.)))
                .text_size(px(if is_active {
                    tab_styles
                        .selected
                        .font_size_px
                        .or(tab_styles.base.font_size_px)
                } else {
                    tab_styles.base.font_size_px
                }
                .unwrap_or(14.)))
                .border_r_1()
                .border_color(separator_color)
                .when(is_active, |style| {
                    // Remove the right border before coloring the bottom border blue.
                    style.border_r_0().border_b_1().border_color(selected_color)
                })
                .bg(if is_external {
                    external_background.into()
                } else if is_active {
                    tab_styles
                        .selected
                        .background
                        .unwrap_or(cx.theme().background)
                } else {
                    tab_styles.base.background.unwrap_or(cx.theme().tab_bar)
                })
                .text_color(if is_external {
                    cx.theme().muted_foreground
                } else if is_active {
                    selected_color
                } else {
                    tab_styles
                        .base
                        .foreground
                        .unwrap_or(cx.theme().tab_foreground)
                })
                .hover(|style| {
                    style
                        .bg(if is_external {
                            external_background.into()
                        } else {
                            tab_styles.hover.background.unwrap_or(if is_active {
                                tab_styles
                                    .selected
                                    .background
                                    .unwrap_or(cx.theme().background)
                            } else {
                                tab_styles.base.background.unwrap_or(cx.theme().tab_bar)
                            })
                        })
                        .text_color(if is_external {
                            cx.theme().muted_foreground
                        } else if is_active {
                            selected_color
                        } else {
                            tab_styles.hover.foreground.unwrap_or(cx.theme().foreground)
                        })
                })
                .tooltip(move |window, cx| Tooltip::new(tooltip_path.clone()).build(window, cx))
                .tooltip_show_delay(Duration::from_millis(1_200))
                .child(div().size(px(16.)).flex_shrink_0().child(icon))
                // Draw the right separator independently so it never inherits blue.
                .when(is_active, |this| {
                    this.child(
                        div()
                            .absolute()
                            .right_0()
                            .top_0()
                            .bottom(px(1.))
                            .w(px(1.))
                            .bg(separator_color),
                    )
                })
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_sm()
                        .when(is_external, |style| style.italic())
                        .child(format!("{}{}", name, if is_dirty { " ●" } else { "" })),
                )
                .child(
                    div()
                        .id(format!("close-editor-tab:{}", path.to_string_lossy()))
                        .flex_shrink_0()
                        .rounded(px(close_styles.base.radius_px.unwrap_or(3.)))
                        .p_1()
                        .hover(|style| {
                            style.bg(close_styles
                                .hover
                                .background
                                .unwrap_or(cx.theme().list_hover))
                        })
                        .child(Icon::new(IconName::Close).xsmall())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.close_tab(close_path.clone(), window, cx);
                        })),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_file(activate_path.clone(), window, cx);
                }))
                .on_drag(drag_payload, |drag, _, _, cx| cx.new(|_| drag.clone()))
                .on_drop(cx.listener(move |this, drag: &EditorTabDrag, _, cx| {
                    this.move_tab_before(&drag.path, &drop_path, cx);
                }))
                .on_mouse_down(
                    MouseButton::Middle,
                    cx.listener(move |this, _, window, cx| {
                        window.prevent_default();
                        cx.stop_propagation();
                        this.close_tab(middle_close_path.clone(), window, cx);
                    }),
                )
        });

        let measured_viewport = self.tabs_scroll.bounds().size.width;
        let viewport = if measured_viewport > px(0.) {
            measured_viewport
        } else {
            (window.bounds().size.width - px(EXPLORER_INITIAL_WIDTH) - px(24.)).max(px(0.))
        };
        let content_width = px(190.) * self.tabs.len();
        let max_scroll = if measured_viewport > px(0.) {
            self.tabs_scroll.max_offset().x
        } else {
            (content_width - viewport).max(px(0.))
        };
        let thumb_width = if max_scroll > px(0.) {
            (viewport * (viewport / (viewport + max_scroll)))
                .max(px(24.))
                .min(viewport)
        } else {
            viewport
        };
        let scroll_position = (-self.tabs_scroll.offset().x).clamp(px(0.), max_scroll);
        let thumb_left = if max_scroll > px(0.) && viewport > thumb_width {
            (scroll_position / max_scroll) * (viewport - thumb_width)
        } else {
            px(0.)
        };

        div()
            .id("editor-tabs-container")
            .relative()
            .w_full()
            .h(px(PANEL_HEADER_HEIGHT))
            .border_b_1()
            .border_color(
                component_styles(cx, ThemeComponent::EditorTabs)
                    .base
                    .border
                    .unwrap_or(cx.theme().border),
            )
            .bg(component_styles(cx, ThemeComponent::EditorTabs)
                .base
                .background
                .unwrap_or(cx.theme().tab_bar))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.tabs_hovered = *hovered;
                cx.notify();
            }))
            .child(
                div()
                    .id("editor-tabs-scroll")
                    .w_full()
                    .h_full()
                    .flex()
                    .flex_row()
                    .track_scroll(&self.tabs_scroll)
                    .overflow_x_scroll()
                    .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, cx| {
                        let delta = event.delta.pixel_delta(window.line_height());
                        let scroll_delta = if delta.x != px(0.) { delta.x } else { delta.y };
                        let max_scroll = this.tabs_scroll.max_offset().x;
                        let current = this.tabs_scroll.offset().x;
                        let next = (current + scroll_delta).clamp(-max_scroll, px(0.));
                        if next != current {
                            this.tabs_scroll.set_offset(point(next, px(0.)));
                            cx.notify();
                        }
                        window.prevent_default();
                        cx.stop_propagation();
                    }))
                    .child(
                        // Let the content grow to the combined tab width so GPUI
                        // retains a nonzero horizontal scroll range after layout.
                        h_flex()
                            .h_full()
                            .flex_none()
                            .w_auto()
                            .min_w_full()
                            .children(tabs),
                    ),
            )
            .when(max_scroll > px(0.) && self.tabs_hovered, |this| {
                // Match the tab-strip indicator to the translucent scrollbar thumb.
                let thumb_color = component_styles(cx, ThemeComponent::Scrollbar)
                    .base
                    .background
                    .unwrap_or(cx.theme().primary)
                    .opacity(0.55);
                this.child(
                    div()
                        .absolute()
                        .left(thumb_left)
                        .top_0()
                        .w(thumb_width)
                        .h(px(2.))
                        .bg(thumb_color),
                )
            })
    }

    pub(crate) fn render_editor_panel(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let style = component_styles(cx, ThemeComponent::Editor).base;
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
        v_flex()
            // Expose the editor extent for layout regression checks when docks disappear.
            .debug_selector(|| "editor-panel-content".into())
            .size_full()
            .min_h_0()
            .child(self.render_tabs(window, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
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
                        Editor::new(&self.editor)
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
                            .into_any_element(),
                    ),
            )
    }
}

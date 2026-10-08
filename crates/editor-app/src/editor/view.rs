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
        let tree_scroll = self.tree_state.read(cx).scroll_handle().clone();
        let tree = gpui_base::Tree::new(&self.tree_state)
            .item(move |index, entry, entry_state, _window, cx| {
                let selected = entry_state.is_selected();
                view.update(cx, |app, cx| {
                    let item = entry.item();
                    // Only explicit selection changes the row appearance; pointer hover is inert.
                    let row_style = if selected {
                        row_styles.selected
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
                    let is_folder = Path::new(item.id.as_str()).is_dir();
                    let icon = file_icon(
                        Path::new(item.id.as_str()),
                        is_folder,
                        &theme::active_theme(app.dark_theme),
                    )
                    // Slightly enlarge explorer icons without changing tab-strip icon sizing.
                    .size(px(16.));
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
                    div()
                        .id(format!("explorer-row-{index}"))
                        .debug_selector(move || format!("explorer-row-{index}").into())
                        .w_full()
                        .min_h(px(24.))
                        // Center the content within the whole row, including its minimum height.
                        .flex()
                        .items_center()
                        .bg(row_background)
                        .when(selected, |this| {
                            this.rounded(px(row_style.radius_px.unwrap_or(5.)))
                        })
                        .text_color(row_style.foreground.unwrap_or(cx.theme().foreground))
                        .py(px(row_style.padding_y_px.unwrap_or(0.3)))
                        .px(px(row_style.padding_x_px.unwrap_or(4.)))
                        .pl(px(14.) * entry.depth() + px(8.))
                        // Selection is indicated by its background, without a leading accent border.
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                // Prevent the base tree from expanding folders on mouse down.
                                cx.stop_propagation();
                                this.select_explorer_row(index, window, cx);
                            }),
                        )
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener({
                                let path = PathBuf::from(item.id.as_str());
                                move |this, event: &MouseDownEvent, window, cx| {
                                    cx.stop_propagation();
                                    this.select_explorer_row(index, window, cx);
                                    this.open_explorer_menu(
                                        Some(path.clone()),
                                        event.position,
                                        window,
                                        cx,
                                    );
                                }
                            }),
                        )
                        .child(
                            h_flex()
                                .w_full()
                                .min_w(px(0.))
                                .items_center()
                                .gap_1()
                                .child(
                                    div()
                                        .id(format!("explorer-disclosure-{index}"))
                                        .debug_selector(move || {
                                            format!("explorer-disclosure-{index}").into()
                                        })
                                        // Center the 12 px glyph inside a slightly larger click target.
                                        .size(px(16.))
                                        .flex_shrink_0()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .when(is_folder, |arrow| {
                                            arrow
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(move |this, _, window, cx| {
                                                        cx.stop_propagation();
                                                        this.select_explorer_row(index, window, cx);
                                                    }),
                                                )
                                                .on_click(cx.listener({
                                                    let path = PathBuf::from(item.id.as_str());
                                                    move |this, _, _, cx| {
                                                        // Consume arrow clicks so the row cannot toggle twice.
                                                        cx.stop_propagation();
                                                        this.toggle_explorer_directory(&path, cx);
                                                    }
                                                }))
                                        })
                                        .child(disclosure),
                                )
                                .child(
                                    h_flex()
                                        .id(format!("explorer-icon-{index}"))
                                        .debug_selector(move || {
                                            format!("explorer-icon-{index}").into()
                                        })
                                        .size(px(16.))
                                        .flex_shrink_0()
                                        .items_center()
                                        .justify_center()
                                        .child(icon),
                                )
                                .child(
                                    div()
                                        .id(format!("explorer-label-{index}"))
                                        .debug_selector(move || {
                                            format!("explorer-label-{index}").into()
                                        })
                                        .flex_1()
                                        .min_w(px(0.))
                                        .truncate()
                                        .child(item.label.clone()),
                                ),
                        )
                        .on_click(cx.listener({
                            let item = item.clone();
                            move |this, event: &ClickEvent, window, cx| {
                                // A single click selects only; double clicks activate the row.
                                if event.click_count() != 2 {
                                    return;
                                }
                                if is_folder {
                                    this.toggle_explorer_directory(Path::new(item.id.as_str()), cx);
                                } else {
                                    this.open_file(PathBuf::from(item.id.as_str()), window, cx);
                                }
                            }
                        }))
                        .into_any_element()
                })
            })
            .list_style(StyleRefinement::default().flex_grow_1().size_full())
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
            .id("explorer-root")
            .debug_selector(|| "explorer-root".into())
            .size_full()
            .min_h_0()
            .bg(tree_style.background.unwrap_or(cx.theme().background))
            .relative()
            .child(tree)
            .child(
                div()
                    .absolute()
                    .right_0()
                    .top_0()
                    .bottom_0()
                    .w(px(8.))
                    .child(ui::controls::vertical_scrollbar(&tree_scroll, cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.open_explorer_menu(None, event.position, window, cx);
                }),
            )
    }

    fn render_tabs(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tab_styles = component_styles(cx, ThemeComponent::EditorTab);
        let close_styles = component_styles(cx, ThemeComponent::EditorTabClose);
        let tabs = self.tabs.iter().enumerate().map(|(index, tab)| {
            let path = tab.path().to_path_buf();
            let is_active = self.active_path.as_ref() == Some(&path);
            let is_external = !path.starts_with(self.workspace.root());
            let is_dirty = tab.is_dirty();
            let disk_state = tab
                .text
                .as_ref()
                .map_or(DiskState::Synced, |text| text.disk_state);
            let name = tab
                .path()
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| t!("editor.untitled").to_string());
            let icon = file_icon(&path, false, &theme::active_theme(self.dark_theme));
            let activate_path = path.clone();
            let context_path = path.clone();
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
                .debug_selector(move || format!("editor-tab-{index}").into())
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
                        .child(format!(
                            "{}{}{}",
                            name,
                            if is_dirty { " ●" } else { "" },
                            match disk_state {
                                DiskState::Synced => "",
                                DiskState::Conflict => " ⚠",
                                DiskState::Deleted => " ×",
                            }
                        )),
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
                    // A tab click activates existing content and follows the explorer preference.
                    if let Some(index) =
                        this.tabs.iter().position(|tab| tab.path() == activate_path)
                    {
                        this.activate_tab(index, window, cx);
                    }
                }))
                .on_drag(drag_payload, |drag, _, _, cx| cx.new(|_| drag.clone()))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |app, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        if let Some(index) =
                            app.tabs.iter().position(|tab| tab.path() == context_path)
                        {
                            app.activate_tab(index, window, cx);
                            app.open_file_provider_menu(event.position, window, cx);
                        }
                    }),
                )
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
            .debug_selector(|| "editor-tabs-container".into())
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
        // No document means no input handler, gutter, tab strip or drop caret.
        if self.tabs.is_empty() {
            return v_flex()
                .debug_selector(|| "editor-panel-content".into())
                .size_full()
                .min_h_0()
                .child(crate::ui::controls::empty_editor_canvas(
                    t!("editor.select_file").to_string(),
                    style.background.unwrap_or(cx.theme().background),
                    cx.theme().muted_foreground,
                ))
                .into_any_element();
        }
        if self.active_text_tab_index().is_none() {
            // A file-only body never mounts the retained background editor or its native input handler.
            if self.editor.focus_handle(cx).is_focused(window) {
                window.blur(cx);
            }
            let body = if let Some(preview) = self.active_editor_preview(cx) {
                let failed = preview.read(cx).file_preview_failed();
                let file_id = self
                    .active_tab_index()
                    .map(|index| self.tabs[index].file_id);
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .relative()
                    .child(preview)
                    .when(failed, |body| {
                        body.child(
                            div().absolute().top_2().right_2().child(
                                Button::new("retry-image-resource")
                                    .label(t!("file_view.retry").to_string())
                                    .small()
                                    .on_click(cx.listener(move |app, _, _, cx| {
                                        if app
                                            .active_tab_index()
                                            .map(|index| app.tabs[index].file_id)
                                            == file_id
                                        {
                                            app.retry_file_view(cx);
                                        }
                                    })),
                            ),
                        )
                    })
                    .into_any_element()
            } else {
                let message = self.file_view_unavailable_reason(cx);
                let file_id = self
                    .active_tab_index()
                    .map(|index| self.tabs[index].file_id);
                v_flex()
                    .debug_selector(|| "file-view-unavailable".into())
                    .flex_1()
                    .min_h_0()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .child(div().text_color(cx.theme().muted_foreground).child(message))
                    .child(
                        // Recovery remains separately measurable even when the failure message changes size.
                        div().debug_selector(|| "file-view-retry".into()).child(
                            Button::new("retry-file-view")
                                .label(t!("file_view.retry").to_string())
                                .small()
                                .on_click(cx.listener(move |app, _, _, cx| {
                                    if app.active_tab_index().map(|index| app.tabs[index].file_id)
                                        == file_id
                                    {
                                        app.retry_file_view(cx);
                                    }
                                })),
                        ),
                    )
                    .into_any_element()
            };
            return v_flex()
                .debug_selector(|| "editor-panel-content".into())
                .size_full()
                .min_h_0()
                .child(self.render_tabs(window, cx))
                .child(body)
                .into_any_element();
        }
        let body = if let Some(preview) = self.active_editor_preview(cx) {
            self.render_editor_preview_body(preview, window, cx)
        } else {
            self.render_native_editor(window, cx)
        };
        v_flex()
            // Expose the editor extent for layout regression checks when docks disappear.
            .debug_selector(|| "editor-panel-content".into())
            .size_full()
            .min_h_0()
            .child(self.render_tabs(window, cx))
            // Keep the editor's flexible height when the body is either a source pane or a split.
            .child(v_flex().flex_1().min_h_0().child(body))
            .into_any_element()
    }
}

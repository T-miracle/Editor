//! Draws editor popovers locally while retaining the upstream editing engine.

use crate::*;
use gpui_base::input::{Enter, Escape, InputOverlayKind, MoveDown, MoveUp};
use gpui_kit::{
    AnyElement, BoxShadow, Hsla, StyleRefinement, StyledText, deferred, relative, rems, rgb,
};
use lsp_types::{Documentation, HoverContents, MarkedString};

/// Select the host renderer only while a popover needs styling unavailable upstream.
pub(super) fn render(
    editor: &Entity<EditorState>,
    selection: &Rc<Cell<(u64, usize)>>,
    style: theme::ResolvedStyle,
    window: &mut Window,
    cx: &mut Context<EditorApp>,
) -> Option<AnyElement> {
    let (completion, hover) = {
        let state = editor.read(cx);
        // Search and code actions retain their native controls and action routing.
        if state.search_session().open || state.code_action_menu_state().open {
            return None;
        }
        (
            state.completion_menu_state().clone(),
            state.hover_popover().cloned(),
        )
    };
    if !completion.open && hover.is_none() {
        selection.set((0, 0));
        return None;
    }
    if selection.get().0 != completion.revision() {
        selection.set((completion.revision(), 0));
    }
    let completion_view = completion
        .open
        .then(|| render_completion(editor, selection, &completion, window, cx))
        .flatten();
    let hover_view = hover
        .as_ref()
        .and_then(|hover| render_hover(editor, hover, window, cx));
    if completion.open {
        install_completion_actions(editor, selection, cx);
    }
    // The base editor keeps the same document, cursor, IME and LSP state. Only
    // its component overlay is replaced for the lifetime of these popovers.
    Some(
        div()
            .id("custom-editor-popovers")
            .debug_selector(|| "custom-editor-popovers".into())
            .relative()
            .size_full()
            .bg(style.background.unwrap_or(cx.theme().background))
            .text_color(style.foreground.unwrap_or(cx.theme().foreground))
            .font_family(cx.theme().mono_font_family.clone())
            .text_size(
                style
                    .font_size_px
                    .map(px)
                    .unwrap_or(cx.theme().mono_font_size),
            )
            // Match gpui-component Editor's 1.5 text line height during the renderer switch.
            .line_height(relative(1.5))
            .child(gpui_base::input::Editor::new(editor))
            .children(completion_view)
            .children(hover_view)
            .into_any_element(),
    )
}

/// Route completion keys to the engine's insertion method and keep selection local.
fn install_completion_actions(
    editor: &Entity<EditorState>,
    selection: &Rc<Cell<(u64, usize)>>,
    cx: &mut Context<EditorApp>,
) {
    let selection = selection.clone();
    let (items, revision, start, end) = {
        let state = editor.read(cx);
        let menu = state.completion_menu_state();
        (
            menu.items.clone(),
            menu.revision(),
            menu.trigger_start_offset.unwrap_or(state.cursor()),
            state.cursor(),
        )
    };
    let editor = editor.clone();
    editor.clone().update(cx, |state, _| {
        state.set_overlay_action_handler(move |kind, action, window, cx| {
            if kind != InputOverlayKind::Completion {
                return false;
            }
            if items.is_empty() {
                return false;
            }
            let selected = selection.get().1.min(items.len() - 1);
            if action.partial_eq(&MoveDown) {
                selection.set((revision, (selected + 1).min(items.len() - 1)));
                cx.notify();
            } else if action.partial_eq(&MoveUp) {
                selection.set((revision, selected.saturating_sub(1)));
                cx.notify();
            } else if Enter::is_primary(&*action) {
                let item = items[selected].clone();
                let editor = editor.clone();
                // Defer the mutation until the engine finishes dispatching this key.
                cx.spawn_in(window, async move |_, cx| {
                    editor.update_in(cx, |state, window, cx| {
                        state.insert_completion(&item, start..end, window, cx);
                    })
                })
                .detach();
            } else if action.partial_eq(&Escape) {
                // The engine closes a handled completion on Escape itself.
            } else {
                return false;
            }
            true
        });
    });
}

/// Position the styled completion list at the cursor using upstream layout data.
fn render_completion(
    editor: &Entity<EditorState>,
    selection: &Rc<Cell<(u64, usize)>>,
    menu: &gpui_base::input::CompletionMenuState,
    window: &mut Window,
    cx: &mut Context<EditorApp>,
) -> Option<AnyElement> {
    let state = editor.read(cx);
    let (cursor, line_height) = state.cursor_layout()?;
    let origin = state.scroll_offset() + cursor.origin - state.input_bounds().origin
        + point(-px(4.), line_height + px(4.));
    let width = state.lsp().completion_menu.max_width;
    let origin_x = origin
        .x
        .max(px(0.))
        .min((window.bounds().size.width - state.input_bounds().origin.x - px(120.)).max(px(0.)));
    let absolute_x = state.input_bounds().origin.x + origin_x;
    let max_width = width
        .min(window.bounds().size.width - absolute_x)
        .max(px(120.));
    // Move documentation below the list when there is no room for two columns.
    let vertical_layout = absolute_x + width + px(4.) + width > window.bounds().size.width;
    let start = menu.trigger_start_offset.unwrap_or(state.cursor());
    let end = state.cursor();
    let selected = selection.get().1.min(menu.items.len().saturating_sub(1));
    let revision = menu.revision();
    let host = cx.entity().downgrade();
    let row_styles = component_styles(cx, ThemeComponent::ExplorerRow);
    let foreground = cx.theme().foreground;
    let muted = cx.theme().muted_foreground;
    let match_color = cx.theme().primary;
    let hover_bg = row_styles.hover.background.unwrap_or(cx.theme().list_hover);
    let selected_bg = row_styles
        .selected
        .background
        .unwrap_or(cx.theme().selection);
    let selected_fg = row_styles.selected.foreground.unwrap_or(foreground);
    let selected_border = row_styles.selected.border.unwrap_or(cx.theme().primary);
    let rows = menu.items.iter().cloned().enumerate().map(|(index, item)| {
        let editor = editor.clone();
        let selection = selection.clone();
        let host = host.clone();
        let deprecated = item.deprecated.unwrap_or(false);
        let matched_len = item
            .filter_text
            .as_ref()
            .map(|text| text.len())
            .unwrap_or(menu.query.len())
            .min(item.label.len());
        let label = StyledText::new(item.label.clone()).with_highlights(vec![(
            0..matched_len,
            HighlightStyle {
                color: Some(match_color),
                ..Default::default()
            },
        )]);
        h_flex()
            .id(("editor-completion", index))
            .debug_selector(move || format!("editor-completion-row-{index}").into())
            .gap_2()
            .px_2()
            .py_1()
            .when(deprecated, |row| row.line_through())
            .when(index == selected, |row| {
                row.bg(selected_bg)
                    .text_color(selected_fg)
                    .border_l_1()
                    .border_color(selected_border)
            })
            .when(index != selected, |row| row.hover(|row| row.bg(hover_bg)))
            .child(div().text_color(foreground).child(label))
            .when_some(item.detail.clone(), |row, detail| {
                row.child(
                    div()
                        .text_color(muted)
                        .italic()
                        .when(deprecated, |detail| detail.line_through())
                        .child(detail),
                )
            })
            .on_hover(move |hovered, _, cx| {
                if *hovered && selection.get().1 != index {
                    // Selection and documentation follow the hovered completion.
                    selection.set((revision, index));
                    let _ = host.update(cx, |app, cx| {
                        let _ = app.editor_panel.update(cx, |_, cx| cx.notify());
                    });
                }
            })
            .on_click(move |_, window, cx| {
                // All edits still pass through the upstream completion insertion path.
                editor.update(cx, |state, cx| {
                    state.insert_completion(&item, start..end, window, cx);
                    state.dismiss_completion_overlay(cx);
                });
                cx.stop_propagation();
            })
    });
    let documentation = menu
        .items
        .get(selected)
        .and_then(|item| item.documentation.as_ref());
    let documentation = documentation.map(|doc| match doc {
        Documentation::String(text) => text.clone(),
        Documentation::MarkupContent(markup) => markup.value.clone(),
    });
    let panel = card(cx, "editor-completion-card", false)
        .p_1()
        .max_w(max_width.max(px(120.)))
        .min_w(px(120.))
        .max_h(px(240.))
        .overflow_y_scroll()
        .children(rows);
    let mut layout = div()
        .flex()
        .flex_row()
        .items_start()
        .gap_1()
        .when(vertical_layout, |layout| layout.flex_col())
        .child(panel);
    if let Some(documentation) = documentation {
        let documentation = if vertical_layout {
            documentation.lines().next().unwrap_or_default().to_string()
        } else {
            documentation
        };
        layout = layout.child(
            card(cx, "editor-completion-documentation-card", false)
                .p_1()
                .px_2()
                .w(if vertical_layout { max_width } else { width })
                .max_h(px(240.))
                .overflow_y_scroll()
                .child(markdown_view(
                    "editor-completion-documentation",
                    documentation,
                    cx,
                )),
        );
    }
    Some(
        deferred(
            div()
                .absolute()
                .left(origin_x)
                .top(origin.y)
                .font_family(cx.theme().mono_font_family.clone())
                .text_size(typography::editor_font_size(cx))
                .child(layout),
        )
        .into_any_element(),
    )
}

/// Anchor the definition card to the symbol while preserving the original white style.
fn render_hover(
    editor: &Entity<EditorState>,
    hover: &gpui_base::input::HoverPopoverState,
    window: &mut Window,
    cx: &mut Context<EditorApp>,
) -> Option<AnyElement> {
    let state = editor.read(cx);
    let bounds = state.range_to_bounds(&hover.symbol_range)?;
    let local = bounds.origin - state.input_bounds().origin;
    let available_below = window.bounds().size.height - bounds.bottom();
    let top = if available_below >= px(180.) {
        local.y + bounds.size.height
    } else {
        (local.y - px(320.)).max(px(0.))
    };
    let left = local.x.max(px(0.));
    let markdown = hover_markdown(&hover.hover.contents);
    Some(
        deferred(
            div().absolute().left(left).top(top).child(
                card(cx, "editor-definition-card", true)
                    .p_4()
                    .max_w(px(500.))
                    .max_h(px(320.))
                    .overflow_y_scroll()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(typography::editor_font_size(cx))
                    // Moving within the card should not request obscured editor text.
                    .on_mouse_move(|_, _, cx| cx.stop_propagation())
                    .child(markdown_view("editor-definition-details", markdown, cx)),
            ),
        )
        .into_any_element(),
    )
}

/// Match the editor font in markdown and fenced code without changing other UI text.
fn markdown_view(
    id: &'static str,
    markdown: String,
    cx: &App,
) -> gpui_kit::component::text::TextView {
    let font_size = typography::editor_font_size(cx);
    let mut style = gpui_kit::component::text::TextViewStyle::default()
        .paragraph_gap(rems(0.5))
        .heading_font_size(|level, size| match level {
            1..=3 => size,
            4 => size * 0.9,
            _ => size * 0.8,
        })
        .code_block(
            StyleRefinement::default()
                .bg(cx.theme().transparent)
                .p_0()
                .text_size(font_size),
        );
    style.heading_base_font_size = font_size;
    gpui_kit::component::text::TextView::markdown(id, markdown)
        .style(style)
        .selectable(true)
}

/// Share the editor's border and contact shadow between both project popovers.
fn card(cx: &App, id: &'static str, light: bool) -> gpui_kit::Stateful<gpui_kit::Div> {
    let shadow = vec![
        BoxShadow {
            color: Hsla {
                h: 0.,
                s: 0.,
                l: 0.,
                a: 0.18,
            },
            blur_radius: px(10.),
            spread_radius: px(-1.),
            offset: point(px(0.), px(2.)),
            inset: false,
        },
        BoxShadow {
            color: Hsla {
                h: 0.,
                s: 0.,
                l: 0.,
                a: 0.18,
            },
            blur_radius: px(3.),
            spread_radius: px(0.),
            offset: point(px(0.), px(1.)),
            inset: false,
        },
    ];
    div()
        .id(id)
        .debug_selector(move || id.into())
        .flex_none()
        .occlude()
        // Definition details retain their white card; completion follows the theme.
        .bg(if light {
            rgb(0xffffff).into()
        } else {
            cx.theme().popover
        })
        .text_color(if light {
            rgb(0x202124).into()
        } else {
            cx.theme().popover_foreground
        })
        .border_1()
        .border_color(if light && cx.theme().is_dark() {
            rgb(0x333333).into()
        } else if light {
            rgb(0xcccccc).into()
        } else {
            cx.theme().border
        })
        .shadow(shadow)
}

/// Normalize LSP hover formats before the project renderer parses Markdown.
fn hover_markdown(contents: &HoverContents) -> String {
    match contents {
        HoverContents::Markup(markup) => markup.value.clone(),
        HoverContents::Scalar(MarkedString::String(text)) => text.clone(),
        HoverContents::Scalar(MarkedString::LanguageString(item)) => {
            format!("```{}\n{}\n```", item.language, item.value)
        }
        HoverContents::Array(items) => items
            .iter()
            .map(|item| match item {
                MarkedString::String(text) => text.clone(),
                MarkedString::LanguageString(item) => {
                    format!("```{}\n{}\n```", item.language, item.value)
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use editor_core::Workspace;
    use gpui_kit::{TestAppContext, component::Root, gpui, size};
    use lsp_types::{CompletionItem, Hover};
    use std::{cell::RefCell, rc::Rc};

    /// Both popovers render in the project while the upstream editor owns their state.
    #[gpui::test]
    fn completion_and_hover_use_project_cards(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            typography::init(cx);
            theme::apply_theme(theme::builtin_theme(false), cx);
            cx.set_reduce_motion(true);
        });
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sample.rs");
        std::fs::write(&path, "sample").unwrap();
        let workspace = Workspace::open(directory.path()).unwrap();
        let slot = Rc::new(RefCell::new(None));
        let capture = slot.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let view = cx.new(|cx| EditorApp::new(workspace, Some(path), window, cx));
            *capture.borrow_mut() = Some(view.clone());
            Root::new(view, window, cx)
        });
        let view = slot.borrow_mut().take().unwrap();
        cx.simulate_resize(size(px(1000.), px(800.)));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        // Popover rendering must preserve the editor's existing text row height.
        let base_line_height = cx.update(|_, cx| {
            view.read(cx)
                .editor
                .read(cx)
                .line_height()
                .expect("editor should have a measured layout")
        });

        cx.update(|_, cx| {
            let editor = view.read(cx).editor.clone();
            editor.update(cx, |state, cx| {
                state.present_completion_items(
                    0,
                    "",
                    vec![CompletionItem {
                        label: "completion".into(),
                        ..Default::default()
                    }],
                    cx,
                );
            });
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(cx.debug_bounds("editor-completion-card").is_some());
        let completion_line_height = cx.update(|_, cx| {
            view.read(cx)
                .editor
                .read(cx)
                .line_height()
                .expect("completion should keep the editor layout")
        });
        assert_eq!(completion_line_height, base_line_height);
        let row = cx.debug_bounds("editor-completion-row-0").unwrap();
        cx.simulate_click(row.center(), Default::default());
        cx.run_until_parked();
        cx.update(|_, cx| {
            let editor = view.read(cx).editor.clone();
            assert!(editor.read(cx).text().to_string().contains("completion"));
        });

        cx.update(|_, cx| {
            let editor = view.read(cx).editor.clone();
            editor.update(cx, |state, cx| {
                state.present_completion_items(
                    state.cursor(),
                    "",
                    vec![CompletionItem {
                        label: "keyboard".into(),
                        ..Default::default()
                    }],
                    cx,
                );
            });
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        cx.update(|_, cx| {
            let editor = view.read(cx).editor.clone();
            assert!(editor.read(cx).text().to_string().contains("keyboard"));
        });

        cx.update(|_, cx| {
            let editor = view.read(cx).editor.clone();
            editor.update(cx, |state, cx| {
                state.dismiss_completion_overlay(cx);
                state.present_hover(
                    0..6,
                    Hover {
                        contents: HoverContents::Scalar(MarkedString::String("details".into())),
                        range: None,
                    },
                    cx,
                );
            });
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(cx.debug_bounds("editor-definition-card").is_some());
        let hover_line_height = cx.update(|_, cx| {
            view.read(cx)
                .editor
                .read(cx)
                .line_height()
                .expect("hover should keep the editor layout")
        });
        assert_eq!(hover_line_height, base_line_height);
    }
}

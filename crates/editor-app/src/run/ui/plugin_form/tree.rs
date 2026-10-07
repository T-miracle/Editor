//! Native configuration tree over gpui-base; local rows and drops only change window drafts.
use super::*;
use crate::ui::controls::{Button, Icon, Input, tree_row};
use gpui_base::TreeItem;
use std::collections::BTreeSet;

#[derive(Clone)]
struct NodeDrag {
    id: String,
    label: String,
    window: gpui_kit::EntityId,
}
impl Render for NodeDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded(px(4.))
            .bg(cx.theme().accent)
            .child(self.label.clone())
    }
}

/// Rebuild changed entries without losing shared expansion state or native rename inputs.
pub(super) fn synchronize(form: &Entity<RunConfigForm>, cx: &mut gpui_kit::App) {
    let state = form.read(cx).plugin.as_ref().unwrap();
    let signature = serde_json::to_string(&(
        &state.draft.tree,
        &state.draft.plugin_configurations,
        &state.selected,
    ))
    .unwrap();
    if signature == state.tree_signature {
        return;
    }
    let Some(tree) = state.tree.clone() else {
        return;
    };
    fn expansion(item: &TreeItem, ids: &mut BTreeSet<String>) {
        if item.is_expanded() {
            ids.insert(item.id.to_string());
        }
        for child in &item.children {
            expansion(child, ids);
        }
    }
    let mut expanded = BTreeSet::new();
    for index in 0.. {
        let Some(entry) = tree.read(cx).entry(index) else {
            break;
        };
        if entry.is_root() {
            expansion(entry.item(), &mut expanded);
        }
    }
    if let Some(id) = &state.selected {
        expanded.extend(state.draft.tree_ancestors(id));
    }
    fn items(
        set: &RunConfigSet,
        parent: Option<&str>,
        expanded: &BTreeSet<String>,
    ) -> Vec<TreeItem> {
        set.tree_children(parent)
            .into_iter()
            .map(|id| {
                let label = set
                    .tree
                    .folders
                    .get(&id)
                    .map(|folder| folder.name.clone())
                    .unwrap_or_else(|| {
                        let name = &set.plugin_configurations[&id].name;
                        if name.is_empty() {
                            t!("run.plugin_unnamed").into()
                        } else {
                            name.clone()
                        }
                    });
                TreeItem::new(id.clone(), label)
                    .children(items(set, Some(&id), expanded))
                    .expanded(expanded.contains(&id))
            })
            .collect()
    }
    let roots = items(&state.draft, None, &expanded);
    let selected = state.selected.clone();
    tree.update(cx, |tree, cx| {
        tree.set_items(roots, cx);
        let index = selected
            .as_ref()
            .and_then(|id| tree.index_of(&id.clone().into()));
        tree.set_selected_index(index, cx);
    });
    form.update(cx, |form, _| {
        form.plugin.as_mut().unwrap().tree_signature = signature
    });
}

/// Base virtualization, keyboard selection and accessibility surround editor-owned tree rows.
pub(super) fn render(
    app: &Entity<EditorApp>,
    form: &Entity<RunConfigForm>,
    cx: &gpui_kit::App,
) -> AnyElement {
    let tree = form.read(cx).plugin.as_ref().unwrap().tree.clone().unwrap();
    let owner = app.clone();
    let form = form.clone();
    let root = app.clone();
    let background = app.clone();
    let key = app.clone();
    let origin = form.entity_id();
    let element = gpui_base::Tree::new(&tree)
        .size_full()
        // The base virtual list also needs a bounded frame; outer sizing alone renders no rows.
        .list_style(StyleRefinement::default().flex_grow_1().size_full())
        .item(move |index, entry, selected, _, cx| {
            let state = form.read(cx).plugin.as_ref().unwrap();
            let id = entry.item().id.to_string();
            let folder = state.draft.tree.folders.contains_key(&id);
            let invalid = state
                .draft
                .plugin_configurations
                .get(&id)
                .is_some_and(|data| !matches!(data.validation, ConfigurationValidation::Valid));
            let label = entry.item().label.to_string();
            let reason = state
                .draft
                .plugin_configurations
                .get(&id)
                .map(|data| actions::validation_reason(&data.validation))
                .filter(|reason| !reason.is_empty());
            let drag = NodeDrag {
                id: id.clone(),
                label: label.clone(),
                window: origin,
            };
            let row_owner = owner.clone();
            let row_tree = state.tree.clone().unwrap();
            let toggle_tree = row_tree.clone();
            let toggle_id = id.clone();
            let toggle: std::rc::Rc<dyn Fn(&mut Window, &mut gpui_kit::App)> =
                std::rc::Rc::new(move |window, cx| {
                    toggle_tree.update(cx, |tree, cx| {
                        let Some(index) = tree.index_of(&toggle_id.clone().into()) else {
                            return;
                        };
                        if let Some(entry) = tree.entry(index) {
                            entry.item().clone().expanded(!entry.is_expanded());
                        }
                        let roots = (0..)
                            .map_while(|index| tree.entry(index))
                            .filter(|entry| entry.is_root())
                            .map(|entry| entry.item().clone())
                            .collect::<Vec<_>>();
                        tree.set_items(roots, cx);
                        let selected = tree.index_of(&toggle_id.clone().into());
                        tree.set_selected_index(selected, cx);
                        tree.focus(window, cx);
                    });
                });
            let drop_owner = owner.clone();
            let before_owner = owner.clone();
            let before_id = id.clone();
            let parent = state.draft.tree_parent(&id);
            let target_parent = if folder {
                Some(id.clone())
            } else {
                parent.clone()
            };
            let row_id = id.clone();
            let selector = format!("run-config-tree-{id}");
            let icon = if folder {
                Icon::new(IconName::Folder)
            } else {
                Icon::default().path("icons/run-start.svg")
            };
            let row = if let Some((renamed, input)) = &state.rename
                && renamed == &id
            {
                div()
                    .h(px(34.))
                    .pl(px(8. + entry.depth() as f32 * 14.))
                    .child(Input::new(input))
                    .into_any_element()
            } else {
                tree_row(
                    format!("run-config-tree-{id}"),
                    label,
                    entry.depth(),
                    icon,
                    folder,
                    entry.is_expanded(),
                    selected.is_selected(),
                    invalid,
                    cx,
                    folder.then_some(toggle),
                )
                .debug_selector(move || selector.clone())
                .when_some(reason, |row, reason| {
                    row.tooltip(move |window, cx| {
                        crate::ui::controls::Tooltip::new(reason.clone()).build(window, cx)
                    })
                })
                // Selection uses the base state but disclosure toggling belongs only to its arrow;
                // dragging a folder must not collapse the descendant used as a drop destination.
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    cx.stop_propagation();
                    row_tree.update(cx, |tree, cx| {
                        tree.set_selected_index(Some(index), cx);
                        tree.focus(window, cx);
                    });
                })
                .on_click(move |event, window, cx| {
                    row_owner.update(cx, |app, cx| {
                        app.select_plugin_configuration(&row_id, cx);
                        if folder && event.click_count() == 2 {
                            app.rename_plugin_folder(&row_id, window, cx);
                        }
                    })
                })
                .on_drag(drag, |drag, _, _, cx| cx.new(|_| drag.clone()))
                .on_drop(move |drag: &NodeDrag, _, cx| {
                    cx.stop_propagation();
                    if drag.window == origin {
                        drop_owner.update(cx, |app, cx| {
                            app.move_plugin_tree_node(&drag.id, target_parent.clone(), None, cx)
                        });
                    }
                })
                .into_any_element()
            };
            v_flex()
                .w_full()
                .child(
                    div()
                        .id(format!("run-config-before-{id}"))
                        .h(px(4.))
                        .w_full()
                        .debug_selector({
                            let id = id.clone();
                            move || format!("run-config-before-{id}")
                        })
                        .on_drop(move |drag: &NodeDrag, _, cx| {
                            cx.stop_propagation();
                            if drag.window == origin {
                                before_owner.update(cx, |app, cx| {
                                    app.move_plugin_tree_node(
                                        &drag.id,
                                        parent.clone(),
                                        Some(&before_id),
                                        cx,
                                    )
                                });
                            }
                        }),
                )
                .child(row)
                .into_any_element()
        });
    div()
        .id("run-config-tree-list")
        .debug_selector(|| "run-config-tree-list".into())
        .size_full()
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            background.update(cx, |app, cx| app.clear_plugin_tree_selection(window, cx));
        })
        .on_key_down(move |event, window, cx| {
            if event.keystroke.key == "f2" {
                key.update(cx, |app, cx| {
                    let chosen = app
                        .run_form
                        .as_ref()
                        .and_then(|form| form.read(cx).plugin.as_ref())
                        .and_then(|state| state.selected.clone());
                    if let Some(id) = chosen {
                        app.rename_plugin_folder(&id, window, cx);
                    }
                });
                cx.stop_propagation();
            }
        })
        .on_drop(move |drag: &NodeDrag, _, cx| {
            if drag.window == origin {
                root.update(cx, |app, cx| {
                    app.move_plugin_tree_node(&drag.id, None, None, cx)
                });
            }
        })
        .child(element)
        .into_any_element()
}

/// Explicit close/delete decisions use the same HWND and local controls as the form.
pub(super) fn decision(
    app: &Entity<EditorApp>,
    form: &Entity<RunConfigForm>,
    cx: &gpui_kit::App,
) -> AnyElement {
    let state = form.read(cx).plugin.as_ref().unwrap();
    let deletion = matches!(state.decision, Some(Decision::Delete(_)));
    let text = match &state.decision {
        Some(Decision::Delete(id)) => {
            let nodes = state.draft.tree_subtree(id);
            let folders = nodes
                .iter()
                .filter(|id| state.draft.tree.folders.contains_key(*id))
                .count();
            t!(
                "run.plugin_delete_count",
                folders = folders,
                configurations = nodes.len() - folders
            )
            .to_string()
        }
        _ => t!("run.plugin_close_question").to_string(),
    };
    let confirm = app.clone();
    let save = app.clone();
    let discard = app.clone();
    let keep = app.clone();
    let mut buttons = h_flex().gap_2().justify_end();
    if deletion {
        buttons = buttons.child(
            Button::new("run-config-confirm-delete")
                .debug_selector(|| "run-config-confirm-delete".into())
                .label(t!("run.form_delete"))
                .on_click(move |_, window, cx| {
                    confirm.update(cx, |app, cx| app.confirm_plugin_delete(window, cx))
                }),
        );
    } else {
        buttons = buttons
            .child(
                Button::new("run-config-close-save")
                    .debug_selector(|| "run-config-close-save".into())
                    .primary()
                    .label(t!("run.form_save_short"))
                    .on_click(move |_, _, cx| {
                        save.update(cx, |app, cx| {
                            if let Some(form) = app.run_form.clone() {
                                form.update(cx, |form, _| {
                                    form.plugin.as_mut().unwrap().decision = None
                                });
                            }
                            app.begin_plugin_commit(CommitMode::Save, cx);
                        });
                    }),
            )
            .child(
                Button::new("run-config-close-discard")
                    .debug_selector(|| "run-config-close-discard".into())
                    .label(t!("run.form_discard"))
                    .on_click(move |_, _, cx| discard.update(cx, |app, cx| app.close_run_form(cx))),
            );
    }
    buttons = buttons.child(
        Button::new("run-config-close-keep")
            .debug_selector(|| "run-config-close-keep".into())
            .label(if deletion {
                t!("run.form_cancel")
            } else {
                t!("run.plugin_continue")
            })
            .on_click(move |_, window, cx| {
                keep.update(cx, |app, cx| app.dismiss_plugin_decision(window, cx))
            }),
    );
    v_flex()
        .size_full()
        .p_5()
        .justify_center()
        .gap_5()
        .child(text)
        .child(buttons)
        .into_any_element()
}

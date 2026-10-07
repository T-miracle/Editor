//! Outline uses Base tree selection/keyboard/virtualization and project-owned row and dock chrome.
use super::*;
use crate::ui::controls::tree_row;
use gpui_base::Tree;

pub(crate) struct OutlinePanel {
    parent: WeakEntity<EditorApp>,
    focus: FocusHandle,
    /// Base exposes tree.focus, but not its private focus handle; the tracked dock handle forwards activation once.
    focus_subscription: Option<Subscription>,
}
impl OutlinePanel {
    /// The dock view borrows its editor owner's active structure; focus never changes that document target.
    pub(crate) fn new(parent: WeakEntity<EditorApp>, cx: &mut Context<Self>) -> Self {
        Self {
            parent,
            focus: cx.focus_handle(),
            focus_subscription: None,
        }
    }
}
impl EventEmitter<PanelEvent> for OutlinePanel {}
impl Focusable for OutlinePanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl dock::BasePanel for OutlinePanel {
    fn panel_name(&self) -> &'static str {
        "Outline"
    }
    fn closable(&self, _: &App) -> bool {
        false
    }
    fn zoomable(&self, _: &App) -> bool {
        false
    }
    fn visible(&self, cx: &App) -> bool {
        self.parent
            .upgrade()
            .is_some_and(|app| app.read(cx).session_state.outline_visible)
    }
}
impl DockPanel for OutlinePanel {
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let follow = self
            .parent
            .upgrade()
            .is_some_and(|app| app.read(cx).session_state.outline_follow_cursor);
        h_flex()
            .w_full()
            .justify_between()
            .items_center()
            .child(div().truncate().child(t!("panel.outline").to_string()))
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        crate::ui::controls::Button::new("outline-follow")
                            .icon(Icon::default().path("icons/explorer-locate.svg"))
                            .small()
                            .compact()
                            .ghost()
                            .tooltip(t!("outline.follow_cursor").to_string())
                            .accessibility_label(t!("outline.follow_cursor").to_string())
                            .when(follow, |button| button.primary())
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.stop_propagation();
                                let _ = this.parent.update(cx, |app, cx| {
                                    app.session_state.outline_follow_cursor =
                                        !app.session_state.outline_follow_cursor;
                                    app.session_state.save();
                                    app.outline.cursor = None;
                                    cx.notify();
                                });
                            })),
                    )
                    .child(
                        crate::ui::controls::Button::new("outline-hide")
                            .icon(IconName::Minus)
                            .small()
                            .compact()
                            .ghost()
                            .tooltip(t!("outline.hide").to_string())
                            .on_click(cx.listener(|this, _, window, cx| {
                                cx.stop_propagation();
                                let _ = this.parent.update(cx, |app, cx| {
                                    app.toggle_outline(&ToggleOutline, window, cx)
                                });
                            })),
                    ),
            )
    }
}
impl Render for OutlinePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.focus_subscription.is_none() {
            self.focus_subscription = Some(cx.on_focus(&self.focus, window, |this, window, cx| {
                let tree = this.parent.upgrade().and_then(|owner| {
                    let app = owner.read(cx);
                    app.outline
                        .snapshot
                        .is_some()
                        .then(|| app.outline.tree.clone())
                });
                if let Some(tree) = tree {
                    tree.update(cx, |tree, cx| tree.focus(window, cx));
                }
            }));
        }
        let Some(owner) = self.parent.upgrade() else {
            return div().into_any_element();
        };
        let app = owner.read(cx);
        if app.outline.snapshot.is_none() {
            let state = if app.outline.error {
                "outline-failed"
            } else if app.outline.pending.is_some() {
                "outline-loading"
            } else {
                "outline-empty"
            };
            return div()
                .id("outline-status")
                .track_focus(&self.focus)
                .debug_selector(move || state.into())
                .p_3()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(if app.outline.error {
                    t!("outline.failed").to_string()
                } else if app.outline.pending.is_some() {
                    t!("outline.loading").to_string()
                } else {
                    t!("outline.empty").to_string()
                })
                .into_any_element();
        }
        let tree = app.outline.tree.clone();
        let definitions = app.outline.definitions.clone();
        let current = app.outline.current.clone();
        let icons = app.outline.snapshot.as_ref().unwrap().icons.clone();
        let row_tree = tree.clone();
        div()
            .id("outline-tree")
            .track_focus(&self.focus)
            .debug_selector(|| "outline-tree".into())
            .key_context("Outline")
            .size_full()
            .on_action({
                let owner = owner.clone();
                move |_: &ActivateOutline, window, cx| {
                    let selected = owner
                        .read(cx)
                        .outline
                        .tree
                        .read(cx)
                        .selected_item()
                        .map(|item| item.id.to_string());
                    if let Some(id) = selected {
                        owner.update(cx, |app, cx| app.navigate_outline(&id, window, cx));
                    }
                }
            })
            .child(
                Tree::new(&tree)
                    .size_full()
                    // Base's virtual list needs its own bounded frame; sizing the outer Tree alone paints only a measure row.
                    .list_style(StyleRefinement::default().flex_grow_1().size_full())
                    .item(move |index, entry, selected, _, cx| {
                        let id = entry.item().id.to_string();
                        let definition = &definitions[&id];
                        let data = definition.icon.as_ref().and_then(|icon| {
                            let path = if cx.theme().is_dark() {
                                icon.dark.as_ref().unwrap_or(&icon.light)
                            } else {
                                &icon.light
                            };
                            icons.get(path)
                        });
                        let icon = data
                            .map(|bytes| Icon::default().data(bytes))
                            .unwrap_or_else(|| {
                                Icon::default().path("icons/outline-definition.svg")
                            });
                        let toggle_tree = row_tree.clone();
                        let toggle_id = id.clone();
                        let toggle = Rc::new(move |window: &mut Window, cx: &mut App| {
                            toggle_tree.update(cx, |tree, cx| {
                                if let Some(ix) = tree.index_of(&toggle_id.clone().into()) {
                                    if let Some(entry) = tree.entry(ix) {
                                        entry.item().clone().expanded(!entry.is_expanded());
                                    }
                                    let roots = (0..)
                                        .map_while(|ix| tree.entry(ix))
                                        .filter(|entry| entry.is_root())
                                        .map(|entry| entry.item().clone())
                                        .collect::<Vec<_>>();
                                    tree.set_items(roots, cx);
                                    // Disclosure focuses/selects its own row, so subsequent direction keys operate on that branch.
                                    tree.set_selected_index(
                                        tree.index_of(&toggle_id.clone().into()),
                                        cx,
                                    );
                                    tree.focus(window, cx);
                                }
                            });
                        });
                        let click_owner = owner.clone();
                        let click_tree = row_tree.clone();
                        let selector = format!("outline-node-{id}");
                        tree_row(
                            format!("outline-row-{id}"),
                            entry.item().label.to_string(),
                            entry.depth(),
                            icon,
                            entry.is_folder(),
                            entry.is_expanded(),
                            selected.is_selected() || current.as_ref() == Some(&id),
                            false,
                            cx,
                            entry
                                .is_folder()
                                .then_some(toggle as Rc<dyn Fn(&mut Window, &mut App)>),
                        )
                        .debug_selector(move || selector.clone())
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            // Base's row click would toggle folders; only the disclosure arrow changes expansion here.
                            cx.stop_propagation();
                            click_tree.update(cx, |tree, cx| {
                                tree.set_selected_index(Some(index), cx);
                                tree.focus(window, cx);
                            });
                        })
                        .on_click(move |_, window, cx| {
                            click_owner.update(cx, |app, cx| app.navigate_outline(&id, window, cx));
                        })
                        .into_any_element()
                    }),
            )
            .into_any_element()
    }
}

impl EditorApp {
    /// A persistent host toggle restores a hidden outline without changing its dock position.
    pub(crate) fn render_outline_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        crate::ui::controls::Button::new("outline-toggle")
            .icon(Icon::default().path("icons/outline-definition.svg"))
            .small()
            .compact()
            .ghost()
            .tooltip(
                t!(if self.session_state.outline_visible {
                    "outline.hide"
                } else {
                    "outline.show"
                })
                .to_string(),
            )
            .accessibility_label(t!("panel.outline").to_string())
            .on_click(
                cx.listener(|app, _, window, cx| app.toggle_outline(&ToggleOutline, window, cx)),
            )
    }
}

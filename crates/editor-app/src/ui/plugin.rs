//! Native plugin view lifecycle. GPUI entities stay here; guests receive typed events only.
mod canvas;
pub(crate) mod controls;
pub(crate) mod images;
mod render;
#[cfg(test)]
mod tests;
mod theme;

use gpui_base::input::{InputEvent, InputState};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FocusHandle, IntoElement, Render, ScrollHandle,
    Subscription, Window,
};
use plugin_runtime::plugin_protocol::{
    Environment,
    ui::{Action, Document, Kind, UiEvent},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

type EventSink = Rc<dyn Fn(UiEvent, &mut App)>;

struct NativeInput {
    state: Entity<InputState>,
    value_revision: u64,
    last_value: String,
    _subscription: Subscription,
}

pub(crate) struct PluginView {
    plugin: String,
    document: Document,
    environment: Environment,
    sink: EventSink,
    inputs: BTreeMap<String, NativeInput>,
    scrolls: BTreeMap<String, ScrollHandle>,
    canvases: BTreeMap<String, Entity<canvas::CanvasView>>,
    dialog_focus: FocusHandle,
    previous_focus: Option<FocusHandle>,
    dismissed_dialog: Option<String>,
}

impl PluginView {
    /// Image decoding belongs to the worker; native children only borrow the matching immutable raster.
    pub(crate) fn update_images(
        &self,
        panel_key: &str,
        images: &images::SceneImages,
        cx: &mut Context<Self>,
    ) {
        for (id, canvas) in &self.canvases {
            let images = images.get(&format!("{panel_key}/canvas/{id}")).cloned();
            canvas.update(cx, |view, cx| {
                if match (&view.images, &images) {
                    (Some(old), Some(new)) => !std::sync::Arc::ptr_eq(old, new),
                    (None, None) => false,
                    _ => true,
                } {
                    view.images = images;
                    cx.notify();
                }
            });
        }
    }
    pub(crate) fn new(
        plugin: String,
        document: Document,
        environment: Environment,
        sink: impl Fn(UiEvent, &mut App) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            plugin,
            document,
            environment,
            sink: Rc::new(sink),
            inputs: BTreeMap::new(),
            scrolls: BTreeMap::new(),
            canvases: BTreeMap::new(),
            dialog_focus: cx.focus_handle(),
            previous_focus: None,
            dismissed_dialog: None,
        };
        this.sync_native(window, cx);
        if this.document.dialog.is_some() {
            this.focus_dialog(window, cx);
        }
        this
    }

    /// Reconcile keyed native state; ordinary guest renders must not recreate focused inputs.
    pub(crate) fn update_document(
        &mut self,
        document: Document,
        environment: Environment,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.document == document && self.environment == environment {
            return;
        }
        let old_dialog = self.document.dialog.as_ref().map(|d| d.id.clone());
        let next_dialog = document.dialog.as_ref().map(|d| d.id.clone());
        self.document = document;
        self.environment = environment;
        if old_dialog != next_dialog {
            self.dismissed_dialog = None;
            if next_dialog.is_some() {
                self.focus_dialog(window, cx);
            } else if let Some(focus) = self.previous_focus.take() {
                focus.focus(window, cx);
            }
        }
        self.sync_native(window, cx);
        cx.notify();
    }

    fn focus_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.previous_focus.is_none() {
            self.previous_focus = window.focused(cx);
        }
        self.dialog_focus.focus(window, cx);
    }

    fn sync_native(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut nodes = vec![];
        self.document
            .root
            .visit(&mut |node| nodes.push(node.clone()));
        if let Some(dialog) = &self.document.dialog {
            dialog.content.visit(&mut |node| nodes.push(node.clone()));
        }
        let input_ids: BTreeSet<_> = nodes
            .iter()
            .filter(|n| matches!(n.kind, Kind::Input(_)))
            .map(|n| n.id.clone())
            .collect();
        self.inputs.retain(|id, _| input_ids.contains(id));
        let scroll_ids: BTreeSet<_> = nodes
            .iter()
            .filter(|n| matches!(n.kind, Kind::Scroll { .. }))
            .map(|n| n.id.clone())
            .collect();
        self.scrolls.retain(|id, _| scroll_ids.contains(id));
        let canvas_ids: BTreeSet<_> = nodes
            .iter()
            .filter(|node| matches!(node.kind, Kind::Canvas(_)))
            .map(|node| node.id.clone())
            .collect();
        self.canvases.retain(|id, _| canvas_ids.contains(id));
        for node in nodes {
            if let Kind::Canvas(drawing) = &node.kind {
                if !self.canvases.contains_key(&node.id) {
                    let owner = cx.entity().downgrade();
                    let id = node.id.clone();
                    let view = cx.new(|cx| {
                        let canvas = cx.entity().downgrade();
                        canvas::CanvasView::new(
                            drawing.clone(),
                            move |event, revision, cx| {
                                let owner = owner.clone();
                                let id = id.clone();
                                let canvas = canvas.clone();
                                let measurement = matches!(
                                    event,
                                    plugin_runtime::plugin_protocol::ui::CanvasEvent::Resize { .. }
                                );
                                cx.defer(move |cx| {
                                    let accepted = owner.update(cx, |this, cx| {
                                        this.emit_version(&id, revision, Action::Canvas(event), cx)
                                    });
                                    if measurement && !matches!(accepted, Ok(true)) {
                                        let _ = canvas.update(cx, |view, cx| {
                                            view.invalidate_measurement();
                                            cx.notify();
                                        });
                                    }
                                });
                            },
                            window,
                            cx,
                        )
                    });
                    self.canvases.insert(node.id.clone(), view);
                }
                let active = self.document.active_node(&node.id).is_some();
                let font =
                    self.environment
                        .font_style(&self.plugin, node.theme_role(), drawing.grid);
                self.canvases[&node.id].update(cx, |view, cx| {
                    view.drawing = drawing.clone();
                    view.font = font;
                    if active && !view.enabled {
                        view.invalidate_measurement();
                    }
                    view.enabled = active;
                    view.foreground = self.environment.foreground;
                    if !active {
                        view.deactivate();
                    } else if !drawing.focusable {
                        view.cancel_composition();
                    }
                    view.revision = self.document.revision;
                    cx.notify();
                });
            }
            if let Kind::Scroll { .. } = &node.kind {
                self.scrolls.entry(node.id.clone()).or_default();
            }
            let Kind::Input(input) = &node.kind else {
                continue;
            };
            if !self.inputs.contains_key(&node.id) {
                let state = cx.new(|cx| {
                    InputState::new(window, cx)
                        .default_value(input.value.clone())
                        .placeholder(input.placeholder.clone())
                });
                let id = node.id.clone();
                let subscription = cx.subscribe_in(
                    &state,
                    window,
                    move |this, state, event: &InputEvent, _, cx| {
                        let value = state.read(cx).value().to_string();
                        let action = match event {
                            InputEvent::Change => {
                                let Some(entry) = this.inputs.get_mut(&id) else {
                                    return;
                                };
                                if entry.last_value == value {
                                    return;
                                }
                                entry.last_value = value.clone();
                                Action::Change(value)
                            }
                            InputEvent::PressEnter { .. } => Action::Submit(value),
                            _ => return,
                        };
                        this.emit(&id, action, cx);
                    },
                );
                self.inputs.insert(
                    node.id.clone(),
                    NativeInput {
                        state,
                        value_revision: input.value_revision,
                        last_value: input.value.clone(),
                        _subscription: subscription,
                    },
                );
            }
            let disabled = self.document.active_node(&node.id).is_none();
            let entry = self.inputs.get_mut(&node.id).unwrap();
            let replace = entry.value_revision != input.value_revision;
            if replace {
                entry.last_value = input.value.clone();
                entry.value_revision = input.value_revision;
            }
            entry.state.update(cx, |state, cx| {
                state.set_disabled(disabled, cx);
                state.set_placeholder(input.placeholder.clone(), window, cx);
                if replace {
                    state.set_value(input.value.clone(), window, cx);
                }
            });
        }
    }

    /// Old elements cannot activate removed/disabled controls or controls behind a modal.
    fn emit(&mut self, id: &str, action: Action, cx: &mut Context<Self>) {
        self.emit_version(id, self.document.revision, action, cx);
    }

    /// Deferred canvas measurements retain the revision at emission instead of borrowing a newer tree.
    fn emit_version(
        &mut self,
        id: &str,
        revision: u64,
        action: Action,
        cx: &mut Context<Self>,
    ) -> bool {
        let event = UiEvent {
            revision,
            node: id.into(),
            action,
        };
        if self.document.validate_event(&event).is_err()
            || (matches!(event.action, Action::Dismiss)
                && self.dismissed_dialog.as_deref() == Some(id))
        {
            return false;
        }
        if matches!(event.action, Action::Dismiss) {
            self.dismissed_dialog = Some(id.into());
            cx.notify();
        }
        (self.sink)(event, cx);
        true
    }
}

impl Render for PluginView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_document(window, cx)
    }
}

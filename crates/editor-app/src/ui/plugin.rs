//! Native plugin view lifecycle. GPUI entities stay here; guests receive typed events only.
pub(crate) mod chrome;
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
    dialog_focus: FocusHandle,
    previous_focus: Option<FocusHandle>,
    dismissed_dialog: Option<String>,
}

impl PluginView {
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
        for node in nodes {
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
        let accepted = match &action {
            Action::Dismiss => {
                self.document.dialog.as_ref().is_some_and(|d| d.id == id)
                    && self.dismissed_dialog.as_deref() != Some(id)
            }
            _ => self
                .document
                .active_node(id)
                .is_some_and(|node| match (&node.kind, &action) {
                    (Kind::Button { .. }, Action::Click)
                    | (Kind::Checkbox { .. }, Action::Toggle(_)) => true,
                    (Kind::Input(_), Action::Change(value) | Action::Submit(value)) => {
                        value.len() <= 65536
                    }
                    (Kind::Choice { options, .. }, Action::Select(id)) => {
                        options.iter().any(|o| &o.id == id && !o.disabled)
                    }
                    (Kind::Tabs { tabs, .. }, Action::Select(id)) => {
                        tabs.iter().any(|t| &t.id == id)
                    }
                    _ => false,
                }),
        };
        if !accepted {
            return;
        }
        if matches!(action, Action::Dismiss) {
            self.dismissed_dialog = Some(id.into());
            cx.notify();
        }
        (self.sink)(
            UiEvent {
                revision: self.document.revision,
                node: id.into(),
                action,
            },
            cx,
        );
    }
}

impl Render for PluginView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_document(window, cx)
    }
}

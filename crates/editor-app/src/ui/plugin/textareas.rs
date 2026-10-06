//! Keyed multiline state uses gpui-base's ordinary editing engine, separate from document editors.
use super::*;
use gpui_base::input::TextareaState;

pub(super) struct NativeTextarea {
    pub state: Entity<TextareaState>,
    value_revision: u64,
    last_value: String,
    _subscription: Subscription,
}

impl PluginView {
    /// Retain native state across acknowledgements and release controls removed from this document.
    pub(super) fn sync_textareas(
        &mut self,
        nodes: &[plugin_runtime::plugin_protocol::ui::Node],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.textareas.retain(|id, _| {
            nodes
                .iter()
                .any(|node| node.id == *id && matches!(node.kind, Kind::Textarea(_)))
        });
        for node in nodes {
            let Kind::Textarea(input) = &node.kind else {
                continue;
            };
            if !self.textareas.contains_key(&node.id) {
                let state = cx.new(|cx| {
                    TextareaState::new(window, cx)
                        .default_value(input.value.clone())
                        .placeholder(input.placeholder.clone())
                        .auto_grow(3, 8)
                });
                let id = node.id.clone();
                let subscription = cx.subscribe_in(
                    &state,
                    window,
                    move |this, state, event: &InputEvent, _, cx| {
                        if !matches!(event, InputEvent::Change) {
                            return;
                        }
                        let value = state.read(cx).value().to_string();
                        let Some(entry) = this.textareas.get_mut(&id) else {
                            return;
                        };
                        if entry.last_value == value {
                            return;
                        }
                        entry.last_value = value.clone();
                        this.emit(&id, Action::Change(value), cx);
                    },
                );
                self.textareas.insert(
                    node.id.clone(),
                    NativeTextarea {
                        state,
                        value_revision: input.value_revision,
                        last_value: input.value.clone(),
                        _subscription: subscription,
                    },
                );
            }
            let active = self.document.active_node(&node.id).is_some();
            let entry = self.textareas.get_mut(&node.id).unwrap();
            let replace = entry.value_revision != input.value_revision;
            if replace {
                entry.value_revision = input.value_revision;
                entry.last_value = input.value.clone();
            }
            entry.state.update(cx, |state, cx| {
                state.set_disabled(!active, cx);
                state.set_placeholder(input.placeholder.clone(), window, cx);
                if replace {
                    state.set_value(input.value.clone(), window, cx);
                }
            });
        }
    }
}

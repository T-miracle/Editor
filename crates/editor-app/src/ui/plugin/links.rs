//! Supplement Base link hit testing with the actual press identity; selection stays owned by Base.
use super::*;
use crate::ui::controls::Button;
use gpui_kit::{
    AnyElement, ClickEvent, InteractiveElement as _, IntoElement as _, MouseButton, MouseDownEvent,
    ParentElement as _, Pixels, Point, Styled as _, div, prelude::FluentBuilder as _, px,
};
use plugin_runtime::plugin_protocol::ui::Node;

pub(super) struct LinkPress {
    pub(super) node: String,
    revision: u64,
    position: Point<Pixels>,
}

pub(super) struct LinkFocus {
    handle: FocusHandle,
    _focused: Subscription,
    _blurred: Subscription,
}

/// URI plus occurrence identity prevents a new destination from inheriting an older link's focus.
fn focus_key(node: &str, index: usize, uri: &str) -> String {
    format!("{node}/{index}/{uri}")
}

impl PluginView {
    /// Reconcile bounded native focus resources; disappearing nodes/targets release their listeners.
    pub(super) fn sync_link_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut keys = BTreeSet::new();
        if self.document.link_events {
            let mut visit = |node: &Node| {
                for (index, link) in node.links.iter().enumerate() {
                    keys.insert(focus_key(&node.id, index, &link.uri));
                }
            };
            self.document.root.visit(&mut visit);
            if let Some(dialog) = &self.document.dialog {
                dialog.content.visit(&mut visit);
            }
        }
        self.link_focus.retain(|key, _| keys.contains(key));
        for key in keys {
            let node = key.split('/').next().unwrap().to_owned();
            self.link_focus.entry(key).or_insert_with(|| {
                let handle = cx.focus_handle();
                let focused = cx.on_focus(&handle, window, move |this, window, cx| {
                    if window.last_input_was_keyboard() {
                        this.reveal_focused_link(&node, cx);
                    } else {
                        cx.notify();
                    }
                });
                let blurred = cx.on_blur(&handle, window, |_, _, cx| cx.notify());
                LinkFocus {
                    handle,
                    _focused: focused,
                    _blurred: blurred,
                }
            });
        }
    }

    /// Linked images/alternative text retain their native content. Rich text keeps Base selection
    /// and pointer hit testing, with separate keyboard targets that become visible only on focus.
    pub(super) fn linked_content(
        &self,
        content: AnyElement,
        node: &Node,
        disabled: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !self.document.link_events || node.links.is_empty() {
            return content;
        }
        let disabled = disabled || self.document.active_node(&node.id).is_none();
        let revision = self.document.revision;
        if !matches!(node.kind, Kind::RichText { .. }) {
            let link = &node.links[0];
            let focus = &self.link_focus[&focus_key(&node.id, 0, &link.uri)].handle;
            let id = node.id.clone();
            let uri = link.uri.clone();
            return Button::new(format!("plugin-image-link-{}-scene-{revision}", node.id))
                .ghost()
                .p_0()
                .h_auto()
                .w_full()
                .disabled(disabled)
                .track_focus(focus)
                .accessibility_label(self.link_label(link))
                .tooltip(link.uri.clone())
                .child(content)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.emit_version(&id, revision, Action::Link { uri: uri.clone() }, cx);
                }))
                .into_any_element();
        }
        let mut body = div().relative().w_full().child(content);
        for (index, link) in node.links.iter().enumerate() {
            let focus = &self.link_focus[&focus_key(&node.id, index, &link.uri)].handle;
            let focused = focus.is_focused(window);
            let id = node.id.clone();
            let uri = link.uri.clone();
            let debug_id = format!("plugin-link-focus-{}-{index}", node.id);
            let button = Button::new(format!(
                "plugin-link-{}-{index}-scene-{revision}-focus-{focused}",
                node.id
            ))
            .ghost()
            .small()
            .compact()
            .outline()
            .max_w_full()
            .disabled(disabled)
            .track_focus(focus)
            .label(self.link_label(link))
            .tooltip(link.uri.clone())
            .on_click(cx.listener(move |this, event, _, cx| {
                // An invisible target can receive Tab, but a pointer press cannot open it.
                // Focus-specific Base IDs also prevent that press surviving its newly revealed cue.
                if focused || matches!(event, ClickEvent::Keyboard(_)) {
                    this.emit_version(&id, revision, Action::Link { uri: uri.clone() }, cx);
                }
            }));
            body = body.child(
                div()
                    .absolute()
                    .right_0()
                    .bottom_0()
                    .when(focused, |view| {
                        view.debug_selector(move || debug_id.clone()).max_w_full()
                    })
                    .when(!focused, |view| {
                        view.size(px(1.)).opacity(0.).overflow_hidden()
                    })
                    .child(button),
            );
        }
        body.into_any_element()
    }

    /// Empty captions still expose an intelligible native accessible name in the selected UI locale.
    fn link_label(&self, link: &plugin_runtime::plugin_protocol::ui::LinkTarget) -> String {
        if link.label.is_empty() {
            rust_i18n::t!(
                "preview.open_link",
                locale = self.environment.locale.as_str()
            )
            .to_string()
        } else {
            link.label.clone()
        }
    }

    /// Capture only the live rich-text block before Base selection consumes pointer input.
    pub(super) fn press_link(&mut self, node: &str, revision: u64, event: &MouseDownEvent) {
        self.link_press = (event.button == MouseButton::Left
            && revision == self.document.revision
            && self.document.link_events
            && self
                .document
                .active_node(node)
                .is_some_and(|node| matches!(node.kind, Kind::RichText { .. })))
        .then(|| LinkPress {
            node: node.into(),
            revision,
            position: event.position,
        });
    }

    /// Base 0.7 synthesizes the down event at MouseUp. Never treat that identity as a real press.
    /// A replacement, release alone, another block or a drag cannot become an intentional click.
    pub(super) fn accept_link_click(
        &mut self,
        node: &str,
        revision: u64,
        event: &ClickEvent,
    ) -> bool {
        let Some(press) = self.link_press.take() else {
            return false;
        };
        let ClickEvent::Mouse(click) = event else {
            return false;
        };
        press.node == node && press.revision == revision && revision == self.document.revision
            && click.up.button == MouseButton::Left
            // Base uses a 2px drag threshold: allow jitter without moving into another hit target.
            && (click.up.position - press.position).magnitude() <= 2.
    }
}

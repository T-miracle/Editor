//! Source-mapped native blocks reveal through their nearest Base scroll handle, using live layout.
use super::*;
use gpui_kit::{Bounds, Pixels, point, px};
use plugin_runtime::plugin_protocol::{api, ui::Node};

/// This index is a derived scene projection, never an editor document or a second scroll state.
#[derive(Default)]
pub(super) struct SceneLayout {
    scroll_owner: BTreeMap<String, String>,
    pending: Option<Reveal>,
    bounds: BTreeMap<String, Bounds<Pixels>>,
}

/// Explicit navigation aligns the block start; keyboard focus only scrolls an invisible cue into view.
struct Reveal {
    node: String,
    focus: bool,
}

impl SceneLayout {
    /// Replacements invalidate queued geometry, including changes to source, tabs or modal ownership.
    pub(super) fn reset(&mut self, document: &Document) {
        self.scroll_owner.clear();
        self.pending = None;
        self.bounds.clear();
        if let Some(dialog) = &document.dialog {
            self.index(&dialog.content, None);
        } else if document.menu.is_none() {
            self.index(&document.root, None);
        }
    }

    fn index(&mut self, node: &Node, scroll: Option<&str>) {
        if node.disabled {
            return;
        }
        if (node.source_range.is_some() || !node.links.is_empty())
            && let Some(scroll) = scroll
        {
            self.scroll_owner.insert(node.id.clone(), scroll.into());
        }
        match &node.kind {
            Kind::Column { children } | Kind::Row { children } => {
                for child in children {
                    self.index(child, scroll);
                }
            }
            Kind::Scroll { content } => self.index(content, Some(&node.id)),
            Kind::Tabs { tabs, selected } => {
                if let Some(tab) = tabs.iter().find(|tab| &tab.id == selected) {
                    self.index(&tab.content, scroll);
                }
            }
            _ => {}
        }
    }
}

impl PluginView {
    /// Queue one owned, current scene block. Actual geometry is read during its next native layout.
    pub(crate) fn reveal_node(
        &mut self,
        node: &str,
        revision: u64,
        cx: &mut Context<Self>,
    ) -> Result<(), api::Failure> {
        if revision != self.document.revision {
            return Err(api::Failure::new(
                api::ErrorCode::StaleRevision,
                "Preview scene changed",
            ));
        }
        if !self.scene_layout.scroll_owner.contains_key(node) {
            return Err(api::Failure::new(
                api::ErrorCode::InvalidState,
                "Block has no active scroll owner",
            ));
        }
        self.scene_layout.pending = Some(Reveal {
            node: node.into(),
            focus: false,
        });
        cx.notify();
        Ok(())
    }

    /// Native keyboard focus uses the live active tree, including ordinary linked images without source maps.
    /// A pointer focus does not call this path: moving an image before MouseUp would invalidate its click.
    pub(super) fn reveal_focused_link(&mut self, node: &str, cx: &mut Context<Self>) {
        if self.scene_layout.scroll_owner.contains_key(node) {
            self.scene_layout.pending = Some(Reveal {
                node: node.into(),
                focus: true,
            });
        }
        cx.notify();
    }

    /// Measurement never emits a guest event or redraw loop. Only an explicit pending reveal scrolls.
    /// The current offset is removed from the measured position, so wrapped text and image reflow
    /// locate the actual block instead of approximating a percentage of the whole document.
    pub(super) fn measure_block(
        &mut self,
        node: &str,
        revision: u64,
        bounds: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if revision != self.document.revision {
            return;
        }
        if self
            .scene_layout
            .bounds
            .insert(node.into(), bounds)
            .is_some_and(|old| old != bounds)
            && self
                .link_press
                .as_ref()
                .is_some_and(|press| press.node == node)
        {
            // Image, scroll and width reflow can move a link without replacing the source scene.
            self.link_press = None;
        }
        let Some(reveal) = self
            .scene_layout
            .pending
            .as_ref()
            .filter(|reveal| reveal.node == node)
        else {
            return;
        };
        let Some(scroll) = self
            .scene_layout
            .scroll_owner
            .get(node)
            .and_then(|id| self.scrolls.get(id))
        else {
            return;
        };
        let offset = scroll.offset();
        let viewport = scroll.bounds();
        let desired = if !reveal.focus {
            // A heading navigation intentionally places the target at the start of its own viewport.
            offset.y - (bounds.top() - viewport.top())
        } else if bounds.bottom() > viewport.bottom() {
            // The rich-text cue is at the block bottom, even when a single paragraph is taller than the pane.
            offset.y - (bounds.bottom() - viewport.bottom())
        } else if bounds.top() < viewport.top() {
            offset.y + (viewport.top() - bounds.top())
        } else {
            offset.y
        };
        let y = desired.max(-scroll.max_offset().y).min(px(0.));
        self.scene_layout.pending = None;
        if y != offset.y {
            scroll.set_offset(point(offset.x, y));
            cx.notify();
        }
    }
}

//! Offscreen source blocks reuse measured native heights instead of rebuilding rich-text layouts.
//! Base retains the real scroll offset; width/content/theme changes invalidate only affected measurements.
use super::*;
use gpui_kit::{Bounds, Pixels, px};
use plugin_runtime::plugin_protocol as protocol;
use protocol::ui::Node;

#[derive(Default)]
pub(super) struct VirtualBlocks {
    heights: BTreeMap<String, (Node, Pixels, Pixels)>,
    active: BTreeMap<String, (Node, Pixels)>,
    anchors: BTreeMap<String, (String, Pixels, gpui_kit::Point<Pixels>)>,
}

impl VirtualBlocks {
    /// Only mapped, vertical document blocks participate; interactive arbitrary plugin layouts do not.
    pub fn begin(&mut self) {
        self.active.clear();
    }

    /// Preserve known heights for unchanged content, including a shifted source range.
    pub fn height(&self, node: &Node, width: Pixels) -> Option<Pixels> {
        let (old, measured_width, height) = self.heights.get(&node.id)?;
        let mut old = old.clone();
        let shift = match (old.source_range, node.source_range) {
            (Some(old), Some(new)) => new.start as i64 - old.start as i64,
            _ => 0,
        };
        protocol::ui::incremental::shift_source(&mut old, shift).ok()?;
        (old == *node && (*measured_width - width).abs() < px(0.5)).then_some(*height)
    }

    /// Capture original block data before any measurement callbacks run.
    pub fn track(&mut self, node: &Node, width: Pixels, skipped: bool) {
        if !skipped {
            self.active.insert(node.id.clone(), (node.clone(), width));
        }
    }

    pub fn measure(&mut self, id: &str, bounds: Bounds<Pixels>) {
        if let Some((node, width)) = self.active.get(id) {
            self.heights
                .insert(id.into(), (node.clone(), *width, bounds.size.height));
        }
    }

    /// Theme/font changes may alter wrapping even at the same container width.
    pub fn clear(&mut self) {
        self.heights.clear();
        self.anchors.clear();
    }

    pub fn retain(&mut self, document: &Document) {
        let mut ids = BTreeSet::new();
        document.root.visit(&mut |node| {
            ids.insert(node.id.clone());
        });
        self.heights.retain(|id, _| ids.contains(id));
    }
}

impl PluginView {
    /// Reuse a block's exact native extent outside the viewport plus one screen of overscan.
    /// Unmeasured offscreen blocks use estimates; scrolling or explicit navigation measures them lazily.
    pub(super) fn windowed_content(&mut self, content: &Node, scroll_id: &str) -> Node {
        let mut result = content.clone();
        let Kind::Column { children } = &mut result.kind else {
            return result;
        };
        if children.len() < 32 || children.iter().any(|node| node.source_range.is_none()) {
            return result;
        }
        let Some(handle) = self.scrolls.get(scroll_id).cloned() else {
            return result;
        };
        let bounds = handle.bounds();
        // Use a fallback only before the first layout; narrow panes must invalidate at their real width.
        let width = if bounds.size.width > px(0.) {
            bounds.size.width
        } else {
            px(320.)
        };
        let mut top = -handle.offset().y;
        let margin = bounds.size.height.max(px(600.));
        let mut y = px(content.layout.padding);
        let heights = children
            .iter()
            .map(|node| {
                self.virtual_blocks
                    .height(node, width)
                    .unwrap_or_else(|| estimate(node, width))
            })
            .collect::<Vec<_>>();
        // Refining spacers must keep the same visible block. A changed native offset means a real
        // gesture/program locate already took control, so its position always wins this correction.
        if self.viewport.pending_target().is_none()
            && self.scene_layout.pending_target().is_none()
            && let Some((anchor, within, offset)) = self.virtual_blocks.anchors.get(scroll_id)
            && *offset == handle.offset()
            && let Some(index) = children.iter().position(|node| &node.id == anchor)
        {
            let next_top = y
                + heights[..index].iter().copied().sum::<Pixels>()
                + px(content.layout.gap * index as f32)
                + *within;
            if (next_top - top).abs() > px(0.5) {
                top = next_top.max(px(0.));
                handle.set_offset(gpui_kit::point(offset.x, -top));
                self.viewport.scroll_changed();
            }
        }
        let mut anchor = None;
        for (node, height) in children.iter_mut().zip(heights) {
            if anchor.is_none() && y + height > top {
                anchor = Some((node.id.clone(), top - y, handle.offset()));
            }
            let contains = |id: &str| {
                let mut found = false;
                node.visit(&mut |child| found |= child.id == id);
                found
            };
            let needed = self.viewport.pending_target().is_some_and(contains)
                || self.scene_layout.pending_target().is_some_and(contains);
            // Keep native keyboard traversal for interactive descendants. Their handles cannot be
            // removed from Base's focus order merely because their containing block is offscreen.
            let mut interactive = false;
            node.visit(&mut |child| {
                interactive |= !child.links.is_empty()
                    || matches!(
                        child.kind,
                        Kind::Button { .. }
                            | Kind::NativeEditor { .. }
                            | Kind::Input(_)
                            | Kind::Checkbox { .. }
                            | Kind::Choice { .. }
                            | Kind::Canvas(_)
                            | Kind::SideTabs(_)
                            | Kind::Tabs { .. }
                    );
            });
            let skip =
                !needed && !interactive && (y + height < top - margin || y > top + margin * 2.);
            self.virtual_blocks.track(node, width, skip);
            if skip {
                // The public mapped identity still supplies geometry, but no rich text/control is laid out.
                let mut placeholder = Node::text(node.id.clone(), "").height(f32::from(height));
                placeholder.source_range = node.source_range;
                placeholder.role = node.role.clone();
                *node = placeholder;
            }
            y += height + px(content.layout.gap);
        }
        if let Some(anchor) = anchor {
            self.virtual_blocks.anchors.insert(scroll_id.into(), anchor);
        }
        result
    }
}

/// Estimates are only spacers, never source positions or saved dimensions. Actual layout replaces them.
fn estimate(node: &Node, width: Pixels) -> Pixels {
    if let Some(height) = node.layout.height {
        return px(height);
    }
    let inner = (f32::from(width) - node.layout.padding * 2.).max(80.);
    let height = match &node.kind {
        Kind::Column { children } => {
            children
                .iter()
                .map(|child| f32::from(estimate(child, px(inner))))
                .sum::<f32>()
                + children.len().saturating_sub(1) as f32 * node.layout.gap
        }
        Kind::Row { children } => children
            .iter()
            .map(|child| f32::from(estimate(child, px(inner / children.len().max(1) as f32))))
            .fold(0., f32::max),
        Kind::Text { text } => text
            .lines()
            .map(|line| (line.chars().count() as f32 * 8. / inner).ceil().max(1.) * 22.)
            .sum::<f32>(),
        Kind::RichText { html } => html
            .lines()
            .map(|line| (line.chars().count() as f32 * 8. / inner).ceil().max(1.) * 22.)
            .sum::<f32>(),
        Kind::CodeBlock { text, .. } => text.lines().count().max(1) as f32 * 22. + 16.,
        Kind::Image { .. } => 180.,
        _ => 32.,
    };
    px(height.max(22.) + node.layout.padding * 2.)
}

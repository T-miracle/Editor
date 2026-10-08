//! Regional split policy reads Base's live layout; it stores neither a second tree nor a remembered dock direction.
use super::*;
use gpui_base::dock::{PaneNode, PaneRef};
use gpui_kit::{Axis, Point};

/// The project uses Base's documented middle merge zone while keeping every panel in an independent leaf.
/// Live pointer geometry also covers a release before another frame has refreshed Base's indicator.
pub(super) fn edge_at(bounds: Bounds<Pixels>, pointer: Point<Pixels>) -> Option<Placement> {
    if !bounds.contains(&pointer) {
        return None;
    }
    if pointer.x < bounds.left() + bounds.size.width * 0.35 {
        Some(Placement::Left)
    } else if pointer.x > bounds.left() + bounds.size.width * 0.65 {
        Some(Placement::Right)
    } else if pointer.y < bounds.top() + bounds.size.height * 0.35 {
        Some(Placement::Top)
    } else if pointer.y > bounds.top() + bounds.size.height * 0.65 {
        Some(Placement::Bottom)
    } else {
        None
    }
}

/// Side bands stack up/down; top/bottom bands stack left/right. The editor itself accepts all four edges.
pub(super) fn resolve(
    area: &DockArea,
    node: NodeId,
    requested: Placement,
    bounds: Bounds<Pixels>,
    pointer: Point<Pixels>,
    cx: &App,
) -> Placement {
    match stack_axis(area, node, cx) {
        Some(Axis::Vertical) => match requested {
            Placement::Top | Placement::Bottom => requested,
            _ if pointer.y <= bounds.center().y => Placement::Top,
            _ => Placement::Bottom,
        },
        Some(Axis::Horizontal) => match requested {
            Placement::Left | Placement::Right => requested,
            _ if pointer.x <= bounds.center().x => Placement::Left,
            _ => Placement::Right,
        },
        None => requested,
    }
}

/// Derive a center-tree band's orientation from its separation from the host editor anchor.
/// Base has no Top DockPlacement: a vertical split above Editor is the native, persisted top band.
pub(crate) fn stack_axis(area: &DockArea, node: NodeId, cx: &App) -> Option<Axis> {
    for region in [
        DockPlacement::Left,
        DockPlacement::Right,
        DockPlacement::Bottom,
    ] {
        if area
            .layout(region)
            .is_some_and(|tree| tree.find_node(node).is_some())
        {
            return Some(if region == DockPlacement::Bottom {
                Axis::Horizontal
            } else {
                Axis::Vertical
            });
        }
    }
    let tree = area.layout(DockPlacement::Center)?;
    let anchor = tree.panels().find(|id| {
        area.panel(*id)
            .is_some_and(|panel| panel.panel_name(cx) == "Editor")
    })?;
    band_axis(tree.root(), node, tree.find_panel_node(anchor)?)
}

/// The nearest split separating target and Editor determines the band's natural stacking axis.
fn band_axis(root: &PaneNode, target: NodeId, anchor: NodeId) -> Option<Axis> {
    let PaneRef::Split { axis, children, .. } = root.kind() else {
        return None;
    };
    let target_child = children.iter().position(|child| contains(child, target))?;
    let anchor_child = children.iter().position(|child| contains(child, anchor))?;
    if target_child != anchor_child {
        Some(match axis {
            Axis::Horizontal => Axis::Vertical,
            Axis::Vertical => Axis::Horizontal,
        })
    } else {
        band_axis(&children[target_child], target, anchor)
    }
}

/// Inspect public immutable nodes; mutations always stay behind Base's normalized edit methods.
fn contains(root: &PaneNode, target: NodeId) -> bool {
    let mut found = false;
    root.walk(&mut |node| found |= node.id() == target);
    found
}

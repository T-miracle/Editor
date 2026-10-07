//! Bind persisted DockArea leaves to existing host and plugin views, then let Base restore them.

use crate::*;
use gpui_base::dock::{
    DockAreaState, DockState, Panel as _, PanelInfo, PanelState, register_panel,
};
use std::collections::HashSet;

/// Registry factories borrow the current editor's views; loading must not recreate their state.
fn bind_panel<P: DockPanel>(name: &str, panel: &Entity<P>, cx: &mut App) {
    let panel = panel.downgrade();
    register_panel(cx, name, move |_, _, _| {
        dock::panel_handle(
            panel
                .upgrade()
                .expect("bound panels live throughout layout restoration"),
        )
    });
}

impl EditorApp {
    /// Snapshot live measurements at exit as well as on completed native layout edits.
    pub(crate) fn capture_dock_layout(&mut self, cx: &App) {
        // Startup must not overwrite a saved plugin layout with the temporary default center.
        if self.pending_dock_restore {
            return;
        }
        let layout = self.dock_area.read(cx).dump(cx);
        for (name, dock) in [
            ("left", &layout.left_dock),
            ("right", &layout.right_dock),
            ("bottom", &layout.bottom_dock),
        ] {
            if let Some(dock) = dock {
                self.session_state
                    .plugin_dock_sizes
                    .insert(name.into(), dock.size() / px(1.));
            }
        }
        self.session_state.plugin_panel_visibility.extend(
            self.plugin_panels
                .iter()
                .map(|(key, panel)| (key.clone(), panel.read(cx).visible(cx))),
        );
        self.session_state.dock_layout = Some(layout);
    }

    /// Restore once installed contributions are known, using stable manifest identities.
    pub(crate) fn restore_dock_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.pending_dock_restore || !self.extensions.read(cx).startup.is_empty() {
            return;
        }
        let Some(mut layout) = self.session_state.dock_layout.clone() else {
            self.pending_dock_restore = false;
            return;
        };
        // Construction happens before plugin viewers exist, even when cached startup is fast.
        if !self.extensions.read(cx).entries.is_empty() && self.plugin_panels.is_empty() {
            return;
        }
        let has_messages = contains_panel(&layout.center, "HostMessages")
            || [&layout.left_dock, &layout.right_dock, &layout.bottom_dock]
                .iter()
                .any(|dock| {
                    dock.as_ref()
                        .is_some_and(|dock| contains_panel(dock.panel(), "HostMessages"))
                });
        let mut names = HashSet::from([
            "Explorer".to_owned(),
            "Editor".to_owned(),
            "HostMessages".to_owned(),
        ]);
        // Explorer can have moved outside the center, so retain its entity independently.
        bind_panel("Explorer", &self.explorer_panel, cx);
        bind_panel("Editor", &self.editor_panel, cx);
        bind_panel("HostMessages", &self.messages, cx);
        for (key, panel) in &self.plugin_panels {
            // File-scoped previews belong to the editor's inner split, never to the outer layout.
            if panel.read(cx).is_editor_preview() {
                continue;
            }
            let name = format!("plugin:{key}");
            bind_panel(&name, panel, cx);
            names.insert(name);
        }
        // Disabled or uninstalled contributions must not create blank placeholder panels.
        prune_unavailable(&mut layout, &names);
        self.pending_dock_restore = false;
        if let Err(error) = self
            .dock_area
            .update(cx, |area, cx| area.load(layout, window, cx))
        {
            tracing::warn!(%error, "saved dock layout could not be restored");
        } else if !has_messages {
            // Extend an older saved tree after loading it; replacing the right dock would lose its peers.
            let panel = dock::panel_handle(self.messages.clone());
            let width = px(self
                .session_state
                .plugin_dock_sizes
                .get("right")
                .copied()
                .unwrap_or(320.));
            self.dock_area.update(cx, |area, cx| {
                local_dock::add_panel_view(
                    area,
                    panel,
                    gpui_base::dock::DockPlacement::Right,
                    Some(width),
                    window,
                    cx,
                )
            });
        }
    }
}

/// A host panel may have moved into any saved split; only truly absent leaves need migration.
fn contains_panel(state: &PanelState, name: &str) -> bool {
    state.panel_name == name
        || state
            .children
            .iter()
            .any(|child| contains_panel(child, name))
}

/// Filter absent leaves while keeping each surviving split size aligned with its child.
fn retain_panels(state: &mut PanelState, names: &HashSet<String>) -> bool {
    match &mut state.info {
        PanelInfo::Panel(_) => names.contains(&state.panel_name),
        PanelInfo::Stack { sizes, .. } => {
            let old_sizes = std::mem::take(sizes);
            let old_children = std::mem::take(&mut state.children);
            for (index, mut child) in old_children.into_iter().enumerate() {
                if retain_panels(&mut child, names) {
                    state.children.push(child);
                    sizes.push(old_sizes.get(index).copied().unwrap_or(px(0.)));
                }
            }
            !state.children.is_empty()
        }
        PanelInfo::Tabs { active_index } => {
            state
                .children
                .retain_mut(|child| retain_panels(child, names));
            *active_index = (*active_index).min(state.children.len().saturating_sub(1));
            !state.children.is_empty()
        }
    }
}

/// Keep Base's persisted dock metadata intact when removing an unavailable contribution.
fn prune_unavailable(layout: &mut DockAreaState, names: &HashSet<String>) {
    retain_panels(&mut layout.center, names);
    for slot in [
        &mut layout.left_dock,
        &mut layout.right_dock,
        &mut layout.bottom_dock,
    ] {
        if let Some(dock) = slot.take() {
            let mut panel = dock.panel().clone();
            if retain_panels(&mut panel, names) {
                *slot = Some(DockState::new(
                    panel,
                    dock.placement(),
                    dock.size(),
                    dock.open(),
                ));
            }
        }
    }
}

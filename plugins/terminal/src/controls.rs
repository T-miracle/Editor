//! Terminal data and actions for the host's native sidebar and explorer-style popup.
use super::*;
use plugin_protocol::ui::{
    Action, CanvasControls, MenuItem, PopupMenu, SideTab, SideTabs, UiEvent,
};
impl Terminal {
    /// Describe the sidebar and menu that the host renders around the terminal canvas.
    pub(super) fn canvas_controls(&self) -> CanvasControls {
        let width = self.effective_tab_width();
        CanvasControls {
            revision: self.ui_revision,
            sidebar: Some(SideTabs {
                id: "sessions".into(),
                width,
                // Only divider dragging changes these limits; window resizing never does.
                min_width: MIN_TAB_WIDTH,
                max_width: MAX_TAB_WIDTH,
                items: self
                    .tabs
                    .iter()
                    .map(|t| SideTab {
                        id: t.id.to_string(),
                        label: t.name.clone(),
                        status: t.exited.then(|| "已退出".into()),
                        disabled: false,
                        closable: true,
                    })
                    .collect(),
                selected: self.tabs.get(self.active).map(|t| t.id.to_string()),
                rename: self.rename.map(|id| id.to_string()),
            }),
            menu: self.menu.map(|kind| PopupMenu {
                id: kind.id().into(),
                x: self.menu_position.0,
                y: self.menu_position.1,
                items: self.menu_items(kind),
            }),
        }
    }
    /// Recompute availability from the current selection, including after new process output.
    fn menu_items(&self, kind: TerminalMenu) -> Vec<MenuItem> {
        let actions = match kind {
            TerminalMenu::Commands => self.menu_actions(),
            TerminalMenu::Output => self.output_menu_actions(),
        };
        let can_copy = self.selected_text().is_some();
        actions
            .into_iter()
            .enumerate()
            .map(|(index, (id, label))| MenuItem {
                disabled: id == "copy" && !can_copy,
                id,
                label,
                separator_before: match kind {
                    TerminalMenu::Commands => index == self.settings.profiles.len(),
                    TerminalMenu::Output => index == 2,
                },
            })
            .collect()
    }
    /// Resolve identities against current sessions so stale events cannot address a different tab.
    pub(super) fn ui_event(&mut self, event: UiEvent) {
        if matches!(
            event.node.as_str(),
            "terminal-menu" | "terminal-output-menu"
        ) {
            let Some(kind) = self.menu.filter(|kind| event.node == kind.id()) else {
                return;
            };
            match event.action {
                Action::Select(id)
                    if self
                        .menu_items(kind)
                        .iter()
                        .any(|item| item.id == id && !item.disabled) =>
                {
                    self.menu = None;
                    self.command(&id, None, None);
                }
                Action::Dismiss => self.menu = None,
                _ => {}
            }
            return;
        }
        if event.node != "sessions" {
            return;
        }
        let index = |id: &str| self.tabs.iter().position(|t| t.id.to_string() == id);
        match event.action {
            Action::Select(id) => {
                if let Some(index) = index(&id) {
                    self.active = index;
                }
            }
            Action::Context { id, x, y } if x.is_finite() && y.is_finite() => {
                if let Some(index) = index(&id) {
                    self.active = index;
                    self.menu = Some(TerminalMenu::Commands);
                    self.menu_position =
                        ((self.tab_left() + x).clamp(0., 10000.), y.clamp(0., 10000.));
                }
            }
            Action::Close(id) => {
                if let Some(index) = index(&id) {
                    self.close(index);
                }
            }
            Action::Rename { id, value } => {
                if let Some(index) = index(&id) {
                    if !value.trim().is_empty() {
                        self.tabs[index].name = value.trim().chars().take(80).collect();
                    }
                }
                self.rename = None;
            }
            Action::Move { from, to } => {
                if let (Some(from), Some(to)) = (index(&from), index(&to)) {
                    let tab = self.tabs.remove(from);
                    self.tabs.insert(to, tab);
                    self.active = to;
                }
            }
            Action::Resize(width) if width.is_finite() => {
                self.tab_width = width.clamp(MIN_TAB_WIDTH, MAX_TAB_WIDTH);
                self.resize_grid();
            }
            _ => {}
        }
    }
}

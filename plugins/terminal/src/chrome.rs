//! Terminal data and actions for the host's native sidebar and explorer-style popup.
use super::*;
use plugin_protocol::ui::{Action, CanvasChrome, MenuItem, PopupMenu, SideTab, SideTabs, UiEvent};
impl Terminal {
    pub(super) fn native_chrome(&self) -> CanvasChrome {
        let width = self.effective_tab_width();
        CanvasChrome {
            revision: self.ui_revision,
            sidebar: Some(SideTabs {
                id: "sessions".into(),
                width,
                min_width: MIN_TAB_WIDTH.min(width),
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
            menu: self.menu.then(|| PopupMenu {
                id: "terminal-menu".into(),
                x: self.menu_position.0,
                y: self.menu_position.1,
                items: self
                    .menu_actions()
                    .into_iter()
                    .enumerate()
                    .map(|(index, (id, label))| MenuItem {
                        id,
                        label,
                        disabled: false,
                        separator_before: index == self.settings.profiles.len(),
                    })
                    .collect(),
            }),
        }
    }
    /// Resolve identities against current sessions so stale events cannot address a different tab.
    pub(super) fn ui_event(&mut self, event: UiEvent) {
        if event.node == "terminal-menu" {
            if !self.menu {
                return;
            }
            match event.action {
                Action::Select(id) if self.menu_actions().iter().any(|(key, _)| key == &id) => {
                    self.menu = false;
                    self.command(&id, None, None);
                }
                Action::Dismiss => self.menu = false,
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
                    self.menu = true;
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

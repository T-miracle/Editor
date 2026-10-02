//! Persists window layout and open document state for one workspace.

use serde::{Deserialize, Serialize};
use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionState {
    pub workspace: String,
    /// Host-local authority; repository files and project plugin overrides cannot change it.
    #[serde(default = "default_true")]
    pub workspace_trusted: bool,
    pub window_width: f32,
    pub window_height: f32,
    pub explorer_width: f32,
    #[serde(default = "default_true")]
    pub explorer_visible: bool,
    /// Newly introduced project roots start expanded for previously saved workspaces too.
    #[serde(default = "default_true")]
    pub explorer_root_expanded: bool,
    /// Tab switches leave the explorer untouched unless this workspace preference is enabled.
    #[serde(default)]
    pub explorer_reveal_on_tab_switch: bool,
    /// Generic plugin-manager dock dimensions persist independently of plugin-owned state.
    #[serde(default = "default_extension_height", alias = "terminal_height")]
    pub extension_height: f32,
    #[serde(default = "default_true", alias = "terminal_visible")]
    pub extensions_visible: bool,
    /// Native dock dimensions and panel visibility are generic host presentation state.
    #[serde(default)]
    pub plugin_dock_sizes: std::collections::BTreeMap<String, f32>,
    #[serde(default)]
    pub plugin_panel_visibility: std::collections::BTreeMap<String, bool>,
    /// Base serializes the complete split tree, dock extents and open state.
    #[serde(default)]
    pub dock_layout: Option<gpui_base::dock::DockAreaState>,
    pub open_tabs: Vec<String>,
    pub active_file: Option<String>,
    pub expanded_directories: Vec<String>,
    /// Local workspace plugin exclusions are applied before startup loading begins.
    #[serde(default)]
    pub disabled_plugins: Vec<String>,
}

impl SessionState {
    pub fn for_workspace(workspace: &Path) -> Self {
        Self {
            workspace: workspace.to_string_lossy().into_owned(),
            // Preserve the editor's existing trust default; users can restrict a workspace locally.
            workspace_trusted: true,
            window_width: 1280.,
            window_height: 820.,
            explorer_width: 280.,
            explorer_visible: true,
            explorer_root_expanded: true,
            explorer_reveal_on_tab_switch: false,
            extension_height: default_extension_height(),
            extensions_visible: true,
            plugin_dock_sizes: Default::default(),
            plugin_panel_visibility: Default::default(),
            dock_layout: None,
            open_tabs: Vec::new(),
            active_file: None,
            expanded_directories: Vec::new(),
            disabled_plugins: Vec::new(),
        }
    }

    fn file_path(&self) -> Option<PathBuf> {
        let mut hasher = DefaultHasher::new();
        self.workspace.hash(&mut hasher);
        Some(
            dirs::config_dir()?
                .join("MeEditor")
                .join(format!("{:016x}.json", hasher.finish())),
        )
    }

    pub fn load(workspace: &Path) -> Self {
        let mut state = Self::for_workspace(workspace);
        if let Some(path) = state.file_path() {
            if let Ok(contents) = fs::read_to_string(path) {
                if let Ok(saved) = serde_json::from_str::<Self>(&contents) {
                    if saved.workspace == state.workspace {
                        state = saved;
                        state.window_width = state.window_width.clamp(800., 7680.);
                        state.window_height = state.window_height.clamp(500., 4320.);
                        state.explorer_width = state.explorer_width.clamp(220., 520.);
                        state.extension_height = state.extension_height.clamp(140., 1200.);
                    }
                }
            }
        }
        state.migrate_plugin_ids();
        state
    }

    /// Restore old exclusions, visibility choices and dock bindings using the current plugin IDs.
    fn migrate_plugin_ids(&mut self) {
        for id in &mut self.disabled_plugins {
            *id = plugin_schema::canonical_plugin_id(id).to_owned();
        }
        let visibility = std::mem::take(&mut self.plugin_panel_visibility);
        for (key, visible) in &visibility {
            if canonical_panel_key(key) == *key {
                self.plugin_panel_visibility.insert(key.clone(), *visible);
            }
        }
        for (key, visible) in visibility {
            self.plugin_panel_visibility
                .entry(canonical_panel_key(&key))
                .or_insert(visible);
        }
        if let Some(layout) = &mut self.dock_layout {
            migrate_panel_names(&mut layout.center);
            for slot in [
                &mut layout.left_dock,
                &mut layout.right_dock,
                &mut layout.bottom_dock,
            ] {
                if let Some(dock) = slot.take() {
                    let mut panel = dock.panel().clone();
                    migrate_panel_names(&mut panel);
                    *slot = Some(gpui_base::dock::DockState::new(
                        panel,
                        dock.placement(),
                        dock.size(),
                        dock.open(),
                    ));
                }
            }
        }
    }

    pub fn save(&self) {
        let Some(path) = self.file_path() else { return };
        let Some(parent) = path.parent() else { return };
        if fs::create_dir_all(parent).is_ok() {
            if let Ok(json) = serde_json::to_vec_pretty(self) {
                let temporary = path.with_extension("json.tmp");
                if fs::write(&temporary, json).is_ok() {
                    let _ = fs::rename(temporary, path);
                }
            }
        }
    }
}

/// Only the owner portion of a persisted panel key changes; the declared panel ID stays intact.
fn canonical_panel_key(key: &str) -> String {
    if let Some((plugin, panel)) = key.split_once('/') {
        format!("{}/{panel}", plugin_schema::canonical_plugin_id(plugin))
    } else {
        plugin_schema::canonical_plugin_id(key).to_owned()
    }
}

/// Rewrite stable dock names without altering split sizes, tab order or the open/closed state.
fn migrate_panel_names(panel: &mut gpui_base::dock::PanelState) {
    if let Some(key) = panel.panel_name.strip_prefix("plugin:") {
        panel.panel_name = format!("plugin:{}", canonical_panel_key(key));
    }
    for child in &mut panel.children {
        migrate_panel_names(child);
    }
}

fn default_true() -> bool {
    true
}

/// Initial plugin manager height leaves room for editing at the default window size.
fn default_extension_height() -> f32 {
    280.
}

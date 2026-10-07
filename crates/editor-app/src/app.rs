//! Window shell, layout, settings, dialogs, and persisted app state.

/// Product name shared by native window titles, the application shell, and LSP identification.
pub(crate) const APP_NAME: &str = "Nanobug";

pub(crate) mod dialog;
pub(crate) mod language_servers;
pub(crate) mod languages;
mod layout;
pub(crate) mod plugins;
pub(crate) mod session;
mod settings;
mod shell;
pub(crate) mod shortcuts;

pub(crate) use settings::SettingsSection;

#[cfg(target_os = "windows")]
pub(crate) use shell::WindowsTimerResolution;
pub(crate) use shell::{EditorDockPanel, EditorDockPanelKind};
